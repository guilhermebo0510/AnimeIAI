//! Importação de arquivos para a biblioteca: modelos 3D (`.glb`/`.gltf`/`.obj`), imagens (textura ou material),
//! paletas (`.gpl`, `.ase`, `.aiaipal`, JSON), poses e pincéis nativos (`.aiaibrush`, `.aiaibrushes`). Cada arquivo vira um
//! recurso novo na pasta do tipo, com manifesto (licença e fonte sempre preenchidas: o que o usuário não disser vira
//! "desconhecida"). Nada é sobrescrito: nome repetido ganha " (2)".
//!
//! Pincéis de outros programas (`.abr`, `.brushset`...) e o diálogo de aceite de licença ficam no painel Pincéis do app
//! (`brush_import`); aqui entram só os formatos do próprio AnimeIAI.

use crate::config::{bump, kind_dir};
use crate::envelope::{Envelope, FileRef, UNKNOWN_LICENSE, parse_tags};
use crate::index::{Index, Storage, sidecar_for};
use crate::kind::{IMAGE_EXTS, Kind, MODEL_EXTS, ext_of};
use crate::palette::{self, Palette};
use crate::store::{sanitize, unique_path, write_atomic, write_envelope};
use aiai_paint::brush_pack::{self, Package};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Maior imagem importada (pixels): 16384 x 16384 é mais que qualquer papel de anime; acima disso é erro.
const MAX_PIXELS: u64 = 16384 * 16384;
const MAX_IMAGE_FILE: u64 = 256 << 20;
const MAX_TEXT_FILE: u64 = 16 << 20;

/// O que o usuário informa ao importar (campo vazio = preenche com o que o arquivo diz, ou "desconhecida").
#[derive(Debug, Clone, Default)]
pub struct ImportOpts {
    pub author: String,
    pub license: String,
    pub source: String,
    pub tags: Vec<String>,
    /// Imagem como `Material` em vez de `Texture` (o padrão).
    pub as_kind: Option<Kind>,
    /// Nome do recurso (padrão: o do arquivo).
    pub name: Option<String>,
    /// Subpasta dentro da pasta do tipo.
    pub folder: String,
}

#[derive(Debug, Clone)]
pub struct Imported {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub path: PathBuf,
    pub warnings: Vec<String>,
}

/// Tipo provável de um arquivo pela extensão (imagem = textura: o usuário pode trocar para material).
pub fn kind_of_file(path: &Path) -> Option<Kind> {
    match ext_of(path).as_str() {
        "aiaibrush" | "aiaibrushes" => Some(Kind::Brush),
        "gpl" | "ase" | "aiaipal" => Some(Kind::Palette),
        "aiaipose" => Some(Kind::Pose),
        "aiaimat" => Some(Kind::Material),
        "vrm" => Some(Kind::Model3d),
        e if MODEL_EXTS.contains(&e) => Some(Kind::Model3d),
        e if IMAGE_EXTS.contains(&e) => Some(Kind::Texture),
        "json" => Some(Kind::Palette),
        _ => None,
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "recurso".into())
}

/// Pasta de destino: a do tipo (+ subpasta pedida).
fn dest_dir(root: &Path, kind: Kind, o: &ImportOpts) -> PathBuf {
    let mut d = kind_dir(root, kind);
    for part in o.folder.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty() && *p != "." && *p != "..") {
        let s = sanitize(part);
        if !s.starts_with(['.', '_']) {
            d = d.join(s);
        }
    }
    d
}

/// Preenche o envelope com o que o usuário informou; o resto vem do arquivo, e licença sem resposta vira "desconhecida".
fn fill(env: &mut Envelope, o: &ImportOpts, file_license: &str, file_author: &str, file_source: &str, src_name: &str) {
    if !o.author.trim().is_empty() {
        env.author.name = o.author.trim().to_owned();
    } else if env.author.name.is_empty() {
        env.author.name = file_author.trim().to_owned();
    }
    if !o.license.trim().is_empty() {
        env.set_license_text(&o.license);
    } else if !env.license.is_set() {
        env.set_license_text(if file_license.trim().is_empty() { UNKNOWN_LICENSE } else { file_license });
    }
    if !o.source.trim().is_empty() {
        env.set_source_text(&o.source);
    } else if env.source.text().is_empty() {
        env.set_source_text(if file_source.trim().is_empty() { src_name } else { file_source });
    }
    if !o.tags.is_empty() {
        for t in &o.tags {
            if !env.tags.iter().any(|x| crate::envelope::fold(x) == crate::envelope::fold(t)) {
                env.tags.push(t.clone());
            }
        }
    }
    if env.source.imported_from.is_empty() {
        env.source.imported_from = ext_of(Path::new(src_name));
    }
}

fn file_ref(name: &str, role: &str, bytes: &[u8]) -> FileRef {
    FileRef { path: name.to_owned(), role: role.to_owned(), blake3: blake3::hash(bytes).to_hex().to_string(), bytes: bytes.len() as u64 }
}

/// Importa um arquivo. Devolve os recursos criados (um `.aiaibrushes` cria vários). Erro = nada foi gravado.
pub fn import_file(root: &Path, src: &Path, o: &ImportOpts) -> Result<Vec<Imported>, String> {
    let name = file_name(src);
    let ext = ext_of(src);
    let r = match ext.as_str() {
        e if MODEL_EXTS.contains(&e) || e == "vrm" => import_model(root, src, o).map(|x| vec![x]),
        e if IMAGE_EXTS.contains(&e) => import_image(root, src, o).map(|x| vec![x]),
        "gpl" | "ase" | "aiaipal" => import_palette(root, src, o).map(|x| vec![x]),
        "json" => match json_kind(src, o) {
            Kind::Pose => import_pose(root, src, o).map(|x| vec![x]),
            _ => import_palette(root, src, o).map(|x| vec![x]),
        },
        "aiaipose" => import_pose(root, src, o).map(|x| vec![x]),
        "aiaimat" => import_embedded(root, src, Kind::Material, o).map(|x| vec![x]),
        "aiaibrush" | "aiaibrushes" => import_brushes(root, src, o),
        other => Err(format!(".{other} não é um formato de recurso da biblioteca (modelo .glb/.gltf/.obj, imagem, paleta .gpl/.ase/.aiaipal, pose, pincel .aiaibrush)")),
    };
    r.map_err(|e| format!("{name}: {e}"))
}

/// `.json` solto: paleta ou pose? `as_kind` decide; senão o envelope (`kind`), e sem envelope a forma: lista de cores ou
/// objeto com `colors` é paleta, o resto é pose.
fn json_kind(src: &Path, o: &ImportOpts) -> Kind {
    if let Some(k @ (Kind::Palette | Kind::Pose)) = o.as_kind {
        return k;
    }
    let v: Option<Value> = std::fs::read(src).ok().and_then(|b| serde_json::from_slice(&b).ok());
    match v {
        Some(Value::Array(_)) => Kind::Palette,
        Some(v) if v.get("format").is_some() => {
            if v.get("kind").and_then(Value::as_str) == Some("pose") { Kind::Pose } else { Kind::Palette }
        }
        Some(v) if v.get("colors").is_some() => Kind::Palette,
        Some(Value::Object(_)) => Kind::Pose,
        _ => Kind::Palette,
    }
}

fn import_model(root: &Path, src: &Path, o: &ImportOpts) -> Result<Imported, String> {
    // o importador valida (limites, formato) e recusa o .vrm com "em breve"
    let model = aiai_3d_model::import_path(src).map_err(|e| e.0)?;
    let info = model.info.clone();
    let nm = o.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| stem(src));
    let dir = dest_dir(root, Kind::Model3d, o);
    let dest = unique_path(&dir, &nm, "glb");
    let ext = ext_of(src);
    // .glb entra como veio (texturas, pele e animação do arquivo ficam para quem ler depois); .gltf/.obj viram o GLB
    // normalizado do app e o original vai para `_Originais`
    let bytes = if ext == "glb" {
        std::fs::read(src).map_err(|e| e.to_string())?
    } else {
        let orig = dir.join("_Originais");
        std::fs::create_dir_all(&orig).map_err(|e| format!("{}: {e}", orig.display()))?;
        let to = unique_path(&orig, &stem(src), &ext);
        std::fs::copy(src, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        aiai_3d_model::glb::write_glb(&model)
    };
    write_atomic(&dest, &bytes)?;
    let mut env = Envelope::new(Kind::Model3d, &nm);
    env.payload = json!({ "model3d": info });
    env.files = vec![file_ref(&file_name(&dest), "model", &bytes)];
    env.source.original_file = file_name(src);
    env.license.attribution = Some(info.license.copyright.clone()).filter(|c| !c.is_empty());
    fill(&mut env, o, &info.license.license, &info.license.author, &info.license.source, &file_name(src));
    let side = sidecar_for(&dest);
    if let Err(e) = write_envelope(&Storage::Sidecar(side), &dest, &env) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(Imported { id: env.id, kind: Kind::Model3d, name: nm, path: dest, warnings: info.warnings })
}

fn import_image(root: &Path, src: &Path, o: &ImportOpts) -> Result<Imported, String> {
    let kind = match o.as_kind {
        Some(k @ (Kind::Texture | Kind::Material)) => k,
        Some(other) => return Err(format!("imagem não pode ser '{}' (use texture ou material)", other.key())),
        None => Kind::Texture,
    };
    let len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > MAX_IMAGE_FILE {
        return Err(format!("arquivo de {} MB acima do limite de {} MB", len >> 20, MAX_IMAGE_FILE >> 20));
    }
    let (w, h) = image::image_dimensions(src).map_err(|e| format!("imagem ilegível: {e}"))?;
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(format!("imagem de {w}x{h} acima do limite de {MAX_PIXELS} pixels"));
    }
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    // decodifica de verdade: arquivo truncado/corrompido é erro agora, não na hora de usar
    image::load_from_memory(&bytes).map_err(|e| format!("imagem corrompida: {e}"))?;
    let nm = o.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| stem(src));
    let dest = unique_path(&dest_dir(root, kind, o), &nm, &ext_of(src));
    write_atomic(&dest, &bytes)?;
    let mut env = Envelope::new(kind, &nm);
    env.payload = json!({ "width": w, "height": h });
    env.files = vec![file_ref(&file_name(&dest), "image", &bytes)];
    env.source.original_file = file_name(src);
    fill(&mut env, o, "", "", "", &file_name(src));
    if let Err(e) = write_envelope(&Storage::Sidecar(sidecar_for(&dest)), &dest, &env) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(Imported { id: env.id, kind, name: nm, path: dest, warnings: Vec::new() })
}

/// Grava uma paleta como `.aiaipal` na pasta Paletas. Usado pela importação e por "Salvar na biblioteca".
pub fn add_palette(root: &Path, p: &Palette, o: &ImportOpts, src_name: &str) -> Result<Imported, String> {
    if p.colors.is_empty() && p.gradients.is_empty() {
        return Err("a paleta não tem cores".into());
    }
    let nm = o.name.clone().filter(|n| !n.trim().is_empty()).or_else(|| Some(p.name.clone()).filter(|n| !n.trim().is_empty())).unwrap_or_else(|| "Paleta".into());
    let mut env = Envelope::new(Kind::Palette, &nm);
    env.payload = p.to_payload();
    fill(&mut env, o, "", "", "", src_name);
    write_new_embedded(root, &env, o)
}

fn write_new_embedded(root: &Path, env: &Envelope, o: &ImportOpts) -> Result<Imported, String> {
    let ext = env.kind.own_ext().ok_or("tipo sem formato próprio")?;
    let dest = unique_path(&dest_dir(root, env.kind, o), &env.name, ext);
    write_envelope(&Storage::Embedded, &dest, env)?;
    Ok(Imported { id: env.id.clone(), kind: env.kind, name: env.name.clone(), path: dest, warnings: Vec::new() })
}

fn import_palette(root: &Path, src: &Path, o: &ImportOpts) -> Result<Imported, String> {
    let len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > MAX_TEXT_FILE {
        return Err(format!("arquivo de {} MB acima do limite de {} MB", len >> 20, MAX_TEXT_FILE >> 20));
    }
    let ext = ext_of(src);
    let mut pal = match ext.as_str() {
        "gpl" => palette::parse_gpl(&std::fs::read_to_string(src).map_err(|e| e.to_string())?)?,
        "ase" => palette::parse_ase(&std::fs::read(src).map_err(|e| e.to_string())?)?,
        _ => {
            let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("JSON inválido: {e}"))?;
            if v.get("format").and_then(Value::as_str).is_some() {
                // o JSON do próprio app: mantém o manifesto (autor, licença, etiquetas); id novo se já existe na biblioteca
                let mut env = Envelope::from_value(v)?;
                if env.kind != Kind::Palette {
                    return Err(format!("o arquivo é um recurso '{}', não uma paleta", env.kind.key()));
                }
                let p = Palette::from_payload(&env.payload)?;
                if p.colors.is_empty() {
                    return Err("a paleta não tem cores".into());
                }
                if Index::scan(root).get(&env.id).is_some() {
                    env.id = brush_pack::new_id();
                }
                fill(&mut env, o, "", "", "", &file_name(src));
                if let Some(n) = o.name.clone().filter(|n| !n.trim().is_empty()) {
                    env.name = n;
                }
                let mut r = write_new_embedded(root, &env, o)?;
                r.warnings = p.warnings;
                return Ok(r);
            }
            palette::parse_loose_json(&v)?
        }
    };
    if pal.name.trim().is_empty() {
        pal.name = stem(src);
    }
    let mut r = add_palette(root, &pal, o, &file_name(src))?;
    r.warnings = pal.warnings;
    Ok(r)
}

fn import_pose(root: &Path, src: &Path, o: &ImportOpts) -> Result<Imported, String> {
    let len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > MAX_TEXT_FILE {
        return Err(format!("arquivo de {} MB acima do limite", len >> 20));
    }
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("JSON inválido: {e}"))?;
    let mut env = if v.get("format").and_then(Value::as_str).is_some() {
        let e = Envelope::from_value(v)?;
        if e.kind != Kind::Pose {
            return Err(format!("o arquivo é um recurso '{}', não uma pose", e.kind.key()));
        }
        e
    } else {
        // pose de outra ferramenta (sem envelope): a pose vai inteira no payload
        let mut e = Envelope::new(Kind::Pose, &stem(src));
        e.payload = v;
        e
    };
    if let Some(n) = o.name.clone().filter(|n| !n.trim().is_empty()) {
        env.name = n;
    }
    if Index::scan(root).get(&env.id).is_some() {
        env.id = brush_pack::new_id();
    }
    fill(&mut env, o, "", "", "", &file_name(src));
    write_new_embedded(root, &env, o)
}

fn import_embedded(root: &Path, src: &Path, kind: Kind, o: &ImportOpts) -> Result<Imported, String> {
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    let mut env = Envelope::from_json(&bytes)?;
    if env.kind != kind {
        return Err(format!("o arquivo é um recurso '{}', não '{}'", env.kind.key(), kind.key()));
    }
    if let Some(n) = o.name.clone().filter(|n| !n.trim().is_empty()) {
        env.name = n;
    }
    if Index::scan(root).get(&env.id).is_some() {
        env.id = brush_pack::new_id();
    }
    fill(&mut env, o, "", "", "", &file_name(src));
    write_new_embedded(root, &env, o)
}

/// `.aiaibrush`/`.aiaibrushes`: valida pelo `brush_pack` e instala em `Pincéis\Importados` (o painel Pincéis lê a pasta;
/// quem tem o app aberto usa a janela de importar dele, que mostra a licença antes). Pincel com o `id` de um que já
/// está na biblioteca é pulado com aviso.
fn import_brushes(root: &Path, src: &Path, o: &ImportOpts) -> Result<Vec<Imported>, String> {
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    let (files, pack_name) = match brush_pack::read(&bytes)? {
        Package::Brush(b) => (vec![*b], None),
        Package::Pack(p) => (p.brushes, Some(p.manifest.name.clone())),
    };
    let have: std::collections::HashSet<String> = Index::scan(root).entries.into_iter().filter(|e| e.kind == Kind::Brush).map(|e| e.id).collect();
    let mut dir = kind_dir(root, Kind::Brush).join("Importados");
    if let Some(n) = &pack_name {
        dir = dir.join(sanitize(n));
    }
    let mut out = Vec::new();
    for mut f in files {
        if have.contains(&f.preset.id) {
            out.push(Imported { id: f.preset.id.clone(), kind: Kind::Brush, name: f.preset.name.clone(), path: PathBuf::new(), warnings: vec![format!("\"{}\" já está na biblioteca (mesmo id): pulado", f.preset.name)] });
            continue;
        }
        if f.preset.license.trim().is_empty() {
            f.preset.license = if o.license.trim().is_empty() { UNKNOWN_LICENSE.to_owned() } else { o.license.trim().to_owned() };
        }
        if !o.tags.is_empty() {
            for t in &o.tags {
                if !f.preset.tags.contains(t) {
                    f.preset.tags.push(t.clone());
                }
            }
        }
        let data = brush_pack::write_brush(&f)?;
        let dest = unique_path(&dir, &f.preset.name, brush_pack::EXT_BRUSH);
        write_atomic(&dest, &data)?;
        out.push(Imported { id: f.preset.id.clone(), kind: Kind::Brush, name: f.preset.name.clone(), path: dest, warnings: Vec::new() });
    }
    bump();
    Ok(out)
}

/// Grava um modelo (bytes de GLB) na biblioteca com o manifesto: "Salvar na biblioteca" a partir de um modelo do projeto.
pub fn add_model_glb(root: &Path, glb: &[u8], info: &aiai_3d_model::ModelInfo, o: &ImportOpts) -> Result<Imported, String> {
    // confere que o GLB abre antes de gravar
    aiai_3d_model::glb::read_glb(glb).map_err(|e| format!("GLB inválido: {}", e.0))?;
    let nm = o.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "Modelo".into());
    let dest = unique_path(&dest_dir(root, Kind::Model3d, o), &nm, "glb");
    write_atomic(&dest, glb)?;
    let mut env = Envelope::new(Kind::Model3d, &nm);
    env.payload = json!({ "model3d": info });
    env.files = vec![file_ref(&file_name(&dest), "model", glb)];
    fill(&mut env, o, &info.license.license, &info.license.author, &info.license.source, &info.file_name);
    if let Err(e) = write_envelope(&Storage::Sidecar(sidecar_for(&dest)), &dest, &env) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
    Ok(Imported { id: env.id, kind: Kind::Model3d, name: nm, path: dest, warnings: Vec::new() })
}

/// Etiquetas digitadas → lista (atalho para quem monta `ImportOpts`).
pub fn tags_from_text(t: &str) -> Vec<String> {
    parse_tags(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-assets-imp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Cubo mínimo como OBJ (o importador lê OBJ sem arquivos vizinhos).
    fn cube_obj() -> &'static str {
        "# Cubo de teste (CC0)\no Cubo\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nf 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n"
    }

    fn opts() -> ImportOpts {
        ImportOpts { author: "Ana".into(), license: "CC0-1.0".into(), source: "https://exemplo.org/x".into(), tags: vec!["teste".into()], ..Default::default() }
    }

    #[test]
    fn imports_obj_as_a_normalized_glb_with_sidecar_and_keeps_the_original() {
        let (root, src) = (tmp("obj"), tmp("obj-src"));
        let f = src.join("cubo.obj");
        std::fs::write(&f, cube_obj()).unwrap();
        let r = import_file(&root, &f, &opts()).unwrap();
        assert_eq!(r.len(), 1);
        let idx = Index::scan(&root);
        let e = idx.get(&r[0].id).unwrap();
        assert_eq!((e.kind, e.env.name.as_str()), (Kind::Model3d, "cubo"));
        assert_eq!(e.path.extension().unwrap(), "glb");
        assert!(!e.inferred, "o manifesto foi gravado");
        assert_eq!(e.stats.triangles, Some(12));
        assert_eq!(e.stats.size_m, Some([1.0, 1.0, 1.0]));
        assert_eq!((e.env.author.name.as_str(), e.env.license.spdx.as_deref(), e.env.source.url.as_str()), ("Ana", Some("CC0-1.0"), "https://exemplo.org/x"));
        assert_eq!(e.env.tags, vec!["teste"]);
        assert_eq!(e.env.source.imported_from, "obj");
        assert!(root.join("Modelos 3D").join("_Originais").join("cubo.obj").exists());
        // o GLB gravado abre no importador do app
        aiai_3d_model::import_path(&e.path).unwrap();
        // importar de novo não sobrescreve
        let again = import_file(&root, &f, &opts()).unwrap();
        assert_ne!(again[0].path, r[0].path);
        assert_eq!(Index::scan(&root).count(Some(Kind::Model3d)), 2);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn glb_keeps_its_bytes_and_unknown_license_is_marked() {
        let (root, src) = (tmp("glb"), tmp("glb-src"));
        let m = aiai_3d_model::import_bytes(cube_obj().as_bytes(), "obj", "cubo.obj", &|_| None).unwrap();
        let glb = aiai_3d_model::glb::write_glb(&m);
        let f = src.join("caixa.glb");
        std::fs::write(&f, &glb).unwrap();
        let r = import_file(&root, &f, &ImportOpts::default()).unwrap();
        let idx = Index::scan(&root);
        let e = idx.get(&r[0].id).unwrap();
        assert_eq!(std::fs::read(&e.path).unwrap(), glb, "o .glb entra como veio");
        assert_eq!(e.env.license_text(), UNKNOWN_LICENSE, "licença não informada vira desconhecida");
        assert_eq!(e.env.source_text(), "caixa.glb");
        assert_eq!(e.env.files[0].blake3, blake3::hash(&glb).to_hex().to_string());
        // vrm: recusado com o motivo, nada gravado
        let v = src.join("a.vrm");
        std::fs::write(&v, b"x").unwrap();
        assert!(import_file(&root, &v, &ImportOpts::default()).unwrap_err().contains("VRM"));
        // arquivo ruim: erro e nada novo
        let bad = src.join("ruim.glb");
        std::fs::write(&bad, b"nao e glb").unwrap();
        assert!(import_file(&root, &bad, &ImportOpts::default()).is_err());
        assert_eq!(Index::scan(&root).count(None), 1);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn imports_images_as_texture_or_material_and_rejects_corrupt_files() {
        let (root, src) = (tmp("img"), tmp("img-src"));
        let png = src.join("papel.png");
        image::RgbaImage::from_pixel(16, 8, image::Rgba([200, 190, 170, 255])).save(&png).unwrap();
        let t = import_file(&root, &png, &opts()).unwrap();
        let m = import_file(&root, &png, &ImportOpts { as_kind: Some(Kind::Material), ..opts() }).unwrap();
        let idx = Index::scan(&root);
        assert_eq!(idx.get(&t[0].id).unwrap().kind, Kind::Texture);
        assert_eq!(idx.get(&m[0].id).unwrap().kind, Kind::Material);
        assert!(idx.get(&t[0].id).unwrap().path.starts_with(root.join("Texturas")));
        assert!(idx.get(&m[0].id).unwrap().path.starts_with(root.join("Materiais")));
        assert_eq!((idx.get(&t[0].id).unwrap().stats.width, idx.get(&t[0].id).unwrap().stats.height), (Some(16), Some(8)));
        // PNG truncado: o cabeçalho abre mas a decodificação falha
        let mut data = std::fs::read(&png).unwrap();
        data.truncate(data.len() / 2);
        let bad = src.join("cortada.png");
        std::fs::write(&bad, &data).unwrap();
        assert!(import_file(&root, &bad, &opts()).is_err());
        assert!(import_file(&root, &png, &ImportOpts { as_kind: Some(Kind::Palette), ..opts() }).is_err());
        assert_eq!(Index::scan(&root).count(None), 2);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn imports_gpl_and_loose_json_palettes_and_round_trips_our_own() {
        let (root, src) = (tmp("pal"), tmp("pal-src"));
        let gpl = src.join("pele.gpl");
        std::fs::write(&gpl, "GIMP Palette\nName: Peles\n255 224 189 Clara\n141 85 36 Escura\n").unwrap();
        let r = import_file(&root, &gpl, &opts()).unwrap();
        let idx = Index::scan(&root);
        let e = idx.get(&r[0].id).unwrap();
        assert_eq!((e.kind, e.env.name.as_str(), e.stats.colors), (Kind::Palette, "Peles", Some(2)));
        assert_eq!(e.path.extension().unwrap(), "aiaipal");
        let pal = Palette::from_payload(&e.env.payload).unwrap();
        assert_eq!(pal.colors[1].hex, "#8d5524");
        // JSON solto
        let js = src.join("cores.json");
        std::fs::write(&js, r##"["#ff0000", "#00ff00"]"##).unwrap();
        let r2 = import_file(&root, &js, &opts()).unwrap();
        assert_eq!(Index::scan(&root).get(&r2[0].id).unwrap().stats.colors, Some(2));
        // o arquivo do próprio app volta como cópia com id novo (o id original já existe na biblioteca)
        let own = std::fs::copy(&e.path, src.join("de-novo.aiaipal")).map(|_| src.join("de-novo.aiaipal")).unwrap();
        let r3 = import_file(&root, &own, &ImportOpts::default()).unwrap();
        assert_ne!(r3[0].id, e.id);
        assert_eq!(Index::scan(&root).get(&r3[0].id).unwrap().env.author.name, "Ana", "o manifesto do arquivo é mantido");
        // não é paleta
        let bad = src.join("x.gpl");
        std::fs::write(&bad, "outra coisa").unwrap();
        assert!(import_file(&root, &bad, &opts()).is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn imports_poses_with_and_without_envelope() {
        let (root, src) = (tmp("pose"), tmp("pose-src"));
        let p = src.join("andando.aiaipose");
        std::fs::write(&p, r#"{"bones": {"braco_l": [0, 0, 30]}}"#).unwrap();
        let r = import_file(&root, &p, &opts()).unwrap();
        let idx = Index::scan(&root);
        let e = idx.get(&r[0].id).unwrap();
        assert_eq!((e.kind, e.env.name.as_str()), (Kind::Pose, "andando"));
        assert_eq!(e.env.payload["bones"]["braco_l"][2], 30);
        // .json solto: objeto sem `colors` é pose; `kind` força; lista de cores é paleta
        let j = src.join("caminhada.json");
        std::fs::write(&j, r#"{"name": "Caminhada", "bones": {"coxa": [0.4, 0, 0]}}"#).unwrap();
        let rj = import_file(&root, &j, &opts()).unwrap();
        assert_eq!(Index::scan(&root).get(&rj[0].id).unwrap().kind, Kind::Pose);
        let forced = import_file(&root, &j, &ImportOpts { as_kind: Some(Kind::Palette), ..opts() });
        assert!(forced.is_err(), "forçado como paleta, o JSON de pose não tem cores");
        let cores = src.join("cores.json");
        std::fs::write(&cores, r##"["#102030"]"##).unwrap();
        let rc = import_file(&root, &cores, &opts()).unwrap();
        assert_eq!(Index::scan(&root).get(&rc[0].id).unwrap().kind, Kind::Palette);
        assert!(import_file(&root, &src.join("nao-existe.aiaipose"), &opts()).is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn imports_native_brushes_and_skips_ones_already_there() {
        let (root, src) = (tmp("brush"), tmp("brush-src"));
        let preset = aiai_paint::brush::BrushPreset::round("Giz", 20.0, 0.8);
        let file = brush_pack::BrushFile::from_preset(&preset, None, &|_| None).unwrap();
        let f = src.join("giz.aiaibrush");
        std::fs::write(&f, brush_pack::write_brush(&file).unwrap()).unwrap();
        let r = import_file(&root, &f, &opts()).unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].path.starts_with(root.join("Pincéis").join("Importados")));
        let idx = Index::scan(&root);
        let e = idx.get(&r[0].id).unwrap();
        assert_eq!((e.kind, e.env.name.as_str()), (Kind::Brush, "Giz"));
        // o mesmo pincel de novo: pulado com aviso, nada gravado
        let again = import_file(&root, &f, &opts()).unwrap();
        assert!(again[0].path.as_os_str().is_empty() && again[0].warnings[0].contains("já está"));
        assert_eq!(Index::scan(&root).count(Some(Kind::Brush)), 1);
        // .aiaibrushes: cada pincel vira um arquivo na subpasta do pacote
        let mut p2 = aiai_paint::brush::BrushPreset::round("Tinta", 10.0, 0.9);
        p2.id = String::new();
        let f2 = brush_pack::BrushFile::from_preset(&p2, None, &|_| None).unwrap();
        let pack = brush_pack::Manifest::new_pack("Meu pacote", Default::default(), "CC0-1.0");
        let pk = src.join("pacote.aiaibrushes");
        std::fs::write(&pk, brush_pack::write_pack(&pack, &[f2]).unwrap()).unwrap();
        let r2 = import_file(&root, &pk, &opts()).unwrap();
        assert_eq!(r2.len(), 1);
        assert!(r2[0].path.starts_with(root.join("Pincéis").join("Importados").join("Meu pacote")));
        // formato de terceiros vai para o painel Pincéis
        assert!(import_file(&root, &src.join("x.abr"), &opts()).unwrap_err().contains("não é um formato"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn existing_aiaibrush_files_stay_readable_and_editable() {
        // um .aiaibrush feito antes da biblioteca (sem tocar no formato): entra no índice e a edição de metadados mantém o id
        let root = tmp("compat");
        let dir = root.join("Pincéis").join("Meus pincéis");
        std::fs::create_dir_all(&dir).unwrap();
        let mut preset = aiai_paint::brush::BrushPreset::round("Nanquim", 18.0, 0.9);
        preset.license = "CC-BY-4.0".into();
        preset.author.name = "Beto".into();
        preset.tags = vec!["tinta".into()];
        let file = brush_pack::BrushFile::from_preset(&preset, None, &|_| None).unwrap();
        let path = dir.join("Nanquim.aiaibrush");
        std::fs::write(&path, brush_pack::write_brush(&file).unwrap()).unwrap();
        let idx = Index::scan(&root);
        assert!(idx.problems.is_empty(), "{:?}", idx.problems);
        let e = idx.entries[0].clone();
        assert_eq!((e.kind, e.env.name.as_str(), e.env.author.name.as_str(), e.env.license.spdx.as_deref()), (Kind::Brush, "Nanquim", "Beto", Some("CC-BY-4.0")));
        assert_eq!(e.folder, "Meus pincéis");
        let env = crate::store::update_meta(&e, &crate::store::MetaEdit { name: Some("Nanquim fino".into()), license: Some("CC0-1.0".into()), ..Default::default() }.with_tag_text("tinta, linha")).unwrap();
        assert_eq!(env.id, e.id);
        // o pincel continua sendo um .aiaibrush válido do brush_pack com o preset intacto
        let Package::Brush(b) = brush_pack::read(&std::fs::read(&path).unwrap()).unwrap() else { panic!("não é pincel") };
        assert_eq!((b.preset.name.as_str(), b.preset.license.as_str(), b.preset.size, b.preset.tags.clone()), ("Nanquim fino", "CC0-1.0", 18.0, vec!["tinta".to_owned(), "linha".to_owned()]));
        assert_eq!(b.preset.id, e.id);
        let _ = std::fs::remove_dir_all(&root);
    }
    /// Gera pincéis de exemplo para a checagem visual do painel (nada é baixado):
    /// `AIAI_SAMPLE_BRUSH_OUT=<pasta> cargo test -p aiai-assets make_sample_brushes -- --ignored`.
    #[test]
    #[ignore]
    fn make_sample_brushes() {
        let Some(dir) = std::env::var_os("AIAI_SAMPLE_BRUSH_OUT").map(PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for (name, size, hard, tags, lic) in [("Nanquim fino", 6.0, 0.95, vec!["tinta", "linha"], "CC0-1.0"), ("Giz de cera", 30.0, 0.4, vec!["textura"], "CC-BY-4.0"), ("Aquarela suave", 48.0, 0.2, vec!["pintura", "fundo"], "CC0-1.0")] {
            let mut p = aiai_paint::brush::BrushPreset::round(name, size, hard);
            p.tags = tags.into_iter().map(String::from).collect();
            p.license = lic.into();
            p.author.name = "AnimeIAI".into();
            let f = brush_pack::BrushFile::from_preset(&p, None, &|_| None).unwrap();
            std::fs::write(dir.join(format!("{name}.aiaibrush")), brush_pack::write_brush(&f).unwrap()).unwrap();
        }
    }
}
