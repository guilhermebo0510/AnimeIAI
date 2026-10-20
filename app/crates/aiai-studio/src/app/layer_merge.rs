//! Mesclar camadas no app (Ctrl+E, Ctrl+Shift+E, o botão e o menu de contexto do painel Camadas, o menu Editar): monta o
//! pedido `LAYER_MERGE` a partir das camadas escolhidas (a ativa + as do Ctrl/Shift+clique) e o roda pelo MESMO caminho
//! da bridge e do MCP (`aiai_layer_merge`), então o undo, o log e as regras (tipos, tempo, recusas) são os de lá. As
//! regras estão em `aiai_tools::layer_merge`.

use super::actions;
use super::commands as cmds;
use super::editor::Editor;
use crate::i18n::tr;
use aiai_core::{CoreSession, Id, LayerType};
use serde_json::{Value, json};

impl Editor {
    /// As camadas escolhidas no painel (ativa + `layer_sel`), de cima para baixo, só as que existem no corte ativo.
    pub(crate) fn chosen_layers(&mut self, core: &CoreSession) -> Vec<Id> {
        let Some(cut) = self.active_cut(core) else { return Vec::new() };
        let active = self.active_layer(core);
        let rows = cmds::layer_rows(&core.project, &cut);
        self.layer_sel.retain(|id| rows.iter().any(|r| &r.id == id));
        rows.iter().filter(|r| Some(&r.id) == active.as_ref() || self.layer_sel.contains(&r.id)).map(|r| r.id.clone()).collect()
    }

    /// Pedido da mesclagem: `visible` junta as visíveis (do pai da camada ativa); senão duas ou mais escolhidas viram uma só
    /// e uma escolhida vai para a de baixo. `None` se não há camada.
    pub(crate) fn merge_request(&mut self, core: &CoreSession, visible: bool) -> Option<Value> {
        let cut = self.active_cut(core)?;
        let chosen = self.chosen_layers(core);
        let active = self.active_layer(core)?;
        Some(if visible {
            let parent = core.project.node(&active).and_then(|n| n.parent.clone()).unwrap_or_else(|| cut.clone());
            json!({ "op": "visible", "cut": cut, "parent": parent })
        } else if chosen.len() >= 2 {
            json!({ "op": "selected", "layers": chosen })
        } else {
            json!({ "op": "down", "layer": active })
        })
    }

    /// Roda a mesclagem; o resultado vira a camada ativa e o aviso diz o que aconteceu (ou por que não deu).
    pub(crate) fn merge_layers(&mut self, core: &mut CoreSession, visible: bool) -> bool {
        let Some(req) = self.merge_request(core, visible) else {
            self.notify(tr("layers.merge.none"));
            return false;
        };
        match actions::run(core, "LAYER_MERGE", req) {
            Ok(v) => {
                if let Some(l) = v.get("layer").and_then(Value::as_str) {
                    let id = Id(l.to_owned());
                    let name = core.project.node(&id).map(|n| n.name.clone()).unwrap_or_default();
                    self.layer = Some(id);
                    self.notify(tr("layers.merge.done").replace("{name}", &name));
                }
                self.layer_sel.clear();
                self.cel_number = None;
                true
            }
            Err(e) => {
                self.notify(e);
                false
            }
        }
    }

    /// Há o que mesclar com o botão/atalho (uma camada de desenho escolhida)?
    pub(crate) fn can_merge_layers(&mut self, core: &CoreSession) -> bool {
        self.active_layer(core).and_then(|l| cmds::layer_data(&core.project, &l).map(|d| d.kind != LayerType::Group)).unwrap_or(false)
    }
}
