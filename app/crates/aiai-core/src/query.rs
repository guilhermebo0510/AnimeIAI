//! Consultas (leitura). Separadas dos comandos: nunca mudam o documento.

use crate::command::Command;
use crate::error::{CoreError, CoreResult};
use crate::ids::Id;
use crate::model::{NodeKind, RootCategory};
use crate::session::CoreSession;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Query {
    /// Cabeçalho do projeto (nome, configurações, bíblia, raízes, revisão). `include_nodes` = documento inteiro.
    Project {
        #[serde(default)]
        include_nodes: bool,
    },
    /// Um nó completo.
    Node { id: Id },
    /// Árvore resumida (id, nome, tipo, filhos) a partir de um nó ou de todas as raízes; `depth` limita a profundidade.
    Tree {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<u32>,
    },
    /// Ids das raízes de uma categoria.
    Roots { category: RootCategory },
    /// Timesheet de um corte.
    Xsheet { cut: Id },
    /// O que cada trilha expõe no quadro (0-based), com o id do cel resolvido.
    Exposure { cut: Id, frame: u32 },
    /// Mapa de exposição: quadros em que algo muda.
    ChangeFrames { cut: Id },
    /// Log da sessão.
    Log {
        #[serde(default)]
        from: u64,
        #[serde(default = "default_limit")]
        limit: u64,
        #[serde(default)]
        include_commands: bool,
    },
    /// Pilhas de undo/redo.
    History,
    /// JSON Schema: "commands", "queries" ou "project".
    Schema { which: String },
    /// Contagens e tamanhos.
    Stats,
    /// Validação estrutural completa do documento.
    Validate,
}

fn default_limit() -> u64 {
    100
}

pub type QueryResult = Value;

impl Query {
    pub fn run(&self, s: &CoreSession) -> CoreResult<QueryResult> {
        let p = &s.project;
        match self {
            Query::Project { include_nodes } => {
                if *include_nodes {
                    Ok(serde_json::to_value(p)?)
                } else {
                    Ok(json!({
                        "name": p.name, "format": p.format, "revision": p.revision, "settings": p.settings,
                        "bible": p.bible, "roots": p.roots, "node_count": p.nodes.len()
                    }))
                }
            }
            Query::Node { id } => Ok(serde_json::to_value(p.node(id).ok_or_else(|| CoreError::NotFound(id.clone()))?)?),
            Query::Tree { id, depth } => {
                let max = depth.unwrap_or(u32::MAX);
                let outline = |id: &Id| outline(s, id, 0, max);
                match id {
                    Some(id) => outline(id),
                    None => {
                        let mut out = serde_json::Map::new();
                        for (cat, list) in p.roots.all() {
                            let items: CoreResult<Vec<Value>> = list.iter().map(outline).collect();
                            out.insert(serde_json::to_value(cat)?.as_str().unwrap_or("?").to_string(), Value::Array(items?));
                        }
                        Ok(Value::Object(out))
                    }
                }
            }
            Query::Roots { category } => Ok(serde_json::to_value(p.roots.list(*category))?),
            Query::Xsheet { cut } => {
                let c = cut_data(s, cut)?;
                Ok(json!({"duration": c.duration, "xsheet": c.xsheet}))
            }
            Query::Exposure { cut, frame } => {
                let c = cut_data(s, cut)?;
                let tracks: Vec<Value> = c
                    .xsheet
                    .tracks
                    .iter()
                    .map(|t| {
                        let exposed = t.exposed_at(*frame);
                        let cel_id = match (exposed, &t.layer) {
                            (Some(crate::xsheet::CellRef::Cel { number }), Some(l)) => p.cel_by_number(l, number).map(|n| n.id.clone()),
                            _ => None,
                        };
                        json!({"track": t.id, "name": t.name, "layer": t.layer, "exposed": exposed, "cel": cel_id})
                    })
                    .collect();
                Ok(json!({"frame": frame, "tracks": tracks}))
            }
            Query::ChangeFrames { cut } => {
                let c = cut_data(s, cut)?;
                Ok(serde_json::to_value(c.xsheet.change_frames(c.duration))?)
            }
            Query::Log { from, limit, include_commands } => {
                let entries: Vec<Value> = s
                    .log()
                    .iter()
                    .skip(*from as usize)
                    .take((*limit).min(1000) as usize)
                    .map(|e| {
                        let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                        if !include_commands {
                            if let Some(o) = v.as_object_mut() {
                                o.remove("command");
                            }
                        }
                        v
                    })
                    .collect();
                Ok(json!({"total": s.log().len(), "entries": entries}))
            }
            Query::History => {
                let (undo, redo) = s.history();
                Ok(json!({"undo": undo, "redo": redo, "revision": p.revision}))
            }
            Query::Schema { which } => match which.as_str() {
                "commands" => Ok(serde_json::to_value(schemars::schema_for!(Command))?),
                "queries" => Ok(serde_json::to_value(schemars::schema_for!(Query))?),
                "project" => Ok(serde_json::to_value(schemars::schema_for!(crate::model::Project))?),
                other => Err(CoreError::invalid("which", format!("esperado commands|queries|project, veio {other}"))),
            },
            Query::Stats => {
                let mut by_type = std::collections::BTreeMap::<&str, usize>::new();
                let mut strokes = 0usize;
                let mut tiles = 0usize;
                for n in p.nodes.values() {
                    *by_type.entry(n.data.type_name()).or_default() += 1;
                    if let NodeKind::Cel(c) = &n.data {
                        strokes += c.strokes.len();
                        tiles += c.tiles.len();
                    }
                }
                Ok(json!({
                    "revision": p.revision, "nodes": p.nodes.len(), "by_type": by_type, "strokes": strokes, "tiles": tiles,
                    "blobs": s.blobs.len(), "blob_bytes": s.blobs.total_bytes(), "log_entries": s.log().len(),
                    "can_undo": s.can_undo(), "can_redo": s.can_redo()
                }))
            }
            Query::Validate => Ok(match p.validate() {
                Ok(()) => json!({"ok": true}),
                Err(e) => json!({"ok": false, "error": e}),
            }),
        }
    }
}

fn outline(s: &CoreSession, id: &Id, depth: u32, max: u32) -> CoreResult<Value> {
    let n = s.project.node(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
    let mut v = json!({"id": n.id, "name": n.name, "type": n.data.type_name()});
    match &n.data {
        NodeKind::Layer(l) => {
            v["layer_type"] = serde_json::to_value(l.kind)?;
            v["visible"] = json!(l.visible);
        }
        NodeKind::Cel(c) => {
            v["number"] = json!(c.number);
            v["strokes"] = json!(c.strokes.len());
            v["tiles"] = json!(c.tiles.len());
        }
        NodeKind::Cut(c) => {
            v["number"] = json!(c.number);
            v["duration"] = json!(c.duration);
        }
        _ => {}
    }
    if !n.children.is_empty() {
        if depth < max {
            let kids: CoreResult<Vec<Value>> = n.children.iter().map(|c| outline(s, c, depth + 1, max)).collect();
            v["children"] = Value::Array(kids?);
        } else {
            v["child_count"] = json!(n.children.len());
        }
    }
    Ok(v)
}

fn cut_data<'a>(s: &'a CoreSession, id: &Id) -> CoreResult<&'a crate::model::CutData> {
    match s.project.node(id).map(|n| &n.data) {
        Some(NodeKind::Cut(c)) => Ok(c),
        Some(other) => Err(CoreError::WrongKind { id: id.clone(), expected: "cut", found: other.type_name() }),
        None => Err(CoreError::NotFound(id.clone())),
    }
}
