//! Painel Copiloto de traço (no dock, coluna própria à direita, altura inteira): o corpo está em `copilot_ui::panel`.

use crate::app::ui_kit::{Env, body_ui};
use egui::{Rect, Ui};

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("copilot_panel").auto_shrink([false, false]).show(ui, |ui| {
        crate::app::copilot_ui::panel(env, ui);
    });
}
