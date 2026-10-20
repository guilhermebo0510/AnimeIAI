//! Formato próprio de pincel do AnimeIAI: `.aiaibrush` (um pincel) e `.aiaibrushes` (pacote).
//!
//! Um zip (como `.bundle` do Krita, `.brushset` do Procreate e `.ora`) com o `mimetype` primeiro e sem compressão:
//!
//! ```text
//! .aiaibrush                          .aiaibrushes
//!   mimetype                            mimetype
//!   manifest.json                       manifest.json            (pacote: lista `brushes`, licença do pacote)
//!   brush.json  (BrushPreset)           thumbnail.png, LICENSE.txt
//!   assets/tip.png, texture.png         brushes/<id>/manifest.json + brush.json + thumbnail.png
//!   thumbnail.png (256×256, motor)      assets/<blake3>.png      (pontas e texturas, por conteúdo)
//!   LICENSE.txt (licença em texto livre)
//! ```
//!
//! O manifesto leva o que o pincel é e de onde veio: identificador estável (UUID v7), nome, categoria, etiquetas, autor,
//! **licença (obrigatória)**, fonte, versão, datas, hashes BLAKE3 de todo arquivo de imagem e um mapa `extra` que
//! sobrevive a regravar. Na leitura tudo é conferido: caminhos do zip (zip slip), limites de tamanho (zip bomba), CRC,
//! hashes, arquivos citados que faltam e arquivos presentes que ninguém cita, `min_reader` (versão mais nova que o
//! leitor = recusa com o motivo) e o próprio preset (`BrushPreset::validate`). As imagens viajam como PNG de 8 bits em
//! cinza (255 = tinta); dentro do app continuam as máscaras "AIM1" (o preset referencia o hash dela).

use crate::brush::{BrushAuthor, BrushPreset, BrushTip};
use crate::mask::Mask;
use aiai_core::BlobHash;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{Read, Write};

pub const FORMAT_BRUSH: &str = "animeiai-brush";
pub const FORMAT_PACK: &str = "animeiai-brushes";
/// Versão do formato que este app grava e `min_reader` que ele exige de quem lê o que grava.
pub const FORMAT_VERSION: u32 = 1;
/// Maior `min_reader` que este leitor entende.
pub const READER_VERSION: u32 = 1;
/// Versão do esquema do `BrushPreset` dentro de `brush.json`.
pub const PRESET_SCHEMA: u32 = 1;
pub const MIME_BRUSH: &str = "application/x-animeiai-brush";
pub const MIME_PACK: &str = "application/x-animeiai-brushes";
pub const EXT_BRUSH: &str = "aiaibrush";
pub const EXT_PACK: &str = "aiaibrushes";
/// Licença padrão do pincel que o próprio usuário cria (ele pode trocar).
pub const DEFAULT_LICENSE: &str = "Proprietário / todos os direitos reservados";
/// Licença de pincel vindo de formato de terceiro (a do autor do pacote original).
pub const UNKNOWN_LICENSE: &str = "desconhecida — verifique com o autor";

/// Licenças com identificador SPDX que o app oferece e reconhece pelo id.
pub const KNOWN_SPDX: [&str; 11] = ["CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0", "CC-BY-NC-4.0", "CC-BY-NC-SA-4.0", "CC-BY-ND-4.0", "MIT", "Apache-2.0", "MPL-2.0", "Unlicense", "OFL-1.1"];
const PROPRIETARY_SPDX: &str = "LicenseRef-Proprietary";

/// Limites contra arquivo malformado ou malicioso. O limite de entradas é maior que o do relatório (1.000) porque uma
/// categoria inteira exportada como pacote tem 3 arquivos por pincel.
const MAX_ENTRIES: usize = 8192;
const MAX_ENTRY: usize = 64 << 20;
const MAX_TOTAL: usize = 256 << 20;
const MAX_JSON: usize = 2 << 20;
const MAX_THUMB: usize = 4 << 20;
const MAX_BRUSHES: usize = 2000;
/// Ponta no máximo 512 px de lado; textura (repete no canvas) até 2048.
const MAX_TIP_SIDE: u32 = 512;
const MAX_TEX_SIDE: u32 = 2048;
const MAX_PIXELS: u64 = 4096 * 4096;

// ------------------------------------------------------------------------------------------------ manifesto

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct License {
    /// Identificador SPDX (CC-BY-4.0...) ou `LicenseRef-Proprietary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spdx: Option<String>,
    /// Texto livre da licença, quando não é um id conhecido.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Arquivo do pacote com o texto integral (`LICENSE.txt`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Atribuição exigida pela licença (CC-BY): como citar o autor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

impl License {
    /// Licença escrita pelo usuário: um id SPDX conhecido vira `spdx`; o texto padrão vira proprietária; o resto é texto.
    pub fn from_text(s: &str) -> License {
        let s = s.trim();
        if let Some(id) = KNOWN_SPDX.iter().find(|k| k.eq_ignore_ascii_case(s)) {
            return License { spdx: Some((*id).to_owned()), ..Default::default() };
        }
        if s == DEFAULT_LICENSE {
            return License { spdx: Some(PROPRIETARY_SPDX.to_owned()), text: Some(s.to_owned()), ..Default::default() };
        }
        License { text: (!s.is_empty()).then(|| s.to_owned()), ..Default::default() }
    }

    /// O que o usuário vê: o texto, ou o id.
    pub fn to_text(&self) -> String {
        match (&self.text, &self.spdx) {
            (Some(t), _) if !t.trim().is_empty() => t.trim().to_owned(),
            (_, Some(s)) if s == PROPRIETARY_SPDX => DEFAULT_LICENSE.to_owned(),
            (_, Some(s)) => s.trim().to_owned(),
            _ => String::new(),
        }
    }

    pub fn is_set(&self) -> bool {
        !self.to_text().is_empty() || self.text_file.is_some()
    }

    /// Link da licença quando é CC (para mostrar antes de instalar).
    pub fn link(&self) -> Option<String> {
        if let Some(u) = self.url.as_ref().filter(|u| !u.is_empty()) {
            return Some(u.clone());
        }
        let id = self.spdx.as_deref()?;
        let low = id.to_ascii_lowercase();
        if let Some(rest) = low.strip_prefix("cc-") {
            let (kind, ver) = rest.rsplit_once('-')?;
            return Some(format!("https://creativecommons.org/licenses/{kind}/{ver}/"));
        }
        (!low.starts_with("licenseref")).then(|| format!("https://spdx.org/licenses/{id}.html"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Source {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// Formato de terceiro de onde o pincel veio (`photoshop-abr`, `procreate-brushset`...); vazio = feito aqui.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub imported_from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub original_file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub original_author: String,
}

impl Source {
    /// O texto de "Fonte" na tela: o arquivo de onde veio, ou o endereço.
    pub fn text(&self) -> String {
        if self.original_file.is_empty() { self.url.clone() } else { self.original_file.clone() }
    }

    fn set_text(&mut self, t: &str) {
        let t = t.trim();
        if t.starts_with("http://") || t.starts_with("https://") {
            self.url = t.to_owned();
            self.original_file.clear();
        } else {
            self.original_file = t.to_owned();
            self.url.clear();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRef {
    pub path: String,
    /// `tip` ou `texture`.
    pub role: String,
    pub blake3: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

/// Um pincel dentro de um pacote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackEntry {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    /// Versão mínima do leitor que entende o arquivo (um app mais antigo recusa, com o motivo).
    #[serde(default)]
    pub min_reader: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub author: BrushAuthor,
    #[serde(default)]
    pub license: License,
    #[serde(default)]
    pub source: Source,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub modified: String,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub preset_schema: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Só no pacote: os pincéis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub brushes: Vec<PackEntry>,
    /// Campos do usuário/de outras ferramentas: preservados ao regravar.
    #[serde(default)]
    pub extra: BTreeMap<String, Value>,
    /// Campos de versões futuras que este leitor não conhece (também preservados).
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Manifest {
    fn blank(format: &str) -> Manifest {
        Manifest {
            format: format.to_owned(),
            format_version: FORMAT_VERSION,
            min_reader: FORMAT_VERSION,
            id: String::new(),
            name: String::new(),
            description: String::new(),
            author: BrushAuthor::default(),
            license: License::default(),
            source: Source::default(),
            version: String::new(),
            created: String::new(),
            modified: String::new(),
            app_version: String::new(),
            preset_schema: PRESET_SCHEMA,
            category: String::new(),
            tags: Vec::new(),
            assets: Vec::new(),
            thumbnail: None,
            brushes: Vec::new(),
            extra: BTreeMap::new(),
            unknown: BTreeMap::new(),
        }
    }

    /// Manifesto de um pacote novo (a licença é obrigatória como a de qualquer pincel).
    pub fn new_pack(name: &str, author: BrushAuthor, license: &str) -> Manifest {
        let now = now_iso();
        let mut m = Manifest::blank(FORMAT_PACK);
        m.id = new_id();
        m.name = name.trim().to_owned();
        m.author = author;
        m.license = License::from_text(if license.trim().is_empty() { DEFAULT_LICENSE } else { license });
        m.version = "1.0.0".into();
        m.created = now.clone();
        m.modified = now;
        m
    }
}

// ------------------------------------------------------------------------------------------------ pincel em memória

/// Um pincel do formato: manifesto + preset (metadados espelhados do manifesto, imagens carregadas) + as imagens.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushFile {
    pub manifest: Manifest,
    pub preset: BrushPreset,
    /// Imagens da ponta e da textura como o app guarda ("AIM1"), por hash.
    pub assets: BTreeMap<BlobHash, Vec<u8>>,
    /// PNG 256×256; `None` = desenhado pelo motor ao gravar.
    pub thumbnail: Option<Vec<u8>>,
    /// Campos de `brush.json` que este app não conhece (versão futura): voltam ao arquivo ao gravar.
    pub preset_extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub manifest: Manifest,
    pub brushes: Vec<BrushFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Package {
    Brush(Box<BrushFile>),
    Pack(Pack),
}

impl BrushFile {
    /// Monta o pincel para gravar: `manifest` é o que o pincel já tinha (leitura anterior; mantém `extra`, fonte e
    /// licença detalhadas), `assets` entrega as imagens "AIM1" por hash. Completa o que falta no preset (id, versão,
    /// datas, licença padrão) e espelha no manifesto.
    pub fn from_preset(preset: &BrushPreset, manifest: Option<Manifest>, assets: &dyn Fn(&BlobHash) -> Option<Vec<u8>>) -> Result<BrushFile, String> {
        let mut preset = preset.clone();
        preset.validate()?;
        if preset.name.trim().is_empty() {
            return Err("o pincel precisa de um nome".into());
        }
        let now = now_iso();
        if preset.id.trim().is_empty() {
            preset.id = new_id();
        }
        if preset.version.trim().is_empty() {
            preset.version = "1.0.0".into();
        }
        if preset.created.trim().is_empty() {
            preset.created = now.clone();
        }
        if preset.modified.trim().is_empty() {
            preset.modified = preset.created.clone();
        }
        if preset.license.trim().is_empty() {
            preset.license = DEFAULT_LICENSE.into();
        }
        let mut found = BTreeMap::new();
        let mut get = |h: &BlobHash, what: &str, mask: Option<&Mask>| -> Result<(), String> {
            if found.contains_key(h) {
                return Ok(());
            }
            let b = assets(h).filter(|b| BlobHash::of(b) == *h).or_else(|| mask.map(|m| m.encode()).filter(|b| BlobHash::of(b) == *h)).ok_or_else(|| format!("imagem da {what} do pincel \"{}\" não encontrada na biblioteca", preset.name))?;
            found.insert(h.clone(), b);
            Ok(())
        };
        if let BrushTip::Image { blob, mask } = &preset.tip {
            get(blob, "ponta", mask.as_ref())?;
        }
        if let Some(t) = &preset.texture {
            if let Some(h) = &t.image {
                get(h, "textura", t.image_mask.as_deref())?;
            }
        }
        // máscaras carregadas no preset (miniatura e prévia usam)
        if let BrushTip::Image { blob, mask } = &mut preset.tip {
            if mask.is_none() {
                *mask = found.get(blob).and_then(|b| Mask::decode(b).ok());
            }
        }
        if let Some(t) = &mut preset.texture {
            if let (Some(h), None) = (&t.image, &t.image_mask) {
                t.image_mask = found.get(h).and_then(|b| Mask::decode(b).ok()).map(std::sync::Arc::new);
            }
        }
        let mut manifest = manifest.filter(|m| m.format == FORMAT_BRUSH).unwrap_or_else(|| Manifest::blank(FORMAT_BRUSH));
        sync_manifest_from_preset(&mut manifest, &preset);
        Ok(BrushFile { manifest, preset, assets: found, thumbnail: None, preset_extra: BTreeMap::new() })
    }
}

/// Preset → manifesto (o que a tela edita vira o que o arquivo diz; o resto do manifesto fica como estava).
fn sync_manifest_from_preset(m: &mut Manifest, p: &BrushPreset) {
    m.format = FORMAT_BRUSH.into();
    m.format_version = FORMAT_VERSION;
    m.min_reader = FORMAT_VERSION;
    m.preset_schema = PRESET_SCHEMA;
    m.id = p.id.clone();
    m.name = p.name.trim().to_owned();
    m.category = p.category.trim().to_owned();
    m.tags = p.tags.clone();
    m.author = p.author.clone();
    if m.license.to_text() != p.license.trim() {
        // a licença mudou na tela: nova (url e atribuição antigas não valem para outra licença)
        m.license = License::from_text(&p.license);
    }
    m.version = p.version.clone();
    m.created = p.created.clone();
    m.modified = p.modified.clone();
    m.app_version = app_version();
    if m.source.text() != p.source.trim() {
        m.source.set_text(&p.source);
    }
}

/// Manifesto → preset (a leitura de um arquivo: o manifesto manda nos metadados).
fn apply_manifest_to_preset(m: &Manifest, p: &mut BrushPreset) {
    p.id = m.id.clone();
    p.name = m.name.clone();
    p.category = m.category.clone();
    p.tags = m.tags.clone();
    p.author = m.author.clone();
    p.license = m.license.to_text();
    p.version = m.version.clone();
    p.created = m.created.clone();
    p.modified = m.modified.clone();
    p.source = m.source.text();
}

fn app_version() -> String {
    format!("AnimeIAI {}", env!("CARGO_PKG_VERSION"))
}

/// Identificador novo (UUID v7: ordenável pelo momento de criação).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Agora, em ISO 8601 UTC (`2026-10-19T12:00:00Z`).
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    iso_from_unix(secs)
}

pub fn iso_from_unix(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // dias desde 1970-01-01 → data civil (algoritmo de Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Compara versões `1.2.0` número a número (parte que não é número vale 0).
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> { s.trim().split('.').map(|p| p.trim().parse::<u64>().unwrap_or(0)).collect() };
    let (mut x, mut y) = (parse(a), parse(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x.cmp(&y)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

// ------------------------------------------------------------------------------------------------ PNG e miniatura

fn encode_png(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(data).map_err(|e| e.to_string())?;
        wr.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Máscara → PNG de 8 bits em cinza (255 = tinta).
pub fn mask_to_png(m: &Mask) -> Result<Vec<u8>, String> {
    encode_png(m.width, m.height, png::ColorType::Grayscale, &m.data)
}

/// PNG → máscara. Cinza vale como está; com transparência vale o alfa; colorido vale a luminância.
pub fn mask_from_png(bytes: &[u8], max_side: u32) -> Result<Mask, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut rd = dec.read_info().map_err(|e| format!("PNG ilegível: {e}"))?;
    let (w, h) = (rd.info().width, rd.info().height);
    if w == 0 || h == 0 || w.max(h) > max_side || w as u64 * h as u64 > MAX_PIXELS {
        return Err(format!("imagem {w}×{h} fora do limite ({max_side} px de lado)"));
    }
    let mut buf = vec![0u8; rd.output_buffer_size()];
    let fi = rd.next_frame(&mut buf).map_err(|e| format!("PNG ilegível: {e}"))?;
    let (ct, bd) = rd.output_color_type();
    if bd != png::BitDepth::Eight {
        return Err("PNG com profundidade de cor não suportada".into());
    }
    let ch = match ct {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("PNG indexado não suportado".into()),
    };
    let px = &buf[..fi.buffer_size()];
    if px.len() != w as usize * h as usize * ch {
        return Err("PNG com tamanho inconsistente".into());
    }
    let data: Vec<u8> = match ct {
        png::ColorType::Grayscale => px.to_vec(),
        png::ColorType::GrayscaleAlpha => {
            let alpha = px.chunks_exact(2).any(|p| p[1] < 250);
            px.chunks_exact(2).map(|p| if alpha { p[1] } else { p[0] }).collect()
        }
        png::ColorType::Rgb => px.chunks_exact(3).map(|p| luma(p[0], p[1], p[2])).collect(),
        _ => {
            let alpha = px.chunks_exact(4).any(|p| p[3] < 250);
            px.chunks_exact(4).map(|p| if alpha { p[3] } else { luma(p[0], p[1], p[2]) }).collect()
        }
    };
    Ok(Mask { width: w, height: h, data })
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) as u8
}

/// Miniatura 256×256 do pincel, desenhada pelo próprio motor (um S com pressão subindo e descendo). O preset precisa
/// ter as máscaras de imagem carregadas (`BrushFile::preset` tem).
pub fn render_thumbnail(preset: &BrushPreset) -> Result<Vec<u8>, String> {
    use crate::brush::{BrushMode, BrushPoint, paint_stroke};
    let (w, h) = (256u32, 256u32);
    let (bg, ink) = ([0.95, 0.95, 0.94, 1.0], [0.1, 0.1, 0.13, 1.0]);
    let mut img = crate::Image::filled(w, h, bg);
    let mut q = preset.clone();
    q.size = q.size.clamp(8.0, 56.0);
    if q.mode == BrushMode::Erase {
        // borracha: a faixa de tinta que ela apaga
        for y in 90..166 {
            for x in 12..w - 12 {
                let k = img.idx(x, y);
                img.data[k] = ink;
            }
        }
    }
    let path: Vec<BrushPoint> = (0..96)
        .map(|k| {
            let t = k as f32 / 95.0;
            BrushPoint { x: 28.0 + t * (w as f32 - 56.0), y: h as f32 / 2.0 + 52.0 * (t * std::f32::consts::TAU).sin(), pressure: 0.2 + 0.8 * (t * std::f32::consts::PI).sin() }
        })
        .collect();
    paint_stroke(&mut img, &q, &path, ink, 11);
    encode_png(w, h, png::ColorType::Rgba, &img.to_rgba8())
}

// ------------------------------------------------------------------------------------------------ zip

/// Entrada para gravar: `deflate` falso = guardada (PNG, mimetype).
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
    pub deflate: bool,
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(data);
    c.sum()
}

/// Grava um zip (guardado/deflate) com data fixa: o mesmo conteúdo dá sempre os mesmos bytes.
pub fn write_zip(entries: &[ZipEntry]) -> Result<Vec<u8>, String> {
    let mut out: Vec<u8> = Vec::new();
    let mut cd: Vec<u8> = Vec::new();
    for e in entries {
        if e.data.len() > u32::MAX as usize / 2 || out.len() > u32::MAX as usize / 2 {
            return Err("arquivo grande demais para o zip".into());
        }
        let (method, body): (u16, Vec<u8>) = if e.deflate {
            let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&e.data).map_err(|x| x.to_string())?;
            let z = enc.finish().map_err(|x| x.to_string())?;
            if z.len() < e.data.len() { (8, z) } else { (0, e.data.clone()) }
        } else {
            (0, e.data.clone())
        };
        let crc = crc32(&e.data);
        let off = out.len() as u32;
        let name = e.name.as_bytes();
        // 0x0021 = 1980-01-01 (data fixa); bit 11 = nomes em UTF-8
        let mut head = Vec::with_capacity(30 + name.len());
        head.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        head.extend_from_slice(&20u16.to_le_bytes());
        head.extend_from_slice(&0x0800u16.to_le_bytes());
        head.extend_from_slice(&method.to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes());
        head.extend_from_slice(&0x0021u16.to_le_bytes());
        head.extend_from_slice(&crc.to_le_bytes());
        head.extend_from_slice(&(body.len() as u32).to_le_bytes());
        head.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        head.extend_from_slice(&(name.len() as u16).to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes());
        head.extend_from_slice(name);
        out.extend_from_slice(&head);
        out.extend_from_slice(&body);
        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&0x0800u16.to_le_bytes());
        cd.extend_from_slice(&method.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0x0021u16.to_le_bytes());
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&(body.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
        cd.extend_from_slice(&[0u8; 12]); // extra, comentário, disco, atributos interno e externo
        cd.extend_from_slice(&off.to_le_bytes());
        cd.extend_from_slice(name);
    }
    if entries.len() > u16::MAX as usize {
        return Err("entradas demais para o zip".into());
    }
    let cd_off = out.len() as u32;
    out.extend_from_slice(&cd);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// Caminho seguro de uma entrada do zip: relativo, com `/`, sem `..`, sem unidade, sem componentes vazios.
fn safe_entry_name(name: &str) -> Result<(), String> {
    let bad = name.is_empty() || name.len() > 240 || name.starts_with('/') || name.contains(['\\', ':', '\0']) || name.split('/').any(|c| c.is_empty() || c == ".." || c == ".");
    if bad { Err(format!("caminho inválido no zip (recusado): {:?}", name.chars().take(80).collect::<String>())) } else { Ok(()) }
}

/// Lê o zip conferindo tudo: entradas, tamanhos, caminhos (zip slip), repetidas, CRC. Devolve na ordem do diretório.
pub fn read_zip_checked(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cut = || "zip cortado ou corrompido".to_owned();
    let le16 = |p: usize| bytes.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize).ok_or_else(cut);
    let le32 = |p: usize| bytes.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize).ok_or_else(cut);
    if bytes.len() < 22 {
        return Err("não é um pincel do AnimeIAI (arquivo muito pequeno)".into());
    }
    let lo = bytes.len().saturating_sub(65_557);
    let eocd = (lo..=bytes.len() - 22).rev().find(|&p| bytes[p..p + 4] == [0x50, 0x4b, 0x05, 0x06]).ok_or("não é um zip")?;
    let count = le16(eocd + 10)?;
    let cd_size = le32(eocd + 12)?;
    let mut p = le32(eocd + 16)?;
    if count > MAX_ENTRIES {
        return Err(format!("zip com entradas demais ({count}; o limite é {MAX_ENTRIES})"));
    }
    if p.checked_add(cd_size).is_none_or(|e| e > bytes.len()) {
        return Err("diretório do zip fora do arquivo".into());
    }
    let mut out = Vec::with_capacity(count);
    let mut seen = HashSet::new();
    let mut total = 0usize;
    for _ in 0..count {
        if le32(p)? != 0x0201_4b50 {
            return Err("diretório do zip inválido".into());
        }
        let flags = le16(p + 8)?;
        let method = le16(p + 10)?;
        let crc = le32(p + 16)? as u32;
        let csize = le32(p + 20)?;
        let usize_ = le32(p + 24)?;
        let (nl, el, cl) = (le16(p + 28)?, le16(p + 30)?, le16(p + 32)?);
        let local = le32(p + 42)?;
        let name = std::str::from_utf8(bytes.get(p + 46..p + 46 + nl).ok_or_else(cut)?).map_err(|_| "nome de arquivo do zip não é UTF-8".to_owned())?.to_owned();
        p += 46 + nl + el + cl;
        if name.ends_with('/') && usize_ == 0 {
            continue; // pasta
        }
        safe_entry_name(&name)?;
        if !seen.insert(name.to_lowercase()) {
            return Err(format!("arquivo repetido no zip (recusado): {name}"));
        }
        if flags & 1 != 0 {
            return Err(format!("{name}: arquivo criptografado não suportado"));
        }
        if usize_ > MAX_ENTRY {
            return Err(format!("{name}: arquivo grande demais ({usize_} bytes; o limite é {MAX_ENTRY})"));
        }
        total += usize_;
        if total > MAX_TOTAL {
            return Err(format!("conteúdo descomprimido grande demais (limite {} MB)", MAX_TOTAL >> 20));
        }
        if le32(local)? != 0x0403_4b50 {
            return Err(format!("{name}: cabeçalho local inválido"));
        }
        let start = local + 30 + le16(local + 26)? + le16(local + 28)?;
        let data = bytes.get(start..start.checked_add(csize).ok_or_else(cut)?).ok_or_else(cut)?;
        let file = match method {
            0 => {
                if csize != usize_ {
                    return Err(format!("{name}: tamanhos inconsistentes"));
                }
                data.to_vec()
            }
            8 => {
                let mut v = Vec::with_capacity(usize_.min(1 << 20));
                flate2::read::DeflateDecoder::new(data).take(usize_ as u64 + 1).read_to_end(&mut v).map_err(|e| format!("{name}: {e}"))?;
                v
            }
            m => return Err(format!("{name}: compressão {m} do zip não suportada")),
        };
        if file.len() != usize_ {
            return Err(format!("{name}: tamanho descomprimido não confere (zip bomba ou corrompido)"));
        }
        if crc32(&file) != crc {
            return Err(format!("{name}: CRC não confere (arquivo corrompido)"));
        }
        out.push((name, file));
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ gravar

fn preset_json(file: &BrushFile) -> Result<Vec<u8>, String> {
    let mut v = serde_json::to_value(&file.preset).map_err(|e| e.to_string())?;
    if let Value::Object(o) = &mut v {
        for (k, x) in &file.preset_extra {
            o.entry(k.clone()).or_insert_with(|| x.clone());
        }
        o.insert("preset_schema".into(), PRESET_SCHEMA.into());
    }
    serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())
}

/// Imagens do pincel como PNG: (papel, AIM1 → PNG, largura, altura).
fn asset_pngs(file: &BrushFile) -> Result<Vec<(&'static str, Vec<u8>, u32, u32)>, String> {
    let mut v = Vec::new();
    let mut add = |role: &'static str, h: &BlobHash| -> Result<(), String> {
        let aim = file.assets.get(h).ok_or_else(|| format!("imagem {h} ausente"))?;
        let m = Mask::decode(aim)?;
        v.push((role, mask_to_png(&m)?, m.width, m.height));
        Ok(())
    };
    if let BrushTip::Image { blob, .. } = &file.preset.tip {
        add("tip", blob)?;
    }
    if let Some(h) = file.preset.texture.as_ref().and_then(|t| t.image.as_ref()) {
        add("texture", h)?;
    }
    Ok(v)
}

fn json_entry(name: &str, v: &impl Serialize) -> Result<ZipEntry, String> {
    Ok(ZipEntry { name: name.to_owned(), data: serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?, deflate: true })
}

fn license_file(m: &mut Manifest) -> Option<ZipEntry> {
    let text = m.license.text.clone().filter(|t| !t.trim().is_empty())?;
    m.license.text_file = Some("LICENSE.txt".into());
    Some(ZipEntry { name: "LICENSE.txt".into(), data: text.into_bytes(), deflate: true })
}

/// Grava um pincel como `.aiaibrush`.
pub fn write_brush(file: &BrushFile) -> Result<Vec<u8>, String> {
    let f = file.clone();
    f.preset.validate()?;
    let mut m = f.manifest.clone();
    sync_manifest_from_preset(&mut m, &f.preset);
    let thumb = match &f.thumbnail {
        Some(t) => t.clone(),
        None => render_thumbnail(&f.preset)?,
    };
    let mut entries = vec![ZipEntry { name: "mimetype".into(), data: MIME_BRUSH.as_bytes().to_vec(), deflate: false }];
    m.assets.clear();
    let mut assets = Vec::new();
    for (role, png, w, h) in asset_pngs(&f)? {
        let path = format!("assets/{role}.png");
        m.assets.push(AssetRef { path: path.clone(), role: role.into(), blake3: blake3_hex(&png), width: w, height: h });
        assets.push(ZipEntry { name: path, data: png, deflate: false });
    }
    m.thumbnail = Some("thumbnail.png".into());
    let lic = license_file(&mut m);
    entries.push(json_entry("manifest.json", &m)?);
    entries.push(ZipEntry { name: "brush.json".into(), data: preset_json(&f)?, deflate: true });
    entries.extend(assets);
    entries.push(ZipEntry { name: "thumbnail.png".into(), data: thumb, deflate: false });
    entries.extend(lic);
    write_zip(&entries)
}

/// Grava vários pincéis como `.aiaibrushes`. `pack` traz nome, autor e licença do pacote; pincel sem autor/licença
/// própria herda os do pacote (cada pincel mantém a licença que tem).
pub fn write_pack(pack: &Manifest, brushes: &[BrushFile]) -> Result<Vec<u8>, String> {
    if brushes.is_empty() {
        return Err("o pacote não tem pincéis".into());
    }
    if brushes.len() > MAX_BRUSHES {
        return Err(format!("pincéis demais para um pacote (limite {MAX_BRUSHES})"));
    }
    let mut root = pack.clone();
    root.format = FORMAT_PACK.into();
    root.format_version = FORMAT_VERSION;
    root.min_reader = FORMAT_VERSION;
    root.app_version = app_version();
    root.assets.clear();
    root.brushes.clear();
    if root.id.is_empty() {
        root.id = new_id();
    }
    if root.name.trim().is_empty() {
        return Err("o pacote precisa de um nome".into());
    }
    if !root.license.is_set() {
        root.license = License::from_text(DEFAULT_LICENSE);
    }
    let mut entries = vec![ZipEntry { name: "mimetype".into(), data: MIME_PACK.as_bytes().to_vec(), deflate: false }];
    let mut body: Vec<ZipEntry> = Vec::new();
    let mut shared: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut ids = HashSet::new();
    let mut first_thumb: Option<Vec<u8>> = None;
    for b in brushes {
        let mut f = b.clone();
        if f.preset.id.is_empty() {
            f.preset.id = new_id();
        }
        if !ids.insert(f.preset.id.clone()) {
            continue; // o mesmo pincel duas vezes
        }
        if f.preset.author.is_empty() {
            f.preset.author = root.author.clone();
        }
        if f.preset.license.trim().is_empty() {
            f.preset.license = root.license.to_text();
        }
        f.preset.validate()?;
        let mut m = f.manifest.clone();
        sync_manifest_from_preset(&mut m, &f.preset);
        m.assets.clear();
        for (role, png, w, h) in asset_pngs(&f)? {
            let hash = blake3_hex(&png);
            let path = format!("assets/{hash}.png");
            m.assets.push(AssetRef { path: path.clone(), role: role.into(), blake3: hash, width: w, height: h });
            shared.insert(path, png);
        }
        let dir = format!("brushes/{}", f.preset.id);
        let thumb = match &f.thumbnail {
            Some(t) => t.clone(),
            None => render_thumbnail(&f.preset)?,
        };
        m.thumbnail = Some(format!("{dir}/thumbnail.png"));
        if m.license.text.as_ref().is_some_and(|t| !t.trim().is_empty()) {
            m.license.text_file = None; // a licença de cada pincel vai inline; só a do pacote tem LICENSE.txt
        }
        body.push(json_entry(&format!("{dir}/manifest.json"), &m)?);
        body.push(ZipEntry { name: format!("{dir}/brush.json"), data: preset_json(&f)?, deflate: true });
        body.push(ZipEntry { name: format!("{dir}/thumbnail.png"), data: thumb.clone(), deflate: false });
        first_thumb.get_or_insert(thumb);
        root.brushes.push(PackEntry { id: f.preset.id.clone(), name: f.preset.name.clone(), path: dir });
    }
    for (path, png) in &shared {
        let (w, h) = png_dims(png);
        root.assets.push(AssetRef { path: path.clone(), role: "shared".into(), blake3: blake3_hex(png), width: w, height: h });
    }
    root.thumbnail = first_thumb.is_some().then(|| "thumbnail.png".to_owned());
    let lic = license_file(&mut root);
    entries.push(json_entry("manifest.json", &root)?);
    entries.extend(lic);
    if let Some(t) = first_thumb {
        entries.push(ZipEntry { name: "thumbnail.png".into(), data: t, deflate: false });
    }
    entries.extend(body);
    for (path, png) in shared {
        entries.push(ZipEntry { name: path, data: png, deflate: false });
    }
    write_zip(&entries)
}

fn blake3_hex(b: &[u8]) -> String {
    BlobHash::of(b).0
}

/// Largura e altura de um PNG pelo cabeçalho (IHDR); (0, 0) se não der.
fn png_dims(png: &[u8]) -> (u32, u32) {
    if png.len() >= 24 && png[12..16] == *b"IHDR" {
        (u32::from_be_bytes([png[16], png[17], png[18], png[19]]), u32::from_be_bytes([png[20], png[21], png[22], png[23]]))
    } else {
        (0, 0)
    }
}

// ------------------------------------------------------------------------------------------------ ler

/// Lê `.aiaibrush` ou `.aiaibrushes` conferindo tudo (ver o topo do arquivo). Erro = motivo em português.
pub fn read(bytes: &[u8]) -> Result<Package, String> {
    let entries = read_zip_checked(bytes)?;
    let Some((first, mime)) = entries.first() else { return Err("zip vazio: não é um pincel do AnimeIAI".into()) };
    if first != "mimetype" {
        return Err("não é um pincel do AnimeIAI: o primeiro arquivo do zip deve ser o mimetype".into());
    }
    let pack = match std::str::from_utf8(mime).unwrap_or("") {
        MIME_BRUSH => false,
        MIME_PACK => true,
        other => return Err(format!("não é um pincel do AnimeIAI (tipo {:?})", other.chars().take(60).collect::<String>())),
    };
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (n, d) in entries.into_iter().skip(1) {
        if n == "mimetype" {
            return Err("mimetype repetido".into());
        }
        files.insert(n, d);
    }
    let manifest = parse_manifest(files.get("manifest.json").ok_or("falta o manifest.json")?, if pack { FORMAT_PACK } else { FORMAT_BRUSH })?;
    let mut cited: HashSet<String> = ["manifest.json".to_owned()].into();
    if let Some(t) = &manifest.thumbnail {
        check_thumbnail(&files, t)?;
        cited.insert(t.clone());
    }
    let mut manifest = manifest;
    if let Some(lf) = manifest.license.text_file.clone() {
        let text = files.get(&lf).ok_or_else(|| format!("{lf}: a licença cita um arquivo que não está no pacote"))?;
        cited.insert(lf);
        if manifest.license.text.as_deref().is_none_or(|t| t.trim().is_empty()) {
            manifest.license.text = Some(String::from_utf8_lossy(text).trim().to_owned()).filter(|t| !t.is_empty());
        }
    }
    if !manifest.license.is_set() {
        return Err("o manifesto não tem licença (obrigatória)".into());
    }
    if !pack {
        let brush = files.get("brush.json").ok_or("falta o brush.json")?;
        cited.insert("brush.json".into());
        let thumb = manifest.thumbnail.as_ref().and_then(|t| files.get(t).cloned());
        let mut b = build_brush_file(&files, manifest, brush, &mut cited)?;
        b.thumbnail = thumb;
        reject_uncited(&files, &cited)?;
        return Ok(Package::Brush(Box::new(b)));
    }
    if manifest.brushes.is_empty() {
        return Err("o pacote não tem pincéis".into());
    }
    if manifest.brushes.len() > MAX_BRUSHES {
        return Err(format!("pacote com pincéis demais ({}; o limite é {MAX_BRUSHES})", manifest.brushes.len()));
    }
    // o pacote cita as imagens compartilhadas
    for a in &manifest.assets {
        cited.insert(a.path.clone());
    }
    let mut out = Vec::new();
    let mut ids = HashSet::new();
    for e in &manifest.brushes {
        if !valid_id(&e.id) || e.path != format!("brushes/{}", e.id) {
            return Err(format!("pincel do pacote com id/caminho inválido: {:?}", e.id.chars().take(40).collect::<String>()));
        }
        if !ids.insert(e.id.clone()) {
            return Err(format!("pincel repetido no pacote: {}", e.id));
        }
        let mp = format!("{}/manifest.json", e.path);
        let bp = format!("{}/brush.json", e.path);
        let bm = parse_manifest(files.get(&mp).ok_or_else(|| format!("falta {mp}"))?, FORMAT_BRUSH)?;
        if bm.id != e.id {
            return Err(format!("{mp}: o id não confere com o do pacote"));
        }
        cited.insert(mp);
        cited.insert(bp.clone());
        if let Some(t) = &bm.thumbnail {
            if !t.starts_with(&format!("{}/", e.path)) {
                return Err(format!("{t}: miniatura fora da pasta do pincel"));
            }
            check_thumbnail(&files, t)?;
            cited.insert(t.clone());
        }
        let thumb = bm.thumbnail.as_ref().and_then(|t| files.get(t).cloned());
        let mut bm = bm;
        if !bm.license.is_set() {
            return Err(format!("{}: o pincel não tem licença (obrigatória)", bm.name));
        }
        if let Some(lf) = bm.license.text_file.clone() {
            let text = files.get(&lf).ok_or_else(|| format!("{lf}: a licença cita um arquivo que não está no pacote"))?;
            cited.insert(lf);
            if bm.license.text.as_deref().is_none_or(|t| t.trim().is_empty()) {
                bm.license.text = Some(String::from_utf8_lossy(text).trim().to_owned()).filter(|t| !t.is_empty());
            }
        }
        let brush = files.get(&bp).ok_or_else(|| format!("falta {bp}"))?;
        let mut b = build_brush_file(&files, bm, brush, &mut cited)?;
        b.thumbnail = thumb;
        out.push(b);
    }
    reject_uncited(&files, &cited)?;
    Ok(Package::Pack(Pack { manifest, brushes: out }))
}

fn parse_manifest(bytes: &[u8], format: &str) -> Result<Manifest, String> {
    if bytes.len() > MAX_JSON {
        return Err("manifest.json grande demais".into());
    }
    let raw: Value = serde_json::from_slice(bytes).map_err(|e| format!("manifest.json ilegível: {e}"))?;
    let fmt = raw.get("format").and_then(Value::as_str).unwrap_or("");
    if fmt != format {
        return Err(format!("o manifesto é de outro formato ({fmt:?}, esperado {format:?})"));
    }
    let fv = raw.get("format_version").and_then(Value::as_u64).unwrap_or(1);
    let min = raw.get("min_reader").and_then(Value::as_u64).unwrap_or(fv);
    if min > READER_VERSION as u64 {
        return Err(format!("este pincel exige uma versão mais nova do AnimeIAI (formato {fv}, leitor mínimo {min}; este app lê até o {READER_VERSION}). Atualize o app para abri-lo."));
    }
    let mut m: Manifest = serde_json::from_value(raw).map_err(|e| format!("manifest.json inválido: {e}"))?;
    m.min_reader = min as u32;
    if m.name.trim().is_empty() || m.name.chars().count() > 200 {
        return Err("manifest.json: nome ausente ou longo demais".into());
    }
    if !valid_id(&m.id) {
        return Err("manifest.json: id inválido".into());
    }
    if m.tags.len() > 64 || m.tags.iter().any(|t| t.chars().count() > 64) {
        return Err("manifest.json: etiquetas demais ou longas demais".into());
    }
    if m.assets.len() > 4 * MAX_BRUSHES {
        return Err("manifest.json: imagens demais".into());
    }
    Ok(m)
}

fn check_thumbnail(files: &BTreeMap<String, Vec<u8>>, path: &str) -> Result<(), String> {
    safe_entry_name(path)?;
    let t = files.get(path).ok_or_else(|| format!("{path}: a miniatura citada não está no arquivo"))?;
    if t.len() > MAX_THUMB || t.get(..8) != Some(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..]) {
        return Err(format!("{path}: miniatura inválida (não é PNG ou grande demais)"));
    }
    Ok(())
}

fn reject_uncited(files: &BTreeMap<String, Vec<u8>>, cited: &HashSet<String>) -> Result<(), String> {
    for n in files.keys() {
        if !cited.contains(n) && n != "README.md" {
            return Err(format!("{n}: arquivo no pacote que o manifesto não cita (recusado)"));
        }
    }
    Ok(())
}

/// Chaves que o `BrushPreset` conhece (pelo esquema): o que sobra em `brush.json` é de versão futura.
fn known_preset_keys() -> &'static BTreeSet<String> {
    static KEYS: std::sync::OnceLock<BTreeSet<String>> = std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        let schema = serde_json::to_value(schemars::schema_for!(BrushPreset)).unwrap_or(Value::Null);
        schema.get("schema").unwrap_or(&schema).get("properties").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default()
    })
}

/// Um pincel a partir do manifesto e do `brush.json`: confere imagens (hash), preset e carrega as máscaras.
fn build_brush_file(files: &BTreeMap<String, Vec<u8>>, manifest: Manifest, brush_json: &[u8], cited: &mut HashSet<String>) -> Result<BrushFile, String> {
    if brush_json.len() > MAX_JSON {
        return Err("brush.json grande demais".into());
    }
    let raw: Value = serde_json::from_slice(brush_json).map_err(|e| format!("brush.json ilegível: {e}"))?;
    let schema = raw.get("preset_schema").and_then(Value::as_u64).unwrap_or(1);
    if schema > PRESET_SCHEMA as u64 {
        return Err(format!("{}: o preset é de uma versão mais nova do AnimeIAI (esquema {schema}; este app lê até o {PRESET_SCHEMA})", manifest.name));
    }
    let Value::Object(map) = &raw else { return Err("brush.json não é um objeto".into()) };
    let known = known_preset_keys();
    let preset_extra: BTreeMap<String, Value> = map.iter().filter(|(k, _)| !known.contains(*k) && *k != "preset_schema").map(|(k, v)| (k.clone(), v.clone())).collect();
    let mut preset: BrushPreset = serde_json::from_value(raw).map_err(|e| format!("{}: brush.json inválido: {e}", manifest.name))?;
    // imagens: existem, conferem com o hash e cada uma é citada
    let mut pngs: BTreeMap<&str, (&AssetRef, &Vec<u8>)> = BTreeMap::new();
    for a in &manifest.assets {
        safe_entry_name(&a.path)?;
        let data = files.get(&a.path).ok_or_else(|| format!("{}: o manifesto cita um arquivo que não está no pacote", a.path))?;
        if !a.blake3.eq_ignore_ascii_case(&blake3_hex(data)) {
            return Err(format!("{}: hash BLAKE3 não confere (arquivo alterado ou corrompido)", a.path));
        }
        cited.insert(a.path.clone());
        pngs.entry(if a.role == "texture" { "texture" } else if a.role == "tip" { "tip" } else { "other" }).or_insert((a, data));
    }
    let mut assets = BTreeMap::new();
    if let BrushTip::Image { blob, mask } = &mut preset.tip {
        let (_, png) = pngs.get("tip").ok_or_else(|| format!("{}: o pincel usa ponta de imagem mas o pacote não traz a imagem", manifest.name))?;
        let m = mask_from_png(png, MAX_TIP_SIDE).map_err(|e| format!("ponta: {e}"))?;
        let aim = m.encode();
        *blob = BlobHash::of(&aim);
        assets.insert(blob.clone(), aim);
        *mask = Some(m);
    }
    if let Some(t) = &mut preset.texture {
        if t.image.is_some() {
            let (_, png) = pngs.get("texture").ok_or_else(|| format!("{}: o pincel usa textura de imagem mas o pacote não traz a imagem", manifest.name))?;
            let m = mask_from_png(png, MAX_TEX_SIDE).map_err(|e| format!("textura: {e}"))?;
            let aim = m.encode();
            let h = BlobHash::of(&aim);
            assets.insert(h.clone(), aim);
            t.image = Some(h);
            t.image_mask = Some(std::sync::Arc::new(m));
        }
    }
    apply_manifest_to_preset(&manifest, &mut preset);
    preset.validate().map_err(|e| format!("{}: {e}", manifest.name))?;
    Ok(BrushFile { manifest, preset, assets, thumbnail: None, preset_extra })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{BrushTexture, TextureKind, TextureMode};

    fn image_brush(name: &str, seed: u8) -> (BrushPreset, BTreeMap<BlobHash, Vec<u8>>) {
        let mut tip = Mask::new(40, 24);
        for (i, v) in tip.data.iter_mut().enumerate() {
            *v = ((i * 7 + seed as usize * 13) % 256) as u8;
        }
        let mut tex = Mask::new(32, 32);
        for (i, v) in tex.data.iter_mut().enumerate() {
            *v = ((i * 11 + seed as usize) % 251) as u8;
        }
        let (tb, xb) = (tip.encode(), tex.encode());
        let mut p = BrushPreset::round(name, 30.0, 0.8);
        p.tip = BrushTip::Image { blob: BlobHash::of(&tb), mask: Some(tip) };
        p.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 1.0, depth: 0.6, mode: TextureMode::Multiply, seed: 0, invert: false, image: Some(BlobHash::of(&xb)), image_mask: Some(std::sync::Arc::new(tex)) });
        p.spacing = 0.25;
        p.category = "cenario".into();
        p.tags = vec!["nuvem".into(), "céu".into()];
        p.author = BrushAuthor { name: "Ana".into(), email: "ana@exemplo.com".into(), url: "https://exemplo.com".into() };
        p.license = "CC-BY-4.0".into();
        p.source = "nuvens.abr".into();
        let assets = [(BlobHash::of(&tb), tb), (BlobHash::of(&xb), xb)].into();
        (p, assets)
    }

    fn file_of(name: &str, seed: u8) -> BrushFile {
        let (p, assets) = image_brush(name, seed);
        BrushFile::from_preset(&p, None, &|h| assets.get(h).cloned()).unwrap()
    }

    #[test]
    fn brush_round_trip_keeps_preset_assets_and_extra() {
        let mut f = file_of("Nuvem cirro", 1);
        f.manifest.extra.insert("meu_campo".into(), serde_json::json!({"a": [1, 2]}));
        f.manifest.unknown.insert("campo_futuro".into(), Value::from(7));
        f.manifest.source.imported_from = "photoshop-abr".into();
        f.manifest.source.original_author = "Zé".into();
        f.preset_extra.insert("dinamica_futura".into(), serde_json::json!({"x": 1}));
        let bytes = write_brush(&f).unwrap();
        // zip: mimetype primeiro, guardado
        assert_eq!(&bytes[30..30 + 8], b"mimetype");
        assert_eq!(&bytes[..4], &[0x50, 0x4b, 3, 4]);
        let Package::Brush(r) = read(&bytes).unwrap() else { panic!("era um pincel") };
        assert_eq!(r.preset, f.preset, "mesmo preset (metadados, ponta e textura)");
        assert_eq!(r.assets, f.assets, "imagens AIM1 idênticas");
        assert_eq!(r.manifest.id, f.preset.id);
        assert_eq!(r.manifest.extra, f.manifest.extra);
        assert_eq!(r.manifest.unknown, f.manifest.unknown);
        assert_eq!(r.manifest.source.imported_from, "photoshop-abr");
        assert_eq!(r.manifest.license.spdx.as_deref(), Some("CC-BY-4.0"));
        assert_eq!(r.preset_extra, f.preset_extra);
        assert_eq!(r.manifest.assets.len(), 2);
        assert!(r.thumbnail.as_ref().is_some_and(|t| t.starts_with(&[0x89, b'P'])));
        // regravar o que foi lido dá os mesmos bytes (PNG das imagens idêntico, miniatura reaproveitada)
        let again = write_brush(&r).unwrap();
        let Package::Brush(r2) = read(&again).unwrap() else { panic!() };
        assert_eq!(r2.preset, r.preset);
        let png = |b: &[u8], n: &str| read_zip_checked(b).unwrap().into_iter().find(|(k, _)| k == n).unwrap().1;
        assert_eq!(png(&bytes, "assets/tip.png"), png(&again, "assets/tip.png"));
        assert_eq!(png(&bytes, "assets/texture.png"), png(&again, "assets/texture.png"));
        assert_eq!(png(&bytes, "thumbnail.png"), png(&again, "thumbnail.png"));
        // o PNG da ponta decodifica para a mesma máscara
        let m = mask_from_png(&png(&bytes, "assets/tip.png"), 512).unwrap();
        let BrushTip::Image { mask: Some(orig), .. } = &f.preset.tip else { panic!() };
        assert_eq!(&m, orig);
    }

    #[test]
    fn license_text_is_kept_and_license_file_written() {
        let (mut p, assets) = image_brush("Texto", 2);
        p.license = "Uso livre em projetos pessoais; proibido revender.".into();
        let f = BrushFile::from_preset(&p, None, &|h| assets.get(h).cloned()).unwrap();
        let bytes = write_brush(&f).unwrap();
        assert!(read_zip_checked(&bytes).unwrap().iter().any(|(n, d)| n == "LICENSE.txt" && d.starts_with(b"Uso livre")));
        let Package::Brush(r) = read(&bytes).unwrap() else { panic!() };
        assert_eq!(r.preset.license, p.license);
        // sem licença escolhida: a padrão do usuário
        let mut q = BrushPreset::round("Simples", 10.0, 0.5);
        q.license.clear();
        let g = BrushFile::from_preset(&q, None, &|_| None).unwrap();
        assert_eq!(g.preset.license, DEFAULT_LICENSE);
        let Package::Brush(r) = read(&write_brush(&g).unwrap()).unwrap() else { panic!() };
        assert_eq!(r.preset.license, DEFAULT_LICENSE);
        assert_eq!(r.manifest.license.spdx.as_deref(), Some(PROPRIETARY_SPDX));
    }

    #[test]
    fn pack_with_three_brushes_shares_assets() {
        let a = file_of("A", 1);
        let b = file_of("B", 2);
        let mut c = BrushFile::from_preset(&BrushPreset::round("C redonda", 12.0, 0.7), None, &|_| None).unwrap();
        c.preset.license = UNKNOWN_LICENSE.into();
        let mut a2 = a.clone();
        a2.preset.name = "A (cópia)".into();
        a2.preset.id = "outro-id".into(); // mesmas imagens que A: um arquivo só em assets/
        let pack = Manifest::new_pack("Meu pacote", BrushAuthor { name: "Ana".into(), ..Default::default() }, "CC0-1.0");
        let bytes = write_pack(&pack, &[a.clone(), b.clone(), c.clone(), a2]).unwrap();
        let names: Vec<String> = read_zip_checked(&bytes).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names[0], "mimetype");
        assert_eq!(names.iter().filter(|n| n.starts_with("assets/")).count(), 4, "A e A (cópia) dividem as 2 imagens; B tem as suas");
        let Package::Pack(p) = read(&bytes).unwrap() else { panic!("era um pacote") };
        assert_eq!(p.brushes.len(), 4);
        assert_eq!(p.manifest.license.spdx.as_deref(), Some("CC0-1.0"));
        assert_eq!(p.brushes[0].preset, a.preset);
        assert_eq!(p.brushes[1].preset, b.preset);
        assert_eq!(p.brushes[1].assets, b.assets);
        assert_eq!(p.brushes[2].preset.license, UNKNOWN_LICENSE, "cada pincel mantém a licença dele");
        assert_eq!(p.brushes[2].preset.author.name, "Ana", "sem autor, herda o do pacote");
        assert_eq!(p.brushes[3].preset.name, "A (cópia)");
    }

    /// Zip na mão para os casos ruins (a entrada `mimetype` primeiro e o manifesto dados).
    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let es: Vec<ZipEntry> = entries.iter().map(|(n, d)| ZipEntry { name: (*n).into(), data: d.to_vec(), deflate: false }).collect();
        write_zip(&es).unwrap()
    }

    #[test]
    fn corrupt_and_hostile_files_are_refused() {
        let f = file_of("Nuvem", 3);
        let good = write_brush(&f).unwrap();
        assert!(read(&good).is_ok());
        // lixo, cortado, zip sem mimetype
        assert!(read(b"nao e um zip de jeito nenhum, mas tem mais de vinte e dois bytes").is_err());
        assert!(read(&good[..good.len() / 2]).is_err());
        let e = read(&zip_of(&[("manifest.json", b"{}")])).unwrap_err();
        assert!(e.contains("mimetype"), "{e}");
        // byte alterado dentro de uma imagem: o CRC acusa
        let mut bad = good.clone();
        let pos = bad.windows(8).position(|w| w == [0x89, b'P', b'N', b'G', 13, 10, 26, 10]).unwrap();
        bad[pos + 40] ^= 0xff;
        let e = read(&bad).unwrap_err();
        assert!(e.contains("CRC") || e.contains("hash"), "{e}");
        // hash do manifesto não confere com o conteúdo (zip íntegro, manifesto adulterado)
        let mut es = read_zip_checked(&good).unwrap();
        for (n, d) in &mut es {
            if n == "manifest.json" {
                let mut m: Value = serde_json::from_slice(d).unwrap();
                m["assets"][0]["blake3"] = Value::from("0".repeat(64));
                *d = serde_json::to_vec(&m).unwrap();
            }
        }
        let rezip = |es: &[(String, Vec<u8>)]| write_zip(&es.iter().map(|(n, d)| ZipEntry { name: n.clone(), data: d.clone(), deflate: false }).collect::<Vec<_>>()).unwrap();
        let e = read(&rezip(&es)).unwrap_err();
        assert!(e.contains("BLAKE3"), "{e}");
        // arquivo presente que o manifesto não cita
        let mut es = read_zip_checked(&good).unwrap();
        es.push(("extra.bin".into(), vec![1, 2, 3]));
        let e = read(&rezip(&es)).unwrap_err();
        assert!(e.contains("não cita"), "{e}");
        // arquivo citado que falta
        let es: Vec<_> = read_zip_checked(&good).unwrap().into_iter().filter(|(n, _)| n != "assets/tip.png").collect();
        let e = read(&rezip(&es)).unwrap_err();
        assert!(e.contains("assets/tip.png"), "{e}");
        // sem licença
        let mut es = read_zip_checked(&good).unwrap();
        for (n, d) in &mut es {
            if n == "manifest.json" {
                let mut m: Value = serde_json::from_slice(d).unwrap();
                m.as_object_mut().unwrap().remove("license");
                *d = serde_json::to_vec(&m).unwrap();
            }
        }
        let e = read(&rezip(&es)).unwrap_err();
        assert!(e.contains("licença"), "{e}");
    }

    #[test]
    fn zip_slip_names_duplicates_and_bombs_are_refused() {
        let f = file_of("Nuvem", 4);
        let good = write_brush(&f).unwrap();
        let es = read_zip_checked(&good).unwrap();
        let with = |extra: (&str, &[u8])| {
            let mut v: Vec<ZipEntry> = es.iter().map(|(n, d)| ZipEntry { name: n.clone(), data: d.clone(), deflate: false }).collect();
            v.push(ZipEntry { name: extra.0.into(), data: extra.1.to_vec(), deflate: false });
            write_zip(&v).unwrap()
        };
        for n in ["../fora.txt", "assets/../../fora.txt", "/abs.txt", "C:/x.txt", "assets\\..\\x", "a//b", "./x"] {
            let e = read(&with((n, b"x"))).unwrap_err();
            assert!(e.contains("caminho inválido"), "{n}: {e}");
        }
        let e = read(&with(("MANIFEST.json", b"{}"))).unwrap_err();
        assert!(e.contains("repetido"), "{e}");
        let e = read(&with(("manifest.json", b"{}"))).unwrap_err();
        assert!(e.contains("repetido"), "{e}");
        // bomba: tamanho declarado menor que o real
        let mut z = zip_of(&[("mimetype", MIME_BRUSH.as_bytes())]);
        // troca o tamanho descomprimido (cabeçalho central) por um absurdo acima do limite
        let cd = z.windows(4).position(|w| w == [0x50, 0x4b, 1, 2]).unwrap();
        z[cd + 24..cd + 28].copy_from_slice(&(70u32 << 20).to_le_bytes());
        let e = read(&z).unwrap_err();
        assert!(e.contains("grande demais"), "{e}");
        // deflate que expande mais do que declarado
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(&vec![0u8; 1 << 20]).unwrap();
        let comp = enc.finish().unwrap();
        let z = zip_of(&[("mimetype", MIME_BRUSH.as_bytes())]);
        let (lh, cd) = (0usize, z.windows(4).position(|w| w == [0x50, 0x4b, 1, 2]).unwrap());
        // reescreve a entrada como deflate de 1 MB declarando só 100 bytes
        let name_len = 8;
        let mut z2 = Vec::new();
        z2.extend_from_slice(&z[lh..lh + 8]);
        z2.extend_from_slice(&8u16.to_le_bytes());
        z2.extend_from_slice(&z[lh + 10..lh + 14]);
        z2.extend_from_slice(&0u32.to_le_bytes());
        z2.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        z2.extend_from_slice(&100u32.to_le_bytes());
        z2.extend_from_slice(&(name_len as u16).to_le_bytes());
        z2.extend_from_slice(&0u16.to_le_bytes());
        z2.extend_from_slice(b"mimetype");
        z2.extend_from_slice(&comp);
        let cd_off = z2.len() as u32;
        let mut c = z[cd..cd + 46 + name_len].to_vec();
        c[10..12].copy_from_slice(&8u16.to_le_bytes());
        c[16..20].copy_from_slice(&0u32.to_le_bytes());
        c[20..24].copy_from_slice(&(comp.len() as u32).to_le_bytes());
        c[24..28].copy_from_slice(&100u32.to_le_bytes());
        c[42..46].copy_from_slice(&0u32.to_le_bytes());
        z2.extend_from_slice(&c);
        let end = z.len() - 22;
        let mut eocd = z[end..].to_vec();
        eocd[12..16].copy_from_slice(&(c.len() as u32).to_le_bytes());
        eocd[16..20].copy_from_slice(&cd_off.to_le_bytes());
        z2.extend_from_slice(&eocd);
        let e = read(&z2).unwrap_err();
        assert!(e.contains("não confere") || e.contains("CRC"), "{e}");
    }

    #[test]
    fn newer_min_reader_and_preset_schema_are_refused_with_a_clear_message() {
        let f = file_of("Futuro", 5);
        let good = write_brush(&f).unwrap();
        let edit = |key: &str, which: &str, val: Value| {
            let mut es = read_zip_checked(&good).unwrap();
            for (n, d) in &mut es {
                if n == which {
                    let mut m: Value = serde_json::from_slice(d).unwrap();
                    m[key] = val.clone();
                    *d = serde_json::to_vec(&m).unwrap();
                }
            }
            write_zip(&es.iter().map(|(n, d)| ZipEntry { name: n.clone(), data: d.clone(), deflate: false }).collect::<Vec<_>>()).unwrap()
        };
        let e = read(&edit("min_reader", "manifest.json", Value::from(2))).unwrap_err();
        assert!(e.contains("versão mais nova"), "{e}");
        // format_version maior mas min_reader 1: ainda abre (compatível)
        assert!(read(&edit("format_version", "manifest.json", Value::from(2))).is_ok());
        let e = read(&edit("preset_schema", "brush.json", Value::from(9))).unwrap_err();
        assert!(e.contains("mais nova"), "{e}");
        // o mimetype de outro tipo
        let e = read(&zip_of(&[("mimetype", b"application/zip")])).unwrap_err();
        assert!(e.contains("não é um pincel"), "{e}");
    }

    #[test]
    fn iso_time_and_versions() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_from_unix(1_782_000_000), "2026-06-21T00:00:00Z");
        assert_eq!(version_cmp("1.2.0", "1.10"), std::cmp::Ordering::Less);
        assert_eq!(version_cmp("2", "2.0.0"), std::cmp::Ordering::Equal);
        assert!(License::from_text("cc-by-4.0").spdx.is_some());
        assert_eq!(License::from_text("CC-BY-4.0").link().as_deref(), Some("https://creativecommons.org/licenses/by/4.0/"));
        assert!(License::from_text("MIT").link().unwrap().contains("spdx.org"));
    }
}
