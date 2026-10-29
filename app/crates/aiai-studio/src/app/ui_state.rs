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
        let maxed = self.dock_main_maximized();
        let panels: Vec<Value> = match self.trees.get(&self.ws) {
            Some(tree) => tree
                .tiles
                .iter()
                .filter_map(|(id, t)| match t {
                    egui_tiles::Tile::Pane(kind) => {
                        // com um painel maximizado, os outros não estão na tela
                        let visible = tree.tiles.is_visible(*id) && maxed.is_none_or(|m| m == *kind);
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
        // painéis e widgets das janelas soltas entram com `viewport: "float:N"` e o retângulo na tela daquela janela
        let mut panels = panels;
        panels.extend(self.dock_panel_entries());
        let mut widgets: Map<String, Value> = self.rects.all().iter().filter(|(k, _)| !k.starts_with("panel:")).map(|(k, r)| {
            let mut v = self.rect_entry(*r, ppp, origin);
            v["viewport"] = "root".into();
            (k.clone(), v)
        }).collect();
        widgets.extend(self.dock_widget_entries());
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
            // S25: a prateleira por grupos das áreas de desenho (ferramenta em uso e as que aparecem)
            "shelf_slot": self.ed.draw_ui.shelf.slot.map(|s| s.key()),
            "shelf_slots": super::shelf::slots_for(self.ws).iter().map(|s| s.key()).collect::<Vec<_>>(),
            "layout": self.layout_state(),
            "panels": panels,
            "widgets": widgets,
            "viewports": self.dock_viewports_json(),
            "overflow": self.layout_overflow(ctx),
            "dialogs": dialogs,
            "popup_open": egui::Popup::is_any_open(ctx),
            "prefs_tab": self.prefs_tab.key(),
            // Preferências de comportamento (S25): troca temporária, cursor, cópias, cores da tela…
            "behavior": serde_json::to_value(&self.prefs.behavior).unwrap_or_default(),
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
        // prévia de intercalação viva (R16): o que a barra mostra
        let ibp_state = aiai_tools::inbetween_preview::current(&self.core).map(|p| {
            json!({ "proposal_id": p.id, "frame": p.frame, "frame_a": p.frame_a, "frame_b": p.frame_b, "stale": p.stale, "acceptable": aiai_tools::inbetween_preview::blocked_reason(&self.core.project, p).is_none(), "blocked_reason": aiai_tools::inbetween_preview::blocked_reason(&self.core.project, p),
                    "refusals": p.refusals.len(), "approximations": p.approximations.len(), "manual_pairs": p.request.manual_pairs, "pairs_mode": ed.ibp.pairs_mode, "pick_a": ed.ibp.pick_a,
                    "show_notes": ed.ibp.show_notes, "veil": ed.ibp.veil })
        });
        let canvas = ed.canvas_rect.map(|r| {
            let xf = ed.view.xf(r, ppp);
            let size = ed.cut.as_ref().map(|c| cmds::canvas_size(p, c)).unwrap_or((0, 0));
            json!({
                "rect_logical": rect4(r, 1.0),
                "viewport": self.dock_canvas_viewport(),
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
            // cor de paleta (id da paleta e da cor) ou direta
            "color_ref": match &ed.color {
                aiai_core::ColorRef::Palette { palette, color } => json!({ "palette": palette.0, "color": color.0 }),
                _ => json!("direct"),
            },
            "palette": ed.palette.as_ref().map(|p| p.0.clone()),
            "canvas": canvas,
            "layers": layers,
            "playback": { "playing": ed.playback.playing, "fps_real": round(ed.playback.fps_real), "looping": ed.playback.looping, "loop_in": ed.playback.loop_in, "loop_out": ed.playback.loop_out },
            "latency": ed.latency.as_ref().map(lat),
            "latency_log": ed.latency_log.iter().map(lat).collect::<Vec<_>>(),
            "live": { "ms": ed.live_stroke.last_ms, "updates": ed.live_stroke.updates, "overlay": ed.live_stroke.overlay.is_some() },
            "selection": ed.selection.as_ref().map(|s| json!({ "mode": format!("{:?}", s.mode), "strokes": s.strokes.iter().map(|i| i.0.clone()).collect::<Vec<_>>(), "whole": s.whole, "offset": [round(s.offset.x), round(s.offset.y)], "rect": s.rect_of(), "mesh": s.mesh.as_ref().map(|m| m.pts.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>()) })),
            "select_tool": super::selection::state_json(ed),
            "lint_marks": lint_json(&ed.lint_marks),
            "gesture_active": !matches!(ed.gesture, super::editor::Gesture::None),
            "rulers": { "new_kind": ed.rulers.new_kind.id(), "snap": ed.rulers.snap, "show": ed.rulers.show, "pending": ed.rulers.pending.len() },
            "cursor_logical": ed.cursor.map(|c| json!([round(c.x), round(c.y)])),
            "cursor_kind": ed.cursor_kind,
            "ghosts": ed.ghosts.len(),
            "node_stroke": ed.node_stroke.as_ref().map(|i| i.0.clone()),
            "node_sel": ed.node_sel,
            "points_mode": format!("{:?}", ed.opts.points_mode),
            "fix_tool": ed.opts.fix.tool.map(|f| f.key()),
            "audio": json!({ "live": ed.live.enabled(), "starts": ed.live.starts, "scrubs": ed.live.scrubs, "error": ed.live.last_error }),
            "tool_options": json!({ "pen_width": o.pen_width, "brush_size": o.brush_size, "brush_opacity": o.brush_opacity, "eraser_size": o.eraser_size, "eraser_hardness": o.eraser_hardness, "eraser_opacity": o.eraser_opacity, "eraser_flow": o.eraser_flow, "eraser_tip": format!("{:?}", o.eraser_tip), "eraser_use_brush": o.eraser_use_brush, "fill_gap": o.fill_gap, "fill_expand": o.fill_expand, "fill_close_leftovers": o.fill_close_leftovers }),
            "tool_options_s25": json!({ "brush_no_overflow": o.brush_no_overflow, "pen_weld": o.pen_weld, "pen_quickshape": o.pen_quickshape, "pen_ghost": o.pen_ghost, "pen_simplify": o.pen_simplify, "pen_taper_in": o.pen_taper_in, "pen_taper_out": o.pen_taper_out, "pen_width": o.pen_width, "pen_tip": o.pen_tip }),
            "pen_taper": json!({ "unit": o.pen_taper_unit, "by_speed": o.pen_taper_speed, "min": o.pen_taper_min, "auto": o.pen_taper_auto }),
            "timeline": { "cell_w": ed.tl_cell_w, "scroll": [round(ed.tl_scroll.x), round(ed.tl_scroll.y)], "header_w": ed.anim.tl.header_w, "ruler_h": ed.anim.tl.ruler_h, "row_h": ed.anim.tl.row_h, "density": if ed.anim.density(self.ws) == super::anim::Density::Compact { "compact" } else { "normal" } },
            "ibp": ibp_state,
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
    /// select_layer {layer}, select_cut {cut}, set_frame {frame}, set_color {color}, fit_view,
    /// set_brush {preset (nome, sem diferenciar maiúsculas), size?, opacity?, custom? (campos do pincel por cima do
    /// preset: vira a cópia do usuário, como mexer nas configurações avançadas)} (pincel da biblioteca em uso; um do grupo
    /// borracha vira também a ponta da ferramenta Borracha).
    /// dock {op: list|maximize|restore|detach|redock|redock_all, panel?, group?} (painel maximizado e janelas soltas).
    /// perf_samples {op: start|stop|read}: amostras brutas por quadro (CPU da interface e desenho da prévia de intercalação)
    /// e pedido → primeira imagem de cada prévia (`present_ms`)
    /// ibp_view {show_notes?, veil?}: vista da barra da prévia de intercalação
    /// modeling_view {layout?: single|split_v|split_h|quad, slot?, view?: perspective|ortho|front|back|right|left|top|bottom}
    /// (multivista da Modelagem).
    /// uv {edit?: bool, select_mode?: vertex|edge|face, select?: all|none|tree_seams, op?: mark_seam|unwrap|pack|…}
    /// (editor de UV da Modelagem).
    /// normals {op: from_center|from_view|from_object|average|smooth|reset, factor?, source? (índice do objeto)}
    /// (normais personalizadas da Modelagem).
    /// sculpt {mode?: bool, brush?, radius? (px), strength?, symmetry? [x,y,z], invert?, paths? [[[x,y]..]..] (0..1 da vista
    /// ativa), op?: mask_invert|mask_clear|smooth_all|voxel_remesh|symmetrize} (escultura da Modelagem).
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
            "set_tool_option" => Some(&["key", "value"]),
            "fit_view" => Some(&[]),
            "lint" | "lint_next" | "lint_prev" | "lint_fill" | "lint_clear" => Some(&[]),
            "set_brush" => Some(&["preset", "value", "size", "opacity", "custom"]),
            "toggle_panel" | "open_panel" => Some(&["panel", "value", "visible"]),
            "layout" => Some(&["op", "name"]),
            "set_theme" => Some(&["theme", "value"]),
            "set_window_size" => Some(&["width", "height", "maximized"]),
            "set_language" => Some(&["language", "lang", "value"]),
            "open_dialog" | "close_dialog" => Some(&["dialog", "value", "tab"]),
            "library_import" => Some(&["files"]),
            "gpu" => Some(&["op", "mb"]),
            "dock" => Some(&["op", "panel", "group"]),
            "modeling_view" => Some(&["layout", "slot", "view"]),
            "sculpt" => Some(&["mode", "brush", "radius", "strength", "invert", "paths", "op", "symmetry"]),
            "uv" => Some(&["edit", "select", "op", "select_mode"]),
            "normals" => Some(&["op", "factor", "source"]),
            "perf_samples" => Some(&["op"]),
            "ibp_view" => Some(&["show_notes", "veil"]),
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
            "set_tool_option" => {
                // opções da ferramenta que os testes ligam sem depender do layout do painel (caixas cortadas)
                let key = text(&m, &["key"]).ok_or_else(|| BridgeError::invalid_params("set_tool_option exige 'key'"))?.to_string();
                let v = m.get("value").cloned().unwrap_or(Value::Null);
                let o = &mut self.ed.opts;
                let flag = |v: &Value| v.as_bool().ok_or_else(|| BridgeError::invalid_params(format!("{key}: 'value' deve ser true/false")));
                let num = |v: &Value| v.as_f64().map(|x| x as f32).filter(|x| x.is_finite()).ok_or_else(|| BridgeError::invalid_params(format!("{key}: 'value' deve ser número")));
                match key.as_str() {
                    "pen_ghost" => o.pen_ghost = flag(&v)?,
                    "pen_weld" => o.pen_weld = flag(&v)?,
                    "pen_quickshape" => o.pen_quickshape = flag(&v)?,
                    "pen_color_trace" => o.pen_color_trace = flag(&v)?,
                    "pen_autoclose" => o.pen_autoclose = flag(&v)?,
                    "stab_tail" => o.stab_tail = flag(&v)?,
                    "brush_no_overflow" => o.brush_no_overflow = flag(&v)?,
                    "pen_width" => o.pen_width = num(&v)?.clamp(0.1, 500.0),
                    "pen_simplify" => o.pen_simplify = num(&v)?.clamp(0.0, 100.0),
                    _ => return Err(BridgeError::invalid_params(format!("opção desconhecida: {key} (pen_ghost, pen_weld, pen_quickshape, pen_color_trace, pen_autoclose, stab_tail, brush_no_overflow, pen_width, pen_simplify)"))),
                }
                json!({ "key": key, "value": v })
            }
            "fit_view" => {
                self.ed.fit_pending = true;
                json!({ "fit": true })
            }
            "lint" | "lint_next" | "lint_prev" | "lint_fill" | "lint_clear" => {
                // o botão Verificar do balde e a navegação pelos buracos marcados
                let t = self.tokens();
                let ws = self.ws;
                let mut notes = Vec::new();
                let mut env = super::ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
                match action.as_str() {
                    "lint" => super::tools::run_lint(&mut env),
                    "lint_next" => super::tools::lint_go(&mut env, 1),
                    "lint_prev" => super::tools::lint_go(&mut env, -1),
                    "lint_fill" => super::tools::lint_fill_current(&mut env),
                    _ => {
                        env.ed.lint_marks = Default::default();
                        env.ed.lint_report = None;
                    }
                }
                json!({ "report": self.ed.lint_report, "lint_marks": lint_json(&self.ed.lint_marks) })
            }
            "set_brush" => {
                let name = text(&m, &["preset", "value"]).ok_or_else(|| BridgeError::invalid_params("set_brush exige 'preset' (nome do pincel)"))?;
                let items = &self.ed.brushes.items;
                let i = items.iter().position(|b| b.preset.name.eq_ignore_ascii_case(name)).or_else(|| items.iter().position(|b| b.preset.name.to_lowercase() == name.to_lowercase()));
                let i = i.ok_or_else(|| BridgeError::invalid_params(format!("pincel desconhecido: {name}; há: {}", items.iter().map(|b| b.preset.name.as_str()).collect::<Vec<_>>().join(", "))))?;
                let p = items[i].preset.clone();
                let num = |k: &str| m.get(k).and_then(Value::as_f64).map(|v| v as f32);
                let o = &mut self.ed.opts;
                o.brush_preset = i;
                o.brush_custom = None;
                o.brush_size = num("size").unwrap_or(p.size).clamp(0.5, 2000.0);
                o.brush_opacity = num("opacity").unwrap_or(p.opacity).clamp(0.0, 1.0);
                // ajustes por cima do pincel (os campos do preset, como nas configurações avançadas): vira a cópia do usuário
                if let Some(c) = m.get("custom").filter(|c| !c.is_null()) {
                    let Value::Object(c) = c else { return Err(BridgeError::invalid_params("set_brush: 'custom' deve ser um objeto com campos do pincel")) };
                    let mut base = serde_json::to_value(&p).map_err(|e| BridgeError::invalid_params(e.to_string()))?;
                    let fields = aiai_paint::brush::BrushPreset::field_names();
                    for (k, v) in c {
                        if !fields.iter().any(|f| f == k) {
                            return Err(BridgeError::invalid_params(format!("set_brush: campo desconhecido no pincel: {k}")));
                        }
                        base[k] = v.clone();
                    }
                    let mut w: aiai_paint::brush::BrushPreset = serde_json::from_value(base).map_err(|e| BridgeError::invalid_params(format!("set_brush: custom inválido: {e}")))?;
                    w.size = o.brush_size;
                    w.opacity = o.brush_opacity;
                    w.validate().map_err(|e| BridgeError::invalid_params(format!("set_brush: {e}")))?;
                    o.brush_custom = Some(w);
                }
                // pincel do grupo borracha: a ferramenta Borracha apaga com ele (como escolher a borracha na prateleira)
                if p.group == "borracha" {
                    o.eraser_use_brush = true;
                    o.eraser_tip = super::editor::EraserTip::Brush;
                    o.eraser_size = o.brush_size;
                }
                json!({ "preset": p.name, "group": p.group, "size": o.brush_size, "opacity": o.brush_opacity })
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
            "dock" => self.bridge_dock(params)?,
            "normals" => {
                // normais personalizadas da Modelagem (escopo = seleção do modo Edição; sem seleção, a malha toda)
                use super::modeling::normals::{self as nm, NormalOp};
                let ms = &mut self.ed.modeling;
                let op = text(&m, &["op"]).ok_or_else(|| BridgeError::invalid_params("normals exige 'op'"))?;
                let op = NormalOp::from_key(op).ok_or_else(|| BridgeError::invalid_params(format!("op desconhecida: {op} ({})", NormalOp::ALL.iter().map(|o| o.key()).collect::<Vec<_>>().join(", "))))?;
                let mut st = ms.normals_ui;
                if let Some(f) = m.get("factor").and_then(Value::as_f64) {
                    st.factor = (f as f32).clamp(0.0, 1.0);
                }
                if let Some(s) = m.get("source").and_then(Value::as_u64) {
                    st.source = Some(s as usize);
                }
                let n = nm::run(ms, op, st).map_err(BridgeError::failed)?;
                json!({ "op": op.key(), "corners": n, "history": ms.history.labels() })
            }
            "uv" => {
                // UV da Modelagem: modo Edição, seleção (all | none | tree_seams = a cruz que abre a malha numa ilha) e
                // uma ação do editor de UV (mark_seam, unwrap, pack, project_box…)
                use super::modeling::uv as muv;
                use super::modeling::EditMode;
                if let Some(on) = m.get("edit").and_then(Value::as_bool) {
                    let t = self.tokens();
                    let ws = self.ws;
                    let mut notes = Vec::new();
                    let mut env = super::ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
                    super::modeling::set_mode(&mut env, if on { EditMode::Edit } else { EditMode::Object });
                }
                let ms = &mut self.ed.modeling;
                if let Some(sm) = text(&m, &["select_mode"]) {
                    let mode = match sm {
                        "vertex" => aiai_3d_edit::SelectMode::Vertex,
                        "edge" => aiai_3d_edit::SelectMode::Edge,
                        "face" => aiai_3d_edit::SelectMode::Face,
                        _ => return Err(BridgeError::invalid_params(format!("select_mode desconhecido: {sm} (vertex, edge, face)"))),
                    };
                    ms.set_select_mode(mode);
                }
                if let Some(sel) = text(&m, &["select"]) {
                    match sel {
                        "all" => ms.uv_select_all(),
                        "none" => ms.uv_select_none(),
                        "tree_seams" => {
                            let mesh = ms.uv_active_object().map(|o| o.mesh.clone()).ok_or_else(|| BridgeError::invalid_params("uv: sem objeto ativo"))?;
                            ms.set_select_mode(aiai_3d_edit::SelectMode::Edge);
                            ms.set_selection(aiai_3d_edit::Selection::from_edges(muv::spanning_tree_seams(&mesh)));
                        }
                        _ => return Err(BridgeError::invalid_params(format!("select desconhecido: {sel} (all, none, tree_seams)"))),
                    }
                }
                let mut msg = None;
                if let Some(a) = text(&m, &["op"]) {
                    let act = muv::action_from_key(a).ok_or_else(|| BridgeError::invalid_params(format!("ação de UV desconhecida: {a} ({})", muv::UvAction::ALL.iter().map(|x| x.key()).collect::<Vec<_>>().join(", "))))?;
                    let mut uvs = std::mem::take(&mut ms.uv);
                    let cam = ms.cam;
                    let r = muv::run_headless(ms, &mut uvs, cam, act);
                    ms.uv = uvs;
                    msg = Some(r.map_err(BridgeError::failed)?);
                }
                let report = ms.uv_active_object().and_then(|o| aiai_3d_edit::uv::metrics::uv_report(&o.mesh, 1024).ok());
                json!({
                    "mode": format!("{:?}", ms.mode).to_lowercase(),
                    "message": msg,
                    "selection": { "verts": ms.sel.verts.len(), "edges": ms.sel.edges.len(), "faces": ms.sel.faces.len() },
                    "islands": report.as_ref().map(|r| r.island_count),
                    "flipped": report.as_ref().map(|r| r.flipped_faces),
                    "coverage": report.as_ref().map(|r| (r.uv_area * 1000.0).round() / 1000.0),
                    "history": ms.history.labels(),
                })
            }
            "sculpt" => {
                // escultura da Modelagem: entrar/sair, pincel e ajustes, traços roteirizados (coordenadas 0..1 da vista
                // ativa) e as ferramentas de malha inteira
                use super::modeling::sculpt as sc;
                use super::modeling::EditMode;
                if let Some(on) = m.get("mode").and_then(Value::as_bool) {
                    let t = self.tokens();
                    let ws = self.ws;
                    let mut notes = Vec::new();
                    let mut env = super::ui_kit::Env { t, core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut notes, tool: &mut self.tool, ws };
                    super::modeling::set_mode(&mut env, if on { EditMode::Sculpt } else { EditMode::Object });
                }
                let ms = &mut self.ed.modeling;
                if let Some(b) = text(&m, &["brush"]) {
                    ms.sculpt.active = sc::brush_from_key(b).ok_or_else(|| BridgeError::invalid_params(format!("pincel desconhecido: {b} ({})", sc::SCULPT_BRUSHES.iter().map(|k| sc::brush_key(*k)).collect::<Vec<_>>().join(", "))))?;
                }
                if let Some(r) = m.get("radius").and_then(Value::as_f64) {
                    ms.sculpt.active_slot_mut().radius_px = (r as f32).clamp(sc::RADIUS_MIN_PX, sc::RADIUS_MAX_PX);
                }
                if let Some(v) = m.get("strength").and_then(Value::as_f64) {
                    ms.sculpt.active_slot_mut().brush.strength = (v as f32).clamp(0.0, sc::STRENGTH_MAX);
                }
                if let Some(sym) = m.get("symmetry").and_then(Value::as_array) {
                    for (i, v) in sym.iter().take(3).enumerate() {
                        ms.sculpt.symmetry.mirror[i] = v.as_bool().unwrap_or(false);
                    }
                }
                let mut dabs = None;
                if let Some(paths) = m.get("paths").and_then(Value::as_array) {
                    if ms.mode != EditMode::Sculpt {
                        return Err(BridgeError::invalid_params("sculpt: entre no modo Escultura (mode: true) antes dos traços"));
                    }
                    let r = ms.views.rects[ms.views.active];
                    if !r.is_positive() {
                        return Err(BridgeError::invalid_params("sculpt: o viewport da Modelagem ainda não foi desenhado"));
                    }
                    let paths: Vec<Vec<glam::Vec2>> = paths.iter().map(|p| p.as_array().map(|pts| pts.iter().filter_map(|q| Some(glam::Vec2::new(q.get(0)?.as_f64()? as f32, q.get(1)?.as_f64()? as f32))).collect()).unwrap_or_default()).collect();
                    let view = sc::ViewInfo { camera: ms.cam, size: glam::Vec2::new(r.width(), r.height()) };
                    let invert = m.get("invert").and_then(Value::as_bool).unwrap_or(false);
                    let brush = ms.sculpt.active;
                    dabs = Some(sc::run_script(ms, &view, brush, invert, &paths));
                }
                if let Some(op) = text(&m, &["op"]) {
                    let r = match op {
                        "mask_invert" => sc::mask_all(ms, true).map(|_| ()),
                        "mask_clear" => sc::mask_all(ms, false).map(|_| ()),
                        "smooth_all" => {
                            let n = ms.sculpt.smooth_all_iters;
                            sc::smooth_all(ms, n, 0.5)
                        }
                        "voxel_remesh" => sc::voxel_remesh(ms).map(|_| ()),
                        "symmetrize" => {
                            let a = ms.sculpt.symmetrize_axis;
                            sc::symmetrize(ms, a, true).map(|_| ())
                        }
                        _ => return Err(BridgeError::invalid_params(format!("op desconhecida: {op} (mask_invert, mask_clear, smooth_all, voxel_remesh, symmetrize)"))),
                    };
                    r.map_err(BridgeError::failed)?;
                }
                let o = ms.active.and_then(|a| ms.doc.objects.get(a));
                json!({
                    "mode": format!("{:?}", ms.mode).to_lowercase(),
                    "brush": sc::brush_key(ms.sculpt.active),
                    "radius": ms.sculpt.slot(ms.sculpt.active).radius_px,
                    "dabs": dabs,
                    "verts": o.map(|o| o.mesh.verts.len()),
                    "faces": o.map(|o| o.mesh.faces.len()),
                    "history": ms.history.labels(),
                    "status": ms.sculpt.status,
                    "error": ms.sculpt.error,
                })
            }
            "perf_samples" => {
                // E1 (R16 §11): start limpa e começa a gravar; stop para; read devolve as amostras brutas (ms por quadro)
                let perf = &mut self.ed.ibp.perf;
                match text(&m, &["op"]).unwrap_or("read") {
                    "start" => {
                        *perf = Default::default();
                        perf.on = true;
                    }
                    "stop" => perf.on = false,
                    "read" => {}
                    other => return Err(BridgeError::invalid_params(format!("perf_samples: op '{other}' (start|stop|read)"))),
                }
                json!({ "recording": perf.on, "ui_ms": perf.ui_ms, "ibp_paint_ms": perf.paint_ms, "present_ms": perf.present_ms })
            }
            "ibp_view" => {
                // estado de vista da barra da prévia (lista de avisos aberta, véu), para os ensaios
                if let Some(v) = m.get("show_notes").and_then(Value::as_bool) {
                    self.ed.ibp.show_notes = v;
                }
                if let Some(v) = m.get("veil").and_then(Value::as_bool) {
                    self.ed.ibp.veil = v;
                }
                json!({ "show_notes": self.ed.ibp.show_notes, "veil": self.ed.ibp.veil })
            }
            "modeling_view" => {
                // multivista da Modelagem: arranjo (single | split_v | split_h | quad) e o tipo de uma vista
                use super::modeling::multiview::{ViewKind, ViewLayout};
                let ms = &mut self.ed.modeling;
                if let Some(l) = text(&m, &["layout"]) {
                    let l = ViewLayout::from_key(l).ok_or_else(|| BridgeError::invalid_params(format!("layout desconhecido: {l} (single, split_v, split_h, quad)")))?;
                    ms.views.set_layout(&mut ms.cam, l);
                }
                if let Some(v) = text(&m, &["view"]) {
                    let k = ViewKind::from_key(v).ok_or_else(|| BridgeError::invalid_params(format!("vista desconhecida: {v} (perspective, ortho, front, back, right, left, top, bottom)")))?;
                    let slot = m.get("slot").and_then(Value::as_u64).map(|x| x as usize).unwrap_or(ms.views.active);
                    if !ms.views.visible().contains(&slot) {
                        return Err(BridgeError::invalid_params(format!("a vista {slot} não está à mostra no arranjo {}", ms.views.layout.key())));
                    }
                    let cam = if slot == ms.views.active { &mut ms.cam } else { &mut ms.views.cams[slot] };
                    k.apply(cam);
                }
                super::modeling::state_json(ms)["views"].clone()
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

/// Buracos do Verificar no GET_UI_STATE (centro, área, borda, parcial; o atual).
fn lint_json(m: &super::editor::LintMarks) -> Value {
    json!({
        "count": m.holes.len(),
        "current": m.current,
        "frame": m.frame,
        "holes": m.holes.iter().map(|h| json!({ "point": [h.point.x, h.point.y], "area": h.area, "border": h.border, "partial": h.partial })).collect::<Vec<_>>(),
    })
}
