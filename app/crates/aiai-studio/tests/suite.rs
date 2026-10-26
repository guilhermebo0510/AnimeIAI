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
        let mut st = self.state();
        // painel em aba atrás de outro (a Animação tem Mesa de luz/Cores e Camadas/Propriedades em abas): traz para a frente
        if !st["widgets"][widget]["rect_logical"].is_array() {
            let panel = match widget.split(':').next().unwrap_or("") {
                "colors" => Some("palette"),
                "layers" | "cels" => Some("layers"),
                "project" => Some("project"),
                "props" => Some("properties"),
                _ => None,
            };
            if let Some(p) = panel {
                self.ok("UI_ACTION", json!({"action": "open_panel", "panel": p}));
                st = self.state();
            }
        }
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

    fn input(&self, events: Value) {
        let d = self.ok("INPUT", json!({ "wait": true, "events": events }));
        assert_eq!(d["done"], true);
    }

    /// Ponto do desenho (pixels do canvas) -> pontos lógicos da janela, pela matriz que o app publica.
    fn canvas_pt(&self, x: f64, y: f64) -> (f64, f64) {
        let st = self.state();
        let c = &st["editor"]["canvas"];
        let o = (c["origin_logical"][0].as_f64().unwrap(), c["origin_logical"][1].as_f64().unwrap());
        let m: Vec<f64> = c["m"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        (o.0 + m[0] * x + m[1] * y, o.1 + m[2] * x + m[3] * y)
    }

    /// Arrasto pelo caminho (pontos do desenho), botão apertado o tempo todo; `pen` = caneta com pressão.
    fn drag_canvas(&self, path: &[(f64, f64)], pen: bool) {
        // como um mouse de verdade: amostras a cada ~4 px do desenho ao longo do caminho
        let mut dense: Vec<(f64, f64)> = vec![path[0]];
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            let n = (((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() / 4.0).ceil().max(1.0) as usize;
            for i in 1..=n {
                dense.push((a.0 + (b.0 - a.0) * i as f64 / n as f64, a.1 + (b.1 - a.1) * i as f64 / n as f64));
            }
        }
        let pts: Vec<(f64, f64)> = dense.iter().map(|(x, y)| self.canvas_pt(*x, *y)).collect();
        let mut ev = vec![json!({"kind": "mouse_move", "x": pts[0].0, "y": pts[0].1}), json!({"kind": "wait", "frames": 1})];
        if pen {
            ev.push(json!({"kind": "pen", "x": pts[0].0, "y": pts[0].1, "pressure": 0.7, "phase": "down"}));
            for p in &pts[1..] {
                ev.push(json!({"kind": "pen", "x": p.0, "y": p.1, "pressure": 0.7, "phase": "move"}));
            }
            let l = pts.last().unwrap();
            ev.push(json!({"kind": "pen", "x": l.0, "y": l.1, "pressure": 0.7, "phase": "up"}));
        } else {
            ev.push(json!({"kind": "mouse_down", "x": pts[0].0, "y": pts[0].1, "button": "primary"}));
            for p in &pts[1..] {
                ev.push(json!({"kind": "mouse_move", "x": p.0, "y": p.1}));
            }
            let l = pts.last().unwrap();
            ev.push(json!({"kind": "mouse_up", "x": l.0, "y": l.1, "button": "primary"}));
        }
        ev.push(json!({"kind": "wait", "frames": 2}));
        self.input(Value::Array(ev));
    }

    fn click_canvas(&self, x: f64, y: f64) {
        let (lx, ly) = self.canvas_pt(x, y);
        self.click(lx, ly);
    }

    fn latency_count(&self) -> usize {
        self.state()["editor"]["latency_log"].as_array().map(|a| a.len()).unwrap_or(0)
    }

    /// Espera o canvas mostrar o documento depois de uma edição (a medida de latência fecha quando a textura nova chega).
    fn wait_canvas(&self, before: usize) {
        let t0 = Instant::now();
        while self.latency_count() <= before {
            assert!(t0.elapsed() < Duration::from_secs(20), "o canvas não atualizou depois da edição: {}", self.state()["editor"]["render"]);
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn render_px(&self, cut: &str, frame: u32, x: usize, y: usize) -> [u8; 4] {
        use base64::Engine as _;
        let r = self.ok("RENDER", json!({"cut": cut, "frame": frame, "inline": true}));
        let png = base64::engine::general_purpose::STANDARD.decode(r["png_base64"].as_str().unwrap()).unwrap();
        image::load_from_memory(&png).unwrap().to_rgba8().get_pixel(x as u32, y as u32).0
    }

    fn strokes(&self, cel: &str) -> Vec<Value> {
        self.node(cel)["strokes"].as_array().cloned().unwrap_or_default()
    }

    /// Dados de um nó (sem o envelope com id/pai/proveniência).
    fn node(&self, id: &str) -> Value {
        self.ok("QUERY", json!({"query": {"kind": "node", "id": id}}))["data"].clone()
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
    // a prateleira é a do perfil da Pintura: balde, pincel e borracha, sem caneta (não se desenha linha aqui)
    assert!(ui["widgets"]["tool:fill"]["rect_logical"].is_array(), "prateleira de ferramentas nos workspaces de canvas");
    assert!(ui["widgets"]["tool:pen"].is_null(), "a caneta não aparece na prateleira da Pintura");
    // a Seleção abre a prateleira (acima do Mover) em todos os workspaces de desenho
    assert_eq!((ui["shelf_tools"][0].clone(), ui["shelf_tools"][1].clone()), (json!("lasso"), json!("move")));
    assert!(ui["shelf_tools"].as_array().unwrap().contains(&json!("fill")));
    c.click_widget("tool:eraser");
    assert_eq!(c.state()["active_tool"], "eraser");
    assert_eq!(c.err_code("UI_ACTION", json!({"action": "select_tool", "tool": "pen"})), -32602, "caneta fora da prateleira da Pintura");
    // Exibir > Painéis lista o catálogo inteiro: painel fora do layout entra (e sai) pelo menu
    c.click_widget("menu:view");
    c.click_widget("menu_item:view.panels");
    c.click_widget("menu_item:view.panel.script");
    let ui = c.state();
    assert!(ui["panels"].as_array().unwrap().iter().any(|p| p["name"] == "script" && p["visible"] == true), "o Roteiro entrou no layout da Pintura");
    assert_eq!(ui["layout"]["label"], "custom");
    c.ok("UI_ACTION", json!({"action": "layout", "op": "save", "name": "com roteiro"}));
    assert_eq!(c.state()["layout"]["label"], "com roteiro");
    c.ok("UI_ACTION", json!({"action": "layout", "op": "reset"}));
    let ui = c.state();
    assert_eq!(ui["layout"]["label"], "factory");
    assert!(!ui["panels"].as_array().unwrap().iter().any(|p| p["name"] == "script"), "a fábrica da Pintura não tem o Roteiro");
    c.ok("UI_ACTION", json!({"action": "layout", "op": "apply", "name": "com roteiro"}));
    assert!(c.state()["panels"].as_array().unwrap().iter().any(|p| p["name"] == "script"));
    c.ok("UI_ACTION", json!({"action": "layout", "op": "delete", "name": "com roteiro"}));
    c.ok("UI_ACTION", json!({"action": "layout", "op": "reset"}));
    assert_eq!(c.state()["layout"]["named"], json!([]));

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
    assert_eq!(c.err_code("COMPARE", json!({})), -32602, "COMPARE existe (S21) e valida os parâmetros");
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
    // Novo abre a janela de projeto; Criar pergunta antes de descartar
    c.click_widget("menu:file");
    c.click_widget("menu_item:file.new");
    assert!(c.state()["dialogs"].as_array().unwrap().iter().any(|d| d == "start"), "Novo abre a janela de projeto");
    c.click_widget("start:create");
    let ui = c.state();
    assert!(ui["dialogs"].as_array().unwrap().iter().any(|d| d == "confirm"), "pergunta sobre alterações não salvas: {}", ui["dialogs"]);
    c.click_widget("confirm:cancel");
    assert_eq!(c.ok("GET_STATUS", json!({}))["nodes"], 1, "Cancelar mantém o documento");
    c.click_widget("start:create");
    c.click_widget("confirm:discard");
    assert!(!c.state()["dialogs"].as_array().unwrap().iter().any(|d| d == "start"), "a janela fecha com o projeto criado");
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

/// O editor de verdade pelo caminho do usuário: projeto, camadas, caneta (com pressão), balde, pincel, borracha, mover,
/// conta-gotas, visibilidade, desfazer, zoom/pan e linha do tempo — tudo conferido pelo documento e pelo render.
fn editor_script(c: &Client) {
    c.ok("NEW_PROJECT", json!({"name": "editor", "settings": {"width": 320, "height": 180}}));
    c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "animacao"}));
    // o balde escolhido antes trouxe o painel Ferramenta para a frente da aba do Projeto
    c.ok("UI_ACTION", json!({"action": "open_panel", "panel": "project"}));
    // projeto vazio: o painel Projeto oferece começar com episódio, cena e corte (um passo de undo)
    c.click_widget("project:quick_start");
    let cut = c.state()["editor"]["cut"].as_str().expect("corte ativo depois de começar").to_string();
    assert_eq!(c.ok("GET_STATUS", json!({}))["undo_steps"], 1, "começar = um passo");

    // menu de nova camada: vetor, preenchimento, raster (cada uma entra acima da ativa)
    for kind in ["vector", "fill", "raster"] {
        c.click_widget("layers:add");
        c.click_widget(&format!("layers:add.{kind}"));
    }
    let st = c.state();
    let kinds: Vec<&str> = st["editor"]["layers"].as_array().unwrap().iter().map(|l| l["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["vector", "raster", "fill"], "o preenchimento entra abaixo da linha; {kinds:?}");
    let id_of = |st: &Value, i: usize| st["editor"]["layers"][i]["id"].as_str().unwrap().to_string();
    let (fill, vector) = (id_of(&st, 2), id_of(&st, 0));
    let cel_of = |layer: &str, nth: usize| c.ok("QUERY", json!({"query": {"kind": "tree", "id": layer}}))["children"][nth]["id"].as_str().unwrap().to_string();

    // caneta: um quadrado fechado por arrasto (caneta com pressão 0,7); o traço cai onde o cursor passou
    c.click_widget("layers:row:0");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "pen"}));
    let n = c.latency_count();
    let square = [(80.0, 40.0), (160.0, 40.0), (240.0, 40.0), (240.0, 90.0), (240.0, 140.0), (160.0, 140.0), (80.0, 140.0), (80.0, 90.0), (80.0, 40.0)];
    c.drag_canvas(&square, true);
    c.wait_canvas(n);
    let vcel = cel_of(&vector, 0);
    let node = c.node(&vcel);
    let strokes = node["strokes"].as_array().unwrap_or_else(|| panic!("o cel não tem traços: {node} (editor: {})", c.state()["editor"]));
    assert_eq!(strokes.len(), 1, "um arrasto = um traço");
    let pts = strokes[0]["points"].as_array().unwrap();
    let (fx, fy) = (pts[0]["x"].as_f64().unwrap(), pts[0]["y"].as_f64().unwrap());
    assert!((fx - 80.0).abs() < 1.0 && (fy - 40.0).abs() < 1.0, "o traço começa onde o cursor apertou: ({fx}, {fy})");
    let xs: Vec<f64> = pts.iter().map(|p| p["x"].as_f64().unwrap()).collect();
    assert!(xs.iter().cloned().fold(f64::MIN, f64::max) > 236.0, "e vai até o outro lado do quadrado");
    assert!(pts.iter().any(|p| (p["pressure"].as_f64().unwrap() - 0.7).abs() < 0.01), "a pressão da caneta entra nas amostras");
    assert!(c.render_px(&cut, 0, 160, 40)[3] > 0, "o render mostra a linha");
    assert_eq!(c.render_px(&cut, 0, 160, 90)[3], 0, "dentro do quadrado ainda é transparente");

    // balde: cor vermelha, clique dentro do quadrado, vai para a camada de preenchimento ligada à linha
    c.ok("UI_ACTION", json!({"action": "set_color", "color": "#ff0000"}));
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "fill"}));
    let n = c.latency_count();
    c.click_canvas(160.0, 90.0);
    c.wait_canvas(n);
    assert_eq!(c.render_px(&cut, 0, 160, 90), [255, 0, 0, 255], "o balde pintou o interior de vermelho");
    assert_eq!(c.render_px(&cut, 0, 20, 20), [0, 0, 0, 0], "fora do quadrado nada muda (a forma fechou)");
    let fcel = cel_of(&fill, 0);
    assert_eq!(c.node(&fcel)["fills"].as_array().unwrap().len(), 1);

    // conta-gotas: troca a cor e copia do canvas
    c.ok("UI_ACTION", json!({"action": "set_color", "color": "#00ff00"}));
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "eyedropper"}));
    c.click_canvas(160.0, 90.0);
    assert_eq!(c.state()["editor"]["color"], "#ff0000ff", "o conta-gotas amostrou o vermelho renderizado");

    // pincel raster: cor azul, traço fora do quadrado
    c.ok("UI_ACTION", json!({"action": "set_color", "color": "#0000ff"}));
    c.click_widget("layers:row:1");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "brush"}));
    let n = c.latency_count();
    c.drag_canvas(&[(20.0, 160.0), (40.0, 162.0), (60.0, 160.0), (90.0, 164.0)], false);
    c.wait_canvas(n);
    let px = c.render_px(&cut, 0, 40, 162);
    assert!(px[3] > 200 && px[2] > 200 && px[0] < 60, "pincel azul no raster: {px:?}");

    // visibilidade: o olho da camada de preenchimento some com o vermelho e volta
    c.click_widget("layers:eye:2");
    assert_eq!(c.render_px(&cut, 0, 160, 90)[3], 0, "camada oculta não aparece no render");
    c.click_widget("layers:eye:2");
    assert_eq!(c.render_px(&cut, 0, 160, 90), [255, 0, 0, 255]);

    // desfazer/refazer pelo teclado: o pincel some e volta
    let rev = c.ok("GET_STATUS", json!({}))["revision"].as_u64().unwrap();
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    c.click_widget("layers:eye:2");
    c.click_widget("layers:eye:2");
    c.input(json!([{"kind": "key", "key": "z", "modifiers": ["ctrl", "shift"]}, {"kind": "wait", "frames": 2}]));
    assert!(c.ok("GET_STATUS", json!({}))["revision"].as_u64().unwrap() > rev);

    // borracha vetorial (modo padrão "trecho tocado"): atravessa o lado de cima do quadrado; o traço fechado vira um
    // pedaço aberto com o vão onde a borracha passou; um passo de undo devolve o quadrado
    c.click_widget("layers:row:0");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "eraser"}));
    let original = c.strokes(&vcel)[0]["id"].as_str().unwrap().to_string();
    c.drag_canvas(&[(160.0, 30.0), (160.0, 50.0)], false);
    let t0 = Instant::now();
    while c.strokes(&vcel).first().and_then(|s| s["id"].as_str()) == Some(original.as_str()) {
        assert!(t0.elapsed() < Duration::from_secs(10), "a borracha não apagou");
        std::thread::sleep(Duration::from_millis(20));
    }
    let left = c.strokes(&vcel);
    assert_eq!(left.len(), 1, "um pedaço aberto: {left:?}");
    assert_eq!(left[0]["closed"], false);
    assert_eq!(c.render_px(&cut, 0, 160, 40)[3], 0, "vão onde a borracha passou");
    assert!(c.render_px(&cut, 0, 240, 90)[3] > 0, "o resto do quadrado fica");
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.strokes(&vcel).len(), 1, "Ctrl+Z devolve o traço apagado");
    assert_eq!(c.strokes(&vcel)[0]["id"], original);

    // selecionar (retângulo) + mover: aplicado ao soltar (sem Enter), o traço anda 30 px para a direita
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "lasso"}));
    c.drag_canvas(&[(60.0, 20.0), (260.0, 160.0)], false);
    assert!(c.state()["editor"]["selection"].is_object(), "seleção criada");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "move"}));
    c.drag_canvas(&[(100.0, 60.0), (115.0, 60.0), (130.0, 60.0)], false);
    let moved = c.node(&vcel)["strokes"][0]["points"][0]["x"].as_f64().unwrap();
    assert!((moved - 110.0).abs() < 1.5, "soltar aplicou o movimento: x = {moved}");
    let sel = c.state()["editor"]["selection"].clone();
    assert!(sel.is_object() && sel["offset"][0].as_f64().unwrap().abs() < 1e-3, "a seleção fica no lugar novo, sem nada pendente: {sel}");
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    assert!((c.node(&vcel)["strokes"][0]["points"][0]["x"].as_f64().unwrap() - 80.0).abs() < 1.0, "um Ctrl+Z desfaz o mover");
    c.input(json!([{"kind": "key", "key": "Escape"}, {"kind": "wait", "frames": 2}]));
    assert!(c.state()["editor"]["selection"].is_null(), "Esc limpa a seleção");

    // Seleção (S) / Selecionar traços: o clique escolhe o traço, no vazio tira a seleção, a caixa pega o que toca. O Mover
    // só move: apertar longe do traço escolhido ainda leva só ele (como o Photoshop com seleção ativa)
    let first_x = |c: &Client| c.node(&vcel)["strokes"][0]["points"][0]["x"].as_f64().unwrap();
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "select", "mode": "strokes"}));
    assert_eq!(c.state()["active_tool"], "lasso", "a Seleção guarda a chave antiga");
    assert_eq!(c.state()["editor"]["select_tool"]["tool"], "strokes");
    c.click_canvas(160.0, 40.0);
    assert_eq!(c.state()["editor"]["selection"]["strokes"].as_array().map(|a| a.len()), Some(1), "o clique no traço o escolhe");
    c.click_canvas(20.0, 160.0);
    assert!(c.state()["editor"]["selection"].is_null(), "clique no vazio tira a seleção");
    c.drag_canvas(&[(60.0, 20.0), (120.0, 60.0)], false);
    assert_eq!(c.state()["editor"]["selection"]["strokes"].as_array().map(|a| a.len()), Some(1), "a caixa pega o traço que ela toca");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "move"}));
    c.drag_canvas(&[(20.0, 160.0), (25.0, 160.0), (30.0, 160.0)], false);
    assert!((first_x(c) - 90.0).abs() < 1.0, "o Mover levou o traço escolhido: x = {}", first_x(c));
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    c.input(json!([{"kind": "key", "key": "Escape"}, {"kind": "wait", "frames": 2}]));
    assert!(c.state()["editor"]["selection"].is_null());

    // regressão do Mover em camada vetorial com área (as linhas "aleatórias" durante o arrasto): a camada aparece em
    // escala 0,5 em volta do centro, então o quadrado 80..240 × 40..140 do cel aparece em 120..200 × 65..115 no canvas.
    // A área feita em volta de onde o traço APARECE leva o traço, e arrastar 20 px no canvas grava 40 px no cel (o
    // que a tela mostrou). Antes, a área era testada contra os pontos crus do cel: nada a mover, e a prévia desenhava
    // esses pontos no lugar errado.
    c.ok("APPLY_COMMAND", json!({"command": {"kind": "set_props", "id": vector, "data": {"scale": 0.5}}}));
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "select", "mode": "rect"}));
    c.drag_canvas(&[(112.0, 58.0), (208.0, 122.0)], false);
    assert_eq!(c.state()["editor"]["selection"]["mode"], "Rect");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "move"}));
    c.drag_canvas(&[(160.0, 90.0), (170.0, 90.0), (180.0, 90.0)], false);
    assert!((first_x(c) - 120.0).abs() < 1.5, "20 px no canvas = 40 px no cel da camada em escala 0,5: x = {}", first_x(c));
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    assert!((first_x(c) - 80.0).abs() < 1.0);
    // a área onde o cel "estaria" (o lado direito, onde o traço não aparece) não leva nada
    c.input(json!([{"kind": "key", "key": "Escape"}, {"kind": "wait", "frames": 2}]));
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "select", "mode": "rect"}));
    c.drag_canvas(&[(215.0, 30.0), (300.0, 150.0)], false);
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "move"}));
    c.drag_canvas(&[(250.0, 90.0), (270.0, 90.0)], false);
    assert!((first_x(c) - 80.0).abs() < 1.0, "a área sem traço não moveu nada: x = {}", first_x(c));
    c.ok("APPLY_COMMAND", json!({"command": {"kind": "set_props", "id": vector, "data": {"scale": 1.0}}}));
    c.input(json!([{"kind": "key", "key": "Escape"}, {"kind": "wait", "frames": 2}, {"kind": "key", "key": "Escape"}, {"kind": "wait", "frames": 2}]));

    // varinha na camada vetorial: lê os pixels desenhados; o clique dentro do quadrado pega só o miolo (não o canvas
    // inteiro), Shift+clique fora soma o lado de fora, Alt+clique dentro tira o miolo de novo
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "select", "mode": "wand"}));
    c.click_canvas(160.0, 90.0);
    let st = c.state();
    assert_eq!(st["editor"]["selection"]["mode"], "Mask", "{}", st["editor"]["selection"]);
    let r = st["editor"]["selection"]["rect"].clone();
    assert!(r[2].as_f64().unwrap() < 165.0 && r[3].as_f64().unwrap() < 105.0, "só o miolo do quadrado: {r}");
    let inner = st["editor"]["select_tool"]["covered_px"].as_u64().unwrap();
    assert!(inner > 12000 && inner < 16000, "miolo ~156×96: {inner}");
    let (ox, oy) = c.canvas_pt(20.0, 160.0);
    c.input(json!([{"kind": "mouse_move", "x": ox, "y": oy}, {"kind": "mouse_down", "x": ox, "y": oy, "button": "primary", "modifiers": ["shift"]}, {"kind": "mouse_up", "x": ox, "y": oy, "button": "primary", "modifiers": ["shift"]}, {"kind": "wait", "frames": 2}]));
    let both = c.state()["editor"]["select_tool"]["covered_px"].as_u64().unwrap();
    assert!(both > 320 * 180 - 4500, "Shift soma o lado de fora: {both}");
    let (ix, iy) = c.canvas_pt(160.0, 90.0);
    c.input(json!([{"kind": "mouse_move", "x": ix, "y": iy}, {"kind": "mouse_down", "x": ix, "y": iy, "button": "primary", "modifiers": ["alt"]}, {"kind": "mouse_up", "x": ix, "y": iy, "button": "primary", "modifiers": ["alt"]}, {"kind": "wait", "frames": 2}]));
    let outer = c.state()["editor"]["select_tool"]["covered_px"].as_u64().unwrap();
    assert!((outer as i64 - (both - inner) as i64).abs() < 50, "Alt tira o miolo: {outer} vs {both} - {inner}");
    assert!(c.state()["editor"]["select_tool"]["outline_loops"].as_u64().unwrap() >= 2, "formigas no contorno de fora e no buraco");
    c.input(json!([{"kind": "key", "key": "D", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    assert!(c.state()["editor"]["selection"].is_null(), "Ctrl+D tira a seleção");

    // zoom pela roda ancorado no cursor e pan com o botão do meio / espaço
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "pen"}));
    let before = c.state()["editor"]["canvas"].clone();
    let (ax, ay) = c.canvas_pt(100.0, 60.0);
    c.input(json!([{"kind": "mouse_move", "x": ax, "y": ay}, {"kind": "wheel", "dx": 0.0, "dy": 50.0}, {"kind": "wait", "frames": 2}]));
    let after = c.state()["editor"]["canvas"].clone();
    assert!(after["zoom"].as_f64().unwrap() > before["zoom"].as_f64().unwrap() * 1.05, "a roda aproximou: {} -> {}", before["zoom"], after["zoom"]);
    let (ax2, ay2) = c.canvas_pt(100.0, 60.0);
    assert!((ax2 - ax).abs() < 1.5 && (ay2 - ay).abs() < 1.5, "o ponto sob o cursor ficou parado: ({ax}, {ay}) -> ({ax2}, {ay2})");
    c.input(json!([
        {"kind": "mouse_move", "x": ax, "y": ay},
        {"kind": "mouse_down", "x": ax, "y": ay, "button": "middle"},
        {"kind": "mouse_move", "x": ax + 40.0, "y": ay + 20.0},
        {"kind": "mouse_up", "x": ax + 40.0, "y": ay + 20.0, "button": "middle"},
        {"kind": "wait", "frames": 2}
    ]));
    let (bx, by) = c.canvas_pt(100.0, 60.0);
    assert!((bx - ax - 40.0).abs() < 1.5 && (by - ay - 20.0).abs() < 1.5, "pan pelo botão do meio: ({ax}, {ay}) -> ({bx}, {by})");
    c.input(json!([
        {"kind": "key", "key": "Space", "pressed": true},
        {"kind": "mouse_move", "x": bx, "y": by},
        {"kind": "mouse_down", "x": bx, "y": by, "button": "primary"},
        {"kind": "mouse_move", "x": bx - 30.0, "y": by},
        {"kind": "mouse_up", "x": bx - 30.0, "y": by, "button": "primary"},
        {"kind": "key", "key": "Space", "pressed": false},
        {"kind": "wait", "frames": 2}
    ]));
    let (cx, _) = c.canvas_pt(100.0, 60.0);
    assert!((cx - bx + 30.0).abs() < 1.5, "pan com espaço + arrastar: {bx} -> {cx}");
    assert_eq!(c.strokes(&vcel).len(), 1, "pan não desenha");
    c.click_widget("canvas:fit");
    assert!(c.state()["editor"]["canvas"]["zoom"].as_f64().unwrap() < after["zoom"].as_f64().unwrap(), "Ajustar volta ao enquadramento");
    c.click_widget("canvas:flip");
    assert_eq!(c.state()["editor"]["canvas"]["flip"], true);
    // com a visão espelhada o traço ainda cai onde o cursor passa
    let n = c.latency_count();
    c.drag_canvas(&[(100.0, 70.0), (140.0, 75.0), (180.0, 70.0)], false);
    c.wait_canvas(n);
    let node = c.node(&vcel);
    let all = node["strokes"].as_array().unwrap();
    assert_eq!(all.len(), 2);
    let first = &all[1]["points"][0];
    assert!((first["x"].as_f64().unwrap() - 100.0).abs() < 1.5 && (first["y"].as_f64().unwrap() - 70.0).abs() < 1.5, "o espelho da visão não desalinha o traço: {first}");
    c.click_widget("canvas:flip");
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.strokes(&vcel).len(), 1);

    // timeline: quadro pela régua, 2º cel na lista de cels, exposição por clique na célula, desenhar no cel certo, scrub
    c.click_widget("timeline:step_forward");
    assert_eq!(c.state()["editor"]["frame"], 1);
    let ui = c.state();
    let area = ui["widgets"]["timeline:area"]["rect_logical"].clone();
    let (cell_w, hdr) = (ui["editor"]["timeline"]["cell_w"].as_f64().unwrap(), ui["editor"]["timeline"]["header_w"].as_f64().unwrap());
    let frame_x = |f: f64| area[0].as_f64().unwrap() + hdr + (f + 0.5) * cell_w;
    let ruler_y = area[1].as_f64().unwrap() + 8.0;
    c.click(frame_x(5.0), ruler_y);
    assert_eq!(c.state()["editor"]["frame"], 5, "clicar na régua leva o cabeçote");
    c.click_widget("layers:row:0");
    c.click_widget("cels:add");
    assert_eq!(c.state()["editor"]["cel_number"], 2);
    let row = c.state()["widgets"]["timeline:row:0"]["rect_logical"].clone();
    let (rx, ry) = (frame_x(6.0), row[1].as_f64().unwrap() + row[3].as_f64().unwrap() / 2.0);
    c.click(rx, ry);
    let ex = |f: u32| c.ok("QUERY", json!({"query": {"kind": "exposure", "cut": cut, "frame": f}})).to_string();
    let cel2 = cel_of(&vector, 1);
    assert!(ex(6).contains(&cel2), "o quadro 6 expõe o cel 2: {}", ex(6));
    assert!(ex(5).contains(&vcel), "o quadro 5 continua no cel 1: {}", ex(5));
    // desenhar no quadro 7 vai para o cel 2
    c.click(frame_x(7.0), ruler_y);
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "pen"}));
    let n = c.latency_count();
    c.drag_canvas(&[(120.0, 100.0), (160.0, 110.0), (200.0, 100.0)], false);
    c.wait_canvas(n);
    assert_eq!(c.strokes(&cel2).len(), 1, "o traço do quadro 7 entrou no cel 2");
    // scrub: o render do quadro 7 tem o traço novo; o do quadro 1 não
    assert!(c.render_px(&cut, 7, 160, 110)[3] > 0 && c.render_px(&cut, 1, 160, 110)[3] == 0);
    // Ctrl+clique = Segurar: remove a mudança do quadro 6 (volta ao cel 1)
    c.input(json!([
        {"kind": "mouse_move", "x": rx, "y": ry},
        {"kind": "mouse_down", "x": rx, "y": ry, "button": "primary", "modifiers": ["ctrl"]},
        {"kind": "mouse_up", "x": rx, "y": ry, "button": "primary", "modifiers": ["ctrl"]},
        {"kind": "wait", "frames": 2}
    ]));
    assert!(ex(6).contains(&vcel), "Segurar removeu a mudança do quadro 6: {}", ex(6));

    // reprodução: toca na taxa do projeto, o quadro avança e pausa
    c.click_widget("timeline:step_back");
    c.click_widget("timeline:play");
    assert_eq!(c.state()["editor"]["playback"]["playing"], true);
    std::thread::sleep(Duration::from_millis(700));
    let f_play = c.state()["editor"]["frame"].as_u64().unwrap();
    assert!(f_play >= 3, "o cabeçote andou na reprodução: {f_play}");
    c.click_widget("timeline:play");
    assert_eq!(c.state()["editor"]["playback"]["playing"], false);

    // idioma e tema não quebram os painéis novos
    c.ok("UI_ACTION", json!({"action": "set_language", "language": "en"}));
    c.ok("UI_ACTION", json!({"action": "set_theme", "theme": "light"}));
    assert_eq!(c.state()["editor"]["layers"].as_array().unwrap().len(), 3);
    c.ok("UI_ACTION", json!({"action": "set_language", "language": "pt"}));
    c.ok("UI_ACTION", json!({"action": "set_theme", "theme": "dark"}));
}

/// Painéis de estrutura pelo mouse: criar episódio/cena/corte, trocar de corte, apagar pelo menu de contexto; mover/apagar/ajustar
/// camadas (opacidade, mistura, linhas do balde); cels; paletas e nome de cor; menu de contexto da linha do tempo; borracha raster
/// e mover a camada inteira.
fn editor_script2(c: &Client) {
    c.ok("NEW_PROJECT", json!({"name": "estrutura", "settings": {"width": 320, "height": 180}}));
    c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "animacao"}));
    let roots = |cat: &str| c.ok("QUERY", json!({"query": {"kind": "roots", "category": cat}})).as_array().unwrap().clone();
    let typed = |text: &str| c.input(json!([{"kind": "key", "key": "A", "modifiers": ["ctrl"]}, {"kind": "text", "text": text}, {"kind": "key", "key": "Enter"}, {"kind": "wait", "frames": 3}]));

    // Projeto: episódio -> cena -> corte pelos botões; o corte novo vira o ativo (o painel divide aba com Pincéis)
    c.ok("UI_ACTION", json!({"action": "open_panel", "panel": "project"}));
    c.click_widget("project:add_episode");
    assert_eq!(roots("episodes").len(), 1);
    c.click_widget("project:add_scene");
    c.click_widget("project:add_cut");
    let cut1 = c.state()["editor"]["cut"].as_str().expect("o corte criado fica ativo").to_string();
    assert_eq!(c.node(&cut1)["duration"], 48);
    assert_eq!(c.state()["editor"]["canvas"]["size"], json!([320, 180]));
    c.click_widget("project:add_cut");
    let cut2 = c.state()["editor"]["cut"].as_str().unwrap().to_string();
    assert_ne!(cut1, cut2, "outro corte na mesma cena");
    c.click_widget("project:node:Corte 1");
    assert_eq!(c.state()["editor"]["cut"], json!(cut1), "clicar no corte da árvore escolhe o corte ativo");
    // menu de contexto: apagar o corte 2 (undo devolve)
    let (x, y) = c.center("project:node:Corte 2");
    c.input(json!([{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "secondary"}, {"kind": "mouse_up", "x": x, "y": y, "button": "secondary"}, {"kind": "wait", "frames": 2}]));
    c.click_widget("project:ctx.delete");
    assert!(c.ok("QUERY", json!({"query": {"kind": "tree"}}))["episodes"][0]["children"][0]["children"].as_array().unwrap().len() == 1, "o corte 2 foi apagado pelo menu de contexto");
    c.click_widget("project:node:Corte 1");

    // Camadas: criar, subir/descer, ajustar opacidade e mistura, ligação do balde, apagar
    for kind in ["vector", "raster", "fill"] {
        c.click_widget("layers:add");
        c.click_widget(&format!("layers:add.{kind}"));
    }
    let st = c.state();
    let layers: Vec<(String, String)> = st["editor"]["layers"].as_array().unwrap().iter().map(|l| (l["id"].as_str().unwrap().to_string(), l["kind"].as_str().unwrap().to_string())).collect();
    let kinds: Vec<&str> = layers.iter().map(|l| l.1.as_str()).collect();
    assert_eq!(kinds, ["raster", "vector", "fill"], "o balde entra abaixo da linha; o raster novo acima da camada ativa");
    let (vec_id, fill_id) = (layers[1].0.clone(), layers[2].0.clone());
    // subir/descer ficam no menu de contexto da linha (S25: a barra do painel só tem ícones de criar e de estado)
    let ctx_menu = |row: &str| {
        let (x, y) = c.center(row);
        c.input(json!([{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "secondary"}, {"kind": "mouse_up", "x": x, "y": y, "button": "secondary"}, {"kind": "wait", "frames": 2}]));
    };
    c.click_widget("layers:row:1");
    ctx_menu("layers:row:1");
    c.click_widget("layers:ctx.up");
    assert_eq!(c.state()["editor"]["layers"][0]["id"], json!(vec_id), "subir move a linha para o topo");
    ctx_menu("layers:row:0");
    c.click_widget("layers:ctx.down");
    assert_eq!(c.state()["editor"]["layers"][1]["id"], json!(vec_id), "descer devolve");
    // mistura: abre o combo e escolhe Multiplicar
    c.click_widget("layers:blend");
    c.click_widget("layers:blend.multiply");
    assert_eq!(c.node(&vec_id)["blend"], "multiply");
    // opacidade: arrasta o controle até perto da esquerda (confirma ao soltar, um passo)
    let before_steps = c.ok("GET_STATUS", json!({}))["undo_steps"].as_u64().unwrap();
    let r = c.state()["widgets"]["layers:opacity"]["rect_logical"].clone();
    let (ox, oy, ow, oh) = (r[0].as_f64().unwrap(), r[1].as_f64().unwrap(), r[2].as_f64().unwrap(), r[3].as_f64().unwrap());
    let (hx0, hy) = (ox + ow * 0.62, oy + oh / 2.0);
    c.input(json!([
        {"kind": "mouse_move", "x": hx0, "y": hy},
        {"kind": "mouse_down", "x": hx0, "y": hy, "button": "primary"},
        {"kind": "mouse_move", "x": hx0 - 30.0, "y": hy},
        {"kind": "mouse_move", "x": hx0 - 60.0, "y": hy},
        {"kind": "mouse_up", "x": hx0 - 60.0, "y": hy, "button": "primary"},
        {"kind": "wait", "frames": 3}
    ]));
    let op = c.node(&vec_id)["opacity"].as_f64().unwrap();
    assert!(op < 0.99 && op > 0.02, "o slider de opacidade mudou o valor: {op}");
    assert_eq!(c.ok("GET_STATUS", json!({}))["undo_steps"].as_u64().unwrap(), before_steps + 1, "arrastar o slider é UM passo de desfazer");
    // balde: desligar a linha pelo menu de linhas
    c.click_widget("layers:row:2");
    assert_eq!(c.node(&fill_id)["line_refs"].as_array().map(|a| a.len()), Some(1));
    c.click_widget("layers:line_refs");
    c.click_widget("layers:line_ref.Vetor 1");
    assert!(c.node(&fill_id).get("line_refs").is_none_or(|v| v.as_array().is_some_and(|a| a.is_empty())), "desmarcar tira a linha do balde: {}", c.node(&fill_id));
    c.input(json!([{"kind": "key", "key": "Escape"}, {"kind": "wait", "frames": 2}]));
    // apagar a camada de preenchimento
    c.click_widget("layers:delete");
    assert_eq!(c.state()["editor"]["layers"].as_array().unwrap().len(), 2);
    c.input(json!([{"kind": "key", "key": "Z", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["editor"]["layers"].as_array().unwrap().len(), 3, "Ctrl+Z desfaz apagar camada");

    // Cels: novo e duplicar (o duplicado carrega o desenho)
    c.click_widget("layers:row:1");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "pen"}));
    let n = c.latency_count();
    c.drag_canvas(&[(60.0, 60.0), (140.0, 70.0), (220.0, 60.0)], false);
    c.wait_canvas(n);
    c.click_widget("cels:add");
    assert_eq!(c.state()["editor"]["cel_number"], 2);
    c.click_widget("layers:row:1");
    c.click_widget("cels:row:1");
    c.click_widget("cels:duplicate");
    assert_eq!(c.state()["editor"]["cel_number"], 3);
    let cel3 = c.ok("QUERY", json!({"query": {"kind": "tree", "id": vec_id}}))["children"][2]["id"].as_str().unwrap().to_string();
    assert_eq!(c.strokes(&cel3).len(), 1, "o cel duplicado tem o traço do cel 1");

    // Cores: paleta nova, cor na paleta, renomear pela interface, trocar a cor na paleta recolore o canvas. Na Animação
    // as Cores ficam numa aba pequena ao lado da mesa de luz; a edição da paleta é feita na Pintura, onde o painel é grande
    c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "pintura"}));
    // S25: as paletas ficam na aba "Conjunto de cores" do painel Cores
    c.click_widget("colors:tab:set");
    c.click_widget("colors:new_palette");
    assert_eq!(roots("palettes").len(), 1);
    c.ok("UI_ACTION", json!({"action": "set_color", "color": "#336699"}));
    c.click_widget("colors:add_to_palette");
    let pal = roots("palettes")[0].as_str().unwrap().to_string();
    assert_eq!(c.node(&pal)["colors"].as_array().unwrap().len(), 1);
    c.click_widget("colors:swatch:0");
    c.click_widget("colors:name");
    typed("Pele");
    assert_eq!(c.node(&pal)["colors"][0]["name"], "Pele", "o nome da cor foi editado pela interface (ReplaceData)");
    c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "animacao"}));

    // linha do tempo: menu de contexto > quadro vazio
    c.click_widget("layers:row:1");
    let ui = c.state();
    let area = ui["widgets"]["timeline:area"]["rect_logical"].clone();
    let (cell_w, hdr) = (ui["editor"]["timeline"]["cell_w"].as_f64().unwrap(), ui["editor"]["timeline"]["header_w"].as_f64().unwrap());
    let row = ui["widgets"]["timeline:row:1"]["rect_logical"].clone();
    let (cx, cy) = (area[0].as_f64().unwrap() + hdr + 9.5 * cell_w, row[1].as_f64().unwrap() + row[3].as_f64().unwrap() / 2.0);
    c.input(json!([{"kind": "mouse_move", "x": cx, "y": cy}, {"kind": "mouse_down", "x": cx, "y": cy, "button": "secondary"}, {"kind": "mouse_up", "x": cx, "y": cy, "button": "secondary"}, {"kind": "wait", "frames": 2}]));
    c.click_widget("timeline:ctx.clear");
    let ex = c.ok("QUERY", json!({"query": {"kind": "exposure", "cut": cut1, "frame": 9}})).to_string();
    assert!(ex.contains("empty"), "o menu de contexto limpou o quadro 10: {ex}");

    // borracha raster e mover a camada inteira
    c.click_widget("layers:row:0");
    c.ok("UI_ACTION", json!({"action": "set_color", "color": "#ff0000"}));
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "brush"}));
    let n = c.latency_count();
    c.drag_canvas(&[(30.0, 150.0), (60.0, 150.0), (90.0, 150.0)], false);
    c.wait_canvas(n);
    assert!(c.render_px(&cut1, 0, 60, 150)[3] > 0, "pintou no raster");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "eraser"}));
    let n = c.latency_count();
    c.drag_canvas(&[(60.0, 140.0), (60.0, 160.0)], false);
    c.wait_canvas(n);
    assert_eq!(c.render_px(&cut1, 0, 60, 150)[3], 0, "a borracha raster apagou o pixel");
    assert!(c.render_px(&cut1, 0, 33, 150)[3] > 0, "e só o trecho tocado");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "move"}));
    c.drag_canvas(&[(100.0, 100.0), (110.0, 100.0), (120.0, 100.0)], false);
    c.input(json!([{"kind": "key", "key": "Enter"}, {"kind": "wait", "frames": 3}]));
    let t0 = Instant::now();
    while c.render_px(&cut1, 0, 33, 150)[3] != 0 {
        assert!(t0.elapsed() < Duration::from_secs(10), "mover a camada raster inteira não andou");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(c.render_px(&cut1, 0, 53, 150)[3] > 0, "o traço vermelho andou 20 px para a direita");
    // zoom e mão
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "zoom"}));
    let z0 = c.state()["editor"]["canvas"]["zoom"].as_f64().unwrap();
    c.click_canvas(160.0, 90.0);
    assert!(c.state()["editor"]["canvas"]["zoom"].as_f64().unwrap() > z0 * 1.9, "a ferramenta zoom aproxima no clique");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "hand"}));
    let (px0, py0) = c.canvas_pt(100.0, 60.0);
    // o Pan mostra a mão (aberta; fechada enquanto arrasta), não a seta de quatro pontas do mover
    c.input(json!([{"kind": "mouse_move", "x": px0, "y": py0}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["editor"]["cursor_kind"], "hand", "Pan: mão aberta sobre o canvas");
    c.input(json!([{"kind": "mouse_down", "x": px0, "y": py0, "button": "primary"}, {"kind": "mouse_move", "x": px0 + 6.0, "y": py0 + 3.0}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["editor"]["cursor_kind"], "grabbing", "Pan: mão fechada ao arrastar");
    c.input(json!([{"kind": "mouse_up", "x": px0 + 6.0, "y": py0 + 3.0, "button": "primary"}, {"kind": "mouse_move", "x": px0, "y": py0}, {"kind": "wait", "frames": 2}]));
    // Espaço apertado: a mão com qualquer ferramenta; ao soltar, o cursor da ferramenta volta
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "move"}));
    c.input(json!([{"kind": "mouse_move", "x": px0 + 2.0, "y": py0}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["editor"]["cursor_kind"], "move", "o Mover mostra o cursor de mover");
    c.input(json!([{"kind": "key", "key": "Space", "pressed": true}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["editor"]["cursor_kind"], "hand", "Espaço: mão sobre o canvas");
    c.input(json!([{"kind": "key", "key": "Space", "pressed": false}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["editor"]["cursor_kind"], "move", "solto o Espaço, volta o cursor da ferramenta");
    assert_eq!(c.state()["active_tool"], "move", "a ferramenta anterior segue ativa");
    c.ok("UI_ACTION", json!({"action": "select_tool", "tool": "hand"}));
    let (px0, py0) = c.canvas_pt(100.0, 60.0); // o arrasto de 6 px acima já moveu a visão
    c.input(json!([{"kind": "mouse_move", "x": px0, "y": py0}, {"kind": "mouse_down", "x": px0, "y": py0, "button": "primary"}, {"kind": "mouse_move", "x": px0 + 25.0, "y": py0 + 10.0}, {"kind": "mouse_up", "x": px0 + 25.0, "y": py0 + 10.0, "button": "primary"}, {"kind": "wait", "frames": 2}]));
    let (px1, py1) = c.canvas_pt(100.0, 60.0);
    assert!((px1 - px0 - 25.0).abs() < 1.5 && (py1 - py0 - 10.0).abs() < 1.5, "a mão move a visão");
    // atalhos de ferramenta e de tamanho
    c.input(json!([{"kind": "key", "key": "P"}, {"kind": "wait", "frames": 2}]));
    assert_eq!(c.state()["active_tool"], "pen");
    c.input(json!([{"kind": "key", "key": "CloseBracket"}, {"kind": "wait", "frames": 2}]));
    assert!(c.state()["editor"]["tool_options"]["pen_width"].as_f64().unwrap() > 3.2, "] aumenta a largura da caneta");
}

/// Mesclar camadas pelo app: Ctrl+E, o botão do painel, o menu Editar e Ctrl+Shift+E, cada um num passo de undo.
fn merge_script(c: &Client) {
    c.ok("NEW_PROJECT", json!({"name": "mesclar", "settings": {"width": 320, "height": 180}}));
    let made = |name: &str, parent: Option<&str>, data: Value| -> String { c.ok("APPLY_COMMAND", create_node(name, parent, data))["created"][0].as_str().unwrap().to_string() };
    let ep = made("EP", None, json!({"type": "episode", "number": 1}));
    let sc = made("S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made("C1", Some(&sc), json!({"type": "cut", "number": 1, "duration": 12}));
    let mut ids = Vec::new();
    for (name, kind) in [("Fundo", "raster"), ("Tinta", "raster"), ("Linha", "vector")] {
        let l = made(name, Some(&cut), json!({"type": "layer", "kind": kind}));
        made("1", Some(&l), json!({"type": "cel", "number": 1}));
        ids.push(l);
    }
    let direct = |r: f32, g: f32, b: f32| json!({"type": "direct", "rgba": [r, g, b, 1.0]});
    c.ok("PAINT", json!({"cut": cut, "frame": 0, "layer": ids[0], "preset": "redondo duro", "preset_overrides": {"size": 30}, "color": direct(1.0, 0.0, 0.0), "strokes": [{"points": [[40, 60], [120, 60]]}]}));
    c.ok("PAINT", json!({"cut": cut, "frame": 0, "layer": ids[1], "preset": "redondo duro", "preset_overrides": {"size": 30}, "color": direct(0.0, 0.0, 1.0), "strokes": [{"points": [[80, 100], [200, 100]]}]}));
    c.ok("DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": ids[2], "curves": [{"points": [[40, 140], [280, 140]], "width": 6, "color": direct(0.0, 0.0, 0.0)}]}));
    c.ok("UI_ACTION", json!({"action": "set_workspace", "workspace": "pintura"}));
    c.ok("UI_ACTION", json!({"action": "open_panel", "panel": "layers"}));
    assert_eq!(c.state()["editor"]["cut"], json!(cut));
    let layers = || c.state()["editor"]["layers"].as_array().unwrap().iter().map(|l| l["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(layers(), vec!["Linha", "Tinta", "Fundo"], "de cima para baixo");
    let key = |k: &str, mods: Value| c.input(json!([{"kind": "key", "key": k, "modifiers": mods}, {"kind": "wait", "frames": 3}]));
    let undo = || key("Z", json!(["ctrl"]));
    let px = |x, y| c.render_px(&cut, 0, x, y);
    let before = (px(100, 60), px(150, 100), px(150, 140));
    assert!(before.0[0] > 200 && before.1[2] > 200 && before.2[3] > 200, "o desenho de partida: {before:?}");

    // Ctrl+E com a camada do meio ativa: vai para a de baixo (Fundo), a de cima fica
    c.click_widget("layers:row:1");
    key("E", json!(["ctrl"]));
    assert_eq!(layers(), vec!["Linha", "Fundo"], "Tinta foi para Fundo");
    assert_eq!((px(100, 60), px(150, 100), px(150, 140)), before, "o quadro é o mesmo");
    assert_eq!(c.state()["editor"]["layer"].as_str().map(|l| c.node(l)["kind"].clone()), Some(json!("raster")));
    undo();
    assert_eq!(layers(), vec!["Linha", "Tinta", "Fundo"], "um passo de undo");

    // o botão do painel, com o vetor ativo: rasteriza para a Tinta
    c.click_widget("layers:row:0");
    c.click_widget("layers:merge");
    assert_eq!(layers(), vec!["Tinta", "Fundo"], "Linha foi para Tinta");
    assert_eq!((px(100, 60), px(150, 100), px(150, 140)), before);
    undo();

    // o menu Editar e o de contexto da camada
    c.click_widget("layers:row:1");
    c.click_widget("menu:edit");
    c.click_widget("menu_item:edit.merge_layers");
    assert_eq!(layers(), vec!["Linha", "Fundo"]);
    undo();
    let (x, y) = c.center("layers:row:1");
    c.input(json!([{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "secondary"}, {"kind": "mouse_up", "x": x, "y": y, "button": "secondary"}, {"kind": "wait", "frames": 2}]));
    c.click_widget("layers:ctx.merge");
    assert_eq!(layers(), vec!["Linha", "Fundo"]);
    undo();

    // duas escolhidas (Ctrl+clique) viram uma só na mais baixa; Ctrl+Shift+E junta todas as visíveis
    c.click_widget("layers:row:0");
    let (x, y) = c.center("layers:row:1");
    c.input(json!([{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary", "modifiers": ["ctrl"]}, {"kind": "mouse_up", "x": x, "y": y, "button": "primary", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 2}]));
    key("E", json!(["ctrl"]));
    assert_eq!(layers(), vec!["Tinta", "Fundo"], "a Linha e a Tinta viraram uma só, na mais baixa das duas (Tinta)");
    assert_eq!((px(100, 60), px(150, 100), px(150, 140)), before);
    undo();
    key("E", json!(["ctrl", "shift"]));
    assert_eq!(layers(), vec!["Fundo"], "visíveis");
    assert_eq!((px(100, 60), px(150, 100), px(150, 140)), before);
    undo();
    assert_eq!(layers().len(), 3);

    // recusa com aviso e sem mudar nada: camada oculta
    c.ok("APPLY_COMMAND", json!({"command": {"kind": "set_props", "id": ids[1], "data": {"visible": false}}}));
    c.click_widget("layers:row:1");
    key("E", json!(["ctrl"]));
    assert_eq!(layers().len(), 3, "recusou");
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
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            script(&client);
            editor_script(&client);
            editor_script2(&client);
            merge_script(&client);
        }));
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
