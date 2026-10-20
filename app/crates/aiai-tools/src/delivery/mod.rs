//! Entrega (S24 4.8–4.12): o que a fila de exportação, os destinos, os entregáveis, a verificação final e o pacote de
//! entrega precisam além do motor de vídeo, sem interface:
//!
//! * [`formats`] — predefinições (YouTube 1080p, master, prévia pequena, H.265, WebM, sequência PNG, WAV, stems, folha
//!   de contato) e a disponibilidade de cada uma neste PC;
//! * [`names`] — modelos de nome (`{projeto}`, `{corte}`...), nome seguro e versão automática;
//! * [`store`] — fila persistida no projeto (`entrega.json`), destinos e biblioteca de entregáveis;
//! * [`preflight`] — a verificação final antes de exportar (semáforo com links para corrigir);
//! * [`poster`] — miniatura, pôster com título e folha de contato;
//! * [`mix`] — mixagem da fonte (para medir loudness e para os stems);
//! * [`package`] — o pacote de entrega (zip com vídeo, legendas, créditos, pôster e checksums SHA-256).
//!
//! Quem exporta de fato é o `EXPORT_VIDEO` da bridge (`aiai_bridge::delivery_tools` junta tudo e roda os itens da fila).

pub mod formats;
pub mod hash;
pub mod mix;
pub mod names;
pub mod package;
pub mod poster;
pub mod preflight;
pub mod store;

use crate::context::{ToolError, ToolResult, cut_data, node};
use crate::edit::{ItemSource, load_edit};
use aiai_core::{Id, NodeKind, Project};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// O que exportar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Edit,
    Episode,
    Cut,
}

/// Fonte de uma exportação: uma edição, um episódio (todos os cortes em ordem) ou um corte (com trecho opcional).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub id: Id,
    /// Só em corte: primeiro quadro (padrão 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    /// Só em corte: quadro seguinte ao último (padrão: a duração do corte).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
}

/// O que se sabe de uma fonte para nomear e avisar.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceInfo {
    /// Nome para mostrar e para `{fonte}` (nome da edição/episódio ou `C003`).
    pub name: String,
    pub episode: String,
    /// `C003` no corte; vazio em episódio e edição.
    pub cut: String,
    pub frames: u32,
}

impl SourceRef {
    pub fn edit(id: Id) -> Self {
        SourceRef { kind: SourceKind::Edit, id, start: None, end: None }
    }
    pub fn episode(id: Id) -> Self {
        SourceRef { kind: SourceKind::Episode, id, start: None, end: None }
    }
    pub fn cut(id: Id) -> Self {
        SourceRef { kind: SourceKind::Cut, id, start: None, end: None }
    }

    /// `{edit}`, `{episode}` ou `{cut, start?, end?}` do `EXPORT_VIDEO`.
    pub fn params(&self) -> Value {
        match self.kind {
            SourceKind::Edit => json!({ "edit": self.id }),
            SourceKind::Episode => json!({ "episode": self.id }),
            SourceKind::Cut => {
                let mut v = json!({ "cut": self.id });
                if let Some(s) = self.start {
                    v["start"] = json!(s);
                }
                if let Some(e) = self.end {
                    v["end"] = json!(e);
                }
                v
            }
        }
    }

    /// Cortes que a fonte exporta, em ordem, com o intervalo [início, fim) de cada um. Edição = vazio (ela tem a sua linha do tempo).
    pub fn segments(&self, p: &Project) -> ToolResult<Vec<(Id, u32, u32)>> {
        match self.kind {
            SourceKind::Edit => Ok(Vec::new()),
            SourceKind::Cut => {
                let dur = cut_data(p, &self.id)?.duration;
                let (s, e) = (self.start.unwrap_or(0), self.end.unwrap_or(dur));
                if s >= e || e > dur {
                    return Err(ToolError::Invalid(format!("intervalo [{s}, {e}) inválido: o corte tem {dur} quadros")));
                }
                Ok(vec![(self.id.clone(), s, e)])
            }
            SourceKind::Episode => {
                let n = node(p, &self.id)?;
                if !matches!(n.data, NodeKind::Episode(_)) {
                    return Err(ToolError::Invalid(format!("{} é {}, não episódio", self.id, n.data.type_name())));
                }
                let mut out = Vec::new();
                for sc in &n.children {
                    let Some(scene) = p.node(sc) else { continue };
                    if !matches!(scene.data, NodeKind::Scene(_)) {
                        continue;
                    }
                    for c in &scene.children {
                        if let Some(NodeKind::Cut(cd)) = p.node(c).map(|n| &n.data) {
                            out.push((c.clone(), 0, cd.duration));
                        }
                    }
                }
                if out.is_empty() {
                    return Err(ToolError::Invalid(format!("o episódio {} não tem cortes", self.id)));
                }
                Ok(out)
            }
        }
    }

    /// Cortes que a fonte usa (na edição: os cortes das trilhas de imagem e de som).
    pub fn used_cuts(&self, p: &Project) -> ToolResult<Vec<Id>> {
        match self.kind {
            SourceKind::Edit => {
                let doc = load_edit(p, &self.id)?;
                let mut out: Vec<Id> = Vec::new();
                for it in &doc.items {
                    if let ItemSource::Cut { cut } | ItemSource::CutAudio { cut, .. } = &it.source {
                        if !out.contains(cut) {
                            out.push(cut.clone());
                        }
                    }
                }
                Ok(out)
            }
            _ => Ok(self.segments(p)?.into_iter().map(|(c, _, _)| c).collect()),
        }
    }

    /// Duração total em quadros.
    pub fn total_frames(&self, p: &Project) -> ToolResult<u32> {
        match self.kind {
            SourceKind::Edit => {
                let doc = load_edit(p, &self.id)?;
                Ok(doc.timeline(p, p.settings.fps() as f32)?.total)
            }
            _ => Ok(self.segments(p)?.iter().map(|(_, s, e)| e - s).sum()),
        }
    }

    /// Nome, episódio, rótulo do corte e duração. Fonte que não existe é erro.
    pub fn info(&self, p: &Project) -> ToolResult<SourceInfo> {
        let n = node(p, &self.id)?;
        let episode_of = |id: &Id| p.ancestors(id).into_iter().filter_map(|a| p.node(&a)).find(|a| matches!(a.data, NodeKind::Episode(_))).map(|a| a.name.clone()).unwrap_or_default();
        let frames = self.total_frames(p)?;
        Ok(match (&n.data, self.kind) {
            (NodeKind::Cut(cd), SourceKind::Cut) => SourceInfo { name: format!("C{:03}", cd.number), episode: episode_of(&self.id), cut: format!("C{:03}", cd.number), frames },
            (_, SourceKind::Episode) => SourceInfo { name: n.name.clone(), episode: n.name.clone(), cut: String::new(), frames },
            (_, SourceKind::Edit) => SourceInfo { name: n.name.clone(), episode: String::new(), cut: String::new(), frames },
            _ => return Err(ToolError::Invalid(format!("{} não é um corte", self.id))),
        })
    }
}

/// Valor de `NameCtx` para uma fonte e um formato num instante.
pub fn name_ctx(p: &Project, info: &SourceInfo, format_label: &str, now_ms: u64) -> names::NameCtx {
    let (date, time) = names::local_date_time_of(now_ms);
    names::NameCtx { project: p.name.clone(), episode: info.episode.clone(), cut: info.cut.clone(), format: format_label.to_owned(), date, time, source: info.name.clone() }
}

#[cfg(test)]
pub(crate) mod testkit {
    use crate::background::{SkyRequest, sky_command};
    use aiai_core::*;
    use serde_json::json;

    /// Instante (ms desde a época Unix, UTC) de uma data e hora.
    pub fn civil_ms(y: i64, m: u32, d: u32, h: u64, min: u64) -> u64 {
        crate::production_history::days_from_civil(y, m, d) as u64 * 86_400_000 + (h * 3600 + min * 60) * 1000
    }

    fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
        s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
    }

    /// Sessão pequena (64×36, 24 fps) com episódio > cena > corte (céu pintado) de `frames` quadros.
    /// Devolve a sessão, o episódio e o corte.
    pub fn session_with_cut(frames: u32) -> (CoreSession, Id, Id) {
        let mut s = CoreSession::for_tests("Entrega");
        s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 64, "height": 36}}) }).unwrap();
        let ep = create(&mut s, None, "Ep 1", NodeKind::Episode(EpisodeData::default()));
        let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(frames)));
        let l = create(&mut s, Some(&cut), "BG", NodeKind::Layer(LayerData::new(LayerType::Raster)));
        create(&mut s, Some(&l), "1", NodeKind::Cel(CelData::new(1)));
        let spec = aiai_paint::texture::SkySpec { time_of_day: "day".into(), zenith: None, horizon: None, horizon_y: 20.0, clouds: 0.4, cloud_scale: 20.0, seed: 3 };
        let req = SkyRequest { cut: cut.clone(), frame: 0, layer: l, sky: spec };
        let c = sky_command(&s.project, &mut s.blobs, &req).unwrap();
        s.apply(c).unwrap();
        (s, ep, cut)
    }
}
