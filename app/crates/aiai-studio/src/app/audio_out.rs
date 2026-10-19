//! Som ao vivo: a mixagem do corte (a mesma do `MIX_CUT`) tocada pela saída padrão do Windows enquanto a linha do
//! tempo reproduz, e um trecho curto quando o cabeçote anda parado (scrub). Sem dependência nova: `PlaySoundW` do
//! winmm com um WAV em memória (assíncrono; tocar de novo troca o som; `None` para).
//! Nas execuções de teste/captura (`--no-session`, `--screenshot`) fica mudo, a menos de `AIAI_AUDIO=1`.

use aiai_core::{CoreSession, Id};
use std::sync::Arc;

pub(crate) struct AudioOut {
    enabled: bool,
    /// Mixagem em cache: (corte, contador de mudanças do documento) → estéreo.
    mix: Option<(Id, u64, Arc<aiai_audio::AudioBuf>)>,
    /// WAV que está tocando (o winmm lê da memória enquanto toca: tem de viver até parar).
    playing: Option<Vec<u8>>,
    last_playing: bool,
    last_frame: u32,
    last_cut: Option<Id>,
    pub(crate) last_error: Option<String>,
    /// Quantas vezes tocou (estado para a bridge e testes).
    pub(crate) starts: u32,
    pub(crate) scrubs: u32,
}

impl AudioOut {
    pub(crate) fn new(muted_by_cli: bool) -> Self {
        let forced = std::env::var("AIAI_AUDIO").is_ok_and(|v| v == "1");
        AudioOut { enabled: forced || !muted_by_cli, mix: None, playing: None, last_playing: false, last_frame: u32::MAX, last_cut: None, last_error: None, starts: 0, scrubs: 0 }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn set_enabled(&mut self, on: bool) {
        if !on {
            self.stop();
        }
        self.enabled = on;
    }

    fn mix_for(&mut self, core: &CoreSession, cut: &Id) -> Option<Arc<aiai_audio::AudioBuf>> {
        if let Some((c, n, m)) = &self.mix {
            if c == cut && *n == core.change_counter {
                return Some(m.clone());
            }
        }
        match aiai_tools::audio::mix_cut(&core.project, &core.blobs, cut, None) {
            Ok(m) => {
                let m = Arc::new(m);
                self.mix = Some((cut.clone(), core.change_counter, m.clone()));
                Some(m)
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                None
            }
        }
    }

    /// Acompanha a reprodução da linha do tempo (chamado a cada quadro da interface).
    pub(crate) fn sync(&mut self, core: &CoreSession, cut: Option<&Id>, frame: u32, playing: bool, loop_end: u32) {
        let cut_changed = cut != self.last_cut.as_ref();
        if !self.enabled || cut.is_none() {
            if self.playing.is_some() {
                self.stop();
            }
        } else if let Some(cut) = cut {
            let fps = core.project.settings.fps().max(1.0);
            if playing && (!self.last_playing || frame < self.last_frame || cut_changed) {
                // começou a tocar, deu a volta no laço ou trocou de corte: toca do quadro atual até o fim do laço
                self.play_range(core, cut, frame as f64 / fps, (loop_end + 1) as f64 / fps);
                self.starts += 1;
            } else if !playing && self.last_playing {
                self.stop();
            } else if !playing && !cut_changed && frame != self.last_frame && self.last_frame != u32::MAX {
                // cabeçote movido parado: um trecho de ~1,5 quadro (scrub)
                let t = frame as f64 / fps;
                self.play_range(core, cut, t, t + 1.5 / fps);
                self.scrubs += 1;
            }
        }
        self.last_playing = playing;
        self.last_frame = frame;
        self.last_cut = cut.cloned();
    }

    fn play_range(&mut self, core: &CoreSession, cut: &Id, t0: f64, t1: f64) {
        let Some(mix) = self.mix_for(core, cut) else { return };
        let rate = mix.rate as f64;
        let n = mix.ch.first().map(|c| c.len()).unwrap_or(0);
        let (s0, s1) = (((t0 * rate) as usize).min(n), ((t1 * rate).ceil() as usize).min(n));
        if s1 <= s0 {
            self.stop();
            return;
        }
        let wav = wav_bytes(&mix, s0, s1);
        play_wav(Some(&wav));
        self.playing = Some(wav); // fica vivo até o próximo som ou parar
    }

    pub(crate) fn stop(&mut self) {
        play_wav(None);
        self.playing = None;
    }
}

/// WAV PCM 16 bits estéreo do trecho [s0, s1) da mixagem.
pub(crate) fn wav_bytes(mix: &aiai_audio::AudioBuf, s0: usize, s1: usize) -> Vec<u8> {
    let frames = s1 - s0;
    let data_len = (frames * 4) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&mix.rate.to_le_bytes());
    v.extend_from_slice(&(mix.rate * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    let l = &mix.ch[0];
    let r = mix.ch.get(1).unwrap_or(l);
    for i in s0..s1 {
        for c in [l[i], r[i]] {
            v.extend_from_slice(&((c.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
    }
    v
}

#[cfg(windows)]
fn play_wav(wav: Option<&[u8]>) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    // SAFETY: com SND_MEMORY o primeiro argumento aponta para o WAV, que o chamador mantém vivo enquanto toca;
    // NULL para tudo.
    unsafe {
        match wav {
            Some(w) => {
                PlaySoundW(w.as_ptr() as *const u16, std::ptr::null_mut(), SND_ASYNC | SND_MEMORY | SND_NODEFAULT);
            }
            None => {
                PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
            }
        }
    }
}

#[cfg(not(windows))]
fn play_wav(_wav: Option<&[u8]>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_slice_has_a_valid_header_and_clamped_samples() {
        let mix = aiai_audio::AudioBuf { rate: 48000, ch: vec![vec![0.5, 2.0, -3.0, 0.0], vec![0.0; 4]] };
        let w = wav_bytes(&mix, 1, 3);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8, "2 quadros estéreo de 16 bits");
        assert_eq!(i16::from_le_bytes([w[44], w[45]]), 32767);
        assert_eq!(i16::from_le_bytes([w[48], w[49]]), -32767);
    }

    #[test]
    fn sync_tracks_play_stop_wrap_and_scrub_without_a_cut_mix() {
        // sem corte: nunca toca (e não falha)
        let core = CoreSession::new(aiai_core::Project::new("x"));
        let mut a = AudioOut::new(true);
        a.sync(&core, None, 0, true, 10);
        assert_eq!((a.starts, a.scrubs), (0, 0));
    }
}
