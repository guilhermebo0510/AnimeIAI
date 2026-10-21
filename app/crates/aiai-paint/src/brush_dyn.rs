//! Modelo de caneta do pincel raster (S25, calibrado no resultado das canetas de fábrica do Clip Studio Paint 5.1.5):
//!
//! - **dinâmicas** por parâmetro (tamanho, fluxo, opacidade, espessura da ponta): pressão, inclinação e velocidade, cada
//!   uma com mínimo e curva, e aleatório com mínimo; os controles se multiplicam (como os "efetores" do Clip Studio);
//! - **velocidade** do traço estimada com média exponencial no tempo das amostras (as amostras de um mesmo quadro chegam
//!   com o mesmo tempo e entram juntas), normalizada por `velocity_full` (px/s que contam como 100%);
//! - **entrada e saída** (afinamento das pontas) por comprimento, com mínimo, no tamanho e/ou na opacidade, opcionalmente
//!   pela velocidade (traço lento quase não afina);
//! - **estabilização** de 0 a 100 com "pela velocidade": segue o caminho com atraso que some quando a mão vai rápido e
//!   alcança a ponta ao soltar (não corta canto nem encurta o fim, como no Clip Studio nos níveis de fábrica);
//! - **borda áspera** (ponta com contorno irregular, sorteado a cada carimbo) para as canetas texturizada/rugosa/G real.
//!
//! Implementação própria a partir dos números do banco de ferramentas e das medidas das folhas de referência
//! (`tools/csp/csp_tooldb.py`, `tools/csp/calib_compare.py`); nenhuma imagem de ponta ou textura de terceiros.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Curva de resposta em 0..1 (pontos x crescentes), interpolada com cúbica monótona (não passa dos pontos, como a curva
/// do editor de pressão). Vazia = identidade.
pub fn curve_eval(pts: &[[f32; 2]], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match pts.len() {
        0 => return x,
        1 => return pts[0][1].clamp(0.0, 1.0),
        _ => {}
    }
    if x <= pts[0][0] {
        return pts[0][1].clamp(0.0, 1.0);
    }
    let n = pts.len();
    if x >= pts[n - 1][0] {
        return pts[n - 1][1].clamp(0.0, 1.0);
    }
    // inclinações de Fritsch-Carlson
    let d: Vec<f32> = pts.windows(2).map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]).max(1e-6)).collect();
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
    }
    for i in 0..n - 1 {
        if d[i].abs() < 1e-9 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    let i = pts.windows(2).position(|w| x >= w[0][0] && x <= w[1][0]).unwrap_or(n - 2);
    let (x0, y0, x1, y1) = (pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1]);
    let h = (x1 - x0).max(1e-6);
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    let y = (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * h * m[i] + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * h * m[i + 1];
    y.clamp(0.0, 1.0)
}

/// Um controle de dinâmica: com a entrada em 0..1, o parâmetro vai de `min` (curva = 0) a 100% (curva = 1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DynInput {
    /// Mínimo (fração 0..1 do valor do parâmetro).
    pub min: f32,
    /// Curva entrada → saída (pontos [x, y] em 0..1); vazia = linear. Para a velocidade, a curva de fábrica desce
    /// (lento = 1, rápido = 0): rápido afina.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curve: Vec<[f32; 2]>,
}

impl DynInput {
    pub fn linear(min: f32) -> Self {
        DynInput { min, curve: Vec::new() }
    }

    /// Velocidade de fábrica: lento = 100%, rápido = `min`.
    pub fn slower_is_bigger(min: f32) -> Self {
        DynInput { min, curve: vec![[0.0, 1.0], [1.0, 0.0]] }
    }

    pub fn factor(&self, input: f32) -> f32 {
        let min = self.min.clamp(0.0, 1.0);
        min + (1.0 - min) * curve_eval(&self.curve, input)
    }
}

/// Dinâmica de um parâmetro (os controles ligados se multiplicam).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Dynamics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<DynInput>,
    /// Inclinação da caneta (0 = em pé, 1 = deitada); sem mesa digitalizadora, conta como em pé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<DynInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<DynInput>,
    /// Aleatório: cada carimbo sorteia entre `random` e 100%.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random: Option<f32>,
    /// Aleatório suave ao longo do traço: o sorteio muda a cada `random_len` px de caminho, com transição suave (como
    /// uma ponta esticada em fita); 0 = um sorteio por carimbo.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub random_len: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn hash1(i: i32, seed: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// Valor aleatório 0..1 que varia suavemente ao longo do caminho (`s` px), trocando a cada `len` px.
pub fn smooth_random(s: f32, len: f32, seed: u32) -> f32 {
    let u = s / len.max(1e-3);
    let i = u.floor();
    let f = u - i;
    let f = f * f * (3.0 - 2.0 * f);
    let (a, b) = (hash1(i as i32, seed), hash1(i as i32 + 1, seed));
    a + (b - a) * f
}

impl Dynamics {
    pub fn pressure(min: f32) -> Self {
        Dynamics { pressure: Some(DynInput::linear(min)), ..Default::default() }
    }

    /// Fator 0..1 sem o aleatório (entra no espaçamento entre carimbos).
    pub fn base(&self, pressure: f32, velocity: f32) -> f32 {
        let mut f = 1.0;
        if let Some(p) = &self.pressure {
            f *= p.factor(pressure);
        }
        if let Some(t) = &self.tilt {
            f *= t.factor(0.0);
        }
        if let Some(v) = &self.velocity {
            f *= v.factor(velocity);
        }
        f
    }

    /// Fator 0..1 com o aleatório (`rnd` uniforme em 0..1).
    pub fn factor(&self, pressure: f32, velocity: f32, rnd: f32) -> f32 {
        let mut f = self.base(pressure, velocity);
        if let Some(m) = self.random {
            let m = m.clamp(0.0, 1.0);
            f *= m + (1.0 - m) * rnd.clamp(0.0, 1.0);
        }
        f
    }

    pub fn uses_random(&self) -> bool {
        self.random.is_some()
    }

    pub fn validate(&self, name: &str) -> Result<(), String> {
        for (k, d) in [("pressure", &self.pressure), ("tilt", &self.tilt), ("velocity", &self.velocity)] {
            if let Some(d) = d {
                if !(0.0..=1.0).contains(&d.min) {
                    return Err(format!("{name}.{k}.min fora de 0..1: {}", d.min));
                }
                if d.curve.iter().any(|p| !(0.0..=1.0).contains(&p[0]) || !(0.0..=1.0).contains(&p[1])) || d.curve.windows(2).any(|w| w[1][0] < w[0][0]) {
                    return Err(format!("{name}.{k}.curve: pontos em 0..1 com x crescente"));
                }
            }
        }
        if let Some(r) = self.random {
            if !(0.0..=1.0).contains(&r) {
                return Err(format!("{name}.random fora de 0..1: {r}"));
            }
        }
        if !(self.random_len.is_finite() && (0.0..=5000.0).contains(&self.random_len)) {
            return Err(format!("{name}.random_len fora de 0..5000 px: {}", self.random_len));
        }
        Ok(())
    }
}

/// Entrada e saída do traço (afinamento das pontas), em px de caminho.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Taper {
    /// Comprimento da entrada (px); 0 = sem.
    #[serde(default)]
    pub start: f32,
    /// Comprimento da saída (px); 0 = sem. A saída só é conhecida quando o traço termina: a prévia ao vivo mostra a
    /// ponta afinada no cursor e os carimbos dos últimos `end` px esperam o resto do traço.
    #[serde(default)]
    pub end: f32,
    /// Valor na ponta (fração 0..1).
    #[serde(default)]
    pub min: f32,
    /// Afina o tamanho.
    #[serde(default = "yes")]
    pub size: bool,
    /// Afina a opacidade.
    #[serde(default)]
    pub opacity: bool,
    /// Pela velocidade: o comprimento é proporcional à velocidade na ponta (traço lento quase não afina).
    #[serde(default)]
    pub by_speed: bool,
}

fn yes() -> bool {
    true
}

impl Taper {
    /// Fator na distância `s` de uma ponta com comprimento `len` (suave: a ponta não fica um degrau).
    pub fn ramp(&self, s: f32, len: f32) -> f32 {
        if len <= 1e-3 {
            return 1.0;
        }
        let t = (s / len).clamp(0.0, 1.0);
        let e = t * (2.0 - t); // sai rápido da ponta e chega no corpo sem quina
        let m = self.min.clamp(0.0, 1.0);
        m + (1.0 - m) * e
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(self.start.is_finite() && self.end.is_finite() && self.start >= 0.0 && self.end >= 0.0 && self.start <= 5000.0 && self.end <= 5000.0) {
            return Err("taper: comprimentos entre 0 e 5000 px".into());
        }
        if !(0.0..=1.0).contains(&self.min) {
            return Err(format!("taper.min fora de 0..1: {}", self.min));
        }
        Ok(())
    }
}

/// Estabilização do traço (correção de tremura) do pincel: nível 0..100 e "pela velocidade".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stabilization {
    pub level: u8,
    #[serde(default)]
    pub by_speed: bool,
}

/// Velocidade (px/s) em que o efeito "pela velocidade" da estabilização cai à metade.
const STAB_SPEED_HALF: f32 = 300.0;
/// Atraso (s) por nível de estabilização com a mão parada.
const STAB_TAU_PER_LEVEL: f32 = 0.0016;

/// Estabilizador do traço: média exponencial da posição no tempo, com constante `nível × 1,6 ms` (nível 100 ≈ 0,16 s
/// de atraso), mais fraca quanto mais rápida a mão quando "pela velocidade" está ligado. Ao soltar, alcança o último
/// ponto (o traço não encurta). As unidades são px do desenho e segundos.
#[derive(Debug, Clone)]
pub struct Stabilizer {
    cfg: Stabilization,
    pos: Option<(f32, f32)>,
    last_t: f32,
    last_raw: Option<(f32, f32, f32)>,
    speed: f32,
}

impl Stabilizer {
    pub fn new(cfg: Stabilization) -> Self {
        Stabilizer { cfg, pos: None, last_t: 0.0, last_raw: None, speed: 0.0 }
    }

    pub fn level(&self) -> u8 {
        self.cfg.level
    }

    /// Amostra bruta (x, y, t em s); devolve a posição estabilizada.
    pub fn feed(&mut self, x: f32, y: f32, t: f32) -> (f32, f32) {
        let prev_raw = self.last_raw;
        self.last_raw = Some((x, y, t));
        let Some((px, py)) = self.pos else {
            self.pos = Some((x, y));
            self.last_t = t;
            return (x, y);
        };
        if self.cfg.level == 0 {
            self.pos = Some((x, y));
            self.last_t = t;
            return (x, y);
        }
        let dt = (t - self.last_t).max(0.0);
        if let Some((rx, ry, rt)) = prev_raw {
            if t > rt {
                let inst = ((x - rx).powi(2) + (y - ry).powi(2)).sqrt() / (t - rt);
                let k = 1.0 - (-(t - rt) / 0.03).exp();
                self.speed += (inst - self.speed) * k;
            }
        }
        let mut tau = self.cfg.level as f32 * STAB_TAU_PER_LEVEL;
        if self.cfg.by_speed {
            tau /= 1.0 + self.speed / STAB_SPEED_HALF;
        }
        // amostras do mesmo quadro (dt = 0) andam um passo mínimo, senão o quadro inteiro ficaria parado
        let k = if dt > 0.0 { 1.0 - (-dt / tau.max(1e-4)).exp() } else { 1.0 - (-0.004 / tau.max(1e-4)).exp() };
        let nx = px + (x - px) * k;
        let ny = py + (y - py) * k;
        self.pos = Some((nx, ny));
        self.last_t = t;
        (nx, ny)
    }

    /// Fim do traço: pontos que levam a posição estabilizada até o último ponto bruto (passos de ~1 px).
    pub fn finish(&mut self) -> Vec<(f32, f32, f32)> {
        let (Some((px, py)), Some((x, y, t))) = (self.pos, self.last_raw) else { return Vec::new() };
        let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
        if d < 0.05 {
            return Vec::new();
        }
        let n = d.ceil().clamp(1.0, 200.0) as usize;
        let out = (1..=n).map(|i| {
            let f = i as f32 / n as f32;
            (px + (x - px) * f, py + (y - py) * f, t)
        }).collect();
        self.pos = Some((x, y));
        out
    }
}

/// Borda áspera da ponta redonda: o raio encolhe conforme um ruído fixo no canvas (grão de `grain` px), então todos os
/// carimbos do traço concordam onde a borda morde e o contorno sai irregular de verdade (como a ponta de imagem áspera
/// esticada em fita das canetas texturizada/rugosa/G real), em vez de uma média lisa de bordas sorteadas.
/// `amount` = quanto o raio encolhe onde o ruído é máximo (fração 0..1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EdgeNoise {
    pub amount: f32,
    #[serde(default = "d_grain")]
    pub grain: f32,
    #[serde(default)]
    pub seed: u32,
}

fn d_grain() -> f32 {
    4.0
}

impl EdgeNoise {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=0.9).contains(&self.amount) || !(self.grain.is_finite() && self.grain >= 0.5 && self.grain <= 500.0) {
            return Err("edge: amount 0..0,9 e grain 0,5..500 px".into());
        }
        Ok(())
    }

    /// Raio relativo (1 - amount .. 1) no pixel (x, y) do canvas.
    pub fn radius_at(&self, x: f32, y: f32) -> f32 {
        let g = self.grain.max(0.5);
        let n = crate::noise::fbm(x / g, y / g, 3, 2.0, 0.5, self.seed ^ 0xED6E);
        1.0 - self.amount * ((n - 0.25) * 2.0).clamp(0.0, 1.0)
    }
}

/// Suavização de serrilhado (como os 4 níveis do Clip Studio): (largura da rampa da borda, recuo do meio da rampa), px.
/// Nível 0 = sem (borda dura); 1 fraca; 2 média; 3 forte. Medido nas folhas de referência: a seção de uma linha da
/// G-pen (média) é a cobertura de um disco de raio `r - 0,6` com rampa de ~1 px.
pub fn aa_ramp(level: u8) -> (f32, f32) {
    if let Some(t) = CALIB.get() {
        let r = t[(level as usize).min(3)];
        return (r[0], r[1]);
    }
    match level {
        0 => (0.0, 0.0),
        1 => (0.7, 0.5),
        2 => (1.15, 0.65),
        _ => (2.0, 0.8),
    }
}

/// Menor semieixo (px) da ponta achatada: a ponta chata fina não some (no Clip Studio, a lateral fina da Caligrafia e do
/// Marcador plano nunca fica abaixo de ~3 px de espessura).
pub fn min_semi_axis() -> f32 {
    CALIB.get().map(|t| t[4][0]).unwrap_or(1.5)
}

static CALIB: std::sync::OnceLock<[[f32; 2]; 5]> = std::sync::OnceLock::new();

/// Só para o calibrador offline: troca a tabela do antisserrilhado (níveis 0..3), o menor semieixo (`[4][0]`) e a
/// constante de tempo da velocidade (`[4][1]`, s; 0 = padrão) uma vez
/// por processo. O app nunca chama.
pub fn calibration_override(t: [[f32; 2]; 5]) {
    let _ = CALIB.set(t);
}

/// Estimador da velocidade do traço (px/s) para as dinâmicas: média exponencial com constante `tau` (s) da velocidade
/// entre amostras com tempos diferentes; amostras com o mesmo tempo (mesmo quadro) se acumulam até o próximo tempo.
#[derive(Debug, Clone, Default)]
pub struct SpeedEstimator {
    anchor: Option<(f32, f32)>,
    travel: f32,
    speed: f32,
}

/// Constante de tempo da velocidade do traço (s).
pub const SPEED_TAU: f32 = 0.015;

impl SpeedEstimator {
    /// Mais `ds` px de caminho chegando no tempo `t` (s); devolve a velocidade suavizada.
    pub fn push(&mut self, ds: f32, t: f32) -> f32 {
        self.travel += ds;
        match self.anchor {
            None => self.anchor = Some((self.travel, t)),
            Some((s0, t0)) if t > t0 + 1e-5 => {
                let inst = (self.travel - s0) / (t - t0);
                let tau = CALIB.get().map(|c| c[4][1]).filter(|v| *v > 0.0).unwrap_or(SPEED_TAU);
                let k = 1.0 - (-(t - t0) / tau).exp();
                self.speed += (inst - self.speed) * k;
                self.anchor = Some((self.travel, t));
            }
            _ => {}
        }
        self.speed
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_monotone_and_hit_the_points() {
        let c = [[0.0, 0.0], [0.5, 0.245], [0.927, 0.709], [1.0, 1.0]];
        let mut last = -1.0;
        for i in 0..=100 {
            let y = curve_eval(&c, i as f32 / 100.0);
            assert!(y >= last - 1e-6, "monótona");
            last = y;
        }
        assert!((curve_eval(&c, 0.5) - 0.245).abs() < 1e-4);
        assert_eq!(curve_eval(&[], 0.3), 0.3);
        let down = DynInput::slower_is_bigger(0.6);
        assert!((down.factor(0.0) - 1.0).abs() < 1e-6 && (down.factor(1.0) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn dynamics_multiply_and_random_stays_in_range() {
        let d = Dynamics { pressure: Some(DynInput::linear(0.2)), velocity: Some(DynInput::slower_is_bigger(0.5)), random: Some(0.8), ..Default::default() };
        assert!((d.base(1.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((d.base(0.0, 0.0) - 0.2).abs() < 1e-6);
        assert!((d.base(1.0, 1.0) - 0.5).abs() < 1e-6);
        assert!((d.factor(1.0, 0.0, 0.0) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn stabilizer_lags_but_reaches_the_end_and_by_speed_weakens_it() {
        let run = |cfg: Stabilization, step: f32| {
            let mut s = Stabilizer::new(cfg);
            let mut out = Vec::new();
            for i in 0..60 {
                out.push(s.feed(i as f32 * step, 0.0, i as f32 * 0.008));
            }
            let lag = 59.0 * step - out.last().unwrap().0;
            let tail = s.finish();
            (lag, tail.last().map(|p| p.0))
        };
        let (lag_strong, end) = run(Stabilization { level: 60, by_speed: false }, 7.0);
        assert!(lag_strong > 5.0, "nível alto atrasa: {lag_strong}");
        assert_eq!(end, Some(59.0 * 7.0), "alcança o último ponto");
        let (lag_speed, _) = run(Stabilization { level: 60, by_speed: true }, 7.0);
        assert!(lag_speed < lag_strong * 0.5, "pela velocidade, rápido estabiliza menos: {lag_speed} x {lag_strong}");
        let (lag0, _) = run(Stabilization { level: 0, by_speed: false }, 7.0);
        assert_eq!(lag0, 0.0);
    }

    #[test]
    fn speed_estimator_handles_samples_of_the_same_frame() {
        let mut e = SpeedEstimator::default();
        // 2 amostras por quadro de 16 ms, 7 px cada: 875 px/s
        let mut t = 0.0;
        for i in 0..80 {
            if i % 2 == 0 {
                t += 0.016;
            }
            e.push(7.0, t);
        }
        assert!((e.speed() - 875.0).abs() < 30.0, "{}", e.speed());
    }

    #[test]
    fn edge_noise_stays_inside_the_radius_and_varies() {
        let e = EdgeNoise { amount: 0.3, grain: 3.0, seed: 1 };
        let v: Vec<f32> = (0..400).map(|i| e.radius_at(i as f32 * 0.7, 13.0)).collect();
        assert!(v.iter().all(|r| (0.6999..=1.0001).contains(r)));
        let (mn, mx) = v.iter().fold((1.0f32, 0.0f32), |a, r| (a.0.min(*r), a.1.max(*r)));
        assert!(mx - mn > 0.15, "varia: {mn}..{mx}");
        let r = smooth_random(10.0, 25.0, 3);
        assert!((0.0..=1.0).contains(&r) && (smooth_random(10.0, 25.0, 3) - r).abs() < 1e-9);
    }
}
