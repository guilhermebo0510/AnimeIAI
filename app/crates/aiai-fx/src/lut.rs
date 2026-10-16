//! LUT 1D/3D no formato Adobe/IRIDAS `.cube`: leitura, escrita, aplicação (interpolação tetraédrica no 3D, linear no
//! 1D), geração por cozimento (bake) de uma cadeia de efeitos de cor pontuais.
//!
//! Formato: `TITLE "texto"`, `LUT_3D_SIZE N` (2..=65) ou `LUT_1D_SIZE N` (2..=65536), `DOMAIN_MIN r g b`,
//! `DOMAIN_MAX r g b`, comentários `#`, e N³ (ou N) linhas `r g b` com o vermelho variando mais rápido.
//! Entradas fora do domínio são limitadas (sem extrapolação HDR).

use crate::util::*;
use aiai_core::{BlobHash, BlobStore};
use aiai_paint::Image;
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LutKind {
    D1,
    D3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    pub title: String,
    pub kind: LutKind,
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// 3D: `size³` entradas com o vermelho mais rápido. 1D: `size` entradas.
    pub data: Vec<[f32; 3]>,
}

impl Lut {
    /// LUT 3D identidade de lado `size`.
    pub fn identity(size: usize) -> Lut {
        Lut::bake(size, "identity", |c| c)
    }

    /// Cozinha uma função de cor (0..1 → 0..1) numa LUT 3D de lado `size` (2..=65).
    pub fn bake<F: Fn([f32; 3]) -> [f32; 3] + Sync>(size: usize, title: &str, f: F) -> Lut {
        let n = size.max(2).min(65);
        let step = 1.0 / (n - 1) as f32;
        let data: Vec<[f32; 3]> = (0..n * n * n)
            .into_par_iter()
            .map(|i| {
                let (r, g, b) = (i % n, (i / n) % n, i / (n * n));
                f([r as f32 * step, g as f32 * step, b as f32 * step])
            })
            .collect();
        Lut { title: title.to_string(), kind: LutKind::D3, size: n, domain_min: [0.0; 3], domain_max: [1.0; 3], data }
    }

    /// Cópia com as entradas limitadas a 0..1 (portável para outros programas; perde um pouco de precisão nas
    /// bordas do gamut onde o Grade saturava, porque o limite cria quinas na tabela).
    pub fn clamped(&self) -> Lut {
        let mut l = self.clone();
        for v in &mut l.data {
            for c in v.iter_mut() {
                *c = c.max(0.0).min(1.0);
            }
        }
        l
    }

    pub fn parse(text: &str) -> Result<Lut, String> {
        let text = text.trim_start_matches('\u{feff}');
        let mut title = String::new();
        let mut size3: Option<usize> = None;
        let mut size1: Option<usize> = None;
        let mut dmin = [0.0f32; 3];
        let mut dmax = [1.0f32; 3];
        let mut data: Vec<[f32; 3]> = Vec::new();
        for (ln, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let err = |m: &str| format!("cube linha {}: {m}", ln + 1);
            let first = line.split_whitespace().next().unwrap_or("");
            match first {
                "TITLE" => {
                    let rest = line[5..].trim();
                    title = rest.trim_matches('"').to_string();
                }
                "LUT_3D_SIZE" => {
                    let n: usize = line[11..].trim().parse().map_err(|_| err("LUT_3D_SIZE inválido"))?;
                    if !(2..=65).contains(&n) {
                        return Err(err("LUT_3D_SIZE fora de 2..=65"));
                    }
                    size3 = Some(n);
                }
                "LUT_1D_SIZE" => {
                    let n: usize = line[11..].trim().parse().map_err(|_| err("LUT_1D_SIZE inválido"))?;
                    if !(2..=65536).contains(&n) {
                        return Err(err("LUT_1D_SIZE fora de 2..=65536"));
                    }
                    size1 = Some(n);
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let v = parse3(&line[10..]).ok_or_else(|| err("esperava 3 números"))?;
                    if first == "DOMAIN_MIN" {
                        dmin = v;
                    } else {
                        dmax = v;
                    }
                }
                "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => {}
                _ => {
                    if first.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.') {
                        data.push(parse3(line).ok_or_else(|| err("esperava 3 números"))?);
                    } else {
                        return Err(err(&format!("palavra-chave desconhecida '{first}'")));
                    }
                }
            }
        }
        let (kind, size) = match (size3, size1) {
            (Some(_), Some(_)) => return Err("cube: LUT_1D_SIZE e LUT_3D_SIZE juntos não são suportados".into()),
            (Some(n), None) => (LutKind::D3, n),
            (None, Some(n)) => (LutKind::D1, n),
            (None, None) => return Err("cube: falta LUT_3D_SIZE/LUT_1D_SIZE".into()),
        };
        let expect = if kind == LutKind::D3 { size * size * size } else { size };
        if data.len() != expect {
            return Err(format!("cube: esperava {expect} entradas, achei {}", data.len()));
        }
        for k in 0..3 {
            if !(dmax[k] > dmin[k]) {
                return Err("cube: DOMAIN_MAX deve ser maior que DOMAIN_MIN".into());
            }
        }
        Ok(Lut { title, kind, size, domain_min: dmin, domain_max: dmax, data })
    }

    /// Texto `.cube` (6 casas decimais).
    pub fn to_cube(&self) -> String {
        let mut s = String::with_capacity(self.data.len() * 24 + 128);
        if !self.title.is_empty() {
            s.push_str(&format!("TITLE \"{}\"\n", self.title.replace('"', "'")));
        }
        match self.kind {
            LutKind::D3 => s.push_str(&format!("LUT_3D_SIZE {}\n", self.size)),
            LutKind::D1 => s.push_str(&format!("LUT_1D_SIZE {}\n", self.size)),
        }
        s.push_str(&format!("DOMAIN_MIN {} {} {}\n", self.domain_min[0], self.domain_min[1], self.domain_min[2]));
        s.push_str(&format!("DOMAIN_MAX {} {} {}\n", self.domain_max[0], self.domain_max[1], self.domain_max[2]));
        for v in &self.data {
            s.push_str(&format!("{:.6} {:.6} {:.6}\n", v[0], v[1], v[2]));
        }
        s
    }

    #[inline]
    fn norm(&self, c: [f32; 3]) -> [f32; 3] {
        let mut o = [0.0; 3];
        for k in 0..3 {
            o[k] = ((c[k] - self.domain_min[k]) / (self.domain_max[k] - self.domain_min[k])).max(0.0).min(1.0);
        }
        o
    }

    /// Cor reta → cor reta pela LUT.
    pub fn sample(&self, c: [f32; 3]) -> [f32; 3] {
        let u = self.norm(c);
        match self.kind {
            LutKind::D1 => {
                let n = self.size;
                let mut o = [0.0; 3];
                for k in 0..3 {
                    let t = u[k] * (n - 1) as f32;
                    let i0 = (t.floor() as usize).min(n - 2);
                    let f = t - i0 as f32;
                    o[k] = lerp(self.data[i0][k], self.data[i0 + 1][k], f);
                }
                o
            }
            LutKind::D3 => self.tetra(u),
        }
    }

    /// Interpolação tetraédrica em coordenadas normalizadas 0..1.
    fn tetra(&self, u: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let m = (n - 1) as f32;
        let mut i0 = [0usize; 3];
        let mut f = [0.0f32; 3];
        for k in 0..3 {
            let t = u[k] * m;
            i0[k] = (t.floor() as usize).min(n - 2);
            f[k] = t - i0[k] as f32;
        }
        let idx = |r: usize, g: usize, b: usize| r + g * n + b * n * n;
        let (r, g, b) = (i0[0], i0[1], i0[2]);
        let v = |dr: usize, dg: usize, db: usize| self.data[idx(r + dr, g + dg, b + db)];
        let (fr, fg, fb) = (f[0], f[1], f[2]);
        let c000 = v(0, 0, 0);
        let c111 = v(1, 1, 1);
        // cadeia de vértices do tetraedro escolhido pela ordem de fr, fg, fb
        let (p1, w1, p2, w2, p3, w3) = if fr >= fg {
            if fg >= fb {
                (v(1, 0, 0), fr, v(1, 1, 0), fg, c111, fb)
            } else if fr >= fb {
                (v(1, 0, 0), fr, v(1, 0, 1), fb, c111, fg)
            } else {
                (v(0, 0, 1), fb, v(1, 0, 1), fr, c111, fg)
            }
        } else if fb >= fg {
            (v(0, 0, 1), fb, v(0, 1, 1), fg, c111, fr)
        } else if fb >= fr {
            (v(0, 1, 0), fg, v(0, 1, 1), fb, c111, fr)
        } else {
            (v(0, 1, 0), fg, v(1, 1, 0), fr, c111, fb)
        };
        let mut o = [0.0f32; 3];
        for k in 0..3 {
            o[k] = c000[k] + w1 * (p1[k] - c000[k]) + w2 * (p2[k] - p1[k]) + w3 * (p3[k] - p2[k]);
        }
        o
    }

    /// Aplica na imagem (cor reta; re-pré-multiplica) com mistura `intensity` 0..1.
    pub fn apply_image(&self, img: &mut Image, intensity: f32) {
        let k = intensity.max(0.0).min(1.0);
        if k <= 0.0 {
            return;
        }
        img.data.par_iter_mut().for_each(|p| {
            if p[3] <= 0.0 {
                return;
            }
            let un = unpremul(*p);
            let o = self.sample(un);
            for c in 0..3 {
                p[c] = lerp(un[c], o[c].max(0.0).min(1.0), k) * p[3];
            }
            fix(p);
        });
    }
}

fn parse3(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace();
    let a: f32 = it.next()?.parse().ok()?;
    let b: f32 = it.next()?.parse().ok()?;
    let c: f32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(a.is_finite() && b.is_finite() && c.is_finite()) {
        return None;
    }
    Some([a, b, c])
}

type Cache = Mutex<Vec<(BlobHash, Arc<Lut>)>>;
static CACHE: Cache = Mutex::new(Vec::new());

/// LUT de um blob `.cube` (com cache pequeno por hash, 8 entradas).
pub fn load(blobs: &BlobStore, hash: &BlobHash) -> Result<Arc<Lut>, String> {
    if let Ok(c) = CACHE.lock() {
        if let Some((_, l)) = c.iter().find(|(h, _)| h == hash) {
            return Ok(l.clone());
        }
    }
    let bytes = blobs.get(hash).ok_or_else(|| format!("lut: blob {hash} não encontrado"))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "lut: o blob não é texto UTF-8 (.cube)".to_string())?;
    let lut = Arc::new(Lut::parse(text)?);
    if let Ok(mut c) = CACHE.lock() {
        if c.len() >= 8 {
            c.remove(0);
        }
        c.push((hash.clone(), lut.clone()));
    }
    Ok(lut)
}
