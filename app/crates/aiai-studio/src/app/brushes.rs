//! Biblioteca de pincéis: os de fábrica (motor) e os do usuário (`%APPDATA%\AnimeIAI\brushes.json`), por categoria.
//! O painel Pincéis mostra as categorias com miniaturas desenhadas pelo próprio motor; "Salvar como pincel novo"
//! guarda a cópia ajustada na biblioteca do usuário. O traço grava o preset inteiro no documento, então mexer na
//! biblioteca nunca muda desenho já feito.

use aiai_paint::brush::BrushPreset;
use std::path::PathBuf;

/// Categorias de fábrica (chave, chave i18n), na ordem do painel.
pub(crate) const CATEGORIES: [(&str, &str); 7] = [
    ("lapis", "brushcat.lapis"),
    ("pintura", "brushcat.pintura"),
    ("aquarela", "brushcat.aquarela"),
    ("textura", "brushcat.textura"),
    ("cenario", "brushcat.cenario"),
    ("borracha", "brushcat.borracha"),
    ("meus", "brushcat.meus"),
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
        BrushLibrary { items: BrushPreset::defaults().into_iter().map(|preset| LibBrush { preset, user: false }).collect(), path: None, version: 1 }
    }

    /// Fábrica + os do usuário do arquivo (ausente = nenhum; ilegível = registrado, o app segue com os de fábrica).
    pub(crate) fn load(path: Option<PathBuf>) -> Self {
        let mut lib = Self::factory();
        if let Some(p) = &path {
            match std::fs::read_to_string(p) {
                Ok(text) => match serde_json::from_str::<Vec<BrushPreset>>(&text) {
                    Ok(v) => lib.items.extend(v.into_iter().filter(|b| b.validate().is_ok()).map(|preset| LibBrush { preset, user: true })),
                    Err(e) => tracing::warn!("pincéis do usuário ilegíveis em {}: {e}", p.display()),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!("pincéis do usuário: {}: {e}", p.display()),
            }
        }
        lib.path = path;
        lib
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

    fn save(&self) -> Result<(), String> {
        let Some(p) = &self.path else { return Ok(()) };
        let user: Vec<&BrushPreset> = self.items.iter().filter(|b| b.user).map(|b| &b.preset).collect();
        let text = serde_json::to_string_pretty(&user).map_err(|e| e.to_string())?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, p).map_err(|e| format!("{}: {e}", p.display()))
    }
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
}
