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
