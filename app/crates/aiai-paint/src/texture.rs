//! Texturas procedurais para cenário e materiais (S11). Todas determinísticas (semente), opcionalmente "tileáveis"
//! (repetem sem emenda) e geradas em qualquer resolução. Cores em sRGB não pré-multiplicado.

use crate::image::Image;
use crate::noise::{Rng, fbm, value_noise, worley};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextureSpec {
    /// Papel: fibras e grão suaves.
    Paper { scale: f32 },
    /// Tela de pintura (trama).
    Canvas { scale: f32 },
    /// Ruído fractal (fBm).
    Noise { scale: f32, #[serde(default = "d_oct")] octaves: u32 },
    /// Nuvens (fBm com limiar suave).
    Clouds { scale: f32, coverage: f32, softness: f32 },
    /// Manchas de aquarela.
    Watercolor { scale: f32 },
    Grain { scale: f32 },
    /// Pedra/rocha (células + fBm).
    Stone { scale: f32 },
    Marble { scale: f32, veins: f32 },
    /// Madeira (anéis + fibras), veio na horizontal.
    Wood { scale: f32, rings: f32 },
    /// Tijolos: tamanho do tijolo (px), argamassa (px), variação de cor 0..1.
    Brick { brick: [f32; 2], mortar: f32, variation: f32 },
    /// Telhas em escama.
    RoofTiles { tile: [f32; 2], variation: f32 },
    Plaster { scale: f32 },
    /// Grama (lâminas verticais).
    Grass { scale: f32, density: f32 },
    /// Folhagem (aglomerados de folhas).
    Foliage { scale: f32 },
    /// Tecido (trama fina).
    Fabric { scale: f32 },
}

fn d_oct() -> u32 {
    5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextureParams {
    pub spec: TextureSpec,
    /// Cores (sRGB 0..1): `dark` nos vales, `light` nos picos; `accent` para argamassa/veios/realces.
    pub dark: [f32; 3],
    pub light: [f32; 3],
    #[serde(default)]
    pub accent: Option<[f32; 3]>,
    #[serde(default)]
    pub seed: u32,
    /// Contraste (1 = normal).
    #[serde(default = "d_one")]
    pub contrast: f32,
    /// Alfa geral (0..1).
    #[serde(default = "d_one")]
    pub opacity: f32,
}

fn d_one() -> f32 {
    1.0
}

/// Valor 0..1 (e opcionalmente máscara de "acento") da textura no ponto (x, y) em px.
fn sample(spec: &TextureSpec, x: f32, y: f32, seed: u32) -> (f32, f32) {
    match spec {
        TextureSpec::Paper { scale } => {
            let s = scale.max(0.5);
            let fib = value_noise(x / (s * 6.0), y / (s * 0.8), seed);
            (0.6 * fbm(x / (s * 8.0), y / (s * 8.0), 4, 2.0, 0.5, seed ^ 3) + 0.25 * fib + 0.15 * value_noise(x / s, y / s, seed ^ 9), 0.0)
        }
        TextureSpec::Canvas { scale } => {
            let s = scale.max(0.5);
            let (u, v) = (x / s, y / s);
            let wx = (u * std::f32::consts::PI).sin().abs();
            let wy = (v * std::f32::consts::PI).sin().abs();
            let over = if ((u.floor() + v.floor()) as i32).rem_euclid(2) == 0 { wx } else { wy };
            (0.45 + 0.35 * over + 0.2 * value_noise(u * 0.5, v * 0.5, seed), 0.0)
        }
        TextureSpec::Noise { scale, octaves } => (fbm(x / scale.max(0.5), y / scale.max(0.5), *octaves, 2.0, 0.5, seed), 0.0),
        TextureSpec::Clouds { scale, coverage, softness } => {
            let n = fbm(x / scale.max(1.0), y / scale.max(1.0), 6, 2.0, 0.52, seed);
            let t = 1.0 - coverage.clamp(0.0, 1.0);
            let s = softness.clamp(0.01, 1.0) * 0.5;
            let v = ((n - t + s) / (2.0 * s)).clamp(0.0, 1.0);
            (v * v * (3.0 - 2.0 * v), 0.0)
        }
        TextureSpec::Watercolor { scale } => {
            let s = scale.max(1.0);
            let warp = fbm(x / (s * 2.0), y / (s * 2.0), 3, 2.0, 0.5, seed ^ 5) * s * 1.5;
            let n = fbm((x + warp) / s, (y + warp) / s, 5, 2.1, 0.55, seed);
            let edge = ((n - 0.5).abs() * 6.0).min(1.0);
            (n * 0.8 + (1.0 - edge) * 0.2, 0.0)
        }
        TextureSpec::Grain { scale } => (value_noise(x / scale.max(0.3), y / scale.max(0.3), seed), 0.0),
        TextureSpec::Stone { scale } => {
            let s = scale.max(1.0);
            let c = worley(x / s, y / s, seed);
            let f = fbm(x / (s * 0.3), y / (s * 0.3), 4, 2.0, 0.5, seed ^ 7);
            let crack = if c < 0.06 { 1.0 } else { 0.0 };
            ((1.0 - c) * 0.5 + f * 0.5, crack)
        }
        TextureSpec::Marble { scale, veins } => {
            let s = scale.max(1.0);
            let t = fbm(x / s, y / s, 5, 2.0, 0.5, seed) * veins * 6.0;
            let v = ((x / s + t) * std::f32::consts::PI).sin().abs();
            (1.0 - v.powf(0.25) * 0.4, if v < 0.08 { 1.0 } else { 0.0 })
        }
        TextureSpec::Wood { scale, rings } => {
            let s = scale.max(1.0);
            let wobble = fbm(x / (s * 4.0), y / (s * 0.6), 3, 2.0, 0.5, seed) * rings;
            let r = ((y / s + wobble) * rings).fract();
            let fiber = value_noise(x / (s * 3.0), y / (s * 0.15), seed ^ 11);
            (0.55 + 0.3 * (r * std::f32::consts::TAU).sin() * 0.5 + 0.15 * fiber, 0.0)
        }
        TextureSpec::Brick { brick, mortar, variation } => {
            let (bw, bh) = (brick[0].max(2.0), brick[1].max(2.0));
            let row = (y / bh).floor();
            let off = if (row as i32).rem_euclid(2) == 1 { bw * 0.5 } else { 0.0 };
            let col = ((x + off) / bw).floor();
            let (lx, ly) = ((x + off) - col * bw, y - row * bh);
            let m = mortar.max(0.0);
            let is_mortar = lx < m || ly < m;
            let tone = 0.5 + (value_noise(col * 7.13, row * 3.71, seed) - 0.5) * variation * 2.0;
            let surface = 0.85 + 0.15 * fbm(x / 4.0, y / 4.0, 3, 2.0, 0.5, seed ^ 13);
            ((tone * surface).clamp(0.0, 1.0), if is_mortar { 1.0 } else { 0.0 })
        }
        TextureSpec::RoofTiles { tile, variation } => {
            let (tw, th) = (tile[0].max(2.0), tile[1].max(2.0));
            let row = (y / th).floor();
            let off = if (row as i32).rem_euclid(2) == 1 { tw * 0.5 } else { 0.0 };
            let col = ((x + off) / tw).floor();
            let (lx, ly) = (((x + off) - col * tw) / tw - 0.5, (y - row * th) / th);
            // escama: borda inferior arredondada
            let edge = ly - (1.0 - (lx * 2.0).powi(2)).max(0.0).sqrt() * 0.35 - 0.62;
            let shade = (1.0 - ly * 0.5).clamp(0.0, 1.0);
            let tone = 0.5 + (value_noise(col * 5.1, row * 2.3, seed) - 0.5) * variation * 2.0;
            ((tone * shade).clamp(0.0, 1.0), if edge > 0.0 && edge < 0.08 { 1.0 } else { 0.0 })
        }
        TextureSpec::Plaster { scale } => {
            let s = scale.max(1.0);
            let n = fbm(x / s, y / s, 6, 2.2, 0.6, seed);
            (0.7 + (n - 0.5) * 0.6, 0.0)
        }
        TextureSpec::Grass { scale, density } => {
            let s = scale.max(1.0);
            // colunas de lâminas: altura variável por coluna, afinando para cima
            let col = (x / (s * 0.35)).floor();
            let h = 0.5 + value_noise(col * 1.7, 0.0, seed) * 0.5;
            let ly = (y / (s * 2.0)).fract();
            let blade = if ly > 1.0 - h * density.clamp(0.1, 1.0) { 1.0 } else { 0.0 };
            let tone = value_noise(col * 3.3, (y / (s * 2.0)).floor(), seed ^ 21);
            (0.15 + 0.55 * tone * blade + 0.3 * blade, 0.0)
        }
        TextureSpec::Foliage { scale } => {
            let s = scale.max(1.0);
            let c = worley(x / s, y / s, seed);
            let leaf = (1.0 - c * 1.6).clamp(0.0, 1.0);
            let light = fbm(x / (s * 3.0), y / (s * 3.0), 3, 2.0, 0.5, seed ^ 31);
            (leaf * 0.6 + light * 0.4, if c > 0.55 { 1.0 } else { 0.0 })
        }
        TextureSpec::Fabric { scale } => {
            let s = scale.max(0.5);
            let (u, v) = (x / s, y / s);
            let a = (u * std::f32::consts::PI * 2.0).sin() * (v * std::f32::consts::PI).sin();
            (0.5 + 0.3 * a + 0.2 * value_noise(u * 0.3, v * 0.3, seed), 0.0)
        }
    }
}

impl TextureParams {
    /// Cor straight (r, g, b, a) no ponto.
    pub fn color_at(&self, x: f32, y: f32) -> [f32; 4] {
        let (v, acc) = sample(&self.spec, x, y, self.seed);
        let v = ((v - 0.5) * self.contrast + 0.5).clamp(0.0, 1.0);
        let mut c = [0.0f32; 3];
        for k in 0..3 {
            c[k] = self.dark[k] + (self.light[k] - self.dark[k]) * v;
        }
        if acc > 0.0 {
            if let Some(a) = self.accent {
                for k in 0..3 {
                    c[k] += (a[k] - c[k]) * acc;
                }
            }
        }
        [c[0], c[1], c[2], self.opacity]
    }

    /// Imagem `w × h` da textura (pré-multiplicada).
    pub fn render(&self, w: u32, h: u32) -> Image {
        let mut img = Image::new(w, h);
        img.data.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
            for (x, px) in row.iter_mut().enumerate() {
                let c = self.color_at(x as f32 + 0.5, y as f32 + 0.5);
                *px = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
            }
        });
        img
    }

    pub fn validate(&self) -> Result<(), String> {
        let ok = |c: &[f32; 3]| c.iter().all(|v| (0.0..=1.0).contains(v));
        if !ok(&self.dark) || !ok(&self.light) || self.accent.as_ref().is_some_and(|a| !ok(a)) {
            return Err("cores da textura fora de 0..1".into());
        }
        if !(0.0..=1.0).contains(&self.opacity) || !(0.0..=10.0).contains(&self.contrast) {
            return Err("opacidade/contraste fora do intervalo".into());
        }
        Ok(())
    }
}

/// Céu de anime por hora do dia: degradê (zênite → horizonte) + nuvens cúmulo estilizadas (base sombreada, borda nítida).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SkySpec {
    /// "day" | "morning" | "sunset" | "dusk" | "night" ou cores explícitas.
    #[serde(default = "d_tod")]
    pub time_of_day: String,
    #[serde(default)]
    pub zenith: Option<[f32; 3]>,
    #[serde(default)]
    pub horizon: Option<[f32; 3]>,
    /// Linha do horizonte (px).
    pub horizon_y: f32,
    /// Cobertura de nuvens 0..1 (0 = céu limpo).
    #[serde(default = "d_cov")]
    pub clouds: f32,
    /// Tamanho das nuvens (px).
    #[serde(default = "d_cscale")]
    pub cloud_scale: f32,
    #[serde(default)]
    pub seed: u32,
}

fn d_tod() -> String {
    "day".into()
}
fn d_cov() -> f32 {
    0.35
}
fn d_cscale() -> f32 {
    220.0
}

fn tod_colors(tod: &str) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3]) {
    // zênite, horizonte, nuvem iluminada, nuvem sombra
    match tod {
        "morning" => ([0.36, 0.60, 0.90], [0.92, 0.90, 0.84], [1.0, 0.98, 0.94], [0.74, 0.78, 0.90]),
        "sunset" => ([0.30, 0.36, 0.66], [1.0, 0.66, 0.42], [1.0, 0.84, 0.66], [0.62, 0.42, 0.56]),
        "dusk" => ([0.14, 0.16, 0.38], [0.78, 0.46, 0.50], [0.86, 0.62, 0.66], [0.32, 0.28, 0.46]),
        "night" => ([0.03, 0.05, 0.14], [0.10, 0.16, 0.32], [0.30, 0.34, 0.50], [0.10, 0.12, 0.22]),
        _ => ([0.22, 0.50, 0.88], [0.72, 0.87, 0.98], [1.0, 1.0, 1.0], [0.72, 0.80, 0.92]),
    }
}

impl SkySpec {
    pub fn render(&self, w: u32, h: u32) -> Image {
        let (mut z, mut hz, cl, cs) = tod_colors(&self.time_of_day);
        if let Some(c) = self.zenith {
            z = c;
        }
        if let Some(c) = self.horizon {
            hz = c;
        }
        let horizon_y = self.horizon_y.max(1.0);
        let mut img = Image::new(w, h);
        let cov = self.clouds.clamp(0.0, 1.0);
        let s = self.cloud_scale.max(10.0);
        img.data.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
            let yf = y as f32 + 0.5;
            let t = (yf / horizon_y).clamp(0.0, 1.0).powf(1.6);
            let mut base = [0.0f32; 3];
            for k in 0..3 {
                base[k] = z[k] + (hz[k] - z[k]) * t;
            }
            for (x, px) in row.iter_mut().enumerate() {
                let xf = x as f32 + 0.5;
                let mut c = base;
                if cov > 0.0 && yf < horizon_y {
                    // nuvens achatadas perto do horizonte (perspectiva)
                    let squash = 1.0 + 2.5 * t;
                    let n = fbm(xf / s, yf * squash / s, 6, 2.0, 0.5, self.seed);
                    let thr = 1.0 - cov;
                    let d = ((n - thr) / 0.06).clamp(0.0, 1.0); // borda nítida, estilo anime
                    if d > 0.0 {
                        // sombreado: compara com a densidade um pouco acima (luz vem de cima)
                        let up = fbm(xf / s, (yf - s * 0.08) * squash / s, 6, 2.0, 0.5, self.seed);
                        let lit = ((n - up) * 12.0 + 0.6).clamp(0.0, 1.0);
                        let mut cc = [0.0f32; 3];
                        for k in 0..3 {
                            cc[k] = cs[k] + (cl[k] - cs[k]) * lit;
                            c[k] += (cc[k] - c[k]) * d * (1.0 - t * 0.5);
                        }
                    }
                }
                *px = [c[0], c[1], c[2], 1.0];
            }
        });
        let mut rng = Rng::new(self.seed as u64);
        if self.time_of_day == "night" {
            // estrelas
            for _ in 0..(w * h / 2500) {
                let (x, y) = ((rng.f32() * w as f32) as u32, (rng.f32() * horizon_y.min(h as f32)) as u32);
                if x < w && y < h {
                    let b = 0.6 + 0.4 * rng.f32();
                    let i = img.idx(x, y);
                    img.data[i] = [b, b, b * 0.95, 1.0];
                }
            }
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_textures_render_with_variation_and_deterministic() {
        let specs = vec![
            TextureSpec::Paper { scale: 3.0 },
            TextureSpec::Canvas { scale: 4.0 },
            TextureSpec::Noise { scale: 20.0, octaves: 5 },
            TextureSpec::Clouds { scale: 60.0, coverage: 0.5, softness: 0.3 },
            TextureSpec::Watercolor { scale: 30.0 },
            TextureSpec::Grain { scale: 1.0 },
            TextureSpec::Stone { scale: 25.0 },
            TextureSpec::Marble { scale: 40.0, veins: 1.0 },
            TextureSpec::Wood { scale: 10.0, rings: 1.5 },
            TextureSpec::Brick { brick: [40.0, 16.0], mortar: 3.0, variation: 0.3 },
            TextureSpec::RoofTiles { tile: [30.0, 20.0], variation: 0.2 },
            TextureSpec::Plaster { scale: 15.0 },
            TextureSpec::Grass { scale: 8.0, density: 0.8 },
            TextureSpec::Foliage { scale: 12.0 },
            TextureSpec::Fabric { scale: 3.0 },
        ];
        for spec in specs {
            let p = TextureParams { spec: spec.clone(), dark: [0.2, 0.2, 0.2], light: [0.9, 0.9, 0.9], accent: Some([0.5, 0.4, 0.3]), seed: 3, contrast: 1.0, opacity: 1.0 };
            let a = p.render(96, 96);
            let b = p.render(96, 96);
            assert_eq!(a.content_hash(), b.content_hash(), "{spec:?} determinística");
            let lum: Vec<f32> = a.data.iter().map(|q| q[0]).collect();
            let (mn, mx) = lum.iter().fold((1.0f32, 0.0f32), |(a, b), &v| (a.min(v), b.max(v)));
            assert!(mx - mn > 0.1, "{spec:?} tem variação: {mn}..{mx}");
        }
    }

    #[test]
    fn sky_gradient_and_clouds() {
        let sky = SkySpec { time_of_day: "day".into(), zenith: None, horizon: None, horizon_y: 300.0, clouds: 0.4, cloud_scale: 120.0, seed: 1 };
        let img = sky.render(320, 360);
        let top = img.get(5, 2);
        let hor = img.get(5, 298);
        assert!(top[2] > top[0] + 0.3 && hor[0] > top[0], "zênite azul, horizonte claro");
        let whites = img.data.iter().filter(|p| p[0] > 0.9 && p[1] > 0.9).count();
        assert!(whites > 500, "há nuvens claras: {whites}");
    }
}
