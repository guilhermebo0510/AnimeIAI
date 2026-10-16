//! SFX procedurais determinísticos por semente: presets (whoosh, impacto, kira, passo, porta, vento, pano, sino, pop,
//! swell mágico) por ruído filtrado + osciladores + envelopes, e um gerador no estilo sfxr (quadrada/serra/seno/ruído com
//! slide, vibrato, arpejo, envelope com punch e filtros). A síntese é original: parâmetros de áudio padrão, sem copiar código.

use crate::buf::AudioBuf;
use crate::rng::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SfxPreset {
    Whoosh,
    Impact,
    /// Brilho/estrelinha (kira-kira).
    Kira,
    Footstep,
    Door,
    Wind,
    /// Roçar de pano.
    Cloth,
    Chime,
    Pop,
    MagicSwell,
    /// Usa `SfxSpec::sfxr`.
    Sfxr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Wave {
    Square,
    Saw,
    Sine,
    Noise,
}

/// Parâmetros do gerador estilo sfxr (todas as unidades em Hz/segundos/oitavas).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SfxrParams {
    pub wave: Wave,
    pub base_freq_hz: f32,
    /// Oitavas por segundo (negativo cai).
    #[serde(default)]
    pub freq_slide: f32,
    /// Largura da onda quadrada 0..1.
    #[serde(default = "d_duty")]
    pub duty: f32,
    #[serde(default)]
    pub duty_sweep: f32,
    #[serde(default)]
    pub vibrato_depth: f32,
    #[serde(default = "d_vib")]
    pub vibrato_rate_hz: f32,
    #[serde(default = "d_att")]
    pub attack_s: f32,
    #[serde(default = "d_sus")]
    pub sustain_s: f32,
    #[serde(default = "d_dec")]
    pub decay_s: f32,
    /// Reforço no início do sustain (0..1).
    #[serde(default)]
    pub punch: f32,
    /// 0 desliga.
    #[serde(default)]
    pub lowpass_hz: f32,
    #[serde(default)]
    pub highpass_hz: f32,
    /// Multiplicador de frequência aplicado em `arp_time_s` (1 = sem arpejo).
    #[serde(default = "d_one")]
    pub arp_mult: f32,
    #[serde(default)]
    pub arp_time_s: f32,
}

fn d_duty() -> f32 {
    0.5
}
fn d_vib() -> f32 {
    8.0
}
fn d_att() -> f32 {
    0.005
}
fn d_sus() -> f32 {
    0.12
}
fn d_dec() -> f32 {
    0.2
}
fn d_one() -> f32 {
    1.0
}

impl Default for SfxrParams {
    fn default() -> Self {
        SfxrParams {
            wave: Wave::Square,
            base_freq_hz: 440.0,
            freq_slide: 0.0,
            duty: 0.5,
            duty_sweep: 0.0,
            vibrato_depth: 0.0,
            vibrato_rate_hz: 8.0,
            attack_s: 0.005,
            sustain_s: 0.12,
            decay_s: 0.2,
            punch: 0.0,
            lowpass_hz: 0.0,
            highpass_hz: 0.0,
            arp_mult: 1.0,
            arp_time_s: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SfxSpec {
    pub preset: SfxPreset,
    /// A mesma semente gera exatamente o mesmo áudio.
    #[serde(default)]
    pub seed: u64,
    /// Duração em segundos; omitido = padrão do preset.
    #[serde(default)]
    pub duration: Option<f32>,
    /// Multiplicador de altura (1 = padrão).
    #[serde(default = "d_one")]
    pub pitch: f32,
    /// 0..1 — abre/fecha filtros e parciais agudos (0,5 = padrão).
    #[serde(default = "d_bright")]
    pub brightness: f32,
    /// Escala o nível de pico (1 = -3 dBFS).
    #[serde(default = "d_one")]
    pub intensity: f32,
    #[serde(default)]
    pub sfxr: Option<SfxrParams>,
    #[serde(default = "d_rate")]
    pub rate: u32,
}

fn d_bright() -> f32 {
    0.5
}
fn d_rate() -> u32 {
    crate::PROJECT_RATE
}

impl SfxSpec {
    pub fn new(preset: SfxPreset, seed: u64) -> Self {
        SfxSpec { preset, seed, duration: None, pitch: 1.0, brightness: 0.5, intensity: 1.0, sfxr: None, rate: crate::PROJECT_RATE }
    }
}

/// Filtro de estado variável (TPT): passa-baixas, passa-faixa e passa-altas com coeficientes variáveis no tempo.
#[derive(Debug, Clone, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// `f` em Hz, `q` > 0. Devolve (lp, bp, hp).
    #[inline]
    fn run(&mut self, x: f32, f: f32, q: f32, fs: f32) -> (f32, f32, f32) {
        let g = (PI * (f / fs).clamp(1e-4, 0.49)).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - k * v1 - v2)
    }
}

fn bright_factor(b: f32) -> f32 {
    2f32.powf((b.clamp(0.0, 1.0) - 0.5) * 2.0)
}

fn len_of(spec: &SfxSpec, default_s: f32) -> usize {
    ((spec.duration.unwrap_or(default_s).clamp(0.02, 30.0)) * spec.rate as f32) as usize
}

fn normalize_peak(x: &mut [f32], peak: f32) {
    let m = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if m > 1e-9 {
        let g = peak / m;
        for v in x.iter_mut() {
            *v *= g;
        }
    }
}

/// Fade-in de 1 ms e fade-out de até 8 ms para eliminar cliques nas pontas.
fn declick(x: &mut [f32], rate: u32) {
    let n = x.len();
    let fi = ((rate / 1000) as usize).min(n / 2);
    let fo = ((rate / 125) as usize).min(n / 2);
    for i in 0..fi {
        x[i] *= i as f32 / fi as f32;
    }
    for i in 0..fo {
        x[n - 1 - i] *= i as f32 / fo as f32;
    }
}

struct BellPartial {
    ratio: f32,
    amp: f32,
    tau: f32,
}

/// Sino/kira: parciais inarmônicos com decaimento próprio, adicionados em `out` a partir de `t0`.
fn add_bell(out: &mut [f32], rate: u32, t0: f32, freq: f32, amp: f32, tau_scale: f32, partials: &[BellPartial], shimmer_hz: f32, shimmer_depth: f32, brightness: f32) {
    let fs = rate as f32;
    let start = (t0 * fs) as usize;
    for p in partials {
        let f = freq * p.ratio;
        if f > fs * 0.45 {
            continue;
        }
        let pa = p.amp * if p.ratio > 2.0 { brightness.min(1.6) } else { 1.0 };
        let tau = p.tau * tau_scale;
        let n = ((tau * 7.0 * fs) as usize).min(out.len().saturating_sub(start));
        let w = 2.0 * PI * f / fs;
        for i in 0..n {
            let t = i as f32 / fs;
            let env = (-t / tau).exp() * (1.0 - (-t / 0.002).exp());
            let trem = 1.0 - shimmer_depth * 0.5 * (1.0 - (2.0 * PI * shimmer_hz * t).cos());
            out[start + i] += amp * pa * env * trem * (w * i as f32).sin();
        }
    }
}

const BELL: [BellPartial; 4] = [
    BellPartial { ratio: 1.0, amp: 1.0, tau: 1.0 },
    BellPartial { ratio: 2.756, amp: 0.55, tau: 0.6 },
    BellPartial { ratio: 5.404, amp: 0.3, tau: 0.35 },
    BellPartial { ratio: 8.933, amp: 0.16, tau: 0.2 },
];

pub fn synth(spec: &SfxSpec) -> AudioBuf {
    let mut x = match spec.preset {
        SfxPreset::Whoosh => whoosh(spec),
        SfxPreset::Impact => impact(spec),
        SfxPreset::Kira => kira(spec),
        SfxPreset::Footstep => footstep(spec),
        SfxPreset::Door => door(spec),
        SfxPreset::Wind => wind(spec),
        SfxPreset::Cloth => cloth(spec),
        SfxPreset::Chime => chime(spec),
        SfxPreset::Pop => pop(spec),
        SfxPreset::MagicSwell => magic_swell(spec),
        SfxPreset::Sfxr => sfxr(spec),
    };
    declick(&mut x, spec.rate);
    normalize_peak(&mut x, 0.708 * spec.intensity.clamp(0.0, 1.4));
    AudioBuf::mono(spec.rate, x)
}

fn whoosh(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 0.9);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness) * s.pitch;
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let u = i as f32 / n as f32;
        // Varredura de frequência: sobe e relaxa.
        let center = (350.0 + 3200.0 * (u * PI).sin().powf(1.3)) * bf;
        let env = (u * PI).sin().powf(2.0) * (1.0 - 0.35 * u);
        let nz = rng.bipolar();
        let (_, bp, _) = f1.run(nz, center, 1.3, fs);
        let (lp, _, _) = f2.run(nz, 220.0 * bf, 0.7, fs);
        *o = (bp * 1.4 + lp * 0.35) * env;
    }
    out
}

fn impact(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 0.7);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let base = 36.0 * s.pitch;
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let mut phase = 0.0f32;
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / fs;
        let f = base + 110.0 * s.pitch * (-t / 0.045).exp();
        phase += 2.0 * PI * f / fs;
        let thump = phase.sin() * (-t / 0.22).exp();
        let nz = rng.bipolar();
        let cutoff = (350.0 + 2600.0 * (-t / 0.05).exp()) * bf;
        let (lp, _, _) = f1.run(nz, cutoff, 0.8, fs);
        let burst = lp * (-t / 0.07).exp() * 0.9;
        let (rum, _, _) = f2.run(nz, 110.0, 0.7, fs);
        let rumble = rum * (-t / 0.4).exp() * 1.4;
        let click = if i < (0.003 * fs) as usize { rng.bipolar() * 0.6 * (1.0 - i as f32 / (0.003 * fs)) } else { 0.0 };
        *o = thump * 1.2 + burst + rumble + click * bf;
    }
    out
}

fn kira(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 1.5);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let mut out = vec![0.0f32; n];
    // Pentatônica aguda ascendente com ordem levemente embaralhada pela semente.
    let scale = [1.0f32, 9.0 / 8.0, 5.0 / 4.0, 3.0 / 2.0, 5.0 / 3.0, 2.0, 9.0 / 4.0];
    let count = 5 + (rng.next_u64() % 3) as usize;
    let base = 1568.0 * s.pitch;
    let mut t0 = 0.0f32;
    let mut hp = Svf::default();
    let mut glint = vec![0.0f32; n];
    for k in 0..count {
        let idx = (k + (rng.next_u64() % 2) as usize).min(scale.len() - 1);
        let f = base * scale[idx];
        let amp = 0.55 + 0.45 * (k as f32 / count as f32) * rng.range(0.7, 1.0);
        add_bell(&mut out, s.rate, t0, f, amp, 0.28, &BELL, rng.range(9.0, 17.0), 0.35, bf);
        // Brilho de ruído agudo no ataque de cada nota.
        let g0 = (t0 * fs) as usize;
        for i in 0..((0.03 * fs) as usize).min(n.saturating_sub(g0)) {
            let t = i as f32 / fs;
            let nz = rng.bipolar();
            let (_, _, h) = hp.run(nz, 6500.0 * bf, 0.7, fs);
            glint[g0 + i] += h * (-t / 0.012).exp() * 0.25;
        }
        t0 += rng.range(0.045, 0.11);
    }
    for (o, g) in out.iter_mut().zip(&glint) {
        *o += g;
    }
    out
}

fn footstep(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 0.3);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let heel_f = 95.0 * s.pitch * rng.range(0.85, 1.2);
    let scuff_f = rng.range(900.0, 1800.0) * bf;
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let toe_t = rng.range(0.07, 0.11);
    let mut ph = 0.0f32;
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / fs;
        let f = heel_f * (0.5 + 0.5 * (-t / 0.03).exp());
        ph += 2.0 * PI * f / fs;
        let thud = ph.sin() * (-t / 0.045).exp();
        let nz = rng.bipolar();
        let (_, bp, _) = f1.run(nz, scuff_f, 1.1, fs);
        let heel_noise = bp * (-t / 0.025).exp() * 0.7;
        let (_, _, hp) = f2.run(nz, 2500.0 * bf, 0.8, fs);
        let dt = t - toe_t;
        let toe = if dt >= 0.0 { hp * (-dt / 0.02).exp() * 0.28 } else { 0.0 };
        *o = thud * 1.1 + heel_noise + toe;
    }
    out
}

fn door(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 0.9);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let (mut f1, mut f2, mut f3) = (Svf::default(), Svf::default(), Svf::default());
    let latch_t = 0.07 + rng.range(0.0, 0.02);
    let mut ph = 0.0f32;
    let mut out = vec![0.0f32; n];
    let modes = [(160.0f32, 1.0f32, 0.16f32), (235.0, 0.7, 0.12), (410.0, 0.4, 0.08)];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / fs;
        ph += 2.0 * PI * (45.0 + 30.0 * (-t / 0.05).exp()) * s.pitch / fs;
        let thud = ph.sin() * (-t / 0.13).exp() * 1.0;
        let nz = rng.bipolar();
        let (lp, _, _) = f1.run(nz, 420.0 * bf, 0.8, fs);
        let body = lp * (-t / 0.09).exp() * 0.9;
        let mut wood = 0.0f32;
        for (f, a, tau) in modes {
            wood += (2.0 * PI * f * s.pitch * t).sin() * a * (-t / tau).exp() * 0.25;
        }
        // Trinco: dois estalos curtos em ~2,4 kHz.
        let mut latch = 0.0f32;
        for (k, off) in [0.0f32, 0.026].iter().enumerate() {
            let dt = t - latch_t - off;
            if dt >= 0.0 {
                let (_, bp, _) = f2.run(nz, 2400.0 * bf, 3.0, fs);
                latch += bp * (-dt / 0.005).exp() * if k == 0 { 0.9 } else { 0.6 };
                latch += (2.0 * PI * 1150.0 * dt).sin() * (-dt / 0.03).exp() * 0.12;
            }
        }
        let (_, _, hp) = f3.run(nz, 3000.0, 0.7, fs);
        let click = hp * (-t / 0.004).exp() * 0.3;
        *o = thud + body + wood + latch + click;
    }
    out
}

fn wind(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 3.0);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness) * s.pitch;
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    // Modulações lentas: ruído suavizado por passa-baixas de ~0,5 Hz.
    let mut lfo_a = 0.0f32;
    let mut lfo_b = 0.0f32;
    let c = (-2.0 * PI * 0.5 / fs).exp();
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        lfo_a = c * lfo_a + (1.0 - c) * rng.bipolar() * 5.0;
        lfo_b = c * lfo_b + (1.0 - c) * rng.bipolar() * 5.0;
        let u = i as f32 / n as f32;
        let fade = (u * 6.0).min(1.0) * ((1.0 - u) * 6.0).min(1.0);
        let cutoff = (520.0 * 2f32.powf(lfo_a.clamp(-1.2, 1.2))) * bf;
        let amp = (0.55 + 0.45 * lfo_b.clamp(-1.0, 1.0)).max(0.1);
        let nz = rng.bipolar();
        let (_, bp, _) = f1.run(nz, cutoff, 2.2, fs);
        let (lp, _, _) = f2.run(nz, 160.0, 0.7, fs);
        *o = (bp * 1.2 + lp * 0.5) * amp * fade;
    }
    out
}

fn cloth(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 0.8);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness) * s.pitch;
    // Envelope por grãos: ~10 rajadas de 20–80 ms em posições aleatórias.
    let mut env = vec![0.0f32; n];
    let grains = 8 + (rng.next_u64() % 6) as usize;
    for _ in 0..grains {
        let c = rng.range(0.05, 0.95) * n as f32;
        let w = rng.range(0.02, 0.08) * fs;
        let a = rng.range(0.3, 1.0);
        let lo = (c - w) as isize;
        let hi = (c + w) as isize;
        for i in lo.max(0)..hi.min(n as isize) {
            let r = (i as f32 - c) / w;
            env[i as usize] += a * (0.5 + 0.5 * (PI * r).cos());
        }
    }
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let nz = rng.bipolar();
        let (_, bp, _) = f1.run(nz, 3200.0 * bf, 0.9, fs);
        let (_, _, hp) = f2.run(nz, 1400.0 * bf, 0.7, fs);
        *o = (bp * 0.8 + hp * 0.4) * env[i].min(1.6);
    }
    out
}

fn chime(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 2.6);
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let mut out = vec![0.0f32; n];
    let f = 880.0 * s.pitch;
    add_bell(&mut out, s.rate, 0.0, f, 1.0, 1.4, &BELL, 5.5, 0.12, bf);
    add_bell(&mut out, s.rate, rng.range(0.1, 0.16), f * 1.5, 0.5, 1.2, &BELL, 6.5, 0.12, bf);
    out
}

fn pop(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 0.16);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let f_start = 280.0 * s.pitch * rng.range(0.9, 1.1);
    let mut ph = 0.0f32;
    let mut f1 = Svf::default();
    let mut out = vec![0.0f32; n];
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / fs;
        let f = f_start * (1.0 + 3.2 * (1.0 - (-t / 0.018).exp()));
        ph += 2.0 * PI * f / fs;
        let nz = rng.bipolar();
        let (_, _, hp) = f1.run(nz, 3000.0 * bf, 0.7, fs);
        *o = ph.sin() * (-t / 0.035).exp() + hp * (-t / 0.002).exp() * 0.35;
    }
    out
}

fn magic_swell(s: &SfxSpec) -> Vec<f32> {
    let n = len_of(s, 2.6);
    let fs = s.rate as f32;
    let mut rng = Rng::new(s.seed);
    let bf = bright_factor(s.brightness);
    let dur = n as f32 / fs;
    let mut out = vec![0.0f32; n];
    let (mut f1, mut f2) = (Svf::default(), Svf::default());
    let detune = [0.0f32, 0.0065, -0.0058];
    let mut phases = [rng.f32(), rng.f32(), rng.f32()];
    let mut shim_ph = [0.0f32; 4];
    for (i, o) in out.iter_mut().enumerate() {
        let u = i as f32 / n as f32;
        let t = u * dur;
        // Subida exponencial de altura e de brilho.
        let f0 = 180.0 * s.pitch * 4f32.powf(u.powf(1.4));
        let env = u.powf(2.2) * if u > 0.78 { ((1.0 - u) / 0.22).max(0.0).powf(0.7) } else { 1.0 };
        let mut v = 0.0f32;
        for (k, d) in detune.iter().enumerate() {
            phases[k] = (phases[k] + f0 * (1.0 + d) / fs).fract();
            // Serra com poucos harmônicos somados (limite de banda aproximado).
            let mut saw = 0.0f32;
            for h in 1..=10u32 {
                if f0 * h as f32 * 1.01 > fs * 0.45 {
                    break;
                }
                saw += (2.0 * PI * phases[k] * h as f32).sin() / h as f32;
            }
            v += saw * 0.33;
        }
        let cutoff = (500.0 + 6500.0 * u * u) * bf;
        let (lp, _, _) = f1.run(v, cutoff, 1.4, fs);
        // Brilho agudo crescente (parciais senoidais com tremolo).
        let mut shimmer = 0.0f32;
        for (k, p) in shim_ph.iter_mut().enumerate() {
            let fk = f0 * (4.0 + k as f32 * 1.5) * 2.0;
            if fk < fs * 0.45 {
                *p += 2.0 * PI * fk / fs;
                shimmer += p.sin() * (0.5 + 0.5 * (2.0 * PI * (7.0 + k as f32 * 2.3) * t).sin()) * 0.08;
            }
        }
        let nz = rng.bipolar();
        let (_, _, hp) = f2.run(nz, 5000.0 * bf, 0.7, fs);
        *o = (lp * 0.8 + shimmer * u * 2.0 + hp * 0.05 * u * u) * env;
    }
    // Faísca final.
    add_bell(&mut out, s.rate, dur * 0.72, 2093.0 * s.pitch, 0.5, 0.35, &BELL, 12.0, 0.3, bf);
    out
}

fn sfxr(s: &SfxSpec) -> Vec<f32> {
    let p = s.sfxr.unwrap_or_default();
    let fs = s.rate as f32;
    let total = (p.attack_s + p.sustain_s + p.decay_s).max(0.01);
    let n = ((s.duration.unwrap_or(total)) * fs) as usize;
    let mut rng = Rng::new(s.seed);
    let mut phase = 0.0f32;
    let mut out = vec![0.0f32; n];
    let (mut lp_y, mut hp_y, mut hp_x) = (0.0f32, 0.0f32, 0.0f32);
    let lp_c = if p.lowpass_hz > 0.0 { 1.0 - (-2.0 * PI * p.lowpass_hz / fs).exp() } else { 1.0 };
    let hp_c = if p.highpass_hz > 0.0 { (-2.0 * PI * p.highpass_hz / fs).exp() } else { 0.0 };
    let mut noise_hold = 0.0f32;
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / fs;
        let mut f = p.base_freq_hz * s.pitch * 2f32.powf(p.freq_slide * t);
        if p.arp_time_s > 0.0 && t >= p.arp_time_s {
            f *= p.arp_mult;
        }
        if p.vibrato_depth > 0.0 {
            f *= 1.0 + p.vibrato_depth * (2.0 * PI * p.vibrato_rate_hz * t).sin();
        }
        let f = f.clamp(10.0, fs * 0.45);
        let dt = f / fs;
        let prev = phase;
        phase = (phase + dt).fract();
        let duty = (p.duty + p.duty_sweep * t).clamp(0.05, 0.95);
        let v = match p.wave {
            Wave::Sine => (2.0 * PI * phase).sin(),
            Wave::Square => {
                let mut v = if phase < duty { 1.0 } else { -1.0 };
                v += poly_blep(phase, dt) - poly_blep((phase + 1.0 - duty).fract(), dt);
                v
            }
            Wave::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
            Wave::Noise => {
                if phase < prev {
                    noise_hold = rng.bipolar();
                }
                noise_hold
            }
        };
        let env = if t < p.attack_s {
            t / p.attack_s.max(1e-4)
        } else if t < p.attack_s + p.sustain_s {
            1.0 + p.punch * 2.0 * (1.0 - (t - p.attack_s) / p.sustain_s.max(1e-4))
        } else {
            (1.0 - (t - p.attack_s - p.sustain_s) / p.decay_s.max(1e-4)).max(0.0).powi(2)
        };
        lp_y += lp_c * (v - lp_y);
        let mut y = lp_y;
        if p.highpass_hz > 0.0 {
            let hy = hp_c * (hp_y + y - hp_x);
            hp_x = y;
            hp_y = hy;
            y = hy;
        }
        *o = y * env;
    }
    out
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SfxPreset; 10] = [
        SfxPreset::Whoosh,
        SfxPreset::Impact,
        SfxPreset::Kira,
        SfxPreset::Footstep,
        SfxPreset::Door,
        SfxPreset::Wind,
        SfxPreset::Cloth,
        SfxPreset::Chime,
        SfxPreset::Pop,
        SfxPreset::MagicSwell,
    ];

    #[test]
    fn every_preset_is_deterministic_finite_and_audible() {
        for p in ALL {
            let a = synth(&SfxSpec::new(p, 7));
            let b = synth(&SfxSpec::new(p, 7));
            assert_eq!(a, b, "{p:?} deve ser determinístico");
            assert!(a.ch[0].iter().all(|v| v.is_finite()), "{p:?}");
            assert!(a.duration_s() > 0.1, "{p:?}");
            assert!(a.rms() > 0.01, "{p:?} silencioso: rms {}", a.rms());
            assert!((a.peak() - 0.708).abs() < 0.01, "{p:?} pico {}", a.peak());
        }
    }

    #[test]
    fn seed_changes_the_sound_and_params_apply() {
        for p in [SfxPreset::Footstep, SfxPreset::Kira, SfxPreset::Cloth, SfxPreset::Wind] {
            assert_ne!(synth(&SfxSpec::new(p, 1)), synth(&SfxSpec::new(p, 2)), "{p:?}");
        }
        let mut s = SfxSpec::new(SfxPreset::Whoosh, 3);
        s.duration = Some(0.5);
        assert_eq!(synth(&s).frames(), 24_000);
        s.intensity = 0.5;
        assert!((synth(&s).peak() - 0.354).abs() < 0.01);
    }

    #[test]
    fn spectral_character_is_plausible() {
        // Passo: energia concentrada em graves; kira: muito mais energia acima de 3 kHz que o impacto.
        let hi_ratio = |b: &AudioBuf| {
            let mut x = b.ch[0].clone();
            crate::dsp::filter_in_place(&mut x, crate::dsp::BiquadCoeffs::highpass(48_000.0, 3000.0, 0.7));
            let hi: f64 = x.iter().map(|v| (*v as f64).powi(2)).sum();
            let all: f64 = b.ch[0].iter().map(|v| (*v as f64).powi(2)).sum();
            hi / all
        };
        let kira = hi_ratio(&synth(&SfxSpec::new(SfxPreset::Kira, 1)));
        let impact = hi_ratio(&synth(&SfxSpec::new(SfxPreset::Impact, 1)));
        assert!(kira > 0.3 && impact < 0.1 && kira > 4.0 * impact, "kira {kira} impacto {impact}");
    }

    #[test]
    fn sfxr_generates_pitch_slide() {
        let mut s = SfxSpec::new(SfxPreset::Sfxr, 1);
        s.sfxr = Some(SfxrParams { wave: Wave::Square, base_freq_hz: 800.0, freq_slide: -2.0, sustain_s: 0.3, decay_s: 0.1, ..Default::default() });
        let b = synth(&s);
        // Contagem de cruzamentos por zero no começo vs fim: a frequência cai.
        let zc = |a: &[f32]| a.windows(2).filter(|w| w[0].signum() != w[1].signum()).count();
        let n = b.frames();
        assert!(zc(&b.ch[0][1000..6000]) > 2 * zc(&b.ch[0][n / 2..n / 2 + 5000]).max(1) / 2, "slide");
        assert!(b.rms() > 0.05);
    }
}
