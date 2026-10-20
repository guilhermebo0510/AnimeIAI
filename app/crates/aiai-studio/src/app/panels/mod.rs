//! Painéis do editor (dock): Projeto, Camadas, Cores, Linha do tempo, Propriedades e, na pré-produção, Estrutura, Roteiro, Cenas e Prancha; na Produção, Episódios e Tarefas; na Edição, Mídia, Monitor e a linha do tempo da edição; na Entrega, Predefinições e Fila; no Áudio, Mídia, Monitor do corte e Mixer; na Composição e na Cor, efeitos, câmera e grade (`satsuei`); em Personagens, lista, folha de modelo e ficha (`characters`); nos workspaces de canvas, o Navegador. Cada um desenha dentro do corpo do
//! painel com o `Env` (documento, estado do editor, ícones) e só altera o documento por comandos (`commands`).

pub(crate) mod audio;
pub(crate) mod audio_ws;
pub(crate) mod board;
pub(crate) mod brush_maker;
pub(crate) mod brushes;
pub(crate) mod char_folder;
pub(crate) mod char_ref;
pub(crate) mod characters;
pub(crate) mod colors;
pub(crate) mod comp_timeline;
pub(crate) mod deliver;
pub(crate) mod edit;
pub(crate) mod edit_panels;
pub(crate) mod edit_timeline;
pub(crate) mod effect_controls;
pub(crate) mod effects_browser;
pub(crate) mod layers;
pub(crate) mod library;
pub(crate) mod light_table;
pub(crate) mod modeling;
pub(crate) mod navigator;
pub(crate) mod outline;
pub(crate) mod prod_charts;
pub(crate) mod prod_logic;
pub(crate) mod prod_manage;
pub(crate) mod prod_views;
pub(crate) mod production;
pub(crate) mod project;
pub(crate) mod properties;
pub(crate) mod rulers;
pub(crate) mod satsuei;
pub(crate) mod scene_info;
pub(crate) mod script;
pub(crate) mod shots;
pub(crate) mod subview;
pub(crate) mod text;
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
        // só um clique conta como mudança aqui: um slider que arredonda o valor (max_decimals) acusa "changed" sozinho
        // a cada quadro quando o documento guarda 0,6 em f32 (60,000002%) e escrevia de volta sem parar
        if r.changed() && r.clicked() && v != current {
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

/// Campo de texto de várias linhas (cresce com o texto) que só entrega o texto ao perder o foco (Esc descarta); o texto vazio
/// é válido (limpa o campo). Guarda o que está sendo digitado na memória do egui, como `text_commit`.
pub(crate) fn text_commit_multi(ui: &mut Ui, id: Id, current: &str, width: f32, hint: &str) -> (Response, Option<String>) {
    let mut buf: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned());
    let r = ui.add(egui::TextEdit::multiline(&mut buf).desired_width(width).desired_rows(1).hint_text(hint));
    let mut out = None;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(id));
        if r.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) && buf != current {
            out = Some(buf);
        }
    }
    (r, out)
}

/// Como `text_commit`, mas aceita o texto vazio (campos opcionais, como emoção e direção).
pub(crate) fn text_commit_opt(ui: &mut Ui, id: Id, current: &str, width: f32, hint: &str) -> (Response, Option<String>) {
    let mut buf: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(width).hint_text(hint));
    let mut out = None;
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(id));
        if r.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) && buf.trim() != current {
            out = Some(buf.trim().to_owned());
        }
    }
    (r, out)
}

#[cfg(test)]
mod commit_tests {
    use super::*;

    #[test]
    fn a_rounding_slider_does_not_commit_by_itself() {
        // opacidade 0,6 guardada em f32 = 60,000002%: o slider com 0 casas não pode escrever de volta sozinho
        let ctx = egui::Context::default();
        let current = 0.6f32 * 100.0;
        let mut commits = 0;
        for _ in 0..5 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let r = commit_on_release(ui, Id::new("op"), current, |ui, v| ui.add(egui::Slider::new(v, 0.0..=100.0).max_decimals(0)));
                if r.is_some() {
                    commits += 1;
                }
            });
            out.textures_delta.clear();
        }
        assert_eq!(commits, 0);
    }
}
