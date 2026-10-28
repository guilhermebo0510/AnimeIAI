//! Cinemática inversa de dois ossos (rigging 2D, auditoria 4.6d): ombro → cotovelo → mão, quadril → joelho → pé.
//! Analítica (lei dos cossenos), sem iteração; alvo fora do alcance estica a corrente na direção dele.

/// Ângulos absolutos (radianos, no sistema da tela: y para baixo, ângulo crescendo no sentido horário) do osso de
/// cima e do de baixo para a ponta da corrente chegar em `target`. `root` é o pivô do osso de cima, `l1` e `l2` os
/// comprimentos. `bend_positive`: de que lado a articulação dobra (o cotovelo de um lado ou do outro da reta
/// ombro-alvo).
pub fn two_bone(root: [f32; 2], l1: f32, l2: f32, target: [f32; 2], bend_positive: bool) -> (f32, f32) {
    let (dx, dy) = (target[0] - root[0], target[1] - root[1]);
    let d = dx.hypot(dy).max(1e-6);
    let base = dy.atan2(dx);
    let reach = (l1 + l2).max(1e-6);
    let near = (l1 - l2).abs();
    // fora do alcance: esticado; perto demais: dobrado ao máximo
    let dd = d.clamp(near + 1e-4, reach - 1e-4);
    // ângulo no ombro entre a reta ombro-alvo e o osso de cima
    let cos_a = ((l1 * l1 + dd * dd - l2 * l2) / (2.0 * l1 * dd)).clamp(-1.0, 1.0);
    let a = cos_a.acos();
    let s = if bend_positive { 1.0 } else { -1.0 };
    let a1 = base - s * a;
    let elbow = [root[0] + l1 * a1.cos(), root[1] + l1 * a1.sin()];
    // o osso de baixo aponta do cotovelo para o alvo (ou para onde a corrente consegue chegar)
    let tgt = if d > reach - 1e-4 || d < near + 1e-4 { [root[0] + dd * base.cos(), root[1] + dd * base.sin()] } else { target };
    let a2 = (tgt[1] - elbow[1]).atan2(tgt[0] - elbow[0]);
    (a1, a2)
}

/// Lado em que a corrente dobra agora (para manter o mesmo lado ao arrastar a ponta): o cotovelo à esquerda ou à
/// direita da reta ombro-ponta.
pub fn bend_side(root: [f32; 2], elbow: [f32; 2], tip: [f32; 2]) -> bool {
    let (ax, ay) = (tip[0] - root[0], tip[1] - root[1]);
    let (bx, by) = (elbow[0] - root[0], elbow[1] - root[1]);
    // cruzado positivo = cotovelo do lado em que `two_bone(.., true)` o põe
    ax * by - ay * bx < 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(root: [f32; 2], l1: f32, l2: f32, a1: f32, a2: f32) -> [f32; 2] {
        let e = [root[0] + l1 * a1.cos(), root[1] + l1 * a1.sin()];
        [e[0] + l2 * a2.cos(), e[1] + l2 * a2.sin()]
    }

    #[test]
    fn reaches_targets_inside_the_range_on_both_sides() {
        let root = [100.0, 100.0];
        for target in [[180.0, 140.0], [60.0, 170.0], [100.0, 40.0], [150.0, 100.0]] {
            for side in [true, false] {
                let (a1, a2) = two_bone(root, 60.0, 50.0, target, side);
                let t = tip(root, 60.0, 50.0, a1, a2);
                assert!((t[0] - target[0]).abs() < 1e-3 && (t[1] - target[1]).abs() < 1e-3, "{target:?} {side}: {t:?}");
                let e = [root[0] + 60.0 * a1.cos(), root[1] + 60.0 * a1.sin()];
                assert_eq!(bend_side(root, e, target), side, "lado do cotovelo");
            }
        }
    }

    #[test]
    fn out_of_reach_stretches_toward_the_target() {
        let (a1, a2) = two_bone([0.0, 0.0], 30.0, 20.0, [200.0, 0.0], true);
        let t = tip([0.0, 0.0], 30.0, 20.0, a1, a2);
        assert!((t[0] - 50.0).abs() < 0.05 && t[1].abs() < 0.5, "{t:?}");
    }
}
