//! Painéis do editor (dock): Projeto, Camadas, Cores, Linha do tempo e Propriedades. Cada um desenha dentro do corpo do
//! painel com o `Env` (documento, estado do editor, ícones) e só altera o documento por comandos (`commands`).

pub(crate) mod colors;
pub(crate) mod layers;
pub(crate) mod project;
pub(crate) mod properties;
pub(crate) mod timeline;

use egui::{Id, Response, Ui};

/// Campo numérico/slider que só entrega o valor quando o gesto termina (soltar o arrasto, Enter ou perder o foco): o
/// desfazer fica com UM passo por ajuste, e o valor em edição sobrevive entre quadros mesmo que o documento ainda não tenha mudado.
pub(crate) fn commit_on_release<T: Copy + Default + PartialEq + Send + Sync + 'static>(ui: &mut Ui, id: Id, current: T, add: impl FnOnce(&mut Ui, &mut T) -> Response) -> Option<T> {
    let mut v: T = ui.data(|d| d.get_temp::<T>(id)).unwrap_or(current);
    let r = add(ui, &mut v);
    let editing = r.dragged() || r.has_focus();
    if r.drag_stopped() || r.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<T>(id));
        return (v != current).then_some(v);
    }
    if editing {
        ui.data_mut(|d| d.insert_temp(id, v));
    } else {
        ui.data_mut(|d| d.remove_temp::<T>(id));
        if r.changed() && v != current {
            return Some(v);
        }
    }
    None
}

/// Campo de texto de uma linha que só entrega o texto ao terminar (Enter ou perder o foco; Esc descarta). O texto em edição
/// fica na memória do egui: o documento só muda quando o usuário confirma, e digitar não é desfeito pelo redesenho do quadro.
pub(crate) fn text_commit(ui: &mut Ui, id: Id, current: &str, width: f32) -> (Response, Option<String>) {
    let mut buf: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(width));
    let mut out = None;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(id));
        if r.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) && buf.trim() != current && !buf.trim().is_empty() {
            out = Some(buf.trim().to_owned());
        }
    }
    (r, out)
}
