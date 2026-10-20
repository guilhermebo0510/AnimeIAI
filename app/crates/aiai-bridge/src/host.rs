//! Ponte entre as threads de conexão e a thread "dona" do estado (a UI do studio).
//!
//! A conexão cria um [`HostRequest`] com um canal de resposta e o envia por um canal LIMITADO; a UI chama
//! [`BridgeReceiver::try_recv_all`] no início do quadro, executa e responde (`request.reply.send(..)`). A UI é
//! acordada com a função `wake` (ex.: `ctx.request_repaint()`).
//!
//! Se a UI não responder no prazo da ação, o pedido é marcado `cancelled` e o cliente recebe `-32002`; quem
//! drena DEVE ignorar pedidos cancelados (nunca aplicar uma mutação tarde demais) — `try_recv_all` já descarta os
//! que estavam cancelados ao sair da fila, e `HostRequest::is_cancelled` permite conferir de novo antes de aplicar.

use crate::protocol::{code, BridgeError, BridgeResult, Request};
use crate::server::Backend;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Resposta de uma ação (a mesma forma do restante da bridge).
pub type BridgeReply = BridgeResult;

/// Pulso da UI: a última vez que ela drenou a fila (um quadro desenhado). Janela minimizada/oculta = sem quadros = sem pulso.
pub struct Heartbeat {
    t0: Instant,
    last_ms: AtomicU64,
    count: AtomicU64,
}

impl Heartbeat {
    fn new() -> Self {
        Self { t0: Instant::now(), last_ms: AtomicU64::new(0), count: AtomicU64::new(0) }
    }
    fn now_ms(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }
    fn beat(&self) {
        self.last_ms.store(self.now_ms().max(1), Relaxed);
        self.count.fetch_add(1, Relaxed);
    }
    fn count(&self) -> u64 {
        self.count.load(Relaxed)
    }
    /// Há quanto tempo a UI não desenha um quadro (`None` = nunca desenhou).
    pub fn idle(&self) -> Option<Duration> {
        match self.last_ms.load(Relaxed) {
            0 => None,
            l => Some(Duration::from_millis(self.now_ms().saturating_sub(l))),
        }
    }
}

/// Um pedido vindo de um cliente (MCP, CLI, Python), a ser atendido pela thread da UI.
pub struct HostRequest {
    pub id: String,
    pub action: String,
    pub params: Value,
    /// Canal de resposta: `request.reply.send(Ok(json))` ou `Err(BridgeError)`. Falha de envio (cliente desistiu) é ignorável.
    pub reply: Sender<BridgeReply>,
    pub cancelled: Arc<AtomicBool>,
}

impl HostRequest {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Relaxed)
    }

    /// Responde ignorando o erro de "cliente já desistiu".
    pub fn respond(self, result: BridgeReply) {
        let _ = self.reply.send(result);
    }
}

/// Lado da UI.
pub struct BridgeReceiver {
    rx: Receiver<HostRequest>,
    beat: Arc<Heartbeat>,
}

impl BridgeReceiver {
    /// Chamar uma vez por quadro: todos os pedidos pendentes (até `MAX_PER_FRAME`), sem os já cancelados.
    pub fn try_recv_all(&self) -> Vec<HostRequest> {
        self.drain(Self::MAX_PER_FRAME)
    }

    pub const MAX_PER_FRAME: usize = 64;

    /// Até `max` pedidos pendentes, descartando os já cancelados (o cliente já recebeu o timeout).
    pub fn drain(&self, max: usize) -> Vec<HostRequest> {
        self.beat.beat();
        let mut out = Vec::new();
        while out.len() < max {
            match self.rx.try_recv() {
                Ok(r) if r.is_cancelled() => {}
                Ok(r) => out.push(r),
                Err(_) => break,
            }
        }
        out
    }

    pub fn pending(&self) -> usize {
        self.rx.len()
    }
}

/// Quanto esperar por UM quadro antes de concluir que a UI não está desenhando.
const FRAME_GRACE: Duration = Duration::from_millis(2500);

/// Um quadro desenhado há menos que isto conta como UI viva (ver `ChannelBackend::call`).
const RECENT_FRAME: Duration = Duration::from_millis(1500);

/// Prazo de cada ação (tempo para a UI responder).
pub fn action_timeout(action: &str) -> Duration {
    match action {
        "SCREENSHOT" | "RENDER" | "COMPARE" => Duration::from_secs(30),
        "OPEN_PROJECT" | "SAVE_PROJECT" | "NEW_PROJECT" => Duration::from_secs(20),
        // ferramentas de produção: o trabalho roda na thread da UI (exportar vídeo bloqueia a janela até terminar)
        "EXPORT_VIDEO" | "ANIMATIC" => Duration::from_secs(1190),
        "STORYBOARD" | "CUTS_FROM_STORYBOARD" | "PRODUCTION" | "RETAKE" => Duration::from_secs(60),
        "RENDER_SHEET" | "EDIT_PREVIEW" | "EDIT_MIX" | "TTS_REGENERATE" => Duration::from_secs(120),
        "BGM" | "TTS_LINE" | "TTS_KOKORO" | "MIX_CUT" | "LOUDNESS" | "AUDIO_EDIT" | "AUDIO_ANALYZE" => Duration::from_secs(120),
        "SKY" | "PATTERN_FILL" | "FILL_SHAPE" | "EDIT_SET" | "TEXT" | "GAG_BG" | "GUIDE3D" | "MODEL_SHEET" | "CHARACTER_MATERIAL" | "INBETWEEN" | "PROPAGATE_FILLS" | "LIPSYNC" => Duration::from_secs(60),
        _ => Duration::from_secs(10),
    }
}

/// Lado das conexões (implementa [`Backend`]).
pub struct ChannelBackend {
    tx: Sender<HostRequest>,
    beat: Arc<Heartbeat>,
    wake: Arc<dyn Fn() + Send + Sync>,
    timeout: fn(&str) -> Duration,
}

/// Cria o par (backend para o servidor, receptor para a UI). `capacity` limita pedidos pendentes.
pub fn channel(capacity: usize, wake: Arc<dyn Fn() + Send + Sync>) -> (ChannelBackend, BridgeReceiver) {
    let (tx, rx) = bounded(capacity.max(1));
    let beat = Arc::new(Heartbeat::new());
    (ChannelBackend { tx, beat: Arc::clone(&beat), wake, timeout: action_timeout }, BridgeReceiver { rx, beat })
}

impl ChannelBackend {
    /// Troca a política de prazos (testes).
    pub fn with_timeout(mut self, f: fn(&str) -> Duration) -> Self {
        self.timeout = f;
        self
    }
}

impl Backend for ChannelBackend {
    fn ui_idle(&self) -> Option<Duration> {
        self.beat.idle()
    }

    fn call(&self, req: &Request) -> BridgeResult {
        let (rtx, rrx) = bounded(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let hr = HostRequest {
            id: req.id_string(),
            action: req.action.clone(),
            params: req.params.clone(),
            reply: rtx,
            cancelled: Arc::clone(&cancelled),
        };
        match self.tx.try_send(hr) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(BridgeError::new(code::BUSY, "fila da UI cheia; tente novamente")),
            Err(TrySendError::Disconnected(_)) => return Err(BridgeError::new(code::SHUTTING_DOWN, "o app está encerrando")),
        }
        let sent = Instant::now();
        let beats_before = self.beat.count();
        // a UI desenhou há pouco? então um quadro sem pulso durante a carência é um quadro LONGO (trocar de workspace com
        // miniaturas, máquina paginando), não janela oculta: espera o prazo inteiro em vez de acusar minimização
        let drew_recently = self.beat.idle().is_some_and(|d| d < RECENT_FRAME);
        (self.wake)();
        let limit = (self.timeout)(&req.action);
        // 1ª espera curta: se a UI nem desenhou um quadro nesse tempo, a janela está minimizada/oculta (sem quadros não há
        // quem drene a fila) — falha logo com a causa em vez de segurar o cliente pelo prazo inteiro.
        let grace = FRAME_GRACE.min(limit);
        let result = match rrx.recv_timeout(grace) {
            Ok(r) => Ok(r),
            Err(_) => {
                // a UI drenou a fila depois do pedido? então está viva, só ocupada: espera o resto do prazo
                let alive = drew_recently || self.beat.count() != beats_before;
                if alive {
                    rrx.recv_timeout(limit.saturating_sub(sent.elapsed()))
                } else {
                    cancelled.store(true, Relaxed);
                    return Err(BridgeError::new(
                        code::UI_TIMEOUT,
                        "a UI não está desenhando quadros: a janela do AnimeIAI está minimizada ou oculta (restaure-a); a ação não foi aplicada",
                    ));
                }
            }
        };
        match result {
            Ok(r) => r,
            Err(_) => {
                cancelled.store(true, Relaxed);
                Err(BridgeError::new(
                    code::UI_TIMEOUT,
                    format!("a UI não respondeu em {} s (janela minimizada/oculta ou ocupada?); a ação não foi aplicada", limit.as_secs()),
                ))
            }
        }
    }
}
