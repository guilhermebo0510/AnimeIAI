//! Suíte única do `aiai-bridge` (um binário só; ver lições de disco): unidades dos módulos públicos + integração por TCP real
//! com uma thread que faz o papel do "app" (`core_ops::handle_core_action_with_path` sobre uma `CoreSession::for_tests`).

use aiai_bridge::core_ops::{self, handle_core_action, handle_core_action_with_path};
use aiai_bridge::discovery::{self, Discovery};
use aiai_bridge::host::{self, BridgeReceiver};
use aiai_bridge::http;
use aiai_bridge::protocol::{self, BridgeError, ct_eq, generate_token, response_json};
use aiai_bridge::server::{BridgeConfig, BridgeServer, parse_loopback_port};
use aiai_bridge::{Backend, Request, code};
use aiai_core::CoreSession;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// ------------------------------------------------------------------------------------------------ infraestrutura

/// Servidor + thread "app" que responde às ações. Ação `UI_ECHO` simula uma ação de UI; o resto vai para o core_ops.
struct Rig {
    server: Option<BridgeServer>,
    stop: Arc<AtomicBool>,
    app: Option<JoinHandle<CoreSession>>,
    discovery: PathBuf,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn start() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let discovery = dir.path().join("bridge").join("bridge.json");
        let mut cfg = BridgeConfig::default();
        cfg.server.discovery = Some(discovery.clone());
        cfg.server.auth_failure_delay = Duration::from_millis(5);
        let (server, rx) = BridgeServer::start(cfg).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        let app = std::thread::spawn(move || app_loop(rx, stop2));
        Rig { server: Some(server), stop, app: Some(app), discovery, _dir: dir }
    }

    fn addr(&self) -> String {
        self.server.as_ref().unwrap().addr()
    }

    fn token(&self) -> String {
        self.server.as_ref().unwrap().token().to_string()
    }

    /// Chamada NDJSON autenticada; devolve a resposta inteira.
    fn rpc(&self, action: &str, params: Value) -> Value {
        ndjson(&self.addr(), &json!({ "id": "t", "action": action, "params": params, "token": self.token() }))
    }

    fn ok(&self, action: &str, params: Value) -> Value {
        let r = self.rpc(action, params);
        assert_eq!(r["success"], true, "{action}: {r}");
        r["data"].clone()
    }

    fn finish(mut self) -> CoreSession {
        self.stop.store(true, Ordering::Relaxed);
        let session = self.app.take().unwrap().join().unwrap();
        drop(self.server.take());
        session
    }
}

fn app_loop(rx: BridgeReceiver, stop: Arc<AtomicBool>) -> CoreSession {
    let mut session = CoreSession::for_tests("rig");
    let mut path = None;
    while !stop.load(Ordering::Relaxed) {
        for req in rx.try_recv_all() {
            let result = if req.action == "UI_ECHO" {
                Ok(json!({ "echo": req.params }))
            } else {
                match handle_core_action_with_path(&mut session, &mut path, &req.action, &req.params) {
                    Some(r) => r,
                    None => Err(BridgeError::unknown_action(&req.action)),
                }
            };
            req.respond(result);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    session
}

fn ndjson(addr: &str, req: &Value) -> Value {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    writeln!(s, "{req}").unwrap();
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap_or_else(|e| panic!("resposta não é JSON ({e}): {line:?}"))
}

/// HTTP cru: devolve (status, corpo JSON).
fn http_raw(addr: &str, raw: &str) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text.split(' ').nth(1).unwrap().parse().unwrap();
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn http_rpc(addr: &str, token: Option<&str>, extra_headers: &str, body: &Value) -> (u16, Value) {
    let b = body.to_string();
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    http_raw(addr, &format!("POST /rpc HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{auth}{extra_headers}\r\n{b}", b.len()))
}

fn create_node(name: &str, parent: Option<&str>, data: Value) -> Value {
    let mut c = json!({ "kind": "create_node", "name": name, "data": data });
    if let Some(p) = parent {
        c["parent"] = json!(p);
    }
    json!({ "command": c })
}

// ------------------------------------------------------------------------------------------------ integração (TCP real)

#[test]
fn auth_failures_are_rejected_before_anything_runs() {
    let rig = Rig::start();
    for token in [None, Some("errado"), Some("")] {
        let mut req = json!({ "id": "a", "action": "APPLY_COMMAND", "params": create_node("EP", None, json!({"type": "episode"})) });
        if let Some(t) = token {
            req["token"] = json!(t);
        }
        let r = ndjson(&rig.addr(), &req);
        assert_eq!(r["success"], false);
        assert_eq!(r["code"], code::UNAUTHORIZED, "{r}");
        assert!(r["error"].as_str().unwrap().contains("Unauthorized"));
    }
    let session = rig.finish();
    assert_eq!(session.project.revision, 0, "nada pode ter sido aplicado sem token");
    assert!(session.project.nodes.is_empty());
}

#[test]
fn unknown_action_and_invalid_params_have_the_right_codes() {
    let rig = Rig::start();
    let r = rig.rpc("NAO_EXISTE", json!({}));
    assert_eq!((r["success"].as_bool(), r["code"].as_i64()), (Some(false), Some(code::UNKNOWN_ACTION as i64)), "{r}");
    for (action, params) in [
        ("APPLY_COMMAND", json!({})),
        ("APPLY_COMMAND", json!({"command": "texto"})),
        ("APPLY_COMMAND", json!({"command": {"kind": 3}})),
        ("APPLY_COMMAND", json!({"command": {"kind": "nao_existe"}})),
        ("APPLY_COMMAND", json!({"command": {"kind": "delete_node"}})),
        ("APPLY_COMMAND", json!({"command": {"kind": "delete_node", "id": "x"}, "if_revision": "alta"})),
        ("QUERY", json!({})),
        ("QUERY", json!({"query": {"kind": "nada"}})),
        ("QUERY", json!({"query": {"kind": "node"}})),
        ("GET_LOG", json!({"from": "zero"})),
        ("NEW_PROJECT", json!({})),
        ("NEW_PROJECT", json!({"name": "x", "settings": {"width": "larga"}})),
        ("OPEN_PROJECT", json!({})),
        ("SAVE_PROJECT", json!({})),
    ] {
        let r = rig.rpc(action, params.clone());
        assert_eq!(r["code"], code::INVALID_PARAMS, "{action} {params} => {r}");
    }
    // semanticamente inválido (nó inexistente) é "ação falhou", sem efeito
    let r = rig.rpc("APPLY_COMMAND", json!({"command": {"kind": "delete_node", "id": "fantasma"}}));
    assert_eq!(r["code"], code::ACTION_FAILED, "{r}");
    let session = rig.finish();
    assert_eq!(session.project.revision, 0);
}

#[test]
fn apply_and_undo_round_trip_over_real_tcp() {
    let rig = Rig::start();
    let ping = rig.ok("PING", json!({}));
    assert_eq!(ping["pong"], true);

    // episódio -> cena -> corte (ids gerados pela sessão; o agente os lê do Outcome)
    let ep = rig.ok("APPLY_COMMAND", create_node("EP01", None, json!({"type": "episode", "number": 1})));
    assert_eq!(ep["revision"], 1);
    assert_eq!(ep["can_undo"], true);
    let ep_id = ep["created"][0].as_str().unwrap().to_string();
    let sc = rig.ok("APPLY_COMMAND", create_node("Cena 1", Some(&ep_id), json!({"type": "scene"})));
    let sc_id = sc["created"][0].as_str().unwrap().to_string();

    // consulta confirma o que foi aplicado (round-trip, não o "ok" do retorno)
    let tree = rig.ok("QUERY", json!({"query": {"kind": "tree"}}));
    assert_eq!(tree["episodes"][0]["name"], "EP01");
    assert_eq!(tree["episodes"][0]["children"][0]["id"], sc_id.as_str());

    // if_revision desatualizado é recusado com código próprio e não muda nada
    let stale = rig.rpc("APPLY_COMMAND", json!({"command": {"kind": "delete_node", "id": sc_id}, "if_revision": 0}));
    assert_eq!(stale["code"], code::STALE_REVISION, "{stale}");
    let stats = rig.ok("QUERY", json!({"query": {"kind": "stats"}}));
    assert_eq!(stats["nodes"], 2);

    // desfazer a cena
    let undo = rig.ok("UNDO", json!({}));
    assert_eq!(undo["revision"], 3);
    assert_eq!(undo["can_redo"], true);
    let tree = rig.ok("QUERY", json!({"query": {"kind": "tree"}}));
    assert!(tree["episodes"][0].get("children").is_none(), "a cena deve ter sumido: {tree}");
    rig.ok("REDO", json!({}));
    let tree = rig.ok("QUERY", json!({"query": {"kind": "tree"}}));
    assert_eq!(tree["episodes"][0]["children"][0]["name"], "Cena 1");

    // log: autor padrão da bridge, e GET_LOG paginado
    let log = rig.ok("GET_LOG", json!({"from": 0, "limit": 2, "include_commands": true}));
    assert_eq!(log["total"], 4);
    assert_eq!(log["entries"].as_array().unwrap().len(), 2);
    assert_eq!(log["entries"][0]["author"], "agent:claude");
    assert_eq!(log["entries"][0]["command"]["kind"], "create_node");
    let log = rig.ok("GET_LOG", json!({}));
    assert!(log["entries"][0].get("command").is_none());
    let ops: Vec<&str> = log["entries"].as_array().unwrap().iter().map(|e| e["op"].as_str().unwrap()).collect();
    assert_eq!(ops, ["apply", "apply", "undo", "redo"]);

    // autor explícito
    rig.ok("APPLY_COMMAND", json!({"command": {"kind": "set_props", "id": ep_id, "name": "EP01-v2"}, "author": "agent:teste"}));
    let log = rig.ok("GET_LOG", json!({"from": 4}));
    assert_eq!(log["entries"][0]["author"], "agent:teste");

    // o autor humano da sessão não foi alterado por UNDO/REDO da bridge
    let session = rig.finish();
    assert_eq!(session.author, "human");
}

#[test]
fn ui_actions_travel_through_the_queue_and_batches_are_one_undo_step() {
    let rig = Rig::start();
    let r = rig.ok("UI_ECHO", json!({"x": [1, 2]}));
    assert_eq!(r["echo"]["x"][1], 2);
    let batch = json!({"command": {"kind": "batch", "label": "estrutura", "commands": [
        {"kind": "create_node", "name": "EP", "data": {"type": "episode"}, "id": "ep"},
        {"kind": "create_node", "name": "S", "parent": "ep", "data": {"type": "scene"}, "id": "sc"},
    ]}});
    let out = rig.ok("APPLY_COMMAND", batch);
    assert_eq!(out["created"].as_array().unwrap().len(), 2);
    rig.ok("UNDO", json!({}));
    let stats = rig.ok("QUERY", json!({"query": {"kind": "stats"}}));
    assert_eq!(stats["nodes"], 0);
    let _ = rig.finish();
}

#[test]
fn project_new_save_open_through_the_bridge() {
    let rig = Rig::start();
    let dir = tempfile::tempdir().unwrap();
    let proj = dir.path().join("p.aiai");
    let p = proj.to_str().unwrap();
    let n = rig.ok("NEW_PROJECT", json!({"name": "Demo", "settings": {"width": 1280, "height": 720}, "path": p}));
    assert_eq!(n["settings"]["width"], 1280);
    assert!(proj.join("project.json").is_file());
    rig.ok("APPLY_COMMAND", create_node("EP01", None, json!({"type": "episode"})));
    let saved = rig.ok("SAVE_PROJECT", json!({}));
    assert_eq!(saved["revision"], 1);
    let text = std::fs::read_to_string(proj.join("project.json")).unwrap();
    assert!(text.contains("EP01") && text.contains("\"width\": 1280"));
    // abrir noutro nome zera o histórico e lê do disco
    rig.ok("NEW_PROJECT", json!({"name": "Outro"}));
    assert_eq!(rig.ok("QUERY", json!({"query": {"kind": "stats"}}))["nodes"], 0);
    let o = rig.ok("OPEN_PROJECT", json!({"path": p}));
    assert_eq!(o["name"], "Demo");
    assert_eq!(rig.ok("QUERY", json!({"query": {"kind": "stats"}}))["nodes"], 1);
    // pasta inexistente é erro, não pânico
    let bad = rig.rpc("OPEN_PROJECT", json!({"path": dir.path().join("nada.aiai").to_str().unwrap()}));
    assert_eq!(bad["success"], false);
    let _ = rig.finish();
}

#[test]
fn http_dialect_rpc_health_and_hardening() {
    let rig = Rig::start();
    let addr = rig.addr();
    let token = rig.token();

    let (st, body) = http_raw(&addr, &format!("GET /health HTTP/1.1\r\nHost: {addr}\r\n\r\n"));
    assert_eq!(st, 200);
    assert_eq!(body["ok"], true);

    let (st, body) = http_rpc(&addr, Some(&token), "", &json!({"id": "h1", "action": "PING"}));
    assert_eq!((st, body["success"].as_bool()), (200, Some(true)), "{body}");
    assert_eq!(body["data"]["pong"], true);

    let (st, body) = http_rpc(&addr, Some(&token), "", &json!({"id": "h2", "action": "APPLY_COMMAND", "params": create_node("EP", None, json!({"type": "episode"}))}));
    assert_eq!(st, 200, "{body}");
    assert_eq!(body["id"], "h2");
    assert_eq!(body["data"]["revision"], 1);

    // sem token / token errado -> 401
    assert_eq!(http_rpc(&addr, None, "", &json!({"action": "PING"})).0, 401);
    assert_eq!(http_rpc(&addr, Some("errado"), "", &json!({"action": "PING"})).0, 401);
    // Origin (navegador) -> 403; Host fora do loopback -> 403
    assert_eq!(http_rpc(&addr, Some(&token), "Origin: http://evil.example\r\n", &json!({"action": "PING"})).0, 403);
    let b = json!({"action": "PING"}).to_string();
    let (st, _) = http_raw(&addr, &format!("POST /rpc HTTP/1.1\r\nHost: evil.example:80\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthorization: Bearer {token}\r\n\r\n{b}", b.len()));
    assert_eq!(st, 403);
    // método/rota/tipo errados
    assert_eq!(http_raw(&addr, &format!("GET /rpc HTTP/1.1\r\nHost: {addr}\r\n\r\n")).0, 405);
    assert_eq!(http_raw(&addr, &format!("GET /nada HTTP/1.1\r\nHost: {addr}\r\n\r\n")).0, 404);
    let (st, _) = http_raw(&addr, &format!("POST /rpc HTTP/1.1\r\nHost: {addr}\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nAuthorization: Bearer {token}\r\n\r\n{{}}"));
    assert_eq!(st, 415);
    // corpo acima do limite -> 413
    let (st, _) = http_raw(&addr, &format!("POST /rpc HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: 99999999\r\n\r\n"));
    assert_eq!(st, 413);

    let session = rig.finish();
    assert_eq!(session.project.nodes.len(), 1, "só a chamada autenticada e válida pode ter aplicado");
}

#[test]
fn ndjson_garbage_is_survivable_and_discovery_follows_the_server_lifetime() {
    let rig = Rig::start();
    let disc = discovery::read(&rig.discovery).unwrap();
    assert_eq!(disc.addr, rig.addr());
    assert_eq!(disc.token, rig.token());
    assert_eq!(disc.pid, std::process::id());
    assert_eq!(disc.protocol, protocol::PROTOCOL_VERSION);
    assert_eq!(disc.token.len(), 64);

    // quadro inválido: erro, conexão segue viva para um quadro válido
    let mut s = TcpStream::connect(rig.addr()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut rd = BufReader::new(s.try_clone().unwrap());
    writeln!(s, "{{ isto nao e json").unwrap();
    let mut line = String::new();
    rd.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["code"], code::INVALID_REQUEST);
    writeln!(s, "{}", json!({"id": 1, "action": "PING", "token": rig.token()})).unwrap();
    line.clear();
    rd.read_line(&mut line).unwrap();
    let r: Value = serde_json::from_str(&line).unwrap();
    assert_eq!((r["id"].as_str(), r["success"].as_bool()), (Some("1"), Some(true)), "id numérico vira texto");
    drop((s, rd));

    let path = rig.discovery.clone();
    let _ = rig.finish();
    assert!(!path.exists(), "a descoberta deve sumir quando o servidor cai");
}

#[test]
fn ping_is_answered_without_the_ui_and_ui_actions_fail_fast_when_nobody_draws() {
    // sem thread de app: PING responde mesmo assim; ação de UI falha rápido (-32002) em vez de pendurar o cliente
    let mut cfg = BridgeConfig::default();
    cfg.server.auth_failure_delay = Duration::from_millis(5);
    let (server, _rx) = BridgeServer::start(cfg).unwrap();
    let call = |action: &str| ndjson(&server.addr(), &json!({"id": "x", "action": action, "token": server.token()}));
    let p = call("PING");
    assert_eq!(p["success"], true);
    assert_eq!(p["data"]["ui_drawing"], false);
    let t = Instant::now();
    let r = call("GET_STATUS");
    assert_eq!(r["code"], code::UI_TIMEOUT, "{r}");
    assert!(r["error"].as_str().unwrap().contains("minimizada"));
    assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
    let m = call("GET_METRICS");
    assert!(m["data"]["requests_total"].as_u64().unwrap() >= 3);
}

// ------------------------------------------------------------------------------------------------ unidades

fn req(action: &str) -> Request {
    serde_json::from_value(json!({ "id": "1", "action": action })).unwrap()
}

#[test]
fn host_request_reaches_the_ui_and_the_reply_comes_back() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    let (backend, rx) = host::channel(4, Arc::new(move || {
        w.fetch_add(1, Ordering::Relaxed);
    }));
    let h = std::thread::spawn(move || backend.call(&req("ALGO")));
    let mut got = Vec::new();
    for _ in 0..400 {
        got = rx.try_recv_all();
        if !got.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].action, "ALGO");
    assert!(woken.load(Ordering::Relaxed) >= 1);
    got.remove(0).reply.send(Ok(json!({"ok": true}))).unwrap();
    assert_eq!(h.join().unwrap().unwrap()["ok"], true);
}

#[test]
fn a_long_frame_right_after_drawing_is_busy_not_hidden() {
    // a UI desenhou um quadro e logo entra num quadro de 3 s (> carência de 2,5 s): o pedido espera e é atendido
    let (backend, rx) = host::channel(4, Arc::new(|| {}));
    let backend = backend.with_timeout(|_| Duration::from_secs(8));
    assert!(rx.try_recv_all().is_empty()); // pulso
    let h = std::thread::spawn(move || backend.call(&req("GET_UI_STATE")));
    std::thread::sleep(Duration::from_millis(3000));
    let mut got = rx.try_recv_all();
    assert_eq!(got.len(), 1, "o pedido não pode ter sido cancelado como 'janela oculta'");
    got.remove(0).reply.send(Ok(json!({"ok": true}))).unwrap();
    assert_eq!(h.join().unwrap().unwrap()["ok"], true);
}

#[test]
fn timeout_cancels_the_request_and_a_late_mutation_is_dropped() {
    let (backend, rx) = host::channel(4, Arc::new(|| {}));
    let backend = backend.with_timeout(|_| Duration::from_millis(60));
    let e = backend.call(&req("APPLY_COMMAND")).unwrap_err();
    assert_eq!(e.code, code::UI_TIMEOUT);
    assert!(rx.try_recv_all().is_empty(), "mutação tardia deve ser descartada");
}

#[test]
fn full_queue_is_busy_and_closed_queue_is_shutting_down() {
    let (backend, rx) = host::channel(1, Arc::new(|| {}));
    let backend = Arc::new(backend.with_timeout(|_| Duration::from_millis(80)));
    let b2 = Arc::clone(&backend);
    let first = std::thread::spawn(move || b2.call(&req("A")));
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(backend.call(&req("B")).unwrap_err().code, code::BUSY);
    let _ = first.join();
    drop(rx);
    assert_eq!(backend.call(&req("C")).unwrap_err().code, code::SHUTTING_DOWN);
}

#[test]
fn ui_idle_tracks_frames() {
    let (backend, rx) = host::channel(4, Arc::new(|| {}));
    assert!(backend.ui_idle().is_none());
    let _ = rx.try_recv_all();
    assert!(backend.ui_idle().unwrap() < Duration::from_secs(1));
}

#[test]
fn protocol_helpers() {
    assert!(ct_eq("abc", "abc") && !ct_eq("abc", "abd") && !ct_eq("abc", "abcd") && ct_eq("", ""));
    let (a, b) = (generate_token().unwrap(), generate_token().unwrap());
    assert!(a.len() == 64 && a.bytes().all(|c| c.is_ascii_hexdigit()) && a != b);
    let err = response_json("x", &Err(BridgeError::new(code::UNAUTHORIZED, "Unauthorized: token")));
    assert_eq!((err["success"].as_bool(), err["code"].as_i64()), (Some(false), Some(-32001)));
    assert!(err.get("data").is_none());
    let r: Request = serde_json::from_str(r#"{"id":7,"action":"PING","params":{"a":1},"token":"t"}"#).unwrap();
    assert_eq!(r.id_string(), "7");
    assert!(serde_json::from_str::<Request>(r#"{"id":"x"}"#).is_err(), "action é obrigatória");
}

#[test]
fn addr_env_parsing_only_accepts_loopback() {
    assert_eq!(parse_loopback_port("127.0.0.1:4555"), Some(4555));
    assert_eq!(parse_loopback_port("localhost:80"), Some(80));
    assert_eq!(parse_loopback_port("0.0.0.0:80"), None);
    assert_eq!(parse_loopback_port("10.0.0.5:80"), None);
    assert_eq!(parse_loopback_port("127.0.0.1"), None);
    assert_eq!(parse_loopback_port("127.0.0.1:porta"), None);
}

#[test]
fn http_parsing_is_strict() {
    let l = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let h = http::parse_head(&l(&["POST /rpc HTTP/1.1", "Host: 127.0.0.1:1", "X-Foo:  bar  "])).unwrap();
    assert_eq!((h.method.as_str(), h.target.as_str(), h.header("x-foo")), ("POST", "/rpc", Some("bar")));
    for bad in ["", "POST /rpc", "post /rpc HTTP/1.1", "POST //evil HTTP/1.1", "POST /rpc HTTP/2"] {
        assert!(http::parse_head(&l(&[bad])).is_err(), "{bad:?}");
    }
    assert!(http::parse_head(&l(&["GET / HTTP/1.1", "Host: a", "host: b"])).is_err(), "Host duplicado");
    assert!(http::parse_head(&l(&["GET / HTTP/1.1", "Host: a", " dobrado"])).is_err());
    assert!(http::host_allowed("127.0.0.1:4000", 4000) && http::host_allowed("LOCALHOST:4000", 4000));
    for bad in ["127.0.0.1", "127.0.0.1:4001", "evil.com:4000", "127.0.0.1.evil.com:4000", "0.0.0.0:4000", ""] {
        assert!(!http::host_allowed(bad, 4000), "{bad:?}");
    }
    assert_eq!(http::bearer("bearer  abc "), Some("abc"));
    assert_eq!(http::bearer("Basic abc"), None);
    assert!(http::is_json_content_type("Application/JSON; charset=utf-8") && !http::is_json_content_type("application/jsonp"));
    let r = String::from_utf8(http::response(401, b"{}", &[("WWW-Authenticate", "Bearer")])).unwrap();
    assert!(r.contains("Connection: close") && r.contains("Cache-Control: no-store") && !r.to_ascii_lowercase().contains("access-control"));
}

#[test]
fn discovery_files_roundtrip_and_only_the_owner_removes_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("bridge.json");
    let d = Discovery { addr: "127.0.0.1:4555".into(), port: 4555, token: "abc123".into(), pid: std::process::id(), started_at: 1, version: "0.1.0".into(), protocol: 1 };
    discovery::write(&path, &d).unwrap();
    assert_eq!(discovery::read(&path).unwrap(), d);
    let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
    assert_eq!(leftovers, 0);
    discovery::remove_if_owner(&path, d.pid + 1, &d.token);
    discovery::remove_if_owner(&path, d.pid, "outro");
    assert!(path.exists());
    // fora do loopback ou sem token: recusado
    discovery::write(&path, &Discovery { addr: "10.0.0.5:1".into(), ..d.clone() }).unwrap();
    assert!(discovery::read(&path).is_err());
    discovery::write(&path, &Discovery { token: String::new(), ..d.clone() }).unwrap();
    assert!(discovery::read(&path).is_err());
    discovery::write(&path, &d).unwrap();
    discovery::remove_if_owner(&path, d.pid, &d.token);
    assert!(!path.exists());
    assert!(discovery::read(&path).is_err());
}

#[test]
fn core_actions_are_pure_functions_over_the_session() {
    let mut s = CoreSession::for_tests("t");
    assert!(handle_core_action(&mut s, "SCREENSHOT", &Value::Null).is_none(), "ações de UI não são do núcleo");
    assert!(core_ops::is_core_action("UNDO") && !core_ops::is_core_action("INPUT"));
    // SAVE sem pasta conhecida é erro claro
    let e = handle_core_action(&mut s, "SAVE_PROJECT", &Value::Null).unwrap().unwrap_err();
    assert_eq!(e.code, code::INVALID_PARAMS);
    // UNDO sem histórico: erro explícito
    assert!(handle_core_action(&mut s, "UNDO", &Value::Null).unwrap().is_err());
    // params que não são objeto
    let e = handle_core_action(&mut s, "QUERY", &json!([1])).unwrap().unwrap_err();
    assert_eq!(e.code, code::INVALID_PARAMS);
    // schema de comandos é consultável (o agente descobre comandos novos sem reiniciar a sessão)
    let schema = handle_core_action(&mut s, "QUERY", &json!({"query": {"kind": "schema", "which": "commands"}})).unwrap().unwrap();
    assert!(schema.to_string().contains("create_node"));
}

// ------------------------------------------------------------------------------------------------ ferramentas (headless)

fn act(s: &mut CoreSession, action: &str, params: Value) -> Value {
    handle_core_action(s, action, &params).unwrap_or_else(|| panic!("{action} não é do núcleo")).unwrap_or_else(|e| panic!("{action}: {e}"))
}

fn act_err(s: &mut CoreSession, action: &str, params: Value) -> BridgeError {
    handle_core_action(s, action, &params).unwrap().unwrap_err()
}

fn made(s: &mut CoreSession, name: &str, parent: Option<&str>, data: Value) -> String {
    let o = act(s, "APPLY_COMMAND", create_node(name, parent, data));
    o["created"][0].as_str().unwrap().to_string()
}

/// Corte 320x180 com linha (vetor), cor (fill -> linha), BG (raster), cada camada com cel 1.
fn tool_doc() -> (CoreSession, String, String, String, String) {
    let mut s = CoreSession::for_tests("tools");
    act(&mut s, "NEW_PROJECT", json!({"name": "ferr", "settings": {"width": 320, "height": 180}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 24}));
    // filhos de baixo para cima: BG primeiro (fundo), depois a cor, depois a linha por cima
    let bg = made(&mut s, "BG", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    let fill = made(&mut s, "A-cor", Some(&cut), json!({"type": "layer", "kind": "fill"}));
    let line = made(&mut s, "A-linha", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    for l in [&line, &fill, &bg] {
        made(&mut s, "c1", Some(l), json!({"type": "cel", "number": 1}));
    }
    act(&mut s, "APPLY_COMMAND", json!({"command": {"kind": "set_props", "id": fill, "data": {"line_refs": [line]}}}));
    (s, cut, line, fill, bg)
}

fn decode(b64: &str) -> image::RgbaImage {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap().to_rgba8()
}

fn px(r: &Value, x: u32, y: u32) -> [u8; 4] {
    decode(r["png_base64"].as_str().unwrap()).get_pixel(x, y).0
}

const WHITE: [u8; 4] = [255, 255, 255, 255];

#[test]
fn tool_actions_round_trip_render_draw_fill_paint_transform_and_undo() {
    let (mut s, cut, line, fill, bg) = tool_doc();
    let direct = |r: f32, g: f32, b: f32| json!({"type": "direct", "rgba": [r, g, b, 1.0]});

    // RENDER vazio: fundo branco opaco, tamanho do projeto, sem avisos, PNG inline (sem out)
    let r = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "background": "#ffffff"}));
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(320), Some(180)));
    let img = decode(r["png_base64"].as_str().unwrap());
    assert_eq!((img.width(), img.height()), (320, 180));
    assert_eq!(px(&r, 10, 10), WHITE);
    assert_eq!(r["warnings"], json!([]));

    // GRADIENT no BG: azul em cima, branco embaixo
    let rev0 = s.project.revision;
    let o = act(&mut s, "GRADIENT", json!({"cut": cut, "frame": 0, "layer": bg, "kind": "linear", "from": [0.0, 0.0], "to": [0.0, 180.0], "stops": [[0.0, "#1030c0ff"], [1.0, "#ffffffff"]]}));
    assert!(o["revision"].as_u64().unwrap() > rev0 && o["can_undo"] == true, "{o}");
    let r = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0}));
    let top = px(&r, 160, 2);
    assert!(top[2] > 150 && top[0] < 80, "topo azul: {top:?}");

    // DRAW_CURVES: retângulo fechado de linha preta (sem preencher)
    let o = act(&mut s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": line, "curves": [
        {"points": [[100, 50], [220, 50], [220, 130], [100, 130]], "width": 4, "closed": true, "geometry": "polyline", "color": direct(0.0, 0.0, 0.0)}]}));
    assert_eq!(o["description"], "Desenhar 1 traço(s)");
    let r = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "layers": [line], "background": "#ffffff"}));
    assert!(px(&r, 160, 50)[0] < 60, "linha de cima desenhada");
    assert_eq!(px(&r, 160, 90), WHITE, "dentro ainda vazio");

    // LINT acha a região; FILL pinta; LINT fica vazio
    let l = act(&mut s, "LINT", json!({"cut": cut, "frame": 0, "layer": fill}));
    let unpainted = l["unpainted"].as_array().unwrap();
    assert_eq!(unpainted.len(), 1, "{l}");
    let pt = unpainted[0]["point"].clone();
    act(&mut s, "FILL", json!({"cut": cut, "frame": 0, "layer": fill, "seeds": [{"x": pt[0], "y": pt[1], "color": direct(1.0, 0.0, 0.0)}]}));
    let l = act(&mut s, "LINT", json!({"cut": cut, "frame": 0, "layer": fill}));
    assert_eq!((l["unpainted"].as_array().unwrap().len(), l["painted"].as_u64()), (0, Some(1)));
    let r = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "background": "#ffffff"}));
    let inside = px(&r, 160, 90);
    assert!(inside[0] > 200 && inside[1] < 60 && inside[2] < 60, "interior vermelho: {inside:?}");

    // PAINT por nome de preset (sem diferenciar maiúsculas) + override, no BG
    let presets = act(&mut s, "BRUSH_PRESETS", json!({}));
    assert!(presets["names"].as_array().unwrap().len() >= 5 && presets["presets"][0]["size"].is_number());
    act(&mut s, "PAINT", json!({"cut": cut, "frame": 0, "layer": bg, "preset": "redondo duro", "preset_overrides": {"size": 20}, "color": direct(0.0, 0.6, 0.0),
        "strokes": [{"points": [[20, 150], [60, 150], [100, 160]]}]}));
    let r = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "layers": [bg], "background": "#ffffff"}));
    let g = px(&r, 40, 150);
    assert!(g[1] > g[0] + 60, "pincelada verde: {g:?}");
    let e = act_err(&mut s, "PAINT", json!({"cut": cut, "frame": 0, "layer": bg, "preset": "nao existe", "color": direct(0.0, 0.0, 0.0), "strokes": []}));
    assert_eq!(e.code, code::INVALID_PARAMS);
    assert!(e.message.contains("Redondo duro"), "lista os presets: {}", e.message);

    // TRANSFORM: desloca o retângulo 40 px para a direita; undo traz de volta; confirmado por render
    let before = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "layers": [line], "background": "#ffffff"}));
    assert_eq!(px(&before, 250, 50), WHITE);
    let o = act(&mut s, "TRANSFORM", json!({"cut": cut, "frame": 0, "layer": line, "deform": {"type": "affine", "translate": [40.0, 0.0], "pivot": [0.0, 0.0]}}));
    assert_eq!(o["can_undo"], true);
    let after = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "layers": [line], "background": "#ffffff"}));
    assert!(px(&after, 250, 50)[0] < 60, "linha deslocada até x=250");
    act(&mut s, "UNDO", json!({}));
    let undone = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "layers": [line], "background": "#ffffff"}));
    assert_eq!(px(&undone, 250, 50), WHITE);
    act(&mut s, "REDO", json!({}));
    let redone = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "layers": [line], "background": "#ffffff"}));
    assert!(px(&redone, 250, 50)[0] < 60);

    // autor padrão agent:claude no log
    let log = act(&mut s, "GET_LOG", json!({"limit": 1000}));
    assert!(log.to_string().contains("agent:claude"));
}

#[test]
fn tool_actions_validate_params_and_render_out_and_size() {
    let (mut s, cut, line, _fill, _bg) = tool_doc();
    for (action, params) in [
        ("RENDER", json!({})),
        ("RENDER", json!({"cut": cut, "background": "vermelho"})),
        ("RENDER", json!({"cut": cut, "frame": 9999})),
        ("RENDER", json!({"cut": "nao-existe"})),
        ("DRAW_CURVES", json!({"cut": cut})),
        ("DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": line, "curves": [{"points": [[1, 2, 3, 4]]}]})),
        ("LINT", json!({"cut": cut})),
        ("FILL", json!([1])),
    ] {
        let e = act_err(&mut s, action, params.clone());
        assert!(e.code == code::INVALID_PARAMS || e.code == code::ACTION_FAILED, "{action} {params}: {e}");
    }
    assert_eq!(act_err(&mut s, "RENDER", json!({"cut": cut, "range": [0, 3]})).code, code::ACTION_FAILED, "range ainda não suportado: erro claro");
    assert_eq!(act_err(&mut s, "RENDER", json!({"cut": cut, "out": "relativo.png"})).code, code::INVALID_PARAMS);
    // if_revision velho: nada muda
    let rev = s.project.revision;
    let e = act_err(&mut s, "GRADIENT", json!({"cut": cut, "frame": 0, "layer": line, "kind": "linear", "from": [0, 0], "to": [1, 1], "stops": [[0.0, "#000000ff"], [1.0, "#ffffffff"]], "if_revision": rev + 5}));
    assert!(e.code == code::STALE_REVISION || e.code == code::INVALID_PARAMS, "{e}");
    assert_eq!(s.project.revision, rev);

    // out + size: grava o arquivo, sem png inline; com inline também devolve o base64
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("sub").join("q.png");
    let r = act(&mut s, "RENDER", json!({"cut": cut, "frame": 0, "size": [160, 90], "out": out.display().to_string(), "background": "#00ff00"}));
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(160), Some(90)));
    assert!(r.get("png_base64").is_none() && r["path"] == json!(out.display().to_string()));
    let img = image::open(&out).unwrap().to_rgba8();
    assert_eq!((img.width(), img.height(), img.get_pixel(5, 5).0), (160, 90, [0, 255, 0, 255]));
    let r = act(&mut s, "RENDER", json!({"cut": cut, "out": out.display().to_string(), "inline": true}));
    assert!(r["png_base64"].is_string());
    // camera:false = canvas inteiro do corte
    let r = act(&mut s, "RENDER", json!({"cut": cut, "camera": false}));
    assert!(r["width"].as_u64().unwrap() > 0);
}

#[test]
fn project_path_follows_the_session_not_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.aiai");
    let mut s = CoreSession::for_tests("p");
    act(&mut s, "NEW_PROJECT", json!({"name": "a", "path": p.display().to_string()}));
    assert_eq!(s.path.as_deref(), Some(p.as_path()));
    made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    // SAVE sem path usa session.path; a variante antiga com path externo continua funcionando
    act(&mut s, "SAVE_PROJECT", json!({}));
    let mut ext = None;
    assert!(handle_core_action_with_path(&mut s, &mut ext, "SAVE_PROJECT", &json!({})).unwrap().is_ok());
    assert_eq!(ext.as_deref(), Some(p.as_path()));
    // novo projeto sem pasta: path some (não grava por cima do anterior)
    act(&mut s, "NEW_PROJECT", json!({"name": "b"}));
    assert!(s.path.is_none());
    assert_eq!(act_err(&mut s, "SAVE_PROJECT", json!({})).code, code::INVALID_PARAMS);
}

// ------------------------------------------------------------------------------------------------ produção (S02 B3)

struct ProdDoc {
    s: CoreSession,
    cut: String,
    ep: String,
    bg: String,
    txt: String,
    line: String,
    parts: String,
    guide: String,
    key: String,
    mouth: String,
}

/// Corte 320x180, 48 quadros: BG/TXT (raster), LINHA/CHAVE/BOCA (vetor), partículas, guia; cada camada com cel 1.
/// A camada CHAVE tem os cels 1 e 2 (chaves da intercalação). Sem timesheet de propósito: um corte com células no
/// timesheet não pode ser editado por set_props nem reaberto do disco (bug do aiai-core: chaves u32 em NodeKind marcado).
fn prod_doc() -> ProdDoc {
    let mut s = CoreSession::for_tests("prod");
    act(&mut s, "NEW_PROJECT", json!({"name": "prod", "settings": {"width": 320, "height": 180}}));
    let ep = made(&mut s, "EP", None, json!({"type": "episode", "number": 1}));
    let sc = made(&mut s, "S", Some(&ep), json!({"type": "scene", "number": 1}));
    let cut = made(&mut s, "C001", Some(&sc), json!({"type": "cut", "number": 1, "duration": 48}));
    let bg = made(&mut s, "BG", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    let txt = made(&mut s, "TXT", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    let line = made(&mut s, "LINHA", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    let key = made(&mut s, "CHAVE", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    let mouth = made(&mut s, "BOCA", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    let parts = made(&mut s, "PETALAS", Some(&cut), json!({"type": "layer", "kind": "particles"}));
    let guide = made(&mut s, "GUIA", Some(&cut), json!({"type": "layer", "kind": "guide"}));
    for l in [&bg, &txt, &line, &key, &guide] {
        made(&mut s, "c1", Some(l), json!({"type": "cel", "number": 1}));
    }
    for n in 1..=9 {
        made(&mut s, &format!("b{n}"), Some(&mouth), json!({"type": "cel", "number": n}));
    }
    made(&mut s, "c2", Some(&key), json!({"type": "cel", "number": 2}));
    ProdDoc { s, cut, ep, bg, txt, line, parts, guide, key, mouth }
}

/// Renderiza só `layers` (fundo transparente por padrão) e devolve a imagem.
fn shot(s: &mut CoreSession, cut: &str, frame: u32, layers: &[&str], extra: Value) -> image::RgbaImage {
    let mut p = json!({"cut": cut, "frame": frame, "layers": layers});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    let r = act(s, "RENDER", p);
    assert_eq!(r["warnings"], json!([]), "render com avisos: {r}");
    decode(r["png_base64"].as_str().unwrap())
}

fn opaque(img: &image::RgbaImage) -> usize {
    img.pixels().filter(|p| p.0[3] > 20).count()
}

fn black() -> Value {
    json!({"type": "direct", "rgba": [0.0, 0.0, 0.0, 1.0]})
}

#[test]
fn production_scenery_and_2d_effects_round_trip() {
    let mut d = prod_doc();
    let (cut, bg, txt, line, parts) = (d.cut.clone(), d.bg.clone(), d.txt.clone(), d.line.clone(), d.parts.clone());
    let rev = d.s.project.revision;

    // SKY: azul no alto, claro no horizonte; entra no undo; autor agent:claude
    let o = act(&mut d.s, "SKY", json!({"cut": cut, "frame": 0, "layer": bg, "sky": {"time_of_day": "day", "horizon_y": 150.0, "clouds": 0.0}}));
    assert!(o["revision"].as_u64().unwrap() > rev && o["can_undo"] == true, "{o}");
    let img = shot(&mut d.s, &cut, 0, &[&bg], json!({}));
    let (top, low) = (img.get_pixel(160, 2).0, img.get_pixel(160, 170).0);
    assert!(top[2] > top[0] && top[3] == 255, "céu azul no topo: {top:?}");
    assert_ne!(top, low, "degradê: topo ≠ horizonte");
    // SKY em camada vetorial é recusado com erro claro
    let e = act_err(&mut d.s, "SKY", json!({"cut": cut, "frame": 0, "layer": line, "sky": {"horizon_y": 100.0}}));
    assert_eq!(e.code, code::INVALID_PARAMS);

    // PATTERN_FILL: textura só dentro da região
    let before = shot(&mut d.s, &cut, 0, &[&bg], json!({}));
    act(&mut d.s, "PATTERN_FILL", json!({"cut": cut, "frame": 0, "layer": bg, "texture": {"spec": {"type": "brick", "brick": [24.0, 10.0], "mortar": 2.0, "variation": 0.3}, "dark": [0.35, 0.1, 0.05], "light": [0.7, 0.3, 0.2]},
        "mapping": {"type": "flat"}, "region": {"type": "rect", "rect": [20, 100, 80, 60]}}));
    let after = shot(&mut d.s, &cut, 0, &[&bg], json!({}));
    assert_ne!(before.get_pixel(50, 130).0, after.get_pixel(50, 130).0, "textura dentro da região");
    assert_eq!(before.get_pixel(200, 30).0, after.get_pixel(200, 30).0, "fora da região não muda");

    // GAG_BG: faixas radiais vermelho/azul (no TXT) cobrem o canvas
    act(&mut d.s, "GAG_BG", json!({"cut": cut, "frame": 0, "layer": txt, "pattern": {"type": "radial_stripes", "center": [160.0, 90.0], "count": 16, "colors": ["#ff0000", "#0000ff"]}}));
    let g = shot(&mut d.s, &cut, 0, &[&txt], json!({}));
    let (mut reds, mut blues) = (0, 0);
    for (x, y) in [(40, 20), (280, 30), (60, 150), (260, 160), (160, 10), (10, 90), (300, 90), (160, 170)] {
        let p = g.get_pixel(x, y).0;
        assert_eq!(p[3], 255, "o fundo de gag cobre o canvas");
        if p[0] > 150 && p[2] < 100 {
            reds += 1;
        }
        if p[2] > 150 && p[0] < 100 {
            blues += 1;
        }
    }
    assert!(reds >= 1 && blues >= 1, "as duas cores aparecem: {reds} vermelhos, {blues} azuis");
    let e = act_err(&mut d.s, "GAG_BG", json!({"cut": cut, "frame": 0, "layer": txt, "pattern": {"type": "flat", "color": "vermelho"}}));
    assert_eq!(e.code, code::INVALID_PARAMS);

    // LINES: linhas de foco pretas na camada vetorial
    assert_eq!(opaque(&shot(&mut d.s, &cut, 0, &[&line], json!({}))), 0);
    act(&mut d.s, "LINES", json!({"cut": cut, "frame": 0, "layer": line, "kind": {"type": "focus", "center": [160.0, 90.0], "clear": [40.0, 30.0]}, "count": 60, "length": [60.0, 200.0], "width": [1.0, 4.0], "color": black(), "seed": 5}));
    let ink = opaque(&shot(&mut d.s, &cut, 0, &[&line], json!({})));
    assert!(ink > 300, "linhas de foco desenhadas: {ink}");

    // TEXT: ドン preenche a camada raster e contorna na vetorial
    let txt2 = made(&mut d.s, "TXT2", Some(&cut), json!({"type": "layer", "kind": "raster"}));
    made(&mut d.s, "c1", Some(&txt2), json!({"type": "cel", "number": 1}));
    let ol = made(&mut d.s, "OUT", Some(&cut), json!({"type": "layer", "kind": "vector"}));
    made(&mut d.s, "c1", Some(&ol), json!({"type": "cel", "number": 1}));
    act(&mut d.s, "TEXT", json!({"cut": cut, "frame": 0, "fill_layer": txt2, "outline_layer": ol, "text": "ドン", "size": 110.0, "center": [160.0, 90.0],
        "color": {"type": "direct", "rgba": [1.0, 0.85, 0.0, 1.0]}, "outline_width": 4.0, "roughness": 2.0}));
    let fill = shot(&mut d.s, &cut, 0, &[&txt2], json!({}));
    let yellow = fill.pixels().filter(|p| p.0[3] > 200 && p.0[0] > 200 && p.0[1] > 150 && p.0[2] < 80).count();
    assert!(yellow > 500, "letras amarelas: {yellow}");
    assert!(opaque(&shot(&mut d.s, &cut, 0, &[&ol], json!({}))) > 200, "contorno desenhado");
    // fonte inexistente e fonte regular
    let e = act_err(&mut d.s, "TEXT", json!({"cut": cut, "frame": 0, "fill_layer": txt2, "text": "x", "size": 40.0, "center": [10.0, 10.0], "color": black(), "font": "C:/nao/existe.otf"}));
    assert_eq!(e.code, code::INVALID_PARAMS);
    act(&mut d.s, "TEXT", json!({"cut": cut, "frame": 0, "fill_layer": txt2, "text": "ABC", "size": 40.0, "center": [60.0, 40.0], "color": black(), "font": "regular"}));

    // PARTICLES: pétalas (preset + overrides); o quadro 12 tem partículas; trocar o sistema substitui o anterior; undo desfaz um passo
    let o = act(&mut d.s, "PARTICLES", json!({"layer": parts, "preset": "petalas", "overrides": {"rate": 60.0}, "seed": 3}));
    assert_eq!(o["canvas"], json!([320, 180]));
    assert_eq!(o["particles"]["rate"], 60.0);
    let f12 = shot(&mut d.s, &cut, 12, &[&parts], json!({}));
    assert!(opaque(&f12) > 20, "partículas no quadro 12: {}", opaque(&f12));
    let rev = d.s.project.revision;
    act(&mut d.s, "PARTICLES", json!({"layer": parts, "preset": "neve", "seed": 1}));
    assert!(d.s.project.revision > rev);
    assert!(opaque(&shot(&mut d.s, &cut, 12, &[&parts], json!({}))) > 5, "neve também aparece");
    act(&mut d.s, "UNDO", json!({}));
    assert_eq!(opaque(&shot(&mut d.s, &cut, 12, &[&parts], json!({}))), opaque(&f12), "undo volta às pétalas (um único passo)");
    // sistema completo + erros claros
    let sys = aiai_anim::particles::preset("chuva", 320.0, 180.0, 9).unwrap();
    act(&mut d.s, "PARTICLES", json!({"layer": parts, "system": sys, "overrides": {"rate": 30.0}}));
    let e = act_err(&mut d.s, "PARTICLES", json!({"layer": parts, "preset": "lava"}));
    assert!(e.code == code::INVALID_PARAMS && e.message.contains("petalas") && e.message.contains("neve"), "{e}");
    assert_eq!(act_err(&mut d.s, "PARTICLES", json!({"layer": parts})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "PARTICLES", json!({"layer": line, "preset": "neve"})).code, code::INVALID_PARAMS, "camada que não é de partículas");
    assert_eq!(act_err(&mut d.s, "PARTICLES", json!({"layer": parts, "preset": "neve", "overrides": {"lifetime": [0.0, 0.0]}})).code, code::INVALID_PARAMS, "validação do sistema");
    // tudo foi pelo autor padrão
    assert!(act(&mut d.s, "GET_LOG", json!({"limit": 1000})).to_string().contains("agent:claude"));
}

#[test]
fn production_guides_character_and_animation() {
    let mut d = prod_doc();
    let (cut, guide, key) = (d.cut.clone(), d.guide.clone(), d.key.clone());

    // GUIDE3D: caixa + manequim vistos por uma câmera viram linhas na camada de guia (aparecem só com include_guides)
    let o = act(&mut d.s, "GUIDE3D", json!({"cut": cut, "frame": 0, "layer": guide,
        "scene": {"objects": [{"shape": {"type": "box", "size": [2.0, 1.0, 1.0]}, "position": [1.0, 0.5, -1.0]}], "mannequins": [{"position": [-0.5, 0.0, 0.0], "pose": "standing"}]},
        "camera": {"position": [2.5, 1.5, 4.0], "target": [0.0, 0.9, 0.0], "focal_mm": 35.0}}));
    assert_eq!(o["can_undo"], true);
    assert_eq!(opaque(&shot(&mut d.s, &cut, 0, &[&guide], json!({}))), 0, "guia não aparece sem include_guides");
    let lines = opaque(&shot(&mut d.s, &cut, 0, &[&guide], json!({"include_guides": true})));
    assert!(lines > 200, "linhas da cena 3D: {lines}");
    let layer_json = act(&mut d.s, "QUERY", json!({"query": {"kind": "node", "id": guide}}));
    assert!(layer_json.to_string().contains("guide3d"), "a cena fica gravada em params.guide3d");
    assert_eq!(act_err(&mut d.s, "GUIDE3D", json!({"cut": cut, "frame": 0, "layer": guide, "scene": {}, "camera": {"position": [0.0, 0.0, 0.0], "target": [0.0, 0.0, 0.0]}})).code, code::INVALID_PARAMS);

    // PERSPECTIVE: pontos de fuga explícitos viram raios + horizonte e voltam no resultado
    let o = act(&mut d.s, "PERSPECTIVE", json!({"cut": cut, "frame": 0, "layer": guide, "points": [[-300.0, 90.0], [620.0, 90.0]], "horizon_y": 90.0, "lines_per_point": 10}));
    assert_eq!(o["perspective"]["points"].as_array().unwrap().len(), 2);
    assert_eq!(o["perspective"]["horizon_y"], 90.0);
    assert!(opaque(&shot(&mut d.s, &cut, 0, &[&guide], json!({"include_guides": true}))) > 100);
    assert_eq!(act_err(&mut d.s, "PERSPECTIVE", json!({"cut": cut, "frame": 0, "layer": guide})).code, code::INVALID_PARAMS, "sem pontos de fuga");

    // MODEL_SHEET + CONSISTENCY
    let ch = made(&mut d.s, "Rin", None, json!({"type": "character"}));
    let o = act(&mut d.s, "MODEL_SHEET", json!({"character": ch, "name": "Rin", "heads_tall": 6.5, "figure_px": 650.0}));
    for k in ["cut", "line_layer", "fill_layer"] {
        assert!(o[k].is_string(), "{k} no resultado: {o}");
    }
    assert_eq!(o["can_undo"], true);
    let sheet_cut = o["cut"].as_str().unwrap().to_string();
    let sheet = act(&mut d.s, "RENDER", json!({"cut": sheet_cut, "camera": false, "include_guides": true, "background": "#ffffff"}));
    assert!(decode(sheet["png_base64"].as_str().unwrap()).pixels().filter(|p| p.0[2] as i32 > p.0[0] as i32 + 50).count() > 1000, "grade e manequim desenhados");
    let ok = act(&mut d.s, "CONSISTENCY", json!({"character": ch, "head_top": 0.0, "chin": 100.0, "eyes": 55.0, "feet": 650.0}));
    assert_eq!(ok["ok"], true, "{ok}");
    let bad = act(&mut d.s, "CONSISTENCY", json!({"character": ch, "head_top": 0.0, "chin": 100.0, "feet": 560.0}));
    assert_eq!(bad["ok"], false);
    let rev = d.s.project.revision;
    act(&mut d.s, "CONSISTENCY", json!({"character": ch, "head_top": 0.0, "chin": 100.0}));
    assert_eq!(d.s.project.revision, rev, "CONSISTENCY só lê");
    assert_eq!(act_err(&mut d.s, "MODEL_SHEET", json!({"character": cut, "heads_tall": 6.0})).code, code::INVALID_PARAMS, "corte não é personagem");

    // VOICE_PROFILE grava `voice` no personagem
    act(&mut d.s, "VOICE_PROFILE", json!({"character": ch, "speaker": 8, "speaker_name": "春日部つむぎ", "params": {"speed_scale": 1.1}}));
    let c = act(&mut d.s, "QUERY", json!({"query": {"kind": "node", "id": ch}}));
    assert!(c.to_string().contains("春日部つむぎ"), "{c}");

    // INBETWEEN: duas chaves desenhadas (cel 1 e cel 2) → 2 intercalações como cels 3 e 4
    let stroke = |dx: f32| json!([{"points": [[40.0 + dx, 40.0], [100.0 + dx, 40.0], [100.0 + dx, 100.0], [40.0 + dx, 100.0]], "width": 3.0, "geometry": "polyline", "closed": true, "color": black()}]);
    act(&mut d.s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": key, "curves": stroke(0.0)}));
    let cel2 = d.s.project.node(&key.as_str().into()).unwrap().children[1].to_string();
    let seg = |dx: f32| json!({"id": "", "points": [{"x": 40.0 + dx, "y": 40.0, "pressure": 1.0}, {"x": 100.0 + dx, "y": 40.0, "pressure": 1.0}, {"x": 100.0 + dx, "y": 100.0, "pressure": 1.0}, {"x": 40.0 + dx, "y": 100.0, "pressure": 1.0}],
        "geometry": "polyline", "width": 3.0, "tip": "marker", "closed": true, "color": {"type": "direct", "rgba": [0.0, 0.0, 0.0, 1.0]}});
    act(&mut d.s, "APPLY_COMMAND", json!({"command": {"kind": "insert_strokes", "cel": cel2, "items": [{"index": 0, "stroke": seg(90.0)}]}}));
    let cels = |s: &CoreSession| s.project.node(&key.as_str().into()).unwrap().children.len();
    assert_eq!(cels(&d.s), 2);
    let o = act(&mut d.s, "INBETWEEN", json!({"cut": cut, "layer": key, "key_a": 1, "key_b": 2, "count": 2}));
    assert_eq!(o["can_undo"], true);
    assert_eq!(cels(&d.s), 4, "dois cels intercalados");
    act(&mut d.s, "UNDO", json!({}));
    assert_eq!(cels(&d.s), 2, "um único passo de undo");
    assert_eq!(act_err(&mut d.s, "INBETWEEN", json!({"cut": cut, "layer": key, "key_a": 1, "key_b": 7, "count": 2})).code, code::INVALID_PARAMS, "chave inexistente");

    // PROPAGATE_FILLS sem chave pintada: erro claro
    let e = act_err(&mut d.s, "PROPAGATE_FILLS", json!({"cut": cut, "fill_layer": d.bg, "key_frame_a": 0, "key_frame_b": 10, "frames": [2, 4]}));
    assert!(e.code == code::INVALID_PARAMS || e.code == code::ACTION_FAILED, "{e}");
}

thread_local! {
    static VOICEVOX_UP: bool = std::net::TcpStream::connect_timeout(&"127.0.0.1:50021".parse().unwrap(), Duration::from_millis(300)).is_ok();
}

#[test]
fn production_audio_sfx_bgm_mix_lipsync_and_voicevox() {
    let mut d = prod_doc();
    let (cut, mouth) = (d.cut.clone(), d.mouth.clone());
    let dir = tempfile::tempdir().unwrap();

    // SFX: asset + clipe posicionado na trilha SFX do corte; determinístico (mesma semente, mesmo blob)
    let a = act(&mut d.s, "SFX", json!({"preset": "impact", "seed": 3, "cut": cut, "start_frame": 6, "gain_db": -3.0}));
    assert!(a["asset"].is_string() && a["placement"].is_string() && a["duration_s"].as_f64().unwrap() > 0.1, "{a}");
    assert_eq!(a["can_undo"], true);
    let blob_of = |s: &CoreSession, id: &str| match &s.project.node(&id.into()).unwrap().data {
        aiai_core::NodeKind::Asset(x) => x.blob.clone().unwrap(),
        _ => panic!(),
    };
    let b = act(&mut d.s, "SFX", json!({"preset": "impact", "seed": 3}));
    assert_eq!(blob_of(&d.s, a["asset"].as_str().unwrap()), blob_of(&d.s, b["asset"].as_str().unwrap()), "mesma semente, mesmo áudio");
    assert_eq!(act_err(&mut d.s, "SFX", json!({"preset": "impact", "cut": cut, "start_frame": 9999})).code, code::INVALID_PARAMS, "fora do corte");

    // BGM com o sintetizador interno, posicionado no corte
    let song = json!({"tempo_bpm": 120.0, "bars": 2, "tail_s": 0.2, "tracks": [{"name": "lead", "program": 73, "notes": [
        {"pitch": 72, "start_beat": 0.0, "length_beats": 2.0}, {"pitch": 76, "start_beat": 2.0, "length_beats": 2.0}, {"pitch": 79, "start_beat": 4.0, "length_beats": 3.0}]}]});
    let m = act(&mut d.s, "BGM", json!({"song": song, "builtin_synth": true, "cut": cut, "start_frame": 0, "gain_db": -6.0}));
    assert_eq!(m["engine"], "builtin");
    assert!(m["duration_s"].as_f64().unwrap() > 3.5, "{m}");

    // MIX_CUT: loudness presente; normalização ao alvo ARIB; WAV gravado
    let plain = act(&mut d.s, "MIX_CUT", json!({"cut": cut}));
    assert!(plain["loudness"]["integrated_lufs"].is_number() && plain["loudness"]["true_peak_dbtp"].is_number(), "{plain}");
    assert!(plain["loudness"].get("momentary_series").is_none() && plain["clips"] == 2);
    let out = dir.path().join("sub").join("mix.wav");
    let rev = d.s.project.revision;
    let norm = act(&mut d.s, "MIX_CUT", json!({"cut": cut, "preset": "arib", "out": out.display().to_string()}));
    assert_eq!(d.s.project.revision, rev, "MIX_CUT não altera o documento");
    let after = norm["normalize"]["after_lufs"].as_f64().unwrap();
    assert!((after - -24.0).abs() < 1.5, "ARIB -24 LKFS: {after}");
    assert!(norm["normalize"]["after_true_peak_dbtp"].as_f64().unwrap() <= -0.9, "{norm}");
    assert!(std::fs::metadata(&out).unwrap().len() > 10_000 && norm["path"] == json!(out.display().to_string()));
    let wav = aiai_audio::wav::read_wav(&std::fs::read(&out).unwrap()).unwrap();
    assert!((wav.duration_s() - 2.0).abs() < 0.05, "48 quadros ≈ 2 s: {}", wav.duration_s());
    // LOUDNESS por clipe, por corte e por arquivo
    for p in [json!({"asset": m["asset"]}), json!({"cut": cut}), json!({"path": out.display().to_string(), "series": true})] {
        let l = act(&mut d.s, "LOUDNESS", p.clone());
        assert!(l["sample_rate"].is_number() && l["duration_s"].as_f64().unwrap() > 0.0, "{p}: {l}");
    }
    assert_eq!(act_err(&mut d.s, "LOUDNESS", json!({})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "MIX_CUT", json!({"cut": cut, "out": "relativo.wav"})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "MIX_CUT", json!({"cut": cut, "out": dir.path().join("x.mp3").display().to_string()})).code, code::INVALID_PARAMS);

    // TTS: motor desligado → erro com o comando para subir; URL que não é loopback é recusada
    let e = act_err(&mut d.s, "TTS_LINE", json!({"text": "おはよう", "speaker": 8, "voicevox_url": "http://127.0.0.1:9"}));
    assert!(e.code == code::ACTION_FAILED && e.message.contains("start.ps1") && e.message.contains("VOICEVOX"), "{e}");
    let e = act_err(&mut d.s, "TTS_LINE", json!({"text": "x", "speaker": 8, "voicevox_url": "http://example.com:50021"}));
    assert_eq!(e.code, code::INVALID_PARAMS);
    // com o motor de pé (se alguém o subiu): fala real, com crédito e moras
    let spoken = if VOICEVOX_UP.with(|u| *u) {
        let t = act(&mut d.s, "TTS_LINE", json!({"text": "おはよう", "speaker": 8, "cut": cut, "start_frame": 0}));
        assert!(t["credit"].as_str().unwrap().starts_with("VOICEVOX:") && t["moras"].as_u64().unwrap() >= 3 && t["duration_s"].as_f64().unwrap() > 0.3, "{t}");
        Some(t)
    } else {
        eprintln!("VOICEVOX não está rodando em 127.0.0.1:50021: parte real do TTS_LINE pulada (suba com tools/voicevox/start.ps1)");
        None
    };

    // LIPSYNC por energia a partir do clipe de SFX (sem moras): exposições na coluna de boca
    let o = act(&mut d.s, "LIPSYNC", json!({"cut": cut, "placement": a["placement"], "mouth_layer": mouth, "mode": "energy",
        "mapping": {"a": 1, "i": 2, "u": 3, "e": 4, "o": 5, "n": 6, "closed": 7, "half": 8, "open": 9}}));
    assert_eq!(o["used_moras"], false);
    assert!(o["exposures"].as_array().unwrap().len() >= 2 && o["first_frame"] == 6, "{o}");
    assert_eq!(act_err(&mut d.s, "LIPSYNC", json!({"cut": cut, "placement": a["placement"], "mouth_layer": mouth, "mode": "moras", "mapping": {"closed": 7}})).code, code::INVALID_PARAMS, "sem moras");

    if let Some(t) = spoken {
        let l = act(&mut d.s, "LIPSYNC", json!({"cut": cut, "placement": t["placement"], "mouth_layer": mouth, "mapping": {"a": 1, "i": 2, "u": 3, "e": 4, "o": 5, "n": 6, "closed": 7, "half": 8, "open": 9}}));
        assert_eq!(l["used_moras"], true);
    }
}

#[test]
fn production_calibration_measure_and_compare() {
    let mut d = prod_doc();
    let (cut, line) = (d.cut.clone(), d.line.clone());
    let dir = tempfile::tempdir().unwrap();
    // linha vertical preta de 4 px em x = 100 (traço "da referência")
    act(&mut d.s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": line, "curves": [{"points": [[100.0, 20.0], [100.0, 160.0]], "width": 4.0, "geometry": "polyline", "tip": "marker", "color": black()}]}));
    let refpng = dir.path().join("ref.png");
    act(&mut d.s, "RENDER", json!({"cut": cut, "frame": 0, "background": "#ffffff", "out": refpng.display().to_string()}));
    let reference = refpng.display().to_string();
    let rev = d.s.project.revision;

    // COMPARE: o render contra si mesmo (PNG salvo) passa; arquivos de diagnóstico saem em out_dir
    let outd = dir.path().join("cmp");
    let c = act(&mut d.s, "COMPARE", json!({"cut": cut, "frame": 0, "reference": reference, "out_dir": outd.display().to_string()}));
    assert_eq!(c["pass"], true, "{c}");
    assert!(c["report"].is_object() && c["files"].as_array().unwrap().len() == 5, "{c}");
    for f in c["files"].as_array().unwrap() {
        assert!(std::path::Path::new(f.as_str().unwrap()).is_file(), "{f}");
    }
    // um desenho diferente reprova
    act(&mut d.s, "LINES", json!({"cut": cut, "frame": 0, "layer": line, "kind": {"type": "focus", "center": [200.0, 90.0], "clear": [10.0, 10.0]}, "count": 80, "length": [80.0, 200.0], "width": [3.0, 6.0], "color": black()}));
    let c = act(&mut d.s, "COMPARE", json!({"cut": cut, "frame": 0, "reference": reference}));
    assert_eq!(c["pass"], false, "{c}");
    assert_eq!(act_err(&mut d.s, "COMPARE", json!({"cut": cut, "frame": 0, "reference": dir.path().join("nao.png").display().to_string()})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "COMPARE", json!({"cut": cut, "frame": 0, "reference": reference, "out_dir": "relativo"})).code, code::INVALID_PARAMS);

    // PROBE_COLOR: branco no fundo, escuro sobre a linha
    let pr = act(&mut d.s, "PROBE_COLOR", json!({"reference": reference, "points": [[10.0, 10.0], [100.0, 90.0]], "radius": 1}));
    assert_eq!(pr["colors"][0]["hex"], "#ffffff");
    assert!(pr["colors"][1]["rgb"][0].as_f64().unwrap() < 0.2, "{pr}");
    assert_eq!((pr["width"].as_u64(), pr["height"].as_u64()), (Some(320), Some(180)));
    assert_eq!(act_err(&mut d.s, "PROBE_COLOR", json!({"reference": reference, "points": []})).code, code::INVALID_PARAMS);

    // MEASURE_LINE: estimativa 2,5 px à esquerda → centro ≈ 100 e largura ≈ 4; não cria nada (decisão 22)
    let rev2 = d.s.project.revision;
    let m = act(&mut d.s, "MEASURE_LINE", json!({"reference": reference, "points": [[97.5, 40.0], [97.5, 80.0], [97.5, 120.0]], "search": 6.0}));
    assert_eq!(m["found"], 3, "{m}");
    for r in m["measures"].as_array().unwrap() {
        assert!((r["center"][0].as_f64().unwrap() - 100.0).abs() < 0.6, "centro: {r}");
        assert!((r["width"].as_f64().unwrap() - 4.0).abs() < 1.3, "largura: {r}");
    }
    assert!(m["note"].as_str().unwrap().contains("decisão 22"));
    let ms = act(&mut d.s, "MEASURE_LINE", json!({"reference": reference, "strokes": [[[97.5, 40.0], [97.5, 80.0]], [[200.0, 40.0], [200.0, 80.0]]]}));
    assert_eq!(ms["strokes"].as_array().unwrap().len(), 2);
    assert_eq!(ms["strokes"][1]["found"], 0, "onde não há linha, não inventa");
    assert_eq!(d.s.project.revision, rev2, "medir não altera o documento");
    assert!(rev2 > rev);
    assert_eq!(act_err(&mut d.s, "MEASURE_LINE", json!({"reference": reference})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "MEASURE_LINE", json!({"reference": reference, "points": [[1.0, 1.0], [2.0, 2.0]], "strokes": [[[1.0, 1.0], [2.0, 2.0]]]})).code, code::INVALID_PARAMS);

    // FILL_SHAPE + MEASURE_EDGE: mancha vermelha chapada (sem antisserrilhado); estimativa 3 px para fora → a borda medida volta ~3 px
    let bg = d.bg.clone();
    let o = act(&mut d.s, "FILL_SHAPE", json!({"cut": cut, "layer": bg, "shapes": [{"points": [[60.0, 50.0], [160.0, 50.0], [160.0, 130.0], [60.0, 130.0]], "geometry": "polyline"}],
        "color": {"type": "direct", "rgba": [1.0, 0.0, 0.0, 1.0]}, "aliased": true}));
    assert_eq!(o["can_undo"], true);
    let blob = shot(&mut d.s, &cut, 0, &[&bg], json!({"background": "#ffffff"}));
    assert_eq!(blob.get_pixel(100, 90).0, [255, 0, 0, 255]);
    assert_eq!(blob.get_pixel(30, 90).0, [255, 255, 255, 255]);
    let ref2 = dir.path().join("massa.png");
    act(&mut d.s, "RENDER", json!({"cut": cut, "layers": [bg], "background": "#ffffff", "out": ref2.display().to_string()}));
    let mut est: Vec<[f32; 2]> = Vec::new();
    for k in 0..=10 {
        est.push([57.0, 60.0 + k as f32 * 6.0]);
    }
    for k in 0..=10 {
        est.push([60.0 + k as f32 * 10.0, 127.0 + 6.0]);
    }
    for k in 0..=10 {
        est.push([163.0, 130.0 - k as f32 * 7.0]);
    }
    let rev3 = d.s.project.revision;
    let m = act(&mut d.s, "MEASURE_EDGE", json!({"reference": ref2.display().to_string(), "points": &est[..11], "search": 8.0}));
    assert!(m["found"].as_u64().unwrap() >= 8, "{m}");
    let mut moved: Vec<f64> = m["measures"].as_array().unwrap().iter().filter(|r| r["strength"].as_f64().unwrap() > 0.0).map(|r| r["moved"].as_f64().unwrap()).collect();
    moved.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = moved[moved.len() / 2];
    assert!((med - 3.0).abs() < 1.2, "borda a ~3 px da estimativa: mediana {med} ({moved:?})");
    assert!(m["note"].as_str().unwrap().contains("decisão 22"));
    assert_eq!(d.s.project.revision, rev3, "MEASURE_EDGE só mede");
    assert_eq!(act_err(&mut d.s, "MEASURE_EDGE", json!({"reference": ref2.display().to_string(), "points": [[1.0, 1.0]]})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "FILL_SHAPE", json!({"cut": cut, "layer": bg, "shapes": [{"points": [[1.0, 1.0], [2.0, 2.0]]}], "color": black()})).code, code::INVALID_PARAMS, "contorno com menos de 3 pontos");

    // as medições valem numa sessão vazia (sem projeto aberto)
    let mut empty = core_ops::empty_session();
    let pr2 = act(&mut empty, "PROBE_COLOR", json!({"reference": reference, "points": [[10.0, 10.0]]}));
    assert_eq!(pr2["colors"][0]["hex"], "#ffffff");
}

#[test]
fn production_export_video_frames_audio_subtitles_and_png_sequence() {
    if std::process::Command::new(aiai_media::ffmpeg()).arg("-version").output().is_err() {
        eprintln!("ffmpeg não encontrado: teste pulado");
        return;
    }
    let mut d = prod_doc();
    let (cut, bg, ep) = (d.cut.clone(), d.bg.clone(), d.ep.clone());
    let dir = tempfile::tempdir().unwrap();
    act(&mut d.s, "GAG_BG", json!({"cut": cut, "frame": 0, "layer": bg, "pattern": {"type": "radial_stripes", "center": [160.0, 90.0], "count": 20, "colors": ["#fff3b0", "#ffd23f"]}}));
    act(&mut d.s, "SFX", json!({"preset": "impact", "seed": 1, "cut": cut, "start_frame": 0}));
    let rev = d.s.project.revision;

    // 24 quadros com áudio: o ffprobe confirma 24 quadros, tamanho do projeto e AAC
    let out = dir.path().join("v").join("gag.mp4");
    let r = act(&mut d.s, "EXPORT_VIDEO", json!({"cut": cut, "start": 0, "end": 24, "out": out.display().to_string(), "crf": 23, "preset": "ebu"}));
    assert_eq!((r["frames"].as_u64(), r["expected_frames"].as_u64()), (Some(24), Some(24)), "{r}");
    assert_eq!(r["probe"]["frames"], 24, "{r}");
    assert_eq!((r["probe"]["width"].as_u64(), r["probe"]["height"].as_u64()), (Some(320), Some(180)));
    assert_eq!((r["probe"]["video_codec"].as_str(), r["probe"]["audio_codec"].as_str()), (Some("h264"), Some("aac")), "{r}");
    assert!(r["audio"]["normalize"]["after_lufs"].is_number() && r["audio"]["loudness"]["integrated_lufs"].is_number(), "{r}");
    assert!(std::fs::metadata(&out).unwrap().len() > 2000);
    assert_eq!(d.s.project.revision, rev, "exportar não altera o documento");
    // o primeiro quadro do vídeo é o fundo de gag (amarelo), não vazio
    let f0 = aiai_media::extract_frame(&out, 0.0).unwrap();
    let c = f0.get(5, 5);
    assert!(c[0] > 0.8 && c[2] < 0.9, "quadro amarelado: {c:?}");

    // episódio inteiro (48 quadros) com legenda queimada; sem áudio
    let out2 = dir.path().join("ep.mp4");
    let r = act(&mut d.s, "EXPORT_VIDEO", json!({"episode": ep, "out": out2.display().to_string(), "audio": false, "subtitles": [{"start": 0.2, "end": 1.5, "text": "Teste"}]}));
    assert_eq!((r["frames"].as_u64(), r["probe"]["frames"].as_u64()), (Some(48), Some(48)), "{r}");
    assert_eq!(r["subtitles_burned"], true);
    assert!(r["probe"]["audio_codec"].is_null());
    assert_eq!(r["segments"].as_array().unwrap().len(), 1);

    // documento de edição: o corte inteiro + 1 s de créditos; EXPORT_VIDEO {edit} desenha tudo e o ffprobe confirma 48 quadros
    let e = act(&mut d.s, "EDIT_SET", json!({"name": "Ep1", "clips": [{"cut": cut, "out_frame": 24}], "subtitles": [{"start_frame": 2, "end_frame": 20, "text": "Teste"}], "credits": ["Fim"], "credits_seconds": 1.0}));
    assert_eq!(e["timeline"]["total_frames"], 48, "{e}");
    let edit_id = e["edit"].as_str().unwrap().to_string();
    let out3 = dir.path().join("edit.mp4");
    let r = act(&mut d.s, "EXPORT_VIDEO", json!({"edit": edit_id, "out": out3.display().to_string()}));
    assert_eq!((r["frames"].as_u64(), r["probe"]["frames"].as_u64()), (Some(48), Some(48)), "{r}");
    assert!(r["probe"]["audio_codec"].is_string(), "o áudio do corte entra na edição: {r}");
    assert_eq!(r["segments"][0]["edit"], json!(edit_id));
    // trocar o documento (mesmo id) e erros
    let e2 = act(&mut d.s, "EDIT_SET", json!({"id": edit_id, "clips": [{"cut": cut, "in_frame": 0, "out_frame": 10}]}));
    assert_eq!(e2["timeline"]["total_frames"], 10);
    assert_eq!(act_err(&mut d.s, "EDIT_SET", json!({"clips": [{"cut": "nao-existe"}]})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "EDIT_SET", json!({"id": cut, "clips": [{"cut": cut}]})).code, code::INVALID_PARAMS, "id que não é edição");
    assert_eq!(act_err(&mut d.s, "EXPORT_VIDEO", json!({"edit": edit_id, "cut": cut, "out": out3.display().to_string()})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "EXPORT_VIDEO", json!({"edit": cut, "out": out3.display().to_string()})).code, code::INVALID_PARAMS);

    // sequência PNG
    let seq = dir.path().join("png").join("q_%05d.png");
    let r = act(&mut d.s, "EXPORT_VIDEO", json!({"cut": cut, "start": 5, "end": 8, "codec": "png_sequence", "out": seq.display().to_string()}));
    assert_eq!(r["frames"], 3);
    assert!(dir.path().join("png").join("q_00003.png").is_file() && !dir.path().join("png").join("q_00004.png").exists());

    // validações
    for p in [
        json!({"out": out.display().to_string()}),
        json!({"cut": cut, "episode": ep, "out": out.display().to_string()}),
        json!({"cut": cut, "out": "relativo.mp4"}),
        json!({"cut": cut, "out": dir.path().join("x.avi").display().to_string()}),
        json!({"cut": cut, "out": dir.path().join("x.mp4").display().to_string(), "start": 30, "end": 20}),
        json!({"cut": cut, "out": dir.path().join("x.mp4").display().to_string(), "end": 9999}),
        json!({"cut": cut, "out": dir.path().join("x.mov").display().to_string(), "codec": "png_sequence"}),
        json!({"cut": cut, "out": dir.path().join("x.mp4").display().to_string(), "background": "azul"}),
        json!({"cut": cut, "out": dir.path().join("x.mp4").display().to_string(), "srt": dir.path().join("nao.srt").display().to_string()}),
    ] {
        let e = act_err(&mut d.s, "EXPORT_VIDEO", p.clone());
        assert_eq!(e.code, code::INVALID_PARAMS, "{p}: {e}");
    }
    assert!(!dir.path().join("x.mp4").exists());
}

#[test]
fn preprod_script_storyboard_sheet_animatic_and_cuts() {
    let mut d = prod_doc();
    let ep = d.ep.clone();
    let dir = tempfile::tempdir().unwrap();

    // SCRIPT: cria, edita por id (um passo de undo), estima e grava o tempo, exporta Fountain
    let o = act(&mut d.s, "SCRIPT", json!({"op": "create", "name": "Roteiro", "title": "Teste", "scenes": [{"heading": "EXT. PONTE", "elements": [
        {"kind": "action", "text": "雨が降り始める。", "translation": "Começa a chover."},
        {"kind": "dialogue", "speaker": "ハル", "text": "傘、持ってる？", "translation": "Trouxe guarda-chuva?"}]}]}));
    let doc = o["doc"].as_str().unwrap().to_string();
    let scene = o["script"]["scenes"][0]["id"].as_str().unwrap().to_string();
    let line = o["script"]["scenes"][0]["elements"][1]["id"].as_str().unwrap().to_string();
    assert_eq!(o["can_undo"], true);
    let o = act(&mut d.s, "SCRIPT", json!({"op": "edit", "doc": doc, "ops": [{"op": "insert_element", "scene": scene, "element": {"kind": "dialogue", "speaker": "ミナ", "text": "ないよ。", "translation": "Não."}}]}));
    assert_eq!(o["created"].as_array().unwrap().len(), 1);
    let t = act(&mut d.s, "SCRIPT", json!({"op": "timing", "doc": doc}));
    assert!(t["total_seconds"].as_f64().unwrap() > 1.0, "{t}");
    act(&mut d.s, "SCRIPT", json!({"op": "apply_timing", "doc": doc}));
    let r = act(&mut d.s, "SCRIPT", json!({"op": "read", "doc": doc}));
    assert!(r["script"]["scenes"][0]["elements"][1]["est_seconds"].as_f64().unwrap() > 0.0);
    let f = act(&mut d.s, "SCRIPT", json!({"op": "to_fountain", "doc": doc}));
    assert!(f["fountain"].as_str().unwrap().contains("EXT. PONTE"));
    assert_eq!(act_err(&mut d.s, "SCRIPT", json!({"op": "edit", "doc": doc, "ops": [{"op": "delete_scene", "scene": "nao-existe"}]})).code, code::INVALID_PARAMS);

    // BEATS
    let b = act(&mut d.s, "BEATS", json!({"op": "create", "name": "Beats"}));
    let bd = b["doc"].as_str().unwrap().to_string();
    act(&mut d.s, "BEATS", json!({"op": "edit", "doc": bd, "ops": [{"op": "add_act", "name": "Início"}, {"op": "add_beat", "act": 0, "title": "Chuva", "target_seconds": 8.0}]}));
    assert_eq!(act(&mut d.s, "BEATS", json!({"op": "read", "doc": bd}))["total_target_seconds"], 8.0);

    // STORYBOARD: dois painéis; o primeiro desenhado no corte do painel, com pan de câmera
    let o = act(&mut d.s, "STORYBOARD", json!({"op": "create", "name": "E-conte", "title": "Teste"}));
    let sb = o["storyboard"].as_str().unwrap().to_string();
    let p1 = act(&mut d.s, "STORYBOARD", json!({"op": "panel_add", "storyboard": sb, "spec": {"shot": "wide", "action": "Ponte na chuva", "seconds": 1.0, "dialogue_refs": [line], "dialogue_text": "傘、持ってる？", "scene": scene,
        "camera_move": {"kind": "pan", "from": {"center": [0.4, 0.5], "zoom": 1.2}, "to": {"center": [0.6, 0.5], "zoom": 1.2}}}}));
    let (pcut, player) = (p1["drawing"]["cut"].as_str().unwrap().to_string(), p1["drawing"]["layer"].as_str().unwrap().to_string());
    act(&mut d.s, "DRAW_CURVES", json!({"cut": pcut, "frame": 0, "layer": player, "curves": [{"points": [[40.0, 200.0], [240.0, 80.0], [440.0, 200.0]], "width": 4.0, "color": black()}]}));
    let p2 = act(&mut d.s, "STORYBOARD", json!({"op": "panel_add", "storyboard": sb, "spec": {"shot": "close", "action": "Haru olha", "seconds": 0.5, "scene": scene}}));
    let pid2 = p2["panel"].as_str().unwrap().to_string();
    act(&mut d.s, "STORYBOARD", json!({"op": "panel_update", "storyboard": sb, "panel": pid2, "patch": {"seconds": 0.75}}));
    let rd = act(&mut d.s, "STORYBOARD", json!({"op": "read", "storyboard": sb}));
    assert_eq!(rd["total_seconds"], 1.75);
    assert_eq!(act(&mut d.s, "STORYBOARD", json!({"op": "panel_drawing", "storyboard": sb, "panel": p1["panel"]}))["layer"], json!(player));
    let sheet = dir.path().join("econte.png");
    let o = act(&mut d.s, "STORYBOARD", json!({"op": "sheet", "storyboard": sb, "out": sheet.display().to_string()}));
    assert!(sheet.exists() && o["width"].as_u64().unwrap() > 100, "{o}");
    assert_eq!(act_err(&mut d.s, "STORYBOARD", json!({"op": "sheet", "storyboard": sb, "out": "relativo.png"})).code, code::INVALID_PARAMS);

    // ANIMATIC (precisa do ffmpeg)
    if std::process::Command::new(aiai_media::ffmpeg()).arg("-version").output().is_ok() {
        let mp4 = dir.path().join("animatic.mp4");
        let o = act(&mut d.s, "ANIMATIC", json!({"storyboard": sb, "out": mp4.display().to_string(), "width": 320, "height": 180, "captions": true, "cut_numbers": true}));
        assert!(mp4.exists(), "{o}");
        assert_eq!(o["panels"].as_array().unwrap().len(), 2);
    }

    // CUTS_FROM_STORYBOARD: uma cena (mesma cena do roteiro), dois cortes; o do pan tem chaves de câmera e camada de referência
    let o = act(&mut d.s, "CUTS_FROM_STORYBOARD", json!({"storyboard": sb, "episode": ep, "scene_per_panel_group": true, "script": doc}));
    assert_eq!(o["scenes"].as_array().unwrap().len(), 1);
    let cuts = o["cuts"].as_array().unwrap();
    assert_eq!(cuts.len(), 2);
    let c1 = act(&mut d.s, "QUERY", json!({"query": {"kind": "node", "id": cuts[0]["cut"]}}));
    assert!(c1.to_string().contains("center_x"), "pan vira chaves de câmera: {c1}");
    let undo = act(&mut d.s, "UNDO", json!({}));
    assert_eq!(undo["can_undo"], true, "cortes do storyboard = um passo de undo");
}

#[test]
fn review_cut_table_stages_lint_corrections_and_retakes() {
    let (mut s, cut, line, fill, _bg) = tool_doc();
    let dir = tempfile::tempdir().unwrap();
    // círculo fechado na linha: uma região sem cor até o balde
    act(&mut s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": line, "curves": [{"points": [[100.0, 60.0], [160.0, 40.0], [220.0, 60.0], [220.0, 120.0], [160.0, 140.0], [100.0, 120.0]], "closed": true, "width": 3.0, "color": black()}]}));
    let l = act(&mut s, "PRODUCTION", json!({"op": "lint_cut", "cut": cut}));
    assert!(l["fill_layers"][0]["frames_checked"].as_u64().unwrap() > 0, "{l}");
    assert!(l["unpainted_regions"].as_u64().unwrap() >= 1 && l["clean"] == false, "{l}");
    act(&mut s, "FILL", json!({"cut": cut, "frame": 0, "layer": fill, "seeds": [{"x": 160, "y": 90, "color": {"type": "direct", "rgba": [1.0, 0.8, 0.7, 1.0]}}]}));
    let l2 = act(&mut s, "PRODUCTION", json!({"op": "lint_cut", "cut": cut}));
    assert!(l2["unpainted_regions"].as_u64().unwrap() < l["unpainted_regions"].as_u64().unwrap(), "{l2}");

    // etapas: manual e sugestão automática
    act(&mut s, "PRODUCTION", json!({"op": "set_stage", "cut": cut, "stage": "layout", "status": "approved"}));
    let sg = act(&mut s, "PRODUCTION", json!({"op": "suggest_stages", "cut": cut, "apply": true}));
    assert!(sg["suggestions"].is_array(), "{sg}");
    let t = act(&mut s, "PRODUCTION", json!({"op": "cut_table"}));
    assert_eq!(t["cuts"][0]["stages"]["layout"], "approved", "{t}");

    // correção em vermelho + retake + comparação depois de mudar o quadro
    let c = act(&mut s, "CORRECTION", json!({"op": "add", "cut": cut, "frame": 0, "note": "fechar o traço", "strokes": [{"points": [[90.0, 50.0], [230.0, 50.0]], "width": 3.0}]}));
    let cid = c["correction"].as_str().unwrap().to_string();
    assert_eq!(act(&mut s, "PRODUCTION", json!({"op": "cut_table"}))["cuts"][0]["open_corrections"], 1);
    let r = act(&mut s, "RETAKE", json!({"op": "create", "cut": cut, "frame": 0, "reason": "cor da pele"}));
    let rid = r["retake"].as_str().unwrap().to_string();
    act(&mut s, "FILL", json!({"cut": cut, "frame": 0, "layer": fill, "seeds": [{"x": 160, "y": 90, "color": {"type": "direct", "rgba": [0.2, 0.4, 1.0, 1.0]}}]}));
    let sheet = dir.path().join("retake.png");
    let cmp = act(&mut s, "RETAKE", json!({"op": "compare", "retake": rid, "out": sheet.display().to_string()}));
    assert!(cmp["changed_fraction"].as_f64().unwrap() > 0.01 && sheet.exists(), "{cmp}");
    act(&mut s, "CORRECTION", json!({"op": "resolve", "correction": cid, "retake": rid}));
    act(&mut s, "RETAKE", json!({"op": "resolve", "retake": rid}));
    let t = act(&mut s, "PRODUCTION", json!({"op": "cut_table"}));
    assert_eq!(t["cuts"][0]["open_corrections"], 0);
    assert_eq!(t["cuts"][0]["open_retakes"], 0);

    let pv = act(&mut s, "PRODUCTION", json!({"op": "provenance", "id": fill, "history": 5}));
    assert!(pv["history"].as_array().unwrap().len() >= 1, "{pv}");
    let md = act(&mut s, "PRODUCTION", json!({"op": "report"}));
    assert!(md["markdown"].as_str().unwrap().contains("Relatório de produção"));
    assert_eq!(act_err(&mut s, "RETAKE", json!({"op": "compare", "retake": "nao-existe"})).code, code::INVALID_PARAMS);
}

/// Coluna média dos pixels escuros (traço vertical) de uma imagem.
fn dark_column(img: &image::RgbaImage) -> f32 {
    let (mut sum, mut n) = (0.0f32, 0.0f32);
    for (x, _, p) in img.enumerate_pixels() {
        if p.0[3] > 128 && p.0[0] < 80 {
            sum += x as f32;
            n += 1.0;
        }
    }
    assert!(n > 0.0, "sem traço");
    sum / n
}

#[test]
fn satsuei_camera_multiplane_effects_and_grade() {
    let mut d = prod_doc();
    let (cut, near, far, bg) = (d.cut.clone(), d.line.clone(), d.key.clone(), d.bg.clone());
    for l in [&near, &far] {
        act(&mut d.s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": l, "curves": [{"points": [[160.0, 20.0], [160.0, 160.0]], "width": 4.0, "geometry": "polyline", "tip": "marker", "color": black()}]}));
    }
    // multiplano: o plano distante (depth 3) anda 1/4 do pan da câmera
    let o = act(&mut d.s, "LAYER_MOTION", json!({"op": "set", "layer": far, "depth": 3.0}));
    assert_eq!(o["layer_motion"]["depth"], 3.0);
    let o = act(&mut d.s, "CAMERA", json!({"op": "move", "cut": cut, "from": {"center": [160.0, 90.0], "zoom": 1.0}, "to": {"center": [200.0, 90.0], "zoom": 1.0}, "start": 0, "end": 40}));
    assert_eq!(o["can_undo"], true);
    let x_near0 = dark_column(&shot(&mut d.s, &cut, 0, &[&near], json!({})));
    let x_near = dark_column(&shot(&mut d.s, &cut, 47, &[&near], json!({})));
    let x_far = dark_column(&shot(&mut d.s, &cut, 47, &[&far], json!({})));
    assert!((x_near0 - x_near - 40.0).abs() < 1.5, "plano da câmera anda o pan inteiro: {x_near0} → {x_near}");
    assert!((x_near0 - x_far - 10.0).abs() < 1.5, "plano distante anda 1/4: {x_near0} → {x_far}");
    let r = act(&mut d.s, "CAMERA", json!({"op": "read", "cut": cut}));
    assert!(r["camera"]["keys"]["center_x"].is_array(), "{r}");
    assert_eq!(act_err(&mut d.s, "CAMERA", json!({"op": "keys", "cut": cut, "property": "foo", "keys": []})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "CAMERA", json!({"op": "move", "cut": cut, "from": {"center": [0.0, 0.0]}, "to": {"center": [1.0, 1.0]}, "start": 10, "end": 99})).code, code::INVALID_PARAMS);
    act(&mut d.s, "CAMERA", json!({"op": "shake", "cut": cut, "amplitude": 6.0, "start": 2, "end": 10}));
    act(&mut d.s, "CAMERA", json!({"op": "clear_keys", "cut": cut}));
    // slide de camada
    act(&mut d.s, "LAYER_MOTION", json!({"op": "slide", "layer": near, "from": [0.0, 0.0], "to": [30.0, 0.0], "start": 0, "end": 10}));
    let xs = dark_column(&shot(&mut d.s, &cut, 10, &[&near], json!({})));
    assert!((xs - x_near0 - 30.0).abs() < 1.5, "slide de 30 px: {x_near0} → {xs}");

    // efeitos na camada: contorno vermelho aparece; lista/remove
    let o = act(&mut d.s, "EFFECTS", json!({"op": "add", "layer": far, "effect": {"type": "outline", "width": 3.0, "color": [1.0, 0.0, 0.0, 1.0]}}));
    assert_eq!(o["effects"][0]["name"], "outline", "{o}");
    let img = shot(&mut d.s, &cut, 0, &[&far], json!({}));
    assert!(img.pixels().any(|p| p.0[0] > 200 && p.0[1] < 60 && p.0[3] > 200), "contorno vermelho");
    assert_eq!(act(&mut d.s, "EFFECTS", json!({"op": "list", "layer": far}))["effects"].as_array().unwrap().len(), 1);
    act(&mut d.s, "EFFECTS", json!({"op": "remove", "layer": far, "index": 0}));
    assert_eq!(act_err(&mut d.s, "EFFECTS", json!({"op": "remove", "layer": far, "index": 0})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "EFFECTS", json!({"op": "add", "layer": far, "effect": {"type": "nao_existe"}})).code, code::INVALID_PARAMS);

    // grade: camada de efeito no topo, esquenta o fundo cinza; segunda chamada reaproveita a camada
    act(&mut d.s, "GAG_BG", json!({"cut": cut, "frame": 0, "layer": bg, "pattern": {"type": "flat", "color": "#808080"}}));
    let full = |s: &mut CoreSession| decode(act(s, "RENDER", json!({"cut": cut, "frame": 0}))["png_base64"].as_str().unwrap()).get_pixel(5, 5).0;
    let before = full(&mut d.s);
    let g = act(&mut d.s, "GRADE", json!({"cut": cut, "effects": [{"effect": {"type": "grade", "gain": [1.3, 1.0, 0.7]}}]}));
    let gl = g["layer"].as_str().unwrap().to_string();
    let after = full(&mut d.s);
    assert!(after[0] > before[0] + 10 && after[2] + 10 < before[2], "grade quente: {before:?} → {after:?}");
    let g2 = act(&mut d.s, "GRADE", json!({"cut": cut, "effects": []}));
    assert_eq!(g2["layer"], json!(gl), "reaproveita a camada");
    let cut_node = act(&mut d.s, "QUERY", json!({"query": {"kind": "node", "id": cut}}));
    assert_eq!(cut_node["children"].as_array().unwrap().last().unwrap(), &json!(gl), "grade no topo: {cut_node}");
}

#[test]
fn preview_sheet_edit_frames_mix_and_sidecar_srt() {
    let mut d = prod_doc();
    let (cut, line) = (d.cut.clone(), d.line.clone());
    let dir = tempfile::tempdir().unwrap();
    act(&mut d.s, "DRAW_CURVES", json!({"cut": cut, "frame": 0, "layer": line, "curves": [{"points": [[100.0, 20.0], [100.0, 160.0]], "width": 4.0, "color": black()}]}));
    act(&mut d.s, "CAMERA", json!({"op": "move", "cut": cut, "from": {"center": [160.0, 90.0]}, "to": {"center": [220.0, 90.0]}}));

    // folha de contato: faixa com passo; a imagem tem uma miniatura por quadro
    let out = dir.path().join("folha.png");
    let o = act(&mut d.s, "RENDER_SHEET", json!({"cut": cut, "start": 0, "end": 47, "step": 8, "columns": 3, "thumb_width": 160, "out": out.display().to_string()}));
    assert_eq!(o["frames"], json!([0, 8, 16, 24, 32, 40]), "{o}");
    let img = image::open(&out).unwrap();
    assert!(img.width() >= 3 * 160 && img.height() > 2 * 90, "{}x{}", img.width(), img.height());
    assert_eq!(act_err(&mut d.s, "RENDER_SHEET", json!({"cut": cut, "frames": [99], "out": out.display().to_string()})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "RENDER_SHEET", json!({"cut": cut, "frames": vec![0; 49], "out": out.display().to_string()})).code, code::INVALID_PARAMS, "mais de 48 quadros");

    // prévia da edição com legenda queimada (difere da mesma prévia sem legenda)
    act(&mut d.s, "SFX", json!({"preset": "impact", "seed": 3, "cut": cut, "start_frame": 6}));
    let e = act(&mut d.s, "EDIT_SET", json!({"name": "Ep", "clips": [{"cut": cut}], "subtitles": [{"start_frame": 2, "end_frame": 30, "text": "Olá, mundo"}]}));
    let edit = e["edit"].as_str().unwrap().to_string();
    let (a, b) = (dir.path().join("com.png"), dir.path().join("sem.png"));
    let o = act(&mut d.s, "EDIT_PREVIEW", json!({"edit": edit, "frames": [10], "out": a.display().to_string()}));
    assert_eq!(o["total_frames"], 48);
    act(&mut d.s, "EDIT_PREVIEW", json!({"edit": edit, "frames": [10], "burn_subtitles": false, "out": b.display().to_string()}));
    assert_ne!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap(), "legenda aparece na prévia");
    let o = act(&mut d.s, "EDIT_PREVIEW", json!({"edit": edit, "seconds": [0.5, 1.0], "out": a.display().to_string()}));
    assert_eq!(o["frames"].as_array().unwrap().len(), 2);

    // mixagem da edição em WAV
    let wav = dir.path().join("mix.wav");
    let o = act(&mut d.s, "EDIT_MIX", json!({"edit": edit, "out": wav.display().to_string(), "preset": "arib"}));
    assert!(wav.exists() && (o["duration_s"].as_f64().unwrap() - 2.0).abs() < 0.1, "{o}");
    assert_eq!(act_err(&mut d.s, "EDIT_MIX", json!({"edit": edit, "out": dir.path().join("x.mp3").display().to_string()})).code, code::INVALID_PARAMS);
    assert_eq!(act_err(&mut d.s, "TTS_REGENERATE", json!({"asset": cut})).code, code::INVALID_PARAMS);

    // .srt de entrega ao lado do MP4 (precisa do ffmpeg)
    if std::process::Command::new(aiai_media::ffmpeg()).arg("-version").output().is_ok() {
        let mp4 = dir.path().join("ep.mp4");
        let o = act(&mut d.s, "EXPORT_VIDEO", json!({"edit": edit, "out": mp4.display().to_string(), "sidecar_srt": true}));
        let srt = std::fs::read_to_string(dir.path().join("ep.srt")).unwrap_or_else(|_| panic!("{o}"));
        assert!(srt.contains("Olá, mundo") && srt.contains("-->"), "{srt}");
    }
}

// ------------------------------------------------------------------------------------------------ chaves desconhecidas

#[test]
fn unknown_param_keys_are_errors_naming_the_key() {
    use aiai_bridge::{core_tools, preprod_tools, preview_tools, production_tools, review_tools, satsuei_tools};
    let (mut s, cut, line, fill, _bg) = tool_doc();

    // o caso que escondeu uma comparação errada: `only_layers` (a chave é `layers`) renderizava tudo sem aviso
    let e = act_err(&mut s, "RENDER", json!({"cut": cut, "only_layers": [line]}));
    assert_eq!(e.code, code::INVALID_PARAMS);
    assert!(e.message.contains("'only_layers'") && e.message.contains("→ 'layers'?") && e.message.contains("válidos: backend, background, camera, cut, "), "{}", e.message);
    act(&mut s, "RENDER", json!({"cut": cut, "layers": [line]}));

    // struct de requisição: chaves do schema + author/if_revision (lidas por `apply`) nas ações que mudam o documento
    let seed = json!([{"x": 10, "y": 10, "color": {"type": "direct", "rgba": [1.0, 0.0, 0.0, 1.0]}}]);
    let e = act_err(&mut s, "FILL", json!({"cut": cut, "layer": fill, "frame": 0, "seeds": seed, "gapp": 3}));
    assert!(e.message.starts_with("FILL: parâmetro desconhecido 'gapp' (quis dizer 'gapp' → 'gap'?)"), "{}", e.message);
    let rev = act(&mut s, "PING", json!({}))["revision"].clone();
    act(&mut s, "FILL", json!({"cut": cut, "layer": fill, "frame": 0, "seeds": seed, "author": "agent:teste", "if_revision": rev}));
    // ação que não muda o documento não aceita `author`
    assert!(act_err(&mut s, "LINT", json!({"cut": cut, "layer": fill, "author": "x"})).message.contains("'author'"));
    // PAINT lê `preset_overrides` por fora da struct
    act(&mut s, "BRUSH_PRESETS", json!({}));
    assert!(act_err(&mut s, "BRUSH_PRESETS", json!({"name": "x"})).message.contains("não recebe parâmetros"));

    // enum com `op`: só as chaves da variante pedida
    act(&mut s, "SCRIPT", json!({"op": "create", "name": "Roteiro"}));
    let e = act_err(&mut s, "SCRIPT", json!({"op": "create", "name": "Roteiro", "doc": "x"}));
    assert!(e.message.contains("'doc'") && !e.message.contains("válidos: doc"), "{}", e.message);
    // `op` inexistente: o erro é o do serde sobre o valor, não sobre as chaves
    let e = act_err(&mut s, "SCRIPT", json!({"op": "criar", "name": "Roteiro"}));
    assert!(e.message.contains("criar") && !e.message.contains("desconhecido '"), "{}", e.message);

    // TODA ação do núcleo recusa uma chave inventada (ação nova sem a verificação quebra aqui)
    let all = [core_ops::CORE_ACTIONS, core_tools::TOOL_ACTIONS, production_tools::PRODUCTION_ACTIONS, preprod_tools::PREPROD_ACTIONS, review_tools::REVIEW_ACTIONS, satsuei_tools::SATSUEI_ACTIONS, preview_tools::PREVIEW_ACTIONS];
    for action in all.concat() {
        let e = act_err(&mut s, action, json!({"zz_inventada": 1}));
        assert!(e.code == code::INVALID_PARAMS && e.message.contains("'zz_inventada'"), "{action} não recusou a chave: {e}");
    }
}
