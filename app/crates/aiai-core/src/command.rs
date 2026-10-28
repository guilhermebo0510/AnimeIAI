//! Command Bus. Todo efeito no documento passa por [`Command::apply`], que valida ANTES de mexer, aplica no lugar e
//! devolve o comando inverso (usado pelo undo). Comandos com ids omitidos são resolvidos pela sessão antes de entrar
//! no histórico, para que refazer recrie exatamente os mesmos ids.

use crate::blob::{BlobHash, BlobStore};
use crate::error::{CoreError, CoreResult};
use crate::ids::{Id, IdGen};
use crate::model::*;
use crate::patch;
use crate::stroke::Stroke;
use crate::xsheet::{CellRef, Track};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CellEdit {
    /// Quadro 0-based.
    pub frame: u32,
    /// `None` remove a entrada (o quadro passa a segurar o que vinha antes).
    pub cell: Option<CellRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndexedStroke {
    pub index: usize,
    pub stroke: Stroke,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TileEdit {
    pub x: i32,
    pub y: i32,
    /// `None` apaga o tile.
    pub blob: Option<BlobHash>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Command {
    /// Cria um nó. `id` omitido = gerado. `parent` omitido = nó de raiz (a categoria vem do tipo).
    CreateNode {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<Id>,
        /// Posição entre os irmãos; omitido = no fim.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        name: String,
        data: NodeKind,
    },
    /// Apaga o nó e toda a subárvore; referências externas a ele (máscara, trilha, paleta do personagem) são limpas.
    DeleteNode { id: Id },
    /// Reinsere uma subárvore apagada (inverso de `delete_node`). `nodes[0]` é a raiz da subárvore.
    RestoreNodes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<Id>,
        index: usize,
        nodes: Vec<Node>,
    },
    /// Move o nó para outro pai/posição.
    MoveNode {
        id: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
    },
    /// Edita propriedades de qualquer nó: `name` e/ou um JSON Merge Patch sobre `data` (o campo `type` não muda).
    SetProps {
        id: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Value::is_null")]
        data: Value,
    },
    /// Substitui os dados inteiros de um nó (mesmo tipo).
    ReplaceData { id: Id, data: NodeKind },
    /// Merge Patch sobre `{name, settings, bible}` do projeto.
    SetProjectProps { patch: Value },
    /// Edita células do timesheet de uma trilha.
    SetCells { cut: Id, track: Id, cells: Vec<CellEdit> },
    AddTrack {
        cut: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<usize>,
        track: Track,
    },
    RemoveTrack { cut: Id, track: Id },
    /// Insere traços num cel (índices referem-se à lista final, aplicados em ordem crescente).
    InsertStrokes { cel: Id, items: Vec<IndexedStroke> },
    RemoveStrokes { cel: Id, ids: Vec<Id> },
    /// Substitui traços existentes (casados por id).
    ReplaceStrokes { cel: Id, strokes: Vec<Stroke> },
    /// Troca tiles raster de um cel (os blobs precisam existir no armazenamento).
    SetTiles { cel: Id, tiles: Vec<TileEdit> },
    /// Substitui as regiões de preenchimento de um cel.
    ReplaceFills { cel: Id, fills: Vec<FillRegion> },
    /// Vários comandos como um passo único de undo.
    Batch {
        commands: Vec<Command>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}

/// Resultado de um comando aplicado.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Outcome {
    pub revision: u64,
    pub description: String,
    /// Ids criados (na ordem).
    pub created: Vec<Id>,
    /// Ids tocados.
    pub affected: Vec<Id>,
}

/// Comando aplicado: o que foi feito e como desfazer.
#[derive(Debug, Clone)]
pub struct Applied {
    pub command: Command,
    pub inverse: Command,
    pub created: Vec<Id>,
    pub affected: Vec<Id>,
}

pub struct ApplyCtx<'a> {
    pub blobs: &'a BlobStore,
    pub author: &'a str,
    pub revision: u64,
}

impl Command {
    pub fn description(&self) -> String {
        match self {
            Command::CreateNode { name, data, .. } => format!("Criar {} \"{name}\"", data.type_name()),
            Command::DeleteNode { id } => format!("Apagar {id}"),
            Command::RestoreNodes { nodes, .. } => format!("Restaurar {} nó(s)", nodes.len()),
            Command::MoveNode { id, .. } => format!("Mover {id}"),
            Command::SetProps { id, name, data } => {
                let keys: Vec<String> = data.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                let what = match (name, keys.is_empty()) {
                    (Some(n), true) => format!("nome → \"{n}\""),
                    (Some(n), false) => format!("nome → \"{n}\", {}", keys.join(", ")),
                    (None, _) => keys.join(", "),
                };
                format!("Editar {what} ({id})")
            }
            Command::ReplaceData { id, .. } => format!("Substituir dados de {id}"),
            Command::SetProjectProps { .. } => "Editar projeto".into(),
            Command::SetCells { cells, .. } => format!("Timesheet: {} célula(s)", cells.len()),
            Command::AddTrack { track, .. } => format!("Adicionar trilha {}", track.name),
            Command::RemoveTrack { track, .. } => format!("Remover trilha {track}"),
            Command::InsertStrokes { items, .. } => format!("Desenhar {} traço(s)", items.len()),
            Command::RemoveStrokes { ids, .. } => format!("Apagar {} traço(s)", ids.len()),
            Command::ReplaceStrokes { strokes, .. } => format!("Editar {} traço(s)", strokes.len()),
            Command::SetTiles { tiles, .. } => format!("Pintar {} tile(s)", tiles.len()),
            Command::ReplaceFills { fills, .. } => format!("Preencher ({} região(ões))", fills.len()),
            Command::Batch { label, commands } => label.clone().unwrap_or_else(|| format!("Lote de {} comandos", commands.len())),
        }
    }

    /// Preenche ids omitidos (nós e traços com id vazio) para o comando ficar determinístico no histórico.
    pub fn resolve_ids(&mut self, idgen: &mut IdGen) {
        match self {
            Command::CreateNode { id, data, .. } => {
                if id.is_none() {
                    *id = Some(idgen.next());
                }
                resolve_data_ids(data, idgen);
            }
            Command::ReplaceData { data, .. } => resolve_data_ids(data, idgen),
            Command::AddTrack { track, .. } => {
                if track.id.0.is_empty() {
                    track.id = idgen.next();
                }
            }
            Command::InsertStrokes { items, .. } => {
                for it in items {
                    if it.stroke.id.0.is_empty() {
                        it.stroke.id = idgen.next();
                    }
                }
            }
            Command::ReplaceFills { fills, .. } => {
                for f in fills {
                    if f.id.0.is_empty() {
                        f.id = idgen.next();
                    }
                }
            }
            Command::Batch { commands, .. } => commands.iter_mut().for_each(|c| c.resolve_ids(idgen)),
            _ => {}
        }
    }

    /// Valida e aplica. Em erro, `project` fica intacto (lotes desfazem o que já tinham aplicado).
    pub fn apply(&self, project: &mut Project, ctx: &ApplyCtx) -> CoreResult<Applied> {
        self.apply_keeping_provenance(project, ctx).map(|(a, _)| a)
    }

    /// Como [`Command::apply`], devolvendo também a proveniência que os nós mexidos tinham antes (para um desfazer de
    /// emergência devolver o metadado de auditoria exato, CX11 C11-04).
    pub fn apply_keeping_provenance(&self, project: &mut Project, ctx: &ApplyCtx) -> CoreResult<(Applied, Vec<(Id, crate::model::Provenance)>)> {
        let mut created = Vec::new();
        let mut affected = Vec::new();
        let inverse = self.apply_inner(project, ctx, &mut created, &mut affected)?;
        let mut prior = Vec::with_capacity(affected.len());
        for id in &affected {
            if let Some(n) = project.nodes.get_mut(id) {
                prior.push((id.clone(), n.provenance.clone()));
                n.provenance.modified_by = ctx.author.to_string();
                n.provenance.revision = ctx.revision;
            }
        }
        Ok((Applied { command: self.clone(), inverse, created, affected }, prior))
    }

    fn apply_inner(&self, p: &mut Project, ctx: &ApplyCtx, created: &mut Vec<Id>, affected: &mut Vec<Id>) -> CoreResult<Command> {
        match self {
            Command::CreateNode { id, parent, index, name, data } => {
                let id = id.clone().ok_or_else(|| CoreError::invalid("id", "id não resolvido (use a sessão)"))?;
                if id.0.is_empty() || p.nodes.contains_key(&id) {
                    return Err(CoreError::invalid("id", format!("id vazio ou já existe: {id}")));
                }
                data.validate().map_err(|e| CoreError::invalid("data", e))?;
                check_blobs_in_data(data, ctx.blobs)?;
                if let NodeKind::Cel(c) = data {
                    if let Some(par) = parent {
                        if p.cel_by_number(par, c.number).is_some() {
                            return Err(CoreError::invalid("number", format!("cel {} já existe na camada", c.number)));
                        }
                    }
                }
                let node = Node {
                    id: id.clone(),
                    name: name.clone(),
                    parent: parent.clone(),
                    children: Vec::new(),
                    data: data.clone(),
                    provenance: Provenance { created_by: ctx.author.to_string(), modified_by: ctx.author.to_string(), revision: ctx.revision },
                };
                link(p, &node, *index)?;
                p.nodes.insert(id.clone(), node);
                created.push(id.clone());
                affected.push(id.clone());
                Ok(Command::DeleteNode { id })
            }

            Command::DeleteNode { id } => {
                let node = p.nodes.get(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
                let parent = node.parent.clone();
                let index = sibling_list(p, &node.parent, &node.data)?
                    .iter()
                    .position(|c| c == id)
                    .ok_or_else(|| CoreError::Hierarchy(format!("{id} não está na lista do pai")))?;
                let ids = p.subtree(id);
                let set: std::collections::BTreeSet<Id> = ids.iter().cloned().collect();
                let saved: Vec<Node> = ids.iter().map(|i| p.nodes[i].clone()).collect();
                // Referências externas à subárvore: guardar dados antigos e limpar.
                let mut restore_data = Vec::new();
                // Só camadas, cortes e personagens guardam referências a outros nós (não clonar cels à toa).
                let external: Vec<Id> = p
                    .nodes
                    .iter()
                    .filter(|(k, n)| !set.contains(*k) && matches!(n.data, NodeKind::Layer(_) | NodeKind::Cut(_) | NodeKind::Character(_)))
                    .map(|(k, _)| k.clone())
                    .collect();
                for ext in external {
                    let n = p.nodes.get_mut(&ext).expect("chave existe");
                    let old = n.data.clone();
                    if clean_refs(&mut n.data, &set) {
                        restore_data.push(Command::ReplaceData { id: ext.clone(), data: old });
                        affected.push(ext);
                    }
                }
                unlink(p, id)?;
                for i in &ids {
                    p.nodes.remove(i);
                }
                affected.push(id.clone());
                let restore = Command::RestoreNodes { parent, index, nodes: saved };
                if restore_data.is_empty() {
                    Ok(restore)
                } else {
                    let mut cmds = vec![restore];
                    cmds.extend(restore_data);
                    Ok(Command::Batch { commands: cmds, label: Some("Desfazer apagar".into()) })
                }
            }

            Command::RestoreNodes { parent, index, nodes } => {
                let root = nodes.first().ok_or_else(|| CoreError::invalid("nodes", "lista vazia"))?;
                for n in nodes {
                    if p.nodes.contains_key(&n.id) {
                        return Err(CoreError::invalid("nodes", format!("{} já existe", n.id)));
                    }
                    n.data.validate().map_err(|e| CoreError::invalid("data", e))?;
                }
                if &root.parent != parent {
                    return Err(CoreError::invalid("parent", "pai da raiz não confere"));
                }
                link(p, root, Some(*index))?;
                for n in nodes {
                    p.nodes.insert(n.id.clone(), n.clone());
                    affected.push(n.id.clone());
                }
                created.push(root.id.clone());
                Ok(Command::DeleteNode { id: root.id.clone() })
            }

            Command::MoveNode { id, parent, index } => {
                let node = p.nodes.get(id).ok_or_else(|| CoreError::NotFound(id.clone()))?.clone();
                if let Some(np) = parent {
                    if p.subtree(id).contains(np) {
                        return Err(CoreError::Hierarchy("não dá para mover um nó para dentro dele mesmo".into()));
                    }
                }
                let old_index = sibling_list(p, &node.parent, &node.data)?.iter().position(|c| c == id).unwrap_or(0);
                let old_parent = node.parent.clone();
                unlink(p, id)?;
                let mut moved = node.clone();
                moved.parent = parent.clone();
                if let Err(e) = link(p, &moved, *index) {
                    // devolve ao lugar original
                    link(p, &node, Some(old_index)).expect("posição original válida");
                    return Err(e);
                }
                p.nodes.get_mut(id).expect("existe").parent = parent.clone();
                affected.push(id.clone());
                Ok(Command::MoveNode { id: id.clone(), parent: old_parent, index: Some(old_index) })
            }

            Command::SetProps { id, name, data } => {
                let node = p.nodes.get(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
                let old_name = node.name.clone();
                let mut inv_data = Value::Null;
                let mut new_kind = None;
                if !data.is_null() {
                    if !data.is_object() {
                        return Err(CoreError::invalid("data", "o patch precisa ser um objeto"));
                    }
                    if data.get("type").is_some() {
                        return Err(CoreError::invalid("data.type", "o tipo do nó não pode mudar"));
                    }
                    let old_v = serde_json::to_value(&node.data)?;
                    let mut v = old_v.clone();
                    patch::merge(&mut v, data);
                    let kind: NodeKind = serde_json::from_value(v).map_err(|e| CoreError::invalid("data", e.to_string()))?;
                    kind.validate().map_err(|e| CoreError::invalid("data", e))?;
                    check_blobs_in_data(&kind, ctx.blobs)?;
                    inv_data = patch::inverse(&old_v, data);
                    new_kind = Some(kind);
                }
                // Cel: número continua único na camada.
                if let (Some(NodeKind::Cel(c)), Some(par)) = (&new_kind, &node.parent) {
                    if let Some(other) = p.cel_by_number(par, c.number) {
                        if &other.id != id {
                            return Err(CoreError::invalid("number", format!("cel {} já existe na camada", c.number)));
                        }
                    }
                }
                let n = p.nodes.get_mut(id).expect("existe");
                if let Some(nm) = name {
                    n.name = nm.clone();
                }
                if let Some(k) = new_kind {
                    n.data = k;
                }
                affected.push(id.clone());
                Ok(Command::SetProps { id: id.clone(), name: name.as_ref().map(|_| old_name), data: inv_data })
            }

            Command::ReplaceData { id, data } => {
                let node = p.nodes.get(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
                if node.data.type_name() != data.type_name() {
                    return Err(CoreError::WrongKind { id: id.clone(), expected: node.data.type_name(), found: data.type_name() });
                }
                data.validate().map_err(|e| CoreError::invalid("data", e))?;
                check_blobs_in_data(data, ctx.blobs)?;
                if let (NodeKind::Cel(c), Some(par)) = (data, &node.parent) {
                    if let Some(other) = p.cel_by_number(par, c.number) {
                        if &other.id != id {
                            return Err(CoreError::invalid("number", format!("cel {} já existe na camada", c.number)));
                        }
                    }
                }
                let n = p.nodes.get_mut(id).expect("existe");
                let old = std::mem::replace(&mut n.data, data.clone());
                affected.push(id.clone());
                Ok(Command::ReplaceData { id: id.clone(), data: old })
            }

            Command::SetProjectProps { patch: pt } => {
                if !pt.is_object() {
                    return Err(CoreError::invalid("patch", "o patch precisa ser um objeto"));
                }
                for k in pt.as_object().expect("objeto").keys() {
                    if !matches!(k.as_str(), "name" | "settings" | "bible") {
                        return Err(CoreError::invalid("patch", format!("campo não editável: {k}")));
                    }
                }
                let old_v = serde_json::json!({"name": p.name, "settings": p.settings, "bible": p.bible});
                let mut v = old_v.clone();
                patch::merge(&mut v, pt);
                let name: String = serde_json::from_value(v["name"].clone()).map_err(|e| CoreError::invalid("name", e.to_string()))?;
                let settings: Settings = serde_json::from_value(v["settings"].clone()).map_err(|e| CoreError::invalid("settings", e.to_string()))?;
                let bible: Bible = serde_json::from_value(v["bible"].clone()).map_err(|e| CoreError::invalid("bible", e.to_string()))?;
                if settings.width == 0 || settings.height == 0 || settings.fps_num == 0 || settings.fps_den == 0 {
                    return Err(CoreError::invalid("settings", "resolução e fps precisam ser positivos"));
                }
                p.name = name;
                p.settings = settings;
                p.bible = bible;
                Ok(Command::SetProjectProps { patch: patch::inverse(&old_v, pt) })
            }

            Command::SetCells { cut, track, cells } => {
                let cd = cut_data_mut(p, cut)?;
                let duration = cd.duration;
                let t = cd.xsheet.track_mut(track).ok_or_else(|| CoreError::NotFound(track.clone()))?;
                if let Some(bad) = cells.iter().find(|c| c.frame >= duration) {
                    return Err(CoreError::invalid("frame", format!("quadro {} fora da duração {}", bad.frame, duration)));
                }
                let mut seen = std::collections::BTreeSet::new();
                if cells.iter().any(|c| !seen.insert(c.frame)) {
                    return Err(CoreError::invalid("cells", "quadro repetido no mesmo comando"));
                }
                let mut inv = Vec::with_capacity(cells.len());
                for c in cells {
                    let old = match c.cell {
                        Some(cell) => t.cells.insert(c.frame, cell),
                        None => t.cells.remove(&c.frame),
                    };
                    inv.push(CellEdit { frame: c.frame, cell: old });
                }
                affected.push(cut.clone());
                Ok(Command::SetCells { cut: cut.clone(), track: track.clone(), cells: inv })
            }

            Command::AddTrack { cut, index, track } => {
                if track.id.0.is_empty() {
                    return Err(CoreError::invalid("track.id", "id não resolvido"));
                }
                if let Some(l) = &track.layer {
                    if !p.nodes.contains_key(l) {
                        return Err(CoreError::NotFound(l.clone()));
                    }
                }
                let cd = cut_data_mut(p, cut)?;
                if cd.xsheet.track(&track.id).is_some() {
                    return Err(CoreError::invalid("track.id", "trilha já existe"));
                }
                let i = index.unwrap_or(cd.xsheet.tracks.len()).min(cd.xsheet.tracks.len());
                cd.xsheet.tracks.insert(i, track.clone());
                affected.push(cut.clone());
                Ok(Command::RemoveTrack { cut: cut.clone(), track: track.id.clone() })
            }

            Command::RemoveTrack { cut, track } => {
                let cd = cut_data_mut(p, cut)?;
                let i = cd.xsheet.tracks.iter().position(|t| &t.id == track).ok_or_else(|| CoreError::NotFound(track.clone()))?;
                let t = cd.xsheet.tracks.remove(i);
                affected.push(cut.clone());
                Ok(Command::AddTrack { cut: cut.clone(), index: Some(i), track: t })
            }

            Command::InsertStrokes { cel, items } => {
                let c = cel_data_mut(p, cel)?;
                let mut sorted: Vec<&IndexedStroke> = items.iter().collect();
                sorted.sort_by_key(|i| i.index);
                let mut ids: std::collections::BTreeSet<&Id> = c.strokes.iter().map(|s| &s.id).collect();
                let final_len = c.strokes.len() + items.len();
                for it in &sorted {
                    it.stroke.validate().map_err(|e| CoreError::invalid("stroke", e))?;
                    if it.stroke.id.0.is_empty() || !ids.insert(&it.stroke.id) {
                        return Err(CoreError::invalid("stroke.id", format!("id vazio ou repetido: {}", it.stroke.id)));
                    }
                    if it.index >= final_len {
                        return Err(CoreError::invalid("index", format!("índice {} além do fim ({final_len})", it.index)));
                    }
                }
                for it in sorted {
                    let i = it.index.min(c.strokes.len());
                    c.strokes.insert(i, it.stroke.clone());
                }
                affected.push(cel.clone());
                Ok(Command::RemoveStrokes { cel: cel.clone(), ids: items.iter().map(|i| i.stroke.id.clone()).collect() })
            }

            Command::RemoveStrokes { cel, ids } => {
                let c = cel_data_mut(p, cel)?;
                let mut removed = Vec::new();
                for id in ids {
                    if !c.strokes.iter().any(|s| &s.id == id) {
                        return Err(CoreError::NotFound(id.clone()));
                    }
                }
                // Índices na lista original, para reinserir na mesma ordem.
                let mut positions: Vec<(usize, Stroke)> = c
                    .strokes
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| ids.contains(&s.id))
                    .map(|(i, s)| (i, s.clone()))
                    .collect();
                positions.sort_by_key(|(i, _)| *i);
                c.strokes.retain(|s| !ids.contains(&s.id));
                for (i, s) in positions {
                    removed.push(IndexedStroke { index: i, stroke: s });
                }
                affected.push(cel.clone());
                Ok(Command::InsertStrokes { cel: cel.clone(), items: removed })
            }

            Command::ReplaceStrokes { cel, strokes } => {
                let c = cel_data_mut(p, cel)?;
                for s in strokes {
                    s.validate().map_err(|e| CoreError::invalid("stroke", e))?;
                    if !c.strokes.iter().any(|o| o.id == s.id) {
                        return Err(CoreError::NotFound(s.id.clone()));
                    }
                }
                let mut old = Vec::new();
                for s in strokes {
                    let slot = c.strokes.iter_mut().find(|o| o.id == s.id).expect("conferido");
                    old.push(std::mem::replace(slot, s.clone()));
                }
                affected.push(cel.clone());
                Ok(Command::ReplaceStrokes { cel: cel.clone(), strokes: old })
            }

            Command::SetTiles { cel, tiles } => {
                for t in tiles {
                    if let Some(h) = &t.blob {
                        if !ctx.blobs.known(h) {
                            return Err(CoreError::MissingBlob(h.0.clone()));
                        }
                    }
                }
                let mut seen = std::collections::BTreeSet::new();
                if tiles.iter().any(|t| !seen.insert((t.x, t.y))) {
                    return Err(CoreError::invalid("tiles", "tile repetido no mesmo comando"));
                }
                let c = cel_data_mut(p, cel)?;
                let mut inv = Vec::with_capacity(tiles.len());
                for t in tiles {
                    let pos = c.tiles.iter().position(|e| e.x == t.x && e.y == t.y);
                    let old = pos.map(|i| c.tiles[i].blob.clone());
                    match (&t.blob, pos) {
                        (Some(h), Some(i)) => c.tiles[i].blob = h.clone(),
                        (Some(h), None) => c.tiles.push(TileEntry { x: t.x, y: t.y, blob: h.clone() }),
                        (None, Some(i)) => {
                            c.tiles.remove(i);
                        }
                        (None, None) => {}
                    }
                    inv.push(TileEdit { x: t.x, y: t.y, blob: old });
                }
                c.sort_tiles();
                affected.push(cel.clone());
                Ok(Command::SetTiles { cel: cel.clone(), tiles: inv })
            }

            Command::ReplaceFills { cel, fills } => {
                for f in fills {
                    if f.id.0.is_empty() {
                        return Err(CoreError::invalid("fill.id", "id não resolvido"));
                    }
                    if let Some(h) = &f.mask {
                        if !ctx.blobs.known(h) {
                            return Err(CoreError::MissingBlob(h.0.clone()));
                        }
                    }
                }
                let c = cel_data_mut(p, cel)?;
                let old = std::mem::replace(&mut c.fills, fills.clone());
                affected.push(cel.clone());
                Ok(Command::ReplaceFills { cel: cel.clone(), fills: old })
            }

            Command::Batch { commands, label } => {
                let mut inverses: Vec<Command> = Vec::with_capacity(commands.len());
                for (i, cmd) in commands.iter().enumerate() {
                    match cmd.apply_inner(p, ctx, created, affected) {
                        Ok(inv) => inverses.push(inv),
                        Err(e) => {
                            // Desfaz o que o lote já aplicou, em ordem inversa.
                            for inv in inverses.iter().rev() {
                                let mut c2 = Vec::new();
                                let mut a2 = Vec::new();
                                inv.apply_inner(p, ctx, &mut c2, &mut a2).expect("inverso de comando recém-aplicado não pode falhar");
                            }
                            return Err(CoreError::invalid(format!("batch[{i}]"), e.to_string()));
                        }
                    }
                }
                inverses.reverse();
                Ok(Command::Batch { commands: inverses, label: label.clone() })
            }
        }
    }
}

fn resolve_data_ids(data: &mut NodeKind, idgen: &mut IdGen) {
    match data {
        NodeKind::Cel(c) => {
            for s in &mut c.strokes {
                if s.id.0.is_empty() {
                    s.id = idgen.next();
                }
            }
            for f in &mut c.fills {
                if f.id.0.is_empty() {
                    f.id = idgen.next();
                }
            }
        }
        NodeKind::Palette(pd) => {
            for col in &mut pd.colors {
                if col.id.0.is_empty() {
                    col.id = idgen.next();
                }
            }
        }
        NodeKind::Cut(cd) => {
            for t in &mut cd.xsheet.tracks {
                if t.id.0.is_empty() {
                    t.id = idgen.next();
                }
            }
            for a in &mut cd.audio {
                if a.id.0.is_empty() {
                    a.id = idgen.next();
                }
            }
        }
        _ => {}
    }
}

fn check_blobs_in_data(data: &NodeKind, blobs: &BlobStore) -> CoreResult<()> {
    let mut missing = None;
    match data {
        NodeKind::Cel(c) => {
            for h in c.tiles.iter().map(|t| &t.blob).chain(c.fills.iter().filter_map(|f| f.mask.as_ref())).chain(c.image.iter()) {
                if !blobs.known(h) {
                    missing = Some(h.clone());
                }
            }
        }
        NodeKind::Asset(a) => {
            if let Some(h) = &a.blob {
                if !blobs.known(h) {
                    missing = Some(h.clone());
                }
            }
        }
        _ => {}
    }
    match missing {
        Some(h) => Err(CoreError::MissingBlob(h.0)),
        None => Ok(()),
    }
}

/// Remove referências a ids de `gone` dentro de `data`. Devolve se mudou algo.
fn clean_refs(data: &mut NodeKind, gone: &std::collections::BTreeSet<Id>) -> bool {
    let mut changed = false;
    match data {
        NodeKind::Layer(l) => {
            if l.mask.as_ref().is_some_and(|m| gone.contains(m)) {
                l.mask = None;
                changed = true;
            }
            let before = l.line_refs.len();
            l.line_refs.retain(|r| !gone.contains(r));
            changed |= before != l.line_refs.len();
        }
        NodeKind::Cut(c) => {
            let before = c.xsheet.tracks.len();
            c.xsheet.tracks.retain(|t| !t.layer.as_ref().is_some_and(|l| gone.contains(l)));
            changed |= before != c.xsheet.tracks.len();
            let before_a = c.audio.len();
            c.audio.retain(|a| !gone.contains(&a.asset));
            changed |= before_a != c.audio.len();
            if c.storyboard_panel.as_ref().is_some_and(|s| gone.contains(s)) {
                c.storyboard_panel = None;
                changed = true;
            }
        }
        NodeKind::Character(ch) => {
            if ch.palette.as_ref().is_some_and(|pl| gone.contains(pl)) {
                ch.palette = None;
                changed = true;
            }
            // material cujo corte sumiu sai da pasta; a paleta de um figurino que sumiu só solta a variante
            let before = ch.materials.len();
            ch.materials.retain(|m| !gone.contains(m.content.cut()));
            changed |= before != ch.materials.len();
            for m in &mut ch.materials {
                if m.palette.as_ref().is_some_and(|pl| gone.contains(pl)) {
                    m.palette = None;
                    changed = true;
                }
            }
        }
        _ => {}
    }
    changed
}

fn sibling_list<'a>(p: &'a Project, parent: &Option<Id>, data: &NodeKind) -> CoreResult<&'a Vec<Id>> {
    match parent {
        Some(par) => Ok(&p.nodes.get(par).ok_or_else(|| CoreError::NotFound(par.clone()))?.children),
        None => {
            let cat = data.root_category().ok_or_else(|| CoreError::Hierarchy(format!("{} não pode ser raiz", data.type_name())))?;
            Ok(p.roots.list(cat))
        }
    }
}

/// Insere `node.id` na lista do pai (ou da raiz) validando a hierarquia. Não insere o nó no mapa.
fn link(p: &mut Project, node: &Node, index: Option<usize>) -> CoreResult<()> {
    let list = match &node.parent {
        Some(par) => {
            let pn = p.nodes.get_mut(par).ok_or_else(|| CoreError::NotFound(par.clone()))?;
            if !pn.data.accepts_child(&node.data) {
                return Err(CoreError::Hierarchy(format!("{} não aceita filho {}", pn.data.type_name(), node.data.type_name())));
            }
            &mut pn.children
        }
        None => {
            let cat = node
                .data
                .root_category()
                .ok_or_else(|| CoreError::Hierarchy(format!("{} precisa de pai", node.data.type_name())))?;
            p.roots.list_mut(cat)
        }
    };
    let i = index.unwrap_or(list.len()).min(list.len());
    list.insert(i, node.id.clone());
    Ok(())
}

fn unlink(p: &mut Project, id: &Id) -> CoreResult<()> {
    let node = p.nodes.get(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
    let parent = node.parent.clone();
    let cat = node.data.root_category();
    let list = match parent {
        Some(par) => &mut p.nodes.get_mut(&par).ok_or_else(|| CoreError::NotFound(par.clone()))?.children,
        None => p.roots.list_mut(cat.ok_or_else(|| CoreError::Hierarchy("raiz sem categoria".into()))?),
    };
    list.retain(|c| c != id);
    Ok(())
}

fn cut_data_mut<'a>(p: &'a mut Project, id: &Id) -> CoreResult<&'a mut CutData> {
    let n = p.nodes.get_mut(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
    let found = n.data.type_name();
    match &mut n.data {
        NodeKind::Cut(c) => Ok(c),
        _ => Err(CoreError::WrongKind { id: id.clone(), expected: "cut", found }),
    }
}

fn cel_data_mut<'a>(p: &'a mut Project, id: &Id) -> CoreResult<&'a mut CelData> {
    let n = p.nodes.get_mut(id).ok_or_else(|| CoreError::NotFound(id.clone()))?;
    let found = n.data.type_name();
    match &mut n.data {
        NodeKind::Cel(c) => Ok(c),
        _ => Err(CoreError::WrongKind { id: id.clone(), expected: "cel", found }),
    }
}
