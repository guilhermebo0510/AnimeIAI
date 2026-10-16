//! Scopes de cor: histograma (RGB + luma, 256 bins), forma de onda de luma (W×256), parade RGB e vetorscópio (Cb/Cr
//! 256×256, Rec.709). Cada função devolve os dados numéricos e uma `Image` opaca com a visualização.
//!
//! Medem a cor reta (não pré-multiplicada) quantizada em 8 bits; pixels com alfa 0 são ignorados. As contagens são
//! inteiras, então o resultado independe da ordem de execução (determinístico).

use aiai_paint::Image;
use rayon::prelude::*;

const BG: [f32; 4] = [0.035, 0.035, 0.045, 1.0];

#[inline]
fn q8(v: f32) -> usize {
    (v.max(0.0).min(1.0) * 255.0 + 0.5) as usize
}

/// Cor reta quantizada (r, g, b, luma Rec.709) de um pixel pré-multiplicado, ou `None` se transparente.
#[inline]
fn straight8(p: &[f32; 4]) -> Option<[usize; 4]> {
    if p[3] <= 0.0 {
        return None;
    }
    let (r, g, b) = (p[0] / p[3], p[1] / p[3], p[2] / p[3]);
    let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    Some([q8(r), q8(g), q8(b), q8(l)])
}

fn blank(w: u32, h: u32) -> Image {
    Image { width: w, height: h, data: vec![BG; w as usize * h as usize] }
}

fn put(img: &mut Image, x: usize, y: usize, c: [f32; 3], a: f32) {
    if x >= img.width as usize || y >= img.height as usize {
        return;
    }
    let p = &mut img.data[y * img.width as usize + x];
    for k in 0..3 {
        p[k] = p[k] * (1.0 - a) + c[k] * a;
    }
}

/// Histograma de 256 bins por canal e de luma.
#[derive(Debug, Clone)]
pub struct Histogram {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub luma: [u32; 256],
    /// Pixels contados (alfa > 0).
    pub pixels: u64,
    /// Visualização 256×160: R, G, B em aditivo e luma em cinza, escala logarítmica.
    pub image: Image,
}

pub fn histogram(img: &Image) -> Histogram {
    let w = (img.width as usize).max(1);
    let parts: Vec<[Vec<u32>; 4]> = img
        .data
        .par_chunks(w * 16)
        .map(|chunk| {
            let mut h = [vec![0u32; 256], vec![0u32; 256], vec![0u32; 256], vec![0u32; 256]];
            for p in chunk {
                if let Some(v) = straight8(p) {
                    for k in 0..4 {
                        h[k][v[k]] += 1;
                    }
                }
            }
            h
        })
        .collect();
    let mut out = Histogram { r: [0; 256], g: [0; 256], b: [0; 256], luma: [0; 256], pixels: 0, image: blank(256, 160) };
    for part in &parts {
        for i in 0..256 {
            out.r[i] += part[0][i];
            out.g[i] += part[1][i];
            out.b[i] += part[2][i];
            out.luma[i] += part[3][i];
        }
    }
    out.pixels = out.luma.iter().map(|&v| v as u64).sum();
    let maxv = out.r.iter().chain(&out.g).chain(&out.b).chain(&out.luma).copied().max().unwrap_or(0).max(1) as f32;
    let norm = (1.0 + maxv).ln();
    let mut vis = blank(256, 160);
    for x in 0..256usize {
        for (arr, col) in [(&out.luma, [0.55, 0.55, 0.55]), (&out.r, [1.0, 0.1, 0.1]), (&out.g, [0.1, 1.0, 0.1]), (&out.b, [0.15, 0.3, 1.0])] {
            let hgt = (((1.0 + arr[x] as f32).ln() / norm) * 158.0) as usize;
            for yy in 0..hgt {
                let y = 159 - yy;
                // aditivo: R, G, B somam; a luma fica atrás
                let p = &mut vis.data[y * 256 + x];
                for k in 0..3 {
                    p[k] = (p[k] + col[k] * 0.45).min(1.0);
                }
            }
        }
    }
    out.image = vis;
    out
}

/// Forma de onda de luma: `counts[x * 256 + nível]` pixels na coluna `x` com aquele nível (0..255).
#[derive(Debug, Clone)]
pub struct Waveform {
    pub width: u32,
    pub height_src: u32,
    pub counts: Vec<u32>,
    /// Imagem W×256 (nível 255 no topo), graticule a 0/25/50/75/100%.
    pub image: Image,
}

fn column_counts(img: &Image, channel: usize, x0: usize, x1: usize) -> Vec<u32> {
    // colunas x0..x1 da imagem → 256 contadores por coluna de saída (x1 - x0 colunas)
    let w = img.width as usize;
    let h = img.height as usize;
    let mut counts = vec![0u32; (x1 - x0) * 256];
    counts.par_chunks_mut(256).enumerate().for_each(|(i, col)| {
        let x = x0 + i;
        for y in 0..h {
            if let Some(v) = straight8(&img.data[y * w + x]) {
                col[v[channel]] += 1;
            }
        }
    });
    counts
}

fn graticule(vis: &mut Image) {
    let (w, h) = (vis.width as usize, vis.height as usize);
    if h < 256 {
        return;
    }
    for pct in [0usize, 25, 50, 75, 100] {
        let level = pct * 255 / 100;
        let y = 255 - level;
        for x in 0..w {
            put(vis, x, y, [0.5, 0.5, 0.55], 0.25);
        }
    }
}

fn render_columns(counts: &[u32], cols: usize, src_h: u32, vis: &mut Image, x_off: usize, tint: [f32; 3]) {
    let h = (src_h as f32).max(1.0);
    for x in 0..cols {
        for level in 0..256usize {
            let c = counts[x * 256 + level];
            if c == 0 {
                continue;
            }
            let v = ((c as f32 / h) * 24.0).min(1.0).sqrt().max(0.18);
            let y = 255 - level;
            let p = &mut vis.data[y * vis.width as usize + x_off + x];
            for k in 0..3 {
                p[k] = (p[k] + tint[k] * v).min(1.0);
            }
        }
    }
}

pub fn waveform(img: &Image) -> Waveform {
    let w = img.width as usize;
    if w == 0 || img.height == 0 {
        return Waveform { width: 0, height_src: 0, counts: Vec::new(), image: blank(1, 256) };
    }
    let counts = column_counts(img, 3, 0, w);
    let mut vis = blank(w as u32, 256);
    graticule(&mut vis);
    render_columns(&counts, w, img.height, &mut vis, 0, [0.85, 0.95, 0.85]);
    Waveform { width: w as u32, height_src: img.height, counts, image: vis }
}

/// Parade RGB: três formas de onda lado a lado; cada uma comprime as colunas da imagem em `sub_width` colunas.
#[derive(Debug, Clone)]
pub struct Parade {
    pub sub_width: u32,
    pub height_src: u32,
    /// `r[x * 256 + nível]` etc., `x < sub_width`.
    pub r: Vec<u32>,
    pub g: Vec<u32>,
    pub b: Vec<u32>,
    /// Imagem (3·sub_width)×256.
    pub image: Image,
}

pub fn rgb_parade(img: &Image) -> Parade {
    let w = img.width as usize;
    if w == 0 || img.height == 0 {
        return Parade { sub_width: 0, height_src: 0, r: vec![], g: vec![], b: vec![], image: blank(1, 256) };
    }
    let sub = w.div_ceil(3).max(1);
    // mapeia coluna de saída → intervalo de colunas da imagem; soma as contagens
    let reduce = |ch: usize| -> Vec<u32> {
        let full = column_counts(img, ch, 0, w);
        let mut out = vec![0u32; sub * 256];
        for x in 0..w {
            let ox = (x * sub / w).min(sub - 1);
            for l in 0..256 {
                out[ox * 256 + l] += full[x * 256 + l];
            }
        }
        out
    };
    let (r, g, b) = (reduce(0), reduce(1), reduce(2));
    let mut vis = blank((3 * sub) as u32, 256);
    graticule(&mut vis);
    // cada coluna de saída recebe ~w/sub colunas somadas: normaliza pela altura × colunas somadas
    let eff_h = img.height * (w as u32).div_ceil(sub as u32);
    render_columns(&r, sub, eff_h, &mut vis, 0, [1.0, 0.25, 0.25]);
    render_columns(&g, sub, eff_h, &mut vis, sub, [0.25, 1.0, 0.25]);
    render_columns(&b, sub, eff_h, &mut vis, 2 * sub, [0.3, 0.45, 1.0]);
    Parade { sub_width: sub as u32, height_src: img.height, r, g, b, image: vis }
}

/// Vetorscópio Rec.709: ponto em (x = Cb, y = Cr invertido) com Cb,Cr ∈ [−0,5; 0,5] mapeados em 0..255.
#[derive(Debug, Clone)]
pub struct Vectorscope {
    /// `counts[y * 256 + x]`.
    pub counts: Vec<u32>,
    pub image: Image,
}

/// Posição (x, y) no vetorscópio de uma cor reta (0..1).
pub fn vectorscope_pos(rgb: [f32; 3]) -> (usize, usize) {
    let y = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    let cb = ((rgb[2] - y) / 1.8556).max(-0.5).min(0.5);
    let cr = ((rgb[0] - y) / 1.5748).max(-0.5).min(0.5);
    (((cb + 0.5) * 255.0 + 0.5) as usize, ((0.5 - cr) * 255.0 + 0.5) as usize)
}

pub fn vectorscope(img: &Image) -> Vectorscope {
    let w = (img.width as usize).max(1);
    let parts: Vec<Vec<u32>> = img
        .data
        .par_chunks(w * 16)
        .map(|chunk| {
            let mut c = vec![0u32; 256 * 256];
            for p in chunk {
                if p[3] > 0.0 {
                    let (x, y) = vectorscope_pos([p[0] / p[3], p[1] / p[3], p[2] / p[3]]);
                    c[y.min(255) * 256 + x.min(255)] += 1;
                }
            }
            c
        })
        .collect();
    let mut counts = vec![0u32; 256 * 256];
    for part in &parts {
        for (a, b) in counts.iter_mut().zip(part) {
            *a += *b;
        }
    }
    let mut vis = blank(256, 256);
    // graticule: círculos em 50% e 100% e eixos; caixas nas cores de barras 75%
    for ang in 0..720 {
        let a = ang as f32 * std::f32::consts::TAU / 720.0;
        for r in [127.0f32, 63.5] {
            put(&mut vis, (127.5 + r * a.cos()) as usize, (127.5 + r * a.sin()) as usize, [0.5, 0.5, 0.55], 0.4);
        }
    }
    for i in 0..256 {
        put(&mut vis, i, 128, [0.5, 0.5, 0.55], 0.2);
        put(&mut vis, 128, i, [0.5, 0.5, 0.55], 0.2);
    }
    for c in [[0.75, 0.0, 0.0], [0.75, 0.75, 0.0], [0.0, 0.75, 0.0], [0.0, 0.75, 0.75], [0.0, 0.0, 0.75], [0.75, 0.0, 0.75]] {
        let (cx, cy) = vectorscope_pos(c);
        for d in -3i32..=3 {
            for (px, py) in [(cx as i32 + d, cy as i32 - 3), (cx as i32 + d, cy as i32 + 3), (cx as i32 - 3, cy as i32 + d), (cx as i32 + 3, cy as i32 + d)] {
                if px >= 0 && py >= 0 {
                    put(&mut vis, px as usize, py as usize, c.map(|v| v + 0.25), 0.8);
                }
            }
        }
    }
    let maxc = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let norm = (1.0 + maxc).ln();
    for y in 0..256usize {
        for x in 0..256usize {
            let c = counts[y * 256 + x];
            if c == 0 {
                continue;
            }
            let v = ((1.0 + c as f32).ln() / norm).max(0.25);
            // cor do ponto = matiz da posição (luma 0,6)
            let cb = x as f32 / 255.0 - 0.5;
            let cr = 0.5 - y as f32 / 255.0;
            let l = 0.6f32;
            let col = [(l + 1.5748 * cr).max(0.0).min(1.0), (l - 0.1873 * cb - 0.4681 * cr).max(0.0).min(1.0), (l + 1.8556 * cb).max(0.0).min(1.0)];
            put(&mut vis, x, y, col, v);
        }
    }
    Vectorscope { counts, image: vis }
}
