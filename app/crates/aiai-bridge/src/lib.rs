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
//! * [`core_tools`] — RENDER, DRAW_CURVES, FILL, LINT, PAINT, GRADIENT, TRANSFORM, REPEAT, BRUSH_PRESETS, READ_POINTS, EDIT_POINTS (também só o documento);
//! * [`preview_tools`] — prévia e entrega: RENDER_SHEET, EDIT_PREVIEW, EDIT_MIX, TTS_REGENERATE;
//! * [`audio_tools`] — edição e análise de áudio (S24): AUDIO_EDIT, AUDIO_ANALYZE;
//! * [`ruler_tools`] — réguas editáveis da camada de guia (S24): RULER;
//! * [`model3d_tools`] — camada 3D de referência (S24): MODEL3D;
//! * [`modeling_tools`] — modelagem por comando (S24): MODELING;
//! * [`library_tools`] — biblioteca de recursos (S24): LIBRARY;
//! * [`delivery_tools`] — entrega (S24): DELIVERY (fila, destinos, entregáveis, verificação final, pacote); [`export_ctl`] — progresso e cancelamento do EXPORT_VIDEO;
//! * [`preprod_tools`] — pré-produção (S16): SCRIPT, BEATS, STORYBOARD, ANIMATIC, CUTS_FROM_STORYBOARD;
//! * [`satsuei_tools`] — satsuei, câmera e cor tipados (S13/S15): EFFECTS, GRADE, CAMERA, LAYER_MOTION;
//! * [`review_tools`] — produção e revisão (S19): PRODUCTION, CORRECTION, RETAKE;
//! * [`production_tools`] — SKY, PATTERN_FILL, TEXT, LINES, GAG_BG, PARTICLES, GUIDE3D, PERSPECTIVE, MODEL_SHEET, CONSISTENCY, INBETWEEN, PROPAGATE_FILLS, áudio (TTS, LIPSYNC, SFX, BGM, MIX_CUT, LOUDNESS), COMPARE, PROBE_COLOR, MEASURE_LINE, EXPORT_VIDEO;
//! * [`params_check`] — chave desconhecida nos parâmetros de uma ação é erro (nomeia a chave e lista as válidas);
//! * [`discovery`] — arquivo `bridge.json` (porta + token) com permissão restrita.
//!
//! Uso no app: `BridgeServer::start(BridgeConfig::from_env()...)`, guardar o `BridgeServer` e, no início de cada quadro,
//! responder cada `HostRequest` do `BridgeReceiver` (documento e ferramentas via `core_ops::handle_core_action`;
//! SCREENSHOT, INPUT, GET_UI_STATE, UI_ACTION, COMPARE, GET_STATUS pelo app).

pub mod audio_tools;
pub mod core_ops;
pub mod core_tools;
pub mod delivery_tools;
pub mod discovery;
pub mod export_ctl;
pub mod host;
pub mod http;
pub mod library_tools;
pub mod model3d_tools;
pub mod modeling_tools;
pub mod params_check;
pub mod preprod_tools;
pub mod preview_tools;
pub mod production_tools;
pub mod review_tools;
pub mod satsuei_tools;
pub mod protocol;
pub mod ruler_tools;
pub mod server;

pub use host::{channel, BridgeReceiver, BridgeReply, ChannelBackend, HostRequest};
pub use protocol::{code, BridgeError, BridgeResult, Request, PROTOCOL_VERSION};
pub use server::{Backend, BridgeConfig, BridgeServer, ServerConfig};
