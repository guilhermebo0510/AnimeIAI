//! Normais personalizadas da Modelagem (o truque do rosto de anime: sombra cel limpa sem mexer na forma), sobre o motor
//! `aiai_3d_edit::normals_tools` (do ANIGO). A interface é nossa e compacta (a do ANIGO, `normals_ui.rs`, tem a vista
//! das normais e a pintura de normais, que ficaram de fora).
//!
//! Escopo: as faces escolhidas; sem faces, os vértices escolhidos; sem nada, a malha inteira.

use super::ModelingState;
use crate::app::modeling::uv::l;
use aiai_3d_edit::normals_tools::{self as nt, AverageMode, NormalBlend, NormalEditOptions, NormalMapping, NormalScope};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NormalOp {
    /// Como se o objeto fosse uma esfera em volta do centro da seleção (rosto de anime).
    FromCenter,
    /// Todas apontando para a câmera (sombra chapada vista de frente).
    FromView,
    /// Copiadas de outro objeto (uma esfera ou cápsula por cima do rosto, como no Blender "Data Transfer").
    FromObject,
    /// Média entre as faces (tira quinas de sombra).
    Average,
    /// Suaviza com os vizinhos.
    Smooth,
    /// Volta às calculadas.
    Reset,
}

impl NormalOp {
    pub(crate) const ALL: [NormalOp; 6] = [NormalOp::FromCenter, NormalOp::FromView, NormalOp::FromObject, NormalOp::Average, NormalOp::Smooth, NormalOp::Reset];

    pub(crate) fn key(self) -> &'static str {
        match self {
            NormalOp::FromCenter => "from_center",
            NormalOp::FromView => "from_view",
            NormalOp::FromObject => "from_object",
            NormalOp::Average => "average",
            NormalOp::Smooth => "smooth",
            NormalOp::Reset => "reset",
        }
    }

    pub(crate) fn from_key(k: &str) -> Option<NormalOp> {
        Self::ALL.into_iter().find(|o| o.key() == k)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            NormalOp::FromCenter => l("Da esfera (centro da seleção)", "From sphere (selection center)"),
            NormalOp::FromView => l("Da vista", "From view"),
            NormalOp::FromObject => l("De outro objeto", "From another object"),
            NormalOp::Average => l("Média", "Average"),
            NormalOp::Smooth => l("Suavizar", "Smooth"),
            NormalOp::Reset => l("Restaurar as calculadas", "Reset to computed"),
        }
    }

    pub(crate) fn tip(self) -> &'static str {
        match self {
            NormalOp::FromCenter => l("As normais saem do centro da seleção como numa esfera: a sombra cel do rosto fica redonda e limpa", "Normals point out from the selection center like a sphere: the face's cel shadow becomes round and clean"),
            NormalOp::FromView => l("Todas apontam para a câmera: a área fica sem sombra vista de frente", "All point at the camera: the area gets no shadow seen from the front"),
            NormalOp::FromObject => l("Copia as normais do objeto escolhido (ponha uma esfera por cima do rosto e transfira)", "Copies the normals of the chosen object (place a sphere over the face and transfer)"),
            NormalOp::Average => l("Média por área das faces em cada vértice: some com as quinas de sombra", "Face-area average at each vertex: removes shadow creases"),
            NormalOp::Smooth => l("Mistura cada normal com a dos vizinhos", "Blends each normal with its neighbors"),
            NormalOp::Reset => l("Tira as normais personalizadas", "Removes the custom normals"),
        }
    }
}

/// Ajustes do painel.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NormalsUi {
    /// Quanto da normal nova entra (0..1).
    pub factor: f32,
    /// Objeto de origem para "de outro objeto".
    pub source: Option<usize>,
    pub smooth_iters: u32,
}

impl Default for NormalsUi {
    fn default() -> Self {
        NormalsUi { factor: 1.0, source: None, smooth_iters: 4 }
    }
}

fn scope(ms: &ModelingState) -> NormalScope {
    if !ms.sel.faces.is_empty() {
        NormalScope::Faces(ms.sel.faces.iter().copied().collect())
    } else if !ms.sel.verts.is_empty() {
        NormalScope::Verts(ms.sel.verts.iter().copied().collect())
    } else {
        NormalScope::All
    }
}

/// Roda a operação no objeto ativo (um passo de desfazer). Devolve quantos cantos mudaram.
pub(crate) fn run(ms: &mut ModelingState, op: NormalOp, ui: NormalsUi) -> Result<usize, String> {
    let a = ms.active.filter(|&a| a < ms.doc.objects.len()).ok_or_else(|| l("Escolha um objeto", "Pick an object").to_owned())?;
    let sc = scope(ms);
    let factor = ui.factor.clamp(0.0, 1.0);
    let opts = NormalEditOptions { blend: if factor >= 0.999 { NormalBlend::Replace } else { NormalBlend::Mix(factor) }, weights: None };
    let model = ms.doc.objects[a].matrix();
    let inv = model.inverse();
    // centro da seleção (ou da malha) no espaço do objeto
    let center = {
        let m = &ms.doc.objects[a].mesh;
        let verts: Vec<u32> = match &sc {
            NormalScope::Faces(fs) => fs.iter().flat_map(|&f| m.faces[f as usize].verts().collect::<Vec<_>>()).collect(),
            NormalScope::Verts(vs) => vs.clone(),
            NormalScope::All => (0..m.vert_count() as u32).collect(),
        };
        let n = verts.len().max(1) as f32;
        verts.iter().map(|&v| m.co(v)).sum::<Vec3>() / n
    };
    let view_dir = inv.transform_vector3(-ms.cam.forward()).normalize_or(Vec3::Z);
    let src = match op {
        NormalOp::FromObject => {
            let s = ui.source.filter(|&s| s != a && s < ms.doc.objects.len()).ok_or_else(|| l("Escolha o objeto de origem (outro que não o editado)", "Pick the source object (not the edited one)").to_owned())?;
            let o = &ms.doc.objects[s];
            let to_dst = inv * o.matrix();
            let mut m = o.evaluated().unwrap_or_else(|_| o.mesh.clone());
            m.transform_positions(|p| to_dst.transform_point3(p));
            Some(m)
        }
        _ => None,
    };
    let label = op.label();
    let mut changed = 0usize;
    ms.edit_labeled(label, |d| {
        let mesh = &mut d.objects[a].mesh;
        let r = match op {
            NormalOp::FromCenter => nt::transfer_normals_from_point(mesh, &sc, center, false, &opts),
            NormalOp::FromView => nt::transfer_normals_directional(mesh, &sc, view_dir, &opts),
            NormalOp::FromObject => nt::transfer_normals_from_mesh(mesh, src.as_ref().expect("origem"), &sc, NormalMapping::ClosestSurface, factor, None, None),
            NormalOp::Average => nt::average_normals(mesh, &sc, AverageMode::FaceArea, &opts),
            NormalOp::Smooth => nt::smooth_normals_region(mesh, &sc, ui.smooth_iters.clamp(1, 64), factor.max(0.05)),
            NormalOp::Reset => nt::reset_normals(mesh, &sc),
        };
        changed = r.map_err(|e| format!("{label}: {e}"))?;
        // "Liso" e "Chapado" ignoram as normais personalizadas: o objeto passa ao automático, que respeita
        if op != NormalOp::Reset {
            d.objects[a].shading = aiai_3d_edit::doc::Shading::Auto;
        }
        Ok(())
    })?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_3d_edit::doc::Primitive;

    #[test]
    fn sphere_normals_on_a_cube_then_reset() {
        let mut ms = ModelingState::default();
        ms.add_primitive(Primitive::Cube).unwrap();
        let n = run(&mut ms, NormalOp::FromCenter, NormalsUi::default()).unwrap();
        assert!(n >= 24, "todos os cantos do cubo: {n}");
        let m = &ms.doc.objects[0].mesh;
        // cada normal aponta para fora do centro, na direção do vértice
        for f in &m.faces {
            for c in &f.corners {
                let want = m.co(c.vert).normalize();
                assert!(c.normal.unwrap().dot(want) > 0.99);
            }
        }
        assert!(ms.undo(), "um passo de desfazer");
        assert!(ms.redo());
        run(&mut ms, NormalOp::Reset, NormalsUi::default()).unwrap();
        assert!(ms.doc.objects[0].mesh.faces.iter().all(|f| f.corners.iter().all(|c| c.normal.is_none())));
    }

    #[test]
    fn transfer_from_another_object_and_keys() {
        let mut ms = ModelingState::default();
        ms.add_primitive(Primitive::Cube).unwrap();
        ms.add_primitive(Primitive::Sphere).unwrap();
        ms.active = Some(0);
        let ui = NormalsUi { source: Some(1), ..NormalsUi::default() };
        assert!(run(&mut ms, NormalOp::FromObject, ui).unwrap() > 0);
        assert!(run(&mut ms, NormalOp::FromObject, NormalsUi { source: Some(0), ..ui }).is_err(), "o próprio objeto não vale");
        for op in NormalOp::ALL {
            assert_eq!(NormalOp::from_key(op.key()), Some(op));
        }
    }
}
