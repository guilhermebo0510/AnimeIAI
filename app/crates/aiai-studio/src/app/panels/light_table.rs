//! Painel Mesa de luz (papel cebola): liga/desliga (`O`), quantos desenhos antes e depois, por quadros, por desenhos ou
//! só as chaves, opacidade, as cores do anterior e do seguinte, e de quais camadas. A imagem é feita em `anim` e só
//! aparece no canvas (nunca no render nem na exportação).

use crate::app::anim::{self, ONION_MAX, ONION_NEXT, ONION_PREV, OnionLayers, OnionMode};
use crate::app::ui_kit::{Env, body_ui, section};
use crate::i18n::tr;
use egui::{Rect, RichText, Ui};

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("light_table").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        // prévia de intercalação do quadro atual (R16), no topo: visível sem rolar (A6-07)
        crate::app::ibp_ui::command_button(env, ui, "light:ibp");
        ui.separator();
        let mut o = env.ed.anim.onion;
        let key = env.ed.keys.label(crate::keymap::Action::OnionToggle);
        let r = ui.checkbox(&mut o.on, RichText::new(tr("anim.onion.enable")).strong()).on_hover_text(format!("{} ({key})", tr("anim.onion.enable_tip")));
        env.note("light:on", r.rect);
        ui.add_enabled_ui(o.on, |ui| {
            section(ui, &t, "anim.onion.show");
            ui.horizontal_wrapped(|ui| {
                for (m, k, tip, name) in [
                    (OnionMode::Frames, "anim.onion.frames", "anim.onion.frames_tip", "frames"),
                    (OnionMode::Drawings, "anim.onion.drawings", "anim.onion.drawings_tip", "drawings"),
                    (OnionMode::Keys, "anim.onion.keys", "anim.onion.keys_tip", "keys"),
                ] {
                    let r = ui.selectable_label(o.mode == m, tr(k)).on_hover_text(tr(tip));
                    env.note(format!("light:mode.{name}"), r.rect);
                    if r.clicked() {
                        o.mode = m;
                    }
                }
            });
            egui::Grid::new("light_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label(tr("anim.onion.before"));
                ui.horizontal(|ui| {
                    let r = ui.add(egui::DragValue::new(&mut o.before).range(0..=ONION_MAX).speed(0.1));
                    env.note("light:before", r.rect);
                    color_button(env, ui, &mut o.prev, ONION_PREV, "light:prev_color");
                });
                ui.end_row();
                ui.label(tr("anim.onion.after"));
                ui.horizontal(|ui| {
                    let r = ui.add(egui::DragValue::new(&mut o.after).range(0..=ONION_MAX).speed(0.1));
                    env.note("light:after", r.rect);
                    color_button(env, ui, &mut o.next, ONION_NEXT, "light:next_color");
                });
                ui.end_row();
                ui.label(tr("anim.onion.opacity"));
                let mut pct = o.opacity * 100.0;
                let r = ui.add(egui::Slider::new(&mut pct, 5.0..=100.0).max_decimals(0).suffix("%"));
                env.note("light:opacity", r.rect);
                o.opacity = pct / 100.0;
                ui.end_row();
            });
            section(ui, &t, "anim.onion.layers");
            for (l, k, name) in [(OnionLayers::Active, "anim.onion.layers_active", "active"), (OnionLayers::Animation, "anim.onion.layers_all", "all")] {
                let r = ui.radio(o.layers == l, tr(k));
                env.note(format!("light:layers.{name}"), r.rect);
                if r.clicked() {
                    o.layers = l;
                }
            }
        });
        env.ed.anim.onion = o;
        ui.add_space(6.0);
        // o que está aparecendo agora (ou por que não)
        let status = if !o.on {
            tr("anim.onion.status_off").to_owned()
        } else if env.ed.playback.playing {
            tr("anim.onion.status_playing").to_owned()
        } else if env.ed.anim.onion_visible() {
            tr("anim.onion.status_on").to_owned()
        } else {
            tr("anim.onion.status_none").to_owned()
        };
        ui.label(RichText::new(status).size(11.5).color(t.text_dim));
        ui.label(RichText::new(tr("anim.onion.view_only")).size(11.0).color(t.text_dim));
        if let Some(cut) = env.ed.active_cut(env.core) {
            let ph = anim::cut_phase(&env.core.project, &cut);
            ui.label(RichText::new(format!("{}: {}", tr("anim.phase"), tr(anim::phase_key(ph)))).size(11.0).color(t.text_dim));
        }
    });
}

/// Amostra de cor com seletor (clique) e volta ao padrão pelo menu de contexto.
fn color_button(env: &mut Env, ui: &mut Ui, c: &mut egui::Color32, default: egui::Color32, name: &str) {
    let r = ui.color_edit_button_srgba(c);
    env.note(name, r.rect);
    r.context_menu(|ui| {
        if ui.button(tr("anim.onion.color_default")).clicked() {
            *c = default;
            ui.close();
        }
    });
}
