//! Lado do app da bridge local (ADR-004): no começo de cada quadro (`bridge_tick`, thread da UI) atende os pedidos que o
//! `aiai-bridge` enfileirou. Documento e ferramentas (APPLY_COMMAND, QUERY, UNDO, REDO, GET_LOG, NEW/OPEN/SAVE_PROJECT,
//! RENDER, DRAW_CURVES, FILL, LINT, PAINT, GRADIENT, TRANSFORM, REPEAT, BRUSH_PRESETS) vão para `aiai_bridge::core_ops` sobre
//! a MESMA `CoreSession` do usuário — entram no histórico, aparecem ao vivo e o Ctrl+Z do usuário os desfaz.
//! Daqui: GET_STATUS (fps, projeto, janela), SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION (e COMPARE com erro honesto até a sprint de métricas).
//!
//! Desligada por padrão: só com `--bridge` (ou `AIAI_BRIDGE=1`), só em 127.0.0.1 e com token.

use super::StudioApp;
use super::doc::is_file_action;
use super::input_queue;
use crate::cli::Cli;
use crate::i18n::tr;
use aiai_bridge::core_ops::{self, handle_core_action};
use aiai_bridge::protocol::{BridgeError, BridgeResult, code};
use aiai_bridge::{BridgeConfig, BridgeReceiver, BridgeServer, HostRequest};
use base64::Engine as _;
use serde_json::{Value, json};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Marca do pedido de screenshot dentro do `egui::UserData` (o `--screenshot` da CLI ignora estes).
pub(crate) struct BridgeShot(pub u64);

struct PendingShot {
    id: u64,
    req: HostRequest,
    /// Recorte em pontos lógicos (`None` = janela inteira) e o nome do alvo.
    crop: Option<(egui::Rect, String)>,
    max_width: Option<u32>,
    inline: bool,
    out: Option<std::path::PathBuf>,
    asked: Instant,
}

struct InputWaiter {
    req: HostRequest,
    queued: usize,
}

pub(crate) struct BridgeHost {
    server: BridgeServer,
    rx: BridgeReceiver,
    shots: Vec<PendingShot>,
    waiters: Vec<InputWaiter>,
    next_shot: u64,
}

impl BridgeHost {
    /// Sobe a bridge se `--bridge` ou `AIAI_BRIDGE=1`. Falha ao subir não derruba o app.
    pub(crate) fn start(cli: &Cli, ctx: &egui::Context) -> Option<Self> {
        let env_on = std::env::var("AIAI_BRIDGE").map(|v| matches!(v.as_str(), "1" | "true" | "yes")).unwrap_or(false);
        if !cli.bridge && !env_on {
            return None;
        }
        let wake_ctx = ctx.clone();
        let mut cfg = BridgeConfig::from_env();
        cfg.wake = Some(Arc::new(move || wake_ctx.request_repaint()));
        cfg.queue_capacity = 64;
        match BridgeServer::start(cfg) {
            Ok((server, rx)) => {
                eprintln!("bridge: escutando em {} (descoberta: {})", server.addr(), server.discovery_path().map(|p| p.display().to_string()).unwrap_or_default());
                Some(BridgeHost { server, rx, shots: Vec::new(), waiters: Vec::new(), next_shot: 1 })
            }
            Err(e) => {
                eprintln!("bridge: falha ao iniciar: {e}");
                tracing::error!("bridge indisponível: {e}");
                None
            }
        }
    }

    pub(crate) fn addr(&self) -> String {
        self.server.addr()
    }

    pub(crate) fn requests_total(&self) -> u64 {
        self.server.metrics().requests_total.load(Relaxed)
    }
}

fn is_png_path(p: &str) -> Result<std::path::PathBuf, BridgeError> {
    let path = std::path::PathBuf::from(p);
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        return Err(BridgeError::invalid_params("'out' deve ser um caminho absoluto, sem '..', terminado em .png"));
    }
    Ok(path)
}

impl StudioApp {
    /// Chamado uma vez por quadro, antes de desenhar: atende pedidos e conclui screenshots/esperas de entrada.
    pub(crate) fn bridge_tick(&mut self, ctx: &egui::Context) {
        let Some(host) = self.bridge.as_mut() else { return };
        let reqs = host.rx.try_recv_all();
        for r in reqs {
            self.bridge_dispatch(ctx, r);
        }
        self.bridge_poll_shots(ctx);
        self.bridge_poll_waiters();
        if self.input.pending() > 0 {
            ctx.request_repaint();
        }
        if self.bridge.as_ref().is_some_and(|b| !b.shots.is_empty() || !b.waiters.is_empty()) {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }

    fn bridge_dispatch(&mut self, ctx: &egui::Context, r: HostRequest) {
        if r.is_cancelled() {
            return;
        }
        let keys: Option<&[&str]> = match r.action.as_str() {
            "SCREENSHOT" => Some(&["panel", "region", "max_width", "out", "inline"]),
            "INPUT" => Some(&["events", "wait"]),
            "GET_STATUS" | "GET_UI_STATE" => Some(&[]),
            _ => None,
        };
        if let Some(Err(e)) = keys.map(|k| aiai_bridge::params_check::check_keys(&r.params, &r.action, k)) {
            return r.respond(Err(e));
        }
        match r.action.as_str() {
            "SCREENSHOT" => self.bridge_screenshot(ctx, r),
            "INPUT" => self.bridge_input(ctx, r),
            // Copiloto com o app aberto: a mesma fila e thread do painel (não congela a janela) e o fantasma no canvas;
            // a resposta sai quando o resultado chegar (CP1-19/20)
            "COPILOT" => {
                let prev = std::mem::replace(&mut self.core.source, aiai_core::InputSource::DirectToolCall);
                self.with_env(|env| super::copilot_ui::bridge(env, r));
                self.core.source = prev;
            }
            _ => {
                // chamada direta da bridge/MCP: as ações gravadas por ela são `DirectToolCall`, e a fonte do quadro volta depois
                let prev = std::mem::replace(&mut self.core.source, aiai_core::InputSource::DirectToolCall);
                let t0 = std::time::Instant::now();
                let res = self.bridge_exec(ctx, &r.action.clone(), &r.params.clone());
                self.core.source = prev;
                // E1: pedido de prévia atendido → marca o início da medida até a primeira imagem dela (CX12 C12-04)
                if r.action == "INBETWEEN_PREVIEW" {
                    if let Some(id) = res.as_ref().ok().and_then(|v| v.get("proposal_id")).and_then(Value::as_u64) {
                        self.ed.ibp.present = Some((id, t0, false));
                    }
                }
                r.respond(res);
            }
        }
    }

    fn bridge_exec(&mut self, ctx: &egui::Context, action: &str, params: &Value) -> BridgeResult {
        if action == "DELIVERY" {
            return super::panels::deliver::bridge(&self.core, &mut self.ed, ctx, params);
        }
        if core_ops::is_core_action(action) {
            let res = handle_core_action(&mut self.core, action, params).unwrap_or_else(|| Err(BridgeError::unknown_action(action)));
            if let Ok(v) = &res {
                if is_file_action(action) {
                    self.mark_saved();
                }
                // abrir pelo agente mostra os mesmos avisos (tiles danificados, trava, diário); salvar grava a miniatura
                match action {
                    "OPEN_PROJECT" => {
                        self.copilot_after_open();
                        self.note_open_report(v);
                    }
                    "NEW_PROJECT" => {
                        super::copilot_ui::load_project_state(&mut self.ed.copilot, None);
                    }
                    "SAVE_PROJECT" => {
                        self.copilot_after_save();
                        self.spawn_thumbnail();
                    }
                    _ => {}
                }
                if core_ops::is_mutating(action) || is_file_action(action) {
                    let what = v.get("description").and_then(Value::as_str).unwrap_or(action).to_string();
                    self.notice(format!("{}: {what}", tr("status.bridge")));
                    ctx.request_repaint();
                }
            }
            return res;
        }
        match action {
            "GET_STATUS" => Ok(self.bridge_status(ctx)),
            "GET_UI_STATE" => Ok(self.bridge_ui_state(ctx)),
            "UI_ACTION" => self.bridge_ui_action(ctx, params),
            "COMPARE" => Err(BridgeError::new(code::ACTION_FAILED, "COMPARE não implementado ainda: as métricas de imagem entram na sprint de validação (aiai-metrics)")),
            other => Err(BridgeError::unknown_action(other)),
        }
    }

    fn fps_value(&self) -> Option<f32> {
        match self.last_active {
            Some(t) if t.elapsed() < Duration::from_millis(600) && self.frame_ms > 0.0 => Some(1000.0 / self.frame_ms),
            _ => None,
        }
    }

    fn bridge_status(&mut self, ctx: &egui::Context) -> Value {
        let ppp = ctx.pixels_per_point();
        let screen = ctx.content_rect();
        let dt = ctx.input(|i| i.stable_dt);
        let (undo, redo) = self.core.history();
        json!({
            "mode": "app",
            "engine": "AnimeIAI Studio",
            "version": crate::brand::VERSION,
            "workspace": self.ws.key(),
            "fps": self.fps_value(),
            "frame_ms": self.frame_ms,
            "frame_dt_ms": dt * 1000.0,
            "frame_count": self.frame_count,
            "gpu": self.gpu_name,
            "project": self.core.path.as_ref().map(|p| p.display().to_string()),
            "name": self.doc_name(),
            "revision": self.core.project.revision,
            "dirty": self.doc_dirty(),
            "can_undo": self.core.can_undo(),
            "can_redo": self.core.can_redo(),
            "undo_steps": undo.len(),
            "redo_steps": redo.len(),
            "nodes": self.core.project.nodes.len(),
            "window_size": [screen.width(), screen.height()],
            "window_size_physical": [(screen.width() * ppp).round(), (screen.height() * ppp).round()],
            "pixels_per_point": ppp,
            "theme": self.prefs.theme.key(),
            "bridge": self.bridge.as_ref().map(|b| json!({ "addr": b.addr(), "requests_total": b.requests_total() })),
        })
    }

    // ------------------------------------------------------------------------------------------------ INPUT

    fn bridge_input(&mut self, ctx: &egui::Context, r: HostRequest) {
        let parsed = (|| -> Result<(Vec<input_queue::Step>, bool), BridgeError> {
            let events = r.params.get("events").and_then(Value::as_array).ok_or_else(|| BridgeError::invalid_params("INPUT exige 'events' (lista de objetos com `kind`)"))?;
            if events.is_empty() || events.len() > input_queue::MAX_EVENTS {
                return Err(BridgeError::invalid_params(format!("INPUT: 'events' deve ter de 1 a {} eventos", input_queue::MAX_EVENTS)));
            }
            let steps = input_queue::parse(events).map_err(BridgeError::invalid_params)?;
            Ok((steps, r.params.get("wait").and_then(Value::as_bool).unwrap_or(false)))
        })();
        let (steps, wait) = match parsed {
            Ok(v) => v,
            Err(e) => return r.respond(Err(e)),
        };
        let queued = steps.len();
        if let Err(e) = self.input.push(steps) {
            return r.respond(Err(BridgeError::new(code::BUSY, e)));
        }
        ctx.request_repaint();
        if wait {
            // a resposta sai quando a fila esvazia (o prazo da ação limita a espera)
            if let Some(b) = self.bridge.as_mut() {
                b.waiters.push(InputWaiter { req: r, queued });
            }
        } else {
            let pending = self.input.pending();
            r.respond(Ok(json!({ "queued": queued, "pending": pending })));
        }
    }

    fn bridge_poll_waiters(&mut self) {
        if self.input.pending() > 0 {
            return;
        }
        let Some(b) = self.bridge.as_mut() else { return };
        for w in std::mem::take(&mut b.waiters) {
            if !w.req.is_cancelled() {
                let queued = w.queued;
                w.req.respond(Ok(json!({ "queued": queued, "pending": 0, "done": true })));
            }
        }
    }

    // ------------------------------------------------------------------------------------------------ SCREENSHOT

    /// `SCREENSHOT {region?: [x,y,w,h] | {x,y,w,h} (pontos lógicos), panel?: nome, max_width?, inline?, out?}`.
    fn bridge_screenshot(&mut self, ctx: &egui::Context, r: HostRequest) {
        let parsed = (|| -> Result<(Option<(egui::Rect, String)>, Option<u32>, bool, Option<std::path::PathBuf>), BridgeError> {
            let p = r.params.as_object().cloned().unwrap_or_default();
            let mut crop = None;
            if let Some(name) = p.get("panel").and_then(Value::as_str) {
                let key = if self.rects.get(name).is_some() { name.to_string() } else { format!("panel:{name}") };
                let rect = self.rects.get(&key).ok_or_else(|| {
                    let known: Vec<&str> = self.rects.all().keys().map(String::as_str).collect();
                    BridgeError::invalid_params(format!("painel/elemento desconhecido ou fechado: {name}; conhecidos agora: {}", known.join(", ")))
                })?;
                crop = Some((rect, key));
            }
            if let Some(reg) = p.get("region").filter(|v| !v.is_null()) {
                let nums: Vec<f64> = match reg {
                    Value::Array(a) => a.iter().filter_map(Value::as_f64).collect(),
                    Value::Object(o) => ["x", "y", "w", "h"].iter().filter_map(|k| o.get(*k).and_then(Value::as_f64)).collect(),
                    _ => Vec::new(),
                };
                if nums.len() != 4 || nums.iter().any(|n| !n.is_finite()) || nums[2] <= 0.0 || nums[3] <= 0.0 {
                    return Err(BridgeError::invalid_params("'region' deve ser [x, y, largura, altura] em pontos lógicos (largura e altura > 0)"));
                }
                crop = Some((egui::Rect::from_min_size(egui::pos2(nums[0] as f32, nums[1] as f32), egui::vec2(nums[2] as f32, nums[3] as f32)), "region".to_string()));
            }
            let max_width = match p.get("max_width").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(v.as_u64().filter(|w| (16..=8192).contains(w)).ok_or_else(|| BridgeError::invalid_params("'max_width' fora de 16..8192"))? as u32),
            };
            let out = p.get("out").and_then(Value::as_str).map(is_png_path).transpose()?;
            let inline = p.get("inline").and_then(Value::as_bool).unwrap_or(false) || out.is_none();
            Ok((crop, max_width, inline, out))
        })();
        let (crop, max_width, inline, out) = match parsed {
            Ok(v) => v,
            Err(e) => return r.respond(Err(e)),
        };
        let Some(b) = self.bridge.as_mut() else { return };
        let id = b.next_shot;
        b.next_shot += 1;
        b.shots.push(PendingShot { id, req: r, crop, max_width, inline, out, asked: Instant::now() });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(BridgeShot(id))));
        ctx.request_repaint();
    }

    fn bridge_poll_shots(&mut self, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        let ws = self.ws.key();
        let Some(b) = self.bridge.as_mut() else { return };
        if b.shots.is_empty() {
            return;
        }
        let events: Vec<(u64, Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => user_data.data.as_ref().and_then(|d| d.downcast_ref::<BridgeShot>()).map(|s| (s.0, image.clone())),
                    _ => None,
                })
                .collect()
        });
        for (id, image) in events {
            if let Some(pos) = b.shots.iter().position(|s| s.id == id) {
                let shot = b.shots.remove(pos);
                if shot.req.is_cancelled() {
                    continue;
                }
                // PNG + base64 fora da thread da UI
                let _ = std::thread::Builder::new().name("aiai-bridge-shot".into()).spawn(move || {
                    let res = encode_shot(&image, &shot, ppp, ws);
                    shot.req.respond(res);
                });
            }
        }
        b.shots.retain(|s| !s.req.is_cancelled() && s.asked.elapsed() < Duration::from_secs(25));
    }
}

/// Recorta (com limites seguros), reduz se pedido, codifica em PNG e monta a resposta.
fn encode_shot(image: &egui::ColorImage, shot: &PendingShot, ppp: f32, ws: &str) -> BridgeResult {
    let (iw, ih) = (image.size[0], image.size[1]);
    if iw == 0 || ih == 0 {
        return Err(BridgeError::failed("captura vazia"));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (0usize, 0usize, iw, ih);
    let mut target = "window".to_string();
    if let Some((rect, name)) = &shot.crop {
        let cx0 = ((rect.min.x * ppp).floor().max(0.0) as usize).min(iw);
        let cy0 = ((rect.min.y * ppp).floor().max(0.0) as usize).min(ih);
        let cx1 = ((rect.max.x * ppp).ceil().max(0.0) as usize).min(iw);
        let cy1 = ((rect.max.y * ppp).ceil().max(0.0) as usize).min(ih);
        if cx1 <= cx0 || cy1 <= cy0 {
            return Err(BridgeError::invalid_params(format!("o recorte ({name}) está fora da janela ({iw}x{ih} px)")));
        }
        (x0, y0, x1, y1) = (cx0, cy0, cx1, cy1);
        target = name.clone();
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in y0..y1 {
        for c in &image.pixels[row * iw + x0..row * iw + x1] {
            rgba.extend_from_slice(&c.to_array());
        }
    }
    let mut img = image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or_else(|| BridgeError::failed("buffer de captura inconsistente"))?;
    if let Some(mw) = shot.max_width.filter(|mw| img.width() > *mw) {
        let nh = ((img.height() as f64) * (mw as f64) / (img.width() as f64)).round().max(1.0) as u32;
        img = image::imageops::resize(&img, mw, nh, image::imageops::FilterType::Triangle);
    }
    let mut png = Vec::new();
    {
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(img.as_raw(), img.width(), img.height(), image::ColorType::Rgba8.into())
            .map_err(|e| BridgeError::failed(format!("PNG: {e}")))?;
    }
    let mut result = json!({ "width": img.width(), "height": img.height(), "target": target, "workspace": ws, "pixels_per_point": ppp, "bytes": png.len() });
    if let Some(path) = &shot.out {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| BridgeError::failed(format!("criando {}: {e}", dir.display())))?;
        }
        std::fs::write(path, &png).map_err(|e| BridgeError::failed(format!("gravando {}: {e}", path.display())))?;
        result["path"] = json!(path.display().to_string());
    }
    if shot.inline {
        result["png_base64"] = json!(base64::engine::general_purpose::STANDARD.encode(&png));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn shot(crop: Option<(egui::Rect, String)>, max_width: Option<u32>, inline: bool, out: Option<std::path::PathBuf>) -> PendingShot {
        let (tx, _rx) = crossbeam_channel::bounded(1);
        let req = HostRequest { id: "t".into(), action: "SCREENSHOT".into(), params: Value::Null, reply: tx, cancelled: Arc::new(AtomicBool::new(false)) };
        PendingShot { id: 1, req, crop, max_width, inline, out, asked: Instant::now() }
    }

    /// Imagem 100x50 em que o pixel (x, y) tem R = x e G = y.
    fn gradient() -> egui::ColorImage {
        let mut img = egui::ColorImage::filled([100, 50], egui::Color32::BLACK);
        for y in 0..50 {
            for x in 0..100 {
                img.pixels[y * 100 + x] = egui::Color32::from_rgb(x as u8, y as u8, 7);
            }
        }
        img
    }

    fn decode(v: &Value) -> image::RgbaImage {
        let b = base64::engine::general_purpose::STANDARD.decode(v["png_base64"].as_str().unwrap()).unwrap();
        image::load_from_memory(&b).unwrap().to_rgba8()
    }

    #[test]
    fn whole_window_crop_by_logical_region_and_resize() {
        let all = encode_shot(&gradient(), &shot(None, None, true, None), 2.0, "pintura").unwrap();
        assert_eq!((all["width"].as_u64(), all["height"].as_u64(), all["target"].as_str()), (Some(100), Some(50), Some("window")));
        assert_eq!(decode(&all).get_pixel(99, 49).0, [99, 49, 7, 255], "PNG sem perdas");

        // região lógica (10,5)-(30,15) com 2 px/ponto = pixels (20,10)-(60,30)
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 5.0), egui::vec2(20.0, 10.0));
        let part = encode_shot(&gradient(), &shot(Some((rect, "region".into())), None, true, None), 2.0, "pintura").unwrap();
        assert_eq!((part["width"].as_u64(), part["height"].as_u64()), (Some(40), Some(20)));
        assert_eq!(decode(&part).get_pixel(0, 0).0, [20, 10, 7, 255]);
        assert_eq!(decode(&part).get_pixel(39, 19).0, [59, 29, 7, 255]);

        // recorte além da borda é limitado; fora da janela é erro
        let edge = egui::Rect::from_min_size(egui::pos2(40.0, 20.0), egui::vec2(100.0, 100.0));
        let clipped = encode_shot(&gradient(), &shot(Some((edge, "panel:x".into())), None, true, None), 2.0, "pintura").unwrap();
        assert_eq!((clipped["width"].as_u64(), clipped["height"].as_u64(), clipped["target"].as_str()), (Some(20), Some(10), Some("panel:x")));
        let outside = egui::Rect::from_min_size(egui::pos2(900.0, 900.0), egui::vec2(10.0, 10.0));
        assert!(encode_shot(&gradient(), &shot(Some((outside, "region".into())), None, true, None), 1.0, "p").is_err());

        let small = encode_shot(&gradient(), &shot(None, Some(50), true, None), 1.0, "p").unwrap();
        assert_eq!((small["width"].as_u64(), small["height"].as_u64()), (Some(50), Some(25)));
    }

    #[test]
    fn out_writes_the_file_and_inline_controls_the_base64() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("a").join("s.png");
        let r = encode_shot(&gradient(), &shot(None, None, false, Some(out.clone())), 1.0, "p").unwrap();
        assert!(r.get("png_base64").is_none() && r["path"] == json!(out.display().to_string()));
        assert_eq!(image::open(&out).unwrap().width(), 100);
        let r = encode_shot(&gradient(), &shot(None, None, true, Some(out)), 1.0, "p").unwrap();
        assert!(r["png_base64"].is_string() && r["path"].is_string());
    }

    #[test]
    fn png_paths_must_be_absolute_png_without_parent_dirs() {
        assert!(is_png_path("relativo.png").is_err());
        assert!(is_png_path(r"C:\x\..\y.png").is_err());
        assert!(is_png_path(r"C:\x\y.txt").is_err());
        assert!(is_png_path(r"C:\x\y.PNG").is_ok());
    }
}
