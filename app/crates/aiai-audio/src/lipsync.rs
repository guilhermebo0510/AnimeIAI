//! Lip-sync: tempos de moras (VOICEVOX) ou envelope de energia (qualquer áudio) → forma de boca por quadro → exposições
//! (quadro, cel) para a coluna de boca do timesheet.
//!
//! Mapeamento vogal → forma (documentado): a→A, i→I, u→U, e→E, o→O, ん (N) e っ (cl) → N (boca fechada), pausa/silêncio → Rest.
//! Modo 3 formas ("anime"): A, O → Open; I, U, E → Half; N, Rest → Closed (Rest). Vogal desvozeada mantém a forma.
//!
//! Regras de suavização (nessa ordem, sobre a sequência de quadros): (1) pausas de até `close_gap_frames` (2) entre fala
//! são "pontes" e seguram a forma anterior — só fecha em pausa maior que isso; (2) cada forma dura no mínimo `min_hold_frames`
//! (2): um quadro isolado é absorvido pelo vizinho de fala mais longo (nunca por silêncio); (3) quadros iguais
//! seguidos viram uma exposição só.

use crate::buf::AudioBuf;
use crate::timing::{Timings, VowelClass};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MouthShape {
    Rest,
    N,
    A,
    I,
    U,
    E,
    O,
    Half,
    Open,
}

impl MouthShape {
    pub fn is_rest(self) -> bool {
        self == MouthShape::Rest
    }

    pub fn name(self) -> &'static str {
        match self {
            MouthShape::Rest => "rest",
            MouthShape::N => "n",
            MouthShape::A => "a",
            MouthShape::I => "i",
            MouthShape::U => "u",
            MouthShape::E => "e",
            MouthShape::O => "o",
            MouthShape::Half => "half",
            MouthShape::Open => "open",
        }
    }

    /// 7 formas → 3 formas (fechada / meia / aberta).
    pub fn reduce3(self) -> MouthShape {
        match self {
            MouthShape::A | MouthShape::O | MouthShape::Open => MouthShape::Open,
            MouthShape::I | MouthShape::U | MouthShape::E | MouthShape::Half => MouthShape::Half,
            MouthShape::N | MouthShape::Rest => MouthShape::Rest,
        }
    }
}

pub fn shape_of_class(c: VowelClass) -> MouthShape {
    match c {
        VowelClass::A => MouthShape::A,
        VowelClass::I => MouthShape::I,
        VowelClass::U => MouthShape::U,
        VowelClass::E => MouthShape::E,
        VowelClass::O => MouthShape::O,
        VowelClass::N | VowelClass::Cl => MouthShape::N,
        VowelClass::Pau => MouthShape::Rest,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShapeSet {
    /// a, i, u, e, o, n (fechada) e repouso.
    #[default]
    Seven,
    /// fechada / meia / aberta (estilo TV).
    Three,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LipsyncOptions {
    pub fps: f64,
    #[serde(default = "d_two")]
    pub min_hold_frames: u32,
    /// Pausas de até este número de quadros não fecham a boca.
    #[serde(default = "d_two")]
    pub close_gap_frames: u32,
    #[serde(default)]
    pub shapes: ShapeSet,
    /// Adianta a boca em relação ao som (s). Anime costuma abrir ~1 quadro antes; 0 = alinhado ao som.
    #[serde(default)]
    pub lead_s: f64,
}

fn d_two() -> u32 {
    2
}

impl LipsyncOptions {
    pub fn new(fps: f64) -> Self {
        LipsyncOptions { fps, min_hold_frames: 2, close_gap_frames: 2, shapes: ShapeSet::Seven, lead_s: 0.0 }
    }
}

/// Forma de boca por quadro a partir das moras. O quadro `f` cobre `[f/fps, (f+1)/fps)` e recebe a forma da mora que contém
/// o seu centro. `frames` = número de quadros (omitido: até o fim dos tempos).
pub fn frames_from_moras(t: &Timings, frames: Option<usize>, o: &LipsyncOptions) -> Vec<MouthShape> {
    let n = frames.unwrap_or_else(|| (t.duration * o.fps).ceil() as usize);
    let mut out = vec![MouthShape::Rest; n];
    let mut mi = 0usize;
    for (f, slot) in out.iter_mut().enumerate() {
        let tc = (f as f64 + 0.5) / o.fps + o.lead_s;
        while mi < t.moras.len() && t.moras[mi].end <= tc {
            mi += 1;
        }
        if let Some(m) = t.moras.get(mi) {
            if m.start <= tc {
                *slot = shape_of_class(m.class);
            }
        }
    }
    finish(out, o)
}

/// Fallback por energia (áudio sem fonemas): RMS por quadro em dB → fechada / meia / aberta. Limiares relativos ao nível de
/// fala (percentil 90 dos quadros audíveis) com piso absoluto em -55 dBFS.
pub fn frames_from_energy(audio: &AudioBuf, o: &LipsyncOptions) -> Vec<MouthShape> {
    let mono = audio.to_mono();
    let x = &mono.ch[0];
    let spf = mono.rate as f64 / o.fps;
    let n = (x.len() as f64 / spf).ceil() as usize;
    let mut level = Vec::with_capacity(n);
    for f in 0..n {
        let a = ((f as f64 + o.lead_s * o.fps) * spf).max(0.0) as usize;
        let b = (((f as f64 + 1.0 + o.lead_s * o.fps) * spf) as usize).min(x.len());
        let a = a.min(b);
        let e = if b > a { x[a..b].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / (b - a) as f64 } else { 0.0 };
        level.push(10.0 * e.max(1e-12).log10());
    }
    let mut audible: Vec<f64> = level.iter().copied().filter(|&l| l > -55.0).collect();
    audible.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ref_db = if audible.is_empty() { 0.0 } else { audible[((audible.len() - 1) as f64 * 0.9) as usize] };
    let closed_thr = (ref_db - 22.0).max(-55.0);
    let open_thr = ref_db - 7.0;
    let shapes: Vec<MouthShape> = level
        .iter()
        .map(|&l| {
            if l < closed_thr {
                MouthShape::Rest
            } else if l >= open_thr {
                MouthShape::Open
            } else {
                MouthShape::Half
            }
        })
        .collect();
    finish(shapes, &LipsyncOptions { shapes: ShapeSet::Three, ..*o })
}

fn finish(mut f: Vec<MouthShape>, o: &LipsyncOptions) -> Vec<MouthShape> {
    if o.shapes == ShapeSet::Three {
        for s in &mut f {
            *s = s.reduce3();
        }
    }
    smooth(f, o.min_hold_frames.max(1) as usize, o.close_gap_frames as usize)
}

fn runs(f: &[MouthShape]) -> Vec<(MouthShape, usize)> {
    let mut r: Vec<(MouthShape, usize)> = Vec::new();
    for &s in f {
        match r.last_mut() {
            Some((p, n)) if *p == s => *n += 1,
            _ => r.push((s, 1)),
        }
    }
    r
}

fn flatten(r: &[(MouthShape, usize)]) -> Vec<MouthShape> {
    r.iter().flat_map(|&(s, n)| std::iter::repeat_n(s, n)).collect()
}

fn merge_equal(r: &mut Vec<(MouthShape, usize)>) {
    let mut i = 0;
    while i + 1 < r.len() {
        if r[i].0 == r[i + 1].0 {
            r[i].1 += r[i + 1].1;
            r.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

/// Suavização por regras (ver o topo do módulo).
pub fn smooth(frames: Vec<MouthShape>, min_hold: usize, close_gap: usize) -> Vec<MouthShape> {
    let mut r = runs(&frames);
    // (1) pausas curtas entre fala seguram a forma anterior.
    for i in 1..r.len().saturating_sub(1) {
        if r[i].0.is_rest() && r[i].1 <= close_gap && !r[i - 1].0.is_rest() && !r[i + 1].0.is_rest() {
            r[i].0 = r[i - 1].0;
        }
    }
    merge_equal(&mut r);
    // (2) duração mínima.
    for _ in 0..(r.len() + 8) * 2 {
        let Some(i) = (0..r.len()).find(|&i| r[i].1 < min_hold && !(r[i].0.is_rest() && (i == 0 || i + 1 == r.len()))) else {
            break;
        };
        let prev = i.checked_sub(1);
        let next = if i + 1 < r.len() { Some(i + 1) } else { None };
        let target = match (prev, next) {
            (Some(p), Some(n)) => {
                let (pr, nr) = (r[p].0.is_rest(), r[n].0.is_rest());
                if pr && !nr {
                    n
                } else if nr && !pr {
                    p
                } else if r[n].1 > r[p].1 {
                    n
                } else {
                    p
                }
            }
            (Some(p), None) => p,
            (None, Some(n)) => n,
            (None, None) => break,
        };
        // Forma de fala curta entre dois silêncios vira silêncio; caso contrário adota a do vizinho escolhido.
        let adopt = r[target].0;
        r[i].0 = adopt;
        merge_equal(&mut r);
    }
    merge_equal(&mut r);
    flatten(&r)
}

/// Números de cel para cada forma. Formas ausentes caem para a mais próxima definida.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MouthMap {
    #[serde(default)]
    pub a: Option<u32>,
    #[serde(default)]
    pub i: Option<u32>,
    #[serde(default)]
    pub u: Option<u32>,
    #[serde(default)]
    pub e: Option<u32>,
    #[serde(default)]
    pub o: Option<u32>,
    /// ん / っ.
    #[serde(default)]
    pub n: Option<u32>,
    /// Repouso/boca fechada.
    #[serde(default, alias = "closed")]
    pub rest: Option<u32>,
    #[serde(default)]
    pub half: Option<u32>,
    #[serde(default)]
    pub open: Option<u32>,
}

impl MouthMap {
    /// Mapa a partir de pares nome→cel (`{"a":3,"closed":1}`).
    pub fn from_pairs(m: &BTreeMap<String, u32>) -> MouthMap {
        let g = |k: &str| m.get(k).copied();
        MouthMap { a: g("a"), i: g("i"), u: g("u"), e: g("e"), o: g("o"), n: g("n"), rest: g("rest").or(g("closed")), half: g("half"), open: g("open") }
    }

    pub fn cel(&self, s: MouthShape) -> Option<u32> {
        use MouthShape::*;
        let chain: &[Option<u32>] = match s {
            Rest => &[self.rest, self.n],
            N => &[self.n, self.rest],
            A => &[self.a, self.open, self.o, self.half],
            O => &[self.o, self.open, self.a, self.half],
            I => &[self.i, self.half, self.e, self.u, self.open],
            U => &[self.u, self.half, self.o, self.i, self.open],
            E => &[self.e, self.half, self.i, self.open],
            Half => &[self.half, self.e, self.i, self.u, self.open],
            Open => &[self.open, self.a, self.o, self.half],
        };
        chain.iter().flatten().copied().next()
    }
}

/// Exposições (quadro absoluto, número do cel) só nas mudanças. `start_frame` desloca a sequência; com `close_at_end`
/// acrescenta a forma de repouso no quadro seguinte ao último.
pub fn exposures(frames: &[MouthShape], start_frame: u32, map: &MouthMap, close_at_end: bool) -> Result<Vec<(u32, u32)>, String> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut last: Option<u32> = None;
    for (i, &s) in frames.iter().enumerate() {
        let cel = map.cel(s).ok_or_else(|| format!("sem cel mapeado para a forma \"{}\"", s.name()))?;
        if last != Some(cel) {
            out.push((start_frame + i as u32, cel));
            last = Some(cel);
        }
    }
    if close_at_end {
        let cel = map.cel(MouthShape::Rest).ok_or_else(|| "sem cel de repouso (closed/rest)".to_string())?;
        if last != Some(cel) {
            out.push((start_frame + frames.len() as u32, cel));
        }
    }
    Ok(out)
}

/// Tabela em markdown (quadro → forma → mora) para evidência e revisão.
pub fn frame_table_md(frames: &[MouthShape], t: Option<&Timings>, fps: f64, start_frame: u32) -> String {
    let mut s = String::from("| quadros | tempo (s) | forma | moras no intervalo |\n|---|---|---|---|\n");
    let mut i = 0usize;
    for (shape, len) in runs(frames) {
        let (a, b) = (i, i + len);
        let (ta, tb) = (a as f64 / fps, b as f64 / fps);
        let moras = t
            .map(|t| t.moras.iter().filter(|m| m.start < tb && m.end > ta && m.class != VowelClass::Pau).map(|m| m.text.clone()).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        let label = if len == 1 { format!("{}", start_frame as usize + a) } else { format!("{}–{}", start_frame as usize + a, start_frame as usize + b - 1) };
        s.push_str(&format!("| {label} | {ta:.3}–{tb:.3} | {} | {} |\n", shape.name(), if moras.is_empty() { "—".into() } else { moras }));
        i = b;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::MoraTiming;

    fn m(text: &str, class: VowelClass, start: f64, end: f64) -> MoraTiming {
        MoraTiming { text: text.into(), consonant: None, vowel: "a".into(), class, devoiced: false, start, vowel_start: start, end, phrase: 0, pitch: 5.5 }
    }

    fn timings(moras: Vec<MoraTiming>) -> Timings {
        let d = moras.last().map_or(0.0, |m| m.end);
        Timings { moras, phonemes: vec![], duration: d, speech_start: 0.0, speech_end: d }
    }

    use MouthShape::*;

    #[test]
    fn min_hold_pause_bridge_and_close() {
        // 24 fps; "a" 6 quadros, pausa de 2 quadros (ponte), "i" 4, pausa de 6 (fecha), "o" 1 quadro (absorvido).
        let fps = 24.0;
        let q = 1.0 / fps;
        let t = timings(vec![
            m("a", VowelClass::A, 0.0, 6.0 * q),
            m("、", VowelClass::Pau, 6.0 * q, 8.0 * q),
            m("i", VowelClass::I, 8.0 * q, 12.0 * q),
            m("、", VowelClass::Pau, 12.0 * q, 18.0 * q),
            m("o", VowelClass::O, 18.0 * q, 19.0 * q),
            m("e", VowelClass::E, 19.0 * q, 24.0 * q),
        ]);
        let f = frames_from_moras(&t, Some(26), &LipsyncOptions::new(fps));
        assert_eq!(&f[0..8], &[A, A, A, A, A, A, A, A], "pausa de 2 quadros segura a forma anterior");
        assert_eq!(&f[8..12], &[I, I, I, I]);
        assert_eq!(&f[12..18], &[Rest; 6], "pausa de 6 quadros fecha");
        // "o" de 1 quadro encosta em silêncio de um lado e fala do outro → adota a fala (e).
        assert_eq!(&f[18..24], &[E, E, E, E, E, E]);
        assert_eq!(&f[24..26], &[Rest, Rest], "depois do fim: fechada");
        // Nenhuma forma de fala com menos de 2 quadros.
        for (s, n) in runs(&f) {
            assert!(n >= 2 || s.is_rest(), "{s:?} dura {n}");
        }
    }

    #[test]
    fn three_shape_reduction_and_n() {
        let fps = 24.0;
        let q = 1.0 / fps;
        let t = timings(vec![m("a", VowelClass::A, 0.0, 4.0 * q), m("i", VowelClass::I, 4.0 * q, 8.0 * q), m("ン", VowelClass::N, 8.0 * q, 12.0 * q), m("o", VowelClass::O, 12.0 * q, 16.0 * q)]);
        let mut o = LipsyncOptions::new(fps);
        let f7 = frames_from_moras(&t, Some(16), &o);
        assert_eq!((f7[0], f7[4], f7[8], f7[12]), (A, I, N, O));
        o.shapes = ShapeSet::Three;
        let f3 = frames_from_moras(&t, Some(16), &o);
        assert_eq!((f3[0], f3[4], f3[8], f3[12]), (Open, Half, Rest, Open));
    }

    #[test]
    fn exposures_map_and_fallbacks() {
        let map = MouthMap { a: Some(3), i: Some(4), rest: Some(1), open: Some(9), ..Default::default() };
        // O sem cel próprio cai para open; E sem cel cai para half→i.
        assert_eq!(map.cel(O), Some(9));
        assert_eq!(map.cel(E), Some(4));
        assert_eq!(map.cel(N), Some(1));
        let ex = exposures(&[Rest, Rest, A, A, A, I, I, Rest, Rest], 10, &map, true).unwrap();
        assert_eq!(ex, vec![(10, 1), (12, 3), (15, 4), (17, 1)]);
        // Fecha no quadro seguinte ao fim quando termina em fala.
        let ex = exposures(&[A, A, A], 0, &map, true).unwrap();
        assert_eq!(ex, vec![(0, 3), (3, 1)]);
        assert!(exposures(&[A], 0, &MouthMap::default(), false).is_err());
        let pairs: BTreeMap<String, u32> = [("a".to_string(), 2), ("closed".to_string(), 1)].into_iter().collect();
        assert_eq!(MouthMap::from_pairs(&pairs).cel(Rest), Some(1));
    }

    #[test]
    fn energy_fallback_follows_envelope() {
        // 1 s a 48 kHz: 0,3 s silêncio, 0,4 s tom forte, 0,3 s silêncio, a 24 fps.
        let rate = 48_000u32;
        let x: Vec<f32> = (0..rate as usize)
            .map(|i| {
                let t = i as f32 / rate as f32;
                if (0.3..0.7).contains(&t) { 0.5 * (t * 2.0 * std::f32::consts::PI * 220.0).sin() } else { 0.0 }
            })
            .collect();
        let f = frames_from_energy(&AudioBuf::mono(rate, x), &LipsyncOptions::new(24.0));
        assert_eq!(f.len(), 24);
        assert_eq!(f[2], Rest);
        assert_eq!(f[12], Open);
        assert_eq!(f[22], Rest);
        let open = f.iter().filter(|&&s| s == Open).count();
        assert!((7..=11).contains(&open), "{open}");
    }

    #[test]
    fn table_lists_runs() {
        let md = frame_table_md(&[Rest, Rest, A, A, A], None, 24.0, 5);
        assert!(md.contains("| 5–6 |") && md.contains("| 7–9 |") && md.contains("| a |"), "{md}");
    }
}
