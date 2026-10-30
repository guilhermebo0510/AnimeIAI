//! Barra de comandos e acesso aos filtros (S25, organização inspirada no Clip Studio, com a nossa cara): uma faixa fina de
//! ícones logo abaixo do menu, só nos workspaces de canvas, com os comandos mais usados que não estão em outra barra
//! (novo, abrir, salvar, desfazer, refazer, limpar, limpar fora da seleção, preencher a seleção, transformar, cancelar e
//! inverter a seleção, borda da seleção, encaixe nas réguas) e o botão Filtro. O que é só de visão (grade, réguas à
//! mostra, rotação, espelho) fica na barra do canvas. O mesmo cardápio de filtros
//! fica no menu Filtro da barra de menus. Um filtro é um efeito da camada ativa (não destrutivo, entra no desfazer); ao
//! aplicar, a janela Filtro mostra os parâmetros da pilha da camada com o canvas atualizando ao vivo.
//!
//! O estado do lado direito que não vai para o documento (barra de materiais, espelho vertical do Navegador, miniaturas
//! das camadas, borda da seleção) mora em [`WorkbarState`], um campo do `Editor`.

use super::StudioApp;
use super::sel_ops;
use super::ui_kit::{self, Env};
use crate::i18n::tr;
use crate::icons::{IconId, Icons};
use crate::keymap::Action;
use crate::theme::Tokens;
use aiai_core::LayerType;
use egui::{Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};

/// Estado das peças do lado direito e do topo (não vai para o documento).
pub(crate) struct WorkbarState {
    /// Barra de materiais: categoria aberta, busca e tamanho.
    pub materials: super::materials::MaterialsState,
    /// Mostrar as formigas da seleção (Clip Studio: "Exibir borda da seleção").
    pub sel_border: bool,
    /// Janela Filtro aberta (parâmetros da pilha de efeitos da camada ativa).
    pub filter_open: bool,
    /// Espelho vertical da visão ligado (é guardado à parte: na `View` ele vira espelho horizontal + 180°).
    pub flip_v: bool,
    /// Miniaturas das camadas (painel Camadas).
    pub thumbs: super::layer_thumbs::LayerThumbs,
    /// Passo de expandir/contrair a seleção pelo lançador (px do canvas).
    pub grow_px: f32,
    /// Onde o lançador da seleção ficou no último quadro (um clique nele não conta como desenho).
    pub launcher_rect: Option<egui::Rect>,
    /// Plugin aberto na janela de parâmetros (Filtro > Plugins) e os valores.
    pub plugin: Option<(aiai_tools::plugin::Plugin, std::collections::BTreeMap<String, f32>)>,
}

impl Default for WorkbarState {
    fn default() -> Self {
        WorkbarState { materials: Default::default(), sel_border: true, filter_open: false, flip_v: false, thumbs: Default::default(), grow_px: 4.0, launcher_rect: None, plugin: None }
    }
}

/// Comandos da barra de comandos e do lançador da seleção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Cmd {
    New,
    Open,
    Save,
    Undo,
    Redo,
    /// Limpa o que está dentro da seleção; sem seleção, a camada inteira (no quadro atual).
    Clear,
    ClearOutside,
    FillSelection,
    Transform,
    Deselect,
    InvertSelection,
    SelectionBorder,
    RulerSnap,
    Filter,
    Grow,
    Shrink,
    CopyToLayer,
}

/// O que o comando precisa saber do editor para estar disponível.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CmdCtx {
    pub has_cut: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Há uma área escolhida (retângulo, laço, varinha...).
    pub area: bool,
    /// Há traços escolhidos (Selecionar traços).
    pub picked: bool,
    /// Camada ativa: tipo e travada.
    pub layer: Option<(LayerType, bool)>,
}

impl CmdCtx {
    pub(crate) fn of(core: &aiai_core::CoreSession, ed: &mut super::editor::Editor) -> CmdCtx {
        let has_cut = ed.active_cut(core).is_some();
        let layer = ed.active_layer(core).and_then(|l| super::commands::layer_data(&core.project, &l).map(|d| (d.kind, d.locked)));
        let sel = ed.selection.as_ref().filter(|s| !s.whole);
        CmdCtx {
            has_cut,
            can_undo: core.can_undo(),
            can_redo: core.can_redo(),
            area: sel.is_some_and(|s| s.strokes.is_empty()),
            picked: sel.is_some_and(|s| !s.strokes.is_empty()),
            layer,
        }
    }

    fn editable(&self) -> Option<LayerType> {
        match self.layer {
            Some((k @ (LayerType::Raster | LayerType::Vector), false)) => Some(k),
            _ => None,
        }
    }
}

/// A barra, da esquerda para a direita (`None` = separador).
pub(crate) const BAR: [Option<Cmd>; 17] = [
    Some(Cmd::New),
    Some(Cmd::Open),
    Some(Cmd::Save),
    None,
    Some(Cmd::Undo),
    Some(Cmd::Redo),
    None,
    Some(Cmd::Clear),
    Some(Cmd::ClearOutside),
    Some(Cmd::FillSelection),
    Some(Cmd::Transform),
    None,
    Some(Cmd::Deselect),
    Some(Cmd::InvertSelection),
    Some(Cmd::SelectionBorder),
    None,
    Some(Cmd::RulerSnap),
];

impl Cmd {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Cmd::New => "new",
            Cmd::Open => "open",
            Cmd::Save => "save",
            Cmd::Undo => "undo",
            Cmd::Redo => "redo",
            Cmd::Clear => "clear",
            Cmd::ClearOutside => "clear_outside",
            Cmd::FillSelection => "fill_selection",
            Cmd::Transform => "transform",
            Cmd::Deselect => "deselect",
            Cmd::InvertSelection => "invert_selection",
            Cmd::SelectionBorder => "selection_border",
            Cmd::RulerSnap => "ruler_snap",
            Cmd::Filter => "filter",
            Cmd::Grow => "grow",
            Cmd::Shrink => "shrink",
            Cmd::CopyToLayer => "copy_to_layer",
        }
    }

    pub(crate) fn icon(self) -> IconId {
        match self {
            Cmd::New => IconId::FileNew,
            Cmd::Open => IconId::FileOpen,
            Cmd::Save => IconId::FileSave,
            Cmd::Undo => IconId::Undo,
            Cmd::Redo => IconId::Redo,
            Cmd::Clear => IconId::ClearLayer,
            Cmd::ClearOutside => IconId::ClearOutside,
            Cmd::FillSelection => IconId::FillSelection,
            Cmd::Transform => IconId::Transform,
            Cmd::Deselect => IconId::Deselect,
            Cmd::InvertSelection => IconId::SelectInvert,
            Cmd::SelectionBorder => IconId::SelectionBorder,
            Cmd::RulerSnap => IconId::Snap,
            Cmd::Filter => IconId::Filter,
            Cmd::Grow => IconId::SelGrow,
            Cmd::Shrink => IconId::SelShrink,
            Cmd::CopyToLayer => IconId::CopyToLayer,
        }
    }

    /// Chave do texto da dica.
    pub(crate) fn tip_key(self) -> &'static str {
        match self {
            Cmd::New => "cmdbar.new",
            Cmd::Open => "cmdbar.open",
            Cmd::Save => "cmdbar.save",
            Cmd::Undo => "cmdbar.undo",
            Cmd::Redo => "cmdbar.redo",
            Cmd::Clear => "cmdbar.clear",
            Cmd::ClearOutside => "cmdbar.clear_outside",
            Cmd::FillSelection => "cmdbar.fill_selection",
            Cmd::Transform => "cmdbar.transform",
            Cmd::Deselect => "cmdbar.deselect",
            Cmd::InvertSelection => "cmdbar.invert_selection",
            Cmd::SelectionBorder => "cmdbar.selection_border",
            Cmd::RulerSnap => "cmdbar.ruler_snap",
            Cmd::Filter => "cmdbar.filter",
            Cmd::Grow => "cmdbar.grow",
            Cmd::Shrink => "cmdbar.shrink",
            Cmd::CopyToLayer => "cmdbar.copy_to_layer",
        }
    }

    /// Atalho em uso (o do mapa de atalhos quando o comando tem ação configurável; os fixos do editor nos outros).
    pub(crate) fn shortcut(self, keys: &crate::keymap::Keymap) -> String {
        match self {
            Cmd::New => keys.label(Action::FileNew),
            Cmd::Open => keys.label(Action::FileOpen),
            Cmd::Save => keys.label(Action::FileSave),
            Cmd::Undo => keys.label(Action::EditUndo),
            Cmd::Redo => keys.label(Action::EditRedo),
            Cmd::RulerSnap => keys.label(Action::RulerSnap),
            Cmd::Transform => "Ctrl+T".into(),
            Cmd::Deselect => "Ctrl+D".into(),
            Cmd::InvertSelection => "Ctrl+Shift+I".into(),
            _ => String::new(),
        }
    }

    /// Dica com o atalho entre parênteses.
    pub(crate) fn tip(self, keys: &crate::keymap::Keymap) -> String {
        let k = self.shortcut(keys);
        if k.is_empty() { tr(self.tip_key()).to_owned() } else { format!("{} ({k})", tr(self.tip_key())) }
    }

    /// O comando pode rodar agora?
    pub(crate) fn available(self, c: &CmdCtx) -> bool {
        let sel = c.area || c.picked;
        match self {
            Cmd::New | Cmd::Open | Cmd::Save => true,
            Cmd::Undo => c.can_undo,
            Cmd::Redo => c.can_redo,
            Cmd::Clear => c.has_cut && c.editable().is_some(),
            Cmd::ClearOutside => sel && c.editable().is_some(),
            // preencher pinta pixels: só camada raster, com uma área
            Cmd::FillSelection => c.area && c.editable() == Some(LayerType::Raster),
            Cmd::Transform => c.has_cut && c.editable().is_some(),
            Cmd::Deselect | Cmd::InvertSelection => sel,
            Cmd::Grow | Cmd::Shrink => c.area,
            Cmd::CopyToLayer => sel && matches!(c.layer, Some((LayerType::Raster | LayerType::Vector, _))),
            Cmd::SelectionBorder | Cmd::RulerSnap => c.has_cut,
            Cmd::Filter => c.layer.is_some_and(|(k, _)| filterable(k)),
        }
    }

    /// Estado ligado/desligado dos comandos que alternam (os outros: `None`).
    pub(crate) fn toggled(self, ed: &super::editor::Editor) -> Option<bool> {
        match self {
            Cmd::SelectionBorder => Some(ed.workbar.sel_border),
            Cmd::RulerSnap => Some(ed.rulers.snap),
            _ => None,
        }
    }
}

/// Camadas que levam efeitos (filtros).
pub(crate) fn filterable(k: LayerType) -> bool {
    !matches!(k, LayerType::Guide)
}

/// Botão quadrado só com ícone, no estilo dos botões dos painéis (sem `Env`: a barra fica fora do dock).
pub(crate) fn bar_button(icons: &mut Icons, ui: &mut Ui, t: &Tokens, icon: IconId, tip: &str, selected: bool, enabled: bool, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if selected {
        p.rect(rect, 4.0, t.accent_dim, Stroke::new(1.0, t.accent), StrokeKind::Inside);
    } else if resp.hovered() && enabled {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    let ink = if !enabled {
        t.text_dim.gamma_multiply(0.4)
    } else if selected || resp.hovered() {
        t.text
    } else {
        t.text_dim
    };
    ui_kit::paint_icon(icons, p, icon, rect.shrink((size * 0.2).round()), ink, false);
    if enabled { resp.on_hover_text(tip) } else { resp.on_disabled_hover_text(tip) }
}

fn bar_sep(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 20.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, t.border));
}

// ------------------------------------------------------------------------------------------------ filtros

/// Grupos do cardápio de filtros (chave do título, filtros: chave do nome e efeito com valores iniciais). `w`, `h` =
/// canvas do corte (os efeitos com posição começam no lugar certo).
pub(crate) fn filter_groups(w: f32, h: f32) -> Vec<(&'static str, Vec<(&'static str, Value)>)> {
    let adj = |a: Value| json!({ "type": "adjust", "adjustment": a });
    let preset = |ty: &str| super::panels::satsuei::effect_presets(w, h).into_iter().find(|(_, v)| v["type"] == ty).map(|(k, v)| (k, v));
    let pick = |tys: &[&str]| tys.iter().filter_map(|t| preset(t)).collect::<Vec<_>>();
    // efeitos rápidos: as pilhas prontas de anime (as mesmas do navegador de efeitos da Composição), em lista
    let quick: Vec<(&'static str, Value)> = super::panels::effects_browser::anime_presets(w, h).into_iter().map(|(k, st)| (k, serde_json::to_value(st).unwrap_or(Value::Null))).collect();
    vec![
        ("filter.group.quick", quick),
        (
            "filter.group.correction",
            vec![
                ("filter.levels", adj(json!({ "type": "levels", "in_black": 0.0, "in_white": 1.0, "gamma": 1.0, "out_black": 0.0, "out_white": 1.0 }))),
                ("filter.curves", adj(json!({ "type": "curves", "rgb": [[0.0, 0.0], [0.25, 0.22], [0.75, 0.8], [1.0, 1.0]] }))),
                ("filter.hue_saturation", adj(json!({ "type": "hue_saturation", "hue": 0.0, "saturation": 0.0, "lightness": 0.0 }))),
                ("filter.color_balance", adj(json!({ "type": "color_balance", "shadows": [0.0, 0.0, 0.0], "midtones": [0.0, 0.0, 0.0], "highlights": [0.0, 0.0, 0.0] }))),
                ("filter.brightness_contrast", adj(json!({ "type": "brightness_contrast", "brightness": 0.0, "contrast": 0.0 }))),
                ("filter.gradient_map", adj(json!({ "type": "gradient_map", "stops": [[0.0, [0.08, 0.04, 0.16]], [1.0, [1.0, 0.94, 0.84]]] }))),
                ("filter.threshold", adj(json!({ "type": "threshold", "level": 0.5 }))),
                ("filter.posterize", adj(json!({ "type": "posterize", "levels": 4 }))),
            ]
            .into_iter()
            .chain(pick(&["invert", "grade"]))
            .collect(),
        ),
        ("filter.group.blur", pick(&["gaussian_blur", "directional_blur", "radial_blur", "color_aware_blur"])),
        ("filter.group.light", pick(&["glow", "diffusion", "halation", "transmitted_light", "light_wrap", "rim_light", "god_rays", "lens_flare"])),
        ("filter.group.style", pick(&["drop_shadow", "outline", "color_overlay", "two_tone", "para"])),
        ("filter.group.lens", pick(&["chromatic_aberration", "vignette", "grain", "deband"])),
        // renderizar: linhas de foco e de velocidade numa camada vetorial nova (traços editáveis, não efeito)
        (
            "filter.group.render",
            vec![
                ("filter.focus_lines", json!({ "render": "focus" })),
                ("filter.speed_lines", json!({ "render": "speed" })),
                // efeitos de anime desenhados (auditoria 4.5j) numa camada raster nova
                ("filter.fx_fire", json!({ "render": "anime_fx", "kind": "fire" })),
                ("filter.fx_smoke", json!({ "render": "anime_fx", "kind": "smoke" })),
                ("filter.fx_lightning", json!({ "render": "anime_fx", "kind": "lightning" })),
            ],
        ),
        // cenário (auditoria 4.5c): céu numa camada nova e texturas (plana, ou em perspectiva no laço de 4 cantos)
        (
            "filter.group.sky",
            ["day", "morning", "sunset", "dusk", "night"].into_iter().map(|tod| (sky_key(tod), json!({ "render": "sky", "tod": tod }))).collect(),
        ),
        (
            "filter.group.texture",
            TEXTURES.iter().map(|(key, kind, _, _, _)| (*key, json!({ "render": "texture", "kind": kind }))).collect(),
        ),
    ]
}

fn sky_key(tod: &str) -> &'static str {
    match tod {
        "morning" => "filter.sky.morning",
        "sunset" => "filter.sky.sunset",
        "dusk" => "filter.sky.dusk",
        "night" => "filter.sky.night",
        _ => "filter.sky.day",
    }
}

/// Texturas de cenário: (chave, tipo, especificação, cor escura, cor clara). Cores de partida para ajustar depois.
const TEXTURES: [(&str, &str, &str, [f32; 3], [f32; 3]); 8] = [
    ("filter.tex.brick", "brick", r#"{"type":"brick","brick":[64,26],"mortar":3,"variation":0.25}"#, [0.42, 0.19, 0.14], [0.72, 0.38, 0.28]),
    ("filter.tex.roof", "roof_tiles", r#"{"type":"roof_tiles","tile":[44,30],"variation":0.2}"#, [0.24, 0.22, 0.3], [0.5, 0.46, 0.56]),
    ("filter.tex.wood", "wood", r#"{"type":"wood","scale":120,"rings":9}"#, [0.36, 0.22, 0.12], [0.64, 0.44, 0.26]),
    ("filter.tex.stone", "stone", r#"{"type":"stone","scale":60}"#, [0.36, 0.36, 0.38], [0.7, 0.7, 0.72]),
    ("filter.tex.grass", "grass", r#"{"type":"grass","scale":40,"density":0.7}"#, [0.14, 0.32, 0.12], [0.46, 0.7, 0.3]),
    ("filter.tex.foliage", "foliage", r#"{"type":"foliage","scale":50}"#, [0.1, 0.28, 0.12], [0.4, 0.62, 0.28]),
    ("filter.tex.plaster", "plaster", r#"{"type":"plaster","scale":80}"#, [0.74, 0.71, 0.65], [0.93, 0.91, 0.86]),
    ("filter.tex.paper", "paper", r#"{"type":"paper","scale":40}"#, [0.84, 0.82, 0.77], [0.98, 0.97, 0.94]),
];

/// Uma camada raster nova acima da ativa, com o nome dado e o cel no quadro atual; devolve (corte, camada).
fn new_raster(env: &mut Env, name: &str) -> Option<(super::commands::NodeId, super::commands::NodeId)> {
    let Some(cut) = env.ed.active_cut(env.core) else {
        env.ed.notify(tr("err.no_cut"));
        return None;
    };
    let active = env.ed.active_layer(env.core);
    let layer = match super::commands::add_layer(env.core, &cut, LayerType::Raster, active.as_ref()) {
        Ok(l) => l,
        Err(e) => {
            env.ed.notify(e);
            return None;
        }
    };
    let _ = super::commands::rename_node(env.core, &layer, name);
    env.ed.layer = Some(layer.clone());
    env.ed.cel_number = None;
    Some((cut, layer))
}

/// Céu (dia, manhã, pôr do sol, crepúsculo, noite) numa camada nova, horizonte a 62% da altura.
fn render_sky(env: &mut Env, tod: &str) {
    let Some((cut, layer)) = new_raster(env, &format!("{} ({})", tr("filter.sky"), tr(sky_key(tod)))) else { return };
    let (_, h) = super::commands::canvas_size(&env.core.project, &cut);
    let frame = env.ed.frame;
    let sky: aiai_paint::texture::SkySpec = match serde_json::from_value(json!({ "time_of_day": tod, "horizon_y": h as f32 * 0.62, "seed": env.core.change_counter as u32 })) {
        Ok(s) => s,
        Err(e) => return env.ed.notify(e.to_string()),
    };
    let planned = super::commands::plan_with_prep(env.core, &cut, &layer, frame, None, vec![], |p, blobs| {
        aiai_tools::background::sky_command(p, blobs, &aiai_tools::background::SkyRequest { cut: cut.clone(), frame, layer: layer.clone(), sky }).map_err(|e| e.to_string())
    });
    match planned.and_then(|c| super::commands::run(env.core, c).map(|_| ())) {
        Ok(()) => {}
        Err(e) => env.ed.notify(e),
    }
}

/// Fogo, fumaça ou raio numa camada raster nova, no meio do canvas (com seleção, dentro da caixa dela). Para animar,
/// a bridge/MCP (`ANIME_FX`) desenha um cel por quadro variando `phase`.
fn render_anime_fx(env: &mut Env, kind: &str) {
    let key = match kind {
        "smoke" => "filter.fx_smoke",
        "lightning" => "filter.fx_lightning",
        _ => "filter.fx_fire",
    };
    let sel_box = env.ed.selection.as_ref().filter(|s| !s.whole).and_then(|s| s.rect_of());
    let Some((cut, layer)) = new_raster(env, tr(key)) else { return };
    let (w, h) = super::commands::canvas_size(&env.core.project, &cut);
    let r = sel_box.unwrap_or([w as f32 * 0.25, h as f32 * 0.15, w as f32 * 0.5, h as f32 * 0.75]);
    let fx = match kind {
        "smoke" => json!({ "type": "smoke", "center": [r[0] + r[2] * 0.4, r[1] + r[3] * 0.75], "radius": r[2].min(r[3]) * 0.16, "puffs": 3, "drift": [r[2] * 0.06, -r[3] * 0.22] }),
        "lightning" => json!({ "type": "lightning", "from": [r[0] + r[2] * 0.65, r[1]], "to": [r[0] + r[2] * 0.35, r[1] + r[3]], "width": (r[2].min(r[3]) * 0.03).max(4.0) }),
        _ => json!({ "type": "fire", "base": [r[0] + r[2] * 0.5, r[1] + r[3] * 0.92], "height": r[3] * 0.8 }),
    };
    let frame = env.ed.frame;
    let req: aiai_tools::anime_fx::AnimeFxRequest = match serde_json::from_value(json!({ "cut": cut, "frame": frame, "layer": layer, "kind": fx, "seed": env.core.change_counter })) {
        Ok(r) => r,
        Err(e) => return env.ed.notify(e.to_string()),
    };
    let planned = super::commands::plan_with_prep(env.core, &cut, &layer, frame, None, vec![], |p, blobs| aiai_tools::anime_fx::anime_fx_command(p, blobs, &req).map_err(|e| e.to_string()));
    if let Err(e) = planned.and_then(|c| super::commands::run(env.core, c).map(|_| ())) {
        env.ed.notify(e);
    }
}

/// Textura numa camada nova: dentro da seleção; um laço poligonal de 4 cantos (sup-esq, sup-dir, inf-dir, inf-esq)
/// vira o plano em perspectiva (o padrão diminui com a distância), como uma parede ou um chão.
fn render_texture(env: &mut Env, kind: &str) {
    let Some((key, _, spec, dark, light)) = TEXTURES.iter().find(|t| t.1 == kind).copied() else { return };
    let sel = env.ed.selection.clone().filter(|s| !s.whole);
    let Some((cut, layer)) = new_raster(env, &format!("{} ({})", tr("filter.texture"), tr(key))) else { return };
    let frame = env.ed.frame;
    let quad = sel.as_ref().filter(|s| s.mode == super::editor::SelMode::Lasso && s.pts.len() == 4).map(|s| s.pts.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>());
    let mapping = match &quad {
        Some(q) => {
            let d = |a: usize, b: usize| ((q[a][0] - q[b][0]).powi(2) + (q[a][1] - q[b][1]).powi(2)).sqrt();
            json!({ "type": "perspective", "quad": q, "size": [d(0, 1).max(d(3, 2)), d(0, 3).max(d(1, 2))] })
        }
        None => json!({ "type": "flat", "scale": 1.0 }),
    };
    let region = if quad.is_some() { None } else { sel.as_ref().and_then(|s| s.shape()) };
    let spec: serde_json::Value = serde_json::from_str(spec).unwrap_or(serde_json::Value::Null);
    let req = json!({ "cut": cut, "frame": frame, "layer": layer, "texture": { "spec": spec, "dark": dark, "light": light, "seed": env.core.change_counter as u32 }, "mapping": mapping, "region": region });
    let req: aiai_tools::background::PatternFillRequest = match serde_json::from_value(req) {
        Ok(r) => r,
        Err(e) => return env.ed.notify(e.to_string()),
    };
    let planned = super::commands::plan_with_prep(env.core, &cut, &layer, frame, None, vec![], |p, blobs| aiai_tools::background::pattern_fill_command(p, blobs, &req).map_err(|e| e.to_string()));
    match planned.and_then(|c| super::commands::run(env.core, c).map(|_| ())) {
        Ok(()) => {}
        Err(e) => env.ed.notify(e),
    }
}

/// Aplica o filtro à camada ativa (um passo de desfazer) e abre a janela com os parâmetros. Um efeito entra no fim da
/// pilha; uma lista (efeito rápido) entra inteira; `{"render": ...}` desenha linhas de foco/velocidade numa camada nova.
pub(crate) fn apply_filter(env: &mut Env, effect: Value) {
    if let Some(kind) = effect.get("render").and_then(Value::as_str) {
        return match kind {
            "sky" => render_sky(env, effect.get("tod").and_then(Value::as_str).unwrap_or("day")),
            "texture" => render_texture(env, effect.get("kind").and_then(Value::as_str).unwrap_or("brick")),
            "anime_fx" => render_anime_fx(env, effect.get("kind").and_then(Value::as_str).unwrap_or("fire")),
            _ => render_lines(env, kind),
        };
    }
    let Some(layer) = env.ed.active_layer(env.core) else {
        env.ed.notify(tr("filter.no_layer"));
        return;
    };
    let r = match effect {
        Value::Array(list) => {
            let mut st: Vec<Value> = super::commands::layer_data(&env.core.project, &layer).map(|l| crate::app::comp::stack(l).into_iter().filter_map(|i| serde_json::to_value(i).ok()).collect()).unwrap_or_default();
            st.extend(list);
            super::actions::run(env.core, "EFFECTS", json!({ "op": "set", "layer": layer, "effects": st }))
        }
        e => super::actions::run(env.core, "EFFECTS", json!({ "op": "add", "layer": layer, "effect": e })),
    };
    match r {
        Ok(_) => env.ed.workbar.filter_open = true,
        Err(e) => env.ed.notify(e),
    }
}

/// Renderizar: linhas de foco (para o centro do canvas) ou de velocidade (horizontais) numa camada vetorial nova acima
/// da ativa, com a cor atual. Os traços ficam editáveis (Editar pontos, borracha vetorial).
fn render_lines(env: &mut Env, kind: &str) {
    let Some(cut) = env.ed.active_cut(env.core) else { return env.ed.notify(tr("err.no_cut")) };
    let (w, h) = super::commands::canvas_size(&env.core.project, &cut);
    let (w, h) = (w as f32, h as f32);
    let active = env.ed.active_layer(env.core);
    let layer = match super::commands::add_layer(env.core, &cut, LayerType::Vector, active.as_ref()) {
        Ok(l) => l,
        Err(e) => return env.ed.notify(e),
    };
    let name_key = if kind == "focus" { "filter.focus_lines" } else { "filter.speed_lines" };
    let _ = super::commands::rename_node(env.core, &layer, tr(name_key));
    let color = super::commands::resolve_color(&env.core.project, &env.ed.color);
    let lines_kind = if kind == "focus" { json!({ "type": "focus", "center": [w / 2.0, h / 2.0], "clear": [w * 0.22, h * 0.26] }) } else { json!({ "type": "speed", "angle": 0.0, "area": [0.0, 0.0, w, h] }) };
    let (len, width, count) = if kind == "focus" { ([w * 0.25, w * 0.6], [2.0, 7.0], 160) } else { ([w * 0.15, w * 0.45], [1.5, 5.0], 90) };
    let p = json!({ "cut": cut, "frame": env.ed.frame, "layer": layer, "kind": lines_kind, "count": count, "length": len, "width": width, "color": { "type": "direct", "rgba": color.0 }, "seed": 25 });
    match super::actions::run(env.core, "LINES", p) {
        Ok(_) => {
            env.ed.layer = Some(layer);
            env.ed.cel_number = None;
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Itens do cardápio de filtros (menu Filtro e botão da barra). Devolve o efeito escolhido.
pub(crate) fn filter_menu_items(ui: &mut Ui, notes: &mut Vec<(String, Rect)>, enabled: bool, canvas: (u32, u32)) -> Option<Value> {
    let mut out = None;
    for (group, items) in filter_groups(canvas.0 as f32, canvas.1 as f32) {
        let r = ui.add_enabled_ui(enabled, |ui| {
            ui.menu_button(tr(group), |ui| {
                for (key, effect) in items {
                    let b = ui.button(tr(key));
                    notes.push((format!("menu_item:filter.{}", key.trim_start_matches("fx.e.").trim_start_matches("filter.")), b.rect));
                    if b.clicked() {
                        out = Some(effect);
                        ui.close();
                    }
                }
            })
        });
        notes.push((format!("menu_item:{group}"), r.response.rect));
    }
    out
}

impl StudioApp {
    /// Contexto dos comandos (o que está disponível agora).
    fn cmd_ctx(&mut self) -> CmdCtx {
        CmdCtx::of(&self.core, &mut self.ed)
    }

    /// Roda um comando da barra (os de arquivo e desfazer pelo caminho do menu; os de seleção e camada em `sel_ops`).
    pub(super) fn run_cmd(&mut self, c: Cmd) {
        match c {
            Cmd::New => self.open_new_project(),
            Cmd::Open => self.request_open(),
            Cmd::Save => self.request_save(None),
            Cmd::Undo => self.doc_undo(),
            Cmd::Redo => self.doc_redo(),
            _ => {
                let t = self.tokens();
                let ws = self.ws;
                let mut notes = Vec::new();
                let mut env = Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
                sel_ops::run(&mut env, c);
            }
        }
    }

    /// Barra de comandos (faixa de ícones abaixo do menu), só nos workspaces de canvas.
    pub(super) fn command_bar(&mut self, ui: &mut Ui) {
        if !self.ws.profile().canvas {
            return;
        }
        let t = self.tokens();
        let c = self.cmd_ctx();
        let mut run: Option<Cmd> = None;
        let mut filter: Option<Value> = None;
        let mut notes: Vec<(String, Rect)> = Vec::new();
        let canvas = self.ed.cut.as_ref().map(|cut| super::commands::canvas_size(&self.core.project, cut)).unwrap_or((1920, 1080));
        let bar = egui::Panel::top("cmdbar")
            .exact_size(30.0)
            .frame(egui::Frame::NONE.fill(t.bg_frame).inner_margin(egui::Margin { left: 8, right: 8, top: 2, bottom: 0 }))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for item in BAR {
                        let Some(cmd) = item else {
                            bar_sep(ui, &t);
                            continue;
                        };
                        let on = cmd.toggled(&self.ed).unwrap_or(false);
                        let r = bar_button(&mut self.icons, ui, &t, cmd.icon(), &cmd.tip(&self.ed.keys), on, cmd.available(&c), 26.0);
                        notes.push((format!("cmd:{}", cmd.id()), r.rect));
                        if r.clicked() {
                            run = Some(cmd);
                        }
                    }
                    bar_sep(ui, &t);
                    let enabled = Cmd::Filter.available(&c);
                    let r = bar_button(&mut self.icons, ui, &t, Cmd::Filter.icon(), &Cmd::Filter.tip(&self.ed.keys), self.ed.workbar.filter_open, enabled, 26.0);
                    notes.push(("cmd:filter".into(), r.rect));
                    egui::Popup::menu(&r).show(|ui| {
                        filter = filter_menu_items(ui, &mut notes, true, canvas);
                        ui.separator();
                        let b = ui.add_enabled(self.ed.active_layer(&self.core).is_some(), egui::Button::new(tr("filter.edit")));
                        notes.push(("menu_item:filter.edit".into(), b.rect));
                        if b.clicked() {
                            self.ed.workbar.filter_open = true;
                            ui.close();
                        }
                    });
                });
            });
        self.rects.note("cmdbar", bar.response.rect);
        for (n, r) in notes {
            self.rects.note(n, r);
        }
        if let Some(cmd) = run {
            self.run_cmd(cmd);
        }
        if let Some(e) = filter {
            self.with_env(|env| apply_filter(env, e));
        }
    }

    /// Ambiente dos painéis fora do dock (barra de comandos, menu Filtro, janela Filtro).
    pub(crate) fn with_env<R>(&mut self, f: impl FnOnce(&mut Env) -> R) -> R {
        let t = self.tokens();
        let ws = self.ws;
        let mut notes = Vec::new();
        let r = {
            let mut env = Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
            f(&mut env)
        };
        for (n, rect) in notes {
            self.rects.note(n, rect);
        }
        r
    }

    /// Menu Filtro da barra de menus (os mesmos filtros da barra de comandos), nos workspaces de canvas.
    /// Menu Selecionar (como o do Clip Studio): tudo, cancelar, inverter, expandir/encolher e a cor atual.
    pub(super) fn menu_select(&mut self, ui: &mut Ui) {
        if !self.ws.profile().canvas {
            return;
        }
        let mut notes = Vec::new();
        let mut pick: Option<&'static str> = None;
        let has_sel = self.ed.selection.is_some();
        let has_layer = self.ed.layer.is_some();
        let r = ui.menu_button(tr("menu.select"), |ui| {
            for (key, enabled, shortcut) in [
                ("sel.menu.all", has_layer, "Ctrl+A"),
                ("sel.menu.none", has_sel, "Ctrl+D"),
                ("sel.menu.invert", has_sel, "Ctrl+Shift+I"),
                ("sel.menu.grow", has_sel, ""),
                ("sel.menu.shrink", has_sel, ""),
                ("sel.menu.color", has_layer, ""),
            ] {
                if key == "sel.menu.grow" || key == "sel.menu.color" {
                    ui.separator();
                }
                let b = ui.add_enabled(enabled, egui::Button::new(tr(key)).shortcut_text(shortcut));
                notes.push((format!("menu_item:{key}"), b.rect));
                if b.clicked() {
                    pick = Some(key);
                    ui.close();
                }
            }
        });
        self.rects.note("menu:select", r.response.rect);
        for (n, rect) in notes {
            self.rects.note(n, rect);
        }
        if let Some(k) = pick {
            self.with_env(|env| match k {
                "sel.menu.all" => super::selection::select_all(env),
                "sel.menu.none" => super::sel_ops::run(env, Cmd::Deselect),
                "sel.menu.invert" => super::sel_ops::run(env, Cmd::InvertSelection),
                "sel.menu.grow" => super::sel_ops::run(env, Cmd::Grow),
                "sel.menu.shrink" => super::sel_ops::run(env, Cmd::Shrink),
                _ => super::selection::select_color(env),
            });
        }
    }

    pub(super) fn menu_filter(&mut self, ui: &mut Ui) {
        if !self.ws.profile().canvas {
            return;
        }
        let c = self.cmd_ctx();
        let enabled = Cmd::Filter.available(&c);
        let canvas = self.ed.cut.as_ref().map(|cut| super::commands::canvas_size(&self.core.project, cut)).unwrap_or((1920, 1080));
        let mut notes = Vec::new();
        let mut filter = None;
        let mut edit = false;
        let r = ui.menu_button(tr("menu.filter"), |ui| {
            if !enabled {
                ui.label(egui::RichText::new(tr("filter.no_layer")).size(11.5).color(self.tokens().text_dim));
            }
            filter = filter_menu_items(ui, &mut notes, enabled, canvas);
            // plugins do usuário (auditoria 4.9g): filtros WGSL da pasta de plugins
            let r = ui.menu_button(tr("filter.group.plugins"), |ui| {
                let dir = aiai_tools::plugin::plugins_dir();
                let (list, bad) = dir.as_deref().map(aiai_tools::plugin::list_plugins).unwrap_or_default();
                if list.is_empty() {
                    ui.label(egui::RichText::new(tr("filter.plugins.none")).size(11.5));
                }
                for pl in list {
                    let b = ui.add_enabled(enabled, egui::Button::new(&pl.manifest.name)).on_hover_text(&pl.manifest.description);
                    notes.push((format!("menu_item:plugin:{}", pl.manifest.id), b.rect));
                    if b.clicked() {
                        let vals = pl.manifest.params.iter().map(|p| (p.key.clone(), p.default)).collect();
                        self.ed.workbar.plugin = Some((pl, vals));
                        ui.close();
                    }
                }
                for b in &bad {
                    ui.label(egui::RichText::new(b).size(10.5).color(egui::Color32::from_rgb(220, 120, 100)));
                }
                ui.separator();
                if ui.button(tr("filter.plugins.open_dir")).clicked() {
                    if let Some(d) = &dir {
                        let _ = std::fs::create_dir_all(d);
                        let _ = std::process::Command::new("explorer").arg(d).spawn();
                    }
                    ui.close();
                }
                if ui.button(tr("filter.plugins.install")).clicked() {
                    if let (Some(src), Some(d)) = (rfd::FileDialog::new().set_title(tr("filter.plugins.install")).add_filter("AnimeIAI plugin", &["aiaiplugin"]).pick_file(), &dir) {
                        let _ = std::fs::create_dir_all(d);
                        let to = d.join(src.file_name().unwrap_or_default());
                        match aiai_tools::plugin::load_plugin(&src).and_then(|_| std::fs::copy(&src, &to).map_err(|e| aiai_tools::context::ToolError::Invalid(e.to_string()))) {
                            Ok(_) => self.ed.notify(tr("filter.plugins.installed")),
                            Err(e) => self.ed.notify(e.to_string()),
                        }
                    }
                    ui.close();
                }
            });
            notes.push(("menu_item:filter.group.plugins".into(), r.response.rect));
            ui.separator();
            let b = ui.add_enabled(enabled, egui::Button::new(tr("filter.edit")));
            notes.push(("menu_item:filter.edit".into(), b.rect));
            if b.clicked() {
                edit = true;
                ui.close();
            }
        });
        self.rects.note("menu:filter", r.response.rect);
        for (n, rect) in notes {
            self.rects.note(n, rect);
        }
        if edit {
            self.ed.workbar.filter_open = true;
        }
        if let Some(e) = filter {
            self.with_env(|env| apply_filter(env, e));
        }
    }

    /// Janela do plugin (Filtro > Plugins): os parâmetros do manifesto e Aplicar na camada raster ativa.
    pub(super) fn plugin_window(&mut self, ctx: &egui::Context) {
        let Some((pl, mut vals)) = self.ed.workbar.plugin.take() else { return };
        let mut open = true;
        let mut apply = false;
        let mut notes = Vec::new();
        let win = egui::Window::new(&pl.manifest.name).id(egui::Id::new("aiai_plugin_window")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            if !pl.manifest.description.is_empty() {
                ui.label(egui::RichText::new(&pl.manifest.description).size(11.5));
            }
            for p in &pl.manifest.params {
                let v = vals.entry(p.key.clone()).or_insert(p.default);
                let (lo, hi) = (p.min.min(p.max), p.max.max(p.min));
                let r = ui.add(egui::Slider::new(v, lo..=hi).text(if p.label.is_empty() { &p.key } else { &p.label }));
                notes.push((format!("plugin:param:{}", p.key), r.rect));
            }
            let b = ui.button(tr("filter.plugins.apply"));
            notes.push(("plugin:apply".to_string(), b.rect));
            apply = b.clicked();
        });
        if let Some(w) = win {
            self.rects.note("dialog:plugin", w.response.rect);
        }
        for (n, r) in notes {
            self.rects.note(n, r);
        }
        if apply {
            let frame = self.ed.frame;
            let (Some(cut), Some(layer)) = (self.ed.cut.clone(), self.ed.active_layer(&self.core)) else {
                self.ed.notify(tr("filter.no_layer"));
                self.ed.workbar.plugin = Some((pl, vals));
                return;
            };
            let req = aiai_tools::plugin::PluginFilterRequest { cut: cut.clone(), frame, layer: layer.clone(), plugin: pl.path.display().to_string(), params: vals.clone() };
            let planned = super::commands::plan_with_prep(&mut self.core, &cut, &layer, frame, None, vec![], |p, blobs| aiai_tools::plugin::plugin_filter_command(p, blobs, &req).map(|(c, _)| c).map_err(|e| e.to_string()));
            match planned.and_then(|c| super::commands::run(&mut self.core, c).map(|_| ())) {
                Ok(()) => self.ed.notify(tr("filter.plugins.applied").replace("{name}", &pl.manifest.name)),
                Err(e) => self.ed.notify(e),
            }
        }
        if open {
            self.ed.workbar.plugin = Some((pl, vals));
        }
    }

    /// Janela Filtro: a pilha de efeitos da camada ativa com os parâmetros (o canvas atualiza ao vivo).
    pub(super) fn filter_window(&mut self, ctx: &egui::Context) {
        if !self.ed.workbar.filter_open || !self.ws.profile().canvas {
            return;
        }
        let mut open = true;
        let t = self.tokens();
        let ws = self.ws;
        let mut notes = Vec::new();
        let win = egui::Window::new(tr("filter.window"))
            .id(egui::Id::new("aiai_filter_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([320.0, 420.0])
            .pivot(egui::Align2::RIGHT_TOP)
            .default_pos(ctx.content_rect().right_top() + egui::vec2(-380.0, 120.0))
            .show(ctx, |ui| {
                let size = ui.available_size().max(egui::vec2(280.0, 340.0));
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let mut env = Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
                super::panels::effect_controls::show(&mut env, ui, rect);
            });
        if let Some(w) = win {
            self.rects.note("dialog:filter", w.response.rect);
        }
        for (n, rect) in notes {
            self.rects.note(n, rect);
        }
        if !open {
            self.ed.workbar.filter_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> CmdCtx {
        CmdCtx { has_cut: true, can_undo: false, can_redo: false, area: false, picked: false, layer: Some((LayerType::Raster, false)) }
    }

    #[test]
    fn commands_are_available_only_when_they_can_run() {
        let c = ctx();
        assert!(Cmd::New.available(&c) && Cmd::Open.available(&c) && Cmd::Save.available(&c));
        assert!(!Cmd::Undo.available(&c) && !Cmd::Redo.available(&c));
        assert!(Cmd::Undo.available(&CmdCtx { can_undo: true, ..c }));
        // sem seleção: limpar a camada e transformar valem; o que depende da seleção não
        assert!(Cmd::Clear.available(&c) && Cmd::Transform.available(&c));
        for k in [Cmd::ClearOutside, Cmd::FillSelection, Cmd::Deselect, Cmd::InvertSelection, Cmd::Grow, Cmd::Shrink, Cmd::CopyToLayer] {
            assert!(!k.available(&c), "{k:?}");
        }
        let sel = CmdCtx { area: true, ..c };
        for k in [Cmd::ClearOutside, Cmd::FillSelection, Cmd::Deselect, Cmd::InvertSelection, Cmd::Grow, Cmd::Shrink, Cmd::CopyToLayer] {
            assert!(k.available(&sel), "{k:?}");
        }
        // camada travada: nada que mexe em pixels
        let locked = CmdCtx { layer: Some((LayerType::Raster, true)), ..sel };
        assert!(!Cmd::Clear.available(&locked) && !Cmd::FillSelection.available(&locked) && !Cmd::Transform.available(&locked));
        // vetor: preencher a seleção não (pinta pixels); traços escolhidos não são área (expandir não vale)
        let vec_pick = CmdCtx { area: false, picked: true, layer: Some((LayerType::Vector, false)), ..c };
        assert!(!Cmd::FillSelection.available(&vec_pick) && !Cmd::Grow.available(&vec_pick));
        assert!(Cmd::Clear.available(&vec_pick) && Cmd::ClearOutside.available(&vec_pick) && Cmd::Deselect.available(&vec_pick));
        // sem corte: só arquivo
        let none = CmdCtx::default();
        assert!(Cmd::New.available(&none) && !Cmd::Clear.available(&none) && !Cmd::RulerSnap.available(&none) && !Cmd::Filter.available(&none));
        // filtro precisa de camada que leve efeito
        assert!(Cmd::Filter.available(&c) && !Cmd::Filter.available(&CmdCtx { layer: Some((LayerType::Guide, false)), ..c }));
    }

    #[test]
    fn bar_items_have_texts_icons_and_shortcuts() {
        let keys = crate::keymap::Keymap::default();
        let mut ids = std::collections::HashSet::new();
        for c in BAR.iter().flatten().copied().chain([Cmd::Filter, Cmd::Grow, Cmd::Shrink, Cmd::CopyToLayer]) {
            assert!(ids.insert(c.id()), "{c:?}");
            for lang in [crate::i18n::Lang::Pt, crate::i18n::Lang::En] {
                assert_ne!(crate::i18n::tr_in(lang, c.tip_key()), c.tip_key());
            }
        }
        // os atalhos que a dica promete são os do mapa
        assert_eq!(Cmd::Undo.shortcut(&keys), keys.label(Action::EditUndo));
        assert!(Cmd::Undo.tip(&keys).contains('('), "{}", Cmd::Undo.tip(&keys));
        assert_eq!(Cmd::Transform.shortcut(&keys), "Ctrl+T");
    }

    #[test]
    fn filters_go_to_the_active_layer_and_render_lines_make_a_vector_layer() {
        let mut core = aiai_core::CoreSession::for_tests("filtro");
        let (cut, line) = super::super::commands::quick_start_with_layers(&mut core, 6, Some([320, 180])).unwrap();
        let mut ed = super::super::editor::Editor::default();
        ed.cut = Some(cut.clone());
        ed.layer = Some(line.clone());
        let (mut icons, mut notes, mut tool) = (crate::icons::Icons::default(), Vec::new(), crate::workspace::ToolId::Brush);
        let mut env = Env { t: crate::theme::Tokens::dark(), core: &mut core, ed: &mut ed, icons: &mut icons, notes: &mut notes, tool: &mut tool, ws: crate::workspace::WorkspaceId::Pintura };
        let groups = filter_groups(320.0, 180.0);
        let blur = groups.iter().flat_map(|(_, i)| i).find(|(k, _)| *k == "fx.e.gaussian_blur").unwrap().1.clone();
        apply_filter(&mut env, blur);
        let stack = |env: &Env, l: &aiai_core::Id| crate::app::comp::stack(super::super::commands::layer_data(&env.core.project, l).unwrap()).len();
        assert_eq!(stack(&env, &line), 1);
        assert!(env.ed.workbar.filter_open, "a janela Filtro abre para ajustar");
        // efeito rápido: a pilha inteira entra de uma vez, num passo só
        let before = env.core.history_commands().0.len();
        let quick = groups.iter().find(|(g, _)| *g == "filter.group.quick").unwrap().1[0].1.clone();
        let n = quick.as_array().unwrap().len();
        apply_filter(&mut env, quick);
        assert_eq!(stack(&env, &line), 1 + n);
        assert_eq!(env.core.history_commands().0.len(), before + 1);
        // linhas de foco: camada vetorial nova, ativa, com traços
        apply_filter(&mut env, json!({ "render": "focus" }));
        let l = env.ed.layer.clone().unwrap();
        assert_ne!(l, line);
        let ld = super::super::commands::layer_data(&env.core.project, &l).unwrap();
        assert_eq!(ld.kind, LayerType::Vector);
        let cel = super::super::commands::exposed_cel(&env.core.project, &cut, &l, 0).unwrap();
        assert!(super::super::commands::cel_data(&env.core.project, &cel).unwrap().strokes.len() > 50);
    }

    #[test]
    fn filter_menu_covers_every_effect_once_and_all_validate() {
        let groups = filter_groups(1920.0, 1080.0);
        let mut seen = std::collections::HashSet::new();
        for (g, items) in &groups {
            assert!(!items.is_empty(), "{g}");
            for lang in [crate::i18n::Lang::Pt, crate::i18n::Lang::En] {
                assert_ne!(crate::i18n::tr_in(lang, g), *g);
            }
            for (k, v) in items {
                assert_ne!(crate::i18n::tr(k), *k, "texto ausente: {k}");
                assert!(seen.insert(k.to_string()), "repetido: {k}");
                if v.get("render").is_some() {
                    continue;
                }
                // efeito rápido: uma pilha de instâncias; o resto, um efeito só
                let list: Vec<aiai_fx::EffectInstance> = match v {
                    Value::Array(_) => serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{k}: {e}")),
                    _ => vec![serde_json::from_value(json!({ "effect": v })).unwrap_or_else(|e| panic!("{k}: {e}"))],
                };
                assert!(!list.is_empty(), "{k}");
                for inst in list {
                    inst.validate().unwrap_or_else(|e| panic!("{k}: {e}"));
                }
            }
        }
        // todos os efeitos do menu "Adicionar efeito" (menos o LUT, que precisa de arquivo) estão no cardápio
        for (k, v) in super::super::panels::satsuei::effect_presets(1920.0, 1080.0) {
            if v["type"] == "adjust" {
                continue;
            }
            assert!(seen.contains(k), "{k} fora do menu Filtro");
        }
    }
}
