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
//! * [`core_tools`] — RENDER, DRAW_CURVES, FILL, LINT, PAINT, GRADIENT, TRANSFORM, REPEAT, BRUSH_PRESETS (também só o documento);
//! * [`preprod_tools`] — pré-produção (S16): SCRIPT, BEATS, STORYBOARD, ANIMATIC, CUTS_FROM_STORYBOARD;
//! * [`satsuei_tools`] — satsuei, câmera e cor tipados (S13/S15): EFFECTS, GRADE, CAMERA, LAYER_MOTION;
//! * [`review_tools`] — produção e revisão (S19): PRODUCTION, CORRECTION, RETAKE;
//! * [`production_tools`] — SKY, PATTERN_FILL, TEXT, LINES, GAG_BG, PARTICLES, GUIDE3D, PERSPECTIVE, MODEL_SHEET, CONSISTENCY, INBETWEEN, PROPAGATE_FILLS, áudio (TTS, LIPSYNC, SFX, BGM, MIX_CUT, LOUDNESS), COMPARE, PROBE_COLOR, MEASURE_LINE, EXPORT_VIDEO;
//! * [`discovery`] — arquivo `bridge.json` (porta + token) com permissão restrita.
//!
//! Uso no app: `BridgeServer::start(BridgeConfig::from_env()...)`, guardar o `BridgeServer` e, no início de cada quadro,
//! responder cada `HostRequest` do `BridgeReceiver` (documento e ferramentas via `core_ops::handle_core_action`;
//! SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION, COMPARE, GET_STATUS pelo app).

pub mod core_ops;
pub mod core_tools;
pub mod discovery;
pub mod host;
pub mod http;
pub mod preprod_tools;
pub mod production_tools;
pub mod review_tools;
pub mod satsuei_tools;
pub mod protocol;
pub mod server;

pub use host::{channel, BridgeReceiver, BridgeReply, ChannelBackend, HostRequest};
pub use protocol::{code, BridgeError, BridgeResult, Request, PROTOCOL_VERSION};
pub use server::{Backend, BridgeConfig, BridgeServer, ServerConfig};
