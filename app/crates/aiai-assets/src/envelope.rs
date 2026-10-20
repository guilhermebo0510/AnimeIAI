//! Envelope de manifesto comum a todos os recursos (S24 formatos (e)): o mesmo desenho do manifesto do pincel
//! (`aiai_paint::brush_pack::Manifest`: autor, licença, fonte, versão, datas, etiquetas, `extra`, campos futuros
//! preservados) com `kind` e um `payload` por tipo. O pincel continua no seu `.aiaibrush`; aqui ele só é lido para o
//! índice (`from_brush_manifest`) e regravado pelo próprio `brush_pack`.
//!
//! Onde o envelope mora:
//! - arquivos de terceiros (`.glb`, `.png`...): ao lado, em `<arquivo>.aiaiasset.json` (sidecar);
//! - formatos próprios em JSON (`.aiaipal`, `.aiaimat`, `.aiaipose`): o envelope é o próprio arquivo;
//! - pincel: dentro do zip do `.aiaibrush`.

use crate::kind::Kind;
use aiai_paint::brush::BrushAuthor;
use aiai_paint::brush_pack::{self, License, Manifest, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub use aiai_paint::brush::BrushAuthor as Author;
pub use aiai_paint::brush_pack::{DEFAULT_LICENSE, UNKNOWN_LICENSE};

pub const FORMAT: &str = "animeiai-asset";
pub const FORMAT_VERSION: u32 = 1;
/// Maior `min_reader` que este leitor entende (arquivo mais novo é recusado com o motivo).
pub const READER_VERSION: u32 = 1;
/// Sufixo do arquivo de manifesto ao lado de um arquivo de terceiros.
pub const SIDECAR_SUFFIX: &str = ".aiaiasset.json";
/// Maior manifesto lido (um manifesto de verdade tem poucos KB).
pub const MAX_MANIFEST: u64 = 4 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileRef {
    pub path: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub blake3: String,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub min_reader: u32,
    pub kind: Kind,
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileRef>,
    /// Dados do tipo (cores da paleta, informações do modelo, parâmetros do material, a pose...).
    #[serde(default)]
    pub payload: Value,
    /// Campos do usuário/de outras ferramentas: preservados ao regravar.
    #[serde(default)]
    pub extra: BTreeMap<String, Value>,
    /// Campos de versões futuras que este leitor não conhece (também preservados).
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Envelope {
    pub fn new(kind: Kind, name: &str) -> Envelope {
        let now = brush_pack::now_iso();
        Envelope {
            format: FORMAT.to_owned(),
            format_version: FORMAT_VERSION,
            min_reader: FORMAT_VERSION,
            kind,
            id: brush_pack::new_id(),
            name: name.trim().to_owned(),
            description: String::new(),
            author: BrushAuthor::default(),
            license: License::default(),
            source: Source::default(),
            version: "1.0.0".to_owned(),
            created: now.clone(),
            modified: now,
            app_version: format!("AnimeIAI {}", env!("CARGO_PKG_VERSION")),
            category: String::new(),
            tags: Vec::new(),
            thumbnail: None,
            files: Vec::new(),
            payload: Value::Null,
            extra: BTreeMap::new(),
            unknown: BTreeMap::new(),
        }
    }

    /// Lê e confere um manifesto: formato, `min_reader` e campos obrigatórios.
    pub fn from_json(bytes: &[u8]) -> Result<Envelope, String> {
        if bytes.len() as u64 > MAX_MANIFEST {
            return Err(format!("manifesto grande demais ({} KB)", bytes.len() >> 10));
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("JSON inválido: {e}"))?;
        Self::from_value(v)
    }

    pub fn from_value(v: Value) -> Result<Envelope, String> {
        match v.get("format").and_then(Value::as_str) {
            Some(FORMAT) => {}
            Some(other) => return Err(format!("formato '{other}' não é um recurso do AnimeIAI ('{FORMAT}')")),
            None => return Err("falta o campo `format` (não é um manifesto de recurso do AnimeIAI)".into()),
        }
        if let Some(min) = v.get("min_reader").and_then(Value::as_u64) {
            if min > READER_VERSION as u64 {
                return Err(format!("o arquivo exige a versão {min} do leitor (este app entende até a {READER_VERSION}); atualize o AnimeIAI"));
            }
        }
        let mut e: Envelope = serde_json::from_value(v).map_err(|e| format!("manifesto inválido: {e}"))?;
        if e.id.trim().is_empty() || e.id.len() > 64 || !e.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
            return Err("manifesto com `id` ausente ou inválido".into());
        }
        e.name = e.name.trim().to_owned();
        if e.name.is_empty() {
            return Err("manifesto sem `name`".into());
        }
        Ok(e)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(self).map_err(|e| e.to_string())
    }

    /// Marca a modificação (data nova).
    pub fn touch(&mut self) {
        self.modified = brush_pack::now_iso();
    }

    /// Envelope a partir do manifesto de um `.aiaibrush` (só para o índice; o arquivo continua sendo do `brush_pack`).
    pub fn from_brush_manifest(m: &Manifest) -> Envelope {
        let mut e = Envelope::new(Kind::Brush, &m.name);
        e.format_version = m.format_version;
        e.min_reader = m.min_reader;
        e.id = m.id.clone();
        e.description = m.description.clone();
        e.author = m.author.clone();
        e.license = m.license.clone();
        e.source = m.source.clone();
        e.version = m.version.clone();
        e.created = m.created.clone();
        e.modified = m.modified.clone();
        e.app_version = m.app_version.clone();
        e.category = m.category.clone();
        e.tags = m.tags.clone();
        e.extra = m.extra.clone();
        e.unknown = BTreeMap::new();
        e
    }

    /// A licença como o usuário vê (vazia = não informada).
    pub fn license_text(&self) -> String {
        self.license.to_text()
    }

    /// Texto da licença → licença (id SPDX conhecido, texto padrão proprietária ou texto livre).
    pub fn set_license_text(&mut self, text: &str) {
        let attribution = self.license.attribution.clone();
        let url = self.license.url.clone();
        self.license = License::from_text(text);
        if self.license.attribution.is_none() {
            self.license.attribution = attribution;
        }
        if self.license.url.is_none() {
            self.license.url = url.filter(|_| !text.trim().is_empty());
        }
    }

    /// Texto da fonte (arquivo de origem ou endereço).
    pub fn source_text(&self) -> String {
        self.source.text()
    }

    pub fn set_source_text(&mut self, t: &str) {
        let t = t.trim();
        if t.starts_with("http://") || t.starts_with("https://") {
            self.source.url = t.to_owned();
            self.source.original_file.clear();
        } else {
            self.source.original_file = t.to_owned();
            self.source.url.clear();
        }
    }
}

/// Etiquetas "a, b ; c" → lista sem repetição (sem acento/caixa para comparar, grafia do primeiro mantida).
pub fn parse_tags(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split([',', ';', '\n']) {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|o| fold(o) == fold(t)) {
            out.push(t.chars().take(40).collect());
        }
    }
    out.truncate(64);
    out
}

/// Minúsculas e sem acento (busca e comparação de etiquetas).
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_extra_and_unknown_fields() {
        let mut e = Envelope::new(Kind::Palette, "Pele");
        e.tags = vec!["pele".into()];
        e.extra.insert("nota".into(), serde_json::json!("minha"));
        e.set_license_text("CC0-1.0");
        e.set_source_text("https://exemplo.org/pele");
        e.payload = serde_json::json!({"colors": []});
        let mut v: Value = serde_json::from_slice(&e.to_json().unwrap()).unwrap();
        v["campo_do_futuro"] = serde_json::json!({"a": 1});
        let back = Envelope::from_json(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(back.kind, Kind::Palette);
        assert_eq!(back.extra["nota"], "minha");
        assert_eq!(back.unknown["campo_do_futuro"]["a"], 1);
        assert_eq!(back.license.spdx.as_deref(), Some("CC0-1.0"));
        assert_eq!(back.source.url, "https://exemplo.org/pele");
        // regravar mantém o campo futuro
        let again: Value = serde_json::from_slice(&back.to_json().unwrap()).unwrap();
        assert_eq!(again["campo_do_futuro"]["a"], 1);
    }

    #[test]
    fn refuses_wrong_format_newer_reader_and_missing_fields() {
        let e = Envelope::new(Kind::Texture, "Papel");
        let ok = serde_json::to_value(&e).unwrap();
        assert!(Envelope::from_value(ok.clone()).is_ok());
        let mut v = ok.clone();
        v["format"] = "outro".into();
        assert!(Envelope::from_value(v).unwrap_err().contains("não é um recurso"));
        let mut v = ok.clone();
        v["min_reader"] = 9.into();
        assert!(Envelope::from_value(v).unwrap_err().contains("atualize"));
        let mut v = ok.clone();
        v["id"] = "".into();
        assert!(Envelope::from_value(v).is_err());
        let mut v = ok;
        v["name"] = "  ".into();
        assert!(Envelope::from_value(v).is_err());
        assert!(Envelope::from_json(b"nao e json").is_err());
    }

    #[test]
    fn tags_parse_dedup_and_fold() {
        assert_eq!(parse_tags("Pele, cabelo; PELE\n  céu "), vec!["Pele", "cabelo", "céu"]);
        assert_eq!(fold("Ação Céu"), "acao ceu");
    }

    #[test]
    fn brush_manifest_maps_into_the_envelope() {
        let m = Manifest::new_pack("Pacote", BrushAuthor { name: "Ana".into(), ..Default::default() }, "CC0-1.0");
        let e = Envelope::from_brush_manifest(&m);
        assert_eq!((e.kind, e.id.as_str(), e.name.as_str()), (Kind::Brush, m.id.as_str(), "Pacote"));
        assert_eq!(e.author.name, "Ana");
        assert_eq!(e.license_text(), "CC0-1.0");
    }
}
