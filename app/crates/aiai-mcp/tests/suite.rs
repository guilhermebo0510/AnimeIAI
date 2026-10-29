//! Suíte única do `aiai-mcp` (um binário só): protocolo stdio contra o binário real em modo headless, CLI headless,
//! roteamento para o app (bridge de verdade, com thread "app"), sandbox e validação dos argumentos das ferramentas.

use aiai_bridge::core_ops::handle_core_action_with_path;
use aiai_bridge::{BridgeConfig, BridgeError, BridgeReceiver, BridgeServer};
use aiai_core::CoreSession;
use aiai_mcp::cli::{self, Cmd};
use aiai_mcp::sandbox::Sandbox;
use aiai_mcp::tools;
use base64::Engine as _;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_aiai-mcp");

// ------------------------------------------------------------------------------------------------ infraestrutura

/// Ambiente isolado: sandbox só na pasta temporária do teste e descoberta da bridge em arquivo próprio.
struct Env {
    dir: tempfile::TempDir,
    discovery: PathBuf,
}

impl Env {
    fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let discovery = dir.path().join("bridge").join("bridge.json");
        Env { dir, discovery }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn apply(&self, cmd: &mut Command) {
        cmd.env("AIAI_MCP_ALLOWED_DIRS", self.dir.path())
            .env("AIAI_BRIDGE_DISCOVERY", &self.discovery)
            .env_remove("AIAI_BRIDGE_ADDR")
            .env_remove("AIAI_BRIDGE_TOKEN")
            .env_remove("AIAI_HEADLESS_PROJECT")
            .env_remove("AIAI_MCP_ALLOW_FS_WRITE");
    }

    fn cli(&self, args: &[&str]) -> Output {
        let mut c = Command::new(BIN);
        self.apply(&mut c);
        c.args(args).output().unwrap()
    }

    fn cli_ok(&self, args: &[&str]) -> Value {
        let o = self.cli(args);
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{args:?}: stdout não é JSON ({e}): {}", String::from_utf8_lossy(&o.stdout)))
    }

    fn mcp(&self) -> McpSession {
        let mut c = Command::new(BIN);
        self.apply(&mut c);
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        McpSession { child, stdin, stdout, next: 1 }
    }
}

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl McpSession {
    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("linha do servidor não é JSON ({e}): {line:?}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let r = self.read();
        assert_eq!(r["id"], id, "{r}");
        r
    }

    /// tools/call; devolve (isError, blocos de conteúdo).
    fn tool(&mut self, name: &str, args: Value) -> (bool, Vec<Value>) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert!(r.get("error").is_none(), "{name}: erro de protocolo {r}");
        let is_err = r["result"]["isError"].as_bool().unwrap_or(false);
        (is_err, r["result"]["content"].as_array().unwrap().clone())
    }

    /// Chamada que deve dar certo; devolve o JSON do primeiro bloco de texto.
    fn tool_ok(&mut self, name: &str, args: Value) -> Value {
        let (err, content) = self.tool(name, args);
        let text = content[0]["text"].as_str().unwrap();
        assert!(!err, "{name} falhou: {text}");
        serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: texto não é JSON ({e}): {text}"))
    }

    fn tool_err(&mut self, name: &str, args: Value) -> String {
        let (err, content) = self.tool(name, args);
        let text = content[0]["text"].as_str().unwrap().to_string();
        assert!(err, "{name} deveria falhar, mas deu: {text}");
        text
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn handshake(s: &mut McpSession) {
    let r = s.request("initialize", json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "teste", "version": "0"}}));
    assert_eq!(r["result"]["serverInfo"]["name"], "aiai-mcp");
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    // notificação: sem resposta (a próxima linha lida é a do ping)
    s.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
}

fn create_node(name: &str, parent: Option<&str>, data: Value) -> Value {
    let mut c = json!({"kind": "create_node", "name": name, "data": data});
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    json!({"command": c})
}

fn stderr_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

fn tiny_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

// ------------------------------------------------------------------------------------------------ stdio + headless

#[test]
fn stdio_protocol_initialize_list_and_headless_apply_query_undo() {
    let env = Env::new();
    let mut s = env.mcp();
    handshake(&mut s);

    let r = s.request("tools/list", json!({}));
    let tool_list = r["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tool_list.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "aiai_status", "aiai_apply", "aiai_query", "aiai_undo", "aiai_redo", "aiai_log", "aiai_project", "aiai_render", "aiai_compare", "aiai_screenshot", "aiai_input", "aiai_ui", "aiai_schema",
        "aiai_draw_curves", "aiai_fill", "aiai_lint", "aiai_paint", "aiai_gradient", "aiai_figure", "aiai_transform", "aiai_repeat", "aiai_brush_presets", "aiai_vectorize",
        "aiai_sky", "aiai_pattern_fill", "aiai_text", "aiai_lines", "aiai_gag_bg", "aiai_particles", "aiai_guide3d", "aiai_perspective", "aiai_model_sheet", "aiai_character_material", "aiai_consistency", "aiai_inbetween",
        "aiai_propagate_fills", "aiai_tts_line", "aiai_tts_kokoro", "aiai_voice_profile", "aiai_lipsync", "aiai_sfx", "aiai_bgm", "aiai_mix", "aiai_loudness", "aiai_probe_color",
        "aiai_measure_line", "aiai_measure_edge", "aiai_fill_shape", "aiai_edit", "aiai_export_video",
    ] {
        assert!(names.contains(&want), "falta a ferramenta {want}: {names:?}");
    }
    for t in tool_list {
        assert!(t["description"].as_str().unwrap().len() > 40, "descrição curta demais em {}", t["name"]);
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
    }
    let apply = tool_list.iter().find(|t| t["name"] == "aiai_apply").unwrap();
    let desc = apply["description"].as_str().unwrap();
    for kind in ["create_node", "set_props", "delete_node", "move_node", "insert_strokes", "set_tiles", "set_cells", "add_track", "batch", "aiai_schema"] {
        assert!(desc.contains(kind), "a cola de aiai_apply não cita {kind}");
    }
    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
    assert_eq!(s.request("nao/existe", json!({}))["error"]["code"], -32601);

    // sem app e sem projeto: status honesto e documento sem projeto é erro claro
    let st = s.tool_ok("aiai_status", json!({}));
    assert_eq!(st["mode"], "headless");
    assert_eq!(st["loaded"], false);
    assert!(s.tool_err("aiai_query", json!({"query": {"kind": "stats"}})).contains("sem projeto headless"));

    // projeto novo em pasta permitida; depois apply -> query vê o nó
    let proj = env.path("exemplo.aiai");
    let p = proj.to_str().unwrap();
    let n = s.tool_ok("aiai_project", json!({"op": "new", "name": "Exemplo", "path": p, "settings": {"width": 1280, "height": 720}}));
    assert_eq!(n["settings"]["width"], 1280);
    assert_eq!(n["_via"], "headless");
    // repetir new sobre projeto existente é recusado
    assert!(s.tool_err("aiai_project", json!({"op": "new", "name": "Outro", "path": p})).contains("já existe"));

    let ep = s.tool_ok("aiai_apply", create_node("EP01", None, json!({"type": "episode", "number": 1})));
    assert_eq!(ep["revision"], 1);
    let ep_id = ep["created"][0].as_str().unwrap().to_string();
    let sc = s.tool_ok("aiai_apply", create_node("Cena 1", Some(&ep_id), json!({"type": "scene"})));
    assert_eq!(sc["can_undo"], true);

    let tree = s.tool_ok("aiai_query", json!({"query": {"kind": "tree"}}));
    assert_eq!(tree["episodes"][0]["name"], "EP01");
    assert_eq!(tree["episodes"][0]["children"][0]["name"], "Cena 1");

    // salvo em disco a cada mudança
    let disk = std::fs::read_to_string(proj.join("project.json")).unwrap();
    assert!(disk.contains("EP01") && disk.contains("Cena 1"));

    // dentro do servidor o histórico vive: undo tira a cena (e salva), redo traz de volta
    s.tool_ok("aiai_undo", json!({}));
    let disk = std::fs::read_to_string(proj.join("project.json")).unwrap();
    assert!(!disk.contains("Cena 1"));
    let tree = s.tool_ok("aiai_query", json!({"query": {"kind": "tree"}}));
    assert!(tree["episodes"][0].get("children").is_none());
    s.tool_ok("aiai_redo", json!({}));
    let log = s.tool_ok("aiai_log", json!({"include_commands": true}));
    assert_eq!(log["entries"][0]["author"], "agent:claude");
    assert_eq!(log["total"], 4);

    // schema de comandos
    let sch = s.tool_ok("aiai_schema", json!({"which": "commands"}));
    assert!(sch.to_string().contains("insert_strokes"));

    // status do headless com projeto
    let st = s.tool_ok("aiai_status", json!({}));
    assert_eq!((st["name"].as_str(), st["revision"].as_u64()), (Some("Exemplo"), Some(4)));

    // ferramentas que precisam da UI dizem o porquê
    for (tool, args) in [
        ("aiai_screenshot", json!({})),
        ("aiai_input", json!({"events": [{"kind": "mouse_move", "x": 1, "y": 2}]})),
        ("aiai_ui", json!({"action": "get_state"})),
    ] {
        let e = s.tool_err(tool, args);
        assert!(e.contains("AnimeIAI Studio não está aberto com --bridge"), "{tool}: {e}");
    }

    // argumentos inválidos e ferramenta desconhecida
    assert!(s.tool_err("aiai_apply", json!({})).contains("Missing 'command'"));
    assert!(s.tool_err("aiai_apply", json!({"command": {"kind": "nao_existe"}})).contains("command inválido"));
    assert!(s.tool_err("aiai_project", json!({"op": "apagar"})).contains("new|open|save"));
    assert!(s.tool_err("aiai_ui", json!({})).contains("Missing 'action'"));
    assert!(s.tool_err("aiai_nao_existe", json!({})).contains("Unknown tool"));

    // sandbox: projeto fora das pastas permitidas é recusado
    let outside = std::env::current_dir().unwrap().join("fora-do-sandbox.aiai");
    let e = s.tool_err("aiai_project", json!({"op": "new", "name": "X", "path": outside.to_str().unwrap()}));
    assert!(e.contains("fora das pastas permitidas"), "{e}");
    assert!(!outside.exists());

    // JSON malformado: erro -32700, e o servidor continua vivo
    writeln!(s.stdin, "{{ nao e json").unwrap();
    assert_eq!(s.read()["error"]["code"], -32700);
    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
}

#[test]
fn headless_project_via_env_and_reopen_in_a_second_session() {
    let env = Env::new();
    let proj = env.path("env.aiai");
    {
        let mut s = env.mcp();
        handshake(&mut s);
        s.tool_ok("aiai_project", json!({"op": "new", "name": "Persiste", "path": proj.to_str().unwrap()}));
        s.tool_ok("aiai_apply", create_node("EP", None, json!({"type": "episode"})));
    }
    // outra sessão MCP (outro processo): abre do disco e vê o nó
    let mut s = env.mcp();
    handshake(&mut s);
    let o = s.tool_ok("aiai_project", json!({"op": "open", "path": proj.to_str().unwrap()}));
    assert_eq!(o["name"], "Persiste");
    assert_eq!(s.tool_ok("aiai_query", json!({"query": {"kind": "stats"}}))["nodes"], 1);
    // abrir uma pasta que não é projeto: erro claro
    let e = s.tool_err("aiai_project", json!({"op": "open", "path": env.path("vazio.aiai").to_str().unwrap()}));
    assert!(!e.is_empty());
}

// ------------------------------------------------------------------------------------------------ CLI headless

#[test]
fn cli_headless_call_creates_applies_queries_and_persists_across_processes() {
    let env = Env::new();
    let proj = env.path("cli.aiai");
    let p = proj.to_str().unwrap();

    let n = env.cli_ok(&["call", "NEW_PROJECT", r#"{"name":"Via CLI"}"#, "--project", p]);
    assert_eq!(n["name"], "Via CLI");
    assert_eq!(n["_via"], "headless");
    assert!(proj.join("project.json").is_file());

    let ep = env.cli_ok(&["call", "APPLY_COMMAND", &create_node("EP01", None, json!({"type": "episode"})).to_string(), "--project", p]);
    let ep_id = ep["created"][0].as_str().unwrap().to_string();
    let sc = env.cli_ok(&["call", "APPLY_COMMAND", &create_node("Cena", Some(&ep_id), json!({"type": "scene"})).to_string(), "--project", p]);
    assert_eq!(sc["revision"], 2);

    // outro processo: a árvore está lá
    let tree = env.cli_ok(&["call", "QUERY", r#"{"query":{"kind":"tree"}}"#, "--project", p]);
    assert_eq!(tree["episodes"][0]["children"][0]["name"], "Cena");

    // mudar o nome de um nó e ler o JSON canônico do disco (diff legível)
    let before = std::fs::read_to_string(proj.join("project.json")).unwrap();
    env.cli_ok(&["call", "APPLY_COMMAND", &json!({"command": {"kind": "set_props", "id": ep_id, "name": "EP01 final"}}).to_string(), "--project", p]);
    let after = std::fs::read_to_string(proj.join("project.json")).unwrap();
    assert!(before.contains("\"name\": \"EP01\"") && after.contains("\"name\": \"EP01 final\""));
    assert_eq!(before.lines().count(), after.lines().count(), "só o nome e a revisão mudam: diff de linhas, não reescrita");

    // sem histórico entre processos: undo diz a verdade
    let o = env.cli(&["call", "UNDO", "--project", p]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr_of(&o).contains("nada para desfazer"), "{}", stderr_of(&o));

    // o log também só vale por processo (vem do WAL? não): a sessão reaberta começa com log vazio
    let st = env.cli_ok(&["call", "GET_STATUS", "--project", p]);
    assert_eq!((st["mode"].as_str(), st["revision"].as_u64()), (Some("headless"), Some(3)));

    // via AIAI_HEADLESS_PROJECT (sem --project)
    let mut c = Command::new(BIN);
    env.apply(&mut c);
    let out = c.env("AIAI_HEADLESS_PROJECT", &proj).args(["call", "QUERY", r#"{"query":{"kind":"stats"}}"#]).output().unwrap();
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["nodes"], 2);

    // params por arquivo (@) com BOM, como o PowerShell 5.1 grava
    let f = env.path("p.json");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(br#"{"query":{"kind":"stats"}}"#);
    std::fs::write(&f, bytes).unwrap();
    let st = env.cli_ok(&["call", "QUERY", &format!("@{}", f.display()), "--project", p]);
    assert_eq!(st["nodes"], 2);
}

#[test]
fn cli_errors_usage_sandbox_and_ui_actions_without_the_app() {
    let env = Env::new();
    // uso incorreto = 2
    for bad in [vec!["call"], vec!["call", "ping"], vec!["call", "PING", "nao-json"], vec!["call", "PING", "--wat"], vec!["call", "PING", "--timeout", "0"], vec!["xyz"]] {
        assert_eq!(env.cli(&bad).status.code(), Some(2), "{bad:?}");
    }
    // ação de UI sem app: 1 e a mensagem pedida
    let o = env.cli(&["call", "SCREENSHOT"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr_of(&o).contains("AnimeIAI Studio não está aberto com --bridge"), "{}", stderr_of(&o));
    // PNG fora das pastas permitidas: recusado antes de qualquer chamada
    let outside = std::env::current_dir().unwrap().join("x.png");
    let o = env.cli(&["call", "SCREENSHOT", "--png", outside.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr_of(&o).contains("fora das pastas permitidas"), "{}", stderr_of(&o));
    // `..` recusado
    let sneaky = format!("{}\\..\\x.png", env.dir.path().display());
    assert_eq!(env.cli(&["call", "SCREENSHOT", "--png", &sneaky]).status.code(), Some(1));
    // --project fora do sandbox
    let o = env.cli(&["call", "NEW_PROJECT", r#"{"name":"x"}"#, "--project", outside.with_extension("aiai").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    // info funciona sem app
    let info = env.cli_ok(&["info"]);
    assert_eq!(info["live"], false);
}

// ------------------------------------------------------------------------------------------------ app aberto (bridge real)

struct App {
    server: BridgeServer,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<CoreSession>>,
}

fn start_app(discovery: &Path, png: Vec<u8>) -> App {
    let mut cfg = BridgeConfig::default();
    cfg.server.discovery = Some(discovery.to_path_buf());
    cfg.server.auth_failure_delay = Duration::from_millis(5);
    let (server, rx) = BridgeServer::start(cfg).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = Arc::clone(&stop);
    let thread = std::thread::spawn(move || app_loop(rx, stop2, png));
    App { server, stop, thread: Some(thread) }
}

impl App {
    fn finish(mut self) -> CoreSession {
        self.stop.store(true, Ordering::Relaxed);
        let s = self.thread.take().unwrap().join().unwrap();
        drop(self.server);
        s
    }
}

fn app_loop(rx: BridgeReceiver, stop: Arc<AtomicBool>, png: Vec<u8>) -> CoreSession {
    let mut session = CoreSession::for_tests("app");
    let mut path = None;
    while !stop.load(Ordering::Relaxed) {
        for req in rx.try_recv_all() {
            let result = match req.action.as_str() {
                "GET_STATUS" => Ok(json!({"mode": "app", "revision": session.project.revision})),
                "SCREENSHOT" => {
                    if req.params["inline"] == json!(true) {
                        Ok(json!({"width": 640, "height": 480, "png_base64": base64::engine::general_purpose::STANDARD.encode(&png)}))
                    } else {
                        Err(BridgeError::invalid_params("o app deveria receber inline:true"))
                    }
                }
                "GET_UI_STATE" => Ok(json!({"workspace": "layout", "scale": 1.25})),
                "UI_ACTION" => Ok(json!({"echo": req.params})),
                "RENDER" => Err(BridgeError::failed("RENDER ainda não implementado (S03)")),
                other => match handle_core_action_with_path(&mut session, &mut path, other, &req.params) {
                    Some(r) => r,
                    None => Err(BridgeError::unknown_action(other)),
                },
            };
            req.respond(result);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    session
}

#[test]
fn with_the_app_open_actions_go_to_the_bridge_and_images_come_back() {
    let env = Env::new();
    let app = start_app(&env.discovery, tiny_png(2000, 1000));
    let mut s = env.mcp();
    handshake(&mut s);

    // status vem do app, não do headless
    assert_eq!(s.tool_ok("aiai_status", json!({}))["mode"], "app");

    // documento: aplica NO APP (a sessão do app muda), sem marca de headless
    let ep = s.tool_ok("aiai_apply", create_node("EP-app", None, json!({"type": "episode"})));
    assert!(ep.get("_via").is_none());
    assert_eq!(ep["revision"], 1);
    assert_eq!(s.tool_ok("aiai_query", json!({"query": {"kind": "tree"}}))["episodes"][0]["name"], "EP-app");

    // UI: get_state e uma ação genérica
    assert_eq!(s.tool_ok("aiai_ui", json!({"action": "get_state"}))["workspace"], "layout");
    let a = s.tool_ok("aiai_ui", json!({"action": "set_workspace", "workspace": "xsheet"}));
    assert_eq!(a["echo"]["workspace"], "xsheet");

    // erro do app chega com o código e o texto
    let e = s.tool_err("aiai_render", json!({"cut": "c1", "frame": 0}));
    assert!(e.contains("não implementado"), "{e}");

    // screenshot: imagem MCP reduzida (<= 1280) + arquivo em tamanho real no sandbox
    let out = env.path("shots/tela.png");
    let (err, content) = s.tool("aiai_screenshot", json!({"out": out.to_str().unwrap()}));
    assert!(!err, "{content:?}");
    let meta: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert!(meta["png_base64"].as_str().unwrap().contains("bytes salvos"), "{meta}");
    let img = content.iter().find(|c| c["type"] == "image").expect("conteúdo de imagem MCP");
    assert_eq!(img["mimeType"], "image/png");
    let shown = image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(img["data"].as_str().unwrap()).unwrap()).unwrap();
    assert!(shown.width().max(shown.height()) <= 1280, "{}x{}", shown.width(), shown.height());
    assert_eq!((shown.width(), shown.height()), (1280, 640));
    let saved = image::open(&out).unwrap();
    assert_eq!((saved.width(), saved.height()), (2000, 1000), "o arquivo é em tamanho real");
    // destino fora do sandbox: recusado e nada gravado
    let outside = std::env::current_dir().unwrap().join("fora.png");
    assert!(s.tool_err("aiai_screenshot", json!({"out": outside.to_str().unwrap()})).contains("fora das pastas permitidas"));
    assert!(!outside.exists());

    // CLI com o app aberto: --png grava o arquivo
    let cli_png = env.path("cli-tela.png");
    let r = env.cli_ok(&["call", "SCREENSHOT", "--png", cli_png.to_str().unwrap()]);
    assert!(r["png_base64"].as_str().unwrap().contains("bytes salvos"));
    assert_eq!(image::open(&cli_png).unwrap().width(), 2000);

    // o app reiniciou (porta e token novos, mesmo arquivo de descoberta): o MCP vivo continua funcionando
    let session = app.finish();
    assert_eq!(session.project.nodes.len(), 1);
    assert!(!env.discovery.exists(), "descoberta some quando o app cai");
    let down = s.tool_ok("aiai_status", json!({}));
    assert_eq!(down["mode"], "headless", "app fechado: o status cai para o headless sem travar");
    let app2 = start_app(&env.discovery, tiny_png(4, 4));
    assert_eq!(s.tool_ok("aiai_status", json!({}))["mode"], "app");
    let _ = app2.finish();
}

#[test]
fn project_flag_forces_headless_even_with_the_app_open() {
    let env = Env::new();
    let app = start_app(&env.discovery, tiny_png(4, 4));
    let proj = env.path("forcado.aiai");
    let p = proj.to_str().unwrap();
    env.cli_ok(&["call", "NEW_PROJECT", r#"{"name":"F"}"#, "--project", p]);
    let r = env.cli_ok(&["call", "APPLY_COMMAND", &create_node("EP", None, json!({"type": "episode"})).to_string(), "--project", p]);
    assert_eq!(r["_via"], "headless");
    // sem --project a mesma chamada vai ao app
    let r = env.cli_ok(&["call", "APPLY_COMMAND", &create_node("EP-app", None, json!({"type": "episode"})).to_string()]);
    assert!(r.get("_via").is_none());
    let session = app.finish();
    assert_eq!(session.project.nodes.len(), 1);
    assert_eq!(std::fs::read_to_string(proj.join("project.json")).unwrap().matches("\"type\": \"episode\"").count(), 1);
}

// ------------------------------------------------------------------------------------------------ unidades

#[test]
fn sandbox_allows_only_listed_dirs_and_blocks_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let allowed = dir.path().join("ok");
    let sb = Sandbox::new(vec![allowed.clone()], true);
    // dentro (ainda inexistente), em subpasta
    let p = sb.check_write(allowed.join("sub").join("a.png").to_str().unwrap()).unwrap();
    assert!(p.ends_with("a.png"));
    // fora, irmão com prefixo parecido, '..', vazio, nulo, fluxo NTFS
    assert!(sb.check_write(dir.path().join("ok2").join("a.png").to_str().unwrap()).is_err(), "prefixo textual não vale");
    assert!(sb.check_write(dir.path().join("a.png").to_str().unwrap()).is_err());
    assert!(sb.check_write(&format!("{}/../a.png", allowed.display())).is_err());
    assert!(sb.check_write("").is_err());
    assert!(sb.check_write("a\0b").is_err());
    assert!(sb.check_write(&format!("{}/a.png:stream", allowed.display())).is_err());
    // relativo vale a partir do diretório atual: fora da lista é recusado, não redirecionado
    assert!(sb.check_write("relativo.png").is_err());
    // gravação desligada
    assert!(Sandbox::new(vec![allowed.clone()], false).check_write(allowed.join("a.png").to_str().unwrap()).is_err());
    #[cfg(windows)]
    {
        // Windows ignora maiúsculas
        let upper = allowed.join("sub").to_string_lossy().to_uppercase();
        assert!(sb.check_write(&upper).is_ok());
    }
}

#[test]
fn tool_arguments_are_validated_and_mapped_to_bridge_actions() {
    let plan = |n: &str, a: Value| tools::plan(n, &a);
    assert_eq!(plan("aiai_apply", json!({"command": {"kind": "batch", "commands": []}, "if_revision": 3})).unwrap().action, "APPLY_COMMAND");
    assert_eq!(plan("aiai_apply", json!({"command": {"kind": "x"}, "if_revision": 3})).unwrap().params["if_revision"], 3);
    assert!(plan("aiai_apply", json!({"command": "texto"})).is_err());
    assert!(plan("aiai_apply", json!([1])).is_err());
    assert_eq!(plan("aiai_ui", json!({"action": "get_state"})).unwrap().action, "GET_UI_STATE");
    let ui = plan("aiai_ui", json!({"action": "set_tool", "tool": "pen"})).unwrap();
    assert_eq!((ui.action, ui.params["tool"].as_str()), ("UI_ACTION", Some("pen")));
    let sh = plan("aiai_screenshot", json!({"panel": "canvas", "out": "x.png"})).unwrap();
    assert_eq!((sh.action, sh.params["inline"].clone(), sh.save_png.as_deref()), ("SCREENSHOT", json!(true), Some("x.png")));
    assert!(sh.params.get("out").is_none(), "o app não recebe o caminho: quem grava é o MCP");
    assert_eq!(plan("aiai_schema", json!({"which": "queries"})).unwrap().params["query"]["which"], "queries");
    assert_eq!(plan("aiai_schema", json!({})).unwrap().params["query"]["which"], "commands");
    assert_eq!(plan("aiai_project", json!({"op": "save"})).unwrap().action, "SAVE_PROJECT");
    assert!(plan("aiai_project", json!({"op": "open"})).is_err(), "open exige path");
    assert!(plan("aiai_input", json!({"events": []})).is_err());
    assert!(plan("aiai_render", json!({"frame": 1})).is_err(), "cut é obrigatório");
    assert!(plan("aiai_zzz", json!({})).is_err());
    // o catálogo é uma lista estável de ferramentas, todas com plano
    let defs = tools::definitions();
    assert_eq!(defs.as_array().unwrap().len(), 97);
    // todas as ferramentas de produção: descrição com exemplo de uma linha, schema de objeto, plano com a ação certa e frame 0 por padrão
    for t in defs.as_array().unwrap() {
        let n = t["name"].as_str().unwrap();
        if ["aiai_status", "aiai_apply", "aiai_query", "aiai_undo", "aiai_redo", "aiai_log", "aiai_project", "aiai_render", "aiai_screenshot", "aiai_input", "aiai_ui", "aiai_schema"].contains(&n) {
            continue;
        }
        assert!(t["description"].as_str().unwrap().contains("Exemplo") || ["aiai_brush_presets", "aiai_lint"].contains(&n) || t["description"].as_str().unwrap().len() > 300, "{n} sem exemplo");
    }
    for (tool, args, action) in [
        ("aiai_sky", json!({"cut": "c", "layer": "l", "sky": {}}), "SKY"),
        ("aiai_pattern_fill", json!({"cut": "c", "layer": "l", "texture": {}, "mapping": {}}), "PATTERN_FILL"),
        ("aiai_text", json!({"cut": "c", "fill_layer": "l", "text": "ドン", "size": 80, "center": [1, 2], "color": {}}), "TEXT"),
        ("aiai_text_layer", json!({"cut": "c", "text": "Olá"}), "TEXT_LAYER"),
        ("aiai_layer_merge", json!({"layer": "l"}), "LAYER_MERGE"),
        ("aiai_gradient_paint", json!({"op": "presets"}), "GRADIENT_PAINT"),
        ("aiai_lines", json!({"cut": "c", "layer": "l", "kind": {}, "count": 3, "length": [1, 2], "width": [1, 2], "color": {}}), "LINES"),
        ("aiai_gag_bg", json!({"cut": "c", "layer": "l", "pattern": {}}), "GAG_BG"),
        ("aiai_anime_fx", json!({"cut": "c", "layer": "l", "kind": {}}), "ANIME_FX"),
        ("aiai_colorize_hints", json!({"cut": "c", "fill_layer": "f", "hint_layer": "h"}), "COLORIZE_HINTS"),
        ("aiai_rig", json!({"op": "parent", "layer": "l", "parent": "p"}), "RIG"),
        ("aiai_plugin", json!({"op": "filter", "cut": "c", "layer": "l", "plugin": "p"}), "PLUGIN_FILTER"),
        ("aiai_plugin", json!({"op": "list"}), "PLUGIN_LIST"),
        ("aiai_particles", json!({"layer": "l", "preset": "neve"}), "PARTICLES"),
        ("aiai_guide3d", json!({"cut": "c", "layer": "l", "scene": {}, "camera": {}}), "GUIDE3D"),
        ("aiai_perspective", json!({"cut": "c", "layer": "l"}), "PERSPECTIVE"),
        ("aiai_model_sheet", json!({"character": "x", "heads_tall": 6}), "MODEL_SHEET"),
        ("aiai_character_material", json!({"character": "x"}), "CHARACTER_MATERIAL"),
        ("aiai_consistency", json!({"character": "x", "head_top": 0, "chin": 10}), "CONSISTENCY"),
        ("aiai_inbetween", json!({"cut": "c", "layer": "l", "key_a": 1, "key_b": 2, "count": 1}), "INBETWEEN"),
        ("aiai_inbetween_preview", json!({"cut": "c", "layer": "l", "frame": 3}), "INBETWEEN_PREVIEW"),
        ("aiai_inbetween_accept", json!({"proposal_id": 7}), "INBETWEEN_ACCEPT"),
        ("aiai_inbetween_discard", json!({"proposal_id": 7}), "INBETWEEN_DISCARD"),
        ("aiai_propagate_fills", json!({"cut": "c", "fill_layer": "l", "key_frame_a": 0, "key_frame_b": 4, "frames": [2]}), "PROPAGATE_FILLS"),
        ("aiai_tts_line", json!({"text": "x"}), "TTS_LINE"),
        ("aiai_tts_kokoro", json!({"text": "x"}), "TTS_KOKORO"),
        ("aiai_voice_profile", json!({"character": "x", "speaker": 8}), "VOICE_PROFILE"),
        ("aiai_lipsync", json!({"cut": "c", "mouth_layer": "l", "mapping": {}}), "LIPSYNC"),
        ("aiai_sfx", json!({"preset": "impact"}), "SFX"),
        ("aiai_bgm", json!({"song": {}}), "BGM"),
        ("aiai_mix", json!({"cut": "c"}), "MIX_CUT"),
        ("aiai_loudness", json!({"cut": "c"}), "LOUDNESS"),
        ("aiai_audio_edit", json!({"asset": "a", "steps": [{"op": "mono"}]}), "AUDIO_EDIT"),
        ("aiai_audio_analyze", json!({"asset": "a"}), "AUDIO_ANALYZE"),
        ("aiai_ruler", json!({"layer": "l", "op": "list"}), "RULER"),
        ("aiai_line_fix", json!({"cut": "c", "layer": "l", "op": "dust"}), "LINE_FIX"),
        ("aiai_vectorize", json!({"cut": "c", "layer": "l", "source": {"image": {"path": "x.png"}}}), "VECTORIZE"),
        ("aiai_fill_area", json!({"cut": "c", "layer": "l", "mode": "enclose", "color": {"rgba": [1, 0, 0, 1]}, "path": [[0, 0], [9, 0], [9, 9]]}), "FILL_AREA"),
        ("aiai_model3d", json!({"op": "list"}), "MODEL3D"),
        ("aiai_modeling", json!({"op": "list"}), "MODELING"),
        ("aiai_library", json!({"op": "list"}), "LIBRARY"),
        ("aiai_delivery", json!({"op": "formats"}), "DELIVERY"),
        ("aiai_compare", json!({"cut": "c", "reference": "r.png"}), "COMPARE"),
        ("aiai_probe_color", json!({"reference": "r.png", "points": [[1, 1]]}), "PROBE_COLOR"),
        ("aiai_measure_line", json!({"reference": "r.png", "points": [[1, 1], [2, 2]]}), "MEASURE_LINE"),
        ("aiai_measure_edge", json!({"reference": "r.png", "points": [[1, 1], [2, 2], [3, 1]]}), "MEASURE_EDGE"),
        ("aiai_fill_shape", json!({"cut": "c", "layer": "l", "shapes": [], "color": {}}), "FILL_SHAPE"),
        ("aiai_edit", json!({"clips": []}), "EDIT_SET"),
        ("aiai_export_video", json!({"edit": "e", "out": "x.mp4"}), "EXPORT_VIDEO"),
        ("aiai_script", json!({"op": "read", "doc": "d"}), "SCRIPT"),
        ("aiai_beats", json!({"op": "read", "doc": "d"}), "BEATS"),
        ("aiai_storyboard", json!({"op": "read", "storyboard": "s"}), "STORYBOARD"),
        ("aiai_animatic", json!({"storyboard": "s", "out": "a.mp4"}), "ANIMATIC"),
        ("aiai_cuts_from_storyboard", json!({"storyboard": "s", "episode": "e"}), "CUTS_FROM_STORYBOARD"),
        ("aiai_production", json!({"op": "cut_table"}), "PRODUCTION"),
        ("aiai_correction", json!({"op": "list", "cut": "c"}), "CORRECTION"),
        ("aiai_retake", json!({"op": "list"}), "RETAKE"),
        ("aiai_effects", json!({"op": "list", "layer": "l"}), "EFFECTS"),
        ("aiai_grade", json!({"cut": "c", "effects": []}), "GRADE"),
        ("aiai_camera", json!({"op": "read", "cut": "c"}), "CAMERA"),
        ("aiai_layer_motion", json!({"op": "read", "layer": "l"}), "LAYER_MOTION"),
        ("aiai_render_sheet", json!({"cut": "c", "out": "x.png"}), "RENDER_SHEET"),
        ("aiai_edit_preview", json!({"edit": "e", "out": "x.png"}), "EDIT_PREVIEW"),
        ("aiai_edit_mix", json!({"edit": "e", "out": "x.wav"}), "EDIT_MIX"),
        ("aiai_edit_ops", json!({"edit": "e", "ops": []}), "EDIT_OPS"),
        ("aiai_edit_captions", json!({"edit": "e", "action": "check"}), "EDIT_CAPTIONS"),
        ("aiai_tts_regenerate", json!({"asset": "a"}), "TTS_REGENERATE"),
    ] {
        let p = plan(tool, args.clone()).unwrap_or_else(|e| panic!("{tool}: {e}"));
        assert_eq!(p.action, action, "{tool}");
        if ["aiai_sky", "aiai_text", "aiai_lines", "aiai_gag_bg", "aiai_anime_fx", "aiai_guide3d", "aiai_perspective", "aiai_compare", "aiai_fill_shape", "aiai_pattern_fill"].contains(&tool) {
            assert_eq!(p.params["frame"], json!(0), "{tool}: frame ausente vira 0");
        }
        assert!(defs.as_array().unwrap().iter().any(|t| t["name"] == tool && t["inputSchema"]["type"] == "object"), "{tool} está no catálogo");
    }
    for (tool, args) in [
        ("aiai_sky", json!({"cut": "c", "layer": "l"})),
        ("aiai_particles", json!({"layer": "l"})),
        ("aiai_export_video", json!({"out": "x.mp4"})),
        ("aiai_measure_line", json!({"reference": "r.png"})),
        ("aiai_measure_edge", json!({"reference": "r.png"})),
        ("aiai_compare", json!({"cut": "c"})),
        ("aiai_sfx", json!({})),
        ("aiai_ruler", json!({"layer": "l"})),
        ("aiai_model3d", json!({"layer": "l"})),
        ("aiai_modeling", json!({"asset": "a"})),
        ("aiai_library", json!({"query": "x"})),
        ("aiai_delivery", json!({"preset": "tv"})),
    ] {
        assert!(plan(tool, args.clone()).is_err(), "{tool} {args}: deveria faltar algo");
    }
    // decisão 22 na descrição das medições
    for n in ["aiai_measure_line", "aiai_measure_edge"] {
        let d = defs.as_array().unwrap().iter().find(|t| t["name"] == n).unwrap()["description"].as_str().unwrap().to_string();
        assert!(d.contains("decisão 22") && d.contains("só MEDE") && d.contains("não cria"), "{n}");
    }
    // a cola de aiai_apply cita as ferramentas de produção
    let apply_desc = defs.as_array().unwrap().iter().find(|t| t["name"] == "aiai_apply").unwrap()["description"].as_str().unwrap().to_string();
    for n in ["aiai_sky", "aiai_text", "aiai_particles", "aiai_export_video", "aiai_measure_line", "aiai_edit", "aiai_mix"] {
        assert!(apply_desc.contains(n), "a cola de aiai_apply não cita {n}");
    }
    // ferramentas curadas: validam o essencial e mapeiam para a ação certa; `frame` ausente vira 0
    let p = plan("aiai_draw_curves", json!({"cut": "c", "layer": "l", "curves": [{"points": [[0, 0], [1, 1]]}]})).unwrap();
    assert_eq!((p.action, p.params["frame"].clone()), ("DRAW_CURVES", json!(0)));
    assert!(plan("aiai_draw_curves", json!({"cut": "c", "layer": "l", "curves": []})).is_err());
    assert!(plan("aiai_fill", json!({"cut": "c", "layer": "l"})).is_err(), "seeds é obrigatório");
    assert_eq!(plan("aiai_lint", json!({"cut": "c", "layer": "l"})).unwrap().action, "LINT");
    assert_eq!(plan("aiai_brush_presets", json!({})).unwrap().action, "BRUSH_PRESETS");
    assert_eq!(plan("aiai_paint", json!({"cut": "c", "layer": "l", "preset": "Redondo duro", "color": {}, "strokes": []})).unwrap().action, "PAINT");
    // o guia de desenho está na descrição e os schemas vêm dos tipos de requisição
    let draw = defs.as_array().unwrap().iter().find(|t| t["name"] == "aiai_draw_curves").unwrap();
    let d = draw["description"].as_str().unwrap();
    for needle in ["origem no canto superior esquerdo", "y PARA BAIXO", "[x,y,pressão", "widths", "g_pen", "maru_pen", "marker", "pencil", "\"palette\"", "horária"] {
        assert!(d.contains(needle), "o guia de aiai_draw_curves não cita {needle}");
    }
    assert!(draw["inputSchema"].to_string().contains("mirror_x"));
    let paint = defs.as_array().unwrap().iter().find(|t| t["name"] == "aiai_paint").unwrap();
    assert!(paint["inputSchema"]["properties"]["preset"]["anyOf"][0]["type"] == "string" && paint["inputSchema"]["properties"]["preset_overrides"].is_object());
}

#[test]
fn sandbox_read_rules_cover_the_repo_and_the_write_dirs_only() {
    let dir = tempfile::tempdir().unwrap();
    let sb = Sandbox::new(vec![dir.path().to_path_buf()], true).with_read_dirs(vec![PathBuf::from(r"C:\AnimeIAI")]);
    assert!(sb.check_read(r"C:\AnimeIAI\calibracao\q0000.png").is_ok());
    assert!(sb.check_read(dir.path().join("x.png").to_str().unwrap()).is_ok());
    for bad in [r"C:\Windows\win.ini", r"C:\AnimeIAI\..\Windows\win.ini", "relativo.png", ""] {
        assert!(sb.check_read(bad).is_err(), "{bad}");
    }
    // leitura não depende de a gravação estar ligada
    assert!(Sandbox::new(vec![dir.path().to_path_buf()], false).with_read_dirs(vec![PathBuf::from(r"C:\AnimeIAI")]).check_read(r"C:\AnimeIAI\x.png").is_ok());
}

#[test]
fn production_tools_over_stdio_headless_gag_shot_measure_and_export() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg não encontrado: teste pulado");
        return;
    }
    let env = Env::new();
    let mut s = env.mcp();
    handshake(&mut s);
    // medições valem SEM projeto (headless sem projeto) e respeitam a leitura do sandbox
    let reference = env.path("ref.png");
    {
        let mut img = image::RgbaImage::from_pixel(64, 40, image::Rgba([255, 255, 255, 255]));
        for y in 0..40 {
            for x in 30..33 {
                img.put_pixel(x, y, image::Rgba([10, 10, 10, 255]));
            }
        }
        img.save(&reference).unwrap();
    }
    let probe = s.tool_ok("aiai_probe_color", json!({"reference": reference.to_str().unwrap(), "points": [[5, 5], [31, 20]]}));
    assert_eq!(probe["colors"][0]["hex"], "#ffffff");
    assert_eq!(probe["_via"], "headless");
    let m = s.tool_ok("aiai_measure_line", json!({"reference": reference.to_str().unwrap(), "points": [[28.0, 8.0], [28.0, 20.0], [28.0, 32.0]], "search": 6}));
    assert_eq!(m["found"], 3, "{m}");
    assert!((m["measures"][1]["center"][0].as_f64().unwrap() - 31.5).abs() < 0.6, "{m}");
    let e = s.tool_err("aiai_probe_color", json!({"reference": r"C:\Windows\win.ini", "points": [[1, 1]]}));
    assert!(e.contains("fora das pastas permitidas"), "{e}");

    // projeto e cena de gag: fundo radial + linhas de foco + ドン + pétalas; SFX no corte; export de 12 quadros com áudio
    let proj = env.path("gag.aiai");
    s.tool_ok("aiai_project", json!({"op": "new", "name": "Gag", "path": proj.to_str().unwrap(), "settings": {"width": 320, "height": 180}}));
    let mk = |s: &mut McpSession, name: &str, parent: Option<&str>, data: Value| -> String { s.tool_ok("aiai_apply", create_node(name, parent, data))["created"][0].as_str().unwrap().to_string() };
    let ep = mk(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = mk(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = mk(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 24}));
    let bg = mk(&mut s, "BG", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    let txt = mk(&mut s, "TXT", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    let lin = mk(&mut s, "LINHAS", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    let pet = mk(&mut s, "PETALAS", Some(&cut), json!({"type": "layer", "kind": "particles"}));
    for l in [&bg, &txt, &lin] {
        mk(&mut s, "c1", Some(l), json!({"type": "cel", "number": 1}));
    }
    let g = s.tool_ok("aiai_gag_bg", json!({"cut": cut, "layer": bg, "pattern": {"type": "radial_stripes", "center": [160.0, 90.0], "count": 24, "colors": ["#fff3b0", "#ffd23f"]}}));
    assert_eq!(g["can_undo"], true);
    s.tool_ok("aiai_lines", json!({"cut": cut, "layer": lin, "kind": {"type": "focus", "center": [160.0, 90.0], "clear": [70.0, 50.0]}, "count": 70, "length": [50.0, 200.0], "width": [1.0, 4.0], "color": {"type": "direct", "rgba": [0, 0, 0, 1]}}));
    s.tool_ok("aiai_text", json!({"cut": cut, "fill_layer": txt, "text": "ドン", "size": 100, "center": [160.0, 90.0], "color": {"type": "direct", "rgba": [1, 0.15, 0.1, 1]}}));
    s.tool_ok("aiai_particles", json!({"layer": pet, "preset": "petalas", "overrides": {"rate": 50}}));
    let sfx = s.tool_ok("aiai_sfx", json!({"preset": "impact", "seed": 2, "cut": cut, "start_frame": 0}));
    assert!(sfx["duration_s"].as_f64().unwrap() > 0.1);
    let mix = s.tool_ok("aiai_mix", json!({"cut": cut, "preset": "streaming"}));
    assert!(mix["normalize"]["after_lufs"].is_number(), "{mix}");
    let out = env.path("gag.mp4");
    let r = s.tool_ok("aiai_export_video", json!({"cut": cut, "start": 0, "end": 12, "out": out.to_str().unwrap()}));
    assert_eq!((r["frames"].as_u64(), r["probe"]["frames"].as_u64()), (Some(12), Some(12)), "{r}");
    assert_eq!(r["probe"]["audio_codec"], "aac");
    // destino fora do sandbox: recusado antes de renderizar
    let outside = std::env::current_dir().unwrap().join("fora.mp4");
    assert!(s.tool_err("aiai_export_video", json!({"cut": cut, "out": outside.to_str().unwrap()})).contains("fora das pastas permitidas"));
    assert!(!outside.exists());
    // erros de ferramenta voltam como erro MCP com o código
    assert!(s.tool_err("aiai_particles", json!({"layer": pet, "preset": "lava"})).contains("petalas"));
    assert!(s.tool_err("aiai_tts_line", json!({"text": "x", "speaker": 8, "voicevox_url": "http://127.0.0.1:9"})).contains("start.ps1"));
    // o projeto em disco guarda tudo e o undo do servidor desfaz o último passo (SFX)
    let disk = std::fs::read_to_string(proj.join("project.json")).unwrap();
    assert!(disk.contains("particles") && disk.contains("audio"));
    s.tool_ok("aiai_undo", json!({}));
}

#[test]
fn cli_parser_forms() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(cli::parse(&a(&[])).unwrap(), Cmd::Serve);
    assert_eq!(cli::parse(&a(&["info"])).unwrap(), Cmd::Info);
    assert_eq!(cli::parse(&a(&["--help"])).unwrap(), Cmd::Help);
    let c = cli::parse(&a(&["call", "QUERY", r#"{"query":{"kind":"stats"}}"#, "--png", "o.png", "--project", "d", "--timeout", "7"])).unwrap();
    match c {
        Cmd::Call { action, params, png, project, timeout } => {
            assert_eq!((action.as_str(), png.as_deref(), project.as_deref(), timeout), ("QUERY", Some("o.png"), Some("d"), Some(7)));
            assert_eq!(params["query"]["kind"], "stats");
        }
        other => panic!("{other:?}"),
    }
    for bad in [vec!["call"], vec!["call", "ping"], vec!["call", "PING; x"], vec!["call", "PING", "[1]"], vec!["call", "PING", "{}", "{}"], vec!["call", "PING", "--png"], vec!["call", "PING", "--timeout", "9999"]] {
        assert!(cli::parse(&a(&bad)).is_err(), "{bad:?}");
    }
}

/// O cliente Python (somente biblioteca padrão) fala HTTP /rpc com a mesma bridge. Pula se não houver Python.
#[test]
fn python_client_talks_to_the_bridge_over_http() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/aiai.py");
    if !script.is_file() {
        eprintln!("tools/aiai.py ausente: teste pulado");
        return;
    }
    let py = |env: &Env, args: &[&str]| -> Option<Output> {
        // `py -3` (lançador do Windows) antes de `python`: o `python` do PATH pode ser o da Microsoft Store, empacotado e com
        // sistema de arquivos virtualizado (não enxerga a pasta temporária do teste).
        let mut c = if cfg!(windows) { Command::new("py") } else { Command::new("python3") };
        env.apply(&mut c);
        if cfg!(windows) {
            c.arg("-3");
        }
        c.arg("-I").arg(&script).args(args).output().ok()
    };
    let env = Env::new();
    let app = start_app(&env.discovery, tiny_png(30, 20));
    let Some(ping) = py(&env, &["PING"]) else {
        eprintln!("python não encontrado: teste pulado");
        let _ = app.finish();
        return;
    };
    assert!(ping.status.success(), "{}", stderr_of(&ping));
    assert_eq!(serde_json::from_slice::<Value>(&ping.stdout).unwrap()["pong"], true);
    let ep = py(&env, &["APPLY_COMMAND", &create_node("EP-py", None, json!({"type": "episode"})).to_string()]).unwrap();
    assert!(ep.status.success(), "{}", stderr_of(&ep));
    assert_eq!(serde_json::from_slice::<Value>(&ep.stdout).unwrap()["revision"], 1);
    let png = env.path("py/tela.png");
    let sh = py(&env, &["SCREENSHOT", "--png", png.to_str().unwrap()]).unwrap();
    assert!(sh.status.success(), "{}", stderr_of(&sh));
    assert_eq!(image::open(&png).unwrap().width(), 30);
    // erro da bridge vira saída 1 com o código
    let bad = py(&env, &["APPLY_COMMAND", r#"{"command":{"kind":"nao_existe"}}"#]).unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert!(stderr_of(&bad).contains("-32602"), "{}", stderr_of(&bad));
    let session = app.finish();
    assert_eq!(session.project.nodes.len(), 1);
    // sem app: mensagem acionável
    let down = py(&env, &["PING"]).unwrap();
    assert_eq!(down.status.code(), Some(1));
    assert!(stderr_of(&down).contains("não está aberto com --bridge"), "{}", stderr_of(&down));
}

// ------------------------------------------------------------------------------------------------ ferramentas curadas (headless)

fn image_block(content: &[Value]) -> image::RgbaImage {
    let img = content.iter().find(|c| c["type"] == "image").expect("conteúdo de imagem MCP");
    assert_eq!(img["mimeType"], "image/png");
    image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(img["data"].as_str().unwrap()).unwrap()).unwrap().to_rgba8()
}

#[test]
fn curated_tools_draw_fill_paint_gradient_and_render_headless_over_stdio() {
    let env = Env::new();
    let mut s = env.mcp();
    handshake(&mut s);
    let proj = env.path("arte.aiai");
    s.tool_ok("aiai_project", json!({"op": "new", "name": "Arte", "path": proj.to_str().unwrap(), "settings": {"width": 320, "height": 180}}));
    let mk = |s: &mut McpSession, name: &str, parent: Option<&str>, data: Value| -> String {
        s.tool_ok("aiai_apply", create_node(name, parent, data))["created"][0].as_str().unwrap().to_string()
    };
    let ep = mk(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = mk(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = mk(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 24}));
    let bg = mk(&mut s, "BG", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    let cor = mk(&mut s, "A-cor", Some(&cut), json!({"type": "layer", "kind": "fill"}));
    let linha = mk(&mut s, "A-linha", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    for l in [&bg, &cor, &linha] {
        mk(&mut s, "c1", Some(l), json!({"type": "cel", "number": 1}));
    }
    s.tool_ok("aiai_apply", json!({"command": {"kind": "set_props", "id": cor, "data": {"line_refs": [linha]}}}));

    // paleta (nó) + cor por referência de paleta
    let pal = mk(&mut s, "Settei", None, json!({"type": "palette", "colors": [{"id": "pele", "name": "pele", "group": "pele", "role": "normal", "rgba": [0.98, 0.89, 0.83, 1.0]}], "active_variant": ""}));

    s.tool_ok("aiai_gradient", json!({"cut": cut, "layer": bg, "kind": "linear", "from": [0, 0], "to": [0, 180], "stops": [[0.0, "#2050d0ff"], [1.0, "#d0e8ffff"]]}));
    s.tool_ok("aiai_draw_curves", json!({"cut": cut, "layer": linha, "curves": [
        {"points": [[110, 40], [210, 40], [210, 140], [110, 140]], "closed": true, "geometry": "polyline", "width": 4}]}));
    let lint = s.tool_ok("aiai_lint", json!({"cut": cut, "layer": cor}));
    let pt = lint["unpainted"][0]["point"].clone();
    s.tool_ok("aiai_fill", json!({"cut": cut, "layer": cor, "seeds": [{"x": pt[0], "y": pt[1], "color": {"type": "palette", "palette": pal, "color": "pele"}}]}));
    assert_eq!(s.tool_ok("aiai_lint", json!({"cut": cut, "layer": cor}))["unpainted"], json!([]));
    let presets = s.tool_ok("aiai_brush_presets", json!({}));
    assert!(presets["names"].as_array().unwrap().iter().any(|n| n == "Redondo duro"));
    s.tool_ok("aiai_paint", json!({"cut": cut, "layer": bg, "preset": "Redondo duro", "preset_overrides": {"size": 16}, "color": {"type": "direct", "rgba": [0.0, 0.5, 0.0, 1.0]}, "strokes": [{"points": [[20, 160], [80, 160]]}]}));

    // render inline: vem como conteúdo de imagem MCP; o interior tem a cor da paleta, o céu o degradê
    let (err, content) = s.tool("aiai_render", json!({"cut": cut, "frame": 0, "inline": true}));
    assert!(!err, "{content:?}");
    let meta: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!((meta["width"].as_u64(), meta["height"].as_u64(), meta["warnings"].clone()), (Some(320), Some(180), json!([])));
    let img = image_block(&content);
    let inside = img.get_pixel(160, 90).0;
    assert!(inside[0] > 230 && inside[1] > 200 && inside[2] > 190, "pele na paleta: {inside:?}");
    let sky = img.get_pixel(20, 5).0;
    assert!(sky[2] > sky[0] + 80, "céu azul: {sky:?}");
    let grass = img.get_pixel(50, 160).0;
    assert!(grass[1] > grass[0] + 60, "pincelada verde: {grass:?}");
    let line = img.get_pixel(160, 40).0;
    assert!(line[0] < 60 && line[1] < 60, "linha preta: {line:?}");

    // render com out (sem inline): grava o arquivo no sandbox e não devolve imagem
    let out = env.path("r/quadro.png");
    let (err, content) = s.tool("aiai_render", json!({"cut": cut, "out": out.to_str().unwrap(), "size": [160, 90]}));
    assert!(!err && content.iter().all(|c| c["type"] != "image"), "{content:?}");
    assert_eq!(image::open(&out).unwrap().width(), 160);
    // destino fora do sandbox: recusado
    let outside = std::env::current_dir().unwrap().join("fora-render.png");
    assert!(s.tool_err("aiai_render", json!({"cut": cut, "out": outside.to_str().unwrap()})).contains("fora das pastas permitidas"));
    assert!(!outside.exists());

    // undo da pintura (histórico do servidor) e confirmação pelo render
    s.tool_ok("aiai_undo", json!({}));
    let (_, content) = s.tool("aiai_render", json!({"cut": cut}));
    let g = image_block(&content).get_pixel(50, 160).0;
    assert!(g[1] <= g[0] + 60, "a pincelada verde sumiu com o undo: {g:?}");

    // erros claros e com o código certo
    assert!(s.tool_err("aiai_fill", json!({"cut": cut, "layer": cor, "seeds": [{"x": 999.0, "y": 40.0, "color": {"type": "direct", "rgba": [1, 0, 0, 1]}}]})).contains("fora do canvas"), "semente fora do canvas");
    assert!(s.tool_err("aiai_paint", json!({"cut": cut, "layer": bg, "preset": "xyz", "color": {"type": "direct", "rgba": [0, 0, 0, 1]}, "strokes": []})).contains("preset desconhecido"));
    assert!(s.tool_err("aiai_render", json!({"cut": "nao-existe"})).contains("corte"));
}

#[test]
fn cli_headless_runs_tool_actions_and_saves_the_png() {
    let env = Env::new();
    let proj = env.path("cli.aiai");
    let p = proj.to_str().unwrap();
    env.cli_ok(&["call", "NEW_PROJECT", r#"{"name":"cli","settings":{"width":64,"height":36}}"#, "--project", p]);
    let id = |v: Value| v["created"][0].as_str().unwrap().to_string();
    let ap = |cmd: Value| id(env.cli_ok(&["call", "APPLY_COMMAND", &cmd.to_string(), "--project", p]));
    let ep = ap(create_node("EP", None, json!({"type": "episode", "number": 1})));
    let sc = ap(create_node("S", Some(&ep), json!({"type": "scene", "number": 1})));
    let cut = ap(create_node("C", Some(&sc), json!({"type": "cut", "number": 1, "duration": 4})));
    let bg = ap(create_node("BG", Some(&cut), json!({"type": "layer", "kind": "raster"})));
    ap(create_node("c1", Some(&bg), json!({"type": "cel", "number": 1})));
    let g = env.cli_ok(&["call", "GRADIENT", &json!({"cut": cut, "frame": 0, "layer": bg, "kind": "linear", "from": [0, 0], "to": [0, 36], "stops": [[0.0, "#ff0000ff"], [1.0, "#ff0000ff"]]}).to_string(), "--project", p]);
    assert_eq!(g["_via"], "headless");
    // outro processo lê do disco: render com --png grava o arquivo
    let png = env.path("cli.png");
    let r = env.cli_ok(&["call", "RENDER", &json!({"cut": cut, "frame": 0}).to_string(), "--png", png.to_str().unwrap(), "--project", p]);
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(64), Some(36)));
    assert!(r["png_base64"].as_str().unwrap().contains("bytes salvos"));
    assert_eq!(image::open(&png).unwrap().to_rgba8().get_pixel(3, 3).0, [255, 0, 0, 255]);
}
