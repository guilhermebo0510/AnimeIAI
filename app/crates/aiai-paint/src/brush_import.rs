//! Importa pincéis de outros programas para pontas e texturas do motor (só as imagens e o pouco de ajuste que dá para
//! ler com segurança; a dinâmica de cada programa é dele). Formatos lidos a partir da descrição pública de cada um,
//! sem código de terceiros:
//!
//! - GIMP `.gbr` (ponta em escala de cinza ou RGBA) e `.gih` (pipe: a primeira ponta de cada célula).
//! - Photoshop `.abr` v1/v2 (pontas "sampled") e v6+ (seção `8BIMsamp`): as imagens das pontas.
//! - Procreate `.brush` e `.brushset` (zip com `Shape.png`, `Grain.png` e `Brush.archive`, um plist binário de onde
//!   vêm o nome e o espaçamento).
//! - Krita `.bundle` (zip): as pontas de `brushes/` e as predefinições `.kpp` (PNG com o XML no texto `preset`).
//! - Imagem `.png`/`.jpg`/`.webp`: vira uma ponta.
//!
//! Clip Studio `.sut` é um banco SQLite com esquema próprio: não é lido aqui.
//!
//! A licença de cada pincel é a do autor do pacote: o importador só lê o arquivo que o usuário escolheu e guarda de
//! onde veio (`source`).

use crate::mask::Mask;
use std::collections::BTreeMap;

/// Ponta importada: imagem (máscara, 255 = tinta) ou redonda paramétrica (Krita "auto_brush").
#[derive(Debug, Clone, PartialEq)]
pub enum ImportedTip {
    Mask(Mask),
    Round { hardness: f32, roundness: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedBrush {
    pub name: String,
    pub tip: ImportedTip,
    /// Textura (grão/papel) em escala de cinza, repetida pelo canvas.
    pub texture: Option<Mask>,
    /// Espaçamento em fração do diâmetro, quando o arquivo diz.
    pub spacing: Option<f32>,
    /// Diâmetro sugerido (px), quando o arquivo diz.
    pub size: Option<f32>,
    /// Arquivo de onde veio (e a pasta dentro dele).
    pub source: String,
}

/// Maior lado guardado de uma ponta importada (pontas maiores são reduzidas).
const MAX_TIP: u32 = 512;
/// Limites contra arquivo malformado (zip bomba, tamanhos absurdos).
const MAX_ENTRY: usize = 64 << 20;
const MAX_PIXELS: u64 = 8192 * 8192;

/// Lê os pincéis de um arquivo pelo nome (extensão) e conteúdo.
pub fn import_brushes(file_name: &str, bytes: &[u8]) -> Result<Vec<ImportedBrush>, String> {
    let lower = file_name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    let stem = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = stem.rsplit_once('.').map(|(a, _)| a).unwrap_or(stem).to_owned();
    let mut out = match ext {
        "gbr" => vec![gbr(bytes)?].into_iter().map(|(n, m)| brush(n, m, None)).collect(),
        "gih" => gih(bytes)?.into_iter().map(|(n, m)| brush(n, m, None)).collect(),
        "abr" => abr(bytes)?.into_iter().enumerate().map(|(i, (sp, m))| brush(format!("{stem} {}", i + 1), m, sp)).collect(),
        "brush" | "brushset" => procreate(bytes, &stem)?,
        "bundle" => krita_bundle(bytes)?,
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => vec![brush(stem.clone(), tip_from_image(bytes)?, None)],
        "sut" => return Err("pincéis do Clip Studio (.sut) são um banco SQLite com formato próprio: ainda não lidos".into()),
        _ => return Err(format!("formato de pincel não reconhecido: .{ext}")),
    };
    for b in &mut out {
        if b.name.trim().is_empty() {
            b.name = stem.clone();
        }
        b.source = if b.source.is_empty() { file_name.to_owned() } else { format!("{file_name} › {}", b.source) };
    }
    if out.is_empty() {
        return Err("nenhuma ponta de pincel encontrada no arquivo".into());
    }
    Ok(out)
}

fn brush(name: String, tip: Mask, spacing: Option<f32>) -> ImportedBrush {
    ImportedBrush { name, tip: ImportedTip::Mask(tip), texture: None, spacing, size: None, source: String::new() }
}

// ------------------------------------------------------------------------------------------------ máscaras

/// Máscara de ponta a partir de valores 0..255 (`w`×`h`): acerta a convenção (tinta = 255, detectada pela borda, que
/// numa ponta é vazia), recorta o vazio em volta e reduz para `MAX_TIP`.
pub fn normalize_tip(w: u32, h: u32, mut data: Vec<u8>) -> Result<Mask, String> {
    if w == 0 || h == 0 || data.len() != w as usize * h as usize {
        return Err("ponta sem pixels".into());
    }
    let mut frame = 0u64;
    let mut n = 0u64;
    for x in 0..w {
        frame += data[x as usize] as u64 + data[((h - 1) * w + x) as usize] as u64;
        n += 2;
    }
    for y in 0..h {
        frame += data[(y * w) as usize] as u64 + data[(y * w + w - 1) as usize] as u64;
        n += 2;
    }
    if frame / n.max(1) > 127 {
        data.iter_mut().for_each(|v| *v = 255 - *v);
    }
    // recorte do vazio (com 1 px de folga)
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for y in 0..h {
        for x in 0..w {
            if data[(y * w + x) as usize] > 3 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x0 >= x1 {
        return Err("ponta vazia".into());
    }
    let (x0, y0, x1, y1) = (x0.saturating_sub(1), y0.saturating_sub(1), (x1 + 1).min(w), (y1 + 1).min(h));
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut crop = Vec::with_capacity((cw * ch) as usize);
    for y in y0..y1 {
        crop.extend_from_slice(&data[(y * w + x0) as usize..(y * w + x1) as usize]);
    }
    Ok(downscale(cw, ch, crop, MAX_TIP))
}

/// Reduz por média de caixa até o maior lado caber em `max`.
fn downscale(w: u32, h: u32, data: Vec<u8>, max: u32) -> Mask {
    let side = w.max(h);
    if side <= max {
        return Mask { width: w, height: h, data };
    }
    let k = side as f32 / max as f32;
    let (nw, nh) = (((w as f32 / k).round() as u32).max(1), ((h as f32 / k).round() as u32).max(1));
    let mut out = vec![0u8; (nw * nh) as usize];
    for ny in 0..nh {
        for nx in 0..nw {
            let (sx0, sy0) = ((nx as f32 * k) as u32, (ny as f32 * k) as u32);
            let (sx1, sy1) = ((((nx + 1) as f32 * k) as u32).clamp(sx0 + 1, w), (((ny + 1) as f32 * k) as u32).clamp(sy0 + 1, h));
            let mut acc = 0u32;
            for y in sy0..sy1 {
                for x in sx0..sx1 {
                    acc += data[(y * w + x) as usize] as u32;
                }
            }
            out[(ny * nw + nx) as usize] = (acc / ((sx1 - sx0) * (sy1 - sy0)).max(1)) as u8;
        }
    }
    Mask { width: nw, height: nh, data: out }
}

/// Imagem comum → ponta: com transparência de verdade vale o alfa; sem, o escuro é tinta (desenho preto no branco).
pub fn tip_from_image(bytes: &[u8]) -> Result<Mask, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("imagem ilegível: {e}"))?.to_rgba8();
    let (w, h) = img.dimensions();
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err("imagem grande demais para ponta".into());
    }
    let alpha = img.pixels().any(|p| p.0[3] < 250);
    // com alfa: a forma é o alfa (a cor não importa); sem: o escuro pinta. `normalize_tip` acerta se vier invertido
    let data: Vec<u8> = img
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            if alpha { a } else { 255 - (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8 }
        })
        .collect();
    normalize_tip(w, h, data)
}

/// Imagem → textura (luminância, sem recorte nem inversão; repetida pelo canvas).
pub fn texture_from_image(bytes: &[u8]) -> Result<Mask, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("imagem ilegível: {e}"))?.to_luma8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
        return Err("textura sem pixels ou grande demais".into());
    }
    Ok(downscale(w, h, img.into_raw(), 1024))
}

// ------------------------------------------------------------------------------------------------ leitura binária

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Self {
        Rd { b, p: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let e = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or("arquivo cortado")?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn skip(&mut self, n: usize) -> Result<(), String> {
        self.take(n).map(|_| ())
    }
    fn left(&self) -> usize {
        self.b.len() - self.p
    }
}

// ------------------------------------------------------------------------------------------------ GIMP

/// `.gbr`: cabeçalho (tamanho, versão, largura, altura, bytes por pixel, na v2 "GIMP" e espaçamento), nome, pixels.
fn gbr(bytes: &[u8]) -> Result<(String, Mask), String> {
    let mut r = Rd::new(bytes);
    let header = r.u32()? as usize;
    let version = r.u32()?;
    let (w, h, depth) = (r.u32()?, r.u32()?, r.u32()?);
    if version == 2 {
        let magic = r.take(4)?;
        if magic != b"GIMP" {
            return Err("gbr sem a marca GIMP".into());
        }
        r.skip(4)?; // espaçamento (em %)
    }
    if header < r.p || header > bytes.len() || w as u64 * h as u64 > MAX_PIXELS || w == 0 || h == 0 {
        return Err("cabeçalho gbr inválido".into());
    }
    let name = String::from_utf8_lossy(&bytes[r.p..header]).trim_end_matches('\0').to_owned();
    r.p = header;
    let n = w as usize * h as usize;
    let px = r.take(n * depth as usize)?;
    let data: Vec<u8> = match depth {
        1 => px.to_vec(),
        4 => px.chunks_exact(4).map(|c| c[3]).collect(),
        _ => return Err(format!("gbr com {depth} bytes por pixel")),
    };
    Ok((name, normalize_tip(w, h, data)?))
}

/// `.gih`: nome na 1ª linha, parâmetros na 2ª, depois as pontas `.gbr` uma atrás da outra (fica a primeira de cada).
fn gih(bytes: &[u8]) -> Result<Vec<(String, Mask)>, String> {
    let nl1 = bytes.iter().position(|&b| b == b'\n').ok_or("gih sem cabeçalho")?;
    let nl2 = nl1 + 1 + bytes[nl1 + 1..].iter().position(|&b| b == b'\n').ok_or("gih sem parâmetros")?;
    let name = String::from_utf8_lossy(&bytes[..nl1]).trim().to_owned();
    let (_, m) = gbr(&bytes[nl2 + 1..])?;
    Ok(vec![(name, m)])
}

// ------------------------------------------------------------------------------------------------ Photoshop

/// `.abr`: (espaçamento em fração, ponta) de cada ponta de imagem.
fn abr(bytes: &[u8]) -> Result<Vec<(Option<f32>, Mask)>, String> {
    let mut r = Rd::new(bytes);
    let version = r.u16()?;
    match version {
        1 | 2 => abr_v12(&mut r, version),
        6 | 7 | 10 => abr_v6(&mut r),
        v => Err(format!("versão de .abr desconhecida: {v}")),
    }
}

fn abr_v12(r: &mut Rd, version: u16) -> Result<Vec<(Option<f32>, Mask)>, String> {
    let count = r.u16()?;
    let mut out = Vec::new();
    for _ in 0..count {
        let kind = r.u16()?;
        let size = r.u32()? as usize;
        let start = r.p;
        if kind == 2 {
            r.skip(4)?; // misc
            let spacing = r.u16()?;
            if version == 2 {
                let n = r.u32()? as usize;
                r.skip(n * 2)?; // nome UTF-16
            }
            r.skip(1)?; // antisserrilhado
            r.skip(8)?; // limites curtos
            let (top, left, bottom, right) = (r.i32()?, r.i32()?, r.i32()?, r.i32()?);
            let depth = r.u16()?;
            let comp = r.u8()?;
            if let Ok(m) = abr_image(r, top, left, bottom, right, depth, comp) {
                out.push(((spacing > 0).then_some(spacing as f32 / 100.0), m));
            }
        }
        r.p = start + size;
        if r.p > r.b.len() {
            break;
        }
    }
    Ok(out)
}

fn abr_v6(r: &mut Rd) -> Result<Vec<(Option<f32>, Mask)>, String> {
    let sub = r.u16()?;
    let mut out = Vec::new();
    // seções "8BIM" + chave de 4 letras + tamanho; as pontas ficam em "samp"
    while r.left() >= 12 {
        let sig = r.take(4)?;
        if sig != b"8BIM" {
            break;
        }
        let key = r.take(4)?.to_vec();
        let len = r.u32()? as usize;
        let end = r.p.checked_add(len).filter(|&e| e <= r.b.len()).ok_or("seção do .abr cortada")?;
        if key == b"samp" {
            while r.p + 4 <= end {
                let blen = r.u32()? as usize;
                let bstart = r.p;
                let bend = (bstart + blen + 3) & !3; // cada ponta é alinhada em 4 bytes
                // identificador (pascal) e campos fixos antes dos limites: 47 bytes na subversão 1, 301 na 2
                if r.skip(if sub == 1 { 47 } else { 301 }).is_ok() {
                    let lim = (r.i32(), r.i32(), r.i32(), r.i32());
                    if let (Ok(top), Ok(left), Ok(bottom), Ok(right)) = lim {
                        if let (Ok(depth), Ok(comp)) = (r.u16(), r.u8()) {
                            if let Ok(m) = abr_image(r, top, left, bottom, right, depth, comp) {
                                out.push((None, m));
                            }
                        }
                    }
                }
                r.p = bend.min(end);
                if blen == 0 {
                    break;
                }
            }
        }
        r.p = end;
        if r.p % 2 == 1 && r.p < r.b.len() {
            r.p += 1;
        }
    }
    Ok(out)
}

/// Pixels da ponta do .abr: 8 ou 16 bits, crus ou em RLE (PackBits, com o tamanho de cada linha antes).
fn abr_image(r: &mut Rd, top: i32, left: i32, bottom: i32, right: i32, depth: u16, comp: u8) -> Result<Mask, String> {
    let (w, h) = ((right - left).max(0) as u32, (bottom - top).max(0) as u32);
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
        return Err("ponta sem tamanho".into());
    }
    let bpp = if depth == 16 { 2 } else { 1 };
    let row = w as usize * bpp;
    let mut raw = Vec::with_capacity(row * h as usize);
    if comp == 0 {
        raw.extend_from_slice(r.take(row * h as usize)?);
    } else {
        let mut lens = Vec::with_capacity(h as usize);
        for _ in 0..h {
            lens.push(r.u16()? as usize);
        }
        for l in lens {
            let src = r.take(l)?;
            let mut line = packbits(src, row);
            line.resize(row, 0);
            raw.extend_from_slice(&line);
        }
    }
    let data: Vec<u8> = if bpp == 2 { raw.chunks_exact(2).map(|c| c[0]).collect() } else { raw };
    normalize_tip(w, h, data)
}

fn packbits(src: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(max);
    let mut i = 0;
    while i < src.len() && out.len() < max {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let c = n as usize + 1;
            let e = (i + c).min(src.len());
            out.extend_from_slice(&src[i..e]);
            i = e;
        } else if n != -128 {
            let c = (1 - n as i32) as usize;
            if let Some(&v) = src.get(i) {
                out.extend(std::iter::repeat_n(v, c));
            }
            i += 1;
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ zip

/// Arquivos de um zip (só "guardado" e "deflate"), pelo diretório central.
pub fn read_zip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let le16 = |p: usize| -> Result<usize, String> { bytes.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize).ok_or_else(|| "zip cortado".to_owned()) };
    let le32 = |p: usize| -> Result<usize, String> { bytes.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize).ok_or_else(|| "zip cortado".to_owned()) };
    let lo = bytes.len().saturating_sub(65_557);
    let eocd = (lo..bytes.len().saturating_sub(21)).rev().find(|&p| bytes[p..p + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or("não é um zip")?;
    let count = le16(eocd + 10)?;
    let mut p = le32(eocd + 16)?;
    let mut out = BTreeMap::new();
    let mut total = 0usize;
    for _ in 0..count {
        if le32(p)? != 0x0201_4b50 {
            return Err("diretório do zip inválido".into());
        }
        let method = le16(p + 10)?;
        let csize = le32(p + 20)?;
        let usize_ = le32(p + 24)?;
        let (nl, el, cl) = (le16(p + 28)?, le16(p + 30)?, le16(p + 32)?);
        let local = le32(p + 42)?;
        let name = String::from_utf8_lossy(bytes.get(p + 46..p + 46 + nl).ok_or("zip cortado")?).replace('\\', "/");
        p += 46 + nl + el + cl;
        if name.ends_with('/') || usize_ > MAX_ENTRY {
            continue;
        }
        total += usize_;
        if total > 8 * MAX_ENTRY {
            return Err("zip grande demais".into());
        }
        if le32(local)? != 0x0403_4b50 {
            continue;
        }
        let start = local + 30 + le16(local + 26)? + le16(local + 28)?;
        let data = bytes.get(start..start + csize).ok_or("zip cortado")?;
        let file = match method {
            0 => data.to_vec(),
            8 => {
                use std::io::Read;
                let mut v = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(data).take(MAX_ENTRY as u64).read_to_end(&mut v).map_err(|e| format!("{name}: {e}"))?;
                v
            }
            m => return Err(format!("{name}: compressão {m} do zip não suportada")),
        };
        out.insert(name, file);
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ Procreate

fn procreate(bytes: &[u8], stem: &str) -> Result<Vec<ImportedBrush>, String> {
    let files = read_zip(bytes)?;
    // cada pincel é uma pasta (no .brushset) ou a raiz (no .brush) com Shape.png
    let mut dirs: Vec<String> = files.keys().filter_map(|k| k.strip_suffix("Shape.png").map(str::to_owned)).collect();
    dirs.sort();
    let mut out = Vec::new();
    for d in dirs {
        let Some(shape) = files.get(&format!("{d}Shape.png")) else { continue };
        let Ok(tip) = tip_from_image(shape) else { continue };
        let texture = files.get(&format!("{d}Grain.png")).and_then(|g| texture_from_image(g).ok());
        let info = files.get(&format!("{d}Brush.archive")).and_then(|a| bplist::parse(a).ok());
        let name = info.as_ref().and_then(|v| bplist::keyed_string(v, "name")).unwrap_or_else(|| if d.is_empty() { stem.to_owned() } else { d.trim_end_matches('/').to_owned() });
        let spacing = info.as_ref().and_then(|v| bplist::keyed_real(v, "plotSpacing")).filter(|s| *s > 0.0).map(|s| (s as f32).clamp(0.01, 5.0));
        out.push(ImportedBrush { name, tip: ImportedTip::Mask(tip), texture, spacing, size: None, source: d.trim_end_matches('/').to_owned() });
    }
    Ok(out)
}

/// Plist binário ("bplist00") mínimo: o suficiente para achar o nome no arquivo NSKeyedArchiver do Procreate.
mod bplist {
    #[derive(Debug, Clone)]
    pub enum V {
        Int(i64),
        Real(f64),
        Str(String),
        Uid(u64),
        Arr(Vec<V>),
        Dict(Vec<(V, V)>),
        Other,
    }

    pub fn parse(b: &[u8]) -> Result<V, String> {
        if b.len() < 40 || &b[..8] != b"bplist00" {
            return Err("não é plist binário".into());
        }
        let t = &b[b.len() - 32..];
        let (osz, rsz) = (t[6] as usize, t[7] as usize);
        let be = |s: &[u8]| s.iter().fold(0u64, |a, &x| (a << 8) | x as u64);
        let n = be(&t[8..16]) as usize;
        let top = be(&t[16..24]) as usize;
        let table = be(&t[24..32]) as usize;
        if osz == 0 || rsz == 0 || n > 1_000_000 || table + n * osz > b.len() {
            return Err("plist malformado".into());
        }
        let offs: Vec<usize> = (0..n).map(|i| be(&b[table + i * osz..table + (i + 1) * osz]) as usize).collect();
        obj(b, &offs, rsz, top, 0)
    }

    fn obj(b: &[u8], offs: &[usize], rsz: usize, i: usize, depth: u32) -> Result<V, String> {
        let be = |s: &[u8]| s.iter().fold(0u64, |a, &x| (a << 8) | x as u64);
        if depth > 64 {
            return Err("plist fundo demais".into());
        }
        let p = *offs.get(i).ok_or("referência fora do plist")?;
        let m = *b.get(p).ok_or("plist cortado")?;
        let (hi, lo) = (m >> 4, (m & 0xf) as usize);
        let get = |s: usize, l: usize| b.get(s..s + l).ok_or_else(|| "plist cortado".to_owned());
        // contagem estendida: o próximo objeto é um inteiro
        let count = |p: usize| -> Result<(usize, usize), String> {
            if lo != 0xf {
                return Ok((lo, p + 1));
            }
            let im = *b.get(p + 1).ok_or("plist cortado")?;
            let l = 1usize << (im & 0xf);
            Ok((be(get(p + 2, l)?) as usize, p + 2 + l))
        };
        Ok(match hi {
            0x1 => V::Int(be(get(p + 1, 1 << lo)?) as i64),
            0x2 => {
                let s = get(p + 1, 1 << lo)?;
                V::Real(if s.len() == 8 { f64::from_be_bytes(s.try_into().unwrap_or([0; 8])) } else if s.len() == 4 { f32::from_be_bytes(s.try_into().unwrap_or([0; 4])) as f64 } else { 0.0 })
            }
            0x5 => {
                let (c, s) = count(p)?;
                V::Str(String::from_utf8_lossy(get(s, c)?).into_owned())
            }
            0x6 => {
                let (c, s) = count(p)?;
                let u: Vec<u16> = get(s, c * 2)?.chunks_exact(2).map(|x| u16::from_be_bytes([x[0], x[1]])).collect();
                V::Str(String::from_utf16_lossy(&u))
            }
            0x8 => V::Uid(be(get(p + 1, lo + 1)?)),
            0xa => {
                let (c, s) = count(p)?;
                let mut v = Vec::with_capacity(c.min(4096));
                for k in 0..c {
                    v.push(obj(b, offs, rsz, be(get(s + k * rsz, rsz)?) as usize, depth + 1)?);
                }
                V::Arr(v)
            }
            0xd => {
                let (c, s) = count(p)?;
                let mut v = Vec::with_capacity(c.min(4096));
                for k in 0..c {
                    let key = obj(b, offs, rsz, be(get(s + k * rsz, rsz)?) as usize, depth + 1)?;
                    let val = obj(b, offs, rsz, be(get(s + (c + k) * rsz, rsz)?) as usize, depth + 1)?;
                    v.push((key, val));
                }
                V::Dict(v)
            }
            _ => V::Other,
        })
    }

    fn dict_get<'a>(d: &'a V, key: &str) -> Option<&'a V> {
        match d {
            V::Dict(v) => v.iter().find(|(k, _)| matches!(k, V::Str(s) if s == key)).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Valor de `key` no objeto raiz do NSKeyedArchiver (seguindo as referências UID para `$objects`).
    fn keyed<'a>(v: &'a V, key: &str) -> Option<&'a V> {
        let objs = match dict_get(v, "$objects")? {
            V::Arr(a) => a,
            _ => return None,
        };
        let resolve = |x: &'a V| -> &'a V {
            match x {
                V::Uid(u) => objs.get(*u as usize).unwrap_or(x),
                _ => x,
            }
        };
        let root = resolve(dict_get(dict_get(v, "$top")?, "root")?);
        Some(resolve(dict_get(root, key)?))
    }

    pub fn keyed_string(v: &V, key: &str) -> Option<String> {
        match keyed(v, key)? {
            V::Str(s) if !s.trim().is_empty() && s != "$null" => Some(s.clone()),
            _ => None,
        }
    }

    pub fn keyed_real(v: &V, key: &str) -> Option<f64> {
        match keyed(v, key)? {
            V::Real(r) => Some(*r),
            V::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------------------------------------ Krita

fn krita_bundle(bytes: &[u8]) -> Result<Vec<ImportedBrush>, String> {
    let files = read_zip(bytes)?;
    let tip_of = |path: &str| -> Option<Mask> {
        let data = files.get(path)?;
        let l = path.to_lowercase();
        if l.ends_with(".gbr") {
            gbr(data).ok().map(|x| x.1)
        } else if l.ends_with(".gih") {
            gih(data).ok().and_then(|v| v.into_iter().next()).map(|x| x.1)
        } else if l.ends_with(".png") || l.ends_with(".jpg") {
            tip_from_image(data).ok()
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    // predefinições: nome, ponta (imagem do pacote ou redonda) e espaçamento
    for (path, data) in files.iter().filter(|(k, _)| k.to_lowercase().ends_with(".kpp")) {
        let Some(xml) = png_text(data, "preset") else { continue };
        let Ok(doc) = roxmltree::Document::parse(&xml) else { continue };
        let root = doc.root_element();
        let name = root.attribute("name").map(str::to_owned).unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path).trim_end_matches(".kpp").to_owned());
        let def = root.children().find(|n| n.attribute("name") == Some("brush_definition")).and_then(|n| n.text()).map(str::to_owned);
        let Some(def) = def else { continue };
        let Ok(bd) = roxmltree::Document::parse(&def) else { continue };
        let b = bd.root_element();
        let spacing = b.attribute("spacing").and_then(|s| s.parse::<f32>().ok()).filter(|s| *s > 0.0).map(|s| s.clamp(0.01, 5.0));
        let tip = match b.attribute("type") {
            Some("auto_brush") => {
                let mg = b.children().find(|n| n.has_tag_name("MaskGenerator"));
                let fade = mg.and_then(|m| m.attribute("hfade")).and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
                let ratio = mg.and_then(|m| m.attribute("ratio")).and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
                Some(ImportedTip::Round { hardness: fade.clamp(0.0, 1.0), roundness: ratio.clamp(0.05, 1.0) })
            }
            _ => b.attribute("filename").and_then(|f| {
                let p = files.keys().find(|k| k.rsplit('/').next() == Some(f)).cloned()?;
                used.insert(p.clone());
                tip_of(&p).map(ImportedTip::Mask)
            }),
        };
        let size = root
            .children()
            .find(|n| n.attribute("name") == Some("brush_definition"))
            .and(b.children().find(|n| n.has_tag_name("MaskGenerator")).and_then(|m| m.attribute("diameter")).and_then(|s| s.parse::<f32>().ok()));
        if let Some(tip) = tip {
            out.push(ImportedBrush { name, tip, texture: None, spacing, size, source: path.clone() });
        }
    }
    // pontas soltas do pacote que nenhuma predefinição usou
    for path in files.keys().filter(|k| k.starts_with("brushes/") && !used.contains(*k)) {
        if let Some(m) = tip_of(path) {
            let name = path.rsplit('/').next().unwrap_or(path);
            let name = name.rsplit_once('.').map(|(a, _)| a).unwrap_or(name).to_owned();
            out.push(ImportedBrush { name, tip: ImportedTip::Mask(m), texture: None, spacing: None, size: None, source: path.clone() });
        }
    }
    Ok(out)
}

/// Texto `key` de um PNG (tEXt, zTXt ou iTXt).
fn png_text(png: &[u8], key: &str) -> Option<String> {
    let dec = png::Decoder::new(std::io::Cursor::new(png));
    let reader = dec.read_info().ok()?;
    let info = reader.info();
    if let Some(t) = info.uncompressed_latin1_text.iter().find(|t| t.keyword == key) {
        return Some(t.text.clone());
    }
    if let Some(t) = info.compressed_latin1_text.iter().find(|t| t.keyword == key) {
        return t.get_text().ok();
    }
    info.utf8_text.iter().find(|t| t.keyword == key).and_then(|t| t.get_text().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ponta de teste: disco escuro no branco (convenção "preto pinta") ou claro no preto.
    fn disc(n: u32, dark_on_white: bool) -> Vec<u8> {
        let c = n as f32 / 2.0;
        (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f32 + 0.5 - c, (i / n) as f32 + 0.5 - c);
                let ink = (x * x + y * y).sqrt() < c * 0.6;
                if ink == dark_on_white { 0 } else { 255 }
            })
            .collect()
    }

    fn gbr_bytes(n: u32, name: &str, data: &[u8]) -> Vec<u8> {
        let mut h = Vec::new();
        let header = 28 + name.len() + 1;
        for v in [header as u32, 2, n, n, 1] {
            h.extend_from_slice(&v.to_be_bytes());
        }
        h.extend_from_slice(b"GIMP");
        h.extend_from_slice(&25u32.to_be_bytes());
        h.extend_from_slice(name.as_bytes());
        h.push(0);
        h.extend_from_slice(data);
        h
    }

    #[test]
    fn gbr_tip_is_read_in_either_convention_and_cropped() {
        for dark in [true, false] {
            let b = import_brushes("disco.gbr", &gbr_bytes(40, "Disco", &disc(40, dark))).unwrap();
            assert_eq!(b[0].name, "Disco");
            let ImportedTip::Mask(m) = &b[0].tip else { panic!() };
            assert!(m.width < 40 && m.width >= 24, "recortado: {}", m.width);
            assert_eq!(m.get(m.width / 2, m.height / 2), 1.0, "o centro é tinta");
            assert_eq!(m.get(0, 0), 0.0, "o canto é vazio");
        }
    }

    #[test]
    fn abr_v6_sampled_tip_raw_and_rle() {
        for comp in [0u8, 1] {
            let n = 32u32;
            let data = disc(n, false);
            let mut img = Vec::new();
            if comp == 0 {
                img.extend_from_slice(&data);
            } else {
                // PackBits "literal" de cada linha (cabeçalho 31 = 32 bytes)
                let rows: Vec<Vec<u8>> = data.chunks(n as usize).map(|r| [&[31u8][..], r].concat()).collect();
                for r in &rows {
                    img.extend_from_slice(&(r.len() as u16).to_be_bytes());
                }
                for r in &rows {
                    img.extend_from_slice(r);
                }
            }
            let mut brush = vec![0u8; 47];
            for v in [0i32, 0, n as i32, n as i32] {
                brush.extend_from_slice(&v.to_be_bytes());
            }
            brush.extend_from_slice(&8u16.to_be_bytes());
            brush.push(comp);
            brush.extend_from_slice(&img);
            while brush.len() % 4 != 0 {
                brush.push(0);
            }
            let mut samp = (brush.len() as u32).to_be_bytes().to_vec();
            samp.extend_from_slice(&brush);
            let mut f = vec![0, 6, 0, 1];
            f.extend_from_slice(b"8BIMsamp");
            f.extend_from_slice(&(samp.len() as u32).to_be_bytes());
            f.extend_from_slice(&samp);
            let b = import_brushes("pacote.abr", &f).unwrap();
            assert_eq!(b.len(), 1, "compressão {comp}");
            assert_eq!(b[0].name, "pacote 1");
            let ImportedTip::Mask(m) = &b[0].tip else { panic!() };
            assert_eq!(m.get(m.width / 2, m.height / 2), 1.0);
        }
    }

    /// Zip guardado (sem compressão) com os arquivos dados.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let off = out.len() as u32;
            let mut local = vec![0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            local.extend_from_slice(&(data.len() as u32).to_le_bytes());
            local.extend_from_slice(&(data.len() as u32).to_le_bytes());
            local.extend_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&local);
            out.extend_from_slice(data);
            let mut c = vec![0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 12]); // extra, comentário, disco, atributos
            c.extend_from_slice(&off.to_le_bytes());
            c.extend_from_slice(name.as_bytes());
            cd.extend_from_slice(&c);
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    fn png_of(n: u32, data: &[u8]) -> Vec<u8> {
        let img = image::GrayImage::from_raw(n, n, data.to_vec()).unwrap();
        let mut v = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut v), image::ImageFormat::Png).unwrap();
        v
    }

    #[test]
    fn procreate_brushset_reads_each_folder_with_grain() {
        let shape = png_of(32, &disc(32, false));
        let grain = png_of(16, &vec![128u8; 256]);
        let z = zip(&[("A1/Shape.png", &shape), ("A1/Grain.png", &grain), ("B2/Shape.png", &shape)]);
        let b = import_brushes("meus.brushset", &z).unwrap();
        assert_eq!(b.len(), 2);
        assert!(b[0].texture.is_some() && b[1].texture.is_none());
        assert_eq!(b[0].source, "meus.brushset › A1");
    }

    #[test]
    fn krita_bundle_loose_tips_and_preset_with_auto_brush() {
        let g = gbr_bytes(24, "Ponta", &disc(24, true));
        // .kpp = PNG com o XML no texto "preset"
        let mut kpp = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut kpp, 1, 1);
            enc.set_color(png::ColorType::Grayscale);
            enc.add_text_chunk("preset".into(), r#"<Preset name="Redondo K" paintopid="paintbrush"><param name="brush_definition" type="string"><![CDATA[<Brush type="auto_brush" spacing="0.08"><MaskGenerator diameter="30" ratio="1" hfade="0.4"/></Brush>]]></param></Preset>"#.into()).unwrap();
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[0]).unwrap();
        }
        let z = zip(&[("brushes/ponta.gbr", &g), ("paintoppresets/r.kpp", &kpp)]);
        let b = import_brushes("pacote.bundle", &z).unwrap();
        let names: Vec<&str> = b.iter().map(|x| x.name.as_str()).collect();
        assert!(names.contains(&"Redondo K") && names.contains(&"ponta"), "{names:?}");
        let k = b.iter().find(|x| x.name == "Redondo K").unwrap();
        assert_eq!(k.spacing, Some(0.08));
        assert_eq!(k.size, Some(30.0));
        assert!(matches!(k.tip, ImportedTip::Round { hardness, .. } if (hardness - 0.4).abs() < 1e-4));
    }

    #[test]
    fn plain_image_becomes_a_tip_and_sut_is_refused_with_a_reason() {
        let b = import_brushes("mancha.png", &png_of(20, &disc(20, true))).unwrap();
        assert_eq!(b[0].name, "mancha");
        assert!(import_brushes("x.sut", b"SQLite format 3\0").unwrap_err().contains("SQLite"));
    }
}
