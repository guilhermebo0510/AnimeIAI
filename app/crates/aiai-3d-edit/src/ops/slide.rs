//! Deslizar aresta/vértice (escrito no AnimeIAI; o ANIGO não tem): cada vértice escolhido anda ao longo de uma aresta que
//! sai da seleção, sem mudar a topologia. Para um loop de arestas, cada vértice tem um "trilho" de cada lado; `factor` em
//! `-1..=1` escolhe o lado (sinal) e o quanto anda (0 = parado, 1 = no vizinho). O lado positivo é o vizinho que mais
//! aponta para `dir` (a direção do arrasto na tela, levada para o espaço do objeto).

use crate::error::{ModelError, ModelResult};
use crate::mesh::EditMesh;
use crate::ops::OpOutput;
use crate::select::Selection;
use crate::topology::Topology;
use glam::Vec3;
use std::collections::BTreeSet;

/// Trilho de um vértice: posição de partida e os dois vizinhos para onde ele pode deslizar.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideRail {
    pub vert: u32,
    pub origin: Vec3,
    pub plus: Option<Vec3>,
    pub minus: Option<Vec3>,
}

/// Trilhos dos `verts` (vizinhos fora da seleção; sem nenhum, qualquer vizinho).
pub fn slide_rails(mesh: &EditMesh, verts: &[u32], dir: Vec3) -> ModelResult<Vec<SlideRail>> {
    if verts.is_empty() {
        return Err(ModelError::EmptySelection("escolha as arestas ou vértices a deslizar"));
    }
    let n = mesh.vert_count();
    if let Some(&v) = verts.iter().find(|&&v| v as usize >= n) {
        return Err(ModelError::VertexOutOfRange(v, n));
    }
    let dir = dir.normalize_or_zero();
    let topo = Topology::build(mesh);
    let sel: BTreeSet<u32> = verts.iter().copied().collect();
    let mut rails = Vec::with_capacity(sel.len());
    for &v in &sel {
        let o = mesh.co(v);
        let all = topo.vert_neighbors(v);
        let outside: Vec<u32> = all.iter().copied().filter(|w| !sel.contains(w)).collect();
        let cands = if outside.is_empty() { all } else { outside };
        let score = |w: u32| (mesh.co(w) - o).normalize_or_zero().dot(dir);
        let best = cands.iter().copied().max_by(|a, b| score(*a).total_cmp(&score(*b)));
        let worst = cands.iter().copied().min_by(|a, b| score(*a).total_cmp(&score(*b)));
        let plus = best.map(|w| mesh.co(w));
        let minus = worst.filter(|w| Some(*w) != best).map(|w| mesh.co(w));
        rails.push(SlideRail { vert: v, origin: o, plus, minus });
    }
    Ok(rails)
}

/// Põe cada vértice no seu trilho: `factor` > 0 vai para `plus`, < 0 para `minus` (limitado a ±1).
pub fn apply_slide(mesh: &mut EditMesh, rails: &[SlideRail], factor: f32) -> ModelResult<OpOutput> {
    if !factor.is_finite() {
        return Err(ModelError::InvalidParameter("fator de deslizar não finito".into()));
    }
    let f = factor.clamp(-1.0, 1.0);
    let n = mesh.vert_count();
    if let Some(r) = rails.iter().find(|r| r.vert as usize >= n) {
        return Err(ModelError::VertexOutOfRange(r.vert, n));
    }
    for r in rails {
        let target = if f >= 0.0 { r.plus } else { r.minus };
        let p = match target {
            Some(t) => r.origin.lerp(t, f.abs()),
            None => r.origin,
        };
        mesh.verts[r.vert as usize].co = p;
    }
    Ok(OpOutput { selection: Selection::from_verts(rails.iter().map(|r| r.vert)), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{catalog, loops};
    use crate::primitives;

    #[test]
    fn loop_slides_along_its_rails_without_changing_topology() {
        let mut m = primitives::cube(2.0);
        // um loop cut cria o anel a deslizar
        let e = Topology::build(&m).edges[0].verts;
        let out = loops::loop_cut(&mut m, e, 1, 0.0).unwrap();
        let o: OpOutput = out.into();
        let verts: Vec<u32> = o.created_verts.clone();
        assert!(!verts.is_empty());
        let (vc, fc) = (m.vert_count(), m.face_count());
        // direção: ao longo do eixo que o corte atravessa
        let any = verts[0];
        let rails = slide_rails(&m, &verts, Vec3::ONE).unwrap();
        let r0 = rails.iter().find(|r| r.vert == any).unwrap();
        assert!(r0.plus.is_some() && r0.minus.is_some(), "anel tem vizinho dos dois lados");
        apply_slide(&mut m, &rails, 0.5).unwrap();
        assert_eq!((m.vert_count(), m.face_count()), (vc, fc));
        m.validate().unwrap();
        let moved = m.co(any);
        let want = r0.origin.lerp(r0.plus.unwrap(), 0.5);
        assert!((moved - want).length() < 1e-5);
        // fator 0 volta ao lugar; o outro lado anda para minus
        apply_slide(&mut m, &rails, 0.0).unwrap();
        assert!((m.co(any) - r0.origin).length() < 1e-6);
        apply_slide(&mut m, &rails, -1.0).unwrap();
        assert!((m.co(any) - r0.minus.unwrap()).length() < 1e-5);
        let _ = catalog::OpKind::ALL;
    }

    #[test]
    fn empty_or_bad_input_is_refused() {
        let mut m = primitives::cube(1.0);
        assert!(slide_rails(&m, &[], Vec3::X).is_err());
        assert!(slide_rails(&m, &[99], Vec3::X).is_err());
        let rails = slide_rails(&m, &[0], Vec3::X).unwrap();
        assert!(apply_slide(&mut m, &rails, f32::NAN).is_err());
    }
}
