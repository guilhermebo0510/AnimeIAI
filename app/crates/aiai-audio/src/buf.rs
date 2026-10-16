//! Buffer de áudio planar em f32 e erros do crate.

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("wav: {0}")]
    Wav(String),
    #[error("http: {0}")]
    Http(String),
    #[error("voicevox: {0}")]
    Voicevox(String),
    #[error("soundfont: {0}")]
    SoundFont(String),
    #[error("{0}")]
    Invalid(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type AudioResult<T> = Result<T, AudioError>;

/// Áudio planar: `ch[c][i]`. Todos os canais têm o mesmo comprimento.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuf {
    pub rate: u32,
    pub ch: Vec<Vec<f32>>,
}

impl AudioBuf {
    pub fn zeros(rate: u32, channels: usize, frames: usize) -> Self {
        AudioBuf { rate, ch: vec![vec![0.0; frames]; channels] }
    }

    pub fn mono(rate: u32, samples: Vec<f32>) -> Self {
        AudioBuf { rate, ch: vec![samples] }
    }

    pub fn stereo(rate: u32, l: Vec<f32>, r: Vec<f32>) -> Self {
        debug_assert_eq!(l.len(), r.len());
        AudioBuf { rate, ch: vec![l, r] }
    }

    pub fn frames(&self) -> usize {
        self.ch.first().map_or(0, |c| c.len())
    }

    pub fn channels(&self) -> usize {
        self.ch.len()
    }

    pub fn duration_s(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    /// Estéreo: mono vira dois canais iguais; mais de dois canais mantém só os dois primeiros.
    pub fn to_stereo(&self) -> AudioBuf {
        match self.ch.len() {
            0 => AudioBuf::zeros(self.rate, 2, 0),
            1 => AudioBuf { rate: self.rate, ch: vec![self.ch[0].clone(), self.ch[0].clone()] },
            _ => AudioBuf { rate: self.rate, ch: vec![self.ch[0].clone(), self.ch[1].clone()] },
        }
    }

    /// Média dos canais.
    pub fn to_mono(&self) -> AudioBuf {
        let n = self.frames();
        let k = self.ch.len().max(1) as f32;
        let mut out = vec![0.0f32; n];
        for c in &self.ch {
            for (o, s) in out.iter_mut().zip(c) {
                *o += s;
            }
        }
        for o in &mut out {
            *o /= k;
        }
        AudioBuf::mono(self.rate, out)
    }

    /// Maior valor absoluto de amostra (linear).
    pub fn peak(&self) -> f32 {
        self.ch.iter().flat_map(|c| c.iter()).fold(0.0f32, |m, &s| m.max(s.abs()))
    }

    pub fn rms(&self) -> f64 {
        let n = (self.frames() * self.channels()).max(1) as f64;
        (self.ch.iter().flat_map(|c| c.iter()).map(|&s| (s as f64).powi(2)).sum::<f64>() / n).sqrt()
    }

    pub fn apply_gain_db(&mut self, db: f64) {
        let g = crate::db_to_lin(db) as f32;
        for c in &mut self.ch {
            for s in c.iter_mut() {
                *s *= g;
            }
        }
    }

    /// Soma `other` (mesmos canais) a partir de `offset` quadros, com ganho linear.
    pub fn mix_in(&mut self, other: &AudioBuf, offset: usize, gain: f32) {
        for (dst, src) in self.ch.iter_mut().zip(&other.ch) {
            for (i, &s) in src.iter().enumerate() {
                if let Some(d) = dst.get_mut(offset + i) {
                    *d += s * gain;
                }
            }
        }
    }

    /// Trecho `[start, start+len)` em quadros (cortado no fim do buffer).
    pub fn slice(&self, start: usize, len: usize) -> AudioBuf {
        let n = self.frames();
        let a = start.min(n);
        let b = (start + len).min(n);
        AudioBuf { rate: self.rate, ch: self.ch.iter().map(|c| c[a..b].to_vec()).collect() }
    }

    pub fn truncate(&mut self, frames: usize) {
        for c in &mut self.ch {
            c.truncate(frames);
        }
    }

    pub fn resize(&mut self, frames: usize) {
        for c in &mut self.ch {
            c.resize(frames, 0.0);
        }
    }

    /// Troca os valores não finitos por zero (defesa contra NaN vindo de DSP).
    pub fn sanitize(&mut self) {
        for c in &mut self.ch {
            for s in c.iter_mut() {
                if !s.is_finite() {
                    *s = 0.0;
                }
            }
        }
    }
}
