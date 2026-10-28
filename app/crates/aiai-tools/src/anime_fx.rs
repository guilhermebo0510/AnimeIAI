//! Efeitos de anime desenhados (auditoria 4.5j): fogo, fumaça e raio como formas chapadas de cel (2 a 3 tons, bordas
//! limpas), pintados sobre o cel raster do quadro. `seed` muda o desenho; `phase` (0..1, por exemplo quadro/duração do
//! ciclo) faz a chama tremular, a fumaça andar e o raio mudar de caminho, para animar quadro a quadro. Sem IA: geometria.

use crate::context::*;
use crate::paint::{cel_raster, tiles_for_region};
use aiai_core::*;
use aiai_paint::poly::fill_polygons;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnimeFxRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada raster que recebe o desenho (com cel no quadro).
    pub layer: Id,
    pub kind: AnimeFxKind,
    /// Muda o desenho (formas diferentes com os mesmos parâmetros).
    #[serde(default)]
    pub seed: u64,
    /// 0..1: o momento do ciclo (tremular, subir, mudar o caminho). Para animar, um valor por quadro.
    #[serde(default)]
    pub phase: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnimeFxKind {
    /// Chama com línguas: `base` = centro da base (px), `height` = altura da chama.
    Fire { base: [f32; 2], height: f32 },
    /// Fumaça em nuvens recortadas: `center` da primeira, `radius` dela, `puffs` nuvens subindo na direção `drift` (px por nuvem).
    Smoke {
        center: [f32; 2],
        radius: f32,
        #[serde(default = "three")]
        puffs: u32,
        #[serde(default = "up")]
        drift: [f32; 2],
    },
    /// Raio em zigue-zague de `from` a `to`, com galhos; `width` = espessura do brilho (px).
    Lightning {
        from: [f32; 2],
        to: [f32; 2],
        #[serde(default = "bolt_width")]
        width: f32,
    },
}

fn three() -> u32 {
    3
}
fn up() -> [f32; 2] {
    [12.0, -60.0]
}
fn bolt_width() -> f32 {
    10.0
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

/// Curva quadrática de `a` a `b` com o meio empurrado `bulge` (fração do comprimento) para a esquerda do caminho.
fn bend(out: &mut Vec<[f32; 2]>, a: [f32; 2], b: [f32; 2], bulge: f32, steps: usize) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let c = [(a[0] + b[0]) / 2.0 - dy * bulge, (a[1] + b[1]) / 2.0 + dx * bulge];
    for k in 1..=steps {
        let t = k as f32 / steps as f32;
        let u = 1.0 - t;
        out.push([u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1]]);
    }
}

/// Contorno de uma chama: base arredondada e `n` línguas de pontas agudas, que tremulam com `phase`.
fn flame(base: [f32; 2], height: f32, n: usize, rng: &mut Rng, phase: f32) -> Vec<[f32; 2]> {
    let bw = height * 0.34;
    let mut pts = Vec::new();
    // base: meia-lua de baixo, da direita para a esquerda
    for k in 0..=16 {
        let a = k as f32 / 16.0 * std::f32::consts::PI;
        pts.push([base[0] + bw * a.cos(), base[1] + bw * 0.45 * a.sin()]);
    }
    // línguas da esquerda para a direita: vale, ponta, vale...
    let mut prev = [base[0] - bw, base[1]];
    for i in 0..n {
        let u = (i as f32 + 0.5) / n as f32;
        let x = base[0] - bw + 2.0 * bw * u;
        let center = 1.0 - (u - 0.5).abs() * 1.3;
        let flick = 1.0 + 0.16 * (TAU * (phase + rng.range(0.0, 1.0))).sin();
        let h = height * center.max(0.35) * rng.range(0.75, 1.0) * flick;
        let lean = height * 0.12 * (TAU * (phase * 1.3 + u)).sin();
        let tip = [x + lean, base[1] - h];
        bend(&mut pts, prev, tip, -0.12, 8);
        let valley_x = base[0] - bw + 2.0 * bw * ((i + 1) as f32 / n as f32);
        let valley = if i + 1 == n { [base[0] + bw, base[1]] } else { [valley_x, base[1] - h * rng.range(0.3, 0.5)] };
        bend(&mut pts, tip, valley, -0.12, 8);
        prev = valley;
    }
    pts
}

/// Contorno de uma nuvem de fumaça: círculo com `lobes` gomos arredondados.
fn puff(center: [f32; 2], r: f32, lobes: usize, rot: f32) -> Vec<[f32; 2]> {
    let mut pts = Vec::new();
    let n = lobes * 12;
    for k in 0..n {
        let a = k as f32 / n as f32 * TAU;
        let lobe = ((a + rot) * lobes as f32 / 2.0).sin().abs().powf(0.6);
        let rr = r * (0.82 + 0.22 * lobe);
        pts.push([center[0] + rr * a.cos(), center[1] + rr * a.sin()]);
    }
    pts
}

/// Caminho em zigue-zague (deslocamento do ponto médio) de `a` a `b`.
fn zigzag(a: [f32; 2], b: [f32; 2], depth: u32, rough: f32, rng: &mut Rng) -> Vec<[f32; 2]> {
    let mut pts = vec![a, b];
    let mut amp = rough * ((b[0] - a[0]).hypot(b[1] - a[1]));
    for _ in 0..depth {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let l = dx.hypot(dy).max(1e-3);
            let off = rng.range(-1.0, 1.0) * amp;
            next.push(p);
            next.push([(p[0] + q[0]) / 2.0 - dy / l * off, (p[1] + q[1]) / 2.0 + dx / l * off]);
        }
        next.push(*pts.last().expect("dois pontos"));
        pts = next;
        amp *= 0.5;
    }
    pts
}

/// Polígono de espessura `w` em volta do caminho, afinando nas duas pontas.
fn ribbon(path: &[[f32; 2]], w: f32) -> Vec<[f32; 2]> {
    let n = path.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let (p, q) = (path[i.saturating_sub(1)], path[(i + 1).min(n - 1)]);
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let l = dx.hypot(dy).max(1e-3);
        let t = i as f32 / (n - 1).max(1) as f32;
        let half = w * 0.5 * (t * (1.0 - t) * 4.0).sqrt().max(0.15);
        left.push([path[i][0] - dy / l * half, path[i][1] + dx / l * half]);
        right.push([path[i][0] + dy / l * half, path[i][1] - dx / l * half]);
    }
    right.reverse();
    left.extend(right);
    left
}

fn rgb(h: u32) -> [f32; 3] {
    [((h >> 16) & 255) as f32 / 255.0, ((h >> 8) & 255) as f32 / 255.0, (h & 255) as f32 / 255.0]
}

/// As formas (contorno, cor, opacidade), de baixo para cima.
fn shapes(kind: &AnimeFxKind, seed: u64, phase: f32) -> Vec<(Vec<[f32; 2]>, [f32; 3], f32)> {
    let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0x51_7CC1);
    let mut out = Vec::new();
    match kind {
        AnimeFxKind::Fire { base, height } => {
            let h = height.max(4.0);
            let n = 5;
            out.push((flame(*base, h, n, &mut rng, phase), rgb(0xE2_3B1A), 1.0));
            out.push((flame([base[0], base[1] - h * 0.02], h * 0.68, n - 1, &mut rng, phase + 0.31), rgb(0xFF_9A1F), 1.0));
            out.push((flame([base[0], base[1] - h * 0.04], h * 0.38, 3, &mut rng, phase + 0.57), rgb(0xFF_EC8A), 1.0));
        }
        AnimeFxKind::Smoke { center, radius, puffs, drift } => {
            let n = (*puffs).clamp(1, 12);
            // a fumaça sobe com a fase: cada nuvem anda uma fração do passo e cresce
            for i in 0..n {
                let f = i as f32 + phase.rem_euclid(1.0);
                let c = [center[0] + drift[0] * f, center[1] + drift[1] * f];
                let r = radius * (1.0 + 0.22 * f);
                let fade = (1.0 - f / (n as f32 + 0.5)).clamp(0.15, 1.0);
                let rot = rng.range(0.0, TAU);
                out.push((puff(c, r, 7, rot), rgb(0x8E_8E9C), fade));
                // luz em cima à esquerda: gomo menor e mais claro
                out.push((puff([c[0] - r * 0.14, c[1] - r * 0.16], r * 0.72, 6, rot + 0.5), rgb(0xD6_D6E0), fade));
            }
        }
        AnimeFxKind::Lightning { from, to, width } => {
            let mut r2 = Rng(rng.0 ^ ((phase * 1000.0) as u64).wrapping_mul(0x9E37));
            let main = zigzag(*from, *to, 6, 0.18, &mut r2);
            let mut paths = vec![(main.clone(), 1.0f32)];
            for _ in 0..2 {
                let k = (r2.range(0.25, 0.7) * main.len() as f32) as usize;
                let s = main[k.min(main.len() - 1)];
                let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
                let ang = dy.atan2(dx) + r2.range(-0.9, 0.9);
                let len = dx.hypot(dy) * r2.range(0.2, 0.35);
                let e = [s[0] + ang.cos() * len, s[1] + ang.sin() * len];
                paths.push((zigzag(s, e, 4, 0.2, &mut r2), 0.55));
            }
            for (p, k) in &paths {
                out.push((ribbon(p, width * 2.2 * k), rgb(0x5A_B8FF), 0.55));
            }
            for (p, k) in &paths {
                out.push((ribbon(p, width * 0.9 * k), rgb(0xC8_ECFF), 1.0));
            }
            for (p, k) in &paths {
                out.push((ribbon(p, width * 0.35 * k), rgb(0xFF_FFFF), 1.0));
            }
        }
    }
    out
}

pub fn anime_fx_command(p: &Project, blobs: &mut BlobStore, req: &AnimeFxRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, cut);
    let layer = match &node(p, &req.layer)?.data {
        NodeKind::Layer(l) => l,
        _ => return Err(ToolError::Invalid("não é camada".into())),
    };
    if layer.kind != LayerType::Raster {
        return Err(ToolError::Invalid("o efeito desenha numa camada raster".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid("camada sem cel no quadro".into()))?;
    let NodeKind::Cel(cel) = &node.data else { return Err(ToolError::Invalid("cel inválido".into())) };
    let mut img = cel_raster(blobs, cel, w, h)?;
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (contour, c, alpha) in shapes(&req.kind, req.seed, req.phase) {
        let Some(cov) = fill_polygons(&[contour], false, w, h) else { continue };
        for yy in 0..cov.height {
            for xx in 0..cov.width {
                let a = cov.cov[(yy * cov.width + xx) as usize] * alpha;
                if a <= 0.0 {
                    continue;
                }
                let (px, py) = (cov.x0 + xx as i32, cov.y0 + yy as i32);
                if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                    continue;
                }
                let i = img.idx(px as u32, py as u32);
                let d = img.data[i];
                // "normal" pré-multiplicado
                img.data[i] = [c[0] * a + d[0] * (1.0 - a), c[1] * a + d[1] * (1.0 - a), c[2] * a + d[2] * (1.0 - a), a + d[3] * (1.0 - a)];
                (x0, y0, x1, y1) = (x0.min(px), y0.min(py), x1.max(px + 1), y1.max(py + 1));
            }
        }
    }
    if x0 >= x1 {
        return Err(ToolError::Invalid("o efeito ficou fora do canvas".into()));
    }
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, [x0, y0, x1, y1]) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_change_with_phase_and_stay_in_place() {
        let fire = AnimeFxKind::Fire { base: [100.0, 200.0], height: 120.0 };
        let a = shapes(&fire, 1, 0.0);
        let b = shapes(&fire, 1, 0.25);
        assert_eq!(a.len(), 3, "três tons");
        assert_ne!(a[0].0, b[0].0, "a chama tremula com a fase");
        let top = a[0].0.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(top < 200.0 - 60.0 && top > 200.0 - 160.0, "altura perto da pedida: {top}");
        let bolt = AnimeFxKind::Lightning { from: [10.0, 0.0], to: [200.0, 300.0], width: 8.0 };
        let s = shapes(&bolt, 3, 0.0);
        assert_eq!(s.len(), 9, "3 caminhos × (brilho, corpo, miolo)");
        let smoke = AnimeFxKind::Smoke { center: [100.0, 100.0], radius: 30.0, puffs: 3, drift: up() };
        assert_eq!(shapes(&smoke, 0, 0.5).len(), 6);
    }
}
