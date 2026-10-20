//! Passos de edição serializáveis (`EditStep`): as operações de clipe de [`crate::edit`], os efeitos de [`crate::fx`]
//! (no clipe ou só num intervalo) e o rack, em uma lista que a bridge/MCP aplica a um asset de áudio. Cada passo recebe o
//! buffer e devolve um novo; nada altera a fonte. Tempos em segundos; intervalo omitido = clipe inteiro.

use crate::buf::{AudioBuf, AudioError, AudioResult};
use crate::edit::{self, FadeCurve, FadeDir, secs_to_frames};
use crate::fx::{AudioFx, Effect, NoiseProfile, Rack};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Teto da duração do resultado (evita `speed`/`time_stretch` extremos estourarem a memória).
pub const MAX_RESULT_SECONDS: f64 = 1800.0;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditStep {
    /// Aparar: fica só com `[start_s, end_s)`.
    Trim { start_s: f64, end_s: f64 },
    /// Apagar (recortar) `[start_s, end_s)` e fechar o buraco.
    Delete { start_s: f64, end_s: f64 },
    /// Silenciar `[start_s, end_s)` mantendo a duração.
    Silence { start_s: f64, end_s: f64 },
    /// Copiar `[start_s, end_s)` e colar (inserir) em `at_s`.
    Duplicate { start_s: f64, end_s: f64, at_s: f64 },
    /// Inserir `duration_s` de silêncio em `at_s`.
    InsertSilence { at_s: f64, duration_s: f64 },
    Fade {
        #[serde(default)]
        start_s: Option<f64>,
        #[serde(default)]
        end_s: Option<f64>,
        dir: FadeDir,
        #[serde(default)]
        curve: FadeCurve,
    },
    Reverse {
        #[serde(default)]
        start_s: Option<f64>,
        #[serde(default)]
        end_s: Option<f64>,
    },
    /// Inverter a fase (trocar o sinal).
    Invert {
        #[serde(default)]
        start_s: Option<f64>,
        #[serde(default)]
        end_s: Option<f64>,
    },
    Gain {
        gain_db: f64,
        #[serde(default)]
        start_s: Option<f64>,
        #[serde(default)]
        end_s: Option<f64>,
    },
    Resample { rate: u32 },
    Mono,
    Stereo,
    Remix { channels: usize },
    /// Muda a velocidade tocando mais rápido/devagar (a altura muda junto).
    Speed { factor: f64 },
    /// Aplica um efeito ao clipe ou ao intervalo. `profile_start_s`/`profile_end_s` marcam o trecho só com ruído de que
    /// a redução de ruído tira o perfil (se o efeito ainda não traz um).
    Fx {
        fx: AudioFx,
        #[serde(default)]
        start_s: Option<f64>,
        #[serde(default)]
        end_s: Option<f64>,
        #[serde(default)]
        profile_start_s: Option<f64>,
        #[serde(default)]
        profile_end_s: Option<f64>,
    },
    /// Aplica um rack (cadeia) ao clipe ou ao intervalo.
    Rack {
        rack: Rack,
        #[serde(default)]
        start_s: Option<f64>,
        #[serde(default)]
        end_s: Option<f64>,
    },
}

impl EditStep {
    pub fn name(&self) -> &'static str {
        match self {
            EditStep::Trim { .. } => "trim",
            EditStep::Delete { .. } => "delete",
            EditStep::Silence { .. } => "silence",
            EditStep::Duplicate { .. } => "duplicate",
            EditStep::InsertSilence { .. } => "insert_silence",
            EditStep::Fade { .. } => "fade",
            EditStep::Reverse { .. } => "reverse",
            EditStep::Invert { .. } => "invert",
            EditStep::Gain { .. } => "gain",
            EditStep::Resample { .. } => "resample",
            EditStep::Mono => "mono",
            EditStep::Stereo => "stereo",
            EditStep::Remix { .. } => "remix",
            EditStep::Speed { .. } => "speed",
            EditStep::Fx { .. } => "fx",
            EditStep::Rack { .. } => "rack",
        }
    }
}

fn invalid<T>(msg: impl Into<String>) -> AudioResult<T> {
    Err(AudioError::Invalid(msg.into()))
}

fn check_time(name: &str, v: f64) -> AudioResult<()> {
    if v.is_finite() && v >= 0.0 { Ok(()) } else { invalid(format!("{name} precisa ser um número >= 0 (recebi {v})")) }
}

/// Intervalo em quadros a partir de tempos opcionais. Início > fim ou intervalo totalmente fora do clipe é erro
/// (não vira um passo vazio sem aviso). Fim acima da duração é cortado na duração.
fn range_of(buf: &AudioBuf, start_s: Option<f64>, end_s: Option<f64>) -> AudioResult<Range<usize>> {
    if let Some(s) = start_s {
        check_time("start_s", s)?;
    }
    if let Some(e) = end_s {
        check_time("end_s", e)?;
    }
    let n = buf.frames();
    let a = start_s.map_or(0, |s| secs_to_frames(buf.rate, s));
    let b = end_s.map_or(n, |s| secs_to_frames(buf.rate, s)).min(n);
    if a > n {
        return invalid(format!("start_s ({:.3}) passa do fim do clipe ({:.3} s)", start_s.unwrap_or(0.0), buf.duration_s()));
    }
    if b < a {
        return invalid(format!("intervalo invertido: start_s {:.3} > end_s {:.3}", start_s.unwrap_or(0.0), end_s.unwrap_or(0.0)));
    }
    Ok(a..b)
}

fn required_range(buf: &AudioBuf, start_s: f64, end_s: f64, op: &str) -> AudioResult<Range<usize>> {
    let r = range_of(buf, Some(start_s), Some(end_s))?;
    if r.is_empty() {
        return invalid(format!("{op}: intervalo vazio ({start_s:.3} a {end_s:.3} s)"));
    }
    Ok(r)
}

/// Aplica um efeito a um intervalo: extrai, processa (a duração pode mudar) e recoloca.
fn apply_to_range(buf: &AudioBuf, r: Range<usize>, f: impl FnOnce(&AudioBuf) -> AudioBuf) -> AudioBuf {
    if r.start == 0 && r.end == buf.frames() {
        return f(buf);
    }
    let processed = f(&edit::copy(buf, r.clone()));
    let head = edit::copy(buf, 0..r.start);
    let tail = edit::copy(buf, r.end..buf.frames());
    let mut out = head;
    for (c, ch) in out.ch.iter_mut().enumerate() {
        ch.extend_from_slice(&processed.ch[c]);
        ch.extend_from_slice(&tail.ch[c]);
    }
    out
}

/// Aplica um passo. O buffer de entrada não é alterado.
pub fn apply_step(buf: &AudioBuf, step: &EditStep) -> AudioResult<AudioBuf> {
    if buf.channels() == 0 {
        return invalid("áudio sem canais");
    }
    let out = match step {
        EditStep::Trim { start_s, end_s } => edit::trim(buf, required_range(buf, *start_s, *end_s, "trim")?),
        EditStep::Delete { start_s, end_s } => edit::delete_range(buf, required_range(buf, *start_s, *end_s, "delete")?),
        EditStep::Silence { start_s, end_s } => {
            let mut o = buf.clone();
            edit::silence(&mut o, required_range(buf, *start_s, *end_s, "silence")?);
            o
        }
        EditStep::Duplicate { start_s, end_s, at_s } => {
            check_time("at_s", *at_s)?;
            let r = required_range(buf, *start_s, *end_s, "duplicate")?;
            let at = secs_to_frames(buf.rate, *at_s);
            if at > buf.frames() {
                return invalid(format!("at_s ({at_s:.3}) passa do fim do clipe ({:.3} s)", buf.duration_s()));
            }
            edit::insert(buf, at, &edit::copy(buf, r))
        }
        EditStep::InsertSilence { at_s, duration_s } => {
            check_time("at_s", *at_s)?;
            check_time("duration_s", *duration_s)?;
            let at = secs_to_frames(buf.rate, *at_s);
            if at > buf.frames() {
                return invalid(format!("at_s ({at_s:.3}) passa do fim do clipe ({:.3} s)", buf.duration_s()));
            }
            edit::insert_silence(buf, at, secs_to_frames(buf.rate, *duration_s))
        }
        EditStep::Fade { start_s, end_s, dir, curve } => {
            let mut o = buf.clone();
            edit::fade(&mut o, range_of(buf, *start_s, *end_s)?, *dir, *curve);
            o
        }
        EditStep::Reverse { start_s, end_s } => {
            let mut o = buf.clone();
            edit::reverse(&mut o, range_of(buf, *start_s, *end_s)?);
            o
        }
        EditStep::Invert { start_s, end_s } => {
            let mut o = buf.clone();
            edit::invert(&mut o, range_of(buf, *start_s, *end_s)?);
            o
        }
        EditStep::Gain { gain_db, start_s, end_s } => {
            if !gain_db.is_finite() {
                return invalid("gain_db precisa ser finito");
            }
            let mut o = buf.clone();
            edit::gain_range(&mut o, range_of(buf, *start_s, *end_s)?, *gain_db);
            o
        }
        EditStep::Resample { rate } => {
            if !(8_000..=384_000).contains(rate) {
                return invalid(format!("rate fica entre 8000 e 384000 (recebi {rate})"));
            }
            edit::resample_to(buf, *rate)
        }
        EditStep::Mono => edit::to_mono(buf),
        EditStep::Stereo => edit::to_stereo(buf),
        EditStep::Remix { channels } => {
            if !(1..=8).contains(channels) {
                return invalid(format!("channels fica entre 1 e 8 (recebi {channels})"));
            }
            edit::remix(buf, *channels)
        }
        EditStep::Speed { factor } => {
            if !(factor.is_finite() && (0.05..=20.0).contains(factor)) {
                return invalid(format!("factor fica entre 0.05 e 20 (recebi {factor})"));
            }
            edit::change_speed(buf, *factor)
        }
        EditStep::Fx { fx, start_s, end_s, profile_start_s, profile_end_s } => {
            let mut fx = fx.clone();
            if let AudioFx::NoiseReduction(nr) = &mut fx
                && nr.profile.is_none()
            {
                let (Some(a), Some(b)) = (profile_start_s, profile_end_s) else {
                    return invalid("noise_reduction exige profile ou profile_start_s + profile_end_s (um trecho só com ruído)");
                };
                let pr = required_range(buf, *a, *b, "profile")?;
                nr.profile = Some(
                    NoiseProfile::from_audio(&edit::copy(buf, pr), crate::fx::spectral::DEFAULT_FFT)
                        .ok_or_else(|| AudioError::Invalid("trecho do perfil de ruído curto demais (mínimo 2048 amostras)".into()))?,
                );
            }
            fx.validate().map_err(AudioError::Invalid)?;
            apply_to_range(buf, range_of(buf, *start_s, *end_s)?, |x| fx.render(x))
        }
        EditStep::Rack { rack, start_s, end_s } => {
            rack.validate().map_err(AudioError::Invalid)?;
            let mut rack = rack.clone();
            apply_to_range(buf, range_of(buf, *start_s, *end_s)?, |x| rack.render(x))
        }
    };
    if out.duration_s() > MAX_RESULT_SECONDS {
        return invalid(format!("o resultado de '{}' passaria de {MAX_RESULT_SECONDS} s ({:.0} s)", step.name(), out.duration_s()));
    }
    let mut out = out;
    out.sanitize();
    Ok(out)
}

/// Aplica os passos em ordem; o erro diz qual passo falhou.
pub fn apply_steps(buf: &AudioBuf, steps: &[EditStep]) -> AudioResult<AudioBuf> {
    let mut cur = buf.clone();
    for (i, s) in steps.iter().enumerate() {
        cur = apply_step(&cur, s).map_err(|e| AudioError::Invalid(format!("passo {i} ({}): {e}", s.name())))?;
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{Amplify, Compressor, Limiter, NoiseReduction, TimeStretch};
    use std::f64::consts::PI;

    fn sine(rate: u32, f: f64, secs: f64, amp: f32) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        AudioBuf::mono(rate, (0..n).map(|i| amp * (2.0 * PI * f * i as f64 / rate as f64).sin() as f32).collect())
    }

    fn parse(j: &str) -> Vec<EditStep> {
        serde_json::from_str(j).unwrap()
    }

    #[test]
    fn json_steps_apply_in_order() {
        let b = sine(1000, 5.0, 4.0, 0.5);
        let steps = parse(r#"[{"op":"trim","start_s":1,"end_s":3},{"op":"fade","dir":"out","curve":"s_curve"},{"op":"gain","gain_db":-6,"start_s":0,"end_s":1},{"op":"insert_silence","at_s":0,"duration_s":0.5}]"#);
        let o = apply_steps(&b, &steps).unwrap();
        assert_eq!(o.frames(), 2500);
        assert!(o.ch[0][..500].iter().all(|&s| s == 0.0));
        assert!(o.ch[0][2499].abs() < 1e-3, "o fade out zera o fim");
        // a fonte não mudou
        assert_eq!(b.frames(), 4000);
        let d = apply_step(&b, &parse(r#"[{"op":"duplicate","start_s":0,"end_s":1,"at_s":4}]"#)[0]).unwrap();
        assert_eq!(d.frames(), 5000);
        assert_eq!(d.ch[0][4000..4100], b.ch[0][..100]);
        assert_eq!(apply_step(&b, &EditStep::Delete { start_s: 1.0, end_s: 2.0 }).unwrap().frames(), 3000);
        assert!(apply_step(&b, &EditStep::Silence { start_s: 1.0, end_s: 2.0 }).unwrap().ch[0][1500..1600].iter().all(|&s| s == 0.0));
        assert_eq!(apply_step(&b, &EditStep::Stereo).unwrap().channels(), 2);
        assert_eq!(apply_step(&b, &EditStep::Resample { rate: 8000 }).unwrap().frames(), 32_000);
        assert_eq!(apply_step(&b, &EditStep::Speed { factor: 2.0 }).unwrap().frames(), 2000);
    }

    #[test]
    fn strict_parsing_and_range_errors() {
        assert!(serde_json::from_str::<Vec<EditStep>>(r#"[{"op":"trim","start_s":1,"end_s":2,"extra":1}]"#).is_err());
        assert!(serde_json::from_str::<Vec<EditStep>>(r#"[{"op":"trim","start_s":1}]"#).is_err());
        assert!(serde_json::from_str::<Vec<EditStep>>(r#"[{"op":"nada"}]"#).is_err());
        let b = sine(1000, 5.0, 2.0, 0.5);
        for bad in [
            EditStep::Trim { start_s: 1.5, end_s: 1.0 },
            EditStep::Delete { start_s: 3.0, end_s: 4.0 },
            EditStep::Silence { start_s: 1.0, end_s: 1.0 },
            EditStep::Trim { start_s: -1.0, end_s: 1.0 },
            EditStep::Trim { start_s: f64::NAN, end_s: 1.0 },
            EditStep::Duplicate { start_s: 0.0, end_s: 1.0, at_s: 9.0 },
            EditStep::Resample { rate: 5 },
            EditStep::Speed { factor: 0.0 },
            EditStep::Remix { channels: 0 },
        ] {
            assert!(apply_step(&b, &bad).is_err(), "{bad:?}");
        }
        // o erro de uma lista diz qual passo
        let e = apply_steps(&b, &[EditStep::Mono, EditStep::Trim { start_s: 5.0, end_s: 6.0 }]).unwrap_err().to_string();
        assert!(e.contains("passo 1") && e.contains("trim"), "{e}");
        // fim além da duração é cortado, não erro
        assert_eq!(apply_step(&b, &EditStep::Trim { start_s: 1.0, end_s: 99.0 }).unwrap().frames(), 1000);
    }

    #[test]
    fn fx_on_range_and_whole_clip() {
        let fs = 8000;
        let b = sine(fs, 440.0, 2.0, 0.1);
        // ganho de 20 dB só de 0,5 a 1,5 s
        let o = apply_step(&b, &EditStep::Fx { fx: AudioFx::Amplify(Amplify { gain_db: 20.0, ..Default::default() }), start_s: Some(0.5), end_s: Some(1.5), profile_start_s: None, profile_end_s: None }).unwrap();
        assert_eq!(o.frames(), b.frames());
        assert_eq!(o.ch[0][..4000], b.ch[0][..4000]);
        assert!((o.ch[0][4100..4300].iter().fold(0.0f32, |m, v| m.max(v.abs())) / b.ch[0][4100..4300].iter().fold(0.0f32, |m, v| m.max(v.abs())) - 10.0).abs() < 0.01);
        assert_eq!(o.ch[0][12_000..], b.ch[0][12_000..]);
        // esticar o tempo de um intervalo muda a duração total
        let t = apply_step(&b, &EditStep::Fx { fx: AudioFx::TimeStretch(TimeStretch { ratio: 2.0, ..Default::default() }), start_s: Some(0.0), end_s: Some(1.0), profile_start_s: None, profile_end_s: None }).unwrap();
        assert!((t.frames() as i64 - 24_000).abs() <= 3, "{}", t.frames());
        // rack no clipe todo
        let mut rack = Rack::new();
        rack.push(AudioFx::Compressor(Compressor::default())).push(AudioFx::Limiter(Limiter { ceiling_db: -6.0, ..Default::default() }));
        let r = apply_step(&sine(fs, 440.0, 1.0, 0.9), &EditStep::Rack { rack, start_s: None, end_s: None }).unwrap();
        assert!(r.peak() <= 0.51, "{}", r.peak());
        // efeito inválido recusado antes de rodar
        let bad = EditStep::Fx { fx: AudioFx::Compressor(Compressor { ratio: 0.2, ..Default::default() }), start_s: None, end_s: None, profile_start_s: None, profile_end_s: None };
        assert!(apply_step(&b, &bad).unwrap_err().to_string().contains("ratio"));
    }

    #[test]
    fn noise_reduction_profile_from_range() {
        let fs = 44_100;
        let mut rng = crate::rng::Rng::new(4);
        let n = (fs as f64 * 4.0) as usize;
        let mut x: Vec<f32> = (0..n).map(|_| rng.bipolar() * 0.02).collect();
        // 1 s de ruído, depois 2 s de tom + ruído, depois 1 s de ruído
        for i in fs as usize..3 * fs as usize {
            x[i] += 0.3 * (2.0 * PI * 1000.0 * i as f64 / fs as f64).sin() as f32;
        }
        let b = AudioBuf::mono(fs, x);
        let step = |ps: Option<f64>, pe: Option<f64>| EditStep::Fx { fx: AudioFx::NoiseReduction(NoiseReduction { reduction_db: 30.0, sensitivity: 3.0, ..Default::default() }), start_s: None, end_s: None, profile_start_s: ps, profile_end_s: pe };
        assert!(apply_step(&b, &step(None, None)).is_err());
        assert!(apply_step(&b, &step(Some(0.0), Some(0.01))).is_err(), "perfil curto demais");
        let o = apply_step(&b, &step(Some(0.0), Some(0.9))).unwrap();
        let rms = |v: &[f32]| (v.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        let tail = |a: &AudioBuf| rms(&a.ch[0][(3.3 * fs as f64) as usize..(3.9 * fs as f64) as usize]);
        let red = 20.0 * (tail(&o) / tail(&b)).log10();
        assert!(red < -12.0, "{red}");
        assert_eq!(o.frames(), b.frames());
    }
}
