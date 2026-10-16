//! Suíte única de integração do `aiai-studio`: o app de verdade (`StudioApp::frame` num `egui::Context` sem janela) com a
//! bridge ligada em loopback, falando por TCP/NDJSON com um cliente em outra thread — o mesmo caminho do `aiai-mcp`.
//! O que não dá para testar sem janela (captura do framebuffer) é coberto por teste de unidade em `bridge_host` e pela
//! demonstração ao vivo (evidências S02).

use aiai_studio::app::StudioApp;
use aiai_studio::cli::Cli;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

struct Client {
    addr: String,
    token: String,
}

impl Client {
    fn call(&self, action: &str, params: Value) -> Value {
        let mut s = TcpStream::connect(&self.addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        writeln!(s, "{}", json!({ "id": "t", "action": action, "params": params, "token": self.token })).unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("resposta não é JSON ({e}): {line:?}"))
    }

    fn ok(&self, action: &str, params: Value) -> Value {
        let r = self.call(action, params);
        assert_eq!(r["success"], true, "{action}: {r}");
        r["data"].clone()
    }

    fn err_code(&self, action: &str, params: Value) -> i64 {
        let r = self.call(action, params);
        assert_eq!(r["success"], false, "{action} deveria falhar: {r}");
        r["code"].as_i64().unwrap()
    }

    fn state(&self) -> Value {
        self.ok("GET_UI_STATE", json!({}))
    }

    fn center(&self, widget: &str) -> (f64, f64) {
        let st = self.state();
        let r = &st["widgets"][widget]["rect_logical"];
        assert!(r.is_array(), "sem retângulo para {widget}: {}", st["widgets"].as_object().unwrap().keys().cloned().collect::<Vec<_>>().join(","));
        (r[0].as_f64().unwrap() + r[2].as_f64().unwrap() / 2.0, r[1].as_f64().unwrap() + r[3].as_f64().unwrap() / 2.0)
    }

    /// Clique de verdade pelo caminho do egui: mover, apertar, soltar (a bridge espera a fila esvaziar).
    fn click(&self, x: f64, y: f64) {
        let d = self.ok(
            "INPUT",
            json!({ "wait": true, "events": [
                {"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": 2}] }),
        );
        assert_eq!(d["done"], true);
    }

    fn click_widget(&self, name: &str) {
        let (x, y) = self.center(name);
        self.click(x, y);
    }
}

fn create_node(name: &str, parent: Option<&str>, data: Value) -> Value {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    json!({ "command": c })
}

fn script(c: &Client) {
    // --- saúde e estado
    assert_eq!(c.ok("PING", json!({}))["pong"], true);
    let st = c.ok("GET_STATUS", json!({}));
    assert_eq!(st["mode"], "app");
    assert_eq!(st["dirty"], false);
    assert_eq!((st["can_undo"].clone(), st["revision"].clone()), (json!(false), json!(0)));
    assert!(st["window_size"][0].as_f64().unwrap() > 1000.0 && st["pixels_per_point"].as_f64().unwrap() > 0.0);
    let m = c.ok("GET_METRICS", json!({}));
    assert!(m["requests_total"].as_u64().unwrap_or(0) > 0, "GET_METRICS é do servidor da bridge (transporte): {m}");
    assert!(st["frame_count"].as_u64().unwrap_or(0) > 0 && st["frame_ms"].is_number(), "{st}");
    let ui = c.state();
    assert_eq!(ui["workspace"], "roteiro");
    assert!(ui["panels"].as_array().unwrap().iter().any(|p| p["name"] == "script" && p["rect_logical"].is_array() && p["rect_physical"].is_array()));
    assert!(ui["widgets"]["ws_tab:pintura"]["rect_logical"].is_array() && ui["widgets"]["menu:file"]["rect_logical"].is_array());
    assert_eq!(ui["window"]["title"], "AnimeIAI - Sem título");

    // --- INPUT: clicar na aba do workspace muda o workspace (mesmo efeito do mouse do usuário)
    c.click_widget("ws_tab:pintura");
    let ui = c.state();
    assert_eq!(ui["workspace"], "pintura", "{ui}");
    assert!(ui["panels"].as_array().unwrap().iter().any(|p| p["name"] == "canvas" && p["rect_logical"].is_array()), "pintura tem canvas");
    assert!(ui["widgets"]["tool:pen"]["rect_logical"].is_array(), "prateleira de ferramentas nos workspaces de canvas");
    c.click_widget("tool:pen");
    assert_eq!(c.state()["active_tool"], "pen");

    // --- UI_ACTION
    assert_eq!(c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "animacao"}))["workspace"], "animacao");
    assert_eq!(c.state()["workspace"], "animacao");
    assert_eq!(c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "fill"}))["active_tool"], "fill");
    let t = c.ok("UI_ACTION", json!({"action": "toggle_panel", "panel": "timeline"}));
    assert_eq!(t["visible"], false);
    let hidden = c.state();
    let tl = hidden["panels"].as_array().unwrap().iter().find(|p| p["name"] == "timeline").unwrap();
    assert_eq!(tl["visible"], false);
    c.ok("UI_ACTION", json!({"action": "toggle_panel", "panel": "timeline", "visible": true}));
    assert_eq!(c.ok("UI_ACTION", json!({"action": "set_theme", "theme": "light"}))["theme"], "light");
    assert_eq!(c.ok("UI_ACTION", json!({"action": "set_language", "language": "en"}))["language"], "en");
    assert_eq!(c.state()["language"], "en");
    c.ok("UI_ACTION", json!({"action": "set_language", "language": "pt"}));
    c.ok("UI_ACTION", json!({"action": "set_theme", "theme": "dark"}));
    c.ok("UI_ACTION", json!({"action": "open_dialog", "dialog": "about"}));
    assert!(c.state()["dialogs"].as_array().unwrap().iter().any(|d| d == "about"));
    c.ok("UI_ACTION", json!({"action": "close_dialog", "dialog": "about"}));
    assert_eq!(c.err_code("UI_ACTION", json!({"action": "set_workspace", "workspace": "nada"})), -32602);
    assert_eq!(c.err_code("UI_ACTION", json!({"action": "voar"})), -32602);
    assert_eq!(c.err_code("UI_ACTION", json!({"action": "toggle_panel", "panel": "mixer"})), -32602, "mixer não existe na Animação");
    assert_eq!(c.err_code("UI_ACTION", json!({})), -32602);
    c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "pintura"}));

    // --- validação de INPUT e SCREENSHOT (sem janela real não há captura; aqui só os erros de parâmetro)
    assert_eq!(c.err_code("INPUT", json!({})), -32602);
    assert_eq!(c.err_code("INPUT", json!({"events": []})), -32602);
    assert_eq!(c.err_code("INPUT", json!({"events": [{"kind": "teletransporte"}]})), -32602);
    assert_eq!(c.err_code("SCREENSHOT", json!({"region": [1, 2, 3]})), -32602);
    assert_eq!(c.err_code("SCREENSHOT", json!({"panel": "nao-existe"})), -32602);
    assert_eq!(c.err_code("SCREENSHOT", json!({"out": "relativo.png"})), -32602);
    assert_eq!(c.err_code("COMPARE", json!({})), -32000, "COMPARE ainda não existe: erro claro");
    assert_eq!(c.err_code("VOAR", json!({})), -32601);

    // --- documento pela bridge: aparece no app (sujo, título com *, status) e os atalhos/menus desfazem
    let ep = c.ok("APPLY_COMMAND", create_node("EP01", None, json!({"type": "episode", "number": 1})));
    assert_eq!(ep["can_undo"], true);
    let st = c.ok("GET_STATUS", json!({}));
    assert_eq!((st["dirty"].clone(), st["name"].clone(), st["revision"].clone()), (json!(true), json!("Sem título"), json!(1)));
    assert_eq!(c.state()["window"]["title"], "AnimeIAI - Sem título*");
    assert_eq!(c.ok("QUERY", json!({"query": {"kind": "tree"}}))["episodes"][0]["name"], "EP01");

    // Ctrl+Z real no egui desfaz o que a bridge fez (mesma sessão do usuário)
    c.ok("INPUT", json!({"wait": true, "events": [{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]}));
    let st = c.ok("GET_STATUS", json!({}));
    assert_eq!((st["can_undo"].clone(), st["can_redo"].clone(), st["dirty"].clone()), (json!(false), json!(true), json!(false)), "{st}");
    // Ctrl+Shift+Z refaz
    c.ok("INPUT", json!({"wait": true, "events": [{"kind": "key", "key": "z", "modifiers": ["ctrl", "shift"]}, {"kind": "wait", "frames": 2}]}));
    assert_eq!(c.ok("GET_STATUS", json!({}))["can_undo"], true);

    // Menu Editar > Desfazer por clique
    c.click_widget("menu:edit");
    c.click_widget("menu_item:edit.undo");
    let st = c.ok("GET_STATUS", json!({}));
    assert_eq!((st["can_undo"].clone(), st["can_redo"].clone()), (json!(false), json!(true)), "{st}");

    // Menu Arquivo > Novo com o documento sujo pergunta antes; Descartar zera
    c.ok("INPUT", json!({"wait": true, "events": [{"kind": "key", "key": "Y", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]}));
    assert_eq!(c.ok("GET_STATUS", json!({}))["dirty"], true);
    c.click_widget("menu:file");
    c.click_widget("menu_item:file.new");
    let ui = c.state();
    assert!(ui["dialogs"].as_array().unwrap().iter().any(|d| d == "confirm"), "pergunta sobre alterações não salvas: {}", ui["dialogs"]);
    c.click_widget("confirm:cancel");
    assert_eq!(c.ok("GET_STATUS", json!({}))["nodes"], 1, "Cancelar mantém o documento");
    c.click_widget("menu:file");
    c.click_widget("menu_item:file.new");
    c.click_widget("confirm:discard");
    let st = c.ok("GET_STATUS", json!({}));
    assert_eq!((st["dirty"].clone(), st["revision"].clone(), st["nodes"].clone()), (json!(false), json!(0), json!(0)), "{st}");

    // --- ferramentas pela bridge dentro do app: desenhar, pintar, renderizar (a imagem confirma o efeito)
    let id = |v: Value| v["created"][0].as_str().unwrap().to_string();
    c.ok("NEW_PROJECT", json!({"name": "arte", "settings": {"width": 160, "height": 90}}));
    let ep = id(c.ok("APPLY_COMMAND", create_node("EP", None, json!({"type": "episode", "number": 1}))));
    let sc = id(c.ok("APPLY_COMMAND", create_node("S", Some(&ep), json!({"type": "scene", "number": 1}))));
    let cut = id(c.ok("APPLY_COMMAND", create_node("C", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}))));
    let bg = id(c.ok("APPLY_COMMAND", create_node("BG", Some(&cut), json!({"type": "layer", "kind": "raster"}))));
    id(c.ok("APPLY_COMMAND", create_node("c1", Some(&bg), json!({"type": "cel", "number": 1}))));
    c.ok("GRADIENT", json!({"cut": cut, "frame": 0, "layer": bg, "kind": "linear", "from": [0, 0], "to": [0, 90], "stops": [[0.0, "#ff0000ff"], [1.0, "#ff0000ff"]]}));
    let r = c.ok("RENDER", json!({"cut": cut, "frame": 0, "inline": true}));
    assert_eq!((r["width"].clone(), r["height"].clone(), r["warnings"].clone()), (json!(160), json!(90), json!([])));
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD.decode(r["png_base64"].as_str().unwrap()).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(80, 45).0, [255, 0, 0, 255], "o app renderizou o gradiente vermelho");
    assert_eq!(c.ok("GET_STATUS", json!({}))["dirty"], true);
    assert!(c.ok("BRUSH_PRESETS", json!({}))["names"].as_array().unwrap().len() >= 5);
    // desfazer o gradiente pela bridge e conferir pelo render
    c.ok("UNDO", json!({}));
    let r = c.ok("RENDER", json!({"cut": cut, "frame": 0, "inline": true}));
    let png = base64::engine::general_purpose::STANDARD.decode(r["png_base64"].as_str().unwrap()).unwrap();
    assert_eq!(image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(80, 45).0[3], 0, "sem o gradiente o quadro é transparente");
}

#[test]
fn the_app_serves_the_bridge_and_reacts_to_real_egui_input() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = dir.path().join("bridge.json");
    // um único teste neste binário: mexer no ambiente é seguro
    unsafe {
        std::env::set_var("AIAI_BRIDGE_DISCOVERY", &discovery);
        std::env::remove_var("AIAI_BRIDGE_ADDR");
        std::env::remove_var("AIAI_BRIDGE_TOKEN");
    }
    let ctx = egui::Context::default();
    let cli = Cli { bridge: true, no_session: true, frames: 0, ..Default::default() };
    let mut app = StudioApp::build(&ctx, cli);
    let disc: Value = serde_json::from_str(&std::fs::read_to_string(&discovery).expect("a bridge gravou o arquivo de descoberta")).unwrap();
    let client = Client { addr: format!("127.0.0.1:{}", disc["port"].as_u64().expect("porta")), token: disc["token"].as_str().expect("token").to_string() };

    let done = Arc::new(AtomicBool::new(false));
    let done2 = Arc::clone(&done);
    let worker = std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| script(&client)));
        done2.store(true, Ordering::SeqCst);
        r
    });

    // o papel do eframe: raw_input_hook + um quadro por iteração
    let t0 = Instant::now();
    let mut raw_template = egui::RawInput::default();
    raw_template.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0)));
    let mut frames = 0u32;
    while !done.load(Ordering::SeqCst) {
        assert!(t0.elapsed() < Duration::from_secs(120), "o teste travou");
        let mut raw = raw_template.clone();
        raw.time = Some(t0.elapsed().as_secs_f64());
        raw.predicted_dt = 1.0 / 60.0;
        eframe::App::raw_input_hook(&mut app, &ctx, &mut raw);
        let mut out = ctx.run_ui(raw, |ui| app.frame(ui));
        out.textures_delta.clear();
        frames += 1;
        std::thread::sleep(Duration::from_millis(3));
    }
    if let Err(e) = worker.join().unwrap() {
        std::panic::resume_unwind(e);
    }
    assert!(frames > 20);
    drop(app);
    assert!(!discovery.exists(), "a descoberta some quando o app fecha");
}
