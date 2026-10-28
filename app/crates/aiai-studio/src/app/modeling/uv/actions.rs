//! Ações de UV do Inspector/editor: cada uma roda dentro de `ModelingSession::edit_uv` (cópia da malha,
//! validação e checkpoint de undo só em caso de sucesso). Lógica livre de UI para ser testável.

use super::geom::{self, mirror_point, rotate90_point};
use super::super::ModelingState;
use aiai_3d_edit::Selection;
use aiai_3d_edit::uv::pack::{self, PackParams};
use aiai_3d_edit::uv::project::{self, BoxLayout};
use aiai_3d_edit::uv::seams::{self, CharacterSeamParams};
use aiai_3d_edit::uv::unwrap::{self, UnwrapMethod, UnwrapParams};
use aiai_3d_edit::{EditMesh, ModelError, ModelResult, SelectMode};
use glam::{Mat4, Vec2, Vec3};

/// Parâmetros editáveis no Inspector (todos `Copy` para entrar em closures sem emprestar a UI).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvParams {
    /// Resolução (px) usada em margens, densidade de texels e relatório.
    pub tex_size: u32,
    /// Limiar diedral de "costura por ângulo" (graus).
    pub seam_angle_deg: f32,
    /// Limiar diedral das costuras automáticas de personagem (graus).
    pub char_angle_deg: f32,
    pub method: UnwrapMethod,
    /// Margem entre ilhas do unwrap, em pixels de `tex_size`.
    pub margin_px: f32,
    pub correct_aspect: bool,
    /// Largura/altura da textura (só vale com `correct_aspect`).
    pub aspect: f32,
    pub fill_holes: bool,
    pub stretch_iters: u32,
    pub pack_margin_px: f32,
    pub pack_rotate: bool,
    /// Eixo normal da projeção planar: 0 = X, 1 = Y, 2 = Z.
    pub planar_axis: usize,
    /// Caixa: layout em cruz (`true`) ou uma célula por direção (`false`).
    pub box_cross: bool,
    pub smart_angle_deg: f32,
}

impl Default for UvParams {
    fn default() -> Self {
        Self {
            tex_size: 1024,
            seam_angle_deg: 60.0,
            char_angle_deg: 70.0,
            method: UnwrapMethod::Lscm,
            margin_px: 4.0,
            correct_aspect: true,
            aspect: 1.0,
            fill_holes: true,
            stretch_iters: 20,
            pack_margin_px: 4.0,
            pack_rotate: true,
            planar_axis: 2,
            box_cross: true,
            smart_angle_deg: 66.0,
        }
    }
}

impl UvParams {
    fn margin_frac(&self) -> f32 {
        self.margin_px / self.tex_size.max(1) as f32
    }

    pub fn unwrap_params(&self) -> UnwrapParams {
        UnwrapParams {
            method: self.method,
            fill_holes: self.fill_holes,
            margin: self.margin_frac(),
            correct_aspect: self.correct_aspect,
            aspect: self.aspect,
            ..UnwrapParams::default()
        }
    }

    pub fn pack_params(&self) -> PackParams {
        PackParams { margin: self.pack_margin_px, texture_size: self.tex_size, rotate: self.pack_rotate, ..PackParams::default() }
    }
}

/// Edita a malha ativa em CÓPIA (como o `ModelingSession::edit_uv` do ANIGO): se `f` der certo e a malha validar,
/// o documento de antes entra no desfazer (só se mudou algo); em erro nada muda.
fn edit_uv<T>(ms: &mut ModelingState, label: &str, f: impl FnOnce(&mut EditMesh, &Selection) -> ModelResult<T>) -> Result<T, ModelError> {
    let idx = ms.active.filter(|&a| a < ms.doc.objects.len()).ok_or(ModelError::EmptySelection("sem objeto ativo"))?;
    let mut mesh = ms.doc.objects[idx].mesh.clone();
    let out = f(&mut mesh, &ms.sel)?;
    mesh.validate()?;
    if mesh != ms.doc.objects[idx].mesh {
        let before = ms.doc.clone();
        ms.doc.objects[idx].mesh = mesh;
        ms.history.push_owned(label, before);
        ms.dirty = true;
        ms.touch();
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvAction {
    MarkSeam,
    ClearSeam,
    ClearAllSeams,
    SeamsByAngle,
    SeamPath,
    AutoSeamsCharacter,
    SeamsFromSharp,
    Unwrap,
    MinimizeStretch,
    Pack,
    NormalizeDensity,
    ProjectPlanar,
    ProjectBox,
    ProjectCylinder,
    ProjectSphere,
    SmartProject,
    ProjectFromView,
    FlipU,
    FlipV,
    Rotate90,
}

impl UvAction {
    pub const ALL: [UvAction; 20] = [
        UvAction::MarkSeam,
        UvAction::ClearSeam,
        UvAction::ClearAllSeams,
        UvAction::SeamsByAngle,
        UvAction::SeamPath,
        UvAction::AutoSeamsCharacter,
        UvAction::SeamsFromSharp,
        UvAction::Unwrap,
        UvAction::MinimizeStretch,
        UvAction::Pack,
        UvAction::NormalizeDensity,
        UvAction::ProjectPlanar,
        UvAction::ProjectBox,
        UvAction::ProjectCylinder,
        UvAction::ProjectSphere,
        UvAction::SmartProject,
        UvAction::ProjectFromView,
        UvAction::FlipU,
        UvAction::FlipV,
        UvAction::Rotate90,
    ];

    /// Chave (ponte/MCP).
    pub fn key(self) -> &'static str {
        match self {
            UvAction::MarkSeam => "mark_seam",
            UvAction::ClearSeam => "clear_seam",
            UvAction::ClearAllSeams => "clear_all_seams",
            UvAction::SeamsByAngle => "seams_by_angle",
            UvAction::SeamPath => "seam_path",
            UvAction::AutoSeamsCharacter => "auto_seams_character",
            UvAction::SeamsFromSharp => "seams_from_sharp",
            UvAction::Unwrap => "unwrap",
            UvAction::MinimizeStretch => "minimize_stretch",
            UvAction::Pack => "pack",
            UvAction::NormalizeDensity => "normalize_density",
            UvAction::ProjectPlanar => "project_planar",
            UvAction::ProjectBox => "project_box",
            UvAction::ProjectCylinder => "project_cylinder",
            UvAction::ProjectSphere => "project_sphere",
            UvAction::SmartProject => "smart_project",
            UvAction::ProjectFromView => "project_from_view",
            UvAction::FlipU => "flip_u",
            UvAction::FlipV => "flip_v",
            UvAction::Rotate90 => "rotate_90",
        }
    }

    /// Rótulo (também usado no histórico de undo).
    pub fn label(self) -> &'static str {
        match self {
            UvAction::MarkSeam => "Marcar costura",
            UvAction::ClearSeam => "Limpar costura",
            UvAction::ClearAllSeams => "Limpar todas as costuras",
            UvAction::SeamsByAngle => "Costuras por ângulo",
            UvAction::SeamPath => "Costura pelo caminho mais curto",
            UvAction::AutoSeamsCharacter => "Costuras automáticas (personagem)",
            UvAction::SeamsFromSharp => "Costuras das arestas duras",
            UvAction::Unwrap => "Unwrap",
            UvAction::MinimizeStretch => "Minimizar estiramento",
            UvAction::Pack => "Empacotar ilhas",
            UvAction::NormalizeDensity => "Normalizar densidade de texel",
            UvAction::ProjectPlanar => "Projeção planar",
            UvAction::ProjectBox => "Projeção em caixa",
            UvAction::ProjectCylinder => "Projeção cilíndrica",
            UvAction::ProjectSphere => "Projeção esférica",
            UvAction::SmartProject => "Smart UV Project",
            UvAction::ProjectFromView => "Projetar da vista",
            UvAction::FlipU => "Espelhar UV (U)",
            UvAction::FlipV => "Espelhar UV (V)",
            UvAction::Rotate90 => "Girar UV 90°",
        }
    }
}

fn faces_bbox(mesh: &EditMesh, scope: Option<&[u32]>) -> Option<(Vec3, Vec3)> {
    let mut it: Box<dyn Iterator<Item = Vec3> + '_> = match scope {
        Some(fs) => Box::new(fs.iter().filter_map(|&f| mesh.faces.get(f as usize)).flat_map(|f| f.corners.iter().map(|c| mesh.co(c.vert)))),
        None => Box::new(mesh.faces.iter().flat_map(|f| f.corners.iter().map(|c| mesh.co(c.vert)))),
    };
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}

/// Transforma os UVs selecionados (ou todos, se nada está selecionado) em torno do centro da caixa deles.
fn transform_selected(
    mesh: &mut EditMesh,
    sel: &aiai_3d_edit::Selection,
    mode: SelectMode,
    f: impl Fn(Vec2, Vec2) -> Vec2,
) -> ModelResult<usize> {
    let mut corners = geom::selected_corners(mesh, sel, mode);
    if corners.is_empty() {
        corners = mesh
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.hidden)
            .flat_map(|(fi, f)| (0..f.corners.len()).map(move |ci| (fi as u32, ci)))
            .collect();
    }
    let (lo, hi) = geom::corners_bounds(mesh, &corners).ok_or(ModelError::EmptySelection("a malha não tem faces"))?;
    let pivot = (lo + hi) * 0.5;
    for &(fi, ci) in &corners {
        if let Some(c) = mesh.faces.get_mut(fi as usize).and_then(|x| x.corners.get_mut(ci)) {
            c.uv = f(c.uv, pivot);
        }
    }
    Ok(corners.len())
}

/// Executa uma ação de UV sobre a malha ativa. `view_proj` = projeção*vista*modelo (só para "da vista").
/// Devolve o texto de sucesso para a barra de status; em erro a malha e o histórico ficam intactos.
pub fn run_action(session: &mut ModelingState, a: UvAction, p: &UvParams, view_proj: Option<Mat4>) -> Result<String, ModelError> {
    let label = a.label();
    let mode = session.sel_mode;
    let (pi, tex) = (*p, p.tex_size);
    // verts selecionados em ordem estável (para o caminho mais curto)
    let sel_verts: Vec<u32> = session.sel.verts.iter().copied().collect();
    edit_uv(session, label, move |mesh: &mut EditMesh, sel: &Selection| {
        let scope_vec = geom::scope_faces(sel);
        let scope = scope_vec.as_deref();
        let scope_txt = if scope.is_some() { "nas faces selecionadas" } else { "na malha toda" };
        match a {
            UvAction::MarkSeam | UvAction::ClearSeam => {
                let edges = geom::selected_edges(mesh, sel);
                if edges.is_empty() {
                    return Err(ModelError::EmptySelection("selecione arestas (ou vértices/faces ligados) primeiro"));
                }
                let mark = a == UvAction::MarkSeam;
                let n = seams::mark_seams(mesh, &edges, mark)?;
                Ok(match (mark, n) {
                    (true, 0) => "as arestas já eram costuras".to_string(),
                    (false, 0) => "nenhuma costura nas arestas selecionadas".to_string(),
                    (true, n) => format!("{n} costura(s) marcada(s)"),
                    (false, n) => format!("{n} costura(s) removida(s)"),
                })
            }
            UvAction::ClearAllSeams => {
                let n = seams::clear_seams(mesh);
                Ok(format!("{n} costura(s) removida(s)"))
            }
            UvAction::SeamsByAngle => {
                let n = seams::auto_seams_by_angle(mesh, pi.seam_angle_deg.to_radians())?;
                Ok(format!("{n} costura(s) nova(s) acima de {:.0}°", pi.seam_angle_deg))
            }
            UvAction::SeamPath => {
                let [from, to] = sel_verts[..] else {
                    return Err(ModelError::InvalidParameter(format!(
                        "selecione exatamente 2 vértices para o caminho (há {} selecionados)",
                        sel_verts.len()
                    )));
                };
                let path = seams::seam_path(mesh, from, to)?;
                let n = seams::mark_seams(mesh, &path, true)?;
                Ok(format!("caminho de {} aresta(s), {n} nova(s) costura(s)", path.len()))
            }
            UvAction::AutoSeamsCharacter => {
                let params = CharacterSeamParams { angle: pi.char_angle_deg.to_radians(), ..CharacterSeamParams::default() };
                let r = seams::auto_seams_for_character_with(mesh, &params)?;
                let mut s = format!(
                    "{} por ângulo, {} de corte, {} em pontas; {} ilha(s)",
                    r.angle_seams, r.cut_seams, r.point_seams, r.islands
                );
                if r.non_disk_islands > 0 {
                    s.push_str(&format!(" ({} ainda não é disco)", r.non_disk_islands));
                }
                Ok(s)
            }
            UvAction::SeamsFromSharp => {
                let n = seams::seams_from_sharp(mesh);
                Ok(format!("{n} costura(s) a partir de arestas duras"))
            }
            UvAction::Unwrap => {
                let r = unwrap::unwrap_lscm(mesh, scope, &pi.unwrap_params())?;
                let mut s = format!("{} ilha(s), {} face(s) {scope_txt}", r.islands, r.faces);
                if r.non_disk_islands > 0 {
                    s.push_str(&format!("; {} ilha(s) não-disco (distorção maior)", r.non_disk_islands));
                }
                if pi.method == UnwrapMethod::LscmArap {
                    s.push_str(&format!("; ARAP melhorou {}", r.arap_improved_islands));
                }
                Ok(s)
            }
            UvAction::MinimizeStretch => {
                let r = unwrap::unwrap_minimum_stretch_in(mesh, scope, pi.stretch_iters.max(1))?;
                Ok(format!(
                    "distorção média {:.3} -> {:.3} ({} de {} ilha(s) melhoraram)",
                    r.mean_before, r.mean_after, r.improved, r.islands
                ))
            }
            UvAction::Pack => {
                let r = pack::pack_islands_in(mesh, scope, &pi.pack_params())?;
                Ok(format!("{} ilha(s), ocupação {:.0}%, {} girada(s)", r.islands, r.uv_area_ratio * 100.0, r.rotated))
            }
            UvAction::NormalizeDensity => {
                let d = pack::normalize_texel_density(mesh, tex, None)?;
                let r = pack::pack_islands(mesh, &pi.pack_params())?;
                Ok(format!("densidade uniforme de {d:.0} px/u; reempacotado ({:.0}% de ocupação)", r.uv_area_ratio * 100.0))
            }
            UvAction::ProjectPlanar => {
                let (lo, hi) = faces_bbox(mesh, scope).ok_or(ModelError::EmptySelection("sem faces para projetar"))?;
                let (u, v) = match pi.planar_axis {
                    0 => (Vec3::Y, Vec3::Z),
                    1 => (Vec3::Z, Vec3::X),
                    _ => (Vec3::X, Vec3::Y),
                };
                let ext = hi - lo;
                let big = ext.dot(u).abs().max(ext.dot(v).abs()).max(1e-6);
                project::project_planar(mesh, scope, lo, u, v, 1.0 / big)?;
                Ok(format!("planar no eixo {} {scope_txt}", ["X", "Y", "Z"][pi.planar_axis.min(2)]))
            }
            UvAction::ProjectBox | UvAction::ProjectCylinder | UvAction::ProjectSphere => {
                let (lo, hi) = faces_bbox(mesh, scope).ok_or(ModelError::EmptySelection("sem faces para projetar"))?;
                let (center, size) = ((lo + hi) * 0.5, (hi - lo).max(Vec3::splat(1e-3)));
                match a {
                    UvAction::ProjectBox => {
                        let layout = if pi.box_cross { BoxLayout::Cross } else { BoxLayout::PerFace };
                        project::project_box(mesh, scope, center, size, layout)?;
                    }
                    UvAction::ProjectCylinder => project::project_cylinder(mesh, scope, center, Vec3::Y, None)?,
                    _ => project::project_sphere(mesh, scope, center)?,
                }
                Ok(format!("{} {scope_txt}", label.to_lowercase()))
            }
            UvAction::SmartProject => {
                let r = project::smart_uv_project_in(mesh, scope, pi.smart_angle_deg.to_radians(), pi.pack_margin_px / tex.max(1) as f32)?;
                Ok(format!("{} grupo(s) planar(es), ocupação {:.0}%", r.groups, r.pack.uv_area_ratio * 100.0))
            }
            UvAction::ProjectFromView => {
                let vp = view_proj.ok_or(ModelError::InvalidParameter("sem câmera para projetar".into()))?;
                project::project_from_view(mesh, scope, vp)?;
                Ok(format!("projetado da vista atual {scope_txt}"))
            }
            UvAction::FlipU | UvAction::FlipV => {
                let horizontal = a == UvAction::FlipU;
                let n = transform_selected(mesh, sel, mode, |p, pivot| mirror_point(p, pivot, horizontal))?;
                Ok(format!("{n} canto(s) espelhado(s)"))
            }
            UvAction::Rotate90 => {
                let n = transform_selected(mesh, sel, mode, rotate90_point)?;
                Ok(format!("{n} canto(s) girado(s)"))
            }
        }
    })
}

/// Arestas do complemento de uma árvore geradora do grafo dual (faces ligadas por aresta), a partir da
/// face 0: abre qualquer malha conexa em uma única ilha. Num cubo isso dá a "cruz" clássica.
pub fn spanning_tree_seams(mesh: &EditMesh) -> Vec<aiai_3d_edit::EdgeKey> {
    use aiai_3d_edit::Topology;
    let topo = Topology::build(mesh);
    let n = mesh.faces.len();
    let mut seen = vec![false; n];
    let mut kept: std::collections::BTreeSet<aiai_3d_edit::EdgeKey> = Default::default();
    for root in 0..n {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut queue = std::collections::VecDeque::from([root as u32]);
        while let Some(f) = queue.pop_front() {
            let face = &mesh.faces[f as usize];
            let k = face.corners.len();
            for i in 0..k {
                let (a, b) = (face.corners[i].vert, face.corners[(i + 1) % k].vert);
                for &o in topo.edge_faces(a, b) {
                    if !seen[o as usize] {
                        seen[o as usize] = true;
                        kept.insert(aiai_3d_edit::edge_key(a, b));
                        queue.push_back(o);
                    }
                }
            }
        }
    }
    topo.edges.iter().map(|e| e.verts).filter(|k| !kept.contains(k) && topo.edge_faces(k.0, k.1).len() == 2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_3d_edit::doc::Primitive;
    use aiai_3d_edit::uv::metrics;

    fn state(kind: Primitive) -> ModelingState {
        let mut ms = ModelingState::default();
        ms.add_primitive(kind).unwrap();
        ms.mode = super::super::super::EditMode::Edit;
        ms
    }

    #[test]
    fn cube_cross_seams_unwrap_into_one_island_and_pack() {
        let mut ms = state(Primitive::Cube);
        let edges = spanning_tree_seams(&ms.doc.objects[0].mesh);
        ms.sel_mode = SelectMode::Edge;
        ms.set_selection(Selection::from_edges(edges));
        let p = UvParams::default();
        let steps = ms.history.labels().len();
        run_action(&mut ms, UvAction::MarkSeam, &p, None).unwrap();
        ms.set_selection(Selection::default());
        run_action(&mut ms, UvAction::Unwrap, &p, None).unwrap();
        run_action(&mut ms, UvAction::Pack, &p, None).unwrap();
        assert!(ms.history.labels().len() >= steps + 2, "costura e desdobrar entram no desfazer (empacotar só se mudou algo)");
        let r = metrics::uv_report(&ms.doc.objects[0].mesh, 1024).unwrap();
        assert_eq!(r.island_count, 1, "a cruz abre o cubo numa ilha só");
        assert_eq!(r.flipped_faces, 0);
    }

    #[test]
    fn failing_action_changes_nothing() {
        let mut ms = state(Primitive::Cube);
        let before = ms.doc.clone();
        let steps = ms.history.labels().len();
        assert!(run_action(&mut ms, UvAction::MarkSeam, &UvParams::default(), None).is_err(), "sem arestas escolhidas");
        assert_eq!(ms.doc, before);
        assert_eq!(ms.history.labels().len(), steps);
    }

    #[test]
    fn projections_and_keys() {
        let mut ms = state(Primitive::Sphere);
        for a in [UvAction::ProjectSphere, UvAction::ProjectBox, UvAction::SmartProject, UvAction::FlipU, UvAction::Rotate90] {
            run_action(&mut ms, a, &UvParams::default(), None).unwrap_or_else(|e| panic!("{a:?}: {e}"));
        }
        for a in UvAction::ALL {
            assert_eq!(super::super::action_from_key(a.key()), Some(a));
        }
    }
}
