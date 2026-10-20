//! Pose de modelos com esqueleto (S24 3D, fases 8 e 9): avaliação da [`Pose3d`] sobre a [`Skin`] do modelo, skinning
//! linear na CPU (a malha posada vai para os dois renderizadores como um modelo rígido), girar um osso por uma rotação de
//! mundo (anel do gizmo), IK de dois ossos para mãos e pés (lei dos cossenos, como o `solve_limb` do `anigo-ik`) e as
//! poses prontas do Manequim (em pé, andando, correndo, sentado, apontando).
//!
//! Convenção (a do Posing do ANIGO): a rotação de cada osso é um delta sobre o repouso, lido nos eixos do mundo quando os
//! pais estão em repouso. Com a rotação de repouso global `Rr` de cada osso:
//! `G_rot(i) = G_rot(pai) · Rr(pai)⁻¹ · D(i) · Rr(i)` e `G_pos(i) = G_pos(pai) + G_rot(pai) · Rr(pai)⁻¹ · (r(i) − r(pai))`.
//! No Manequim (repouso com rotação identidade) isso é `G_rot(i) = G_rot(pai) · D(i)`.

use crate::model::{Model, Skin};
use aiai_core::BlobHash;
use aiai_core::model3d::Pose3d;
use glam::{Mat3, Mat4, Quat, Vec3};
use std::sync::{Arc, Mutex, OnceLock};

/// Pose avaliada de um osso: rotação e posição globais (espaço do modelo).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoneXf {
    pub rot: Quat,
    pub pos: Vec3,
}

fn rest_rot(skin: &Skin, i: usize) -> Quat {
    Quat::from_array(skin.bones[i].rest_rot).normalize()
}

fn delta(pose: &Pose3d, name: &str) -> Quat {
    Quat::from_array(pose.rotation(name)).normalize()
}

/// Transformações globais de todos os ossos na pose.
pub fn globals(skin: &Skin, pose: &Pose3d) -> Vec<BoneXf> {
    let mut out: Vec<BoneXf> = Vec::with_capacity(skin.bones.len());
    for (i, b) in skin.bones.iter().enumerate() {
        let d = delta(pose, &b.name);
        let rr = rest_rot(skin, i);
        let rp = Vec3::from(b.rest_pos);
        let x = match b.parent.filter(|&p| p < i) {
            Some(p) => {
                let pg = out[p];
                let a = pg.rot * rest_rot(skin, p).inverse();
                BoneXf { rot: (a * d * rr).normalize(), pos: pg.pos + a * (rp - Vec3::from(skin.bones[p].rest_pos)) }
            }
            None => BoneXf { rot: (d * rr).normalize(), pos: rp + Vec3::from(pose.hips) },
        };
        out.push(x);
    }
    out
}

/// Rotação "de chegada" do pai (`G_rot(pai) · Rr(pai)⁻¹`): converte uma rotação de mundo no delta do osso.
fn parent_frame(skin: &Skin, g: &[BoneXf], i: usize) -> Quat {
    match skin.bones[i].parent {
        Some(p) => (g[p].rot * rest_rot(skin, p).inverse()).normalize(),
        None => Quat::IDENTITY,
    }
}

/// Matriz de skinning de cada osso: leva o vértice do repouso à pose.
pub fn skin_matrices(skin: &Skin, g: &[BoneXf]) -> Vec<Mat4> {
    skin.bones
        .iter()
        .enumerate()
        .map(|(i, b)| Mat4::from_rotation_translation(g[i].rot * rest_rot(skin, i).inverse(), g[i].pos) * Mat4::from_translation(-Vec3::from(b.rest_pos)))
        .collect()
}

/// Modelo posado (skinning linear na CPU). Sem pele, devolve uma cópia igual.
pub fn apply(model: &Model, pose: &Pose3d) -> Model {
    let mut out = model.clone();
    let Some(skin) = &model.skin else { return out };
    let g = globals(skin, pose);
    let mats = skin_matrices(skin, &g);
    let rots: Vec<Mat3> = mats.iter().map(|m| Mat3::from_mat4(*m)).collect();
    for (p, ps) in out.parts.iter_mut().zip(&skin.parts) {
        if ps.joints.len() != p.positions.len() {
            continue;
        }
        for v in 0..p.positions.len() {
            let (j, w) = (ps.joints[v], ps.weights[v]);
            let sum: f32 = w.iter().sum();
            if sum <= 1e-6 {
                continue;
            }
            let (q, n) = (Vec3::from(p.positions[v]), Vec3::from(p.normals[v]));
            let mut pq = Vec3::ZERO;
            let mut pn = Vec3::ZERO;
            for k in 0..4 {
                if w[k] <= 0.0 {
                    continue;
                }
                let b = j[k] as usize;
                pq += mats[b].transform_point3(q) * w[k];
                pn += rots[b] * n * w[k];
            }
            p.positions[v] = (pq / sum).to_array();
            p.normals[v] = pn.normalize_or(n).to_array();
        }
    }
    out.refresh_info();
    out
}

/// Chave de cena/cache de um modelo posado: o hash do blob quando não há pose; senão, hash do blob + pose.
pub fn posed_key(hash: &BlobHash, pose: Option<&Pose3d>) -> BlobHash {
    match pose {
        None => hash.clone(),
        Some(p) => {
            let mut v = hash.0.as_bytes().to_vec();
            v.extend_from_slice(serde_json::to_string(p).unwrap_or_default().as_bytes());
            BlobHash::of(&v)
        }
    }
}

static POSED: OnceLock<Mutex<Vec<(BlobHash, Arc<Model>)>>> = OnceLock::new();
const POSED_CACHE: usize = 6;

/// Modelo na pose (com cache das últimas poses): o mesmo `Arc` quando não há pose ou o modelo não tem pele.
pub fn posed(hash: &BlobHash, model: &Arc<Model>, pose: Option<&Pose3d>) -> Arc<Model> {
    let Some(pose) = pose.filter(|_| model.skin.is_some()) else { return Arc::clone(model) };
    let key = posed_key(hash, Some(pose));
    let cache = POSED.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(c) = cache.lock() {
        if let Some((_, m)) = c.iter().find(|(k, _)| *k == key) {
            return Arc::clone(m);
        }
    }
    let m = Arc::new(apply(model, pose));
    if let Ok(mut c) = cache.lock() {
        c.push((key, Arc::clone(&m)));
        while c.len() > POSED_CACHE {
            c.remove(0);
        }
    }
    m
}

/// Gira o osso `bone` por uma rotação de mundo `w` em volta da própria junta (o anel do gizmo): `D' = A⁻¹ · w · A · D`.
pub fn rotate_bone_world(skin: &Skin, pose: &Pose3d, bone: usize, w: Quat) -> Pose3d {
    let g = globals(skin, pose);
    let a = parent_frame(skin, &g, bone);
    let name = &skin.bones[bone].name;
    let d = delta(pose, name);
    let mut out = pose.clone();
    out.set_rotation(name, (a.inverse() * w * a * d).normalize().to_array());
    out
}

/// Define a rotação global do osso (conserva os pais): `D = A⁻¹ · G · Rr⁻¹`.
pub fn set_bone_world_rotation(skin: &Skin, pose: &Pose3d, bone: usize, world: Quat) -> Pose3d {
    let g = globals(skin, pose);
    let a = parent_frame(skin, &g, bone);
    let mut out = pose.clone();
    out.set_rotation(&skin.bones[bone].name, (a.inverse() * world * rest_rot(skin, bone).inverse()).normalize().to_array());
    out
}

/// Rotação mínima que leva `a` a `b`.
fn arc(a: Vec3, b: Vec3) -> Quat {
    let (a, b) = (a.normalize_or_zero(), b.normalize_or_zero());
    if a == Vec3::ZERO || b == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    let d = a.dot(b);
    if d > 1.0 - 1e-7 {
        return Quat::IDENTITY;
    }
    if d < -1.0 + 1e-7 {
        let axis = if a.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }.cross(a).normalize();
        return Quat::from_axis_angle(axis, std::f32::consts::PI);
    }
    Quat::from_rotation_arc(a, b)
}

/// Cadeia de IK de um efetuador (mão ou pé): [raiz, meio, ponta] (o osso, o pai e o avô).
pub fn ik_chain(skin: &Skin, end: usize) -> Option<[usize; 3]> {
    let mid = skin.bones[end].parent?;
    let root = skin.bones[mid].parent?;
    Some([root, mid, end])
}

/// Efetuadores com IK no Manequim (nomes humanoides do VRM).
pub const IK_ENDS: [&str; 4] = ["leftHand", "rightHand", "leftFoot", "rightFoot"];

/// IK de dois ossos: leva a junta `end` até `target` (espaço do modelo), mantendo o plano de dobra atual (ou o natural:
/// joelho para a frente, cotovelo para trás) e a orientação de mundo da ponta. Os comprimentos não mudam; alvo fora do
/// alcance estica o membro na direção dele. `None` se o osso não tem dois ancestrais.
pub fn solve_ik(skin: &Skin, pose: &Pose3d, end: usize, target: Vec3) -> Option<Pose3d> {
    let [r, m, t] = ik_chain(skin, end)?;
    let g0 = globals(skin, pose);
    let (a, b, c) = (g0[r].pos, g0[m].pos, g0[t].pos);
    let end_rot = g0[t].rot;
    let (l1, l2) = ((b - a).length(), (c - b).length());
    if l1 < 1e-6 || l2 < 1e-6 {
        return None;
    }
    let to = target - a;
    let d = to.length().clamp((l1 - l2).abs() + 1e-4, l1 + l2 - 1e-4);
    let u = to.normalize_or(c - a);
    // lado da dobra: o cotovelo/joelho atual fora da reta raiz→alvo; reto, a dobra natural
    let mut v = (b - a) - u * (b - a).dot(u);
    if v.length() < 1e-4 {
        let leg = skin.bones[end].name.to_lowercase().contains("foot");
        let hint = if leg { Vec3::Z } else { -Vec3::Z };
        v = hint - u * hint.dot(u);
    }
    let v = v.normalize_or(Vec3::Y);
    let cos_a = ((l1 * l1 + d * d - l2 * l2) / (2.0 * l1 * d)).clamp(-1.0, 1.0);
    let b_new = a + u * (l1 * cos_a) + v * (l1 * (1.0 - cos_a * cos_a).max(0.0).sqrt());
    let mut out = rotate_bone_world(skin, pose, r, arc(b - a, b_new - a));
    let g1 = globals(skin, &out);
    let target_c = a + u * d;
    out = rotate_bone_world(skin, &out, m, arc(g1[t].pos - g1[m].pos, target_c - g1[m].pos));
    // a mão/pé conserva a orientação de mundo
    out = set_bone_world_rotation(skin, &out, t, end_rot);
    Some(out)
}

/// Juntas que a interface mostra para posar (os ossos humanoides principais que existirem no esqueleto).
pub const POSE_JOINTS: [&str; 22] = [
    "hips",
    "spine",
    "chest",
    "upperChest",
    "neck",
    "head",
    "leftShoulder",
    "leftUpperArm",
    "leftLowerArm",
    "leftHand",
    "rightShoulder",
    "rightUpperArm",
    "rightLowerArm",
    "rightHand",
    "leftUpperLeg",
    "leftLowerLeg",
    "leftFoot",
    "leftToes",
    "rightUpperLeg",
    "rightLowerLeg",
    "rightFoot",
    "rightToes",
];

/// Ossos posáveis do modelo: os de [`POSE_JOINTS`] que existem; num esqueleto sem nomes humanoides, todos.
pub fn pose_bones(skin: &Skin) -> Vec<usize> {
    let named: Vec<usize> = POSE_JOINTS.iter().filter_map(|n| skin.index(n)).collect();
    if named.len() >= 6 { named } else { (0..skin.bones.len()).collect() }
}

// ----------------------------------------------------------------------------------------------------- poses prontas

/// Poses prontas (chaves estáveis; os nomes na interface vêm do i18n `m3d.pose.preset.<chave>`).
pub const PRESETS: [&str; 6] = ["stand", "walk", "run", "sit", "point", "tpose"];

fn qa(axis: Vec3, deg: f32) -> Quat {
    Quat::from_axis_angle(axis, deg.to_radians())
}

fn set(p: &mut Pose3d, bone: &str, q: Quat) {
    p.set_rotation(bone, q.normalize().to_array());
}

/// Braço de lado: o braço em T (esquerdo em +X) baixado `down` graus e balançado `swing` graus (+ = para a frente),
/// cotovelo dobrado `elbow` graus para a frente.
fn arm(p: &mut Pose3d, left: bool, down: f32, swing: f32, elbow: f32) {
    let (side, s) = if left { ("left", -1.0) } else { ("right", 1.0) };
    // baixar: em volta de Z (esquerdo: -), balanço em volta de X (para a frente: -)
    set(p, &format!("{side}UpperArm"), qa(Vec3::X, -swing) * qa(Vec3::Z, s * down));
    // cotovelo: o antebraço (em T ao longo de X) vai para +Z: em volta de Y (esquerdo: -)
    set(p, &format!("{side}LowerArm"), qa(Vec3::Y, s * elbow));
}

/// Perna: coxa balançada `thigh` graus (+ = para a frente), joelho dobrado `knee` graus (canela para trás).
fn leg(p: &mut Pose3d, left: bool, thigh: f32, knee: f32) {
    let side = if left { "left" } else { "right" };
    set(p, &format!("{side}UpperLeg"), qa(Vec3::X, -thigh));
    set(p, &format!("{side}LowerLeg"), qa(Vec3::X, knee));
}

/// Dedos fechados (punho) de uma mão, menos os de `except`.
fn fist(p: &mut Pose3d, left: bool, except: &[&str]) {
    let (side, s) = if left { ("left", -1.0) } else { ("right", 1.0) };
    for f in ["Index", "Middle", "Ring", "Little"] {
        if except.contains(&f) {
            continue;
        }
        for (seg, deg) in [("Proximal", 70.0), ("Intermediate", 85.0), ("Distal", 55.0)] {
            set(p, &format!("{side}{f}{seg}"), qa(Vec3::Z, s * deg));
        }
    }
    if !except.contains(&"Thumb") {
        set(p, &format!("{side}ThumbProximal"), qa(Vec3::Y, s * 25.0));
        set(p, &format!("{side}ThumbDistal"), qa(Vec3::Y, s * 35.0));
    }
}

fn dist(skin: &Skin, a: &str, b: &str) -> Option<f32> {
    Some((Vec3::from(skin.bones[skin.index(a)?].rest_pos) - Vec3::from(skin.bones[skin.index(b)?].rest_pos)).length())
}

/// Pose pronta para o esqueleto (o quadril desce pelo comprimento das pernas dele). `None` = chave desconhecida.
pub fn preset(name: &str, skin: &Skin) -> Option<Pose3d> {
    let mut p = Pose3d::default();
    let thigh = dist(skin, "leftUpperLeg", "leftLowerLeg").unwrap_or(0.42);
    match name {
        "tpose" => {}
        "stand" => {
            arm(&mut p, true, 72.0, 0.0, 12.0);
            arm(&mut p, false, 72.0, 0.0, 12.0);
        }
        "walk" => {
            arm(&mut p, true, 74.0, -22.0, 15.0);
            arm(&mut p, false, 74.0, 26.0, 22.0);
            leg(&mut p, true, 26.0, 8.0);
            leg(&mut p, false, -18.0, 24.0);
            set(&mut p, "spine", qa(Vec3::Y, 4.0));
            p.hips = [0.0, -0.025 * thigh / 0.42, 0.0];
        }
        "run" => {
            arm(&mut p, true, 70.0, -40.0, 85.0);
            arm(&mut p, false, 70.0, 45.0, 90.0);
            leg(&mut p, true, 55.0, 45.0);
            leg(&mut p, false, -30.0, 85.0);
            set(&mut p, "spine", qa(Vec3::X, 14.0));
            set(&mut p, "head", qa(Vec3::X, -10.0));
            fist(&mut p, true, &[]);
            fist(&mut p, false, &[]);
            p.hips = [0.0, -0.12 * thigh, 0.0];
        }
        "sit" => {
            arm(&mut p, true, 76.0, 28.0, 55.0);
            arm(&mut p, false, 76.0, 28.0, 55.0);
            leg(&mut p, true, 88.0, 88.0);
            leg(&mut p, false, 88.0, 88.0);
            set(&mut p, "spine", qa(Vec3::X, 6.0));
            p.hips = [0.0, -thigh, 0.0];
        }
        "point" => {
            arm(&mut p, true, 72.0, 0.0, 12.0);
            // braço direito em T (ao longo de -X) levado para a frente (+Z) e um pouco acima do ombro
            set(&mut p, "rightUpperArm", qa(Vec3::X, -8.0) * qa(Vec3::Y, 82.0));
            set(&mut p, "rightLowerArm", qa(Vec3::Y, 4.0));
            fist(&mut p, false, &["Index"]);
            set(&mut p, "head", qa(Vec3::Y, -12.0));
            set(&mut p, "chest", qa(Vec3::Y, -8.0));
        }
        _ => return None,
    }
    // só os ossos que o esqueleto tem
    p.rotations.retain(|n, _| skin.index(n).is_some());
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelInfo, Part, PartSkin, SkinBone};

    /// Braço de três ossos ao longo de +X (ombro na origem, cotovelo em 1, mão em 2), com um vértice preso em cada.
    fn arm_model() -> Model {
        let bones = vec![
            SkinBone { name: "leftUpperArm".into(), parent: None, rest_pos: [0.0, 0.0, 0.0], rest_rot: [0.0, 0.0, 0.0, 1.0] },
            SkinBone { name: "leftLowerArm".into(), parent: Some(0), rest_pos: [1.0, 0.0, 0.0], rest_rot: [0.0, 0.0, 0.0, 1.0] },
            SkinBone { name: "leftHand".into(), parent: Some(1), rest_pos: [2.0, 0.0, 0.0], rest_rot: [0.0, 0.0, 0.0, 1.0] },
        ];
        let part = Part {
            name: "braco".into(),
            positions: vec![[0.5, 0.0, 0.0], [1.5, 0.0, 0.0], [2.5, 0.0, 0.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            indices: vec![0, 1, 2],
            color: [1.0; 4],
            material: "m".into(),
        };
        let ps = PartSkin { joints: vec![[0, 0, 0, 0], [1, 0, 0, 0], [2, 0, 0, 0]], weights: vec![[1.0, 0.0, 0.0, 0.0]; 3] };
        let mut m = Model { parts: vec![part], info: ModelInfo::default(), skin: Some(Skin { bones, parts: vec![ps] }) };
        m.refresh_info();
        m
    }

    fn close(a: Vec3, b: [f32; 3]) -> bool {
        (a - Vec3::from(b)).length() < 1e-4
    }

    #[test]
    fn rest_pose_leaves_the_mesh_alone_and_rotations_follow_the_parents() {
        let m = arm_model();
        assert_eq!(apply(&m, &Pose3d::default()).parts[0].positions, m.parts[0].positions);
        // ombro -90° em Z: o braço inteiro aponta para -Y
        let mut p = Pose3d::default();
        p.set_rotation("leftUpperArm", qa(Vec3::Z, -90.0).to_array());
        let posed = apply(&m, &p);
        assert!(close(Vec3::new(0.0, -2.5, 0.0), posed.parts[0].positions[2]), "{:?}", posed.parts[0].positions);
        // cotovelo +90° em Y (eixos do mundo no repouso): o antebraço, já para baixo, vai para... -Y girado: continua no plano
        p.set_rotation("leftLowerArm", qa(Vec3::Y, -90.0).to_array());
        let g = globals(m.skin.as_ref().unwrap(), &p);
        assert!(close(g[2].pos, [0.0, -1.0, 1.0]), "{:?}", g[2].pos);
        // quadril (raiz) desloca tudo
        let mut h = Pose3d::default();
        h.hips = [0.0, 1.0, 0.0];
        assert!(close(Vec3::from(apply(&m, &h).parts[0].positions[0]), [0.5, 1.0, 0.0]));
    }

    #[test]
    fn ring_rotation_turns_the_bone_around_its_joint_in_world_axes() {
        let m = arm_model();
        let s = m.skin.as_ref().unwrap();
        let mut p = Pose3d::default();
        p.set_rotation("leftUpperArm", qa(Vec3::Z, -90.0).to_array());
        // com o braço para baixo, girar o antebraço 90° em volta de X do mundo leva a mão para +Z ou -Z, na junta do cotovelo
        let q = rotate_bone_world(s, &p, 1, qa(Vec3::X, 90.0));
        let g = globals(s, &q);
        assert!(close(g[1].pos, [0.0, -1.0, 0.0]), "o cotovelo não sai do lugar: {:?}", g[1].pos);
        assert!(close(g[2].pos, [0.0, -1.0, -1.0]) || close(g[2].pos, [0.0, -1.0, 1.0]), "{:?}", g[2].pos);
    }

    #[test]
    fn ik_reaches_the_target_and_keeps_lengths() {
        let m = arm_model();
        let s = m.skin.as_ref().unwrap();
        let target = Vec3::new(1.0, 1.0, 0.3);
        let p = solve_ik(s, &Pose3d::default(), 2, target).unwrap();
        let g = globals(s, &p);
        assert!((g[2].pos - target).length() < 1e-3, "mão em {:?}", g[2].pos);
        assert!(((g[1].pos - g[0].pos).length() - 1.0).abs() < 1e-4);
        assert!(((g[2].pos - g[1].pos).length() - 1.0).abs() < 1e-4);
        // alvo fora do alcance: estica na direção dele
        let far = solve_ik(s, &Pose3d::default(), 2, Vec3::new(0.0, 10.0, 0.0)).unwrap();
        let gf = globals(s, &far);
        assert!(gf[2].pos.y > 1.99 && gf[2].pos.x.abs() < 0.05, "{:?}", gf[2].pos);
        // a orientação de mundo da mão é a mesma de antes
        assert!(g[2].rot.dot(Quat::IDENTITY).abs() > 0.9999);
    }

    #[test]
    fn presets_only_touch_bones_the_skeleton_has() {
        let m = arm_model();
        let s = m.skin.as_ref().unwrap();
        for k in PRESETS {
            let p = preset(k, s).unwrap();
            assert!(p.rotations.keys().all(|n| s.index(n).is_some()), "{k}");
        }
        assert!(preset("voar", s).is_none());
        let stand = preset("stand", s).unwrap();
        // braço baixado: a mão fica abaixo do ombro
        let g = globals(s, &stand);
        assert!(g[2].pos.y < -1.5, "{:?}", g[2].pos);
    }
}
