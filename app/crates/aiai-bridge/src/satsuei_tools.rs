//! Satsuei, câmera e cor (S13/S15) com schema tipado: `EFFECTS` (pilha de efeitos de uma camada ou camada de efeito),
//! `GRADE` (camada de efeito de cor no topo do corte), `CAMERA` (câmera do corte: valores, chaves, movimento, tremor) e
//! `LAYER_MOTION` (deslocamento, profundidade de multiplano e chaves de uma camada).
//!
//! Antes, tudo isso só era alcançável por `APPLY_COMMAND set_props` com JSON escrito à mão (e `params.effects` não tem
//! schema no documento). Aqui cada operação monta o dado tipado, valida (efeitos por `aiai_fx::stack_from_params`, chaves
//! por `KeyTracks::validate`) e aplica um `ReplaceData` pela `CoreSession` — um passo de undo, autor `agent:claude`.

use crate::core_tools::{apply, request};
use crate::protocol::BridgeError;
use aiai_core::keys::{Ease, Key, KeyTracks};
use aiai_core::{Command, CoreSession, Id, LayerData, LayerType, NodeKind};
use aiai_fx::{Effect, EffectInstance};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SATSUEI_ACTIONS: &[&str] = &["EFFECTS", "GRADE", "CAMERA", "LAYER_MOTION"];

pub fn is_mutating(action: &str) -> bool {
    SATSUEI_ACTIONS.contains(&action)
}

const CAMERA_PROPS: &[&str] = &["center_x", "center_y", "zoom", "rotation", "shake"];
const LAYER_PROPS: &[&str] = &["offset_x", "offset_y", "opacity", "rotation", "scale"];

// ------------------------------------------------------------------------------------------------- requisições

/// `EFFECTS`: pilha de efeitos de satsuei de uma camada. Numa camada comum o efeito vale para o conteúdo dela (glow de
/// um brilho, contorno, sombra); numa camada do tipo `effect`, para tudo o que está composto abaixo (difusão, grão,
/// vinheta, grade do corte).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EffectsRequest {
    List { layer: Id },
    /// Substitui a pilha inteira.
    Set { layer: Id, effects: Vec<EffectInstance> },
    /// Acrescenta um efeito (no fim ou em `index`).
    Add {
        layer: Id,
        effect: Effect,
        #[serde(default)]
        index: Option<usize>,
        /// Segura o efeito por N quadros (2 ou 3 = efeito "em 2s/3s" de anime); padrão 1.
        #[serde(default)]
        posterize_time: Option<u32>,
        /// Chaves por campo numérico do efeito (ex.: {"intensity": [{frame, value, ease}]}).
        #[serde(default)]
        keys: Option<KeyTracks>,
    },
    /// Muda o efeito `index` (campos omitidos ficam).
    Update {
        layer: Id,
        index: usize,
        #[serde(default)]
        effect: Option<Effect>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        posterize_time: Option<u32>,
        #[serde(default)]
        keys: Option<KeyTracks>,
    },
    Remove { layer: Id, index: usize },
    Clear { layer: Id },
}

/// `GRADE`: cor do corte numa camada de efeito no topo (criada se faltar, sempre movida para o topo).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GradeRequest {
    pub cut: Id,
    /// Pilha de cor: tipicamente `grade` (lift/gamma/gain, saturação, temperatura, contraste), `adjust`, `vignette`,
    /// `diffusion`, `grain`. Lista vazia remove a cor (mantém a camada vazia).
    pub effects: Vec<EffectInstance>,
    /// Nome da camada (padrão "Grade").
    #[serde(default)]
    pub name: Option<String>,
    /// Opacidade da camada (mistura da cor), padrão 1.
    #[serde(default)]
    pub opacity: Option<f32>,
}

/// Enquadramento: centro em pixels do canvas, zoom (1 = quadro do projeto), rotação em graus.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct Framing {
    pub center: [f32; 2],
    #[serde(default = "one")]
    pub zoom: f32,
    #[serde(default)]
    pub rotation: Option<f32>,
}

fn one() -> f32 {
    1.0
}

/// `CAMERA`: câmera do corte. Planos com `depth` > 0 (LAYER_MOTION) se deslocam menos no pan/zoom (multiplano).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CameraRequest {
    Read { cut: Id },
    /// Valores fixos (sem chave). `center` em pixels do canvas; `clear_center` volta ao centro do canvas.
    Set {
        cut: Id,
        #[serde(default)]
        center: Option<[f32; 2]>,
        #[serde(default)]
        clear_center: bool,
        #[serde(default)]
        zoom: Option<f32>,
        #[serde(default)]
        rotation: Option<f32>,
        #[serde(default)]
        shake_seed: Option<u64>,
    },
    /// Substitui as chaves de uma propriedade (center_x, center_y, zoom, rotation, shake); lista vazia apaga.
    Keys { cut: Id, property: String, keys: Vec<Key> },
    /// Movimento de câmera (pan/tilt/zoom/rotação) de `from` a `to` entre `start` e `end` (padrão: o corte todo):
    /// grava as chaves de center_x/center_y/zoom (e rotation se dada). `ease` padrão ease_in_out; antes de `start` e
    /// depois de `end` a câmera fica parada.
    Move {
        cut: Id,
        from: Framing,
        to: Framing,
        #[serde(default)]
        start: Option<u32>,
        #[serde(default)]
        end: Option<u32>,
        #[serde(default)]
        ease: Option<Ease>,
    },
    /// Tremor (impacto/terremoto): amplitude em px de `start` a `end`; `decay` cai até 0 no fim.
    Shake {
        cut: Id,
        amplitude: f32,
        start: u32,
        end: u32,
        #[serde(default = "yes")]
        decay: bool,
        #[serde(default)]
        seed: Option<u64>,
    },
    ClearKeys { cut: Id },
}

fn yes() -> bool {
    true
}

/// `LAYER_MOTION`: posição/profundidade/chaves de uma camada (cenário maior que o quadro, slide, multiplano).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LayerMotionRequest {
    Read { layer: Id },
    /// `depth` 0 = plano da câmera; maior = mais longe (move 1/(1+depth) do pan da câmera). `offset` em px.
    Set {
        layer: Id,
        #[serde(default)]
        offset: Option<[f32; 2]>,
        #[serde(default)]
        depth: Option<f32>,
        #[serde(default)]
        opacity: Option<f32>,
    },
    /// Substitui as chaves de uma propriedade (offset_x, offset_y, opacity, rotation, scale); lista vazia apaga.
    Keys { layer: Id, property: String, keys: Vec<Key> },
    /// Desliza a camada de `from` a `to` (offset em px) entre `start` e `end`.
    Slide {
        layer: Id,
        from: [f32; 2],
        to: [f32; 2],
        start: u32,
        end: u32,
        #[serde(default)]
        ease: Option<Ease>,
    },
    ClearKeys { layer: Id },
}

// --------------------------------------------------------------------------------------------------- despacho

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "EFFECTS" => effects(session, params),
        "GRADE" => grade(session, params),
        "CAMERA" => camera(session, params),
        "LAYER_MOTION" => layer_motion(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

fn layer_of(session: &CoreSession, id: &Id) -> Result<LayerData, BridgeError> {
    match session.project.node(id).map(|n| &n.data) {
        Some(NodeKind::Layer(l)) => Ok(l.clone()),
        Some(other) => Err(BridgeError::invalid_params(format!("{id} é {}, não camada", other.type_name()))),
        None => Err(BridgeError::invalid_params(format!("não encontrado: camada {id}"))),
    }
}

fn stack_of(l: &LayerData) -> Result<Vec<EffectInstance>, BridgeError> {
    aiai_fx::stack_from_params(&l.params).map_err(BridgeError::invalid_params)
}

fn with_stack(mut l: LayerData, stack: &[EffectInstance]) -> Result<LayerData, BridgeError> {
    for e in stack {
        e.keys.validate().map_err(|m| BridgeError::invalid_params(format!("chaves do efeito: {m}")))?;
        if e.posterize_time == 0 {
            return Err(BridgeError::invalid_params("posterize_time deve ser ≥ 1"));
        }
    }
    if !l.params.is_object() {
        l.params = json!({});
    }
    l.params["effects"] = serde_json::to_value(stack).map_err(|e| BridgeError::failed(e.to_string()))?;
    Ok(l)
}

fn stack_json(stack: &[EffectInstance]) -> Value {
    json!(stack.iter().enumerate().map(|(i, e)| json!({ "index": i, "name": e.effect.name(), "instance": e })).collect::<Vec<_>>())
}

fn replace_layer(session: &mut CoreSession, params: &Value, id: &Id, l: LayerData) -> Result<Value, BridgeError> {
    apply(session, params, Command::ReplaceData { id: id.clone(), data: NodeKind::Layer(l) })
}

fn effects(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let req: EffectsRequest = request(params, "EFFECTS")?;
    let id = match &req {
        EffectsRequest::List { layer }
        | EffectsRequest::Set { layer, .. }
        | EffectsRequest::Add { layer, .. }
        | EffectsRequest::Update { layer, .. }
        | EffectsRequest::Remove { layer, .. }
        | EffectsRequest::Clear { layer } => layer.clone(),
    };
    let l = layer_of(session, &id)?;
    let mut stack = stack_of(&l)?;
    let bad_index = |i: usize, n: usize| BridgeError::invalid_params(format!("índice {i} fora da pilha ({n} efeito(s))"));
    match req {
        EffectsRequest::List { .. } => return Ok(json!({ "layer": id, "kind": l.kind, "effects": stack_json(&stack) })),
        EffectsRequest::Set { effects, .. } => stack = effects,
        EffectsRequest::Add { effect, index, posterize_time, keys, .. } => {
            let inst = EffectInstance { effect, enabled: true, posterize_time: posterize_time.unwrap_or(1), keys: keys.unwrap_or_default() };
            let at = index.unwrap_or(stack.len()).min(stack.len());
            stack.insert(at, inst);
        }
        EffectsRequest::Update { index, effect, enabled, posterize_time, keys, .. } => {
            let n = stack.len();
            let e = stack.get_mut(index).ok_or_else(|| bad_index(index, n))?;
            if let Some(v) = effect {
                e.effect = v;
            }
            if let Some(v) = enabled {
                e.enabled = v;
            }
            if let Some(v) = posterize_time {
                e.posterize_time = v;
            }
            if let Some(v) = keys {
                e.keys = v;
            }
        }
        EffectsRequest::Remove { index, .. } => {
            if index >= stack.len() {
                return Err(bad_index(index, stack.len()));
            }
            stack.remove(index);
        }
        EffectsRequest::Clear { .. } => stack.clear(),
    }
    let kind = l.kind;
    let l = with_stack(l, &stack)?;
    let mut v = replace_layer(session, params, &id, l)?;
    v["effects"] = stack_json(&stack);
    if !matches!(kind, LayerType::Effect) && stack.iter().any(|e| matches!(e.effect, Effect::Grade { .. } | Effect::Vignette { .. } | Effect::Grain { .. })) {
        v["hint"] = json!("grade/vinheta/grão numa camada comum valem só para o conteúdo dela; para o quadro inteiro use uma camada do tipo effect (ou aiai_grade)");
    }
    Ok(v)
}

fn grade(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let r: GradeRequest = request(params, "GRADE")?;
    let cut = session.project.node(&r.cut).ok_or_else(|| BridgeError::invalid_params(format!("não encontrado: corte {}", r.cut)))?;
    if !matches!(cut.data, NodeKind::Cut(_)) {
        return Err(BridgeError::invalid_params(format!("{} não é corte", r.cut)));
    }
    let name = r.name.clone().unwrap_or_else(|| "Grade".into());
    let existing = cut
        .children
        .iter()
        .find(|c| session.project.node(c).is_some_and(|n| n.name == name && matches!(&n.data, NodeKind::Layer(l) if l.kind == LayerType::Effect)))
        .cloned();
    let top = cut.children.len();
    let mut cmds = Vec::new();
    let (id, mut l) = match existing {
        Some(id) => {
            let l = layer_of(session, &id)?;
            cmds.push(Command::MoveNode { id: id.clone(), parent: Some(r.cut.clone()), index: Some(top.saturating_sub(1)) });
            (id, l)
        }
        None => {
            let id = session.ids.next();
            cmds.push(Command::CreateNode { id: Some(id.clone()), parent: Some(r.cut.clone()), index: None, name: name.clone(), data: NodeKind::Layer(LayerData::new(LayerType::Effect)) });
            (id, LayerData::new(LayerType::Effect))
        }
    };
    if let Some(o) = r.opacity {
        if !(0.0..=1.0).contains(&o) {
            return Err(BridgeError::invalid_params("opacity fora de 0..1"));
        }
        l.opacity = o;
    }
    let l = with_stack(l, &r.effects)?;
    cmds.push(Command::ReplaceData { id: id.clone(), data: NodeKind::Layer(l) });
    let mut v = apply(session, params, Command::Batch { commands: cmds, label: Some(format!("Cor do corte ({name})")) })?;
    v["layer"] = json!(id);
    v["effects"] = stack_json(&r.effects);
    Ok(v)
}

fn check_keys(prop: &str, allowed: &[&str], keys: &[Key]) -> Result<(), BridgeError> {
    if !allowed.contains(&prop) {
        return Err(BridgeError::invalid_params(format!("propriedade '{prop}' desconhecida (use {})", allowed.join(", "))));
    }
    let mut t = KeyTracks::default();
    t.0.insert(prop.into(), keys.to_vec());
    t.validate().map_err(BridgeError::invalid_params)
}

fn set_track(tracks: &mut KeyTracks, prop: &str, mut keys: Vec<Key>) {
    keys.sort_by_key(|k| k.frame);
    if keys.is_empty() {
        tracks.0.remove(prop);
    } else {
        tracks.0.insert(prop.into(), keys);
    }
}

fn two_keys(start: u32, end: u32, a: f32, b: f32, ease: Ease) -> Vec<Key> {
    vec![Key { frame: start, value: a, ease }, Key { frame: end, value: b, ease: Ease::Linear }]
}

fn camera(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let req: CameraRequest = request(params, "CAMERA")?;
    let cut_id = match &req {
        CameraRequest::Read { cut }
        | CameraRequest::Set { cut, .. }
        | CameraRequest::Keys { cut, .. }
        | CameraRequest::Move { cut, .. }
        | CameraRequest::Shake { cut, .. }
        | CameraRequest::ClearKeys { cut } => cut.clone(),
    };
    let mut cd = match session.project.node(&cut_id).map(|n| &n.data) {
        Some(NodeKind::Cut(c)) => c.clone(),
        Some(other) => return Err(BridgeError::invalid_params(format!("{cut_id} é {}, não corte", other.type_name()))),
        None => return Err(BridgeError::invalid_params(format!("não encontrado: corte {cut_id}"))),
    };
    let last = cd.duration.saturating_sub(1);
    let in_cut = |f: u32, what: &str| -> Result<(), BridgeError> {
        if f > last { Err(BridgeError::invalid_params(format!("{what} {f} fora do corte (0..{last})"))) } else { Ok(()) }
    };
    match req {
        CameraRequest::Read { .. } => {
            let (w, h) = (session.project.settings.width, session.project.settings.height);
            let (cw, ch) = cd.canvas.map(|c| (c[0], c[1])).unwrap_or((w, h));
            return Ok(json!({ "cut": cut_id, "camera": cd.camera, "duration": cd.duration, "canvas": [cw, ch], "output": [w, h] }));
        }
        CameraRequest::Set { center, clear_center, zoom, rotation, shake_seed, .. } => {
            if clear_center {
                cd.camera.center = None;
            }
            if let Some(c) = center {
                cd.camera.center = Some(c);
            }
            if let Some(z) = zoom {
                if !(z.is_finite() && z > 0.0) {
                    return Err(BridgeError::invalid_params("zoom deve ser > 0"));
                }
                cd.camera.zoom = z;
            }
            if let Some(r) = rotation {
                cd.camera.rotation = r;
            }
            if let Some(s) = shake_seed {
                cd.camera.shake_seed = s;
            }
        }
        CameraRequest::Keys { property, keys, .. } => {
            check_keys(&property, CAMERA_PROPS, &keys)?;
            set_track(&mut cd.camera.keys, &property, keys);
        }
        CameraRequest::Move { from, to, start, end, ease, .. } => {
            let (s, e) = (start.unwrap_or(0), end.unwrap_or(last));
            in_cut(s, "start")?;
            in_cut(e, "end")?;
            if e <= s {
                return Err(BridgeError::invalid_params("end deve ser maior que start"));
            }
            if !(from.zoom > 0.0 && to.zoom > 0.0) {
                return Err(BridgeError::invalid_params("zoom deve ser > 0"));
            }
            let ease = ease.unwrap_or(Ease::EaseInOut);
            let k = &mut cd.camera.keys;
            set_track(k, "center_x", two_keys(s, e, from.center[0], to.center[0], ease));
            set_track(k, "center_y", two_keys(s, e, from.center[1], to.center[1], ease));
            set_track(k, "zoom", two_keys(s, e, from.zoom, to.zoom, ease));
            if from.rotation.is_some() || to.rotation.is_some() {
                set_track(k, "rotation", two_keys(s, e, from.rotation.unwrap_or(0.0), to.rotation.unwrap_or(0.0), ease));
            }
            cd.camera.center = Some(from.center);
            cd.camera.zoom = from.zoom;
        }
        CameraRequest::Shake { amplitude, start, end, decay, seed, .. } => {
            in_cut(start, "start")?;
            in_cut(end, "end")?;
            if end <= start || !(amplitude.is_finite() && amplitude >= 0.0) {
                return Err(BridgeError::invalid_params("tremor: end > start e amplitude ≥ 0"));
            }
            let mut keys = Vec::new();
            if start > 0 {
                keys.push(Key { frame: start - 1, value: 0.0, ease: Ease::Hold });
            }
            keys.push(Key { frame: start, value: amplitude, ease: if decay { Ease::EaseOut } else { Ease::Hold } });
            keys.push(Key { frame: end, value: if decay { 0.0 } else { amplitude }, ease: Ease::Hold });
            if !decay && end < last {
                keys.push(Key { frame: end + 1, value: 0.0, ease: Ease::Hold });
            }
            set_track(&mut cd.camera.keys, "shake", keys);
            if let Some(s) = seed {
                cd.camera.shake_seed = s;
            }
        }
        CameraRequest::ClearKeys { .. } => cd.camera.keys = KeyTracks::default(),
    }
    let camera = cd.camera.clone();
    let mut v = apply(session, params, Command::ReplaceData { id: cut_id, data: NodeKind::Cut(cd) })?;
    v["camera"] = serde_json::to_value(camera).map_err(|e| BridgeError::failed(e.to_string()))?;
    Ok(v)
}

fn layer_motion(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let req: LayerMotionRequest = request(params, "LAYER_MOTION")?;
    let id = match &req {
        LayerMotionRequest::Read { layer }
        | LayerMotionRequest::Set { layer, .. }
        | LayerMotionRequest::Keys { layer, .. }
        | LayerMotionRequest::Slide { layer, .. }
        | LayerMotionRequest::ClearKeys { layer } => layer.clone(),
    };
    let mut l = layer_of(session, &id)?;
    match req {
        LayerMotionRequest::Read { .. } => {
            return Ok(json!({ "layer": id, "offset": l.offset, "depth": l.depth, "opacity": l.opacity, "keys": l.keys }));
        }
        LayerMotionRequest::Set { offset, depth, opacity, .. } => {
            if let Some(o) = offset {
                l.offset = o;
            }
            if let Some(d) = depth {
                if !(d.is_finite() && d >= 0.0) {
                    return Err(BridgeError::invalid_params("depth deve ser ≥ 0"));
                }
                l.depth = d;
            }
            if let Some(o) = opacity {
                if !(0.0..=1.0).contains(&o) {
                    return Err(BridgeError::invalid_params("opacity fora de 0..1"));
                }
                l.opacity = o;
            }
        }
        LayerMotionRequest::Keys { property, keys, .. } => {
            check_keys(&property, LAYER_PROPS, &keys)?;
            set_track(&mut l.keys, &property, keys);
        }
        LayerMotionRequest::Slide { from, to, start, end, ease, .. } => {
            if end <= start {
                return Err(BridgeError::invalid_params("end deve ser maior que start"));
            }
            let ease = ease.unwrap_or(Ease::EaseInOut);
            set_track(&mut l.keys, "offset_x", two_keys(start, end, from[0], to[0], ease));
            set_track(&mut l.keys, "offset_y", two_keys(start, end, from[1], to[1], ease));
            l.offset = from;
        }
        LayerMotionRequest::ClearKeys { .. } => l.keys = KeyTracks::default(),
    }
    let out = json!({ "offset": l.offset, "depth": l.depth, "opacity": l.opacity, "keys": l.keys });
    let mut v = replace_layer(session, params, &id, l)?;
    v["layer_motion"] = out;
    Ok(v)
}
