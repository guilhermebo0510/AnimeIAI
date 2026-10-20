//! Controle de uma exportação em andamento: progresso por quadro, pausa, retomada e cancelamento.
//!
//! `EXPORT_VIDEO` roda inteiro numa thread; quem quer acompanhar (a fila do app) cria um [`ExportControl`], instala-o nessa
//! thread com [`install`] antes de chamar a ação e guarda o `Arc` para ler o progresso e pedir pausa/cancelamento. A ação
//! olha o controle a cada quadro. Sem controle instalado (MCP, testes) nada muda.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Mensagem do erro devolvido quando o pedido de cancelamento é atendido.
pub const CANCELED: &str = "exportação cancelada";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Lendo o documento e conferindo o pedido.
    Preparing,
    /// Mixando e normalizando o som.
    Audio,
    /// Desenhando e codificando os quadros.
    Frames,
    /// Fechando o arquivo, legendas e medição do resultado.
    Finishing,
}

impl Phase {
    fn from_u8(v: u8) -> Phase {
        match v {
            1 => Phase::Audio,
            2 => Phase::Frames,
            3 => Phase::Finishing,
            _ => Phase::Preparing,
        }
    }
}

#[derive(Debug)]
pub struct ExportControl {
    total: AtomicU64,
    done: AtomicU64,
    phase: AtomicU8,
    cancel: AtomicBool,
    pause: AtomicBool,
    /// Quando o primeiro quadro começou (para a velocidade e o ETA, sem contar o tempo de preparo e de pausa).
    frames_started: std::sync::Mutex<Option<Instant>>,
    paused_total: std::sync::Mutex<Duration>,
    paused_since: std::sync::Mutex<Option<Instant>>,
}

impl Default for ExportControl {
    fn default() -> Self {
        ExportControl {
            total: AtomicU64::new(0),
            done: AtomicU64::new(0),
            phase: AtomicU8::new(0),
            cancel: AtomicBool::new(false),
            pause: AtomicBool::new(false),
            frames_started: std::sync::Mutex::new(None),
            paused_total: std::sync::Mutex::new(Duration::ZERO),
            paused_since: std::sync::Mutex::new(None),
        }
    }
}

impl ExportControl {
    pub fn new() -> Arc<ExportControl> {
        Arc::new(ExportControl::default())
    }

    // ---------------------------------------------------------------------------------------- lado da exportação

    /// Define o total de quadros e a fase de partida.
    pub fn begin(&self, total: u64, phase: Phase) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        self.set_phase(phase);
    }

    pub fn set_phase(&self, p: Phase) {
        self.phase.store(p as u8, Ordering::Relaxed);
        if p == Phase::Frames {
            let mut g = self.frames_started.lock().unwrap_or_else(|e| e.into_inner());
            g.get_or_insert_with(Instant::now);
        }
    }

    pub fn frame_done(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Ponto de espera entre os quadros: parado enquanto estiver em pausa; `Err` se foi cancelada.
    pub fn checkpoint(&self) -> Result<(), ()> {
        while self.pause.load(Ordering::Relaxed) && !self.cancel.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(40));
        }
        if self.cancel.load(Ordering::Relaxed) { Err(()) } else { Ok(()) }
    }

    // ---------------------------------------------------------------------------------------- lado de quem acompanha

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_canceled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, on: bool) {
        let was = self.pause.swap(on, Ordering::Relaxed);
        if on && !was {
            *self.paused_since.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        } else if !on && was {
            if let Some(t0) = self.paused_since.lock().unwrap_or_else(|e| e.into_inner()).take() {
                *self.paused_total.lock().unwrap_or_else(|e| e.into_inner()) += t0.elapsed();
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    pub fn phase(&self) -> Phase {
        Phase::from_u8(self.phase.load(Ordering::Relaxed))
    }

    pub fn frames(&self) -> (u64, u64) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// Quadros por segundo desde o primeiro quadro, sem contar as pausas; `None` antes de haver amostra.
    pub fn speed(&self) -> Option<f64> {
        let t0 = (*self.frames_started.lock().unwrap_or_else(|e| e.into_inner()))?;
        let mut el = t0.elapsed().saturating_sub(*self.paused_total.lock().unwrap_or_else(|e| e.into_inner()));
        if let Some(p) = *self.paused_since.lock().unwrap_or_else(|e| e.into_inner()) {
            el = el.saturating_sub(p.elapsed());
        }
        let done = self.done.load(Ordering::Relaxed);
        (done >= 2 && el.as_secs_f64() > 0.05).then(|| done as f64 / el.as_secs_f64())
    }

    /// Tempo que falta, em segundos, pelo ritmo até agora.
    pub fn eta_secs(&self) -> Option<f64> {
        let (done, total) = self.frames();
        if self.phase() != Phase::Frames || total <= done {
            return None;
        }
        self.speed().filter(|s| *s > 0.0).map(|s| (total - done) as f64 / s)
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<ExportControl>>> = const { RefCell::new(None) };
}

/// Instala o controle na thread atual até o guarda sair do escopo.
pub struct Guard(Option<Arc<ExportControl>>);

pub fn install(ctl: Arc<ExportControl>) -> Guard {
    Guard(CURRENT.with(|c| c.borrow_mut().replace(ctl)))
}

impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|c| *c.borrow_mut() = self.0.take());
    }
}

/// O controle instalado na thread atual (se houver).
pub fn current() -> Option<Arc<ExportControl>> {
    CURRENT.with(|c| c.borrow().clone())
}
