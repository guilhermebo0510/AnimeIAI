//! Leitura e gravação de WAV (hound). Entrada: PCM 8/16/24/32 e float 32; saída: PCM16, PCM24 ou float 32.

use crate::buf::{AudioBuf, AudioError, AudioResult};
use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    Pcm16,
    Pcm24,
    Float32,
}

pub fn read_wav(bytes: &[u8]) -> AudioResult<AudioBuf> {
    let mut r = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| AudioError::Wav(e.to_string()))?;
    let spec = r.spec();
    let nch = spec.channels as usize;
    if nch == 0 || nch > 8 {
        return Err(AudioError::Wav(format!("{} canais não suportado", spec.channels)));
    }
    let mut ch: Vec<Vec<f32>> = vec![Vec::new(); nch];
    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => {
            for (i, s) in r.samples::<f32>().enumerate() {
                ch[i % nch].push(s.map_err(|e| AudioError::Wav(e.to_string()))?);
            }
        }
        (hound::SampleFormat::Int, bits @ (8 | 16 | 24 | 32)) => {
            let scale = 1.0 / (1u64 << (bits - 1)) as f64;
            for (i, s) in r.samples::<i32>().enumerate() {
                ch[i % nch].push((s.map_err(|e| AudioError::Wav(e.to_string()))? as f64 * scale) as f32);
            }
        }
        (f, b) => return Err(AudioError::Wav(format!("formato não suportado: {f:?} {b} bits"))),
    }
    Ok(AudioBuf { rate: spec.sample_rate, ch })
}

pub fn write_wav(buf: &AudioBuf, fmt: WavFormat) -> AudioResult<Vec<u8>> {
    let nch = buf.channels().max(1);
    let (bits, sample_format) = match fmt {
        WavFormat::Pcm16 => (16, hound::SampleFormat::Int),
        WavFormat::Pcm24 => (24, hound::SampleFormat::Int),
        WavFormat::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels: nch as u16, sample_rate: buf.rate, bits_per_sample: bits, sample_format };
    let mut out = Cursor::new(Vec::<u8>::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).map_err(|e| AudioError::Wav(e.to_string()))?;
        let n = buf.frames();
        for i in 0..n {
            for c in 0..nch {
                let s = buf.ch.get(c).map_or(0.0, |v| v[i]);
                let r = match fmt {
                    WavFormat::Float32 => w.write_sample(s),
                    WavFormat::Pcm16 => w.write_sample((s.clamp(-1.0, 1.0) * 32767.0).round() as i16),
                    WavFormat::Pcm24 => w.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32),
                };
                r.map_err(|e| AudioError::Wav(e.to_string()))?;
            }
        }
        w.finalize().map_err(|e| AudioError::Wav(e.to_string()))?;
    }
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_formats() {
        let x: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
        let b = AudioBuf::stereo(48_000, x.clone(), x.iter().map(|v| -v).collect());
        for (fmt, tol) in [(WavFormat::Float32, 0.0), (WavFormat::Pcm24, 2e-7), (WavFormat::Pcm16, 5e-5)] {
            let back = read_wav(&write_wav(&b, fmt).unwrap()).unwrap();
            assert_eq!((back.rate, back.channels(), back.frames()), (48_000, 2, 1000));
            let err = back.ch[1].iter().zip(&b.ch[1]).map(|(a, c)| (a - c).abs()).fold(0.0f32, f32::max);
            assert!(err <= tol + 1e-9, "{fmt:?}: {err}");
        }
        assert!(read_wav(b"not a wav").is_err());
    }
}
