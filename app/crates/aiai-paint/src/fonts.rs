//! Catálogo de fontes para texto: as embutidas (Noto Sans JP, OFL), as instaladas no Windows (`%WINDIR%\Fonts`) e as
//! do usuário (`Documentos\AnimeIAI\Fontes`). Só o cabeçalho de cada arquivo é lido na varredura (tabelas `name`,
//! `OS/2` e `head`: família, estilo, peso e itálico), então listar centenas de fontes custa pouco; os bytes inteiros
//! só são lidos (e guardados em cache) quando um texto de fato usa a fonte.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Família usada quando o texto não pede nenhuma (embutida: o render nunca depende de fonte instalada).
pub const DEFAULT_FAMILY: &str = "Noto Sans JP";

/// Noto Sans JP Regular (OFL) embutida no app.
pub fn default_font() -> &'static [u8] {
    include_bytes!("../../aiai-studio/assets/fonts/NotoSansJP-Regular.otf")
}

/// Noto Sans JP Black (OFL): onomatopeias, títulos e o "negrito" da família embutida.
pub fn heavy_font() -> &'static [u8] {
    include_bytes!("../../aiai-studio/assets/fonts/NotoSansJP-Black.otf")
}

/// Onde os bytes de uma face moram.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FaceSource {
    Embedded { heavy: bool },
    File { path: PathBuf, index: u32 },
}

/// Uma face (família + estilo) do catálogo.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceInfo {
    pub family: String,
    pub style: String,
    pub bold: bool,
    pub italic: bool,
    /// Peso CSS (100..900) do OS/2; 400 quando a fonte não diz.
    pub weight: u16,
    /// Veio da pasta do usuário (não do sistema).
    pub user: bool,
    pub source: FaceSource,
}

/// Fonte já carregada para o layout: bytes inteiros, o índice dentro da coleção e o que faltou da pedida (negrito/itálico
/// sintéticos quando a família não tem a face).
#[derive(Debug, Clone)]
pub struct LoadedFont {
    pub data: FontData,
    pub synth_bold: bool,
    pub synth_italic: bool,
    pub family: String,
}

#[derive(Debug, Clone)]
pub struct FontData {
    pub bytes: Arc<Vec<u8>>,
    pub index: u32,
}

impl FontData {
    pub fn font(&self) -> Option<ab_glyph::FontRef<'_>> {
        ab_glyph::FontRef::try_from_slice_and_index(&self.bytes, self.index).ok()
    }
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub faces: Vec<FaceInfo>,
}

impl Catalog {
    /// Famílias em ordem alfabética (sem repetir; sem diferenciar maiúsculas).
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for f in &self.faces {
            if !v.iter().any(|x| x.eq_ignore_ascii_case(&f.family)) {
                v.push(f.family.clone());
            }
        }
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    /// Faces de uma família.
    pub fn family_faces(&self, family: &str) -> Vec<&FaceInfo> {
        self.faces.iter().filter(|f| f.family.eq_ignore_ascii_case(family)).collect()
    }

    /// A face que melhor serve ao pedido e o que ficou faltando (negrito, itálico) para o render sintetizar.
    pub fn pick(&self, family: &str, bold: bool, italic: bool) -> Option<(&FaceInfo, bool, bool)> {
        let faces = self.family_faces(family);
        let best = faces.into_iter().min_by_key(|f| {
            // estilo certo primeiro; entre empates, a mais "normal" (peso perto de 400, ou de 700 se pediu negrito)
            let target = if bold { 700i32 } else { 400 };
            let miss = (f.bold != bold) as i32 * 1000 + (f.italic != italic) as i32 * 100;
            miss + (f.weight as i32 - target).abs() / 50 + f.user as i32
        })?;
        Some((best, bold && !best.bold, italic && !best.italic))
    }
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(o)?, *b.get(o + 1)?]))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

fn read_at(f: &mut std::fs::File, off: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut v = vec![0u8; len];
    let mut got = 0;
    while got < len {
        let n = f.read(&mut v[got..]).ok()?;
        if n == 0 {
            break;
        }
        got += n;
    }
    v.truncate(got);
    (got > 0).then_some(v)
}

/// Decodifica uma string do `name`: plataforma 3 (UTF-16 BE) ou 1/0 (Mac Roman ≈ Latin-1 para nomes de fonte).
fn decode_name(data: &[u8], platform: u16) -> String {
    if platform == 3 || platform == 0 {
        let u: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else {
        data.iter().map(|&b| b as char).collect()
    }
}

/// Lê (família, estilo, peso, itálico) da fonte `index` do arquivo, só pelas tabelas pequenas.
fn read_face_header(file: &mut std::fs::File, index: u32) -> Option<(String, String, u16, bool, bool)> {
    let head = read_at(file, 0, 12)?;
    let tag = &head[0..4];
    let sfnt_off: u64 = if tag == b"ttcf" {
        let n = be32(&head, 8)?;
        if index >= n {
            return None;
        }
        let offs = read_at(file, 12 + 4 * index as u64, 4)?;
        be32(&offs, 0)? as u64
    } else if tag == [0, 1, 0, 0] || tag == b"OTTO" || tag == b"true" {
        0
    } else {
        return None;
    };
    let hdr = read_at(file, sfnt_off, 12)?;
    let ntab = be16(&hdr, 4)? as usize;
    let dir = read_at(file, sfnt_off + 12, ntab * 16)?;
    let table = |t: &[u8; 4]| -> Option<(u64, usize)> {
        for i in 0..ntab {
            let r = i * 16;
            if dir.get(r..r + 4)? == t {
                return Some((be32(&dir, r + 8)? as u64, be32(&dir, r + 12)? as usize));
            }
        }
        None
    };
    let (noff, nlen) = table(b"name")?;
    let name = read_at(file, noff, nlen.min(64 * 1024))?;
    let count = be16(&name, 2)? as usize;
    let str_off = be16(&name, 4)? as usize;
    // melhor nome por id: inglês dos EUA (0x409) na plataforma 3 vale mais que qualquer outro
    let mut best: HashMap<u16, (i32, String)> = HashMap::new();
    for i in 0..count {
        let r = 6 + i * 12;
        let (plat, lang, nid, len, off) = (be16(&name, r)?, be16(&name, r + 4)?, be16(&name, r + 6)?, be16(&name, r + 8)? as usize, be16(&name, r + 10)? as usize);
        if !matches!(nid, 1 | 2 | 16 | 17) {
            continue;
        }
        let Some(raw) = name.get(str_off + off..str_off + off + len) else { continue };
        let score = match (plat, lang) {
            (3, 0x409) => 3,
            (3, _) => 2,
            (1, 0) => 1,
            _ => 0,
        };
        let s = decode_name(raw, plat);
        if s.trim().is_empty() {
            continue;
        }
        if best.get(&nid).is_none_or(|(sc, _)| score > *sc) {
            best.insert(nid, (score, s));
        }
    }
    let family = best.get(&1).map(|(_, s)| s.clone()).or_else(|| best.get(&16).map(|(_, s)| s.clone()))?;
    let style = best.get(&2).map(|(_, s)| s.clone()).unwrap_or_default();
    let (mut weight, mut italic, mut bold) = (400u16, false, false);
    if let Some((o, l)) = table(b"OS/2") {
        if let Some(os2) = read_at(file, o, l.min(80)) {
            weight = be16(&os2, 4).unwrap_or(400).clamp(100, 900);
            if let Some(sel) = be16(&os2, 62) {
                italic = sel & 1 != 0;
                bold = sel & 32 != 0;
            }
        }
    }
    if let Some((o, l)) = table(b"head") {
        if let Some(h) = read_at(file, o, l.min(54)) {
            if let Some(mac) = be16(&h, 44) {
                bold |= mac & 1 != 0;
                italic |= mac & 2 != 0;
            }
        }
    }
    let st = style.to_lowercase();
    italic |= st.contains("italic") || st.contains("oblique");
    bold |= st.contains("bold") || weight >= 600;
    Some((family, style, weight, bold, italic))
}

fn scan_dir(dir: &Path, user: bool, out: &mut Vec<FaceInfo>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();
    for p in files {
        if p.is_dir() {
            if user {
                scan_dir(&p, user, out);
            }
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        if !matches!(ext.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
            continue;
        }
        let Ok(mut f) = std::fs::File::open(&p) else { continue };
        let count = if matches!(ext.as_deref(), Some("ttc" | "otc")) { read_at(&mut f, 8, 4).and_then(|b| be32(&b, 0)).unwrap_or(1).min(32) } else { 1 };
        for index in 0..count {
            if let Some((family, style, weight, bold, italic)) = read_face_header(&mut f, index) {
                // fontes de símbolos/ícones não servem para texto corrido, mas o usuário pode querer: ficam na lista
                out.push(FaceInfo { family, style, bold, italic, weight, user, source: FaceSource::File { path: p.clone(), index } });
            }
        }
    }
}

/// Pasta de fontes do usuário: `Documentos\AnimeIAI\Fontes` (a pasta Documentos pode estar no OneDrive e ter o nome em
/// português ou em inglês, então testa os lugares usuais e fica com o primeiro que tem `AnimeIAI\Fontes`).
/// `AIAI_FONTS_DIR` substitui (testes).
pub fn user_fonts_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("AIAI_FONTS_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial", "USERPROFILE"] {
        if let Some(b) = std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from) {
            for sub in ["Documentos", "Documents"] {
                bases.push(b.join(sub));
            }
        }
    }
    let cands: Vec<PathBuf> = bases.into_iter().map(|b| b.join("AnimeIAI").join("Fontes")).collect();
    cands.iter().find(|p| p.is_dir()).cloned().or_else(|| cands.into_iter().find(|p| p.parent().and_then(|a| a.parent()).is_some_and(|d| d.is_dir())))
}

fn windows_fonts_dir() -> Option<PathBuf> {
    let w = std::env::var_os("WINDIR").or_else(|| std::env::var_os("SystemRoot")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    let d = w.join("Fonts");
    d.is_dir().then_some(d)
}

fn embedded_faces(out: &mut Vec<FaceInfo>) {
    out.push(FaceInfo { family: DEFAULT_FAMILY.into(), style: "Regular".into(), bold: false, italic: false, weight: 400, user: false, source: FaceSource::Embedded { heavy: false } });
    out.push(FaceInfo { family: DEFAULT_FAMILY.into(), style: "Black".into(), bold: true, italic: false, weight: 900, user: false, source: FaceSource::Embedded { heavy: true } });
}

fn build_catalog() -> Catalog {
    let mut faces = Vec::new();
    embedded_faces(&mut faces);
    if let Some(d) = user_fonts_dir() {
        scan_dir(&d, true, &mut faces);
    }
    if let Some(d) = windows_fonts_dir() {
        scan_dir(&d, false, &mut faces);
    }
    Catalog { faces }
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Catálogo completo (varre as pastas na primeira chamada; bloqueia até terminar).
pub fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(build_catalog)
}

/// O catálogo, se a varredura já terminou (a interface não espera por ela).
pub fn catalog_if_ready() -> Option<&'static Catalog> {
    CATALOG.get()
}

/// Começa a varredura em segundo plano (chamar cedo: ao escolher a ferramenta Texto ou abrir o painel).
pub fn warm_up() {
    if CATALOG.get().is_none() {
        let _ = std::thread::Builder::new().name("aiai-fonts".into()).spawn(|| {
            let _ = catalog();
        });
    }
}

fn file_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<u8>>>> {
    static C: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn read_cached(path: &Path) -> Option<Arc<Vec<u8>>> {
    let mut c = file_cache().lock().ok()?;
    if let Some(b) = c.get(path) {
        return Some(Arc::clone(b));
    }
    let bytes = Arc::new(std::fs::read(path).ok()?);
    if c.len() >= 8 {
        c.clear();
    }
    c.insert(path.to_path_buf(), Arc::clone(&bytes));
    Some(bytes)
}

fn embedded_bytes(heavy: bool) -> FontData {
    static REG: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    static HEAVY: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    let cell = if heavy { &HEAVY } else { &REG };
    let bytes = cell.get_or_init(|| Arc::new(if heavy { heavy_font() } else { default_font() }.to_vec()));
    FontData { bytes: Arc::clone(bytes), index: 0 }
}

/// A fonte embutida padrão (regular), sempre disponível.
pub fn embedded(heavy: bool) -> LoadedFont {
    LoadedFont { data: embedded_bytes(heavy), synth_bold: false, synth_italic: false, family: DEFAULT_FAMILY.into() }
}

/// Carrega a face pedida. Família vazia ou igual à embutida não varre o disco. Família desconhecida cai na embutida
/// (o texto nunca some por falta de fonte; o chamador pode comparar `family` para avisar).
pub fn load(family: &str, bold: bool, italic: bool) -> LoadedFont {
    let fam = family.trim();
    let builtin = fam.is_empty() || fam.eq_ignore_ascii_case(DEFAULT_FAMILY);
    if !builtin {
        if let Some((face, sb, si)) = catalog().pick(fam, bold, italic) {
            let data = match &face.source {
                FaceSource::Embedded { heavy } => Some(embedded_bytes(*heavy)),
                FaceSource::File { path, index } => read_cached(path).map(|bytes| FontData { bytes, index: *index }),
            };
            if let Some(data) = data {
                return LoadedFont { data, synth_bold: sb, synth_italic: si, family: face.family.clone() };
            }
        }
    }
    let heavy = bold && builtin;
    let mut f = embedded(heavy);
    // fonte embutida sem a face pedida: negrito vira Black; itálico é sintético
    f.synth_italic = italic;
    if !builtin {
        f.family = DEFAULT_FAMILY.into();
        f.synth_bold = bold;
    }
    f
}

/// Carrega um arquivo de fonte qualquer (caminho absoluto), para o MCP e testes.
pub fn load_file(path: &Path) -> Result<LoadedFont, String> {
    let bytes = read_cached(path).ok_or_else(|| format!("fonte {}: não foi possível ler", path.display()))?;
    let data = FontData { bytes, index: 0 };
    data.font().ok_or_else(|| format!("fonte inválida: {}", path.display()))?;
    Ok(LoadedFont { data, synth_bold: false, synth_italic: false, family: path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_family_needs_no_disk_scan() {
        let f = load("", false, false);
        assert!(f.data.font().is_some());
        let b = load("Noto Sans JP", true, false);
        assert!(b.data.bytes.len() > f.data.bytes.len() / 2 && !b.synth_bold, "negrito = Black embutida, não sintético");
    }

    #[test]
    fn pick_prefers_exact_style_and_flags_what_is_missing() {
        let mk = |style: &str, bold, italic, weight| FaceInfo { family: "Foo".into(), style: style.into(), bold, italic, weight, user: false, source: FaceSource::Embedded { heavy: false } };
        let c = Catalog { faces: vec![mk("Regular", false, false, 400), mk("Bold", true, false, 700)] };
        let (f, sb, si) = c.pick("foo", true, false).unwrap();
        assert!(f.bold && !sb && !si);
        let (f, sb, si) = c.pick("Foo", true, true).unwrap();
        assert!(f.bold && !sb && si, "itálico falta: sintético");
        let (f, sb, _) = c.pick("Foo", false, false).unwrap();
        assert!(!f.bold && !sb);
        assert!(c.pick("Bar", false, false).is_none());
    }

    #[test]
    fn windows_catalog_lists_families_when_present() {
        // só roda onde há fontes do sistema (Windows); a varredura precisa achar ao menos uma família além da embutida
        if windows_fonts_dir().is_some() {
            let fams = catalog().families();
            assert!(fams.iter().any(|f| f == DEFAULT_FAMILY));
            assert!(fams.len() > 3, "famílias: {fams:?}");
        }
    }
}
