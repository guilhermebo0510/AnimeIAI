//! # aiai-bridge
//!
//! Servidor local (loopback + token por sessão) pelo qual o `aiai-mcp`, a CLI e o cliente Python controlam o
//! AnimeIAI Studio aberto com `--bridge` (ADR-004). Padrão copiado do ANIGO (`anigo-bridge`).
//!
//! * [`protocol`] — envelope NDJSON, erros e token;
//! * [`http`] — HTTP/1.1 mínimo e estrito (Host/Origin): `POST /rpc`, `GET /health`;
//! * [`server`] — threads + limites + auth; NDJSON e HTTP na mesma porta; [`BridgeServer::start`];
//! * [`host`] — ponte para a thread da UI (canal limitado, prazo, cancelamento): [`BridgeReceiver::try_recv_all`];
//! * [`core_ops`] — ações de documento que só precisam da `CoreSession` ([`core_ops::handle_core_action`]);
//! * [`discovery`] — arquivo `bridge.json` (porta + token) com permissão restrita.
//!
//! Uso no app (parte B): ligar com `BridgeServer::start(BridgeConfig::from_env()...)`, guardar o `BridgeServer` e,
//! no início de cada quadro, responder cada `HostRequest` do `BridgeReceiver` (ações de documento via
//! `core_ops::handle_core_action`; SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION, RENDER, COMPARE, GET_STATUS pelo app).

pub mod core_ops;
pub mod discovery;
pub mod host;
pub mod http;
pub mod protocol;
pub mod server;

pub use host::{channel, BridgeReceiver, BridgeReply, ChannelBackend, HostRequest};
pub use protocol::{code, BridgeError, BridgeResult, Request, PROTOCOL_VERSION};
pub use server::{Backend, BridgeConfig, BridgeServer, ServerConfig};
