//! Operações de edição de clipe sobre `AudioBuf` (planar, f32, qualquer taxa e nº de canais). Tudo devolve buffers novos
//! ou altera o recebido em um intervalo de quadros: não há estado global, então o histórico (desfazer) fica com quem chama.
//!
//! Intervalos são `Range<usize>` em quadros e são cortados nos limites do buffer (início > fim vira intervalo vazio).

use crate::buf::AudioBuf;
use crate::resample::resample;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::ops::Range;

/// Segundos → quadros (arredonda; negativo vira 0).
pub fn secs_to_frames(rate: u32, s: f64) -> usize {
    (s * rate as f64).round().max(0.0) as usize
}

/// Corta `r` nos limites de um buffer de `len` quadros.
pub fn clamp_range(len: usize, r: Range<usize>) -> Range<usize> {
    let a = r.start.min(len);
    let b = r.end.min(len).max(a);
    a..b
}

// ---------------------------------------------------------------- layout entrelaçado (o formato dos efeitos)

/// Planar → entrelaçado (`L R L R ...`).
pub fn interleave(buf: &AudioBuf) -> Vec<f32> {
    let (n, nch) = (buf.frames(), buf.channels());
    let mut out = vec![0.0f32; n * nch];
    for (c, x) in buf.ch.iter().enumerate() {
        for (i, &s) in x.iter().enumerate() {
            out[i * nch + c] = s;
        }
    }
    out
}

/// Entrelaçado → planar. Sobras (menos que um quadro inteiro) são ignoradas.
pub fn deinterleave(data: &[f32], channels: usize, rate: u32) -> AudioBuf {
    let nch = channels.max(1);
    let n = data.len() / nch;
    let mut ch = vec![Vec::with_capacity(n); nch];
    for i in 0..n {
        for (c, v) in ch.iter_mut().enumerate() {
            v.push(data[i * nch + c]);
        }
    }
    AudioBuf { rate, ch }
}

// ---------------------------------------------------------------- cortar, copiar, colar

/// Copia o intervalo (a área de transferência do editor).
pub fn copy(buf: &AudioBuf, r: Range<usize>) -> AudioBuf {
    let r = clamp_range(buf.frames(), r);
    buf.slice(r.start, r.len())
}

/// Aparar: fica só com o intervalo.
pub fn trim(buf: &AudioBuf, r: Range<usize>) -> AudioBuf {
    copy(buf, r)
}

/// Apaga o intervalo e fecha o buraco (o que vem depois sobe).
pub fn delete_range(buf: &AudioBuf, r: Range<usize>) -> AudioBuf {
    let r = clamp_range(buf.frames(), r);
    let ch = buf
        .ch
        .iter()
        .map(|c| {
            let mut v = Vec::with_capacity(c.len() - r.len());
            v.extend_from_slice(&c[..r.start]);
            v.extend_from_slice(&c[r.end..]);
            v
        })
        .collect();
    AudioBuf { rate: buf.rate, ch }
}

/// Recortar: `(restante, trecho)`.
pub fn cut(buf: &AudioBuf, r: Range<usize>) -> (AudioBuf, AudioBuf) {
    (delete_range(buf, r.clone()), copy(buf, r))
}

/// Ajusta `clip` à taxa e ao nº de canais de destino (reamostra e remixa).
pub fn conform(clip: &AudioBuf, rate: u32, channels: usize) -> AudioBuf {
    let r = if clip.rate == rate { clip.clone() } else { resample(clip, rate) };
    remix(&r, channels)
}

/// Cola `clip` na posição `at` (empurra o resto para frente). `clip` é adaptado à taxa/canais de `buf`.
pub fn insert(buf: &AudioBuf, at: usize, clip: &AudioBuf) -> AudioBuf {
    let clip = conform(clip, buf.rate, buf.channels());
    let at = at.min(buf.frames());
    let ch = buf
        .ch
        .iter()
        .zip(&clip.ch)
        .map(|(b, c)| {
            let mut v = Vec::with_capacity(b.len() + c.len());
            v.extend_from_slice(&b[..at]);
            v.extend_from_slice(c);
            v.extend_from_slice(&b[at..]);
            v
        })
        .collect();
    AudioBuf { rate: buf.rate, ch }
}

/// Insere silêncio de `frames` quadros em `at`.
pub fn insert_silence(buf: &AudioBuf, at: usize, frames: usize) -> AudioBuf {
    insert(buf, at, &AudioBuf::zeros(buf.rate, buf.channels(), frames))
}

/// Silenciar o intervalo (mantém a duração).
pub fn silence(buf: &mut AudioBuf, r: Range<usize>) {
    let r = clamp_range(buf.frames(), r);
    for c in &mut buf.ch {
        c[r.clone()].fill(0.0);
    }
}

/// Ganho (dB) só no intervalo.
pub fn gain_range(buf: &mut AudioBuf, r: Range<usize>, db: f64) {
    let r = clamp_range(buf.frames(), r);
    let g = crate::db_to_lin(db) as f32;
    for c in &mut buf.ch {
        for s in &mut c[r.clone()] {
            *s *= g;
        }
    }
}

/// Inverte a ordem das amostras do intervalo.
pub fn reverse(buf: &mut AudioBuf, r: Range<usize>) {
    let r = clamp_range(buf.frames(), r);
    for c in &mut buf.ch {
        c[r.clone()].reverse();
    }
}

/// Troca o sinal (inverte a fase) no intervalo.
pub fn invert(buf: &mut AudioBuf, r: Range<usize>) {
    let r = clamp_range(buf.frames(), r);
    for c in &mut buf.ch {
        for s in &mut c[r.clone()] {
            *s = -*s;
        }
    }
}

// ---------------------------------------------------------------- fades

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FadeCurve {
    /// Ganho linear em amplitude.
    #[default]
    Linear,
    /// Logarítmica: linear em dB (60 dB de faixa), soa como rampa natural de volume.
    Log,
    /// Curva S (smoothstep): começa e termina suave.
    SCurve,
    /// Potência constante (seno): a soma de duas rampas espelhadas mantém a energia (crossfade de sons descorrelacionados).
    EqualPower,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FadeDir {
    #[default]
    In,
    Out,
}

/// Ganho de uma rampa de entrada em `t` de 0 a 1 (0 = mudo, 1 = cheio).
pub fn fade_gain(curve: FadeCurve, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match curve {
        FadeCurve::Linear => t,
        FadeCurve::Log => {
            let floor = 10f64.powf(-3.0);
            (10f64.powf(3.0 * (t - 1.0)) - floor) / (1.0 - floor)
        }
        FadeCurve::SCurve => t * t * (3.0 - 2.0 * t),
        FadeCurve::EqualPower => (t * PI / 2.0).sin(),
    }
}

/// Fade in/out sobre o intervalo. O ganho da rampa vai de 0 a 1 (in) ou de 1 a 0 (out) ao longo de todo o intervalo.
pub fn fade(buf: &mut AudioBuf, r: Range<usize>, dir: FadeDir, curve: FadeCurve) {
    let r = clamp_range(buf.frames(), r);
    let n = r.len();
    if n == 0 {
        return;
    }
    let den = (n.max(2) - 1) as f64;
    let g: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f64 / den;
            fade_gain(curve, if dir == FadeDir::In { t } else { 1.0 - t }) as f32
        })
        .collect();
    for c in &mut buf.ch {
        for (s, g) in c[r.clone()].iter_mut().zip(&g) {
            *s *= g;
        }
    }
}

/// Une `a` e `b` sobrepondo `overlap` quadros: o fim de `a` desce e o começo de `b` sobe (rampas espelhadas da curva).
/// A duração final é `a + b - overlap`. `b` é adaptado ao formato de `a`; a sobreposição é limitada ao menor dos dois.
pub fn crossfade(a: &AudioBuf, b: &AudioBuf, overlap: usize, curve: FadeCurve) -> AudioBuf {
    let b = conform(b, a.rate, a.channels());
    let ov = overlap.min(a.frames()).min(b.frames());
    let (na, nb) = (a.frames(), b.frames());
    let den = (ov.max(2) - 1) as f64;
    let ch = a
        .ch
        .iter()
        .zip(&b.ch)
        .map(|(x, y)| {
            let mut out = Vec::with_capacity(na + nb - ov);
            out.extend_from_slice(&x[..na - ov]);
            for i in 0..ov {
                let t = i as f64 / den;
                out.push((x[na - ov + i] as f64 * fade_gain(curve, 1.0 - t) + y[i] as f64 * fade_gain(curve, t)) as f32);
            }
            out.extend_from_slice(&y[ov..]);
            out
        })
        .collect();
    AudioBuf { rate: a.rate, ch }
}

// ---------------------------------------------------------------- canais e taxa

/// Converte para `channels` canais: mono = média de todos; mais canais a partir de menos = repete os existentes em ciclo;
/// de N para M < N (M >= 2) mantém os M primeiros.
pub fn remix(buf: &AudioBuf, channels: usize) -> AudioBuf {
    let channels = channels.max(1);
    if buf.channels() == channels {
        return buf.clone();
    }
    if buf.channels() == 0 {
        return AudioBuf::zeros(buf.rate, channels, 0);
    }
    if channels == 1 {
        return buf.to_mono();
    }
    let ch = (0..channels).map(|c| buf.ch[c % buf.channels()].clone()).collect();
    AudioBuf { rate: buf.rate, ch }
}

pub fn to_mono(buf: &AudioBuf) -> AudioBuf {
    remix(buf, 1)
}

pub fn to_stereo(buf: &AudioBuf) -> AudioBuf {
    remix(buf, 2)
}

/// Reamostra para `rate` (sinc com janela Kaiser, ~100 dB de rejeição).
pub fn resample_to(buf: &AudioBuf, rate: u32) -> AudioBuf {
    resample(buf, rate)
}

/// Aproximação racional `num/den` de `x` com `den <= max_den` (busca direta; suficiente para razões de áudio).
pub(crate) fn rational(x: f64, max_den: u32) -> (u32, u32) {
    let mut best = (1u32, 1u32, f64::INFINITY);
    for den in 1..=max_den {
        let num = (x * den as f64).round().max(1.0) as u32;
        let err = (num as f64 / den as f64 - x).abs();
        if err < best.2 - 1e-12 {
            best = (num, den, err);
        }
        if err < 1e-7 {
            break;
        }
    }
    (best.0, best.1)
}

/// Mudar velocidade: toca `factor` vezes mais rápido (2.0 = metade da duração e uma oitava acima), mantendo a taxa.
pub fn change_speed(buf: &AudioBuf, factor: f64) -> AudioBuf {
    let f = factor.clamp(0.05, 20.0);
    if (f - 1.0).abs() < 1e-9 || buf.frames() == 0 {
        return buf.clone();
    }
    // duração nova = duração / f: trata o buffer como gravado a `rate·num/den` e reamostra de volta para `rate`.
    let (num, den) = rational(f, 512);
    let tmp = AudioBuf { rate: buf.rate * num, ch: buf.ch.clone() };
    let mut out = resample(&tmp, buf.rate * den);
    out.rate = buf.rate;
    out
}

/// Cruzamento por zero mais próximo de `frame` (até `search` quadros para cada lado), útil para cortar sem estalo (`Z`).
/// Usa a soma dos canais; sem cruzamento na janela devolve o próprio `frame`.
pub fn nearest_zero_crossing(buf: &AudioBuf, frame: usize, search: usize) -> usize {
    let n = buf.frames();
    if n < 2 {
        return frame.min(n);
    }
    let m = |i: usize| -> f32 { buf.ch.iter().map(|c| c[i]).sum::<f32>() };
    let frame = frame.min(n - 1);
    for d in 0..=search {
        for cand in [frame.checked_sub(d), Some(frame + d)].into_iter().flatten() {
            if cand + 1 < n && (m(cand) <= 0.0) != (m(cand + 1) <= 0.0) {
                return cand;
            }
        }
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, f: f64, secs: f64, amp: f32) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        AudioBuf::mono(rate, (0..n).map(|i| amp * (2.0 * PI * f * i as f64 / rate as f64).sin() as f32).collect())
    }

    fn ramp(n: usize) -> AudioBuf {
        AudioBuf::stereo(1000, (0..n).map(|i| i as f32).collect(), (0..n).map(|i| -(i as f32)).collect())
    }

    #[test]
    fn interleave_roundtrip() {
        let b = ramp(10);
        let d = interleave(&b);
        assert_eq!(&d[..4], &[0.0, 0.0, 1.0, -1.0]);
        assert_eq!(deinterleave(&d, 2, 1000), b);
    }

    #[test]
    fn cut_copy_delete_trim_insert() {
        let b = ramp(10);
        assert_eq!(copy(&b, 2..5).ch[0], vec![2.0, 3.0, 4.0]);
        assert_eq!(trim(&b, 8..100).ch[1], vec![-8.0, -9.0]);
        assert_eq!(delete_range(&b, 2..8).ch[0], vec![0.0, 1.0, 8.0, 9.0]);
        let (rest, clip) = cut(&b, 0..3);
        assert_eq!((rest.frames(), clip.frames()), (7, 3));
        // intervalo invertido ou fora: vazio, nada some
        assert_eq!(delete_range(&b, 7..3).frames(), 10);
        assert_eq!(copy(&b, 50..60).frames(), 0);
        // colar: o resto empurra
        let p = insert(&b, 5, &copy(&b, 0..2));
        assert_eq!(p.ch[0], vec![0.0, 1.0, 2.0, 3.0, 4.0, 0.0, 1.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        // colar mono em estéreo e em outra taxa: adapta
        let m = sine(2000, 100.0, 0.1, 0.5);
        let p = insert(&b, 0, &m);
        assert_eq!((p.channels(), p.rate), (2, 1000));
        assert!((p.frames() as i64 - 110).abs() <= 1, "{}", p.frames());
        assert_eq!(insert_silence(&b, 3, 5).frames(), 15);
        assert_eq!(insert_silence(&b, 3, 5).ch[0][3..8], [0.0; 5]);
    }

    #[test]
    fn silence_gain_reverse_invert() {
        let mut b = AudioBuf::mono(1000, vec![1.0; 10]);
        silence(&mut b, 2..4);
        assert_eq!(b.ch[0][..5], [1.0, 1.0, 0.0, 0.0, 1.0]);
        gain_range(&mut b, 0..2, -6.0206);
        assert!((b.ch[0][0] - 0.5).abs() < 1e-3 && (b.ch[0][4] - 1.0).abs() < 1e-6);
        let mut r = ramp(6);
        reverse(&mut r, 1..5);
        assert_eq!(r.ch[0], vec![0.0, 4.0, 3.0, 2.0, 1.0, 5.0]);
        invert(&mut r, 0..6);
        assert_eq!(r.ch[0][1], -4.0);
    }

    #[test]
    fn fades_have_expected_shape() {
        for curve in [FadeCurve::Linear, FadeCurve::Log, FadeCurve::SCurve, FadeCurve::EqualPower] {
            let mut b = AudioBuf::mono(1000, vec![1.0; 101]);
            fade(&mut b, 0..101, FadeDir::In, curve);
            let x = &b.ch[0];
            assert!(x[0].abs() < 1e-6, "{curve:?} começa mudo");
            assert!((x[100] - 1.0).abs() < 1e-6, "{curve:?} termina cheio");
            assert!(x.windows(2).all(|w| w[1] >= w[0] - 1e-6), "{curve:?} monótona");
            let mut o = AudioBuf::mono(1000, vec![1.0; 101]);
            fade(&mut o, 0..101, FadeDir::Out, curve);
            assert!((o.ch[0][0] - 1.0).abs() < 1e-6 && o.ch[0][100].abs() < 1e-6);
        }
        // meio da rampa: linear 0,5; log bem abaixo (-30 dB); S = 0,5; potência constante = sen 45 graus
        assert!((fade_gain(FadeCurve::Linear, 0.5) - 0.5).abs() < 1e-12);
        assert!(fade_gain(FadeCurve::Log, 0.5) < 0.05);
        assert!((fade_gain(FadeCurve::SCurve, 0.5) - 0.5).abs() < 1e-12);
        assert!((fade_gain(FadeCurve::EqualPower, 0.5) - 0.5f64.sqrt()).abs() < 1e-12);
        // só o intervalo muda
        let mut b = AudioBuf::mono(1000, vec![1.0; 100]);
        fade(&mut b, 50..100, FadeDir::Out, FadeCurve::Linear);
        assert!(b.ch[0][..50].iter().all(|&s| s == 1.0) && b.ch[0][99].abs() < 1e-6);
    }

    #[test]
    fn crossfade_length_and_level() {
        let a = AudioBuf::mono(1000, vec![1.0; 500]);
        let b = AudioBuf::mono(1000, vec![1.0; 500]);
        assert_eq!(crossfade(&a, &b, 100, FadeCurve::Linear).frames(), 900);
        // sinais iguais com rampas lineares ou S: amplitude constante na emenda
        for curve in [FadeCurve::Linear, FadeCurve::SCurve] {
            let x = crossfade(&a, &b, 100, curve);
            assert!(x.ch[0].iter().all(|&s| (s - 1.0).abs() < 1e-5), "{curve:?}");
        }
        // ruídos descorrelacionados: potência constante mantém a energia, a linear cai ~3 dB no meio
        let mut rng = crate::rng::Rng::new(5);
        let n1 = AudioBuf::mono(1000, (0..4000).map(|_| rng.bipolar()).collect());
        let n2 = AudioBuf::mono(1000, (0..4000).map(|_| rng.bipolar()).collect());
        let mid_rms = |c: FadeCurve| {
            let x = crossfade(&n1, &n2, 2000, c);
            let s = &x.ch[0][2000 + 800..2000 + 1200];
            (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
        };
        let (rl, re) = (mid_rms(FadeCurve::Linear), mid_rms(FadeCurve::EqualPower));
        assert!(re > rl * 1.2, "potência constante ({re}) deve ficar acima da linear ({rl})");
        // overlap maior que o clipe curto: limita
        assert_eq!(crossfade(&a, &AudioBuf::mono(1000, vec![1.0; 10]), 100, FadeCurve::Linear).frames(), 500);
    }

    #[test]
    fn remix_channels() {
        let st = AudioBuf::stereo(1000, vec![1.0, 1.0], vec![0.0, -1.0]);
        assert_eq!(to_mono(&st).ch[0], vec![0.5, 0.0]);
        let m = to_stereo(&to_mono(&st));
        assert_eq!(m.channels(), 2);
        assert_eq!(m.ch[0], m.ch[1]);
        assert_eq!(remix(&st, 4).channels(), 4);
        assert_eq!(remix(&st, 4).ch[2], st.ch[0]);
    }

    #[test]
    fn resample_and_speed() {
        let b = sine(44_100, 1000.0, 1.0, 0.5);
        let r = resample_to(&b, 48_000);
        assert_eq!(r.rate, 48_000);
        assert!((r.frames() as i64 - 48_000).abs() <= 1);
        // velocidade 2x: metade da duração, taxa mantida
        let s = change_speed(&b, 2.0);
        assert_eq!(s.rate, 44_100);
        assert!((s.frames() as i64 - 22_050).abs() <= 2, "{}", s.frames());
        // e uma oitava acima: ~2000 Hz (cruzamentos por zero)
        let x = &s.ch[0][2000..20_000];
        let zc = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64;
        let f = zc / (x.len() as f64 / 44_100.0);
        assert!((f - 2000.0).abs() < 20.0, "{f}");
        assert_eq!(change_speed(&b, 1.0).frames(), b.frames());
    }

    #[test]
    fn zero_crossing_snap() {
        let b = sine(1000, 10.0, 1.0, 1.0);
        let z = nearest_zero_crossing(&b, 27, 50);
        assert!(z <= 80 && (b.ch[0][z] <= 0.0) != (b.ch[0][z + 1] <= 0.0));
        // sem cruzamento: devolve o ponto
        assert_eq!(nearest_zero_crossing(&AudioBuf::mono(1000, vec![1.0; 100]), 40, 10), 40);
    }
}
