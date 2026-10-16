//! Estado e ações de interface para a bridge: registro dos retângulos dos elementos (`UiRects`), `GET_UI_STATE` e `UI_ACTION`.
//!
//! Todas as coordenadas lógicas são pontos do egui relativos ao canto superior esquerdo da área cliente da janela; as
//! físicas são `lógica x pixels_per_point` (inclui o zoom da interface). `rect_screen` soma a posição da área cliente na
//! tela (quando o sistema informa) — é o que o harness de mouse real usa para clicar.

use super::StudioApp;
use crate::i18n::Lang;
use crate::layout::PanelKind;
use crate::theme::ThemeKind;
use crate::workspace::{ToolId, WorkspaceId};
use aiai_bridge::protocol::{BridgeError, BridgeResult};
use egui::{Pos2, Rect};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// Retângulos (em pontos) que a interface desenhou: o quadro anterior completo (`prev`) é o que as consultas leem.
#[derive(Default)]
pub(crate) struct UiRects {
    cur: BTreeMap<String, Rect>,
    prev: BTreeMap<String, Rect>,
}

impl UiRects {
    pub(crate) fn note(&mut self, name: impl Into<String>, rect: Rect) {
        self.cur.insert(name.into(), rect);
    }

    /// Fim do quadro: o que foi desenhado vira a referência das consultas.
    pub(crate) fn end_frame(&mut self) {
        self.prev = std::mem::take(&mut self.cur);
    }

    pub(crate) fn get(&self, name: &str) -> Option<Rect> {
        self.prev.get(name).copied()
    }

    pub(crate) fn all(&self) -> &BTreeMap<String, Rect> {
        &self.prev
    }
}

/// Retângulo → `[x, y, largura, altura]`.
pub(crate) fn rect4(r: Rect, scale: f32) -> Value {
    json!([round(r.min.x * scale), round(r.min.y * scale), round(r.width() * scale), round(r.height() * scale)])
}

fn round(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

fn as_obj(v: &Value) -> Result<Map<String, Value>, BridgeError> {
    match v {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(BridgeError::invalid_params("params deve ser um objeto JSON")),
    }
}

fn text<'a>(m: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| m.get(*k).and_then(Value::as_str))
}

impl StudioApp {
    /// Posição (em pixels físicos da tela) do canto da área cliente, se o sistema informou.
    fn client_origin_physical(&self, ctx: &egui::Context) -> Option<Pos2> {
        let ppp = ctx.pixels_per_point();
        ctx.input(|i| i.viewport().inner_rect).map(|r| Pos2::new(r.min.x * ppp, r.min.y * ppp))
    }

    fn rect_entry(&self, r: Rect, ppp: f32, origin: Option<Pos2>) -> Value {
        let mut v = json!({ "rect_logical": rect4(r, 1.0), "rect_physical": rect4(r, ppp) });
        if let Some(o) = origin {
            v["rect_screen"] = json!([round(o.x + r.min.x * ppp), round(o.y + r.min.y * ppp), round(r.width() * ppp), round(r.height() * ppp)]);
            v["center_screen"] = json!([round(o.x + r.center().x * ppp), round(o.y + r.center().y * ppp)]);
        }
        v
    }

    pub(crate) fn bridge_ui_state(&self, ctx: &egui::Context) -> Value {
        let ppp = ctx.pixels_per_point();
        let origin = self.client_origin_physical(ctx);
        let vp = ctx.input(|i| i.viewport().clone());
        let screen = ctx.content_rect();
        let panels: Vec<Value> = match self.trees.get(&self.ws) {
            Some(tree) => tree
                .tiles
                .iter()
                .filter_map(|(id, t)| match t {
                    egui_tiles::Tile::Pane(kind) => {
                        let visible = tree.tiles.is_visible(*id);
                        let rect = self.rects.get(&format!("panel:{}", kind.key())).or_else(|| tree.tiles.rect(*id));
                        let mut v = json!({ "name": kind.key(), "title": crate::i18n::tr(kind.title_key()), "visible": visible });
                        if let (true, Some(r)) = (visible, rect) {
                            v["rect_logical"] = rect4(r, 1.0);
                            v["rect_physical"] = rect4(r, ppp);
                            if let Some(o) = origin {
                                v["rect_screen"] = json!([round(o.x + r.min.x * ppp), round(o.y + r.min.y * ppp), round(r.width() * ppp), round(r.height() * ppp)]);
                            }
                        }
                        Some(v)
                    }
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        };
        let widgets: Map<String, Value> = self.rects.all().iter().filter(|(k, _)| !k.starts_with("panel:")).map(|(k, r)| (k.clone(), self.rect_entry(*r, ppp, origin))).collect();
        let mut dialogs = Vec::new();
        if self.about_open {
            dialogs.push("about");
        }
        if self.prefs_open {
            dialogs.push("prefs");
        }
        if self.confirm.is_some() {
            dialogs.push("confirm");
        }
        if self.dialog_open() {
            dialogs.push("file_dialog");
        }
        json!({
            "workspace": self.ws.key(),
            "workspaces": WorkspaceId::ALL.iter().map(|w| w.key()).collect::<Vec<_>>(),
            "active_tool": self.tool.key(),
            "tools": ToolId::ALL.iter().map(|t| t.key()).collect::<Vec<_>>(),
            "panels": panels,
            "widgets": widgets,
            "dialogs": dialogs,
            "focused": {
                "window": vp.focused,
                "text_input": ctx.egui_wants_keyboard_input(),
                "pointer_over_ui": ctx.is_pointer_over_egui(),
            },
            "pixels_per_point": ppp,
            "native_pixels_per_point": vp.native_pixels_per_point,
            "ui_zoom": self.prefs.ui_zoom,
            "theme": self.prefs.theme.key(),
            "language": match self.prefs.lang { Lang::Pt => "pt", Lang::En => "en" },
            "window": {
                "size_logical": [round(screen.width()), round(screen.height())],
                "size_physical": [round(screen.width() * ppp), round(screen.height() * ppp)],
                "inner_rect_logical": vp.inner_rect.map(|r| rect4(r, 1.0)),
                "client_origin_screen_physical": origin.map(|o| json!([round(o.x), round(o.y)])),
                "maximized": vp.maximized,
                "minimized": vp.minimized,
                "title": self.window_title(),
            },
            "input_pending": self.input.pending(),
            "project": { "name": self.doc_name(), "path": self.core.path.as_ref().map(|p| p.display().to_string()), "revision": self.core.project.revision, "dirty": self.doc_dirty() },
        })
    }

    /// `UI_ACTION {action, ...}`: set_workspace {workspace}, select_tool {tool}, toggle_panel {panel, visible?},
    /// set_theme {theme: light|dark}, set_language {language: pt|en}, open_dialog/close_dialog {dialog: about|prefs}.
    /// Apelidos aceitos: set_tool = select_tool; open_panel = toggle_panel com visible=true.
    pub(crate) fn bridge_ui_action(&mut self, ctx: &egui::Context, params: &Value) -> BridgeResult {
        let m = as_obj(params)?;
        let action = text(&m, &["action"]).ok_or_else(|| BridgeError::invalid_params("UI_ACTION exige 'action'"))?.to_string();
        let result = match action.as_str() {
            "set_workspace" => {
                let key = text(&m, &["workspace", "value"]).ok_or_else(|| BridgeError::invalid_params("set_workspace exige 'workspace'"))?;
                let w = WorkspaceId::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("workspace desconhecido: {key}; válidos: {}", WorkspaceId::ALL.map(|w| w.key()).join(", "))))?;
                let previous = self.ws.key();
                self.switch_workspace(w);
                json!({ "workspace": self.ws.key(), "previous": previous })
            }
            "select_tool" | "set_tool" => {
                let key = text(&m, &["tool", "value"]).ok_or_else(|| BridgeError::invalid_params("select_tool exige 'tool'"))?;
                let t = ToolId::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("ferramenta desconhecida: {key}; válidas: {}", ToolId::ALL.map(|t| t.key()).join(", "))))?;
                self.tool = t;
                json!({ "active_tool": self.tool.key() })
            }
            "toggle_panel" | "open_panel" => {
                let key = text(&m, &["panel", "value"]).ok_or_else(|| BridgeError::invalid_params("toggle_panel exige 'panel'"))?;
                let kind = PanelKind::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("painel desconhecido: {key}")))?;
                let want = m.get("visible").and_then(Value::as_bool);
                let ws = self.ws;
                let tree = self.trees.entry(ws).or_insert_with(|| crate::layout::default_tree(ws));
                let id = tree
                    .tiles
                    .iter()
                    .find_map(|(id, t)| matches!(t, egui_tiles::Tile::Pane(k) if *k == kind).then_some(*id))
                    .ok_or_else(|| BridgeError::invalid_params(format!("o painel '{key}' não existe no workspace {}", ws.key())))?;
                let now = tree.tiles.is_visible(id);
                let target = if action == "open_panel" { true } else { want.unwrap_or(!now) };
                tree.tiles.set_visible(id, target);
                json!({ "panel": key, "visible": target, "workspace": ws.key() })
            }
            "set_theme" => {
                let key = text(&m, &["theme", "value"]).ok_or_else(|| BridgeError::invalid_params("set_theme exige 'theme' (light|dark)"))?;
                let t = ThemeKind::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("tema desconhecido: {key} (light|dark)")))?;
                self.set_theme(ctx, t);
                json!({ "theme": self.prefs.theme.key() })
            }
            "set_language" => {
                let key = text(&m, &["language", "lang", "value"]).ok_or_else(|| BridgeError::invalid_params("set_language exige 'language' (pt|en)"))?;
                let l = Lang::from_code(key).ok_or_else(|| BridgeError::invalid_params(format!("idioma desconhecido: {key} (pt|en)")))?;
                self.set_lang(l);
                json!({ "language": match self.prefs.lang { Lang::Pt => "pt", Lang::En => "en" } })
            }
            "open_dialog" | "close_dialog" => {
                let open = action == "open_dialog";
                match text(&m, &["dialog", "value"]) {
                    Some("about") => self.about_open = open,
                    Some("prefs") => self.prefs_open = open,
                    other => return Err(BridgeError::invalid_params(format!("diálogo desconhecido: {other:?} (about|prefs)"))),
                }
                json!({ "about": self.about_open, "prefs": self.prefs_open })
            }
            other => {
                return Err(BridgeError::invalid_params(format!(
                    "UI_ACTION desconhecida: {other}; use set_workspace, select_tool, toggle_panel, set_theme, set_language, open_dialog, close_dialog"
                )));
            }
        };
        ctx.request_repaint();
        let mut out = result;
        out["action"] = json!(action);
        Ok(out)
    }
}
