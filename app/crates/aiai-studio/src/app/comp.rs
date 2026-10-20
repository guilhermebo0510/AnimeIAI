//! Composição (撮影) como no After Effects (S24 3.1–3.5): o estado do viewer e da Linha do tempo da composição, o modelo
//! das propriedades animáveis e as operações nas chaves.
//!
//! Uma propriedade (`PropRef`) é de uma camada (a transformação: Âncora, Posição, Escala, Rotação, Opacidade) ou de um
//! efeito da pilha da camada (um parâmetro numérico, ou um vetor como `center` [x, y] e `color` [r, g, b]). Cada uma tem
//! 1 a 4 trilhas no motor: as chaves da camada ficam em `LayerData.keys` (`offset_x`, `anchor_y`...) e as do efeito em
//! `EffectInstance.keys` pelo caminho do parâmetro (`radius`, `center.0`). Sem chave vale o valor fixo (os campos da
//! camada, o JSON do efeito); com o cronômetro ligado, mudar o valor grava uma chave no quadro atual. É o mesmo dado
//! que o render, a exportação e o MCP (`aiai_layer_motion`, `aiai_effects`) leem.
//!
//! Toda edição monta uma cópia da camada, valida (chaves em ordem e em parâmetros que existem) e grava num passo de
//! desfazer (`ReplaceData`).

use super::commands::{self as cmds, NodeId};
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::keys::{Ease, KeyInterp, KeyTracks};
use aiai_core::{Command, CoreSession, LayerData, LayerType, NodeKind, Project};
use aiai_fx::EffectInstance;
use egui::{Rect, Vec2};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Propriedades de transformação, na ordem do After Effects.
pub(crate) const TRANSFORM: [&str; 5] = ["anchor", "position", "scale", "rotation", "opacity"];

/// Dono de uma propriedade: a transformação da camada ou o efeito `índice` da pilha dela.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(crate) enum Owner {
    Layer(NodeId),
    Effect(NodeId, usize),
}

impl Owner {
    pub(crate) fn layer(&self) -> &NodeId {
        match self {
            Owner::Layer(l) | Owner::Effect(l, _) => l,
        }
    }
}

/// Uma propriedade animável (uma linha da linha do tempo).
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(crate) struct PropRef {
    pub owner: Owner,
    /// Transformação: `anchor`, `position`, `scale`, `rotation`, `opacity`; efeito: caminho base do parâmetro.
    pub name: String,
}

impl PropRef {
    pub(crate) fn layer(layer: &NodeId, name: &str) -> PropRef {
        PropRef { owner: Owner::Layer(layer.clone()), name: name.to_owned() }
    }
    pub(crate) fn effect(layer: &NodeId, index: usize, name: &str) -> PropRef {
        PropRef { owner: Owner::Effect(layer.clone(), index), name: name.to_owned() }
    }
}

/// Como mostrar o valor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Unit {
    Px,
    Percent,
    Degrees,
    Plain,
    Color,
}

/// Trilhas do motor de uma propriedade de transformação.
pub(crate) fn transform_tracks(name: &str) -> &'static [&'static str] {
    match name {
        "anchor" => &["anchor_x", "anchor_y"],
        "position" => &["offset_x", "offset_y"],
        "scale" => &["scale"],
        "rotation" => &["rotation"],
        "opacity" => &["opacity"],
        _ => &[],
    }
}

pub(crate) fn unit(p: &PropRef) -> Unit {
    match (&p.owner, p.name.as_str()) {
        (Owner::Layer(_), "anchor" | "position") => Unit::Px,
        (Owner::Layer(_), "scale" | "opacity") => Unit::Percent,
        (Owner::Layer(_), "rotation") => Unit::Degrees,
        (Owner::Effect(..), n) if matches!(n.rsplit('.').next(), Some("color" | "color_from" | "color_to" | "dark" | "light")) => Unit::Color,
        (Owner::Effect(..), n) if matches!(n.rsplit('.').next(), Some("angle" | "hue")) => Unit::Degrees,
        _ => Unit::Plain,
    }
}

/// Pilha de efeitos da camada (vazia se não há ou se é ilegível — o render avisa do ilegível).
pub(crate) fn stack(l: &LayerData) -> Vec<EffectInstance> {
    aiai_fx::stack_from_params(&l.params).unwrap_or_default()
}

fn put_stack(l: &mut LayerData, st: &[EffectInstance]) -> Result<(), String> {
    if !l.params.is_object() {
        l.params = serde_json::json!({});
    }
    l.params["effects"] = serde_json::to_value(st).map_err(|e| e.to_string())?;
    Ok(())
}

/// Linhas de parâmetro de um efeito: os caminhos numéricos agrupando os vetores (`center.0`, `center.1` → `center`).
pub(crate) fn effect_rows(e: &aiai_fx::Effect) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in aiai_fx::numeric_params(e) {
        let base = match p.rsplit_once('.') {
            Some((b, last)) if last.parse::<usize>().is_ok() => b.to_owned(),
            _ => p,
        };
        if !out.contains(&base) {
            out.push(base);
        }
    }
    out
}

/// Trilhas do motor de uma propriedade na camada `l`.
pub(crate) fn tracks(l: &LayerData, p: &PropRef) -> Vec<String> {
    match &p.owner {
        Owner::Layer(_) => transform_tracks(&p.name).iter().map(|s| s.to_string()).collect(),
        Owner::Effect(_, i) => {
            let Some(inst) = stack(l).into_iter().nth(*i) else { return Vec::new() };
            let v = serde_json::to_value(&inst.effect).unwrap_or_default();
            let mut cur = &v;
            for part in p.name.split('.') {
                cur = match cur.get(part) {
                    Some(c) => c,
                    None => return Vec::new(),
                };
            }
            match cur {
                serde_json::Value::Array(a) => (0..a.len()).map(|k| format!("{}.{k}", p.name)).collect(),
                serde_json::Value::Number(_) => vec![p.name.clone()],
                _ => Vec::new(),
            }
        }
    }
}

fn keys_of(l: &LayerData, owner: &Owner) -> KeyTracks {
    match owner {
        Owner::Layer(_) => l.keys.clone(),
        Owner::Effect(_, i) => stack(l).into_iter().nth(*i).map(|e| e.keys).unwrap_or_default(),
    }
}

/// Muda as chaves do dono (as da camada ou as do efeito, regravando a pilha).
fn with_keys<R>(l: &mut LayerData, owner: &Owner, f: impl FnOnce(&mut KeyTracks) -> R) -> Result<R, String> {
    match owner {
        Owner::Layer(_) => Ok(f(&mut l.keys)),
        Owner::Effect(_, i) => {
            let mut st = stack(l);
            let n = st.len();
            let inst = st.get_mut(*i).ok_or_else(|| format!("efeito {i} fora da pilha ({n})"))?;
            let r = f(&mut inst.keys);
            put_stack(l, &st)?;
            Ok(r)
        }
    }
}

/// Valor fixo (sem chave) de uma trilha.
pub(crate) fn static_value(l: &LayerData, owner: &Owner, track: &str) -> f32 {
    match owner {
        Owner::Layer(_) => match track {
            "offset_x" => l.offset[0],
            "offset_y" => l.offset[1],
            "opacity" => l.opacity,
            "scale" => l.scale,
            "rotation" => l.rotation,
            "anchor_x" => l.anchor[0],
            "anchor_y" => l.anchor[1],
            _ => 0.0,
        },
        Owner::Effect(_, i) => stack(l)
            .into_iter()
            .nth(*i)
            .and_then(|e| serde_json::to_value(&e.effect).ok())
            .and_then(|v| aiai_fx::param_get(&v, track))
            .unwrap_or(0.0) as f32,
    }
}

fn set_static(l: &mut LayerData, owner: &Owner, track: &str, v: f32) -> Result<(), String> {
    if !v.is_finite() {
        return Err(format!("valor não finito em '{track}'"));
    }
    match owner {
        Owner::Layer(_) => {
            match track {
                "offset_x" => l.offset[0] = v,
                "offset_y" => l.offset[1] = v,
                "opacity" => l.opacity = v.clamp(0.0, 1.0),
                "scale" => l.scale = v.max(0.001),
                "rotation" => l.rotation = v,
                "anchor_x" => l.anchor[0] = v,
                "anchor_y" => l.anchor[1] = v,
                other => return Err(format!("propriedade desconhecida '{other}'")),
            }
            Ok(())
        }
        Owner::Effect(_, i) => {
            let mut st = stack(l);
            let n = st.len();
            let inst = st.get_mut(*i).ok_or_else(|| format!("efeito {i} fora da pilha ({n})"))?;
            let mut j = serde_json::to_value(&inst.effect).map_err(|e| e.to_string())?;
            if !aiai_fx::param_set(&mut j, track, v as f64) {
                return Err(format!("'{track}' não é parâmetro de '{}'", inst.effect.name()));
            }
            inst.effect = serde_json::from_value(j).map_err(|e| e.to_string())?;
            put_stack(l, &st)
        }
    }
}

/// Valor da trilha no quadro (chave, se houver; senão o fixo).
pub(crate) fn track_value(l: &LayerData, owner: &Owner, keys: &KeyTracks, track: &str, frame: f32) -> f32 {
    keys.eval(track, frame).unwrap_or_else(|| static_value(l, owner, track))
}

pub(crate) fn values(l: &LayerData, p: &PropRef, frame: u32) -> Vec<f32> {
    let keys = keys_of(l, &p.owner);
    tracks(l, p).iter().map(|t| track_value(l, &p.owner, &keys, t, frame as f32)).collect()
}

/// Cronômetro ligado: alguma trilha da propriedade tem chave.
pub(crate) fn animated(l: &LayerData, p: &PropRef) -> bool {
    let keys = keys_of(l, &p.owner);
    tracks(l, p).iter().any(|t| keys.has(t))
}

/// Quadros com chave (união das trilhas), em ordem.
pub(crate) fn key_frames(l: &LayerData, p: &PropRef) -> Vec<u32> {
    let keys = keys_of(l, &p.owner);
    let set: BTreeSet<u32> = tracks(l, p).iter().flat_map(|t| keys.frames(t)).collect();
    set.into_iter().collect()
}

/// Easing do trecho que começa na chave do quadro (da 1ª trilha que tem chave ali).
pub(crate) fn key_ease(l: &LayerData, p: &PropRef, frame: u32) -> Option<Ease> {
    let keys = keys_of(l, &p.owner);
    tracks(l, p).iter().find_map(|t| keys.0.get(t).and_then(|ks| ks.iter().find(|k| k.frame == frame)).map(|k| k.ease))
}

/// Chaves cruas de uma trilha (editor de gráfico).
pub(crate) fn track_keys(l: &LayerData, p: &PropRef, track: &str) -> Vec<aiai_core::Key> {
    keys_of(l, &p.owner).0.get(track).cloned().unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------ edições (puras)

/// Muda o valor no quadro: com o cronômetro ligado grava chave em todas as trilhas; sem, muda o valor fixo.
pub(crate) fn set_values(l: &mut LayerData, p: &PropRef, frame: u32, vals: &[f32]) -> Result<(), String> {
    let tr = tracks(l, p);
    let anim = animated(l, p);
    for (t, v) in tr.iter().zip(vals) {
        if anim {
            if !v.is_finite() {
                return Err(format!("valor não finito em '{t}'"));
            }
            let v = if t == "opacity" { v.clamp(0.0, 1.0) } else { *v };
            with_keys(l, &p.owner, |k| k.set_key(t, frame, v))?;
        } else {
            set_static(l, &p.owner, t, *v)?;
        }
    }
    Ok(())
}

/// Liga/desliga o cronômetro. Ligar grava uma chave no quadro com o valor atual; desligar apaga as chaves e fixa o
/// valor que a propriedade tinha no quadro atual (como no After Effects). Devolve se ficou animada.
pub(crate) fn toggle_stopwatch(l: &mut LayerData, p: &PropRef, frame: u32) -> Result<bool, String> {
    let tr = tracks(l, p);
    if animated(l, p) {
        let vals = values(l, p, frame);
        with_keys(l, &p.owner, |k| {
            for t in &tr {
                k.0.remove(t);
            }
        })?;
        for (t, v) in tr.iter().zip(vals) {
            set_static(l, &p.owner, t, v)?;
        }
        Ok(false)
    } else {
        let vals = values(l, p, frame);
        with_keys(l, &p.owner, |k| {
            for (t, v) in tr.iter().zip(&vals) {
                k.set_key(t, frame, *v);
            }
        })?;
        Ok(true)
    }
}

/// Losango do quadro atual: cria a chave (com o valor atual) se falta, tira se existe. Só com o cronômetro ligado.
pub(crate) fn toggle_key_here(l: &mut LayerData, p: &PropRef, frame: u32) -> Result<(), String> {
    if !animated(l, p) {
        toggle_stopwatch(l, p, frame)?;
        return Ok(());
    }
    if key_frames(l, p).contains(&frame) {
        delete_keys(l, p, &[frame], frame)
    } else {
        let vals = values(l, p, frame);
        let tr = tracks(l, p);
        with_keys(l, &p.owner, |k| {
            for (t, v) in tr.iter().zip(&vals) {
                k.set_key(t, frame, *v);
            }
        })
    }
}

pub(crate) fn move_keys(l: &mut LayerData, p: &PropRef, frames: &[u32], delta: i64) -> Result<(), String> {
    let tr = tracks(l, p);
    with_keys(l, &p.owner, |k| tr.iter().try_for_each(|t| k.move_keys(t, frames, delta)))?
}

/// Apaga as chaves dos quadros. Se a propriedade fica sem chave, o valor fixo passa a ser o que ela tinha em `now`.
pub(crate) fn delete_keys(l: &mut LayerData, p: &PropRef, frames: &[u32], now: u32) -> Result<(), String> {
    let tr = tracks(l, p);
    let vals = values(l, p, now);
    with_keys(l, &p.owner, |k| {
        for t in &tr {
            for f in frames {
                k.remove_key(t, *f);
            }
        }
    })?;
    if !animated(l, p) {
        for (t, v) in tr.iter().zip(vals) {
            set_static(l, &p.owner, t, v)?;
        }
    }
    Ok(())
}

pub(crate) fn set_interp(l: &mut LayerData, p: &PropRef, frames: &[u32], interp: KeyInterp) -> Result<(), String> {
    let tr = tracks(l, p);
    with_keys(l, &p.owner, |k| {
        for t in &tr {
            k.set_interp(t, Some(frames), interp);
        }
    })
}

/// Easing cru do trecho que começa na chave `frame` (alças da Bézier no editor de gráfico).
pub(crate) fn set_segment_ease(l: &mut LayerData, p: &PropRef, frame: u32, ease: Ease) -> Result<(), String> {
    let tr = tracks(l, p);
    with_keys(l, &p.owner, |k| {
        for t in &tr {
            k.set_ease(t, Some(&[frame]), ease);
        }
    })
}

/// Valor de UMA chave de uma trilha (arrasto vertical no editor de gráfico).
pub(crate) fn set_key_value(l: &mut LayerData, p: &PropRef, track: &str, frame: u32, v: f32) -> Result<(), String> {
    if !v.is_finite() {
        return Err("valor não finito".into());
    }
    let v = if track == "opacity" { v.clamp(0.0, 1.0) } else { v };
    with_keys(l, &p.owner, |k| {
        if k.frames(track).contains(&frame) {
            k.set_key(track, frame, v);
        }
    })
}

/// Validação de tudo o que o motor exige antes de gravar.
pub(crate) fn validate(l: &LayerData) -> Result<(), String> {
    l.keys.validate()?;
    for (i, e) in stack(l).iter().enumerate() {
        e.validate().map_err(|m| format!("efeito {}: {m}", i + 1))?;
    }
    Ok(())
}

/// Grava a camada num passo de desfazer com o rótulo dado.
pub(crate) fn commit(core: &mut CoreSession, layer: &NodeId, l: LayerData, label: &str) -> Result<(), String> {
    validate(&l)?;
    cmds::run(core, Command::Batch { commands: vec![Command::ReplaceData { id: layer.clone(), data: NodeKind::Layer(l) }], label: Some(label.to_owned()) }).map(|_| ())
}

/// Edita a camada: `f` muda a cópia; se mudou, grava (um passo de desfazer); erro vira aviso. Devolve se gravou.
pub(crate) fn edit(core: &mut CoreSession, ed: &mut super::editor::Editor, layer: &NodeId, label_key: &str, f: impl FnOnce(&mut LayerData) -> Result<(), String>) -> bool {
    let Some(orig) = cmds::layer_data(&core.project, layer).cloned() else { return false };
    let mut l = orig.clone();
    match f(&mut l).and_then(|_| if l == orig { Ok(false) } else { commit(core, layer, l, tr(label_key)).map(|_| true) }) {
        Ok(changed) => changed,
        Err(e) => {
            ed.notify(e);
            false
        }
    }
}

// ------------------------------------------------------------------------------------------------ estado

/// O que a camada aberta mostra (atalhos A P S R T, E e U do After Effects).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub(crate) enum Reveal {
    /// Tudo (abrir pela setinha).
    #[default]
    All,
    /// Só estas propriedades de transformação (P = posição; Shift soma).
    Only(BTreeSet<&'static str>),
    /// Só o que tem chave (U).
    Animated,
    /// Só os efeitos (E).
    Effects,
}

/// Chave selecionada na linha do tempo.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub(crate) struct KeySel {
    pub prop: PropRef,
    pub frame: u32,
}

/// Chave copiada: propriedade (nome), quadro relativo à primeira copiada, valores por trilha e easing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClipKey {
    pub name: String,
    pub effect: bool,
    pub rel: u32,
    pub vals: Vec<f32>,
    pub ease: Ease,
}

/// Arrasto no viewer (ferramentas da Composição).
#[derive(Clone, Debug)]
pub(crate) enum CompDrag {
    /// V: desloca a camada.
    Move { layer: NodeId, start: Vec2, orig: LayerData },
    /// W: gira em volta da âncora.
    Rotate { layer: NodeId, pivot: Vec2, start_angle: f32, orig: LayerData },
    /// Y: move a âncora sem mover a camada.
    Anchor { layer: NodeId, start: Vec2, orig: LayerData },
    /// C: arrasta o quadro da câmera (Shift = zoom, Ctrl = rotação).
    Camera { start: Vec2, orig: aiai_core::CameraData },
    /// Região de interesse.
    Roi { start: Vec2, cur: Vec2 },
}

/// Arrasto de chaves na linha do tempo.
#[derive(Clone, Debug)]
pub(crate) struct KeyDrag {
    pub start_x: f32,
    pub delta: i64,
}

pub(crate) struct CompState {
    // ---- viewer
    /// Divisor da resolução do viewer: 1 (completa), 2 (1/2), 4 (1/4).
    pub resolution: u32,
    pub checker: bool,
    pub alpha_view: bool,
    /// Região de interesse (canvas: x0, y0, x1, y1).
    pub roi: Option<[f32; 4]>,
    pub roi_pick: bool,
    pub drag: Option<CompDrag>,
    /// Ctrl apertado no viewer (o arrasto da câmera gira com Ctrl).
    pub ctrl: bool,
    /// Prévia do arrasto do viewer: camada ou câmera como vão ficar ao soltar.
    pub preview: Option<(NodeId, LayerData)>,
    pub cam_preview: Option<aiai_core::CameraData>,
    pub alpha_tex: Option<(super::render_service::FrameKey, egui::TextureHandle)>,
    // ---- linha do tempo
    pub expanded: HashSet<NodeId>,
    pub reveal: Reveal,
    /// Efeitos abertos dentro de uma camada aberta.
    pub fx_open: HashSet<(NodeId, usize)>,
    /// Efeitos fechados no painel Controles de efeito.
    pub fx_closed: HashSet<(NodeId, usize)>,
    pub solo: BTreeSet<NodeId>,
    pub sel_keys: BTreeSet<KeySel>,
    /// Propriedade selecionada (alvo do editor de gráfico, de colar e de J/K).
    pub sel_prop: Option<PropRef>,
    pub key_drag: Option<KeyDrag>,
    /// Caixa de seleção de chaves (tela).
    pub marquee: Option<(egui::Pos2, egui::Pos2)>,
    pub clipboard: Vec<ClipKey>,
    pub px_per_frame: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub graph: bool,
    /// Faixa de valores do gráfico fixada durante um arrasto (não pula enquanto se arrasta).
    pub graph_range: Option<(f32, f32)>,
    pub graph_drag: Option<GraphDrag>,
    /// Barras de duração: (contador, corte) → camada → (primeiro, último+1) quadro com desenho exposto.
    pub bars: Option<(u64, NodeId, BTreeMap<NodeId, Option<(u32, u32)>>)>,
    // ---- efeitos e predefinições
    pub search: String,
    pub favorites: BTreeSet<String>,
    pub presets: Vec<super::panels::effects_browser::UserPreset>,
    pub presets_loaded: bool,
    pub thumbs: BTreeMap<String, egui::TextureHandle>,
    /// Efeito (ou predefinição) sendo arrastado do navegador: aplica ao soltar numa camada ou no viewer.
    pub dragging_fx: Option<super::panels::effects_browser::FxPayload>,
    pub preset_name: String,
    /// Pastas do usuário (Documentos\AnimeIAI\Predefinições): desligado com `--no-session`.
    pub user_files: bool,
}

/// Arrasto no editor de gráfico.
#[derive(Clone, Debug)]
pub(crate) enum GraphDrag {
    /// Chave: trilha, quadro original, deslocamento atual (quadros) e valor novo.
    Key { track: String, frame: u32, df: i64, value: f32 },
    /// Alça da Bézier do trecho que começa em `frame`: `out` = alça de saída (x1, y1), senão a de chegada (x2, y2).
    Handle { frame: u32, out: bool, b: [f32; 4] },
}

impl Default for CompState {
    fn default() -> Self {
        CompState {
            resolution: 1,
            checker: true,
            alpha_view: false,
            roi: None,
            roi_pick: false,
            drag: None,
            ctrl: false,
            preview: None,
            cam_preview: None,
            alpha_tex: None,
            expanded: HashSet::new(),
            reveal: Reveal::All,
            fx_open: HashSet::new(),
            fx_closed: HashSet::new(),
            solo: BTreeSet::new(),
            sel_keys: BTreeSet::new(),
            sel_prop: None,
            key_drag: None,
            marquee: None,
            clipboard: Vec::new(),
            px_per_frame: 10.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            graph: false,
            graph_range: None,
            graph_drag: None,
            bars: None,
            search: String::new(),
            favorites: BTreeSet::new(),
            presets: Vec::new(),
            presets_loaded: false,
            thumbs: BTreeMap::new(),
            dragging_fx: None,
            preset_name: String::new(),
            user_files: false,
        }
    }
}

// ------------------------------------------------------------------------------------------------ operações da UI

/// Linhas visíveis de propriedade de uma camada aberta, conforme o `Reveal`.
pub(crate) fn shows(reveal: &Reveal, l: &LayerData, p: &PropRef) -> bool {
    match (reveal, &p.owner) {
        (Reveal::All, _) => true,
        (Reveal::Only(set), Owner::Layer(_)) => set.contains(p.name.as_str()),
        (Reveal::Only(_), Owner::Effect(..)) => false,
        (Reveal::Animated, _) => animated(l, p),
        (Reveal::Effects, Owner::Effect(..)) => true,
        (Reveal::Effects, Owner::Layer(_)) => false,
    }
}

/// Chaves selecionadas agrupadas por camada → propriedade → quadros.
fn grouped(sel: &BTreeSet<KeySel>) -> BTreeMap<NodeId, BTreeMap<PropRef, Vec<u32>>> {
    let mut out: BTreeMap<NodeId, BTreeMap<PropRef, Vec<u32>>> = BTreeMap::new();
    for k in sel {
        out.entry(k.prop.owner.layer().clone()).or_default().entry(k.prop.clone()).or_default().push(k.frame);
    }
    out
}

/// Interpolação nas chaves selecionadas (F9 e os botões). Um passo de desfazer por camada.
pub(crate) fn interp_selected(core: &mut CoreSession, ed: &mut super::editor::Editor, interp: KeyInterp) {
    let sel = ed.comp.sel_keys.clone();
    if sel.is_empty() {
        ed.notify(tr("comp.no_keys_selected").to_owned());
        return;
    }
    for (layer, props) in grouped(&sel) {
        edit(core, ed, &layer, "comp.undo.interp", |l| props.iter().try_for_each(|(p, frames)| set_interp(l, p, frames, interp)));
    }
}

/// Apaga as chaves selecionadas.
pub(crate) fn delete_selected(core: &mut CoreSession, ed: &mut super::editor::Editor) {
    let sel = std::mem::take(&mut ed.comp.sel_keys);
    let now = ed.frame;
    for (layer, props) in grouped(&sel) {
        edit(core, ed, &layer, "comp.undo.delete_keys", |l| props.iter().try_for_each(|(p, frames)| delete_keys(l, p, frames, now)));
    }
}

/// Desloca as chaves selecionadas (arrasto dos losangos). A seleção acompanha.
pub(crate) fn move_selected(core: &mut CoreSession, ed: &mut super::editor::Editor, delta: i64) {
    if delta == 0 {
        return;
    }
    let sel = ed.comp.sel_keys.clone();
    let mut ok = true;
    for (layer, props) in grouped(&sel) {
        ok &= edit(core, ed, &layer, "comp.undo.move_keys", |l| props.iter().try_for_each(|(p, frames)| move_keys(l, p, frames, delta)));
    }
    if ok {
        ed.comp.sel_keys = sel.into_iter().map(|k| KeySel { frame: (k.frame as i64 + delta).max(0) as u32, ..k }).collect();
    }
}

/// Copia as chaves selecionadas (valores de cada trilha e easing), com o tempo relativo à primeira.
pub(crate) fn copy_selected(p: &Project, ed: &mut super::editor::Editor) {
    let sel = ed.comp.sel_keys.clone();
    let Some(first) = sel.iter().map(|k| k.frame).min() else { return };
    let mut clip = Vec::new();
    for k in &sel {
        let Some(l) = cmds::layer_data(p, k.prop.owner.layer()) else { continue };
        let keys = keys_of(l, &k.prop.owner);
        let vals = tracks(l, &k.prop).iter().map(|t| track_value(l, &k.prop.owner, &keys, t, k.frame as f32)).collect();
        clip.push(ClipKey { name: k.prop.name.clone(), effect: matches!(k.prop.owner, Owner::Effect(..)), rel: k.frame - first, vals, ease: key_ease(l, &k.prop, k.frame).unwrap_or_default() });
    }
    ed.comp.clipboard = clip;
}

/// Cola no quadro atual: na propriedade selecionada (se as chaves vieram de uma só propriedade) ou nas propriedades de
/// mesmo nome da camada ativa. Liga o cronômetro se preciso.
pub(crate) fn paste(core: &mut CoreSession, ed: &mut super::editor::Editor) {
    let clip = ed.comp.clipboard.clone();
    if clip.is_empty() {
        return;
    }
    let names: BTreeSet<(&str, bool)> = clip.iter().map(|c| (c.name.as_str(), c.effect)).collect();
    let now = ed.frame;
    let targets: Vec<(PropRef, String)> = match (&ed.comp.sel_prop, names.len()) {
        (Some(p), 1) => vec![(p.clone(), clip[0].name.clone())],
        _ => {
            let Some(layer) = ed.layer.clone() else { return };
            names.iter().filter(|(_, fx)| !fx).map(|(n, _)| (PropRef::layer(&layer, n), n.to_string())).collect()
        }
    };
    let mut new_sel = BTreeSet::new();
    for (target, from) in targets {
        let layer = target.owner.layer().clone();
        let mine: Vec<ClipKey> = clip.iter().filter(|c| c.name == from).cloned().collect();
        edit(core, ed, &layer, "comp.undo.paste_keys", |l| {
            let tr = tracks(l, &target);
            if tr.is_empty() {
                return Err(tr_owned("comp.paste_mismatch"));
            }
            for c in &mine {
                if c.vals.len() != tr.len() {
                    return Err(tr_owned("comp.paste_mismatch"));
                }
                let f = now + c.rel;
                with_keys(l, &target.owner, |k| {
                    for (t, v) in tr.iter().zip(&c.vals) {
                        k.set_key(t, f, *v);
                        k.set_ease(t, Some(&[f]), c.ease);
                    }
                })?;
                new_sel.insert(KeySel { prop: target.clone(), frame: f });
            }
            Ok(())
        });
    }
    if !new_sel.is_empty() {
        ed.comp.sel_keys = new_sel;
    }
}

fn tr_owned(k: &str) -> String {
    tr(k).to_owned()
}

/// Propriedades animáveis de uma camada, na ordem da linha do tempo (transformação, depois cada efeito).
pub(crate) fn all_props(l: &LayerData, layer: &NodeId) -> Vec<PropRef> {
    let mut out: Vec<PropRef> = TRANSFORM.iter().map(|n| PropRef::layer(layer, n)).collect();
    for (i, e) in stack(l).iter().enumerate() {
        for r in effect_rows(&e.effect) {
            out.push(PropRef::effect(layer, i, &r));
        }
    }
    out
}

/// J/K: quadro da chave anterior/seguinte entre as propriedades da camada ativa (visíveis se a camada está aberta).
pub(crate) fn neighbor_key(p: &Project, ed: &super::editor::Editor, forward: bool) -> Option<u32> {
    let layer = ed.layer.clone()?;
    let l = cmds::layer_data(p, &layer)?;
    let mut frames = BTreeSet::new();
    for prop in all_props(l, &layer) {
        if ed.comp.expanded.contains(&layer) && !shows(&ed.comp.reveal, l, &prop) {
            continue;
        }
        frames.extend(key_frames(l, &prop));
    }
    let now = ed.frame;
    if forward { frames.range(now + 1..).next().copied() } else { frames.range(..now).next_back().copied() }
}

/// Revela propriedades (A P S R T; Shift soma ao que já está à mostra; a mesma tecla de novo recolhe).
pub(crate) fn reveal(ed: &mut super::editor::Editor, what: Reveal, add: bool) {
    let Some(layer) = ed.layer.clone() else { return };
    let mut layers: Vec<NodeId> = ed.layer_sel.clone();
    if !layers.contains(&layer) {
        layers.push(layer.clone());
    }
    let st = &mut ed.comp;
    let open = st.expanded.contains(&layer);
    let next = match (&st.reveal, &what, add) {
        (Reveal::Only(cur), Reveal::Only(new), true) if open => {
            let mut s = cur.clone();
            for n in new {
                if !s.remove(n) {
                    s.insert(n);
                }
            }
            if s.is_empty() { None } else { Some(Reveal::Only(s)) }
        }
        (cur, new, _) if open && cur == new => None,
        _ => Some(what),
    };
    match next {
        Some(r) => {
            st.reveal = r;
            st.expanded.extend(layers);
        }
        None => {
            for l in &layers {
                st.expanded.remove(l);
            }
        }
    }
}

/// Atalhos da Composição (só neste workspace, antes dos gerais): ferramentas W/Y/C, A P S R T E U, J/K, F9 e
/// variações, Delete nas chaves, Ctrl+C/Ctrl+V nas chaves e Alt+Shift+(A P S R T) cria chave no quadro.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) || super::text_tool::editing(env.ed) {
        return;
    }
    use egui::{Key, Modifiers as M};
    let has_sel = !env.ed.comp.sel_keys.is_empty();
    let has_clip = !env.ed.comp.clipboard.is_empty();
    struct Hit {
        tool: Option<ToolId>,
        reveal: Option<(Reveal, bool)>,
        make_key: Option<&'static str>,
        jump: i32,
        interp: Option<KeyInterp>,
        delete: bool,
        copy: bool,
        paste: bool,
    }
    let h = ctx.input_mut(|i| {
        let exact = |i: &egui::InputState, m: M| {
            let x = i.modifiers;
            (x.ctrl || x.command) == (m.ctrl || m.command) && x.shift == m.shift && x.alt == m.alt
        };
        let mut take = |m: M, k: Key| exact(i, m) && i.consume_key(m, k);
        let mut h = Hit { tool: None, reveal: None, make_key: None, jump: 0, interp: None, delete: false, copy: false, paste: false };
        for (k, t) in [(Key::W, ToolId::Rotate), (Key::Y, ToolId::Anchor), (Key::C, ToolId::Camera)] {
            if take(M::NONE, k) {
                h.tool = Some(t);
            }
        }
        for (k, name) in [(Key::A, "anchor"), (Key::P, "position"), (Key::S, "scale"), (Key::R, "rotation"), (Key::T, "opacity")] {
            if take(M::SHIFT | M::ALT, k) {
                h.make_key = Some(name);
            } else if take(M::SHIFT, k) {
                h.reveal = Some((Reveal::Only([name].into_iter().collect()), true));
            } else if take(M::NONE, k) {
                h.reveal = Some((Reveal::Only([name].into_iter().collect()), false));
            }
        }
        if take(M::NONE, Key::U) {
            h.reveal = Some((Reveal::Animated, false));
        }
        if take(M::NONE, Key::E) {
            h.reveal = Some((Reveal::Effects, false));
        }
        if take(M::NONE, Key::J) {
            h.jump = -1;
        }
        if take(M::NONE, Key::K) {
            h.jump = 1;
        }
        if take(M::COMMAND | M::SHIFT, Key::F9) {
            h.interp = Some(KeyInterp::EasyEaseOut);
        } else if take(M::SHIFT, Key::F9) {
            h.interp = Some(KeyInterp::EasyEaseIn);
        } else if take(M::NONE, Key::F9) {
            h.interp = Some(KeyInterp::EasyEase);
        }
        if has_sel && (take(M::NONE, Key::Delete) || take(M::NONE, Key::Backspace)) {
            h.delete = true;
        }
        if has_sel && take(M::COMMAND, Key::C) {
            h.copy = true;
        }
        if has_clip && take(M::COMMAND, Key::V) {
            h.paste = true;
        }
        h
    });
    if let Some(t) = h.tool {
        *env.tool = t;
    }
    if let Some((r, add)) = h.reveal {
        reveal(env.ed, r, add);
    }
    if let Some(name) = h.make_key {
        if let Some(layer) = env.ed.layer.clone() {
            let p = PropRef::layer(&layer, name);
            let f = env.ed.frame;
            edit(env.core, env.ed, &layer, "comp.undo.add_key", |l| {
                if animated(l, &p) {
                    let vals = values(l, &p, f);
                    set_values(l, &p, f, &vals)
                } else {
                    toggle_stopwatch(l, &p, f).map(|_| ())
                }
            });
        }
    }
    if h.jump != 0 {
        if let Some(f) = neighbor_key(&env.core.project, env.ed, h.jump > 0) {
            env.ed.playback.playing = false;
            env.ed.frame = f;
        }
    }
    if let Some(i) = h.interp {
        interp_selected(env.core, env.ed, i);
    }
    if h.delete {
        delete_selected(env.core, env.ed);
    }
    if h.copy {
        copy_selected(&env.core.project, env.ed);
    }
    if h.paste {
        paste(env.core, env.ed);
    }
}

/// Faixa (primeiro, último+1) dos quadros com desenho exposto de cada camada do corte (barras de duração). Texto e
/// camadas sem cel (efeito, ajuste, partículas) valem o corte inteiro; pasta não tem barra.
pub(crate) fn duration_bars(core: &CoreSession, cut: &NodeId, ed: &mut super::editor::Editor) -> BTreeMap<NodeId, Option<(u32, u32)>> {
    if let Some((c, k, m)) = &ed.comp.bars {
        if *c == core.change_counter && k == cut {
            return m.clone();
        }
    }
    let p = &core.project;
    let dur = cmds::cut_data(p, cut).map(|c| c.duration).unwrap_or(1);
    let mut out = BTreeMap::new();
    for row in cmds::layer_rows(p, cut) {
        let Some(l) = cmds::layer_data(p, &row.id) else { continue };
        // texto e as camadas sem cel (efeito, ajuste, partículas, 3D) valem o corte inteiro
        let span = if l.kind.holds_cels() && l.kind != LayerType::Text {
            let mut first = None;
            let mut last = None;
            for f in 0..dur {
                if cmds::exposed_cel(p, cut, &row.id, f).is_some() {
                    first.get_or_insert(f);
                    last = Some(f);
                }
            }
            first.zip(last).map(|(a, b)| (a, b + 1))
        } else if l.kind == LayerType::Group {
            None
        } else {
            Some((0, dur))
        };
        out.insert(row.id.clone(), span);
    }
    ed.comp.bars = Some((core.change_counter, cut.clone(), out.clone()));
    out
}

/// Retângulo de tela de uma faixa de quadros na trilha.
pub(crate) fn frame_x(track: Rect, st: &CompState, f: f32) -> f32 {
    track.left() + 6.0 + f * st.px_per_frame - st.scroll_x
}

pub(crate) fn x_frame(track: Rect, st: &CompState, x: f32) -> f32 {
    (x - track.left() - 6.0 + st.scroll_x) / st.px_per_frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;
    use crate::app::editor::Editor;

    fn setup() -> (CoreSession, Editor, NodeId) {
        let mut core = CoreSession::for_tests("comp");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = cmds::quick_start(&mut core, 24, None).unwrap();
        let layer = cmds::add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        let mut ed = Editor::default();
        ed.set_cut(cut);
        ed.layer = Some(layer.clone());
        (core, ed, layer)
    }

    fn layer(core: &CoreSession, id: &NodeId) -> LayerData {
        cmds::layer_data(&core.project, id).unwrap().clone()
    }

    #[test]
    fn stopwatch_keys_values_and_static_values() {
        let (mut core, mut ed, id) = setup();
        let pos = PropRef::layer(&id, "position");
        // sem cronômetro: muda o valor fixo (offset)
        assert!(edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &pos, 3, &[10.0, -4.0])));
        assert_eq!(layer(&core, &id).offset, [10.0, -4.0]);
        assert!(layer(&core, &id).keys.is_empty());
        // liga no quadro 0, valor novo no quadro 12 vira chave
        edit(&mut core, &mut ed, &id, "comp.undo.stopwatch", |l| toggle_stopwatch(l, &pos, 0).map(|_| ()));
        edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &pos, 12, &[30.0, -4.0]));
        let l = layer(&core, &id);
        assert_eq!(key_frames(&l, &pos), vec![0, 12]);
        assert_eq!(values(&l, &pos, 6), vec![20.0, -4.0]);
        // escala e rotação fixas novas, âncora animada
        let sc = PropRef::layer(&id, "scale");
        edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &sc, 0, &[0.5]));
        assert_eq!(layer(&core, &id).scale, 0.5);
        // desligar o cronômetro fixa o valor do quadro atual
        edit(&mut core, &mut ed, &id, "comp.undo.stopwatch", |l| toggle_stopwatch(l, &pos, 6).map(|_| ()));
        let l = layer(&core, &id);
        assert!(!animated(&l, &pos) && l.offset == [20.0, -4.0]);
        // um passo de desfazer por edição
        core.undo().unwrap();
        assert!(animated(&layer(&core, &id), &pos));
    }

    #[test]
    fn moving_keys_and_easy_ease_on_the_selection() {
        let (mut core, mut ed, id) = setup();
        let rot = PropRef::layer(&id, "rotation");
        edit(&mut core, &mut ed, &id, "comp.undo.stopwatch", |l| toggle_stopwatch(l, &rot, 0).map(|_| ()));
        edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &rot, 10, &[90.0]));
        edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &rot, 20, &[0.0]));
        // arrastar a chave do meio 4 quadros para a direita: a seleção acompanha
        ed.comp.sel_keys = [KeySel { prop: rot.clone(), frame: 10 }].into_iter().collect();
        move_selected(&mut core, &mut ed, 4);
        assert_eq!(key_frames(&layer(&core, &id), &rot), vec![0, 14, 20]);
        assert_eq!(ed.comp.sel_keys.iter().next().unwrap().frame, 14);
        assert_eq!(values(&layer(&core, &id), &rot, 14), vec![90.0]);
        // Easy Ease (F9) na chave selecionada: chegada e saída em Bézier, valor da chave intacto
        interp_selected(&mut core, &mut ed, KeyInterp::EasyEase);
        let l = layer(&core, &id);
        let ks = &l.keys.0["rotation"];
        assert!(matches!(ks[0].ease, Ease::Bezier { y2, .. } if y2 == 1.0), "{:?}", ks[0].ease);
        assert!(matches!(ks[1].ease, Ease::Bezier { y1, .. } if y1 == 0.0), "{:?}", ks[1].ease);
        let near = values(&l, &rot, 13)[0];
        assert!(near > 85.0, "chega devagar: {near}");
        // Hold e de volta ao linear
        interp_selected(&mut core, &mut ed, KeyInterp::Hold);
        assert_eq!(values(&layer(&core, &id), &rot, 19), vec![90.0]);
        // copiar e colar no quadro 22; apagar a seleção
        copy_selected(&core.project, &mut ed);
        ed.frame = 22;
        ed.comp.sel_prop = Some(rot.clone());
        paste(&mut core, &mut ed);
        assert_eq!(key_frames(&layer(&core, &id), &rot), vec![0, 14, 20, 22]);
        delete_selected(&mut core, &mut ed);
        assert_eq!(key_frames(&layer(&core, &id), &rot), vec![0, 14, 20]);
        // mover para antes do quadro 0 é recusado sem mexer no documento
        ed.comp.sel_keys = [KeySel { prop: rot.clone(), frame: 0 }, KeySel { prop: rot.clone(), frame: 14 }].into_iter().collect();
        let before = core.change_counter;
        move_selected(&mut core, &mut ed, -20);
        assert_eq!(core.change_counter, before);
    }

    #[test]
    fn effect_params_get_keys_like_the_transform() {
        let (mut core, mut ed, id) = setup();
        crate::app::actions::run(&mut core, "EFFECTS", serde_json::json!({ "op": "add", "layer": id, "effect": { "type": "radial_blur", "center": [32.0, 18.0], "amount": 0.2 } })).unwrap();
        let l = layer(&core, &id);
        assert_eq!(effect_rows(&stack(&l)[0].effect), vec!["amount", "center"]);
        let center = PropRef::effect(&id, 0, "center");
        assert_eq!(tracks(&l, &center), vec!["center.0", "center.1"]);
        edit(&mut core, &mut ed, &id, "comp.undo.stopwatch", |l| toggle_stopwatch(l, &center, 0).map(|_| ()));
        edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &center, 8, &[40.0, 10.0]));
        let l = layer(&core, &id);
        assert_eq!(values(&l, &center, 4), vec![36.0, 14.0]);
        assert_eq!(stack(&l)[0].at_frame(4).unwrap(), aiai_fx::Effect::RadialBlur { center: [36.0, 14.0], amount: 0.2 });
        // valor fixo de parâmetro sem chave
        let amount = PropRef::effect(&id, 0, "amount");
        edit(&mut core, &mut ed, &id, "comp.undo.value", |l| set_values(l, &amount, 3, &[0.5]));
        assert_eq!(values(&layer(&core, &id), &amount, 0), vec![0.5]);
    }

    #[test]
    fn reveal_keys_toggle_like_after_effects() {
        let (_core, mut ed, id) = setup();
        reveal(&mut ed, Reveal::Only(["position"].into_iter().collect()), false);
        assert!(ed.comp.expanded.contains(&id));
        reveal(&mut ed, Reveal::Only(["scale"].into_iter().collect()), true);
        assert_eq!(ed.comp.reveal, Reveal::Only(["position", "scale"].into_iter().collect()));
        // a mesma tecla de novo recolhe
        reveal(&mut ed, Reveal::Animated, false);
        reveal(&mut ed, Reveal::Animated, false);
        assert!(!ed.comp.expanded.contains(&id));
    }
}
