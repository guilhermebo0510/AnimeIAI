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

/// Seis casas decimais (matriz do canvas: com 2 casas o erro cresce com a distância e desalinha o harness de mouse).
fn fine(v: f32) -> f64 {
    (v as f64 * 1_000_000.0).round() / 1_000_000.0
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

    /// Estouro de layout do quadro anterior: todo widget cujo retângulo inteiro passa do recorte do pai na horizontal
    /// (texto/campo cortado pela borda do painel ou da janela). O recorte vertical é ignorado (rolagem normal). Cada item
    /// diz o painel onde caiu (`panel`), quanto passou (`excess`, pontos), o retângulo e, quando o egui guardou, o rótulo.
    /// Painéis com rolagem horizontal intencional (linhas do tempo) entram com `scroll_x: true` para o chamador
    /// poder separar.
    pub(crate) fn layout_overflow(&self, ctx: &egui::Context) -> Value {
        let panels: Vec<(String, Rect)> = self.rects.all().iter().filter_map(|(k, r)| k.strip_prefix("panel:").map(|n| (n.to_string(), *r))).collect();
        let screen = ctx.content_rect();
        let mut out = Vec::new();
        ctx.viewport(|vp| {
            let w = &vp.prev_pass.widgets;
            for (_, list) in w.layers() {
                for wr in list {
                    let (full, vis) = (wr.rect, wr.interact_rect);
                    if !full.is_positive() || vis.height() <= 0.5 || full.width() > screen.width() * 2.0 {
                        continue;
                    }
                    let right = (full.max.x - vis.max.x).max(0.0);
                    let left = (vis.min.x - full.min.x).max(0.0);
                    let excess = right.max(left);
                    if excess <= 1.0 {
                        continue;
                    }
                    // o ponto de prova fica dentro do recorte do pai (um widget empurrado inteiro para fora cairia no painel vizinho)
                    let probe = if vis.width() > 0.0 { vis.center() } else if right >= left { egui::pos2(vis.max.x - 1.0, vis.center().y) } else { egui::pos2(vis.min.x + 1.0, vis.center().y) };
                    let panel = panels.iter().filter(|(_, r)| r.contains(probe)).min_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(n, _)| n.clone());
                    let scroll_x = matches!(panel.as_deref(), Some("timeline"));
                    let info = w.info(wr.id);
                    out.push(json!({
                        "panel": panel,
                        "excess": round(excess),
                        "side": if right >= left { "right" } else { "left" },
                        "hidden": vis.width() <= 0.0,
                        "scroll_x": scroll_x,
                        "rect": rect4(full, 1.0),
                        "visible_rect": rect4(vis, 1.0),
                        "type": info.map(|i| format!("{:?}", i.typ)),
                        "label": info.and_then(|i| i.label.clone().or_else(|| i.current_text_value.clone())),
                    }));
                }
            }
        });
        out.sort_by(|a, b| b["excess"].as_f64().unwrap_or(0.0).total_cmp(&a["excess"].as_f64().unwrap_or(0.0)));
        Value::Array(out)
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
        if self.start.open {
            dialogs.push("start");
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
            // a prateleira deste workspace (`WorkspaceProfile`) e o layout em uso
            "shelf_tools": self.ws.profile().tools.iter().map(|t| t.key()).collect::<Vec<_>>(),
            "layout": self.layout_state(),
            "panels": panels,
            "widgets": widgets,
            "overflow": self.layout_overflow(ctx),
            "dialogs": dialogs,
            "popup_open": egui::Popup::is_any_open(ctx),
            "prefs_tab": self.prefs_tab.key(),
            "key_capture": self.key_capture.map(|a| a.id()),
            "shortcuts": crate::keymap::ACTIONS.iter().map(|a| (a.1.to_owned(), Value::from(self.ed.keys.label(a.0)))).collect::<Map<String, Value>>(),
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
            "editor": self.editor_state(ctx),
            "notice": self.notice.as_ref().filter(|(_, at)| at.elapsed().as_secs() < 6).map(|(m, _)| m.clone()),
            "input_pending": self.input.pending(),
            "project": { "name": self.doc_name(), "path": self.core.path.as_ref().map(|p| p.display().to_string()), "revision": self.core.project.revision, "dirty": self.doc_dirty() },
            // memória de vídeo: uso por subsistema, alocador do wgpu, uso/orçamento do sistema, pressão e despejos
            "gpu": self.gpu.to_json(),
        })
    }

    /// Estado do editor para a bridge e para o harness de mouse real: corte/camada/quadro ativos, a transformação do canvas
    /// (`origin_logical` = onde o pixel (0,0) do desenho cai na janela, `m` = pontos lógicos por pixel do desenho; some o
    /// canto da área cliente em pixels físicos para chegar à tela), as camadas, a reprodução e as medidas de latência.
    pub(crate) fn editor_state(&self, ctx: &egui::Context) -> Value {
        use super::commands as cmds;
        let ed = &self.ed;
        let p = &self.core.project;
        let ppp = ctx.pixels_per_point();
        let canvas = ed.canvas_rect.map(|r| {
            let xf = ed.view.xf(r, ppp);
            let size = ed.cut.as_ref().map(|c| cmds::canvas_size(p, c)).unwrap_or((0, 0));
            json!({
                "rect_logical": rect4(r, 1.0),
                "zoom": fine(ed.view.zoom),
                "rotation_deg": round(ed.view.rotation_deg()),
                "flip": ed.view.flip,
                "center": [fine(ed.view.center.x), fine(ed.view.center.y)],
                "origin_logical": [fine(xf.origin.x), fine(xf.origin.y)],
                "m": xf.m.iter().map(|v| fine(*v)).collect::<Vec<_>>(),
                "size": [size.0, size.1],
                "shown_frame": ed.shown.as_ref().map(|s| s.rendered.key.frame),
                "shown_counter": ed.shown.as_ref().map(|s| s.rendered.key.counter),
            })
        });
        let layers: Vec<Value> = ed.cut.as_ref().map(|cut| {
            cmds::layer_rows(p, cut).into_iter().filter_map(|r| {
                let n = p.node(&r.id)?;
                let l = cmds::layer_data(p, &r.id)?;
                let cel = cmds::exposed_number(p, cut, &r.id, ed.frame);
                Some(json!({ "id": r.id.0, "name": n.name, "kind": super::panels::layers::kind_id(l.kind), "depth": r.depth, "visible": l.visible, "locked": l.locked, "opacity": l.opacity, "exposed_cel": cel }))
            }).collect()
        }).unwrap_or_default();
        let lat = |l: &super::editor::Latency| json!({ "label": l.label, "tool_ms": round(l.tool_ms), "render_ms": round(l.render_ms), "total_ms": round(l.total_ms) });
        let o = &ed.opts;
        let render = ed.render.as_ref().map(|r| json!({ "avg_ms": round(r.avg_ms), "renders_done": r.renders_done, "cached_bytes": r.cached_bytes(), "busy": r.busy(), "error": r.last_error.as_ref().map(|(_, m)| m.clone()), "warnings": self.ed.shown.as_ref().map(|s| s.rendered.warnings.clone()) }));
        json!({
            "cut": ed.cut.as_ref().map(|c| c.0.clone()),
            "layer": ed.layer.as_ref().map(|c| c.0.clone()),
            "frame": ed.frame,
            "cel_number": ed.cel_number,
            "color": cmds::resolve_color(p, &ed.color).to_hex(),
            "canvas": canvas,
            "layers": layers,
            "playback": { "playing": ed.playback.playing, "fps_real": round(ed.playback.fps_real), "looping": ed.playback.looping, "loop_in": ed.playback.loop_in, "loop_out": ed.playback.loop_out },
            "latency": ed.latency.as_ref().map(lat),
            "latency_log": ed.latency_log.iter().map(lat).collect::<Vec<_>>(),
            "live": { "ms": ed.live_stroke.last_ms, "updates": ed.live_stroke.updates, "overlay": ed.live_stroke.overlay.is_some() },
            "selection": ed.selection.as_ref().map(|s| json!({ "mode": format!("{:?}", s.mode), "strokes": s.strokes.iter().map(|i| i.0.clone()).collect::<Vec<_>>(), "whole": s.whole, "offset": [round(s.offset.x), round(s.offset.y)], "rect": s.rect_of() })),
            "select_tool": super::selection::state_json(ed),
            "gesture_active": !matches!(ed.gesture, super::editor::Gesture::None),
            "cursor_logical": ed.cursor.map(|c| json!([round(c.x), round(c.y)])),
            "cursor_kind": ed.cursor_kind,
            "ghosts": ed.ghosts.len(),
            "node_stroke": ed.node_stroke.as_ref().map(|i| i.0.clone()),
            "node_sel": ed.node_sel,
            "points_mode": format!("{:?}", ed.opts.points_mode),
            "audio": json!({ "live": ed.live.enabled(), "starts": ed.live.starts, "scrubs": ed.live.scrubs, "error": ed.live.last_error }),
            "tool_options": json!({ "pen_width": o.pen_width, "brush_size": o.brush_size, "brush_opacity": o.brush_opacity, "eraser_size": o.eraser_size, "eraser_hardness": o.eraser_hardness, "eraser_opacity": o.eraser_opacity, "eraser_flow": o.eraser_flow, "eraser_tip": format!("{:?}", o.eraser_tip), "eraser_use_brush": o.eraser_use_brush, "fill_gap": o.fill_gap, "fill_expand": o.fill_expand, "fill_close_leftovers": o.fill_close_leftovers }),
            "timeline": { "cell_w": ed.tl_cell_w, "scroll": [round(ed.tl_scroll.x), round(ed.tl_scroll.y)], "header_w": ed.anim.tl.header_w, "ruler_h": ed.anim.tl.ruler_h, "row_h": ed.anim.tl.row_h, "density": if ed.anim.density(self.ws) == super::anim::Density::Compact { "compact" } else { "normal" } },
            "anim": { "onion": ed.anim.onion.on, "onion_visible": ed.anim.onion_visible(), "onion_renders": ed.anim.onion_renders, "phase": ed.cut.as_ref().map(|c| if super::anim::cut_phase(&self.core.project, c) == aiai_core::CutPhase::Douga { "douga" } else { "genga" }), "layout_variant": format!("{:?}", self.layouts.variant(crate::workspace::WorkspaceId::Animacao)) },
            "render": render,
            "modeling": super::modeling::state_json(&ed.modeling),
        })
    }

    /// Layout do workspace atual para o GET_UI_STATE: rótulo (`factory`, `custom` ou o nome), nomes salvos e o aviso de
    /// fábrica nova pendente.
    fn layout_state(&self) -> Value {
        let ws = self.ws;
        let label = match self.layouts.label(ws) {
            None => "factory".to_owned(),
            Some(n) if n.is_empty() => "custom".to_owned(),
            Some(n) => n,
        };
        let named: Vec<String> = self.layouts.get(ws).map(|e| e.named.iter().map(|n| n.name.clone()).collect()).unwrap_or_default();
        json!({ "label": label, "named": named, "factory_rev": crate::layout::factory_rev(ws), "update_pending": self.layouts.factory_update_pending(ws), "persisted": self.layouts_path.is_some() })
    }

    /// `UI_ACTION {action, ...}`: set_workspace {workspace}, select_tool {tool}, toggle_panel {panel, visible?},
    /// open_panel {panel} (insere o painel do catálogo que não está no layout), layout {op: list|save|apply|delete|reset, name?},
    /// set_theme {theme: light|dark}, set_language {language: pt|en}, open_dialog/close_dialog {dialog: about|prefs|start, tab?: general|keys|folders|performance},
    /// set_window_size {width, height, maximized?} (pontos lógicos), library_import {files} (abre a janela de importar da Biblioteca),
    /// select_layer {layer}, select_cut {cut}, set_frame {frame}, set_color {color}, fit_view.
    /// Apelidos aceitos: set_tool = select_tool; open_panel = toggle_panel com visible=true.
    pub(crate) fn bridge_ui_action(&mut self, ctx: &egui::Context, params: &Value) -> BridgeResult {
        let m = as_obj(params)?;
        let action = text(&m, &["action"]).ok_or_else(|| BridgeError::invalid_params("UI_ACTION exige 'action'"))?.to_string();
        // chaves de cada sub-ação (com os apelidos que `text` aceita); sub-ação desconhecida dá o erro do `match` abaixo
        let keys: Option<&[&str]> = match action.as_str() {
            "set_workspace" => Some(&["workspace", "value"]),
            "select_tool" | "set_tool" => Some(&["tool", "value", "mode"]),
            "select_layer" => Some(&["layer", "id", "value"]),
            "select_cut" => Some(&["cut", "id", "value"]),
            "set_frame" => Some(&["frame"]),
            "set_color" => Some(&["color", "value"]),
            "fit_view" => Some(&[]),
            "toggle_panel" | "open_panel" => Some(&["panel", "value", "visible"]),
            "layout" => Some(&["op", "name"]),
            "set_theme" => Some(&["theme", "value"]),
            "set_window_size" => Some(&["width", "height", "maximized"]),
            "set_language" => Some(&["language", "lang", "value"]),
            "open_dialog" | "close_dialog" => Some(&["dialog", "value", "tab"]),
            "library_import" => Some(&["files"]),
            "gpu" => Some(&["op", "mb"]),
            _ => None,
        };
        if let Some(keys) = keys {
            let mut keys = keys.to_vec();
            keys.push("action");
            aiai_bridge::params_check::check_keys(params, &format!("UI_ACTION {action}"), &keys)?;
        }
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
                let shelf = self.ws.profile().tools;
                if !shelf.contains(&t) {
                    let list: Vec<&str> = shelf.iter().map(|t| t.key()).collect();
                    return Err(BridgeError::invalid_params(format!("a ferramenta '{key}' não está na prateleira do workspace {}; nele: [{}]", self.ws.key(), list.join(", "))));
                }
                self.tool = t;
                // Seleção: `mode` escolhe a sub-ferramenta (rect, ellipse, lasso, polygon, wand, strokes)
                if let Some(mode) = text(&m, &["mode"]) {
                    let st = super::selection::SelTool::ALL.into_iter().find(|s| s.id() == mode).filter(|_| t == ToolId::Lasso);
                    let st = st.ok_or_else(|| BridgeError::invalid_params(format!("modo desconhecido para {key}: {mode}; válidos (Seleção): {}", super::selection::SelTool::ALL.map(|s| s.id()).join(", "))))?;
                    self.ed.opts.select_mode = st;
                }
                json!({ "active_tool": self.tool.key() })
            }
            "select_layer" => {
                let id = text(&m, &["layer", "id", "value"]).ok_or_else(|| BridgeError::invalid_params("select_layer exige 'layer' (id)"))?;
                let id = aiai_core::Id(id.to_string());
                if super::commands::layer_data(&self.core.project, &id).is_none() {
                    return Err(BridgeError::invalid_params(format!("camada desconhecida: {id}")));
                }
                self.ed.layer = Some(id.clone());
                self.ed.cel_number = None;
                json!({ "layer": id.0 })
            }
            "select_cut" => {
                let id = text(&m, &["cut", "id", "value"]).ok_or_else(|| BridgeError::invalid_params("select_cut exige 'cut' (id)"))?;
                let id = aiai_core::Id(id.to_string());
                if !super::commands::is_cut(&self.core.project, &id) {
                    return Err(BridgeError::invalid_params(format!("corte desconhecido: {id}")));
                }
                self.ed.set_cut(id.clone());
                json!({ "cut": id.0 })
            }
            "set_frame" => {
                let f = m.get("frame").and_then(Value::as_u64).ok_or_else(|| BridgeError::invalid_params("set_frame exige 'frame' (inteiro, 0-based)"))?;
                self.ed.playback.playing = false;
                self.ed.frame = f as u32;
                json!({ "frame": self.ed.frame })
            }
            "set_color" => {
                let hex = text(&m, &["color", "value"]).ok_or_else(|| BridgeError::invalid_params("set_color exige 'color' (#rrggbb)"))?;
                let c = aiai_core::Rgba::from_hex(hex).ok_or_else(|| BridgeError::invalid_params(format!("cor inválida: {hex}")))?;
                self.ed.set_color_direct(c);
                json!({ "color": c.to_hex() })
            }
            "fit_view" => {
                self.ed.fit_pending = true;
                json!({ "fit": true })
            }
            "toggle_panel" | "open_panel" => {
                let key = text(&m, &["panel", "value"]).ok_or_else(|| BridgeError::invalid_params("toggle_panel exige 'panel'"))?;
                let kind = PanelKind::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("painel desconhecido: {key}")))?;
                let want = m.get("visible").and_then(Value::as_bool);
                let ws = self.ws;
                self.ensure_tree(ws);
                let found = self.trees.get(&ws).and_then(|t| crate::layout::find_pane(t, kind).map(|id| t.tiles.is_visible(id)));
                let target = if action == "open_panel" { true } else { want.unwrap_or(!found.unwrap_or(false)) };
                // alternar um painel que não está no layout é engano provável: só open_panel / visible=true o insere
                if found.is_none() && action == "toggle_panel" && want.is_none() {
                    return Err(BridgeError::invalid_params(format!("o painel '{key}' não está no layout do workspace {}; use open_panel (ou visible: true) para inserir", ws.key())));
                }
                // aberto = também a aba da frente quando divide espaço com outros (ex.: Projeto e Pincéis)
                self.set_panel_visible(kind, target).map_err(BridgeError::invalid_params)?;
                json!({ "panel": key, "visible": target, "workspace": ws.key(), "inserted": found.is_none() && target })
            }
            "layout" => {
                let op = text(&m, &["op"]).ok_or_else(|| BridgeError::invalid_params("layout exige 'op' (list|save|apply|delete|reset)"))?.to_owned();
                let name = text(&m, &["name"]).map(str::to_owned);
                let need = |n: &Option<String>| n.clone().filter(|s| !s.trim().is_empty()).ok_or_else(|| BridgeError::invalid_params(format!("layout {op} exige 'name'")));
                match op.as_str() {
                    "list" => {}
                    "save" => {
                        self.layout_save_named(&need(&name)?).map_err(BridgeError::invalid_params)?;
                    }
                    "apply" => self.layout_apply(&need(&name)?).map_err(BridgeError::invalid_params)?,
                    "delete" => {
                        let n = need(&name)?;
                        if !self.layout_delete(&n) {
                            return Err(BridgeError::invalid_params(format!("layout desconhecido neste workspace: {n}")));
                        }
                    }
                    "reset" => self.reset_layout(self.ws),
                    other => return Err(BridgeError::invalid_params(format!("layout op desconhecida: {other} (list|save|apply|delete|reset)"))),
                }
                let mut v = self.layout_state();
                v["workspace"] = json!(self.ws.key());
                v
            }
            "set_theme" => {
                let key = text(&m, &["theme", "value"]).ok_or_else(|| BridgeError::invalid_params("set_theme exige 'theme' (light|dark)"))?;
                let t = ThemeKind::from_key(key).ok_or_else(|| BridgeError::invalid_params(format!("tema desconhecido: {key} (light|dark)")))?;
                self.set_theme(ctx, t);
                json!({ "theme": self.prefs.theme.key() })
            }
            "set_window_size" => {
                // Tamanho da área cliente em pontos lógicos (varredura de estouro nas larguras mínimas); tira o maximizado antes.
                let w = m.get("width").and_then(Value::as_f64).ok_or_else(|| BridgeError::invalid_params("set_window_size exige 'width' e 'height' (pontos)"))?;
                let h = m.get("height").and_then(Value::as_f64).ok_or_else(|| BridgeError::invalid_params("set_window_size exige 'width' e 'height' (pontos)"))?;
                if m.get("maximized").and_then(Value::as_bool) == Some(true) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w as f32, h as f32)));
                }
                json!({ "requested": [w, h] })
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
                    Some("prefs") => {
                        self.prefs_open = open;
                        if let Some(tab) = text(&m, &["tab"]).and_then(super::prefs_ui::PrefsTab::from_key) {
                            self.prefs_tab = tab;
                        }
                    }
                    Some("start") => self.start.open = open,
                    other => return Err(BridgeError::invalid_params(format!("diálogo desconhecido: {other:?} (about|prefs|start)"))),
                }
                json!({ "about": self.about_open, "prefs": self.prefs_open, "start": self.start.open })
            }
            "library_import" => {
                // abre a janela de importar da Biblioteca com estes arquivos (o diálogo de arquivos do sistema não se automatiza)
                let files: Vec<std::path::PathBuf> = m.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(std::path::PathBuf::from).collect()).unwrap_or_default();
                if files.is_empty() {
                    return Err(BridgeError::invalid_params("library_import exige 'files' (lista de caminhos absolutos)"));
                }
                self.ed.library.begin_import(files.clone());
                json!({ "files": files.len() })
            }
            // memória de vídeo: testar o orçamento e a resiliência sem esgotar a placa (ver gpu_evict::gpu_bridge)
            "gpu" => self.gpu_bridge(text(&m, &["op"]).unwrap_or(""), m.get("mb").and_then(Value::as_u64))?,
            other => {
                return Err(BridgeError::invalid_params(format!(
                    "UI_ACTION desconhecida: {other}; use set_workspace, select_tool, toggle_panel, open_panel, layout, set_theme, set_language, set_window_size, open_dialog, close_dialog, select_layer, select_cut, set_frame, set_color, fit_view"
                )));
            }
        };
        ctx.request_repaint();
        let mut out = result;
        out["action"] = json!(action);
        Ok(out)
    }
}
