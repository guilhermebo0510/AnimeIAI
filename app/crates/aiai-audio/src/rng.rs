//! Gerador pseudoaleatório próprio (SplitMix64 + xorshift): resultado fixo por semente em qualquer versão de dependência,
//! requisito de reprodutibilidade dos SFX e da BGM.

#[derive(Debug, Clone)]
pub struct Rng {
    s: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // SplitMix64 para espalhar sementes pequenas.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng { s: if z == 0 { 0x1234_5678_9ABC_DEF1 } else { z } }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniforme em [0, 1).
    pub fn f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Uniforme em [-1, 1).
    pub fn bipolar(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }
}
