//! AnimeIAI: só o gerador xorshift64* do `ops/smooth.rs` do ANIGO (a edição proporcional aleatória do `transform.rs`
//! usa); as ferramentas de suavização não foram trazidas.

// ----------------------------------------------------------------------------- PRNG

/// Gerador xorshift64* determinístico (sem dependências). Mesma semente → mesma
/// sequência em qualquer plataforma.
#[derive(Debug, Clone)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// Cria o gerador; a semente `0` é válida (é misturada com uma constante).
    pub fn new(seed: u64) -> Self {
        let mut s = Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        };
        if s.state == 0 {
            s.state = 0x2545_F491_4F6C_DD1D;
        }
        // aquece para descorrelacionar sementes vizinhas
        for _ in 0..4 {
            s.next_u64();
        }
        s
    }

    /// Gerador independente para o elemento `index` (ordem de iteração irrelevante).
    pub fn for_index(seed: u64, index: u64) -> Self {
        Self::new(seed.wrapping_add(index.wrapping_mul(0xD1B5_4A32_D192_ED03)))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniforme em `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) / (1u64 << 24) as f32
    }

    /// Uniforme em `[-1, 1)`.
    pub fn next_signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}
