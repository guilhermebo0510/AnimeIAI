//! Sistema de partículas 2D determinístico (S14). Cada partícula nasce num instante fixo (taxa ou rajada), com
//! parâmetros sorteados por um RNG semeado pelo seu índice, e é integrada do nascimento até o tempo pedido — então
//! qualquer quadro pode ser renderizado isoladamente e sempre igual (render paralelo, scrub, golden tests).

use aiai_paint::Image;
use aiai_paint::brush::{TipShape, shape_coverage};
use aiai_paint::noise::{Rng, perlin};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EmitterShape {
    Point { at: [f32; 2] },
    Line { a: [f32; 2], b: [f32; 2] },
    Rect { rect: [f32; 4] },
    Circle { center: [f32; 2], radius: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Sprite {
    /// Forma procedural (pétala, folha, estrela, nuvem, quadrado, rosa) ou círculo.
    Shape { shape: TipShape },
    Circle,
    /// Risco (chuva, faísca): comprimento em múltiplos do tamanho, alinhado à velocidade.
    Streak { length: f32 },
    /// Cels desenhados na própria camada de partículas (números); cada partícula sorteia um.
    Cels { numbers: Vec<u32> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ParticleSystem {
    #[serde(default)]
    pub seed: u64,
    pub emitter: EmitterShape,
    /// Partículas por segundo (contínuo).
    #[serde(default)]
    pub rate: f32,
    /// Rajadas: (quadro, quantidade).
    #[serde(default)]
    pub bursts: Vec<(u32, u32)>,
    /// Vida (s) mín/máx.
    pub lifetime: [f32; 2],
    /// Velocidade inicial (px/s) mín/máx, direção (graus, 0 = direita, 90 = baixo) e abertura (graus).
    pub speed: [f32; 2],
    #[serde(default = "d_dir")]
    pub direction: f32,
    #[serde(default)]
    pub spread: f32,
    /// Gravidade e vento (px/s²).
    #[serde(default)]
    pub gravity: [f32; 2],
    #[serde(default)]
    pub wind: [f32; 2],
    /// Turbulência: força (px/s²) e escala (px).
    #[serde(default)]
    pub turbulence: [f32; 2],
    /// Arrasto (1/s).
    #[serde(default)]
    pub drag: f32,
    /// Tamanho (px) mín/máx e curva pela vida [(t, mult)].
    pub size: [f32; 2],
    #[serde(default)]
    pub size_over_life: Vec<[f32; 2]>,
    #[serde(default)]
    pub opacity_over_life: Vec<[f32; 2]>,
    /// Rotação inicial e giro (graus, graus/s) mín/máx.
    #[serde(default)]
    pub rotation: [f32; 2],
    #[serde(default)]
    pub spin: [f32; 2],
    /// Giro 3D falso (pétala capotando): achata a largura com cos(fase), frequência (Hz) mín/máx.
    #[serde(default)]
    pub tumble: Option<[f32; 2]>,
    /// Cores sorteadas (sRGB + alfa).
    pub colors: Vec<[f32; 4]>,
    pub sprite: Sprite,
    /// Começa "cheio": simula estes segundos antes do quadro 0.
    #[serde(default)]
    pub prewarm: f32,
    #[serde(default = "d_max")]
    pub max_particles: u32,
    /// Cintilação (brilhos): a opacidade oscila nesta frequência (Hz).
    #[serde(default)]
    pub twinkle: f32,
}

fn d_dir() -> f32 {
    90.0
}
fn d_max() -> u32 {
    3000
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleState {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub size: f32,
    pub rotation: f32,
    pub opacity: f32,
    /// Fator de largura (giro 3D falso), 0..1.
    pub squash: f32,
    pub color: [f32; 4],
    /// Índice do sprite (Cels).
    pub variant: usize,
    pub life_t: f32,
}

fn curve(c: &[[f32; 2]], t: f32) -> f32 {
    if c.is_empty() {
        return 1.0;
    }
    if t <= c[0][0] {
        return c[0][1];
    }
    for w in c.windows(2) {
        if t <= w[1][0] {
            let f = (t - w[0][0]) / (w[1][0] - w[0][0]).max(1e-6);
            return w[0][1] + (w[1][1] - w[0][1]) * f;
        }
    }
    c[c.len() - 1][1]
}

fn lerp(r: [f32; 2], t: f32) -> f32 {
    r[0] + (r[1] - r[0]) * t
}

impl ParticleSystem {
    pub fn validate(&self) -> Result<(), String> {
        if self.lifetime[0] <= 0.0 || self.lifetime[1] < self.lifetime[0] {
            return Err("vida das partículas inválida".into());
        }
        if self.colors.is_empty() {
            return Err("pelo menos uma cor".into());
        }
        if self.rate < 0.0 || self.rate > 5000.0 {
            return Err("taxa fora de 0..5000/s".into());
        }
        if let Sprite::Cels { numbers } = &self.sprite {
            if numbers.is_empty() {
                return Err("sprite de cels sem números".into());
            }
        }
        Ok(())
    }

    /// Estado de todas as partículas vivas no tempo `t` (s desde o quadro 0), integrando a `fps`.
    pub fn simulate(&self, t: f32, fps: f32) -> Vec<ParticleState> {
        let dt = 1.0 / (fps.max(1.0) * 2.0);
        let mut spawns: Vec<f32> = Vec::new();
        if self.rate > 0.0 {
            let start = -self.prewarm.max(0.0);
            let first = (start * self.rate).ceil() as i64;
            let last = (t * self.rate).floor() as i64;
            for k in first..=last {
                spawns.push(k as f32 / self.rate);
            }
        }
        for (f, count) in &self.bursts {
            let ts = *f as f32 / fps;
            if ts <= t {
                for _ in 0..*count {
                    spawns.push(ts);
                }
            }
        }
        let max_life = self.lifetime[1];
        let mut out = Vec::new();
        for (idx, &ts) in spawns.iter().enumerate() {
            let age = t - ts;
            if age < 0.0 || age > max_life {
                continue;
            }
            // índice estável: taxa usa o instante; rajada usa a posição na lista (as rajadas vêm depois da taxa)
            let key = ((ts * 1000.0).round() as i64 as u64).wrapping_mul(0x9E37_79B9).wrapping_add(idx as u64);
            let mut rng = Rng::new(self.seed ^ key.wrapping_mul(0xD1B5_4A32_D192_ED03));
            let life = lerp(self.lifetime, rng.f32());
            if age > life {
                continue;
            }
            let (mut x, mut y) = match &self.emitter {
                EmitterShape::Point { at } => (at[0], at[1]),
                EmitterShape::Line { a, b } => {
                    let u = rng.f32();
                    (a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u)
                }
                EmitterShape::Rect { rect } => (rect[0] + rng.f32() * rect[2], rect[1] + rng.f32() * rect[3]),
                EmitterShape::Circle { center, radius } => {
                    let a = rng.f32() * std::f32::consts::TAU;
                    let r = radius * rng.f32().sqrt();
                    (center[0] + r * a.cos(), center[1] + r * a.sin())
                }
            };
            let dir = (self.direction + self.spread * rng.signed() * 0.5).to_radians();
            let sp = lerp(self.speed, rng.f32());
            let (mut vx, mut vy) = (dir.cos() * sp, dir.sin() * sp);
            let size0 = lerp(self.size, rng.f32());
            let rot0 = lerp(self.rotation, rng.f32());
            let spin = lerp(self.spin, rng.f32());
            let tumble = self.tumble.map(|r| lerp(r, rng.f32()));
            let tumble_phase = rng.f32() * std::f32::consts::TAU;
            let color = self.colors[(rng.f32() * self.colors.len() as f32) as usize % self.colors.len()];
            let variant = (rng.f32() * 1000.0) as usize;
            let tw_phase = rng.f32() * std::f32::consts::TAU;
            let tseed = (self.seed as u32) ^ (idx as u32).wrapping_mul(2654435761);
            let steps = (age / dt).floor() as usize;
            let rem = age - steps as f32 * dt;
            let mut tt = 0.0f32;
            for s in 0..=steps {
                let h = if s == steps { rem } else { dt };
                if h <= 0.0 {
                    continue;
                }
                let (mut ax, mut ay) = (self.gravity[0] + self.wind[0], self.gravity[1] + self.wind[1]);
                if self.turbulence[0] != 0.0 {
                    let sc = self.turbulence[1].max(1.0);
                    ax += self.turbulence[0] * perlin(x / sc, y / sc + tt * 0.5, tseed);
                    ay += self.turbulence[0] * perlin(x / sc + 31.7, y / sc - tt * 0.5, tseed ^ 0x55);
                }
                vx += (ax - self.drag * vx) * h;
                vy += (ay - self.drag * vy) * h;
                x += vx * h;
                y += vy * h;
                tt += h;
            }
            let lt = (age / life).clamp(0.0, 1.0);
            let mut opacity = curve(&self.opacity_over_life, lt);
            if self.twinkle > 0.0 {
                opacity *= 0.55 + 0.45 * (age * self.twinkle * std::f32::consts::TAU + tw_phase).sin();
            }
            out.push(ParticleState {
                x,
                y,
                vx,
                vy,
                size: size0 * curve(&self.size_over_life, lt),
                rotation: rot0 + spin * age,
                opacity: opacity.clamp(0.0, 1.0),
                squash: tumble.map(|f| (age * f * std::f32::consts::TAU + tumble_phase).cos().abs().max(0.08)).unwrap_or(1.0),
                color,
                variant,
                life_t: lt,
            });
            if out.len() >= self.max_particles as usize {
                break;
            }
        }
        out
    }

    /// Renderiza as partículas numa imagem `w × h`. `sprites`: imagens dos cels (para `Sprite::Cels`), na ordem de
    /// `numbers`.
    pub fn render(&self, w: u32, h: u32, t: f32, fps: f32, sprites: &[Image]) -> Image {
        let mut img = Image::new(w, h);
        for p in self.simulate(t, fps) {
            if p.opacity <= 0.0 || p.size <= 0.1 {
                continue;
            }
            let r = p.size * 0.5;
            let (len_mul, ang) = match &self.sprite {
                Sprite::Streak { length } => (length.max(1.0), p.vy.atan2(p.vx).to_degrees() + 90.0),
                _ => (1.0, p.rotation),
            };
            let reach = r * len_mul.max(1.0) + 2.0;
            let (x0, y0) = (((p.x - reach).floor() as i32).max(0), ((p.y - reach).floor() as i32).max(0));
            let (x1, y1) = (((p.x + reach).ceil() as i32).min(w as i32), ((p.y + reach).ceil() as i32).min(h as i32));
            let (sn, cs) = ang.to_radians().sin_cos();
            let premul = |a: f32| [p.color[0] * a, p.color[1] * a, p.color[2] * a, a];
            for y in y0..y1 {
                for x in x0..x1 {
                    let (dx, dy) = (x as f32 + 0.5 - p.x, y as f32 + 0.5 - p.y);
                    // espaço local: (u, v) ∈ [-1, 1], v para "cima" da partícula
                    let lx = dx * cs + dy * sn;
                    let ly = -dx * sn + dy * cs;
                    let u = lx / (r * p.squash).max(0.05);
                    let v = ly / (r * len_mul);
                    let cov = match &self.sprite {
                        Sprite::Shape { shape } => shape_coverage(*shape, u, v, 1.0 / r.max(0.5)),
                        Sprite::Circle | Sprite::Streak { .. } => {
                            let d = (u * u + v * v).sqrt();
                            ((1.0 - d) * r.max(1.0)).clamp(0.0, 1.0)
                        }
                        Sprite::Cels { .. } => {
                            if sprites.is_empty() {
                                0.0
                            } else {
                                let sp = &sprites[p.variant % sprites.len()];
                                let (su, sv) = ((u * 0.5 + 0.5) * sp.width as f32, (v * 0.5 + 0.5) * sp.height as f32);
                                if su < 0.0 || sv < 0.0 || su >= sp.width as f32 || sv >= sp.height as f32 {
                                    0.0
                                } else {
                                    let px = sp.get(su as u32, sv as u32);
                                    // sprite colorido: usa a cor do próprio cel, modulada pela opacidade
                                    let a = px[3] * p.opacity * p.color[3];
                                    if a > 0.0 {
                                        let i = img.idx(x as u32, y as u32);
                                        let s = if px[3] > 0.0 { a / px[3] } else { 0.0 };
                                        Image::over(&mut img.data[i], [px[0] * s, px[1] * s, px[2] * s, a], 1.0);
                                    }
                                    continue;
                                }
                            }
                        }
                    };
                    let a = cov * p.opacity * p.color[3];
                    if a > 0.0 {
                        let i = img.idx(x as u32, y as u32);
                        Image::over(&mut img.data[i], premul(a), 1.0);
                    }
                }
            }
        }
        img
    }
}

/// Presets típicos de anime.
pub fn preset(name: &str, w: f32, h: f32, seed: u64) -> Option<ParticleSystem> {
    let base = |emitter, sprite, colors: Vec<[f32; 4]>| ParticleSystem {
        seed,
        emitter,
        rate: 0.0,
        bursts: vec![],
        lifetime: [3.0, 5.0],
        speed: [30.0, 80.0],
        direction: 90.0,
        spread: 30.0,
        gravity: [0.0, 0.0],
        wind: [0.0, 0.0],
        turbulence: [0.0, 100.0],
        drag: 0.0,
        size: [10.0, 18.0],
        size_over_life: vec![],
        opacity_over_life: vec![[0.0, 0.0], [0.1, 1.0], [0.85, 1.0], [1.0, 0.0]],
        rotation: [0.0, 360.0],
        spin: [-90.0, 90.0],
        tumble: None,
        colors,
        sprite,
        prewarm: 4.0,
        max_particles: 3000,
        twinkle: 0.0,
    };
    let top = EmitterShape::Line { a: [-0.1 * w, -20.0], b: [1.1 * w, -20.0] };
    Some(match name {
        "petalas" | "rose_petals" => {
            let mut p = base(top, Sprite::Shape { shape: TipShape::Petal }, vec![[0.93, 0.36, 0.45, 1.0], [0.98, 0.55, 0.62, 1.0], [0.85, 0.25, 0.35, 1.0]]);
            p.rate = 10.0;
            p.speed = [40.0, 90.0];
            p.direction = 100.0;
            p.wind = [25.0, 0.0];
            p.gravity = [0.0, 8.0];
            p.turbulence = [60.0, 160.0];
            p.tumble = Some([0.4, 1.2]);
            p.lifetime = [h / 50.0, h / 30.0];
            p
        }
        "brilhos" | "sparkles" => {
            let mut p = base(EmitterShape::Rect { rect: [0.0, 0.0, w, h] }, Sprite::Shape { shape: TipShape::Star }, vec![[1.0, 1.0, 0.9, 1.0], [1.0, 0.95, 0.7, 1.0]]);
            p.rate = 8.0;
            p.speed = [0.0, 6.0];
            p.lifetime = [0.6, 1.4];
            p.size = [8.0, 22.0];
            p.size_over_life = vec![[0.0, 0.2], [0.5, 1.0], [1.0, 0.2]];
            p.spin = [-30.0, 30.0];
            p.twinkle = 3.0;
            p.prewarm = 1.5;
            p
        }
        "neve" | "snow" => {
            let mut p = base(top, Sprite::Circle, vec![[1.0, 1.0, 1.0, 0.95]]);
            p.rate = 40.0;
            p.speed = [30.0, 60.0];
            p.size = [3.0, 8.0];
            p.turbulence = [25.0, 120.0];
            p.lifetime = [h / 30.0, h / 20.0];
            p
        }
        "chuva" | "rain" => {
            let mut p = base(top, Sprite::Streak { length: 6.0 }, vec![[0.75, 0.82, 0.95, 0.55]]);
            p.rate = 220.0;
            p.speed = [900.0, 1200.0];
            p.direction = 100.0;
            p.spread = 3.0;
            p.size = [1.5, 2.5];
            p.lifetime = [h / 900.0 + 0.1, h / 700.0 + 0.2];
            p.opacity_over_life = vec![];
            p.prewarm = 1.0;
            p
        }
        "poeira" | "dust" => {
            let mut p = base(EmitterShape::Rect { rect: [0.0, 0.0, w, h] }, Sprite::Circle, vec![[1.0, 0.95, 0.8, 0.6]]);
            p.rate = 20.0;
            p.speed = [2.0, 10.0];
            p.size = [2.0, 5.0];
            p.turbulence = [6.0, 200.0];
            p.lifetime = [3.0, 6.0];
            p
        }
        "folhas" | "leaves" => {
            let mut p = base(top, Sprite::Shape { shape: TipShape::Leaf }, vec![[0.45, 0.62, 0.25, 1.0], [0.75, 0.6, 0.2, 1.0], [0.6, 0.4, 0.15, 1.0]]);
            p.rate = 4.0;
            p.wind = [40.0, 0.0];
            p.tumble = Some([0.3, 0.8]);
            p.turbulence = [50.0, 180.0];
            p.lifetime = [h / 40.0, h / 25.0];
            p
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_frames_independent() {
        let p = preset("petalas", 640.0, 360.0, 7).unwrap();
        p.validate().unwrap();
        let a = p.render(640, 360, 1.0, 24.0, &[]);
        let b = p.render(640, 360, 1.0, 24.0, &[]);
        assert_eq!(a.content_hash(), b.content_hash());
        let c = p.render(640, 360, 1.5, 24.0, &[]);
        assert_ne!(a.content_hash(), c.content_hash(), "anima");
        assert!(p.simulate(1.0, 24.0).len() > 10, "começa cheio (prewarm)");
        for name in ["brilhos", "neve", "chuva", "poeira", "folhas"] {
            let s = preset(name, 640.0, 360.0, 1).unwrap();
            let img = s.render(640, 360, 2.0, 24.0, &[]);
            assert!(!img.is_blank(), "{name} desenha algo");
        }
    }

    #[test]
    fn burst_spawns_at_frame() {
        let mut p = preset("brilhos", 200.0, 200.0, 3).unwrap();
        p.rate = 0.0;
        p.prewarm = 0.0;
        p.bursts = vec![(12, 20)];
        assert!(p.simulate(11.0 / 24.0, 24.0).is_empty());
        assert_eq!(p.simulate(12.5 / 24.0, 24.0).len(), 20);
    }
}
