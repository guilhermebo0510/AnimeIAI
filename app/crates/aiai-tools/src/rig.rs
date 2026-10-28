//! Rigging 2D nativo (auditoria 4.6d): hierarquia pai-filho entre camadas (pegbar: o filho anda, gira e escala com o
//! pai em volta do pivô de cada um), a ponta do osso de cada camada e a cinemática inversa de dois ossos (arrastar a
//! mão e o ombro e o cotovelo giram para ela chegar lá), gravada como chaves de rotação.
//!
//! O osso de uma camada vai do pivô dela (centro do canvas + âncora) até a ponta (`params.bone_tip`, px do canvas em
//! repouso); a corrente do IK é a camada (osso de baixo) e o pai dela (osso de cima).

use crate::context::*;
use aiai_core::*;
use aiai_paint::Affine;
use aiai_render::compose::{cut_canvas, layer_motion_affine, layer_parent, layer_prop, parent_chain_affine};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RigRequest {
    /// Pai (pegbar) da camada; `parent: null` solta.
    Parent { layer: Id, parent: Option<Id> },
    /// Ponta do osso da camada (px do canvas, na pose de repouso); `tip: null` tira.
    BoneTip { layer: Id, tip: Option<[f32; 2]> },
    /// Leva a ponta do osso de `layer` até `target` (px do canvas) girando o pai e a camada.
    Ik {
        cut: Id,
        layer: Id,
        target: [f32; 2],
        #[serde(default)]
        frame: u32,
        /// Lado da dobra; sem valor, mantém o lado de agora.
        #[serde(default)]
        bend: Option<bool>,
        /// Grava chaves de rotação no quadro (padrão); `false` muda a rotação fixa (só sem chaves de rotação).
        #[serde(default = "yes")]
        keyframe: bool,
    },
}

fn yes() -> bool {
    true
}

fn layer_node(p: &Project, id: &Id) -> ToolResult<LayerData> {
    Ok(layer_data(p, id)?.clone())
}

fn with_param(mut l: LayerData, key: &str, v: Option<Value>) -> LayerData {
    if !l.params.is_object() {
        l.params = json!({});
    }
    let m = l.params.as_object_mut().expect("objeto");
    match v {
        Some(v) => {
            m.insert(key.into(), v);
        }
        None => {
            m.remove(key);
        }
    }
    l
}

/// Pivô da camada em repouso (centro do canvas + âncora no quadro).
fn pivot(l: &LayerData, frame: u32, w: u32, h: u32) -> [f32; 2] {
    [w as f32 / 2.0 + layer_prop(l, "anchor_x", frame, l.anchor[0]), h as f32 / 2.0 + layer_prop(l, "anchor_y", frame, l.anchor[1])]
}

fn apply(a: &Affine, p: [f32; 2]) -> [f32; 2] {
    let (x, y) = a.apply(p[0], p[1]);
    [x, y]
}

fn compose(m2: &Affine, m1: &Affine) -> Affine {
    let (a, b) = (m2.0, m1.0);
    Affine([
        a[0] * b[0] + a[1] * b[3],
        a[0] * b[1] + a[1] * b[4],
        a[0] * b[2] + a[1] * b[5] + a[2],
        a[3] * b[0] + a[4] * b[3],
        a[3] * b[1] + a[4] * b[4],
        a[3] * b[2] + a[4] * b[5] + a[5],
    ])
}

/// Põe (ou troca) a chave de `prop` no quadro.
fn set_key(l: &mut LayerData, prop: &str, frame: u32, value: f32) {
    let track = l.keys.0.entry(prop.to_string()).or_default();
    match track.iter_mut().find(|k| k.frame == frame) {
        Some(k) => k.value = value,
        None => {
            track.push(Key { frame, value, ease: Default::default() });
            track.sort_by_key(|k| k.frame);
        }
    }
}

/// Onde estão o ombro (pivô do pai), o cotovelo (pivô da camada) e a ponta, na tela, no quadro.
pub fn chain_points(p: &Project, cut: &Id, layer: &Id, frame: u32) -> ToolResult<([f32; 2], [f32; 2], [f32; 2])> {
    let (w, h) = cut_canvas(p, cut_data(p, cut)?);
    let c = layer_data(p, layer)?;
    let pid = layer_parent(c).ok_or_else(|| ToolError::Invalid("a camada não tem pai: o IK precisa de dois ossos (camada + pai)".into()))?;
    let pl = layer_data(p, &pid)?;
    let tip: [f32; 2] = c
        .params
        .get("bone_tip")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| ToolError::Invalid("a camada não tem ponta de osso (bone_tip)".into()))?;
    let g = parent_chain_affine(p, pl, frame, w, h);
    let mp = compose(&g, &layer_motion_affine(pl, frame, w, h));
    let mc = compose(&mp, &layer_motion_affine(c, frame, w, h));
    let root = apply(&mp, pivot(pl, frame, w, h));
    let elbow = apply(&mc, pivot(c, frame, w, h));
    Ok((root, elbow, apply(&mc, tip)))
}

fn wrap_deg(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// O comando (um passo de desfazer) e o resumo.
pub fn rig_command(p: &Project, req: &RigRequest) -> ToolResult<(Command, Value)> {
    match req {
        RigRequest::Parent { layer, parent } => {
            let l = layer_node(p, layer)?;
            if let Some(pid) = parent {
                layer_data(p, pid)?;
                // sem ciclo: o pai não pode descender (pela cadeia de pais) da própria camada
                let mut cur = Some(pid.clone());
                let mut n = 0;
                while let Some(c) = cur {
                    if &c == layer || n > 32 {
                        return Err(ToolError::Invalid("o pai escolhido faria um ciclo (é filho desta camada)".into()));
                    }
                    cur = layer_data(p, &c).ok().and_then(layer_parent);
                    n += 1;
                }
            }
            let l = with_param(l, "parent", parent.as_ref().map(|i| json!(i.0)));
            Ok((Command::ReplaceData { id: layer.clone(), data: NodeKind::Layer(l) }, json!({ "layer": layer, "parent": parent })))
        }
        RigRequest::BoneTip { layer, tip } => {
            if tip.is_some_and(|t| !t.iter().all(|v| v.is_finite())) {
                return Err(ToolError::Invalid("ponta não finita".into()));
            }
            let l = with_param(layer_node(p, layer)?, "bone_tip", tip.map(|t| json!(t)));
            Ok((Command::ReplaceData { id: layer.clone(), data: NodeKind::Layer(l) }, json!({ "layer": layer, "bone_tip": tip })))
        }
        RigRequest::Ik { cut, layer, target, frame, bend, keyframe } => {
            if !target.iter().all(|v| v.is_finite()) {
                return Err(ToolError::Invalid("alvo não finito".into()));
            }
            let (root, elbow, tip) = chain_points(p, cut, layer, *frame)?;
            let l1 = (elbow[0] - root[0]).hypot(elbow[1] - root[1]);
            let l2 = (tip[0] - elbow[0]).hypot(tip[1] - elbow[1]);
            if l1 < 1e-3 || l2 < 1e-3 {
                return Err(ToolError::Invalid("osso de comprimento zero (pivô do pai, pivô da camada e ponta no mesmo lugar)".into()));
            }
            let side = bend.unwrap_or_else(|| aiai_anim::ik::bend_side(root, elbow, tip));
            let (a1, a2) = aiai_anim::ik::two_bone(root, l1, l2, *target, side);
            let c1 = (elbow[1] - root[1]).atan2(elbow[0] - root[0]);
            let c2 = (tip[1] - elbow[1]).atan2(tip[0] - elbow[0]);
            let d1 = wrap_deg((a1 - c1).to_degrees());
            let d2 = wrap_deg((a2 - c2).to_degrees() - d1);
            let mut c = layer_node(p, layer)?;
            let pid = layer_parent(&c).expect("checado em chain_points");
            let mut pl = layer_node(p, &pid)?;
            let rot = |l: &LayerData| layer_prop(l, "rotation", *frame, l.rotation);
            let (np, nc) = (rot(&pl) + d1, rot(&c) + d2);
            for (l, v) in [(&mut pl, np), (&mut c, nc)] {
                if *keyframe {
                    set_key(l, "rotation", *frame, v);
                } else if l.keys.has("rotation") {
                    return Err(ToolError::Invalid("a rotação tem chaves: use keyframe = true".into()));
                } else {
                    l.rotation = v;
                }
            }
            let reach = l1 + l2;
            let dist = (target[0] - root[0]).hypot(target[1] - root[1]);
            let cmd = Command::Batch {
                label: Some("IK".into()),
                commands: vec![Command::ReplaceData { id: pid.clone(), data: NodeKind::Layer(pl) }, Command::ReplaceData { id: layer.clone(), data: NodeKind::Layer(c) }],
            };
            Ok((cmd, json!({ "parent": pid, "parent_rotation": np, "rotation": nc, "reached": dist <= reach + 0.01, "bend": side })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
        s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
    }

    /// Braço: "ombro" (pivô em 300,200, osso até 380,200) e "antebraço" filho (pivô em 380,200, ponta em 450,200).
    fn arm() -> (CoreSession, Id, Id, Id) {
        let mut s = CoreSession::for_tests("rig");
        s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 600, "height": 400}}) }).unwrap();
        let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(24)));
        let mut up = LayerData::new(LayerType::Vector);
        up.anchor = [0.0, 0.0];
        let upper = create(&mut s, Some(&cut), "braço", NodeKind::Layer(up));
        let mut lo = LayerData::new(LayerType::Vector);
        lo.anchor = [80.0, 0.0];
        let lower = create(&mut s, Some(&cut), "antebraço", NodeKind::Layer(lo));
        for r in [RigRequest::Parent { layer: lower.clone(), parent: Some(upper.clone()) }, RigRequest::BoneTip { layer: lower.clone(), tip: Some([450.0, 200.0]) }] {
            let (c, _) = rig_command(&s.project, &r).unwrap();
            s.apply(c).unwrap();
        }
        (s, cut, upper, lower)
    }

    #[test]
    fn the_child_follows_the_parent_rotation() {
        let (mut s, cut, upper, lower) = arm();
        let (_, e0, t0) = chain_points(&s.project, &cut, &lower, 0).unwrap();
        assert!((e0[0] - 380.0).abs() < 1e-3 && (t0[0] - 450.0).abs() < 1e-3);
        // gira o braço 90°: o cotovelo e a mão giram em volta do ombro (300,200)
        let mut l = layer_data(&s.project, &upper).unwrap().clone();
        l.rotation = 90.0;
        s.apply(Command::ReplaceData { id: upper.clone(), data: NodeKind::Layer(l) }).unwrap();
        let (_, e, t) = chain_points(&s.project, &cut, &lower, 0).unwrap();
        assert!((e[0] - 300.0).abs() < 1e-3 && (e[1] - 280.0).abs() < 1e-3, "{e:?}");
        assert!((t[0] - 300.0).abs() < 1e-3 && (t[1] - 350.0).abs() < 1e-3, "{t:?}");
    }

    #[test]
    fn ik_puts_the_hand_on_the_target_and_keeps_the_bend() {
        let (mut s, cut, _upper, lower) = arm();
        for (frame, target) in [(0u32, [380.0f32, 300.0f32]), (6, [250.0, 260.0]), (12, [340.0, 120.0])] {
            let (c, v) = rig_command(&s.project, &RigRequest::Ik { cut: cut.clone(), layer: lower.clone(), target, frame, bend: Some(true), keyframe: true }).unwrap();
            assert_eq!(v["reached"], true);
            s.apply(c).unwrap();
            let (root, _, t) = chain_points(&s.project, &cut, &lower, frame).unwrap();
            assert!((t[0] - target[0]).abs() < 0.05 && (t[1] - target[1]).abs() < 0.05, "quadro {frame}: mão em {t:?}");
            assert!((root[0] - 300.0).abs() < 1e-3, "o ombro não sai do lugar");
        }
        // um passo de desfazer por IK, e as chaves ficaram nos quadros
        let l = layer_data(&s.project, &lower).unwrap();
        assert_eq!(l.keys.0["rotation"].iter().map(|k| k.frame).collect::<Vec<_>>(), vec![0, 6, 12]);
        // o mesmo lado da dobra sem dizer qual
        let (c, v) = rig_command(&s.project, &RigRequest::Ik { cut: cut.clone(), layer: lower.clone(), target: [390.0, 290.0], frame: 0, bend: None, keyframe: true }).unwrap();
        assert_eq!(v["bend"], true);
        s.apply(c).unwrap();
    }

    #[test]
    fn a_parent_cycle_is_refused() {
        let (s, _cut, upper, lower) = arm();
        assert!(rig_command(&s.project, &RigRequest::Parent { layer: upper, parent: Some(lower) }).is_err());
    }
}
