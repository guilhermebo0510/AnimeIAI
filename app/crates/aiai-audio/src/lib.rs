//! aiai-audio — motor de áudio do AnimeIAI (S17). Tudo local e gratuito; nenhuma dependência do núcleo do documento:
//! a camada `aiai-tools` converte clipes/blobs do projeto para os tipos daqui.
//!
//! * [`wav`], [`resample`]: leitura/gravação de WAV e reamostragem sinc com janela Kaiser;
//! * [`loudness`]: ITU-R BS.1770-4 / EBU R128 (momentâneo, curto prazo, integrado com portas, LRA, true peak 4x);
//! * [`dsp`], [`mixer`]: biquads, EQ de 3 bandas, compressor, limitador com lookahead, reverb, ducking e o mixer do corte;
//! * [`sfx`]: SFX procedurais determinísticos por semente; [`bgm`]: sequenciador com SoundFont (rustysynth) e síntese própria de reserva;
//! * [`kokoro`]: Kokoro-82M (PT-BR/EN) por processo externo (`uv run --script`);
//! * [`voicevox`]: cliente HTTP do VOICEVOX Engine (consulta de áudio, edição de moras, síntese, tempos);
//! * [`lipsync`]: moras/energia → forma de boca por quadro.

pub mod bgm;
pub mod buf;
pub mod dsp;
pub mod http;
pub mod kokoro;
pub mod lipsync;
pub mod loudness;
pub mod mixer;
pub mod resample;
pub mod rng;
pub mod sfx;
pub mod timing;
pub mod voicevox;
pub mod wav;

pub use buf::{AudioBuf, AudioError, AudioResult};

/// Taxa de amostragem padrão do projeto.
pub const PROJECT_RATE: u32 = 48_000;

pub fn db_to_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn lin_to_db(x: f64) -> f64 {
    if x <= 0.0 { f64::NEG_INFINITY } else { 20.0 * x.log10() }
}
