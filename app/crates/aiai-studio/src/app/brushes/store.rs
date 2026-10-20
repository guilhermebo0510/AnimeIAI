//! A pasta de pincéis do usuário no disco: `Documentos\AnimeIAI\Pincéis\<Categoria>\*.aiaibrush`.
//!
//! - A pasta é a verdade: um `.aiaibrush` por pincel (formato em `aiai_paint::brush_pack`), dentro de pastas de
//!   categoria; `Importados\<pacote>\` guarda os que vieram de formatos de terceiros e `_Originais\` o arquivo original
//!   de onde vieram (prova de origem e licença). Pastas que começam com `.` ou `_` não são varridas.
//! - Varrer lê os arquivos e entrega os pincéis e a lista de arquivos com problema (nada some em silêncio).
//! - Migração única do `brushes.json` + `brush_assets` antigos (`%APPDATA%\AnimeIAI`) para arquivos `.aiaibrush`: os
//!   arquivos antigos não são mexidos (ficam de cópia) e a migração é registrada em `brushes.migrated.json` ao lado.
//! - `AIAI_BRUSHES_DIR` troca a pasta (testes e capturas de tela).

use crate::prefs::BrushAuthorPrefs;
use aiai_paint::brush_pack::{self, BrushFile, Package};
use std::path::{Path, PathBuf};

/// Maior `.aiaibrush` lido da pasta (o formato limita cada entrada a 64 MB; um pincel de verdade é pequeno).
const MAX_FILE: u64 = 64 << 20;

/// A pasta de pincéis: `AIAI_BRUSHES_DIR`; senão `Documentos\AnimeIAI\Pincéis` (a Documentos do sistema, OneDrive
/// incluído); sem Documentos, `%APPDATA%\AnimeIAI\Pincéis`.
pub(crate) fn user_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("AIAI_BRUSHES_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    aiai_paint::userdirs::animeiai_dir().map(|d| d.join("Pincéis")).or_else(|| crate::prefs::default_path().and_then(|p| p.parent().map(|d| d.join("Pincéis"))))
}

/// Nome da pasta de uma categoria (as de fábrica em português, estável ao trocar o idioma; as do usuário como escritas).
pub(crate) fn cat_folder(cat: &str) -> String {
    match cat {
        "lapis" => "Lápis e canetas".to_owned(),
        "pintura" => "Pintura".to_owned(),
        "aquarela" => "Aquarela e mistura".to_owned(),
        "textura" => "Textura".to_owned(),
        "cenario" => "Cenário".to_owned(),
        "borracha" => "Borrachas".to_owned(),
        "meus" => "Meus pincéis".to_owned(),
        "importados" => "Importados".to_owned(),
        other => sanitize(other),
    }
}

/// Nome de arquivo/pasta seguro no Windows (sem caracteres proibidos, nomes reservados, ponto/espaço no fim).
pub(crate) fn sanitize(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    out = out.trim().trim_end_matches(['.', ' ']).chars().take(60).collect::<String>().trim().to_owned();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if out.is_empty() {
        return "pincel".to_owned();
    }
    if reserved || out.starts_with('.') {
        out.insert(0, '_');
    }
    out
}

/// Primeiro caminho livre `<pasta>\<nome>.<ext>`, `<nome> (2).<ext>`...
pub(crate) fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = sanitize(stem);
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..10_000).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// Grava o pincel em arquivo novo (nunca por cima de outro), confere relendo e devolve o caminho.
pub(crate) fn write_new(dir: &Path, stem: &str, file: &BrushFile) -> Result<PathBuf, String> {
    let bytes = brush_pack::write_brush(file)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = unique_path(dir, stem, brush_pack::EXT_BRUSH);
    aiai_core::persist::write_file_atomic(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    // releitura: o arquivo gravado tem que abrir e dar o mesmo pincel (uma gravação ruim não passa em silêncio)
    let back = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()));
    let ok = back.and_then(|b| brush_pack::read(&b)).is_ok_and(|p| matches!(p, Package::Brush(b) if b.preset.id == file.preset.id));
    if !ok {
        let _ = std::fs::remove_file(&path);
        return Err(format!("{}: o arquivo gravado não passou na conferência", path.display()));
    }
    Ok(path)
}

pub(crate) struct Scan {
    pub items: Vec<(PathBuf, BrushFile)>,
    /// Arquivos que não deu para ler (nome e motivo): vão para o aviso, o arquivo fica onde está.
    pub problems: Vec<(PathBuf, String)>,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if p.is_dir() {
            if depth < 6 && !name.starts_with(['.', '_']) {
                walk(&p, depth + 1, out);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(brush_pack::EXT_BRUSH)) {
            out.push(p);
        }
    }
}

/// Lê todos os `.aiaibrush` da pasta.
pub(crate) fn scan(root: &Path) -> Scan {
    let mut files = Vec::new();
    walk(root, 0, &mut files);
    let mut s = Scan { items: Vec::new(), problems: Vec::new() };
    for p in files {
        let r = (|| -> Result<BrushFile, String> {
            let len = std::fs::metadata(&p).map_err(|e| e.to_string())?.len();
            if len > MAX_FILE {
                return Err(format!("arquivo grande demais ({} MB)", len >> 20));
            }
            match brush_pack::read(&std::fs::read(&p).map_err(|e| e.to_string())?)? {
                Package::Brush(b) => Ok(*b),
                Package::Pack(_) => Err("é um pacote (.aiaibrushes) com a extensão de um pincel: use Importar".into()),
            }
        })();
        match r {
            Ok(b) => s.items.push((p, b)),
            Err(e) => s.problems.push((p, e)),
        }
    }
    s
}

/// De que formato de terceiro veio um pincel, pelo texto de `source` ("arquivo.abr" ou "arquivo.brushset › pasta").
pub(crate) fn imported_from_kind(source: &str) -> &'static str {
    let file = source.split(" › ").next().unwrap_or(source).to_lowercase();
    match file.rsplit('.').next().unwrap_or("") {
        "abr" => "photoshop-abr",
        "brush" | "brushset" => "procreate",
        "bundle" => "krita-bundle",
        "gbr" | "gih" => "gimp",
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => "image",
        _ => "other",
    }
}

// ------------------------------------------------------------------------------------------------ migração

pub(crate) struct Migration {
    pub written: usize,
    pub warning: Option<String>,
}

/// Arquivo que registra que a migração aconteceu (ao lado do `brushes.json` antigo).
pub(crate) fn migration_marker(legacy_json: &Path) -> PathBuf {
    legacy_json.with_file_name("brushes.migrated.json")
}

/// Migra uma vez o `brushes.json` antigo (e a pasta `brush_assets` ao lado dele) para `.aiaibrush` em `dest`. Os
/// arquivos antigos ficam como estão. Arquivo ausente ou já migrado = nada a fazer; arquivo que não deu para ler agora
/// = tenta de novo na próxima abertura; arquivo corrompido = cópia `brushes.json.corrupt-<data>` e aviso (e não tenta
/// mais); entrada inválida ou sem a imagem = fica só no arquivo antigo, com aviso.
pub(crate) fn migrate_legacy(legacy_json: &Path, dest: &Path, author: &BrushAuthorPrefs) -> Migration {
    let none = Migration { written: 0, warning: None };
    let marker = migration_marker(legacy_json);
    if marker.exists() {
        return none;
    }
    let bytes = match std::fs::read(legacy_json) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return none,
        Err(e) => {
            tracing::warn!("pincéis do usuário: {}: {e}", legacy_json.display());
            return Migration { written: 0, warning: Some(crate::i18n::tr("msg.brushes_unreadable").replace("{path}", &legacy_json.display().to_string()).replace("{err}", &e.to_string())) };
        }
    };
    let (good, bad, errs) = match super::parse_user_brushes(&bytes) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("pincéis do usuário ilegíveis em {}: {e}", legacy_json.display());
            let warning = match aiai_core::persist::keep_corrupt_copy(legacy_json) {
                Ok(copy) => crate::i18n::tr("msg.brushes_corrupt").replace("{err}", &e).replace("{path}", &copy.display().to_string()),
                Err(ce) => crate::i18n::tr("msg.brushes_unreadable").replace("{path}", &legacy_json.display().to_string()).replace("{err}", &format!("{e}; {ce}")),
            };
            write_marker(&marker, dest, 0, 1);
            return Migration { written: 0, warning: Some(warning) };
        }
    };
    let assets_dir = legacy_json.with_file_name("brush_assets");
    let lookup = |h: &aiai_core::BlobHash| std::fs::read(assets_dir.join(format!("{h}.aim"))).ok().filter(|b| aiai_core::BlobHash::of(b) == *h);
    let (mut written, mut failed) = (0usize, Vec::new());
    for mut p in good {
        let imported = !p.source.trim().is_empty();
        p.id.clear();
        p.created.clear();
        p.modified.clear();
        if imported {
            // veio de formato de terceiro: a licença é a do autor do pacote original
            p.license = brush_pack::UNKNOWN_LICENSE.to_owned();
        } else {
            if p.author.is_empty() {
                p.author = aiai_paint::brush::BrushAuthor { name: author.name.clone(), email: author.email.clone(), url: author.url.clone() };
            }
            if p.license.trim().is_empty() {
                p.license = author_license(author);
            }
        }
        let name = p.name.clone();
        let cat = super::cat_of(&p);
        let r = BrushFile::from_preset(&p, None, &lookup).and_then(|mut f| {
            if imported {
                f.manifest.source.imported_from = imported_from_kind(&p.source).to_owned();
            }
            let folder = if imported && cat == "importados" { dest.join(cat_folder(&cat)).join(sanitize(p.source.split(" › ").next().unwrap_or("pacote").rsplit_once('.').map(|(a, _)| a).unwrap_or("pacote"))) } else { dest.join(cat_folder(&cat)) };
            write_new(&folder, &name, &f)
        });
        match r {
            Ok(_) => written += 1,
            Err(e) => failed.push(format!("\"{name}\": {e}")),
        }
    }
    write_marker(&marker, dest, written, bad.len() + failed.len());
    let mut notes = errs;
    notes.extend(failed);
    let warning = (!notes.is_empty()).then(|| crate::i18n::tr("msg.brushes_skipped").replace("{n}", &notes.len().to_string()).replace("{err}", &notes.join("; ")));
    Migration { written, warning }
}

/// Licença escolhida nas preferências, ou a padrão.
pub(crate) fn author_license(a: &BrushAuthorPrefs) -> String {
    if a.license.trim().is_empty() { brush_pack::DEFAULT_LICENSE.to_owned() } else { a.license.trim().to_owned() }
}

fn write_marker(marker: &Path, dest: &Path, migrated: usize, left_behind: usize) {
    let v = serde_json::json!({
        "migrated_at": brush_pack::now_iso(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "to": dest.display().to_string(),
        "migrated": migrated,
        "left_in_legacy_file": left_behind,
        "note": "brushes.json e brush_assets continuam aqui, intactos, como cópia de segurança; os pincéis agora são arquivos .aiaibrush na pasta indicada em \"to\".",
    });
    if let Ok(text) = serde_json::to_string_pretty(&v) {
        if let Err(e) = aiai_core::persist::write_file_atomic(marker, text.as_bytes()) {
            tracing::warn!("não deu para registrar a migração dos pincéis em {}: {e}", marker.display());
        }
    }
}
