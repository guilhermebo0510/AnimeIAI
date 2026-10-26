//! Biblioteca de pincéis: os de fábrica (motor) e os do usuário, por categoria.
//! Os do usuário são arquivos `.aiaibrush` (formato próprio, com autor, licença e origem) em
//! `Documentos\AnimeIAI\Pincéis\<Categoria>\`; a pasta é a verdade e a biblioteca a lê na abertura e quando o usuário
//! pede para reler. Os pincéis do `brushes.json` antigo migram uma vez para arquivos. O painel Ferramenta mostra as
//! categorias com miniaturas desenhadas pelo próprio motor; "Salvar como pincel novo" guarda a cópia ajustada. O traço
//! grava o preset inteiro no documento, então mexer na biblioteca nunca muda desenho já feito.

mod store;

use crate::prefs::BrushAuthorPrefs;
use aiai_paint::brush::{BrushAuthor, BrushPreset};
use aiai_paint::brush_pack::{self, BrushFile, License, Manifest};
use std::path::PathBuf;

/// Categorias de fábrica (chave, chave i18n), na ordem do painel.
pub(crate) const CATEGORIES: [(&str, &str); 8] = [
    ("lapis", "brushcat.lapis"),
    ("pintura", "brushcat.pintura"),
    ("aquarela", "brushcat.aquarela"),
    ("textura", "brushcat.textura"),
    ("cenario", "brushcat.cenario"),
    ("borracha", "brushcat.borracha"),
    ("meus", "brushcat.meus"),
    ("importados", "brushcat.importados"),
];

#[derive(Clone)]
pub(crate) struct LibBrush {
    pub preset: BrushPreset,
    /// Do usuário (pode apagar); os de fábrica não saem.
    pub user: bool,
    /// Arquivo `.aiaibrush` do pincel do usuário (`None` = de fábrica ou sem pasta, nas execuções sem sessão).
    pub file: Option<PathBuf>,
    /// Manifesto do arquivo (origem detalhada, licença com link, `extra`...), regravado junto com o pincel.
    pub manifest: Option<Manifest>,
}

/// Autor e licença confirmados na exportação.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ExportMeta {
    pub author: BrushAuthor,
    pub license: String,
    /// Como citar o autor (licenças CC-BY exigem).
    pub attribution: String,
}

pub(crate) struct BrushLibrary {
    pub items: Vec<LibBrush>,
    /// Pasta dos pincéis do usuário (`None` nas execuções sem sessão: testes e capturas não leem nem gravam nada).
    dir: Option<PathBuf>,
    /// Muda a cada alteração (miniaturas refeitas).
    pub version: u64,
    /// Imagens das pontas e texturas dos pincéis do usuário (máscaras "AIM1" por hash), lidas dos arquivos.
    mem_assets: std::collections::HashMap<aiai_core::BlobHash, Vec<u8>>,
    /// Avisos para o usuário (a UI lê uma vez e mostra na janela de avisos).
    warnings: Vec<String>,
    /// Arquivos da pasta que não deu para ler na última varredura (nome e motivo).
    pub problems: Vec<(PathBuf, String)>,
    /// Autor e licença do perfil do usuário (últimos valores; vão para as preferências).
    pub author: BrushAuthorPrefs,
    author_changed: bool,
}

impl Default for BrushLibrary {
    fn default() -> Self {
        Self::factory()
    }
}

/// `%APPDATA%\AnimeIAI\brushes.json`: o arquivo antigo, só lido para migrar.
pub(crate) fn default_path() -> Option<PathBuf> {
    crate::prefs::default_path().and_then(|p| p.parent().map(|d| d.join("brushes.json")))
}

pub(crate) use store::{cat_folder, imported_from_kind, sanitize};

impl BrushLibrary {
    pub(crate) fn factory() -> Self {
        BrushLibrary {
            items: BrushPreset::defaults().into_iter().map(|preset| LibBrush { preset, user: false, file: None, manifest: None }).collect(),
            dir: None,
            version: 1,
            mem_assets: Default::default(),
            warnings: Vec::new(),
            problems: Vec::new(),
            author: BrushAuthorPrefs::default(),
            author_changed: false,
        }
    }

    /// Abertura do app: com sessão lê a pasta do usuário (migrando o `brushes.json` antigo na primeira vez); sem sessão
    /// só os de fábrica e nada é lido nem gravado.
    /// `AIAI_BRUSHES_DIR` aponta a biblioteca para outra pasta mesmo sem sessão (testes e capturas de tela; sem migrar
    /// o `brushes.json`).
    pub(crate) fn open(use_folder: bool, author: &BrushAuthorPrefs) -> Self {
        if use_folder {
            return Self::open_at(store::user_dir(), default_path(), author);
        }
        match std::env::var_os("AIAI_BRUSHES_DIR").filter(|v| !v.is_empty()) {
            Some(d) => Self::open_at(Some(PathBuf::from(d)), None, author),
            None => Self::factory(),
        }
    }

    /// Fábrica + os `.aiaibrush` de `dir`. `legacy_json` = o `brushes.json` antigo, migrado uma vez para `dir`.
    pub(crate) fn open_at(dir: Option<PathBuf>, legacy_json: Option<PathBuf>, author: &BrushAuthorPrefs) -> Self {
        let mut lib = Self::factory();
        lib.author = author.clone();
        if let Some(d) = dir {
            if let Some(j) = legacy_json {
                let m = store::migrate_legacy(&j, &d, author);
                if m.written > 0 {
                    tracing::info!("{} pincéis migrados de {} para {}", m.written, j.display(), d.display());
                }
                lib.warnings.extend(m.warning);
            }
            lib.dir = Some(d);
            lib.rescan();
        }
        lib
    }

    pub(crate) fn dir(&self) -> Option<&std::path::Path> {
        self.dir.as_deref()
    }

    /// Lê de novo a pasta (arquivos novos, apagados ou trocados fora do app). Devolve quantos pincéis do usuário há.
    pub(crate) fn rescan(&mut self) -> usize {
        let Some(dir) = self.dir.clone() else { return 0 };
        self.items.retain(|b| !b.user);
        let mut s = store::scan(&dir);
        s.items.sort_by_key(|(_, b)| (cat_of(&b.preset).to_lowercase(), b.preset.name.to_lowercase()));
        for (path, b) in s.items {
            self.mem_assets.extend(b.assets);
            self.items.push(LibBrush { preset: b.preset, user: true, file: Some(path), manifest: Some(b.manifest) });
        }
        if !s.problems.is_empty() {
            let list: Vec<String> = s.problems.iter().map(|(p, e)| format!("{}: {e}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).collect();
            tracing::warn!("pincéis com problema em {}: {}", dir.display(), list.join("; "));
            self.warnings.push(crate::i18n::tr("msg.brushes_files_bad").replace("{n}", &s.problems.len().to_string()).replace("{dir}", &dir.display().to_string()).replace("{err}", &list.join("; ")));
        }
        self.problems = s.problems;
        self.version += 1;
        self.items.iter().filter(|b| b.user).count()
    }

    /// Aviso pendente sobre a pasta de pincéis (uma vez).
    pub(crate) fn take_warning(&mut self) -> Option<String> {
        if self.warnings.is_empty() { None } else { Some(std::mem::take(&mut self.warnings).join("\n")) }
    }

    /// Autor e licença do perfil mudaram (exportação ou criador): a UI leva para as preferências.
    pub(crate) fn set_author(&mut self, a: BrushAuthorPrefs) {
        if self.author != a {
            self.author = a;
            self.author_changed = true;
        }
    }

    pub(crate) fn take_author_change(&mut self) -> Option<BrushAuthorPrefs> {
        std::mem::take(&mut self.author_changed).then(|| self.author.clone())
    }

    /// Autor do perfil como `BrushAuthor`.
    pub(crate) fn profile_author(&self) -> BrushAuthor {
        BrushAuthor { name: self.author.name.clone(), email: self.author.email.clone(), url: self.author.url.clone() }
    }

    pub(crate) fn profile_license(&self) -> String {
        store::author_license(&self.author)
    }

    /// Guarda a imagem (máscara "AIM1") de uma ponta/textura; devolve o hash que o preset referencia. A imagem vai para
    /// o arquivo do pincel quando ele é salvo.
    pub(crate) fn put_asset(&mut self, bytes: Vec<u8>) -> Result<aiai_core::BlobHash, String> {
        let h = aiai_core::BlobHash::of(&bytes);
        self.mem_assets.insert(h.clone(), bytes);
        Ok(h)
    }

    pub(crate) fn has_asset(&self, h: &aiai_core::BlobHash) -> bool {
        self.mem_assets.contains_key(h)
    }

    fn asset(&self, h: &aiai_core::BlobHash) -> Option<Vec<u8>> {
        self.mem_assets.get(h).cloned()
    }

    /// A imagem "AIM1" guardada na biblioteca com este hash.
    pub(crate) fn asset_bytes(&self, h: &aiai_core::BlobHash) -> Option<Vec<u8>> {
        self.asset(h)
    }

    /// Hashes de imagem que o preset usa (ponta e textura).
    fn asset_refs(p: &BrushPreset) -> Vec<aiai_core::BlobHash> {
        let mut v = Vec::new();
        if let aiai_paint::brush::BrushTip::Image { blob, .. } = &p.tip {
            v.push(blob.clone());
        }
        if let Some(h) = p.texture.as_ref().and_then(|t| t.image.clone()) {
            v.push(h);
        }
        v
    }

    /// O preset com as máscaras de imagem carregadas da biblioteca (miniaturas e prévia do criador).
    pub(crate) fn with_masks(&self, p: &BrushPreset) -> BrushPreset {
        let mut q = p.clone();
        if let aiai_paint::brush::BrushTip::Image { blob, mask } = &mut q.tip {
            if mask.is_none() {
                *mask = self.asset(blob).and_then(|b| aiai_paint::mask::Mask::decode(&b).ok());
            }
        }
        if let Some(t) = &mut q.texture {
            if let (Some(h), None) = (t.image.clone(), &t.image_mask) {
                t.image_mask = self.asset(&h).and_then(|b| aiai_paint::mask::Mask::decode(&b).ok()).map(std::sync::Arc::new);
            }
        }
        q
    }

    /// Põe no projeto as imagens que o preset usa e que ele ainda não tem (pincel do usuário usado num projeto novo).
    pub(crate) fn ensure_blobs(&self, blobs: &mut aiai_core::BlobStore, p: &BrushPreset) {
        for h in Self::asset_refs(p) {
            if !blobs.contains(&h) {
                if let Some(b) = self.asset(&h) {
                    blobs.put(b);
                }
            }
        }
    }

    pub(crate) fn get(&self, i: usize) -> Option<&BrushPreset> {
        self.items.get(i).map(|b| &b.preset)
    }

    /// Índice do pincel do usuário com este id.
    pub(crate) fn find_id(&self, id: &str) -> Option<usize> {
        if id.is_empty() { None } else { self.items.iter().position(|b| b.user && b.preset.id == id) }
    }

    /// Índice do pincel que era `(id, nome)` antes de reler a pasta (a escolha do usuário sobrevive).
    pub(crate) fn index_of(&self, id: &str, name: &str) -> Option<usize> {
        self.find_id(id).or_else(|| self.items.iter().position(|b| b.preset.name == name))
    }

    /// Guarda uma cópia como pincel novo do usuário (arquivo novo na pasta da categoria); devolve o índice. Pincel
    /// novo = id, datas e versão novos; autor e licença do perfil quando o preset ainda não tem.
    pub(crate) fn add_user(&mut self, mut preset: BrushPreset, name: &str, category: &str) -> Result<usize, String> {
        preset.name = name.trim().to_owned();
        preset.category = category.trim().to_owned();
        if preset.name.is_empty() {
            return Err(crate::i18n::tr("brushes.need_name").to_owned());
        }
        preset.validate()?;
        preset.id.clear();
        preset.version.clear();
        preset.created.clear();
        preset.modified.clear();
        if preset.author.is_empty() {
            preset.author = self.profile_author();
        }
        if preset.license.trim().is_empty() {
            preset.license = self.profile_license();
        }
        let file = BrushFile::from_preset(&preset, None, &|h| self.asset(h))?;
        self.install_file(file, None)
    }

    /// Instala um pincel (criado aqui ou lido de um pacote) na pasta da categoria dele (e na subpasta `sub`, se houver).
    pub(crate) fn install_file(&mut self, file: BrushFile, sub: Option<&str>) -> Result<usize, String> {
        let path = match &self.dir {
            Some(d) => {
                let mut folder = d.join(cat_folder(&cat_of(&file.preset)));
                if let Some(s) = sub.filter(|s| !s.trim().is_empty()) {
                    folder = folder.join(store::sanitize(s));
                }
                Some(store::write_new(&folder, &file.preset.name, &file)?)
            }
            None => None,
        };
        self.mem_assets.extend(file.assets.clone());
        self.items.push(LibBrush { preset: file.preset, user: true, file: path, manifest: Some(file.manifest) });
        self.version += 1;
        Ok(self.items.len() - 1)
    }

    /// Guarda o arquivo original de um pacote de terceiros em `_Originais` (prova de origem e licença).
    pub(crate) fn keep_original(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let Some(d) = &self.dir else { return Ok(()) };
        let dir = d.join("_Originais");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let (stem, ext) = name.rsplit_once('.').unwrap_or((name, "bin"));
        let to = store::unique_path(&dir, stem, &store::sanitize(ext));
        aiai_core::persist::write_file_atomic(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))
    }

    /// Tira um pincel do usuário da biblioteca: o arquivo vai para `_Lixeira` na pasta de pincéis (dá para recuperar).
    pub(crate) fn remove_user(&mut self, i: usize) -> Result<(), String> {
        if !self.items.get(i).is_some_and(|b| b.user) {
            return Ok(());
        }
        if let (Some(f), Some(d)) = (self.items[i].file.clone(), self.dir.clone()) {
            let trash = d.join("_Lixeira");
            std::fs::create_dir_all(&trash).map_err(|e| format!("{}: {e}", trash.display()))?;
            let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "pincel".into());
            let to = store::unique_path(&trash, &stem, brush_pack::EXT_BRUSH);
            std::fs::rename(&f, &to).map_err(|e| format!("{}: {e}", f.display()))?;
        }
        self.items.remove(i);
        self.version += 1;
        Ok(())
    }

    /// Renomeia um pincel do usuário (S25, menu do painel Grupo de ferramentas): grava o arquivo com o nome novo (mesmo
    /// id, mesma categoria) e o antigo vai para `_Lixeira`. Devolve o índice novo (o pincel vai para o fim da lista).
    pub(crate) fn rename_user(&mut self, i: usize, name: &str) -> Result<usize, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err(crate::i18n::tr("brushes.need_name").to_owned());
        }
        if !self.items.get(i).is_some_and(|b| b.user) {
            return Err(crate::i18n::tr("subtool.factory.tip").to_owned());
        }
        if self.items[i].preset.name == name {
            return Ok(i);
        }
        let mut file = self.brush_file(i)?;
        file.preset.name = name.to_owned();
        file.manifest.name = name.to_owned();
        self.remove_user(i)?;
        self.install_file(file, None)
    }

    /// O pincel como arquivo do formato (manifesto que ele já tinha + imagens da biblioteca).
    fn brush_file(&self, i: usize) -> Result<BrushFile, String> {
        let b = self.items.get(i).ok_or("pincel não encontrado")?;
        BrushFile::from_preset(&b.preset, b.manifest.clone(), &|h| self.asset(h))
    }

    /// `.aiaibrush` do pincel `i`, com autor e licença confirmados na exportação.
    pub(crate) fn export_brush(&self, i: usize, meta: &ExportMeta) -> Result<Vec<u8>, String> {
        let mut f = self.brush_file(i)?;
        apply_meta(&mut f, meta);
        brush_pack::write_brush(&f)
    }

    /// `.aiaibrushes` com os pincéis do usuário da categoria `cat`; devolve os bytes e quantos pincéis entraram.
    pub(crate) fn export_category(&self, cat: &str, pack_name: &str, meta: &ExportMeta) -> Result<(Vec<u8>, usize), String> {
        let mut files = Vec::new();
        for (i, b) in self.items.iter().enumerate() {
            if b.user && cat_of(&b.preset) == cat {
                files.push(self.brush_file(i)?);
            }
        }
        if files.is_empty() {
            return Err(crate::i18n::tr("brushes.export.empty").to_owned());
        }
        let mut pack = Manifest::new_pack(pack_name, meta.author.clone(), &meta.license);
        if !meta.attribution.trim().is_empty() {
            pack.license.attribution = Some(meta.attribution.trim().to_owned());
        }
        Ok((brush_pack::write_pack(&pack, &files)?, files.len()))
    }

    /// Pincel de outro programa sem licença conhecida? (a exportação pede o aceite de que pode redistribuir.)
    pub(crate) fn license_unknown(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|b| b.preset.license.trim() == brush_pack::UNKNOWN_LICENSE)
    }

    /// Há pincel do usuário na categoria?
    pub(crate) fn category_has_user(&self, cat: &str) -> bool {
        self.items.iter().any(|b| b.user && cat_of(&b.preset) == cat)
    }
}

/// Autor, licença e atribuição da exportação sobre o pincel.
fn apply_meta(f: &mut BrushFile, meta: &ExportMeta) {
    f.preset.author = meta.author.clone();
    f.preset.license = if meta.license.trim().is_empty() { brush_pack::DEFAULT_LICENSE.to_owned() } else { meta.license.trim().to_owned() };
    let mut lic = if f.manifest.license.to_text() == f.preset.license { f.manifest.license.clone() } else { License::from_text(&f.preset.license) };
    lic.attribution = Some(meta.attribution.trim().to_owned()).filter(|a| !a.is_empty()).or(lic.attribution);
    f.manifest.license = lic;
}

/// Lista de pincéis do `brushes.json` antigo, entrada por entrada: (válidos, entradas inválidas cruas, motivos). Erro só
/// se o arquivo inteiro não é uma lista JSON.
fn parse_user_brushes(bytes: &[u8]) -> Result<(Vec<BrushPreset>, Vec<serde_json::Value>, Vec<String>), String> {
    let list: Vec<serde_json::Value> = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let (mut good, mut bad, mut errs) = (Vec::new(), Vec::new(), Vec::new());
    for (i, v) in list.into_iter().enumerate() {
        let name = v.get("name").and_then(|n| n.as_str()).map(|n| format!("\"{n}\"")).unwrap_or_else(|| format!("nº {}", i + 1));
        match serde_json::from_value::<BrushPreset>(v.clone()).map_err(|e| e.to_string()).and_then(|p| p.validate().map(|_| p)) {
            Ok(p) => good.push(p),
            Err(e) => {
                errs.push(format!("{name}: {e}"));
                bad.push(v);
            }
        }
    }
    Ok((good, bad, errs))
}

/// Categoria do pincel (sem categoria = "meus").
pub(crate) fn cat_of(p: &BrushPreset) -> String {
    if p.category.trim().is_empty() { "meus".to_owned() } else { p.category.trim().to_owned() }
}

/// Nome da categoria para a tela (as de fábrica traduzidas; as do usuário como foram escritas).
pub(crate) fn cat_label(c: &str) -> String {
    CATEGORIES.iter().find(|(k, _)| *k == c).map(|(_, key)| crate::i18n::tr(key).to_owned()).unwrap_or_else(|| c.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_paint::brush::BrushTip;
    use aiai_paint::brush_pack::Package;
    use aiai_paint::mask::Mask;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-brushes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn author() -> BrushAuthorPrefs {
        BrushAuthorPrefs { name: "Ana".into(), email: "ana@exemplo.com".into(), url: String::new(), license: "CC-BY-4.0".into() }
    }

    /// Pincel com ponta de imagem: o preset e a máscara "AIM1" com o hash.
    fn image_preset(name: &str) -> (BrushPreset, Vec<u8>) {
        let mut m = Mask::new(20, 12);
        m.data.iter_mut().enumerate().for_each(|(i, v)| *v = (i * 5 % 256) as u8);
        let aim = m.encode();
        let mut p = BrushPreset::round(name, 30.0, 0.8);
        p.tip = BrushTip::Image { blob: aiai_core::BlobHash::of(&aim), mask: None };
        p.spacing = 0.25;
        (p, aim)
    }

    #[test]
    fn the_factory_library_has_no_eraser_brush_and_no_eraser_category() {
        // S25: as borrachas de fábrica são pincéis do grupo borracha (Borracha dura, macia...), mas o preset "Borracha"
        // da ferramenta não entra na biblioteca e só elas apagam
        let lib = BrushLibrary::open_at(None, None, &author());
        assert!(lib.items.iter().all(|b| b.preset.name != "Borracha"));
        assert!(lib.items.iter().all(|b| (b.preset.mode == aiai_paint::brush::BrushMode::Erase) == (b.preset.group == "borracha")));
        assert!(lib.items.iter().filter(|b| b.preset.mode == aiai_paint::brush::BrushMode::Erase).all(|b| cat_of(&b.preset) == "borracha"));
        // a borracha raster da ferramenta nasce da própria função, não da biblioteca
        let e = crate::app::tools::eraser_preset(&crate::app::editor::ToolOptions::default(), None);
        assert_eq!((e.mode, e.name.as_str()), (aiai_paint::brush::BrushMode::Erase, "Borracha"));
    }

    #[test]
    fn user_brushes_are_aiaibrush_files_that_reload_and_factory_stays() {
        let dir = tmp("lib");
        let mut lib = BrushLibrary::open_at(Some(dir.clone()), None, &author());
        let n0 = lib.items.len();
        assert!(lib.items.iter().all(|b| !b.user && !b.preset.category.is_empty()), "todos os de fábrica têm categoria");
        let mut p = lib.get(0).unwrap().clone();
        p.size = 77.0;
        let i = lib.add_user(p, "Nuvem cirro", "cenario").unwrap();
        assert_eq!(i, n0);
        let f = lib.items[i].file.clone().unwrap();
        assert_eq!(f, dir.join("Cenário").join("Nuvem cirro.aiaibrush"), "um arquivo por pincel, na pasta da categoria");
        let (it, a) = (&lib.items[i].preset, &lib.items[i].preset.author);
        assert_eq!((it.license.as_str(), a.name.as_str(), it.version.as_str()), ("CC-BY-4.0", "Ana", "1.0.0"), "autor e licença do perfil");
        assert!(!it.id.is_empty() && !it.created.is_empty());
        let again = BrushLibrary::open_at(Some(dir.clone()), None, &author());
        assert_eq!(again.items.len(), n0 + 1);
        assert_eq!((again.items[n0].preset.name.as_str(), again.items[n0].preset.size, again.items[n0].user), ("Nuvem cirro", 77.0, true));
        assert_eq!(again.items[n0].preset, lib.items[i].preset);
        // de fábrica não sai; do usuário vai para _Lixeira (recuperável), não é apagado
        lib.remove_user(0).unwrap();
        assert_eq!(lib.items.len(), n0 + 1);
        lib.remove_user(n0).unwrap();
        assert!(!f.exists() && dir.join("_Lixeira").join("Nuvem cirro.aiaibrush").exists());
        assert_eq!(BrushLibrary::open_at(Some(dir.clone()), None, &author()).items.len(), n0);
        // dois pincéis com o mesmo nome não se sobrescrevem
        let p = lib.get(0).unwrap().clone();
        lib.add_user(p.clone(), "Igual", "meus").unwrap();
        lib.add_user(p, "Igual", "meus").unwrap();
        assert_eq!(BrushLibrary::open_at(Some(dir.clone()), None, &author()).items.len(), n0 + 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn without_a_folder_nothing_is_read_or_written() {
        let mut lib = BrushLibrary::open(false, &author());
        assert!(lib.dir().is_none());
        let n0 = lib.items.len();
        let p = lib.get(0).unwrap().clone();
        let i = lib.add_user(p, "Só memória", "meus").unwrap();
        assert!(lib.items[i].file.is_none());
        assert_eq!(lib.rescan(), 0, "sem pasta, reler não faz nada");
        assert_eq!(lib.items.len(), n0 + 1);
        assert_eq!(BrushLibrary::open(false, &author()).items.len(), n0);
    }

    #[test]
    fn corrupt_or_foreign_files_in_the_folder_are_reported_not_lost() {
        let dir = tmp("bad");
        let mut lib = BrushLibrary::open_at(Some(dir.clone()), None, &author());
        let n0 = lib.items.len();
        let p = lib.get(0).unwrap().clone();
        lib.add_user(p, "Boa", "meus").unwrap();
        let good = lib.items[n0].file.clone().unwrap();
        let junk = good.with_file_name("Quebrada.aiaibrush");
        let mut bytes = std::fs::read(&good).unwrap();
        bytes.truncate(bytes.len() / 2);
        std::fs::write(&junk, &bytes).unwrap();
        std::fs::write(good.with_file_name("Lixo.aiaibrush"), b"isto nao e um zip, mas passa de vinte e dois bytes").unwrap();
        let mut lib = BrushLibrary::open_at(Some(dir.clone()), None, &author());
        assert_eq!(lib.items.len(), n0 + 1, "a boa carrega");
        assert_eq!(lib.problems.len(), 2);
        assert!(junk.exists(), "o arquivo ruim continua onde está");
        let w = lib.take_warning().expect("aviso");
        assert!(w.contains("Quebrada.aiaibrush") && w.contains("Lixo.aiaibrush"), "{w}");
        assert!(lib.take_warning().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_brushes_json_migrates_once_and_stays_as_backup() {
        let root = tmp("migrate");
        let appdata = root.join("appdata");
        let dest = root.join("Documentos").join("Pincéis");
        std::fs::create_dir_all(appdata.join("brush_assets")).unwrap();
        let legacy = appdata.join("brushes.json");
        // um pincel com ponta de imagem (a imagem em brush_assets), um importado, um sem imagem, um inválido
        let (img, aim) = image_preset("Estrela de imagem");
        let BrushTip::Image { blob, .. } = img.tip.clone() else { panic!() };
        let blob = &blob;
        std::fs::write(appdata.join("brush_assets").join(format!("{blob}.aim")), &aim).unwrap();
        let mut img = img;
        img.category = "cenario".into();
        let mut imported = BrushPreset::round("Giz do Krita", 20.0, 0.5);
        imported.category = "importados".into();
        imported.source = "giz.bundle › pastel".into();
        let mut plain = BrushPreset::round("Meu lápis", 5.0, 0.9);
        plain.category = "meus".into();
        let bad = serde_json::json!({ "name": "Do futuro", "tip": { "Hexagono": {} } });
        let text = serde_json::to_string_pretty(&serde_json::json!([img, imported, plain, bad])).unwrap();
        std::fs::write(&legacy, &text).unwrap();

        let mut lib = BrushLibrary::open_at(Some(dest.clone()), Some(legacy.clone()), &author());
        let user: Vec<_> = lib.items.iter().filter(|b| b.user).collect();
        assert_eq!(user.len(), 3, "os três válidos viraram arquivos");
        assert!(dest.join("Cenário").join("Estrela de imagem.aiaibrush").is_file());
        assert!(dest.join("Importados").join("giz").join("Giz do Krita.aiaibrush").is_file(), "importado: Importados\\<pacote>");
        assert!(dest.join("Meus pincéis").join("Meu lápis.aiaibrush").is_file());
        let star = user.iter().find(|b| b.preset.name == "Estrela de imagem").unwrap();
        assert!(matches!(&star.preset.tip, BrushTip::Image { blob: b, mask: Some(_) } if b == blob), "a imagem foi junto");
        assert!(lib.has_asset(blob));
        assert_eq!((star.preset.author.name.as_str(), star.preset.license.as_str()), ("Ana", "CC-BY-4.0"));
        let giz = user.iter().find(|b| b.preset.name == "Giz do Krita").unwrap();
        assert_eq!(giz.preset.license, brush_pack::UNKNOWN_LICENSE, "de terceiro: licença desconhecida");
        assert_eq!(giz.manifest.as_ref().unwrap().source.imported_from, "krita-bundle");
        assert!(giz.preset.author.is_empty());
        let w = lib.take_warning().expect("o inválido gera aviso");
        assert!(w.contains("Do futuro"), "{w}");
        // o arquivo antigo está intacto e a migração ficou registrada
        assert_eq!(std::fs::read_to_string(&legacy).unwrap(), text);
        assert!(appdata.join("brush_assets").join(format!("{blob}.aim")).is_file());
        let marker: serde_json::Value = serde_json::from_slice(&std::fs::read(appdata.join("brushes.migrated.json")).unwrap()).unwrap();
        assert_eq!((marker["migrated"].as_u64(), marker["left_in_legacy_file"].as_u64()), (Some(3), Some(1)));
        // reabrir não migra de novo nem duplica
        let again = BrushLibrary::open_at(Some(dest.clone()), Some(legacy.clone()), &author());
        assert_eq!(again.items.iter().filter(|b| b.user).count(), 3);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_legacy_file_keeps_a_copy_and_warns_and_is_not_touched() {
        let root = tmp("migrate-bad");
        let legacy = root.join("brushes.json");
        let garbage = b"[{\"name\": \"Nuvem\", \"size\": 3".to_vec();
        std::fs::write(&legacy, &garbage).unwrap();
        let dest = root.join("Pincéis");
        let mut lib = BrushLibrary::open_at(Some(dest.clone()), Some(legacy.clone()), &author());
        assert!(lib.take_warning().is_some_and(|w| w.contains("brushes.json.corrupt-")), "o usuário é avisado com o caminho da cópia");
        let copies = || std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("brushes.json.corrupt-")).map(|e| e.path()).collect::<Vec<_>>();
        assert_eq!(copies().len(), 1);
        assert_eq!(std::fs::read(&legacy).unwrap(), garbage, "abrir não mexe no original");
        // reabrir não acumula cópias
        let _ = BrushLibrary::open_at(Some(dest), Some(legacy), &author());
        assert_eq!(copies().len(), 1);
        assert_eq!(std::fs::read(&copies()[0]).unwrap(), garbage);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn export_and_import_between_libraries_keep_author_license_and_images() {
        let (da, db) = (tmp("export-a"), tmp("export-b"));
        let mut a = BrushLibrary::open_at(Some(da.clone()), None, &author());
        let (p, aim) = image_preset("Folha");
        a.put_asset(aim.clone()).unwrap();
        let i = a.add_user(p, "Folha", "cenario").unwrap();
        let p2 = a.get(0).unwrap().clone();
        a.add_user(p2.clone(), "Lápis meu", "cenario").unwrap();
        a.add_user(p2, "Outro", "meus").unwrap();
        // exportar um pincel: o diálogo pode trocar autor e licença; o resto do manifesto fica
        let meta = ExportMeta { author: BrushAuthor { name: "Beto".into(), email: "b@x.com".into(), url: "https://b.example".into() }, license: "CC-BY-4.0".into(), attribution: "Beto (b.example)".into() };
        let bytes = a.export_brush(i, &meta).unwrap();
        let Package::Brush(f) = brush_pack::read(&bytes).unwrap() else { panic!() };
        assert_eq!((f.preset.author.name.as_str(), f.preset.license.as_str()), ("Beto", "CC-BY-4.0"));
        assert_eq!(f.manifest.license.attribution.as_deref(), Some("Beto (b.example)"));
        assert_eq!(f.manifest.license.link().as_deref(), Some("https://creativecommons.org/licenses/by/4.0/"));
        assert_eq!(f.preset.id, a.items[i].preset.id, "o id viaja");
        // instalar em outra biblioteca: o pincel e a imagem chegam
        let mut b = BrushLibrary::open_at(Some(db.clone()), None, &BrushAuthorPrefs::default());
        let j = b.install_file(*f, None).unwrap();
        assert!(b.items[j].file.as_ref().unwrap().starts_with(&db));
        assert!(matches!(&b.items[j].preset.tip, BrushTip::Image { blob, .. } if b.has_asset(blob)));
        assert_eq!(b.find_id(&a.items[i].preset.id), Some(j));
        // a categoria inteira como pacote: só os pincéis do usuário dela
        let (pack, n) = a.export_category("cenario", "Cenário da Ana", &meta).unwrap();
        assert_eq!(n, 2);
        let Package::Pack(pk) = brush_pack::read(&pack).unwrap() else { panic!() };
        assert_eq!((pk.brushes.len(), pk.manifest.name.as_str(), pk.manifest.author.name.as_str()), (2, "Cenário da Ana", "Beto"));
        assert!(a.export_category("lapis", "x", &meta).is_err(), "categoria sem pincel do usuário");
        for f in pk.brushes {
            b.install_file(f, Some("Cenário da Ana")).unwrap();
        }
        assert!(db.join("Cenário").join("Cenário da Ana").join("Folha.aiaibrush").exists() || std::fs::read_dir(db.join("Cenário").join("Cenário da Ana")).unwrap().count() == 2);
        let again = BrushLibrary::open_at(Some(db.clone()), None, &BrushAuthorPrefs::default());
        assert_eq!(again.items.iter().filter(|x| x.user).count(), 3);
        let _ = std::fs::remove_dir_all(da);
        let _ = std::fs::remove_dir_all(db);
    }

    #[test]
    fn folder_names_are_safe_and_stable() {
        assert_eq!(cat_folder("cenario"), "Cenário");
        assert_eq!(cat_folder("Minhas/ruins:?"), "Minhas_ruins__");
        assert_eq!(store::sanitize("CON"), "_CON");
        assert_eq!(store::sanitize("  ..  "), "pincel");
        assert_eq!(store::sanitize("nome. "), "nome");
        assert_eq!(imported_from_kind("nuvens.abr"), "photoshop-abr");
        assert_eq!(imported_from_kind("x.brushset › Pasta"), "procreate");
    }
}
