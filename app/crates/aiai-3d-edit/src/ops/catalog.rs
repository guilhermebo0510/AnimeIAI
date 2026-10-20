//! Catálogo dos operadores do modo Edição (copiado do `anigo-studio/src/app/operators.rs` do ANIGO: `OpKind`, `OpParams`,
//! `apply_kind`, puros). A interface (painel Operadores e "Ajustar última operação") e a MCP chamam o mesmo `apply_kind`:
//! o operador valida antes de mudar a malha (erro = malha intacta) e devolve a seleção resultante.

use crate::ops::{bevel, bisect, bridge, delete, dissolve, extrude, fill, flip, inset, loops, merge, misc, smooth, triangulate, OpOutput};
use crate::{EditMesh, ModelError, ModelResult, Selection};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Extrude,
    ExtrudeIndividual,
    Inset,
    InsetIndividual,
    Bevel,
    LoopCut,
    Bisect,
    DeleteVerts,
    DeleteEdges,
    DeleteFaces,
    DeleteOnlyFaces,
    MergeCenter,
    MergeByDistance,
    DissolveVerts,
    DissolveEdges,
    DissolveFaces,
    LimitedDissolve,
    FillHole,
    GridFill,
    Bridge,
    Triangulate,
    TrisToQuads,
    FlipNormals,
    RecalcOutside,
    Smooth,
    Relax,
    Subdivide,
    Poke,
    MarkSharp,
    ClearSharp,
    MarkSeam,
    ClearSeam,
    SetCrease,
    HideSelected,
    RevealAll,
}

impl OpKind {
    pub const ALL: [OpKind; 35] = [
        OpKind::Extrude,
        OpKind::ExtrudeIndividual,
        OpKind::Inset,
        OpKind::InsetIndividual,
        OpKind::Bevel,
        OpKind::LoopCut,
        OpKind::Bisect,
        OpKind::DeleteVerts,
        OpKind::DeleteEdges,
        OpKind::DeleteFaces,
        OpKind::DeleteOnlyFaces,
        OpKind::MergeCenter,
        OpKind::MergeByDistance,
        OpKind::DissolveVerts,
        OpKind::DissolveEdges,
        OpKind::DissolveFaces,
        OpKind::LimitedDissolve,
        OpKind::FillHole,
        OpKind::GridFill,
        OpKind::Bridge,
        OpKind::Triangulate,
        OpKind::TrisToQuads,
        OpKind::FlipNormals,
        OpKind::RecalcOutside,
        OpKind::Smooth,
        OpKind::Relax,
        OpKind::Subdivide,
        OpKind::Poke,
        OpKind::MarkSharp,
        OpKind::ClearSharp,
        OpKind::MarkSeam,
        OpKind::ClearSeam,
        OpKind::SetCrease,
        OpKind::HideSelected,
        OpKind::RevealAll,
    ];

    /// Nome estável (snake_case) usado pela bridge/MCP.
    pub fn key(self) -> &'static str {
        match self {
            OpKind::Extrude => "extrude",
            OpKind::ExtrudeIndividual => "extrude_individual",
            OpKind::Inset => "inset",
            OpKind::InsetIndividual => "inset_individual",
            OpKind::Bevel => "bevel",
            OpKind::LoopCut => "loop_cut",
            OpKind::Bisect => "bisect",
            OpKind::DeleteVerts => "delete_verts",
            OpKind::DeleteEdges => "delete_edges",
            OpKind::DeleteFaces => "delete_faces",
            OpKind::DeleteOnlyFaces => "delete_only_faces",
            OpKind::MergeCenter => "merge_center",
            OpKind::MergeByDistance => "merge_by_distance",
            OpKind::DissolveVerts => "dissolve_verts",
            OpKind::DissolveEdges => "dissolve_edges",
            OpKind::DissolveFaces => "dissolve_faces",
            OpKind::LimitedDissolve => "limited_dissolve",
            OpKind::FillHole => "fill_hole",
            OpKind::GridFill => "grid_fill",
            OpKind::Bridge => "bridge",
            OpKind::Triangulate => "triangulate",
            OpKind::TrisToQuads => "tris_to_quads",
            OpKind::FlipNormals => "flip_normals",
            OpKind::RecalcOutside => "recalc_outside",
            OpKind::Smooth => "smooth",
            OpKind::Relax => "relax",
            OpKind::Subdivide => "subdivide",
            OpKind::Poke => "poke",
            OpKind::MarkSharp => "mark_sharp",
            OpKind::ClearSharp => "clear_sharp",
            OpKind::MarkSeam => "mark_seam",
            OpKind::ClearSeam => "clear_seam",
            OpKind::SetCrease => "set_crease",
            OpKind::HideSelected => "hide_selected",
            OpKind::RevealAll => "reveal_all",
        }
    }

    pub fn from_key(k: &str) -> Option<OpKind> {
        Self::ALL.into_iter().find(|o| o.key() == k)
    }

    /// Operadores que exigem uma seleção/ação direta (sem painel de ajuste de parâmetros).
    pub fn has_params(self) -> bool {
        matches!(
            self,
            OpKind::Extrude
                | OpKind::Inset
                | OpKind::InsetIndividual
                | OpKind::Bevel
                | OpKind::LoopCut
                | OpKind::Bisect
                | OpKind::MergeByDistance
                | OpKind::LimitedDissolve
                | OpKind::Bridge
                | OpKind::Smooth
                | OpKind::Relax
                | OpKind::Subdivide
                | OpKind::Poke
                | OpKind::SetCrease
                | OpKind::GridFill
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            OpKind::Extrude => "Extrudar região",
            OpKind::ExtrudeIndividual => "Extrudar individual",
            OpKind::Inset => "Inset região",
            OpKind::InsetIndividual => "Inset individual",
            OpKind::Bevel => "Chanfro",
            OpKind::LoopCut => "Corte em loop",
            OpKind::Bisect => "Cortar por plano",
            OpKind::DeleteVerts => "Excluir vértices",
            OpKind::DeleteEdges => "Excluir arestas",
            OpKind::DeleteFaces => "Excluir faces",
            OpKind::DeleteOnlyFaces => "Excluir só faces",
            OpKind::MergeCenter => "Unir no centro",
            OpKind::MergeByDistance => "Unir por distância",
            OpKind::DissolveVerts => "Dissolver vértices",
            OpKind::DissolveEdges => "Dissolver arestas",
            OpKind::DissolveFaces => "Dissolver faces",
            OpKind::LimitedDissolve => "Dissolução limitada",
            OpKind::FillHole => "Preencher buraco",
            OpKind::GridFill => "Preencher em grade",
            OpKind::Bridge => "Ponte entre loops",
            OpKind::Triangulate => "Triangular",
            OpKind::TrisToQuads => "Triângulos para quads",
            OpKind::FlipNormals => "Inverter normais",
            OpKind::RecalcOutside => "Recalcular normais (fora)",
            OpKind::Smooth => "Suavizar vértices",
            OpKind::Relax => "Relaxar vértices",
            OpKind::Subdivide => "Subdividir",
            OpKind::Poke => "Poke",
            OpKind::MarkSharp => "Marcar aresta dura",
            OpKind::ClearSharp => "Limpar aresta dura",
            OpKind::MarkSeam => "Marcar costura",
            OpKind::ClearSeam => "Limpar costura",
            OpKind::SetCrease => "Vinco de subdivisão",
            OpKind::HideSelected => "Ocultar seleção",
            OpKind::RevealAll => "Revelar tudo",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpParams {
    pub distance: f32,
    pub thickness: f32,
    pub depth: f32,
    pub outset: bool,
    pub even: bool,
    pub width: f32,
    pub segments: u32,
    pub profile: f32,
    pub cuts: u32,
    pub slide: f32,
    /// 0 = X, 1 = Y, 2 = Z (plano do bisect, em espaço local).
    pub plane_axis: usize,
    pub plane_offset: f32,
    pub clear_inner: bool,
    pub clear_outer: bool,
    pub fill: bool,
    pub factor: f32,
    pub iterations: u32,
    pub merge_distance: f32,
    pub dissolve_angle_deg: f32,
    pub subdiv_cuts: u32,
    pub subdiv_smooth: f32,
    pub crease: f32,
    pub poke_offset: f32,
    pub lock_boundary: bool,
}

impl Default for OpParams {
    fn default() -> Self {
        Self {
            distance: 0.0,
            thickness: 0.15,
            depth: 0.0,
            outset: false,
            even: true,
            width: 0.1,
            segments: 2,
            profile: 0.5,
            cuts: 1,
            slide: 0.0,
            plane_axis: 1,
            plane_offset: 0.0,
            clear_inner: false,
            clear_outer: false,
            fill: true,
            factor: 0.5,
            iterations: 1,
            merge_distance: 0.001,
            dissolve_angle_deg: 5.0,
            subdiv_cuts: 1,
            subdiv_smooth: 0.0,
            crease: 1.0,
            poke_offset: 0.0,
            lock_boundary: true,
        }
    }
}

impl OpParams {
    /// Aplica sobreposições vindas de JSON (`{"thickness":0.2,"segments":3,…}`). Chave desconhecida ou tipo errado = erro
    /// (nada é aplicado pela metade).
    pub fn apply_json(&mut self, m: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
        let mut next = self.clone();
        for (k, v) in m {
            let num = || v.as_f64().filter(|f| f.is_finite()).map(|f| f as f32).ok_or_else(|| format!("'{k}' deve ser número"));
            let uint = || v.as_u64().filter(|n| *n <= 10_000).map(|n| n as u32).ok_or_else(|| format!("'{k}' deve ser inteiro 0..10000"));
            let flag = || v.as_bool().ok_or_else(|| format!("'{k}' deve ser true|false"));
            match k.as_str() {
                "distance" => next.distance = num()?,
                "thickness" => next.thickness = num()?,
                "depth" => next.depth = num()?,
                "outset" => next.outset = flag()?,
                "even" => next.even = flag()?,
                "width" => next.width = num()?,
                "segments" => next.segments = uint()?.max(1),
                "profile" => next.profile = num()?,
                "cuts" => next.cuts = uint()?.max(1),
                "slide" => next.slide = num()?,
                "plane_axis" => next.plane_axis = (uint()? as usize).min(2),
                "plane_offset" => next.plane_offset = num()?,
                "clear_inner" => next.clear_inner = flag()?,
                "clear_outer" => next.clear_outer = flag()?,
                "fill" => next.fill = flag()?,
                "factor" => next.factor = num()?,
                "iterations" => next.iterations = uint()?.max(1),
                "merge_distance" => next.merge_distance = num()?,
                "dissolve_angle_deg" => next.dissolve_angle_deg = num()?,
                "subdiv_cuts" => next.subdiv_cuts = uint()?.max(1),
                "subdiv_smooth" => next.subdiv_smooth = num()?,
                "crease" => next.crease = num()?,
                "poke_offset" => next.poke_offset = num()?,
                "lock_boundary" => next.lock_boundary = flag()?,
                other => return Err(format!("parâmetro desconhecido: '{other}'")),
            }
        }
        *self = next;
        Ok(())
    }
}

/// Aplica `kind` com `params` na malha. Puro: nenhuma dependência de UI (testável).
pub fn apply_kind(kind: OpKind, params: &OpParams, mesh: &mut EditMesh, sel: &Selection) -> ModelResult<OpOutput> {
    let faces: Vec<u32> = sel.faces.iter().copied().collect();
    match kind {
        OpKind::Extrude => {
            if !faces.is_empty() {
                if params.distance != 0.0 {
                    extrude::extrude_faces_region_along_normal(mesh, &faces, params.distance)
                } else {
                    extrude::extrude_faces_region(mesh, &faces)
                }
            } else if !sel.edges.is_empty() {
                let edges: Vec<_> = sel.edges.iter().copied().collect();
                extrude::extrude_edges(mesh, &edges)
            } else {
                let verts: Vec<u32> = sel.verts.iter().copied().collect();
                extrude::extrude_verts(mesh, &verts)
            }
        }
        OpKind::ExtrudeIndividual => extrude::extrude_faces_individual(mesh, &faces),
        OpKind::Inset => inset::inset_faces_region(mesh, &faces, params.thickness, params.depth, params.outset, true, params.even),
        OpKind::InsetIndividual => inset::inset_faces_individual(mesh, &faces, params.thickness, params.depth),
        OpKind::Bevel => {
            let edges: Vec<_> = sel.edges.iter().copied().collect();
            bevel::bevel_edges(mesh, &edges, params.width, params.segments, params.profile, true)
        }
        OpKind::LoopCut => {
            let Some(edge) = sel.edges.iter().next().copied() else {
                return Err(ModelError::EmptySelection("selecione uma aresta do anel a cortar"));
            };
            loops::loop_cut(mesh, edge, params.cuts, params.slide).map(Into::into)
        }
        OpKind::Bisect => {
            let mut normal = Vec3::ZERO;
            normal[params.plane_axis.min(2)] = 1.0;
            bisect::bisect(mesh, normal * params.plane_offset, normal, params.clear_inner, params.clear_outer, params.fill, 1e-5)
        }
        _ => apply_more(kind, params, mesh, sel),
    }
}

fn components(edges: &[crate::EdgeKey]) -> Vec<Vec<crate::EdgeKey>> {
    let mut parent: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    fn find(p: &mut std::collections::BTreeMap<u32, u32>, x: u32) -> u32 {
        let mut r = x;
        while p.get(&r).copied().unwrap_or(r) != r {
            r = p[&r];
        }
        p.insert(x, r);
        r
    }
    for &(a, b) in edges {
        parent.entry(a).or_insert(a);
        parent.entry(b).or_insert(b);
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent.insert(ra, rb);
        }
    }
    let mut groups: std::collections::BTreeMap<u32, Vec<crate::EdgeKey>> = std::collections::BTreeMap::new();
    for &e in edges {
        let r = find(&mut parent, e.0);
        groups.entry(r).or_default().push(e);
    }
    groups.into_values().collect()
}

fn apply_more(kind: OpKind, params: &OpParams, mesh: &mut EditMesh, sel: &Selection) -> ModelResult<OpOutput> {
    let faces: Vec<u32> = sel.faces.iter().copied().collect();
    let edges: Vec<crate::EdgeKey> = sel.edges.iter().copied().collect();
    let verts: Vec<u32> = sel.verts.iter().copied().collect();
    match kind {
        OpKind::DeleteVerts => delete::delete_verts(mesh, &verts),
        OpKind::DeleteEdges => delete::delete_edges(mesh, &edges),
        OpKind::DeleteFaces => delete::delete_faces(mesh, &faces),
        OpKind::DeleteOnlyFaces => delete::delete_faces_keep_verts(mesh, &faces),
        OpKind::MergeCenter => merge::merge_verts(mesh, &verts, merge::MergeMode::Center),
        OpKind::MergeByDistance => {
            let scope = (!verts.is_empty()).then_some(verts.as_slice());
            merge::merge_by_distance(mesh, scope, params.merge_distance, merge::ByDistanceOptions::default())
        }
        OpKind::DissolveVerts => dissolve::dissolve_verts(mesh, &verts, false, false),
        OpKind::DissolveEdges => dissolve::dissolve_edges(mesh, &edges, true),
        OpKind::DissolveFaces => dissolve::dissolve_faces(mesh, &faces),
        OpKind::LimitedDissolve => dissolve::limited_dissolve(
            mesh,
            params.dissolve_angle_deg.to_radians(),
            false,
            dissolve::Delimit::default(),
        ),
        OpKind::FillHole => fill::fill_hole(mesh, &edges),
        OpKind::GridFill => fill::grid_fill(mesh, &edges, 0, 0, false),
        OpKind::Bridge => {
            let comps = components(&edges);
            if comps.len() != 2 {
                return Err(ModelError::InvalidParameter(format!(
                    "a ponte precisa de exatamente 2 loops de arestas selecionados (há {})",
                    comps.len()
                )));
            }
            let topo = crate::Topology::build(mesh);
            let path = |c: &Vec<crate::EdgeKey>| -> ModelResult<loops::EdgePath> {
                let start = c[0];
                if topo.is_boundary_edge(start.0, start.1) {
                    loops::boundary_loop(mesh, &topo, start)
                } else {
                    loops::edge_loop(mesh, &topo, start)
                }
            };
            let (a, b) = (path(&comps[0])?, path(&comps[1])?);
            bridge::bridge_edge_loops(mesh, &a, &b, 1, 0, bridge::BridgeInterpolation::Linear)
        }
        OpKind::Triangulate => triangulate::triangulate_faces(
            mesh,
            &faces,
            triangulate::QuadMethod::Beauty,
            triangulate::NgonMethod::Beauty,
        ),
        OpKind::TrisToQuads => triangulate::tris_to_quads(mesh, &faces, 40f32.to_radians(), true, true, true),
        OpKind::FlipNormals => flip::flip_faces(mesh, &faces),
        OpKind::RecalcOutside => flip::recalc_normals_outside(mesh, false),
        OpKind::Smooth => smooth::smooth_verts(
            mesh,
            &verts,
            params.factor,
            params.iterations,
            params.lock_boundary,
            [true; 3],
            smooth::SmoothWeights::Uniform,
        ),
        OpKind::Relax => smooth::relax_verts(
            mesh,
            &verts,
            params.factor,
            params.iterations,
            smooth::RelaxMethod::Taubin { pass_band: 0.1 },
            params.lock_boundary,
        ),
        OpKind::Subdivide => {
            if !faces.is_empty() {
                misc::subdivide_faces(mesh, &faces, params.subdiv_cuts as usize, params.subdiv_smooth)
            } else {
                misc::subdivide_edges(mesh, &edges, params.subdiv_cuts as usize, params.subdiv_smooth, 0.0, 1)
            }
        }
        OpKind::Poke => misc::poke_faces(mesh, &faces, params.poke_offset, true),
        OpKind::MarkSharp => misc::mark_sharp(mesh, &edges, true),
        OpKind::ClearSharp => misc::mark_sharp(mesh, &edges, false),
        OpKind::MarkSeam => misc::mark_seam(mesh, &edges, true),
        OpKind::ClearSeam => misc::mark_seam(mesh, &edges, false),
        OpKind::SetCrease => misc::set_crease(mesh, &edges, params.crease),
        OpKind::HideSelected => misc::hide_faces(mesh, &faces),
        OpKind::RevealAll => Ok(misc::reveal_all(mesh)),
        // os demais já foram tratados em `apply_kind`
        _ => Err(ModelError::InvalidParameter("operador sem implementação neste ramo".into())),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{primitives, SelectMode, Topology};

    #[test]
    fn op_keys_are_unique_snake_case_and_roundtrip() {
        let mut seen = std::collections::HashSet::new();
        for k in OpKind::ALL {
            assert!(seen.insert(k.key()), "chave repetida: {}", k.key());
            assert!(k.key().chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", k.key());
            assert_eq!(OpKind::from_key(k.key()), Some(k));
        }
        assert_eq!(seen.len(), 35);
        assert_eq!(OpKind::from_key("nao_existe"), None);
    }

    #[test]
    fn json_overrides_are_validated_and_atomic() {
        let mut p = OpParams::default();
        let ok = serde_json::json!({ "thickness": 0.3, "segments": 4, "outset": true, "cuts": 0 });
        p.apply_json(ok.as_object().unwrap()).unwrap();
        assert_eq!((p.thickness, p.segments, p.outset), (0.3, 4, true));
        assert_eq!(p.cuts, 1, "contagens nunca chegam a zero");
        let before = p.clone();
        for bad in [serde_json::json!({ "width": 0.9, "bogus": 1 }), serde_json::json!({ "width": "x" }), serde_json::json!({ "segments": 99999 }), serde_json::json!({ "outset": 1 })] {
            assert!(p.apply_json(bad.as_object().unwrap()).is_err(), "{bad}");
            assert_eq!(p, before, "{bad}");
        }
    }

    fn run(m: &mut EditMesh, sel: &Selection, kind: OpKind) -> ModelResult<OpOutput> {
        let before = m.clone();
        let r = apply_kind(kind, &OpParams::default(), m, sel);
        if r.is_err() {
            assert_eq!(*m, before, "{kind:?}: erro tem de deixar a malha intacta");
        }
        r
    }

    #[test]
    fn extrude_inset_bevel_segments_loopcut_bisect() {
        let mut m = primitives::cube(2.0);
        let out = { let sel = Selection::from_faces(&m, [0u32]); run(&mut m, &sel, OpKind::Extrude) }.unwrap();
        assert_eq!(out.selection.faces.len(), 1);
        assert_eq!((m.vert_count(), m.face_count()), (12, 10));
        assert!(Topology::build(&m).is_closed_manifold());
        let mut m = primitives::cube(2.0);
        let _ = { let sel = Selection::from_faces(&m, [2u32]); run(&mut m, &sel, OpKind::Inset) }.unwrap();
        m.validate().unwrap();
        // chanfro com 3 segmentos e perfil
        let mut m = primitives::cube(2.0);
        let all = Selection::select_all(&m).flush(&m, SelectMode::Edge);
        let p = OpParams { segments: 3, width: 0.2, ..OpParams::default() };
        apply_kind(OpKind::Bevel, &p, &mut m, &all).unwrap();
        m.validate().unwrap();
        assert!(m.face_count() > 6 + 12 * 3 - 1, "3 segmentos por aresta: {}", m.face_count());
        // loop cut com 2 cortes numa esfera quad
        let mut m = primitives::quad_sphere(1.0, 3);
        let f0 = m.face_count();
        let e = Topology::build(&m).edges[0].verts;
        let p = OpParams { cuts: 2, ..OpParams::default() };
        apply_kind(OpKind::LoopCut, &p, &mut m, &Selection::from_edges([e])).unwrap();
        assert!(m.face_count() > f0);
        // bisect no plano Y = 0
        let mut m = primitives::quad_sphere(1.0, 3);
        apply_kind(OpKind::Bisect, &OpParams { fill: true, ..OpParams::default() }, &mut m, &Selection::default()).unwrap();
        m.validate().unwrap();
    }

    #[test]
    fn delete_fill_triangulate_flip_smooth_subdivide_and_attributes() {
        let mut m = primitives::cube(2.0);
        let _ = { let sel = Selection::from_faces(&m, [0u32]); run(&mut m, &sel, OpKind::DeleteOnlyFaces) }.unwrap();
        assert_eq!(m.face_count(), 5);
        let boundary: Vec<_> = Topology::build(&m).edges.iter().filter(|e| e.faces.len() == 1).map(|e| e.verts).collect();
        assert_eq!(boundary.len(), 4);
        run(&mut m, &Selection::from_edges(boundary), OpKind::FillHole).unwrap();
        assert!(Topology::build(&m).is_closed_manifold());
        let all = Selection::select_all(&m);
        run(&mut m, &all, OpKind::Triangulate).unwrap();
        assert_eq!(m.face_count(), 12);
        let all = Selection::select_all(&m);
        run(&mut m, &all, OpKind::TrisToQuads).unwrap();
        assert_eq!(m.face_count(), 6);
        let all = Selection::select_all(&m);
        run(&mut m, &all, OpKind::FlipNormals).unwrap();
        run(&mut m, &all, OpKind::RecalcOutside).unwrap();
        for f in 0..m.face_count() as u32 {
            assert!(m.face_normal(f).dot(m.face_center(f) - m.centroid()) > 0.0);
        }
        let edges = Selection::select_all(&m).flush(&m, SelectMode::Edge);
        run(&mut m, &edges, OpKind::MarkSeam).unwrap();
        assert!(m.edge_attrs.values().all(|e| e.seam));
        run(&mut m, &edges, OpKind::ClearSeam).unwrap();
        assert!(m.edge_attrs.values().all(|e| !e.seam));
        let all = Selection::select_all(&m);
        run(&mut m, &all, OpKind::Subdivide).unwrap();
        assert_eq!(m.face_count(), 24);
        let h = m.content_hash();
        let all = Selection::select_all(&m);
        run(&mut m, &all, OpKind::Smooth).unwrap();
        assert_ne!(h, m.content_hash());
        m.validate().unwrap();
    }

    #[test]
    fn merge_dissolve_and_spin_free_ops_do_not_corrupt() {
        let mut m = primitives::cube(2.0);
        let all = Selection::select_all(&m);
        run(&mut m, &all, OpKind::MergeByDistance).unwrap();
        assert_eq!(m.vert_count(), 8);
        let mut g = primitives::grid(3, 3, 2.0, 2.0);
        let interior: Vec<_> = Topology::build(&g).edges.iter().filter(|e| e.faces.len() == 2).map(|e| e.verts).collect();
        run(&mut g, &Selection::from_edges(interior), OpKind::DissolveEdges).unwrap();
        assert_eq!(g.face_count(), 1);
    }

    #[test]
    fn invalid_selection_errors_are_atomic() {
        let mut m = primitives::cube(2.0);
        for kind in [OpKind::DeleteFaces, OpKind::Bridge, OpKind::FillHole, OpKind::DissolveFaces, OpKind::Bevel, OpKind::LoopCut] {
            assert!(run(&mut m, &Selection::default(), kind).is_err(), "{kind:?} sem seleção deveria falhar");
        }
    }
}
