//! Tipos de recurso da biblioteca e a pasta de cada um em `Documentos\AnimeIAI`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Model3d,
    Material,
    Texture,
    Palette,
    Pose,
    Brush,
}

impl Kind {
    /// Ordem da árvore da biblioteca.
    pub const ALL: [Kind; 6] = [Kind::Model3d, Kind::Material, Kind::Texture, Kind::Palette, Kind::Pose, Kind::Brush];

    /// Chave estável (manifesto, bridge/MCP, `i18n`).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Model3d => "model3d",
            Kind::Material => "material",
            Kind::Texture => "texture",
            Kind::Palette => "palette",
            Kind::Pose => "pose",
            Kind::Brush => "brush",
        }
    }

    pub fn from_key(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == s.trim().to_ascii_lowercase())
    }

    /// Pasta do tipo dentro de `Documentos\AnimeIAI` (nomes em português, estáveis ao trocar o idioma do app).
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Model3d => "Modelos 3D",
            Kind::Material => "Materiais",
            Kind::Texture => "Texturas",
            Kind::Palette => "Paletas",
            Kind::Pose => "Poses",
            Kind::Brush => "Pincéis",
        }
    }

    /// Extensão do arquivo próprio do tipo (formatos de terceiros, como `.glb` ou `.png`, não têm).
    pub fn own_ext(self) -> Option<&'static str> {
        match self {
            Kind::Model3d | Kind::Texture => None,
            Kind::Material => Some("aiaimat"),
            Kind::Palette => Some("aiaipal"),
            Kind::Pose => Some("aiaipose"),
            Kind::Brush => Some("aiaibrush"),
        }
    }
}

/// Extensões de imagem que a biblioteca lê (textura e material de imagem).
pub const IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];
/// Extensões de modelo 3D (o `.vrm` é recusado pelo importador com "em breve").
pub const MODEL_EXTS: [&str; 3] = ["glb", "gltf", "obj"];

pub fn ext_of(path: &std::path::Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_folders_are_distinct() {
        let mut seen = std::collections::HashSet::new();
        for k in Kind::ALL {
            assert_eq!(Kind::from_key(k.key()), Some(k));
            assert!(seen.insert(k.folder()));
            assert_eq!(serde_json::to_string(&k).unwrap(), format!("\"{}\"", k.key()));
        }
        assert_eq!(Kind::from_key(" Model3D "), Some(Kind::Model3d));
        assert_eq!(Kind::from_key("fonte"), None);
    }
}
