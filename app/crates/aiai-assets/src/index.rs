//! Índice em memória da biblioteca: a pasta é a verdade e o índice é reconstruído a cada varredura (nada de cache em
//! disco que possa discordar da pasta). Cada recurso vira um [`Entry`] com o envelope dele; arquivo sem manifesto
//! (um `.glb` jogado na pasta) entra com o envelope inferido (`inferred`) e só ganha manifesto quando o usuário edita.
//! Arquivo que não deu para ler vai para `problems` (nome e motivo): nada some em silêncio.

use crate::config::kind_dir;
use crate::envelope::{Envelope, MAX_MANIFEST, SIDECAR_SUFFIX, fold};
use crate::kind::{IMAGE_EXTS, Kind, MODEL_EXTS, ext_of};
use crate::palette;
use aiai_paint::brush_pack::{self, Package};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Onde mora o envelope do recurso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Storage {
    /// Arquivo de terceiros com `<arquivo>.aiaiasset.json` ao lado (existente ou a criar na primeira edição).
    Sidecar(PathBuf),
    /// O próprio arquivo é o envelope (`.aiaipal`, `.aiaimat`, `.aiaipose`).
    Embedded,
    /// Dentro do zip do `.aiaibrush`.
    Brush,
}

/// Números para a tela (tudo opcional: só o que o tipo tem e o arquivo diz sem abrir o modelo).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stats {
    pub triangles: Option<u64>,
    pub vertices: Option<u64>,
    pub parts: Option<u64>,
    /// Tamanho do modelo (largura, altura, profundidade) em metros.
    pub size_m: Option<[f32; 3]>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub colors: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    /// Arquivo principal (absoluto).
    pub path: PathBuf,
    pub storage: Storage,
    pub env: Envelope,
    /// Sem manifesto no disco: o envelope foi inferido do nome e do arquivo.
    pub inferred: bool,
    pub bytes: u64,
    /// Última modificação do arquivo (segundos Unix).
    pub mtime: i64,
    /// Subpasta dentro da pasta do tipo (vazio = na raiz do tipo), com `/`.
    pub folder: String,
    pub stats: Stats,
}

impl Entry {
    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Arquivo do sidecar (existente ou futuro), se o recurso usa sidecar.
    pub fn sidecar_path(&self) -> Option<&Path> {
        match &self.storage {
            Storage::Sidecar(p) => Some(p),
            _ => None,
        }
    }

    /// Texto que a busca compara.
    fn haystack(&self) -> String {
        let e = &self.env;
        fold(&format!("{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}", e.name, e.tags.join(" "), e.author.name, e.license_text(), e.source_text(), e.description, e.category, self.file_name()))
    }
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Newest,
    Size,
    Kind,
}

impl Sort {
    pub fn key(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Newest => "newest",
            Sort::Size => "size",
            Sort::Kind => "kind",
        }
    }

    pub fn from_key(s: &str) -> Option<Sort> {
        [Sort::Name, Sort::Newest, Sort::Size, Sort::Kind].into_iter().find(|k| k.key() == s.trim().to_ascii_lowercase())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub kind: Option<Kind>,
    pub text: String,
    /// Todas as etiquetas pedidas têm que estar no recurso.
    pub tags: Vec<String>,
    pub sort: Sort,
}

#[derive(Debug, Clone, Default)]
pub struct Index {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub problems: Vec<Problem>,
}

/// Maior arquivo de pincel lido na varredura (o formato limita cada entrada a 64 MB).
const MAX_BRUSH_FILE: u64 = 64 << 20;
const MAX_DEPTH: usize = 6;

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if p.is_dir() {
            // pastas que começam com `.` ou `_` (_Originais, _Lixeira, .cache) não são varridas
            if depth < MAX_DEPTH && !name.starts_with(['.', '_']) {
                walk(&p, depth + 1, out);
            }
        } else {
            out.push(p);
        }
    }
}

/// Que tipo de recurso é este arquivo, dado o tipo da pasta onde está (`None` = não é recurso: sidecar, `.bin` do glTF,
/// `.mtl`...). Formatos próprios valem em qualquer pasta; imagem só em Texturas/Materiais; `.json` só em Poses.
pub fn classify(folder_kind: Kind, path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.ends_with(SIDECAR_SUFFIX) {
        return None;
    }
    match ext_of(path).as_str() {
        "aiaibrush" => Some(Kind::Brush),
        "aiaipal" | "gpl" | "ase" => Some(Kind::Palette),
        "aiaipose" => Some(Kind::Pose),
        "aiaimat" => Some(Kind::Material),
        e if MODEL_EXTS.contains(&e) => Some(Kind::Model3d),
        e if IMAGE_EXTS.contains(&e) => matches!(folder_kind, Kind::Texture | Kind::Material).then_some(folder_kind),
        "json" => (folder_kind == Kind::Pose).then_some(Kind::Pose),
        _ => None,
    }
}

/// Caminho do sidecar de um arquivo de terceiros: o nome inteiro + `.aiaiasset.json`.
pub fn sidecar_for(path: &Path) -> PathBuf {
    let mut n = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    n.push(SIDECAR_SUFFIX);
    path.with_file_name(n)
}

fn mtime_of(md: &std::fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Id estável de um arquivo sem manifesto: não muda entre varreduras e vira o `id` do sidecar na primeira edição.
fn inferred_id(kind: Kind, rel: &str) -> String {
    let h = blake3::hash(format!("{}|{}", kind.key(), rel.to_lowercase()).as_bytes()).to_hex();
    format!("f-{}", &h.as_str()[..20])
}

fn stats_of(env: &Envelope, path: &Path) -> Stats {
    let mut s = Stats::default();
    match env.kind {
        Kind::Model3d => {
            let m = env.payload.get("model3d").unwrap_or(&env.payload);
            s.triangles = m.get("triangles").and_then(|v| v.as_u64());
            s.vertices = m.get("vertices").and_then(|v| v.as_u64());
            s.parts = m.get("parts").and_then(|v| v.as_u64());
            if let Some(b) = m.get("bbox").and_then(|v| v.as_array()).filter(|b| b.len() == 2) {
                let f = |i: usize, k: usize| b[i].get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                s.size_m = Some([f(1, 0) - f(0, 0), f(1, 1) - f(0, 1), f(1, 2) - f(0, 2)]);
            }
        }
        Kind::Texture | Kind::Material => {
            s.width = env.payload.get("width").and_then(|v| v.as_u64()).map(|v| v as u32);
            s.height = env.payload.get("height").and_then(|v| v.as_u64()).map(|v| v as u32);
            if s.width.is_none() && IMAGE_EXTS.contains(&ext_of(path).as_str()) {
                if let Ok((w, h)) = image::image_dimensions(path) {
                    (s.width, s.height) = (Some(w), Some(h));
                }
            }
        }
        Kind::Palette => s.colors = env.payload.get("colors").and_then(|v| v.as_array()).map(|a| a.len() as u32),
        Kind::Pose | Kind::Brush => {}
    }
    s
}

/// Lê um recurso da pasta. `Err` = o arquivo não vale como recurso nenhum (vai para os problemas); problemas parciais
/// (sidecar ruim) entram em `notes` e o recurso continua listado com o envelope inferido.
fn load_entry(kind_folder: &Path, folder_kind: Kind, path: &Path, notes: &mut Vec<Problem>) -> Result<Option<Entry>, String> {
    let Some(kind) = classify(folder_kind, path) else { return Ok(None) };
    let md = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let (bytes, mtime) = (md.len(), mtime_of(&md));
    let rel = path.strip_prefix(kind_folder).unwrap_or(path);
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    let folder = rel.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = ext_of(path);

    let (env, storage, inferred) = match kind {
        Kind::Brush => {
            if bytes > MAX_BRUSH_FILE {
                return Err(format!("arquivo grande demais ({} MB)", bytes >> 20));
            }
            match brush_pack::read(&std::fs::read(path).map_err(|e| e.to_string())?)? {
                Package::Brush(b) => (Envelope::from_brush_manifest(&b.manifest), Storage::Brush, false),
                Package::Pack(_) => return Err("é um pacote (.aiaibrushes) com a extensão de um pincel".into()),
            }
        }
        Kind::Palette | Kind::Material | Kind::Pose if matches!(ext.as_str(), "aiaipal" | "aiaimat" | "aiaipose") => {
            if bytes > MAX_MANIFEST {
                return Err(format!("arquivo grande demais ({} KB)", bytes >> 10));
            }
            let data = std::fs::read(path).map_err(|e| e.to_string())?;
            match Envelope::from_json(&data) {
                Ok(e) => (e, Storage::Embedded, false),
                // pose escrita por outra ferramenta, sem envelope: entra como pose inferida em vez de sumir
                Err(why) if kind == Kind::Pose && why.contains("`format`") => {
                    let v: serde_json::Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
                    let mut e = Envelope::new(kind, &stem);
                    e.id = inferred_id(kind, &rel_s);
                    e.payload = v;
                    (e, Storage::Sidecar(sidecar_for(path)), true)
                }
                Err(why) => return Err(why),
            }
        }
        _ => {
            let side = sidecar_for(path);
            let from_disk = match std::fs::metadata(&side) {
                Ok(m) if m.len() > MAX_MANIFEST => {
                    notes.push(Problem { path: side.clone(), reason: "manifesto grande demais".into() });
                    None
                }
                Ok(_) => match std::fs::read(&side).map_err(|e| e.to_string()).and_then(|b| Envelope::from_json(&b)) {
                    Ok(e) => Some(e),
                    Err(why) => {
                        notes.push(Problem { path: side.clone(), reason: why });
                        None
                    }
                },
                Err(_) => None,
            };
            match from_disk {
                Some(e) => (e, Storage::Sidecar(side), false),
                None => {
                    let mut e = Envelope::new(kind, &stem);
                    e.id = inferred_id(kind, &rel_s);
                    e.created = brush_pack::iso_from_unix(mtime);
                    e.modified = e.created.clone();
                    e.source.original_file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    // paleta de texto solta (`.gpl`/`.ase`): lê as cores para as amostras e a contagem
                    if kind == Kind::Palette {
                        let parsed = match ext.as_str() {
                            "gpl" => std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| palette::parse_gpl(&t)),
                            _ => std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| palette::parse_ase(&b)),
                        };
                        match parsed {
                            Ok(p) => {
                                if !p.name.is_empty() {
                                    e.name = p.name.clone();
                                }
                                e.payload = p.to_payload();
                            }
                            Err(why) => return Err(why),
                        }
                    }
                    (e, Storage::Sidecar(side), true)
                }
            }
        }
    };
    // pose solta em JSON: o conteúdo vem sempre do arquivo (o sidecar só guarda os metadados)
    let mut env = env;
    if kind == Kind::Pose && ext == "json" && bytes <= MAX_MANIFEST {
        if let Ok(v) = std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string())) {
            env.payload = v;
        }
    }
    let stats = stats_of(&env, path);
    Ok(Some(Entry { id: env.id.clone(), kind: env.kind, path: path.to_path_buf(), storage, env, inferred, bytes, mtime, folder, stats }))
}

impl Index {
    /// Varre as pastas dos tipos sob `root` (pasta que não existe = tipo vazio).
    pub fn scan(root: &Path) -> Index {
        let mut idx = Index { root: root.to_path_buf(), ..Default::default() };
        let mut seen: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
        for folder_kind in Kind::ALL {
            let dir = kind_dir(root, folder_kind);
            let mut files = Vec::new();
            walk(&dir, 0, &mut files);
            for f in files {
                let mut notes = Vec::new();
                match load_entry(&dir, folder_kind, &f, &mut notes) {
                    Ok(Some(mut e)) => {
                        // `id` repetido (arquivo copiado com o manifesto): o segundo ganha um id só na memória
                        if let Some(first) = seen.get(&e.id) {
                            notes.push(Problem { path: f.clone(), reason: format!("id repetido com {}; edite um dos dois (Duplicar gera id novo)", first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) });
                            e.id = format!("{}~{}", e.id, seen.len());
                        }
                        seen.insert(e.id.clone(), f.clone());
                        idx.entries.push(e);
                    }
                    Ok(None) => {}
                    Err(reason) => idx.problems.push(Problem { path: f.clone(), reason }),
                }
                idx.problems.extend(notes);
            }
        }
        idx
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn count(&self, kind: Option<Kind>) -> usize {
        self.entries.iter().filter(|e| kind.is_none_or(|k| e.kind == k)).count()
    }

    /// Etiquetas em uso (do tipo, se dado) com a contagem; as mais usadas primeiro.
    pub fn tag_counts(&self, kind: Option<Kind>) -> Vec<(String, usize)> {
        let mut m: std::collections::BTreeMap<String, (String, usize)> = std::collections::BTreeMap::new();
        for e in self.entries.iter().filter(|e| kind.is_none_or(|k| e.kind == k)) {
            for t in &e.env.tags {
                m.entry(fold(t)).or_insert_with(|| (t.clone(), 0)).1 += 1;
            }
        }
        let mut v: Vec<(String, usize)> = m.into_values().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| fold(&a.0).cmp(&fold(&b.0))));
        v
    }

    /// Recursos que combinam com a consulta, ordenados.
    pub fn search(&self, q: &Query) -> Vec<&Entry> {
        let words: Vec<String> = fold(&q.text).split_whitespace().map(str::to_owned).collect();
        let tags: Vec<String> = q.tags.iter().map(|t| fold(t)).collect();
        let mut out: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| q.kind.is_none_or(|k| e.kind == k))
            .filter(|e| tags.iter().all(|t| e.env.tags.iter().any(|x| fold(x) == *t)))
            .filter(|e| {
                if words.is_empty() {
                    return true;
                }
                let h = e.haystack();
                words.iter().all(|w| h.contains(w.as_str()))
            })
            .collect();
        match q.sort {
            Sort::Name => out.sort_by_key(|e| (fold(&e.env.name), e.path.clone())),
            Sort::Newest => out.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| fold(&a.env.name).cmp(&fold(&b.env.name)))),
            Sort::Size => out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| fold(&a.env.name).cmp(&fold(&b.env.name)))),
            Sort::Kind => out.sort_by_key(|e| (e.kind, fold(&e.env.name))),
        }
        out
    }

    pub fn to_json(&self, e: &Entry) -> serde_json::Value {
        entry_json(e)
    }
}

/// Recurso como JSON para a bridge/MCP.
pub fn entry_json(e: &Entry) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "kind": e.kind.key(),
        "name": e.env.name,
        "path": e.path.display().to_string(),
        "folder": e.folder,
        "tags": e.env.tags,
        "author": e.env.author.name,
        "license": e.env.license_text(),
        "source": e.env.source_text(),
        "description": e.env.description,
        "version": e.env.version,
        "modified": e.env.modified,
        "bytes": e.bytes,
        "inferred": e.inferred,
        "stats": e.stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Kind;

    pub(crate) fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-assets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn classify_by_extension_and_folder() {
        let c = |k, f: &str| classify(k, Path::new(f));
        assert_eq!(c(Kind::Model3d, "a/b.GLB"), Some(Kind::Model3d));
        assert_eq!(c(Kind::Model3d, "a/b.bin"), None);
        assert_eq!(c(Kind::Model3d, "a/b.png"), None, "textura de um glTF não é recurso");
        assert_eq!(c(Kind::Texture, "a/b.png"), Some(Kind::Texture));
        assert_eq!(c(Kind::Material, "a/b.jpg"), Some(Kind::Material));
        assert_eq!(c(Kind::Palette, "a/b.gpl"), Some(Kind::Palette));
        assert_eq!(c(Kind::Pose, "a/b.json"), Some(Kind::Pose));
        assert_eq!(c(Kind::Texture, "a/b.json"), None);
        assert_eq!(c(Kind::Model3d, "a/b.glb.aiaiasset.json"), None);
        assert_eq!(sidecar_for(Path::new("a/b.glb")), Path::new("a/b.glb.aiaiasset.json"));
    }

    #[test]
    fn scan_reads_sidecars_infers_plain_files_and_reports_problems() {
        let root = tmp("scan");
        let models = root.join("Modelos 3D");
        std::fs::create_dir_all(models.join("Cenário")).unwrap();
        std::fs::create_dir_all(models.join("_Lixeira")).unwrap();
        // um .glb solto (sem manifesto)
        std::fs::write(models.join("solto.glb"), b"glTF").unwrap();
        // um com sidecar
        std::fs::write(models.join("Cenário").join("casa.glb"), b"glTF").unwrap();
        let mut e = Envelope::new(Kind::Model3d, "Casa de dois andares");
        e.tags = vec!["cenário".into(), "casa".into()];
        e.author.name = "Ana".into();
        e.set_license_text("CC0-1.0");
        e.payload = serde_json::json!({"model3d": {"triangles": 1234, "vertices": 700, "parts": 3, "bbox": [[0.0, 0.0, 0.0], [4.0, 6.0, 5.0]]}});
        std::fs::write(sidecar_for(&models.join("Cenário").join("casa.glb")), e.to_json().unwrap()).unwrap();
        // na lixeira: não aparece
        std::fs::write(models.join("_Lixeira").join("velho.glb"), b"glTF").unwrap();
        // sidecar corrompido: o arquivo continua listado e o problema aparece
        std::fs::write(models.join("quebrado.glb"), b"glTF").unwrap();
        std::fs::write(sidecar_for(&models.join("quebrado.glb")), b"{ nao e json").unwrap();
        // pose solta em JSON (de outra ferramenta): entra com o conteúdo no payload
        std::fs::create_dir_all(root.join("Poses")).unwrap();
        std::fs::write(root.join("Poses").join("andando.json"), r#"{"bones": {"braco": [0, 0, 30]}}"#).unwrap();
        // textura e paleta própria
        std::fs::create_dir_all(root.join("Texturas")).unwrap();
        image::RgbaImage::from_pixel(8, 4, image::Rgba([10, 20, 30, 255])).save(root.join("Texturas").join("papel.png")).unwrap();
        std::fs::create_dir_all(root.join("Paletas")).unwrap();
        std::fs::write(root.join("Paletas").join("solta.gpl"), "GIMP Palette\nName: Solta\n255 0 0 Vermelho\n0 0 255 Azul\n").unwrap();
        // arquivo ilegível numa pasta de formato próprio
        std::fs::write(root.join("Paletas").join("ruim.aiaipal"), b"[]").unwrap();

        let idx = Index::scan(&root);
        let names: Vec<_> = idx.entries.iter().map(|e| (e.kind, e.env.name.as_str(), e.inferred)).collect();
        assert!(names.contains(&(Kind::Model3d, "solto", true)), "{names:?}");
        assert!(names.contains(&(Kind::Model3d, "Casa de dois andares", false)));
        assert!(names.contains(&(Kind::Model3d, "quebrado", true)));
        assert!(names.contains(&(Kind::Texture, "papel", true)));
        assert!(names.contains(&(Kind::Palette, "Solta", true)));
        assert_eq!(idx.entries.iter().find(|e| e.kind == Kind::Pose).unwrap().env.payload["bones"]["braco"][2], 30);
        assert!(!names.iter().any(|n| n.1 == "velho"), "a lixeira não é varrida");
        let casa = idx.entries.iter().find(|e| e.env.name.starts_with("Casa")).unwrap();
        assert_eq!(casa.folder, "Cenário");
        assert_eq!(casa.stats.triangles, Some(1234));
        assert_eq!(casa.stats.size_m, Some([4.0, 6.0, 5.0]));
        assert_eq!(casa.env.license.spdx.as_deref(), Some("CC0-1.0"));
        let papel = idx.entries.iter().find(|e| e.env.name == "papel").unwrap();
        assert_eq!((papel.stats.width, papel.stats.height), (Some(8), Some(4)));
        assert_eq!(idx.entries.iter().find(|e| e.env.name == "Solta").unwrap().stats.colors, Some(2));
        // problemas: o sidecar corrompido e a paleta ilegível, com o motivo
        let probs: Vec<_> = idx.problems.iter().map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert!(probs.contains(&"quebrado.glb.aiaiasset.json".to_owned()), "{probs:?}");
        assert!(probs.contains(&"ruim.aiaipal".to_owned()), "{probs:?}");
        // o id inferido é estável entre varreduras
        let again = Index::scan(&root);
        assert_eq!(idx.entries.iter().find(|e| e.env.name == "solto").unwrap().id, again.entries.iter().find(|e| e.env.name == "solto").unwrap().id);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_sort_and_tags() {
        let root = tmp("search");
        let dir = root.join("Texturas");
        std::fs::create_dir_all(&dir).unwrap();
        for (n, tags, lic) in [("Papel áspero", "papel, grão", "CC0-1.0"), ("Grão de filme", "grão", "MIT"), ("Tramas", "tom, manga", "CC-BY-4.0")] {
            let p = dir.join(format!("{n}.png"));
            image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])).save(&p).unwrap();
            let mut e = Envelope::new(Kind::Texture, n);
            e.tags = crate::envelope::parse_tags(tags);
            e.set_license_text(lic);
            std::fs::write(sidecar_for(&p), e.to_json().unwrap()).unwrap();
        }
        let idx = Index::scan(&root);
        let names = |q: &Query| idx.search(q).iter().map(|e| e.env.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&Query::default()), vec!["Grão de filme", "Papel áspero", "Tramas"]);
        assert_eq!(names(&Query { text: "grao".into(), ..Default::default() }), vec!["Grão de filme", "Papel áspero"]);
        assert_eq!(names(&Query { text: "papel aspero".into(), ..Default::default() }), vec!["Papel áspero"]);
        assert_eq!(names(&Query { tags: vec!["GRAO".into()], ..Default::default() }), vec!["Grão de filme", "Papel áspero"]);
        assert_eq!(names(&Query { text: "cc-by".into(), ..Default::default() }), vec!["Tramas"]);
        assert_eq!(names(&Query { kind: Some(Kind::Model3d), ..Default::default() }), Vec::<String>::new());
        let tags = idx.tag_counts(None);
        assert_eq!(tags[0], ("grão".to_owned(), 2));
        assert_eq!(idx.count(Some(Kind::Texture)), 3);
        let _ = std::fs::remove_dir_all(&root);
    }
}
