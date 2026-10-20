//! Gravação na pasta da biblioteca: nomes seguros, manifesto (sidecar/embutido/pincel), editar metadados, duplicar e
//! mandar para a lixeira. Toda gravação é atômica e vai para o contador de mudanças ([`crate::config::bump`]).

use crate::config::bump;
use crate::envelope::{Envelope, SIDECAR_SUFFIX, parse_tags};
use crate::index::{Entry, Storage, sidecar_for};
use crate::kind::Kind;
use aiai_paint::brush_pack::{self, Package};
use std::path::{Path, PathBuf};

/// Nome de arquivo/pasta seguro no Windows (sem caracteres proibidos, nomes reservados, ponto/espaço no fim).
pub fn sanitize(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    out = out.trim().trim_end_matches(['.', ' ']).chars().take(60).collect::<String>().trim().to_owned();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if out.is_empty() {
        return "recurso".to_owned();
    }
    if reserved || out.starts_with('.') {
        out.insert(0, '_');
    }
    out
}

/// Primeiro caminho livre `<pasta>\<nome>.<ext>`, `<nome> (2).<ext>`... (livre = nem o arquivo nem o sidecar existem).
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = sanitize(stem);
    let name = |s: &str| if ext.is_empty() { s.to_owned() } else { format!("{s}.{ext}") };
    let free = |p: &Path| !p.exists() && !sidecar_for(p).exists();
    let first = dir.join(name(&stem));
    if free(&first) {
        return first;
    }
    (2..10_000).map(|n| dir.join(name(&format!("{stem} ({n})")))).find(|p| free(p)).unwrap_or(first)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    aiai_core::persist::write_file_atomic(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Grava o envelope do recurso: no sidecar ou no próprio arquivo (formatos em JSON). Pincel: use [`update_meta`].
pub fn write_envelope(storage: &Storage, path: &Path, env: &Envelope) -> Result<(), String> {
    let bytes = env.to_json()?;
    let target = match storage {
        Storage::Sidecar(p) => p.as_path(),
        Storage::Embedded => path,
        Storage::Brush => return Err("o manifesto do pincel é gravado junto com o pincel".into()),
    };
    write_atomic(target, &bytes)?;
    // releitura: manifesto gravado tem que abrir
    let back = std::fs::read(target).map_err(|e| format!("{}: {e}", target.display()))?;
    Envelope::from_json(&back).map_err(|e| format!("{}: o manifesto gravado não passou na conferência ({e})", target.display()))?;
    bump();
    Ok(())
}

/// O que a tela de metadados edita (campo `None` = não mexe).
#[derive(Debug, Clone, Default)]
pub struct MetaEdit {
    pub name: Option<String>,
    pub description: Option<String>,
    /// Lista nova de etiquetas (substitui).
    pub tags: Option<Vec<String>>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub source: Option<String>,
    pub category: Option<String>,
}

impl MetaEdit {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.description.is_none() && self.tags.is_none() && self.author.is_none() && self.license.is_none() && self.source.is_none() && self.category.is_none()
    }

    /// Etiquetas digitadas num campo de texto.
    pub fn with_tag_text(mut self, text: &str) -> Self {
        self.tags = Some(parse_tags(text));
        self
    }

    fn apply(&self, e: &mut Envelope) -> Result<(), String> {
        if let Some(n) = &self.name {
            if n.trim().is_empty() {
                return Err("o nome não pode ficar vazio".into());
            }
            e.name = n.trim().to_owned();
        }
        if let Some(d) = &self.description {
            e.description = d.trim().to_owned();
        }
        if let Some(t) = &self.tags {
            e.tags = t.clone();
        }
        if let Some(a) = &self.author {
            e.author.name = a.trim().to_owned();
        }
        if let Some(l) = &self.license {
            e.set_license_text(l);
        }
        if let Some(s) = &self.source {
            e.set_source_text(s);
        }
        if let Some(c) = &self.category {
            e.category = c.trim().to_owned();
        }
        e.touch();
        Ok(())
    }
}

/// Grava a edição de metadados e devolve o envelope como ficou. Recurso sem manifesto (inferido) ganha o sidecar agora.
/// Relê o arquivo do disco antes de aplicar (o que o usuário editou fora do app não é perdido).
pub fn update_meta(e: &Entry, edit: &MetaEdit) -> Result<Envelope, String> {
    if edit.is_empty() {
        return Ok(e.env.clone());
    }
    match &e.storage {
        Storage::Brush => {
            let bytes = std::fs::read(&e.path).map_err(|e2| format!("{}: {e2}", e.path.display()))?;
            let Package::Brush(mut b) = brush_pack::read(&bytes)? else { return Err("não é um pincel".into()) };
            let mut env = Envelope::from_brush_manifest(&b.manifest);
            edit.apply(&mut env)?;
            b.preset.name = env.name.clone();
            b.preset.tags = env.tags.clone();
            b.preset.author = env.author.clone();
            b.preset.license = env.license_text();
            b.preset.source = env.source_text();
            b.preset.category = env.category.clone();
            b.preset.modified = env.modified.clone();
            b.manifest.description = env.description.clone();
            // a licença nova mantém url/atribuição que o usuário já tinha quando o texto é o mesmo (o `brush_pack` decide)
            let out = brush_pack::write_brush(&b)?;
            write_atomic(&e.path, &out)?;
            let back = std::fs::read(&e.path).map_err(|e2| e2.to_string())?;
            let ok = matches!(brush_pack::read(&back), Ok(Package::Brush(x)) if x.preset.id == b.preset.id);
            if !ok {
                return Err(format!("{}: o arquivo gravado não passou na conferência", e.path.display()));
            }
            bump();
            match brush_pack::read(&back)? {
                Package::Brush(x) => Ok(Envelope::from_brush_manifest(&x.manifest)),
                Package::Pack(_) => Err("não é um pincel".into()),
            }
        }
        storage => {
            let mut env = if e.inferred {
                e.env.clone()
            } else {
                let target = match storage {
                    Storage::Sidecar(p) => p.as_path(),
                    _ => e.path.as_path(),
                };
                std::fs::read(target).map_err(|e2| format!("{}: {e2}", target.display())).and_then(|b| Envelope::from_json(&b))?
            };
            edit.apply(&mut env)?;
            write_envelope(storage, &e.path, &env)?;
            Ok(env)
        }
    }
}

/// Arquivos de um recurso (o principal e o sidecar, se existir).
fn files_of(e: &Entry) -> Vec<PathBuf> {
    let mut v = vec![e.path.clone()];
    if let Storage::Sidecar(s) = &e.storage {
        if s.exists() {
            v.push(s.clone());
        }
    }
    v
}

/// Duplica o recurso na mesma pasta com id novo e nome "(cópia)". Devolve o caminho do arquivo novo.
pub fn duplicate(e: &Entry) -> Result<PathBuf, String> {
    let dir = e.path.parent().ok_or("recurso sem pasta")?;
    let stem = e.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "recurso".into());
    let ext = e.path.extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    let to = unique_path(dir, &format!("{stem} (cópia)"), &ext);
    let new_name = format!("{} (cópia)", e.env.name);
    match &e.storage {
        Storage::Brush => {
            let bytes = std::fs::read(&e.path).map_err(|e2| format!("{}: {e2}", e.path.display()))?;
            let Package::Brush(mut b) = brush_pack::read(&bytes)? else { return Err("não é um pincel".into()) };
            b.preset.id = brush_pack::new_id();
            b.preset.name = new_name;
            b.preset.created.clear();
            b.preset.modified.clear();
            b.preset.version = "1.0.0".into();
            write_atomic(&to, &brush_pack::write_brush(&b)?)?;
        }
        Storage::Embedded => {
            let mut env = e.env.clone();
            env.id = brush_pack::new_id();
            env.name = new_name;
            env.created = brush_pack::now_iso();
            env.modified = env.created.clone();
            write_atomic(&to, &env.to_json()?)?;
        }
        Storage::Sidecar(_) => {
            let bytes = std::fs::read(&e.path).map_err(|e2| format!("{}: {e2}", e.path.display()))?;
            write_atomic(&to, &bytes)?;
            let mut env = e.env.clone();
            env.id = brush_pack::new_id();
            env.name = new_name;
            env.created = brush_pack::now_iso();
            env.modified = env.created.clone();
            write_envelope(&Storage::Sidecar(sidecar_for(&to)), &to, &env)?;
        }
    }
    bump();
    Ok(to)
}

/// Manda o recurso (arquivo e sidecar) para `_Lixeira` na pasta do tipo; dá para recuperar à mão. Devolve onde foi.
pub fn trash(root: &Path, e: &Entry) -> Result<PathBuf, String> {
    let dir = crate::config::kind_dir(root, e.kind).join("_Lixeira");
    std::fs::create_dir_all(&dir).map_err(|e2| format!("{}: {e2}", dir.display()))?;
    let stem = e.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "recurso".into());
    let ext = e.path.extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    let to = unique_path(&dir, &stem, &ext);
    for f in files_of(e) {
        let dest = if f == e.path { to.clone() } else { sidecar_for(&to) };
        // rename falha entre volumes: copia e apaga
        if std::fs::rename(&f, &dest).is_err() {
            std::fs::copy(&f, &dest).map_err(|e2| format!("{}: {e2}", f.display()))?;
            std::fs::remove_file(&f).map_err(|e2| format!("{}: {e2}", f.display()))?;
        }
    }
    bump();
    Ok(to)
}

/// Garante as pastas dos tipos (a biblioteca existe mesmo vazia: o usuário a encontra no Explorador).
pub fn ensure_folders(root: &Path) -> Result<(), String> {
    for k in Kind::ALL {
        let d = crate::config::kind_dir(root, k);
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    Ok(())
}

/// O recurso é um sidecar? (nome do arquivo termina com o sufixo.)
pub fn is_sidecar(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().ends_with(SIDECAR_SUFFIX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Index;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-assets-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn texture(root: &Path, name: &str) {
        let dir = root.join("Texturas");
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([9, 9, 9, 255])).save(dir.join(format!("{name}.png"))).unwrap();
    }

    #[test]
    fn sanitize_and_unique() {
        assert_eq!(sanitize("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize("CON"), "_CON");
        assert_eq!(sanitize("  "), "recurso");
        let d = tmp("unique");
        std::fs::write(d.join("x.png"), b"1").unwrap();
        assert_eq!(unique_path(&d, "x", "png"), d.join("x (2).png"));
        assert_eq!(unique_path(&d, "y", "png"), d.join("y.png"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn editing_an_inferred_file_creates_the_sidecar_and_keeps_the_id() {
        let root = tmp("edit");
        texture(&root, "papel");
        let idx = Index::scan(&root);
        let e = idx.entries[0].clone();
        assert!(e.inferred);
        let env = update_meta(&e, &MetaEdit { name: Some("Papel áspero".into()), author: Some("Ana".into()), license: Some("CC0-1.0".into()), source: Some("https://exemplo.org/p".into()), ..Default::default() }.with_tag_text("papel, grão")).unwrap();
        assert_eq!(env.id, e.id, "o id inferido vira o id do manifesto");
        assert!(sidecar_for(&e.path).exists());
        let back = Index::scan(&root);
        let b = &back.entries[0];
        assert!(!b.inferred);
        assert_eq!((b.id.as_str(), b.env.name.as_str(), b.env.author.name.as_str()), (e.id.as_str(), "Papel áspero", "Ana"));
        assert_eq!(b.env.tags, vec!["papel", "grão"]);
        assert_eq!(b.env.license.spdx.as_deref(), Some("CC0-1.0"));
        assert_eq!(b.env.source.url, "https://exemplo.org/p");
        // nome vazio é recusado e nada muda
        assert!(update_meta(b, &MetaEdit { name: Some("  ".into()), ..Default::default() }).is_err());
        assert_eq!(Index::scan(&root).entries[0].env.name, "Papel áspero");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn duplicate_gets_a_new_id_and_trash_moves_both_files() {
        let root = tmp("dup");
        texture(&root, "grao");
        let e = Index::scan(&root).entries[0].clone();
        update_meta(&e, &MetaEdit { license: Some("MIT".into()), ..Default::default() }).unwrap();
        let e = Index::scan(&root).entries[0].clone();
        let copy = duplicate(&e).unwrap();
        assert_eq!(copy.file_name().unwrap().to_string_lossy(), "grao (cópia).png");
        let idx = Index::scan(&root);
        assert_eq!(idx.entries.len(), 2);
        let c = idx.entries.iter().find(|x| x.path == copy).unwrap();
        assert_ne!(c.id, e.id);
        assert_eq!(c.env.name, "grao (cópia)");
        assert_eq!(c.env.license.spdx.as_deref(), Some("MIT"));
        // lixeira: arquivo e sidecar saem da pasta e ficam em _Lixeira
        let to = trash(&root, c).unwrap();
        assert!(to.exists() && sidecar_for(&to).exists());
        assert!(!copy.exists() && !sidecar_for(&copy).exists());
        assert_eq!(Index::scan(&root).entries.len(), 1);
        // duas exclusões com o mesmo nome não se sobrescrevem
        let copy2 = duplicate(&e).unwrap();
        let c2 = Index::scan(&root).entries.into_iter().find(|x| x.path == copy2).unwrap();
        let to2 = trash(&root, &c2).unwrap();
        assert_ne!(to, to2);
        let _ = std::fs::remove_dir_all(&root);
    }
}
