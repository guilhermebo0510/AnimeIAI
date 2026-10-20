//! Biblioteca de pincéis: os de fábrica (motor) e os do usuário (`%APPDATA%\AnimeIAI\brushes.json`), por categoria.
//! O painel Pincéis mostra as categorias com miniaturas desenhadas pelo próprio motor; "Salvar como pincel novo"
//! guarda a cópia ajustada na biblioteca do usuário. O traço grava o preset inteiro no documento, então mexer na
//! biblioteca nunca muda desenho já feito.

use aiai_paint::brush::BrushPreset;
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
}

pub(crate) struct BrushLibrary {
    pub items: Vec<LibBrush>,
    /// Arquivo dos pincéis do usuário (`None` nas execuções sem sessão: testes e capturas não gravam nada).
    path: Option<PathBuf>,
    /// Muda a cada alteração (miniaturas refeitas).
    pub version: u64,
    /// Imagens das pontas e texturas dos pincéis do usuário (máscaras "AIM1" por hash), em
    /// `%APPDATA%\AnimeIAI\brush_assets\<hash>.aim`; também em memória (execuções sem sessão não gravam).
    assets_dir: Option<PathBuf>,
    mem_assets: std::collections::HashMap<aiai_core::BlobHash, Vec<u8>>,
    /// Entradas de `brushes.json` que não deu para entender (versão nova do app, edição à mão): voltam ao arquivo no
    /// próximo salvar, nunca são descartadas.
    kept_raw: Vec<serde_json::Value>,
    /// O arquivo existe mas não pôde ser lido/entendido e ainda não tem cópia guardada: salvar só sobrescreve depois de
    /// guardar a cópia (`brushes.json.corrupt-<data>`).
    unsafe_to_overwrite: Option<String>,
    /// Aviso para o usuário (a UI lê uma vez e mostra na janela de avisos).
    warning: Option<String>,
}

impl Default for BrushLibrary {
    fn default() -> Self {
        Self::factory()
    }
}

/// `%APPDATA%\AnimeIAI\brushes.json`.
pub(crate) fn default_path() -> Option<PathBuf> {
    crate::prefs::default_path().and_then(|p| p.parent().map(|d| d.join("brushes.json")))
}

impl BrushLibrary {
    pub(crate) fn factory() -> Self {
        BrushLibrary {
            items: BrushPreset::defaults().into_iter().map(|preset| LibBrush { preset, user: false }).collect(),
            path: None,
            version: 1,
            assets_dir: None,
            mem_assets: Default::default(),
            kept_raw: Vec::new(),
            unsafe_to_overwrite: None,
            warning: None,
        }
    }

    /// Fábrica + os do usuário do arquivo. Ausente = nenhum. Entrada inválida = pulada (as boas ficam; a ruim volta ao
    /// arquivo ao salvar). Arquivo ilegível ou que não é uma lista = cópia `brushes.json.corrupt-<data>` antes de
    /// qualquer gravação por cima, e aviso ao usuário; o app segue com os de fábrica.
    pub(crate) fn load(path: Option<PathBuf>) -> Self {
        let mut lib = Self::factory();
        if let Some(p) = &path {
            match std::fs::read(p) {
                Ok(bytes) => match parse_user_brushes(&bytes) {
                    Ok((good, bad, errs)) => {
                        lib.items.extend(good.into_iter().map(|preset| LibBrush { preset, user: true }));
                        if !bad.is_empty() {
                            tracing::warn!("pincéis inválidos em {}: {}", p.display(), errs.join("; "));
                            lib.warning = Some(crate::i18n::tr("msg.brushes_skipped").replace("{n}", &bad.len().to_string()).replace("{err}", &errs.join("; ")));
                            lib.kept_raw = bad;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("pincéis do usuário ilegíveis em {}: {e}", p.display());
                        lib.quarantine(p, &e);
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    // ilegível agora (antivírus, permissão): sem os pincéis dele, não pode ser sobrescrito às cegas
                    tracing::warn!("pincéis do usuário: {}: {e}", p.display());
                    lib.unsafe_to_overwrite = Some(e.to_string());
                    lib.warning = Some(crate::i18n::tr("msg.brushes_unreadable").replace("{path}", &p.display().to_string()).replace("{err}", &e.to_string()));
                }
            }
        }
        lib.assets_dir = path.as_ref().and_then(|p| p.parent().map(|d| d.join("brush_assets")));
        lib.path = path;
        lib
    }

    /// Arquivo que não é uma lista de pincéis: guarda a cópia; sem cópia, salvar fica bloqueado.
    fn quarantine(&mut self, p: &std::path::Path, err: &str) {
        match aiai_core::persist::keep_corrupt_copy(p) {
            Ok(copy) => {
                self.unsafe_to_overwrite = None;
                self.warning = Some(crate::i18n::tr("msg.brushes_corrupt").replace("{err}", err).replace("{path}", &copy.display().to_string()));
            }
            Err(e) => {
                self.unsafe_to_overwrite = Some(format!("{err}; cópia falhou: {e}"));
                self.warning = Some(crate::i18n::tr("msg.brushes_unreadable").replace("{path}", &p.display().to_string()).replace("{err}", &format!("{err}; {e}")));
            }
        }
    }

    /// Aviso pendente sobre o arquivo de pincéis (uma vez).
    pub(crate) fn take_warning(&mut self) -> Option<String> {
        self.warning.take()
    }

    /// Guarda a imagem (máscara "AIM1") de uma ponta/textura; devolve o hash que o preset referencia.
    pub(crate) fn put_asset(&mut self, bytes: Vec<u8>) -> Result<aiai_core::BlobHash, String> {
        let h = aiai_core::BlobHash::of(&bytes);
        if let Some(d) = &self.assets_dir {
            let f = d.join(format!("{h}.aim"));
            if !f.exists() {
                std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
                std::fs::write(&f, &bytes).map_err(|e| format!("{}: {e}", f.display()))?;
            }
        }
        self.mem_assets.insert(h.clone(), bytes);
        Ok(h)
    }

    pub(crate) fn has_asset(&self, h: &aiai_core::BlobHash) -> bool {
        self.asset(h).is_some()
    }

    fn asset(&self, h: &aiai_core::BlobHash) -> Option<Vec<u8>> {
        if let Some(b) = self.mem_assets.get(h) {
            return Some(b.clone());
        }
        let d = self.assets_dir.as_ref()?;
        std::fs::read(d.join(format!("{h}.aim"))).ok().filter(|b| aiai_core::BlobHash::of(b) == *h)
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

    /// Guarda uma cópia como pincel novo do usuário; devolve o índice.
    pub(crate) fn add_user(&mut self, mut preset: BrushPreset, name: &str, category: &str) -> Result<usize, String> {
        preset.name = name.trim().to_owned();
        preset.category = category.trim().to_owned();
        if preset.name.is_empty() {
            return Err(crate::i18n::tr("brushes.need_name").to_owned());
        }
        preset.validate()?;
        self.items.push(LibBrush { preset, user: true });
        self.version += 1;
        self.save()?;
        Ok(self.items.len() - 1)
    }

    pub(crate) fn remove_user(&mut self, i: usize) -> Result<(), String> {
        if self.items.get(i).is_some_and(|b| b.user) {
            self.items.remove(i);
            self.version += 1;
            self.save()?;
        }
        Ok(())
    }

    /// Antes de sobrescrever um `brushes.json` que não pôde ser lido na abertura: tenta ler de novo (erro passageiro =
    /// os pincéis dele entram na biblioteca); continua ruim = guarda a cópia; nem isso = recusa salvar.
    fn make_safe_to_overwrite(&mut self, p: &std::path::Path) -> Result<(), String> {
        let Some(why) = self.unsafe_to_overwrite.clone() else { return Ok(()) };
        match std::fs::read(p) {
            Ok(bytes) => match parse_user_brushes(&bytes) {
                Ok((good, bad, _)) => {
                    self.items.extend(good.into_iter().map(|preset| LibBrush { preset, user: true }));
                    self.kept_raw.extend(bad);
                    self.unsafe_to_overwrite = None;
                }
                Err(e) => self.quarantine(p, &e),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.unsafe_to_overwrite = None,
            Err(e) => return Err(format!("{}: {} ({why}; {e})", crate::i18n::tr("msg.brushes_not_saved"), p.display())),
        }
        match &self.unsafe_to_overwrite {
            Some(why) => Err(format!("{}: {} ({why})", crate::i18n::tr("msg.brushes_not_saved"), p.display())),
            None => Ok(()),
        }
    }

    fn save(&mut self) -> Result<(), String> {
        let Some(p) = self.path.clone() else { return Ok(()) };
        self.make_safe_to_overwrite(&p)?;
        let mut all = Vec::new();
        for b in self.items.iter().filter(|b| b.user) {
            all.push(serde_json::to_value(&b.preset).map_err(|e| e.to_string())?);
        }
        all.extend(self.kept_raw.iter().cloned());
        let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        aiai_core::persist::write_file_atomic(&p, text.as_bytes()).map_err(|e| format!("{}: {e}", p.display()))
    }
}

/// Lista de pincéis do `brushes.json`, entrada por entrada: (válidos, entradas inválidas cruas, motivos). Erro só se o
/// arquivo inteiro não é uma lista JSON.
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

    #[test]
    fn user_brushes_round_trip_and_factory_stays() {
        let dir = std::env::temp_dir().join(format!("aiai-brushes-{}", std::process::id()));
        let path = dir.join("brushes.json");
        let mut lib = BrushLibrary::load(Some(path.clone()));
        let n0 = lib.items.len();
        assert!(lib.items.iter().all(|b| !b.user && !b.preset.category.is_empty()), "todos os de fábrica têm categoria");
        let mut p = lib.get(0).unwrap().clone();
        p.size = 77.0;
        let i = lib.add_user(p, "Nuvem cirro", "cenario").unwrap();
        assert_eq!(i, n0);
        let again = BrushLibrary::load(Some(path.clone()));
        assert_eq!(again.items.len(), n0 + 1);
        assert_eq!((again.items[n0].preset.name.as_str(), again.items[n0].preset.size, again.items[n0].user), ("Nuvem cirro", 77.0, true));
        lib.remove_user(0).unwrap(); // de fábrica não sai
        assert_eq!(lib.items.len(), n0 + 1);
        lib.remove_user(n0).unwrap();
        assert_eq!(BrushLibrary::load(Some(path)).items.len(), n0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_brushes_file_is_backed_up_before_save_and_invalid_entries_survive() {
        let dir = std::env::temp_dir().join(format!("aiai-brushes-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("brushes.json");
        let garbage = b"[{\"name\": \"Nuvem\", \"size\": 3".to_vec();
        std::fs::write(&path, &garbage).unwrap();
        let mut lib = BrushLibrary::load(Some(path.clone()));
        let n0 = lib.items.len();
        assert!(lib.take_warning().is_some_and(|w| w.contains("brushes.json.corrupt-")), "o usuário é avisado com o caminho da cópia");
        let copies = || std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("brushes.json.corrupt-")).map(|e| e.path()).collect::<Vec<_>>();
        assert_eq!(copies().len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), garbage, "abrir não mexe no original");
        // reabrir não acumula cópias
        let _ = BrushLibrary::load(Some(path.clone()));
        assert_eq!(copies().len(), 1);
        // salvar sobrescreve só agora, com a cópia guardada
        let p = lib.get(0).unwrap().clone();
        lib.add_user(p, "Novo", "meus").unwrap();
        assert_eq!(std::fs::read(&copies()[0]).unwrap(), garbage);
        assert_eq!(BrushLibrary::load(Some(path.clone())).items.len(), n0 + 1);

        // por entrada: a válida entra, a inválida é pulada com aviso e volta ao arquivo no próximo salvar
        let good = serde_json::to_value(lib.get(0).unwrap()).unwrap();
        let bad = serde_json::json!({ "name": "Do futuro", "tip": { "Hexagono": {} } });
        std::fs::write(&path, serde_json::to_string(&serde_json::json!([good, bad])).unwrap()).unwrap();
        let mut lib = BrushLibrary::load(Some(path.clone()));
        assert_eq!(lib.items.len(), n0 + 1, "a válida fica");
        assert!(lib.take_warning().is_some_and(|w| w.contains("Do futuro")));
        let p = lib.get(0).unwrap().clone();
        lib.add_user(p, "Outro", "meus").unwrap();
        let saved: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.len(), 3);
        assert!(saved.contains(&bad), "a entrada inválida não é descartada");
        let _ = std::fs::remove_dir_all(dir);
    }
}
