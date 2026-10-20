//! A base gera uma malha fechada, simétrica, com esqueleto humanoide e pesos que somam 1 (AnimeIAI: só o que o Manequim
//! usa; os testes completos ficam no ANIGO).

use aiai_3d_charbase::{generate, Gender};

#[test]
fn base_generates_skin_skeleton_and_weights() {
    let t0 = std::time::Instant::now();
    let c = generate(Gender::Blend(0.5));
    let topo = c.topology();
    eprintln!("charbase: {} vértices, {} quads, {} ossos em {:?}", c.positions.len(), topo.faces.len(), c.skeleton.len(), t0.elapsed());
    assert_eq!(c.positions.len(), topo.n_verts);
    assert!(c.skeleton.index("hips").is_some() && c.skeleton.index("leftHand").is_some());
    assert_eq!(topo.weights.len(), c.positions.len());
    for w in &topo.weights {
        let s: f32 = w.iter().map(|(_, x)| x).sum();
        assert!((s - 1.0).abs() < 1e-3, "soma {s}");
    }
    // altura de gente (m) e pés no chão
    let (lo, hi) = c.positions.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
    eprintln!("altura {lo}..{hi}");
    assert!(hi - lo > 1.2 && hi - lo < 2.2);
}
