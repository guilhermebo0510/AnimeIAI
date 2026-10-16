//! Ruído procedural determinístico (texturas de pincel, papel, nuvem, pedra...). Sem estado global: tudo derivado de
//! uma semente, então o mesmo parâmetro gera sempre a mesma textura (golden tests, reprodutibilidade de projeto).

/// Gerador SplitMix64 (jitter de pincel, dispersão, partículas).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniforme em [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniforme em [-1, 1).
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
}

#[inline]
fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^ (h >> 15)
}

#[inline]
fn hash_f(x: i32, y: i32, seed: u32) -> f32 {
    (hash2(x, y, seed) & 0x00ff_ffff) as f32 / 16_777_215.0
}

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Ruído de gradiente 2D (Perlin) em [-1, 1] aprox.
pub fn perlin(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (xf, yf) = (x - xi as f32, y - yi as f32);
    let grad = |ix: i32, iy: i32, dx: f32, dy: f32| {
        let a = hash_f(ix, iy, seed) * std::f32::consts::TAU;
        a.cos() * dx + a.sin() * dy
    };
    let n00 = grad(xi, yi, xf, yf);
    let n10 = grad(xi + 1, yi, xf - 1.0, yf);
    let n01 = grad(xi, yi + 1, xf, yf - 1.0);
    let n11 = grad(xi + 1, yi + 1, xf - 1.0, yf - 1.0);
    let (u, v) = (fade(xf), fade(yf));
    let a = n00 + (n10 - n00) * u;
    let b = n01 + (n11 - n01) * u;
    (a + (b - a) * v) * std::f32::consts::SQRT_2
}

/// Ruído de valor 2D em [0, 1].
pub fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (u, v) = (fade(x - xi as f32), fade(y - yi as f32));
    let a = hash_f(xi, yi, seed);
    let b = hash_f(xi + 1, yi, seed);
    let c = hash_f(xi, yi + 1, seed);
    let d = hash_f(xi + 1, yi + 1, seed);
    let top = a + (b - a) * u;
    let bot = c + (d - c) * u;
    top + (bot - top) * v
}

/// fBm de Perlin normalizado para [0, 1].
pub fn fbm(x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32, seed: u32) -> f32 {
    let (mut amp, mut freq, mut sum, mut norm) = (1.0f32, 1.0f32, 0.0f32, 0.0f32);
    for o in 0..octaves.max(1) {
        sum += amp * perlin(x * freq, y * freq, seed.wrapping_add(o * 1013));
        norm += amp;
        amp *= gain;
        freq *= lacunarity;
    }
    (sum / norm * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Ruído celular (Worley): distância ao ponto de célula mais próximo (F1), normalizada aprox. para [0, 1].
pub fn worley(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let mut best = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let px = cx as f32 + hash_f(cx, cy, seed);
            let py = cy as f32 + hash_f(cx, cy, seed ^ 0x5555);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            best = best.min(d);
        }
    }
    (best / std::f32::consts::SQRT_2).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_bounded() {
        assert_eq!(perlin(3.3, 7.1, 9), perlin(3.3, 7.1, 9));
        assert_ne!(perlin(3.3, 7.1, 9), perlin(3.3, 7.1, 10));
        for i in 0..2000 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.11);
            let f = fbm(x, y, 5, 2.0, 0.5, 1);
            let w = worley(x, y, 1);
            let v = value_noise(x, y, 1);
            assert!((0.0..=1.0).contains(&f) && (0.0..=1.0).contains(&w) && (0.0..=1.0).contains(&v));
        }
        let mut r = Rng::new(42);
        let a: Vec<f32> = (0..5).map(|_| r.f32()).collect();
        let mut r2 = Rng::new(42);
        let b: Vec<f32> = (0..5).map(|_| r2.f32()).collect();
        assert_eq!(a, b);
    }
}
