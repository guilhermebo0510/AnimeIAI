//! Troca temporária de ferramenta (Preferências > Ferramentas, como no Clip Studio): segurar a tecla de uma ferramenta a
//! usa só enquanto a tecla estiver apertada e, ao soltar depois do tempo escolhido, volta para a ferramenta de antes. Um
//! toque rápido (solta antes do tempo) troca de vez, como sempre foi.

use super::ui_kit::Env;
use crate::behavior_prefs::{hold_step, plain_keys_pressed, HoldStep, HoldSwitch};
use crate::workspace::ToolId;
use std::time::Instant;

/// Teclas apertadas neste quadro (sem Ctrl/Alt); ler ANTES dos atalhos, que consomem os eventos.
pub(crate) fn pressed(ctx: &egui::Context) -> Vec<egui::Key> {
    plain_keys_pressed(ctx)
}

/// Depois dos atalhos de ferramenta: arma a troca temporária se uma tecla trocou a ferramenta (`before` = a de antes) e
/// resolve a que já estava armada (volta a ferramenta, deixa a nova ou esquece).
pub(crate) fn after_shortcuts(ctx: &egui::Context, env: &mut Env, before: ToolId, pressed: &[egui::Key]) {
    if !env.ed.beh.prefs.tool_hold {
        env.ed.beh.hold = None;
        return;
    }
    if *env.tool != before {
        env.ed.beh.hold = pressed.first().map(|&key| {
            // segurando uma tecla e apertando outra: volta sempre à ferramenta-base
            let prev = env.ed.beh.hold.filter(|h| h.tool == before).map(|h| h.prev).unwrap_or(before);
            HoldSwitch { key, prev, tool: *env.tool, since: Instant::now(), released: false }
        });
    }
    let Some(h) = env.ed.beh.hold else { return };
    let busy = !matches!(env.ed.gesture, super::editor::Gesture::None) || env.ed.beh.size_drag.is_some();
    let down = ctx.input(|i| i.key_down(h.key));
    match hold_step(&h, down, h.since.elapsed().as_millis(), env.ed.beh.prefs.tool_hold_ms, *env.tool, busy) {
        HoldStep::Keep => {}
        HoldStep::Stay | HoldStep::Drop => env.ed.beh.hold = None,
        HoldStep::Restore => {
            *env.tool = h.prev;
            env.ed.beh.hold = None;
        }
    }
}
