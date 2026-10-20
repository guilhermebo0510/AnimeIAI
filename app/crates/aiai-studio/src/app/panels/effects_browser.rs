//! Painel Efeitos e predefinições da Composição (S24 3.4), como o Effects & Presets do After Effects: busca, favoritos
//! (★), árvore por categoria com todos os efeitos do `aiai-fx` e uma miniatura de cada um (o efeito aplicado a uma
//! amostra fixa, feita uma vez e guardada; no máximo duas por quadro), as predefinições de anime (pilhas prontas) e as
//! do usuário (pilha da camada selecionada salva em `Documentos\AnimeIAI\Predefinições\*.aiaifx`).
//!
//! Aplicar: arrastar o item para uma camada da Linha do tempo da composição ou para o viewer (vale a camada
//! selecionada), ou duplo clique (camada selecionada). Tudo passa pela ação `EFFECTS` da bridge (validada, um passo de
//! desfazer). Com `--no-session` nada é lido nem gravado no disco (favoritos e predefinições ficam só na memória).

use crate::app::actions;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, dim_label, paint_icon, section};
use crate::i18n::tr;
use crate::icons::IconId;
use aiai_fx::EffectInstance;
use egui::{Rect, Sense, Ui};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Categorias da árvore (chave i18n, tipos de efeito na ordem do menu).
pub(crate) const CATEGORIES: &[(&str, &[&str])] = &[
    ("comp.cat.blur", &["gaussian_blur", "directional_blur", "radial_blur", "color_aware_blur"]),
    ("comp.cat.light", &["glow", "diffusion", "halation", "transmitted_light", "light_wrap", "rim_light", "god_rays", "lens_flare"]),
    ("comp.cat.color", &["grade", "adjust", "color_overlay", "para", "two_tone", "invert", "lut"]),
    ("comp.cat.style", &["drop_shadow", "outline"]),
    ("comp.cat.lens", &["chromatic_aberration", "vignette", "grain", "deband"]),
];

/// O que se arrasta do navegador.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FxPayload {
    /// Um efeito pelo tipo (valores iniciais de `satsuei::effect_presets`).
    Effect(String),
    /// Uma pilha pronta (predefinição de anime ou do usuário).
    Stack { name: String, effects: Vec<EffectInstance> },
}

/// Predefinição do usuário (arquivo `.aiaifx`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct UserPreset {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub author: String,
    pub effects: Vec<EffectInstance>,
    /// Arquivo de onde veio (não vai para o disco).
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

pub(crate) const PRESET_FORMAT: &str = "aiai-fx-preset";
pub(crate) const PRESET_EXT: &str = "aiaifx";

/// Pasta das predefinições: `Documentos\AnimeIAI\Predefinições` (a Documentos do sistema, como a de pincéis).
pub(crate) fn presets_dir() -> Option<PathBuf> {
    aiai_paint::userdirs::animeiai_dir().map(|d| d.join("Predefinições"))
}

/// Nome de arquivo seguro a partir do nome da predefinição.
pub(crate) fn file_stem(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let s = s.trim().to_owned();
    if s.is_empty() { "predefinicao".to_owned() } else { s.chars().take(60).collect() }
}

/// Lê as predefinições de uma pasta (arquivos ilegíveis viram aviso; não somem calados).
pub(crate) fn load_presets(dir: &Path) -> (Vec<UserPreset>, Vec<String>) {
    let mut out = Vec::new();
    let mut warn = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (out, warn) };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == PRESET_EXT)).collect();
    files.sort();
    for f in files {
        match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<UserPreset>(&s).map_err(|e| e.to_string())) {
            Ok(mut p) if p.format == PRESET_FORMAT => {
                match p.effects.iter().try_for_each(|e| e.validate()) {
                    Ok(()) => {
                        p.path = Some(f);
                        out.push(p);
                    }
                    Err(e) => warn.push(format!("{}: {e}", f.display())),
                }
            }
            Ok(_) => warn.push(format!("{}: {}", f.display(), tr("comp.preset_bad_format"))),
            Err(e) => warn.push(format!("{}: {e}", f.display())),
        }
    }
    (out, warn)
}

/// Grava a predefinição na pasta (nome repetido ganha sufixo). Devolve o caminho.
pub(crate) fn save_preset(dir: &Path, p: &UserPreset) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = file_stem(&p.name);
    let mut path = dir.join(format!("{stem}.{PRESET_EXT}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{PRESET_EXT}"));
        n += 1;
    }
    let text = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Predefinições de anime prontas (pilhas). `w`, `h` = canvas do corte.
pub(crate) fn anime_presets(w: f32, h: f32) -> Vec<(&'static str, Vec<EffectInstance>)> {
    let inst = |v: Value| -> EffectInstance { serde_json::from_value(json!({ "effect": v })).expect("predefinição válida") };
    vec![
        ("comp.preset.soft_focus", vec![inst(json!({ "type": "diffusion", "radius": 14.0, "amount": 0.35, "threshold": 0.45 })), inst(json!({ "type": "halation", "threshold": 0.85, "radius": 10.0, "intensity": 0.35 }))]),
        ("comp.preset.magic_glow", vec![inst(json!({ "type": "transmitted_light", "radius": 18.0, "intensity": 1.0, "color": [1.0, 0.85, 0.6] })), inst(json!({ "type": "glow", "threshold": 0.6, "radius": 24.0, "intensity": 0.7 }))]),
        ("comp.preset.impact", vec![inst(json!({ "type": "two_tone", "threshold": 0.5, "dark": [0.06, 0.03, 0.12], "light": [1.0, 0.96, 0.9] })), inst(json!({ "type": "grain", "amount": 0.06, "size": 1.5 }))]),
        ("comp.preset.film", vec![inst(json!({ "type": "chromatic_aberration", "amount": 1.5 })), inst(json!({ "type": "vignette", "amount": 0.3, "softness": 0.6 })), inst(json!({ "type": "grain", "amount": 0.035, "size": 1.0 }))]),
        ("comp.preset.sunset", vec![inst(json!({ "type": "para", "from": [w * 0.5, 0.0], "to": [w * 0.5, h], "color_from": [1.0, 0.55, 0.25, 0.45], "color_to": [0.4, 0.2, 0.6, 0.0], "mode": "overlay" })), inst(json!({ "type": "god_rays", "center": [w * 0.7, h * 0.15], "threshold": 0.7, "length": 0.3, "intensity": 0.5 }))]),
    ]
}

/// Efeito com valores iniciais para o canvas (`None` para o LUT, que precisa de um arquivo .cube).
pub(crate) fn default_effect(ty: &str, w: f32, h: f32) -> Option<Value> {
    super::satsuei::effect_presets(w, h).into_iter().find(|(_, v)| v["type"] == ty).map(|(_, v)| v)
}

fn label(ty: &str) -> String {
    let k = format!("fx.e.{ty}");
    let s = tr(&k);
    if s == k { ty.replace('_', " ") } else { s.to_owned() }
}

/// Aplica o que veio do navegador à camada (um passo de desfazer).
pub(crate) fn apply(env: &mut Env, layer: &NodeId, what: &FxPayload) {
    let Some(cut) = env.ed.cut.clone() else { return };
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    let r = match what {
        FxPayload::Effect(ty) => match default_effect(ty, w as f32, h as f32) {
            Some(e) => actions::run(env.core, "EFFECTS", json!({ "op": "add", "layer": layer, "effect": e })),
            None => Err(tr("comp.lut_needs_file").to_owned()),
        },
        FxPayload::Stack { effects, .. } => {
            let mut st = cmds::layer_data(&env.core.project, layer).map(crate::app::comp::stack).unwrap_or_default();
            st.extend(effects.iter().cloned());
            actions::run(env.core, "EFFECTS", json!({ "op": "set", "layer": layer, "effects": st }))
        }
    };
    if actions::notify_err(env.ed, r).is_some() {
        env.ed.layer = Some(layer.clone());
    }
}

/// Aplica na camada selecionada (duplo clique) ou avisa que falta camada.
fn apply_selected(env: &mut Env, what: &FxPayload) {
    match env.ed.active_layer(env.core) {
        Some(l) => apply(env, &l, what),
        None => env.ed.notify(tr("comp.select_layer_first").to_owned()),
    }
}

fn favorites_path() -> Option<PathBuf> {
    presets_dir().map(|d| d.join("favoritos.json"))
}

/// Lê favoritos e predefinições do usuário uma vez (só com sessão).
fn ensure_loaded(env: &mut Env) {
    if env.ed.comp.presets_loaded {
        return;
    }
    env.ed.comp.presets_loaded = true;
    if !env.ed.comp.user_files {
        return;
    }
    if let Some(dir) = presets_dir() {
        let (p, warn) = load_presets(&dir);
        env.ed.comp.presets = p;
        for w in warn {
            env.ed.notify(w);
        }
    }
    if let Some(f) = favorites_path() {
        if let Ok(s) = std::fs::read_to_string(&f) {
            match serde_json::from_str::<Vec<String>>(&s) {
                Ok(v) => env.ed.comp.favorites = v.into_iter().collect(),
                Err(e) => env.ed.notify(format!("{}: {e}", f.display())),
            }
        }
    }
}

fn toggle_favorite(env: &mut Env, ty: &str) {
    let fav = &mut env.ed.comp.favorites;
    if !fav.remove(ty) {
        fav.insert(ty.to_owned());
    }
    if env.ed.comp.user_files {
        if let Some(f) = favorites_path() {
            let list: Vec<&String> = env.ed.comp.favorites.iter().collect();
            let r = f.parent().map(std::fs::create_dir_all).transpose().map_err(|e| e.to_string()).and_then(|_| std::fs::write(&f, serde_json::to_string_pretty(&list).unwrap_or_default()).map_err(|e| e.to_string()));
            if let Err(e) = r {
                env.ed.notify(format!("{}: {e}", f.display()));
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------ miniaturas

/// Amostra das miniaturas (2× do tamanho mostrado): fundo de céu (o `below` do light wrap) e uma forma de cel com luz e
/// linha, em transparente.
fn sample(w: u32, h: u32) -> (aiai_paint::Image, aiai_paint::Image) {
    let mut bg = aiai_paint::Image::new(w, h);
    let mut fg = aiai_paint::Image::new(w, h);
    let (cx, cy, r) = (w as f32 * 0.5, h as f32 * 0.55, h as f32 * 0.32);
    for y in 0..h {
        for x in 0..w {
            let t = y as f32 / h as f32;
            let i = bg.idx(x, y);
            bg.data[i] = [0.18 + 0.3 * t, 0.22 + 0.25 * t, 0.42 + 0.2 * t, 1.0];
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                // cel laranja com um realce branco e a linha escura na borda
                let hl = ((x as f32 - cx + r * 0.35).powi(2) + (y as f32 - cy + r * 0.35).powi(2)).sqrt() < r * 0.28;
                let line = d > r - 2.5;
                let c = if line { [0.1, 0.06, 0.08] } else if hl { [1.0, 0.98, 0.92] } else { [0.95, 0.55, 0.25] };
                fg.data[i] = [c[0] * a, c[1] * a, c[2] * a, a];
            }
        }
    }
    (bg, fg)
}

/// Miniatura de um efeito: aplicado à amostra e composto sobre o fundo. `None` se o efeito não tem valor inicial (LUT).
fn thumb_image(ty: &str) -> Option<egui::ColorImage> {
    let (w, h) = (144u32, 96u32);
    let e: aiai_fx::Effect = serde_json::from_value(default_effect(ty, w as f32, h as f32)?).ok()?;
    let (bg, mut fg) = sample(w, h);
    let blobs = aiai_core::BlobStore::default();
    let ctx = aiai_fx::FxContext { frame: 0, below: Some(&bg), blobs: &blobs, seed: 7 };
    // efeitos de quadro inteiro (cor, lente) mostram melhor sobre o fundo composto
    let whole = matches!(ty, "grade" | "adjust" | "vignette" | "grain" | "deband" | "chromatic_aberration" | "invert" | "two_tone" | "color_overlay" | "god_rays" | "lens_flare" | "diffusion" | "halation");
    let mut out = bg.clone();
    if whole {
        out.draw_over(&fg, 0, 0, 1.0);
        e.apply(&mut out, &ctx).ok()?;
    } else {
        e.apply(&mut fg, &ctx).ok()?;
        out.draw_over(&fg, 0, 0, 1.0);
    }
    let rgba = out.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba))
}

/// Textura da miniatura (cache; cria no máximo `budget` por quadro).
fn thumb(env: &mut Env, ctx: &egui::Context, ty: &str, budget: &mut u32) -> Option<egui::TextureId> {
    if let Some(t) = env.ed.comp.thumbs.get(ty) {
        return Some(t.id());
    }
    if *budget == 0 {
        ctx.request_repaint();
        return None;
    }
    *budget -= 1;
    let img = thumb_image(ty)?;
    let tex = ctx.load_texture(format!("fx_thumb_{ty}"), img, egui::TextureOptions::LINEAR);
    let id = tex.id();
    env.ed.comp.thumbs.insert(ty.to_owned(), tex);
    Some(id)
}

// ------------------------------------------------------------------------------------------------ painel

/// Uma linha clicável/arrastável do navegador. Devolve a resposta.
fn item_row(env: &mut Env, ui: &mut Ui, name: &str, note: &str, thumb: Option<egui::TextureId>, star: Option<bool>, enabled: bool) -> (egui::Response, bool) {
    let t = env.t;
    let h = 30.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), if enabled { Sense::click_and_drag() } else { Sense::hover() });
    env.note(note, rect);
    let p = ui.painter_at(rect);
    if resp.hovered() && enabled {
        p.rect_filled(rect, 4.0, t.bg_hover);
    }
    let th = Rect::from_min_size(rect.min + egui::vec2(2.0, 3.0), egui::vec2(36.0, 24.0));
    match thumb {
        Some(id) => {
            p.image(id, th, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(th, 3.0, t.bg_widget);
        }
    }
    let ink = if enabled { t.text } else { t.text_dim.gamma_multiply(0.6) };
    p.text(egui::pos2(th.right() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(12.5), ink);
    let mut star_clicked = false;
    if let Some(on) = star {
        let sr = Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), egui::vec2(18.0, 18.0));
        let sresp = ui.interact(sr, ui.id().with(("star", note)), Sense::click());
        env.note(format!("{note}:star"), sr);
        if on || sresp.hovered() || resp.hovered() {
            paint_icon(env.icons, &p, IconId::Star, sr.shrink(2.0), if on { t.accent } else { t.text_dim }, false);
        }
        star_clicked = sresp.on_hover_text(tr(if on { "comp.unfavorite" } else { "comp.favorite" })).clicked();
    }
    (resp, star_clicked)
}

/// Começa o arrasto de um item e desenha o rótulo que segue o cursor.
fn drag_feedback(env: &mut Env, ui: &Ui, resp: &egui::Response, payload: FxPayload, name: &str) {
    if resp.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), payload.clone());
        env.ed.comp.dragging_fx = Some(payload);
    }
    if resp.dragged() {
        if let Some(p) = ui.ctx().pointer_interact_pos() {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("fx_drag")));
            let g = painter.layout_no_wrap(format!("+ {name}"), egui::FontId::proportional(12.5), env.t.text);
            let r = Rect::from_min_size(p + egui::vec2(14.0, 6.0), g.size() + egui::vec2(10.0, 6.0));
            painter.rect_filled(r, 4.0, env.t.bg_panel);
            painter.rect_stroke(r, 4.0, egui::Stroke::new(1.0, env.t.accent), egui::StrokeKind::Inside);
            painter.galley(r.min + egui::vec2(5.0, 3.0), g, env.t.text);
        }
    }
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    ensure_loaded(env);
    if !ui.ctx().input(|i| i.pointer.any_down()) {
        env.ed.comp.dragging_fx = None;
    }
    // busca
    let mut q = env.ed.comp.search.clone();
    let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text(tr("comp.search_hint")).desired_width(f32::INFINITY));
    env.note("fxb:search", r.rect);
    if q != env.ed.comp.search {
        env.ed.comp.search = q.clone();
    }
    let ql = q.trim().to_lowercase();
    let matches = |ty: &str| ql.is_empty() || ty.contains(&ql) || label(ty).to_lowercase().contains(&ql);
    let layer = env.ed.active_layer(env.core);
    let layer_name = layer.as_ref().and_then(|l| env.core.project.node(l)).map(|n| n.name.clone());
    dim_label(ui, &t, &match &layer_name {
        Some(n) => format!("{} {n}", tr("comp.apply_to")),
        None => tr("comp.select_layer_first").to_owned(),
    });
    let ctx = ui.ctx().clone();
    let mut budget = 2u32;
    let (w, h) = env.ed.cut.as_ref().map(|c| cmds::canvas_size(&env.core.project, c)).unwrap_or((1920, 1080));
    egui::ScrollArea::vertical().id_salt("fxb_scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        // favoritos
        let favs: Vec<String> = env.ed.comp.favorites.iter().filter(|f| matches(f)).cloned().collect();
        if !favs.is_empty() {
            section(ui, &t, "comp.cat.favorites");
            for ty in favs {
                effect_item(env, ui, &ctx, &ty, w as f32, h as f32, &mut budget, "fav");
            }
        }
        for (cat, types) in CATEGORIES {
            let shown: Vec<&str> = types.iter().copied().filter(|t| matches(t)).collect();
            if shown.is_empty() {
                continue;
            }
            let open = if ql.is_empty() { None } else { Some(true) };
            let resp = egui::CollapsingHeader::new(tr(cat)).id_salt(("fxb_cat", *cat)).default_open(true).open(open).show(ui, |ui| {
                for ty in shown {
                    effect_item(env, ui, &ctx, ty, w as f32, h as f32, &mut budget, "e");
                }
            });
            env.note(format!("fxb:cat:{}", cat.trim_start_matches("comp.cat.")), resp.header_response.rect);
        }
        // pilhas prontas de anime
        let anime: Vec<(&str, Vec<EffectInstance>)> = anime_presets(w as f32, h as f32).into_iter().filter(|(k, _)| ql.is_empty() || tr(k).to_lowercase().contains(&ql)).collect();
        if !anime.is_empty() {
            let open = if ql.is_empty() { None } else { Some(true) };
            egui::CollapsingHeader::new(tr("comp.cat.anime")).id_salt("fxb_anime").default_open(true).open(open).show(ui, |ui| {
                for (k, effects) in anime {
                    let name = tr(k).to_owned();
                    let (resp, _) = item_row(env, ui, &name, &format!("fxb:anime:{}", k.trim_start_matches("comp.preset.")), None, None, true);
                    let payload = FxPayload::Stack { name: name.clone(), effects: effects.clone() };
                    let tip = effects.iter().map(|e| label(e.effect.name())).collect::<Vec<_>>().join(" + ");
                    let resp = resp.on_hover_text(tip);
                    drag_feedback(env, ui, &resp, payload.clone(), &name);
                    if resp.double_clicked() {
                        apply_selected(env, &payload);
                    }
                }
            });
        }
        // do usuário
        let mine: Vec<UserPreset> = env.ed.comp.presets.iter().filter(|p| ql.is_empty() || p.name.to_lowercase().contains(&ql)).cloned().collect();
        egui::CollapsingHeader::new(tr("comp.cat.mine")).id_salt("fxb_mine").default_open(true).show(ui, |ui| {
            if mine.is_empty() {
                dim_label(ui, &t, tr("comp.no_user_presets"));
            }
            for (i, p) in mine.iter().enumerate() {
                let (resp, _) = item_row(env, ui, &p.name, &format!("fxb:mine:{i}"), None, None, true);
                let tip = p.effects.iter().map(|e| label(e.effect.name())).collect::<Vec<_>>().join(" + ");
                let resp = resp.on_hover_text(tip);
                let payload = FxPayload::Stack { name: p.name.clone(), effects: p.effects.clone() };
                drag_feedback(env, ui, &resp, payload.clone(), &p.name);
                if resp.double_clicked() {
                    apply_selected(env, &payload);
                }
            }
            ui.add_space(4.0);
            save_row(env, ui);
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn effect_item(env: &mut Env, ui: &mut Ui, ctx: &egui::Context, ty: &str, w: f32, h: f32, budget: &mut u32, salt: &str) {
    let has_default = default_effect(ty, w, h).is_some();
    let th = if has_default { thumb(env, ctx, ty, budget) } else { None };
    let fav = env.ed.comp.favorites.contains(ty);
    let name = label(ty);
    let (resp, star) = item_row(env, ui, &name, &format!("fxb:{salt}:{ty}"), th, Some(fav), has_default);
    if star {
        toggle_favorite(env, ty);
    }
    if !has_default {
        let _ = resp.on_hover_text(tr("comp.lut_needs_file"));
        return;
    }
    let payload = FxPayload::Effect(ty.to_owned());
    let resp = resp.on_hover_text(tr("comp.drag_or_double"));
    drag_feedback(env, ui, &resp, payload.clone(), &name);
    if resp.double_clicked() {
        apply_selected(env, &payload);
    }
}

/// Salvar a pilha da camada selecionada como predefinição do usuário.
fn save_row(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let layer = env.ed.active_layer(env.core);
    let st = layer.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).map(crate::app::comp::stack).unwrap_or_default();
    ui.horizontal(|ui| {
        let mut name = env.ed.comp.preset_name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text(tr("comp.preset_name")).desired_width((ui.available_width() - 90.0).max(60.0)));
        env.note("fxb:preset_name", r.rect);
        env.ed.comp.preset_name = name.clone();
        let b = ui.add_enabled(!st.is_empty(), egui::Button::new(tr("comp.save_preset")));
        env.note("fxb:save_preset", b.rect);
        if b.on_hover_text(tr("comp.save_preset.tip")).clicked() {
            let nm = if name.trim().is_empty() { layer.as_ref().and_then(|l| env.core.project.node(l)).map(|n| n.name.clone()).unwrap_or_default() } else { name.trim().to_owned() };
            let mut p = UserPreset { format: PRESET_FORMAT.into(), version: 1, name: nm, author: env.core.author.clone(), effects: st.clone(), path: None };
            if env.ed.comp.user_files {
                match presets_dir().ok_or_else(|| tr("comp.no_documents").to_owned()).and_then(|d| save_preset(&d, &p)) {
                    Ok(path) => {
                        p.path = Some(path.clone());
                        env.ed.notify(format!("{} {}", tr("comp.preset_saved"), path.display()));
                        env.ed.comp.presets.push(p);
                        env.ed.comp.preset_name.clear();
                    }
                    Err(e) => env.ed.notify(e),
                }
            } else {
                // sem sessão: fica só na memória
                env.ed.notify(tr("comp.preset_memory_only").to_owned());
                env.ed.comp.presets.push(p);
                env.ed.comp.preset_name.clear();
            }
        }
    });
    if st.is_empty() {
        dim_label(ui, &t, tr("comp.preset_needs_stack"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_effect_exists_and_is_creatable_except_lut() {
        let all: Vec<&str> = CATEGORIES.iter().flat_map(|(_, v)| v.iter().copied()).collect();
        // todos os efeitos do motor estão na árvore, uma vez só
        let mut seen = std::collections::HashSet::new();
        for ty in &all {
            assert!(seen.insert(*ty), "{ty} repetido");
        }
        assert_eq!(all.len(), 25, "a árvore deve listar os 25 efeitos do aiai-fx");
        for ty in all {
            match default_effect(ty, 64.0, 36.0) {
                Some(v) => {
                    let e: aiai_fx::Effect = serde_json::from_value(v).unwrap_or_else(|e| panic!("{ty}: {e}"));
                    assert_eq!(e.name(), ty);
                    assert!(thumb_image(ty).is_some(), "{ty} sem miniatura");
                }
                None => assert_eq!(ty, "lut"),
            }
            for lang in [crate::i18n::Lang::Pt, crate::i18n::Lang::En] {
                let k = format!("fx.e.{ty}");
                assert_ne!(crate::i18n::tr_in(lang, &k), k);
            }
        }
        for (k, st) in anime_presets(64.0, 36.0) {
            assert!(st.iter().all(|e| e.validate().is_ok()), "{k}");
            assert_ne!(crate::i18n::tr_in(crate::i18n::Lang::En, k), k);
        }
    }

    #[test]
    fn user_presets_round_trip_through_a_folder() {
        let dir = std::env::temp_dir().join(format!("aiai_fxp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let st = anime_presets(64.0, 36.0).remove(0).1;
        let p = UserPreset { format: PRESET_FORMAT.into(), version: 1, name: "Brilho/forte?".into(), author: "t".into(), effects: st, path: None };
        let a = save_preset(&dir, &p).unwrap();
        let b = save_preset(&dir, &p).unwrap();
        assert_ne!(a, b, "nome repetido ganha sufixo");
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("Brilho_forte_"));
        std::fs::write(dir.join("ruim.aiaifx"), "{nao é json").unwrap();
        let (list, warn) = load_presets(&dir);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].effects, p.effects);
        assert_eq!(warn.len(), 1, "arquivo ilegível vira aviso: {warn:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
