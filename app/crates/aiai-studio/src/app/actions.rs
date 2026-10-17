//! Ações da bridge (SCRIPT, BEATS, STORYBOARD, ANIMATIC, CUTS_FROM_STORYBOARD...) chamadas de dentro do app, no mesmo
//! documento e pelo MESMO caminho que o MCP usa (`handle_core_action`): o que o usuário edita no Roteiro e no Storyboard
//! entra no undo e no log exatamente como a edição de um agente, só com outro autor. As leituras (`script_read`,
//! `storyboard_read`) são puras e passam por um cache por `change_counter`, para o painel não reler o JSON a cada quadro.
//! Também fica aqui a escolha do documento selecionado (roteiro/storyboard) e da cena/elemento/painel, que se corrige
//! sozinha quando o documento muda (desfazer, abrir outro projeto).

use super::commands::NodeId;
use super::doc::AUTHOR;
use super::editor::Editor;
use aiai_bridge::core_ops::handle_core_action;
use aiai_core::{CoreSession, DocumentData, DocumentType, Id, NodeKind, Project};
use aiai_tools::preprod::{self, ScriptDoc, ScriptOp, StoryboardDoc};
use serde_json::{Value, json};
use std::sync::Arc;

/// Executa uma ação da bridge no documento do app, com o autor do usuário. Erro vira texto para o aviso da interface.
pub(crate) fn run(core: &mut CoreSession, action: &str, mut params: Value) -> Result<Value, String> {
    if let Some(o) = params.as_object_mut() {
        o.entry("author").or_insert_with(|| json!(AUTHOR));
    }
    match handle_core_action(core, action, &params) {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => Err(e.message),
        None => Err(format!("ação desconhecida: {action}")),
    }
}

/// Erro de uma ação vira aviso do editor; o sucesso devolve o valor.
pub(crate) fn notify_err<T>(ed: &mut Editor, r: Result<T, String>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            ed.notify(e);
            None
        }
    }
}

/// `SCRIPT {op:"edit"}` com várias operações num único passo de desfazer. Devolve os ids criados (`insert_*`).
pub(crate) fn script_edit(core: &mut CoreSession, doc: &NodeId, ops: Vec<ScriptOp>) -> Result<Vec<Id>, String> {
    let v = run(core, "SCRIPT", json!({ "op": "edit", "doc": doc, "ops": ops }))?;
    Ok(v.get("created").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| Id(s.to_owned()))).collect()).unwrap_or_default())
}

/// `STORYBOARD` com uma operação (`panel_update`, `panel_move`...).
pub(crate) fn storyboard_op(core: &mut CoreSession, op: &str, mut fields: Value) -> Result<Value, String> {
    if let Some(o) = fields.as_object_mut() {
        o.insert("op".into(), json!(op));
    }
    run(core, "STORYBOARD", fields)
}

// ------------------------------------------------------------------------------------------------ documentos

/// O que o app sabe mostrar de um documento do projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocKind {
    Script,
    Storyboard,
    /// Quadro de beats: documento Notes com `schema: "beats"`.
    Beats,
    /// Documento de edição (sequência de cortes, transições, legendas, créditos).
    Edit,
}

pub(crate) fn doc_kind(d: &DocumentData) -> Option<DocKind> {
    match d.doc {
        DocumentType::Script => Some(DocKind::Script),
        DocumentType::Storyboard => Some(DocKind::Storyboard),
        DocumentType::Edit => Some(DocKind::Edit),
        DocumentType::Notes if d.content.get("schema").and_then(Value::as_str) == Some("beats") => Some(DocKind::Beats),
        _ => None,
    }
}

/// Documentos do projeto de um tipo (raiz "Documentos"), na ordem do documento: (id, nome).
pub(crate) fn docs_of(p: &Project, kind: DocKind) -> Vec<(NodeId, String)> {
    p.roots
        .documents
        .iter()
        .filter_map(|id| {
            let n = p.node(id)?;
            match &n.data {
                NodeKind::Document(d) if doc_kind(d) == Some(kind) => Some((id.clone(), n.name.clone())),
                _ => None,
            }
        })
        .collect()
}

/// Personagens do projeto (id, nome) para escolher quem fala.
pub(crate) fn characters(p: &Project) -> Vec<(NodeId, String)> {
    p.roots.characters.iter().filter_map(|id| p.node(id).map(|n| (id.clone(), n.name.clone()))).collect()
}

/// Nome seguinte livre para um documento novo ("Roteiro", "Roteiro 2"...).
pub(crate) fn next_doc_name(p: &Project, kind: DocKind, base: &str) -> String {
    let taken: Vec<String> = docs_of(p, kind).into_iter().map(|(_, n)| n).collect();
    if !taken.iter().any(|n| n == base) {
        return base.to_owned();
    }
    (2..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(n)).unwrap_or_else(|| base.to_owned())
}

// ------------------------------------------------------------------------------------------------ cache de leitura

/// Roteiro e storyboard já decodificados do documento, válidos enquanto o `change_counter` não muda.
#[derive(Default)]
pub(crate) struct DocCache {
    script: Option<(u64, NodeId, Option<Arc<ScriptDoc>>)>,
    story: Option<(u64, NodeId, Option<Arc<StoryboardDoc>>)>,
}

impl DocCache {
    pub(crate) fn script(&mut self, core: &CoreSession, id: &NodeId) -> Option<Arc<ScriptDoc>> {
        if let Some((c, i, d)) = &self.script {
            if *c == core.change_counter && i == id {
                return d.clone();
            }
        }
        let d = preprod::script_read(&core.project, id).ok().map(Arc::new);
        self.script = Some((core.change_counter, id.clone(), d.clone()));
        d
    }

    pub(crate) fn story(&mut self, core: &CoreSession, id: &NodeId) -> Option<Arc<StoryboardDoc>> {
        if let Some((c, i, d)) = &self.story {
            if *c == core.change_counter && i == id {
                return d.clone();
            }
        }
        let d = preprod::storyboard_read(&core.project, id).ok().map(Arc::new);
        self.story = Some((core.change_counter, id.clone(), d.clone()));
        d
    }
}

// ------------------------------------------------------------------------------------------------ seleção

/// Escolhe/corrige o roteiro, a cena e o elemento selecionados. Devolve o documento aberto, se houver roteiro.
pub(crate) fn sync_script_sel(ed: &mut Editor, core: &CoreSession) -> Option<(NodeId, Arc<ScriptDoc>)> {
    let docs = docs_of(&core.project, DocKind::Script);
    let id = pick(ed.sel_script.as_ref(), docs.iter().map(|(i, _)| i))?;
    if ed.sel_script.as_ref() != Some(&id) {
        ed.sel_script = Some(id.clone());
        ed.sel_scene = None;
        ed.sel_element = None;
    }
    let doc = ed.docs.script(core, &id)?;
    if ed.sel_scene.as_ref().is_none_or(|s| doc.scene(s).is_none()) {
        ed.sel_scene = doc.scenes.first().map(|s| s.id.clone());
    }
    if ed.sel_element.as_ref().is_some_and(|e| doc.element(e).is_none()) {
        ed.sel_element = None;
    }
    Some((id, doc))
}

/// Escolhe/corrige o storyboard e o painel selecionados.
pub(crate) fn sync_story_sel(ed: &mut Editor, core: &CoreSession) -> Option<(NodeId, Arc<StoryboardDoc>)> {
    let docs = docs_of(&core.project, DocKind::Storyboard);
    let id = pick(ed.sel_storyboard.as_ref(), docs.iter().map(|(i, _)| i))?;
    if ed.sel_storyboard.as_ref() != Some(&id) {
        ed.sel_storyboard = Some(id.clone());
        ed.sel_panel = None;
    }
    let doc = ed.docs.story(core, &id)?;
    if ed.sel_panel.as_ref().is_none_or(|p| doc.panel(p).is_none()) {
        ed.sel_panel = doc.panels.first().map(|p| p.id.clone());
    }
    Some((id, doc))
}

/// A escolha atual se ainda existir; senão a primeira da lista.
fn pick<'a>(current: Option<&NodeId>, mut all: impl Iterator<Item = &'a NodeId> + Clone) -> Option<NodeId> {
    if let Some(c) = current {
        if all.clone().any(|i| i == c) {
            return Some(c.clone());
        }
    }
    all.next().cloned()
}

/// Segundos estimados de um elemento (explícito ou estimado pelo texto), como o `SCRIPT timing` calcula.
pub(crate) fn element_secs(doc: &ScriptDoc, e: &preprod::ScriptElement) -> f32 {
    preprod::element_seconds(&doc.language, e)
}

/// Segundos de uma cena do roteiro.
pub(crate) fn scene_secs(doc: &ScriptDoc, s: &preprod::ScriptScene) -> f32 {
    s.elements.iter().map(|e| element_secs(doc, e)).sum()
}

/// Texto curto de uma linha (primeira linha, cortada com reticências) para listas.
pub(crate) fn excerpt(s: &str, max_chars: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() <= max_chars {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_tools::preprod::{ElementSpec, SceneSpec};

    fn session_with_script() -> (CoreSession, NodeId) {
        let mut core = CoreSession::new(Project::new("t"));
        core.author = AUTHOR.into();
        let v = run(
            &mut core,
            "SCRIPT",
            json!({ "op": "create", "name": "Roteiro", "title": "Teste", "scenes": [SceneSpec { heading: "INT. SALA - DIA".into(), elements: vec![ElementSpec::action("Ela entra.", "Ela entra.")] }] }),
        )
        .unwrap();
        let id = Id(v["doc"].as_str().unwrap().to_owned());
        (core, id)
    }

    #[test]
    fn run_applies_as_the_app_author_and_goes_to_undo() {
        let (mut core, id) = session_with_script();
        let before = core.change_counter;
        let created = script_edit(&mut core, &id, vec![ScriptOp::InsertScene { index: None, heading: "EXT. RUA - NOITE".into() }]).unwrap();
        assert_eq!(created.len(), 1);
        assert!(core.change_counter > before);
        assert_eq!(preprod::script_read(&core.project, &id).unwrap().scenes.len(), 2);
        assert_eq!(core.log().last().map(|e| e.author.as_str()), Some(AUTHOR));
        core.undo().unwrap();
        assert_eq!(preprod::script_read(&core.project, &id).unwrap().scenes.len(), 1);
    }

    #[test]
    fn errors_come_back_as_text() {
        let mut core = CoreSession::new(Project::new("t"));
        let e = run(&mut core, "SCRIPT", json!({ "op": "read", "doc": "nao-existe" })).unwrap_err();
        assert!(!e.is_empty());
        assert!(run(&mut core, "NAO_EXISTE", json!({})).is_err());
    }

    #[test]
    fn detects_script_storyboard_and_beats_documents() {
        let (mut core, script) = session_with_script();
        run(&mut core, "STORYBOARD", json!({ "op": "create", "name": "Storyboard" })).unwrap();
        run(&mut core, "BEATS", json!({ "op": "create", "name": "Beats" })).unwrap();
        // um Notes comum não é beats
        let p = &core.project;
        assert_eq!(docs_of(p, DocKind::Script), vec![(script.clone(), "Roteiro".to_owned())]);
        assert_eq!(docs_of(p, DocKind::Storyboard).len(), 1);
        assert_eq!(docs_of(p, DocKind::Beats).len(), 1);
        let notes = DocumentData { doc: DocumentType::Notes, content: json!({ "texto": "x" }) };
        assert_eq!(doc_kind(&notes), None);
        assert_eq!(next_doc_name(p, DocKind::Script, "Roteiro"), "Roteiro 2");
        assert_eq!(next_doc_name(p, DocKind::Storyboard, "Outro"), "Outro");
    }

    #[test]
    fn selection_falls_back_to_the_first_valid_item() {
        let (mut core, id) = session_with_script();
        let mut ed = Editor::default();
        let (sel, doc) = sync_script_sel(&mut ed, &core).unwrap();
        assert_eq!(sel, id);
        assert_eq!(ed.sel_scene.as_ref(), doc.scenes.first().map(|s| &s.id));
        // cena que não existe mais volta para a primeira; desfazer a criação do roteiro some com a seleção
        ed.sel_scene = Some(Id("fantasma".into()));
        sync_script_sel(&mut ed, &core).unwrap();
        assert_eq!(ed.sel_scene.as_ref(), doc.scenes.first().map(|s| &s.id));
        core.undo().unwrap();
        assert!(sync_script_sel(&mut ed, &core).is_none());
    }

    #[test]
    fn cache_follows_the_change_counter() {
        let (mut core, id) = session_with_script();
        let mut cache = DocCache::default();
        assert_eq!(cache.script(&core, &id).unwrap().scenes.len(), 1);
        script_edit(&mut core, &id, vec![ScriptOp::InsertScene { index: None, heading: "X".into() }]).unwrap();
        assert_eq!(cache.script(&core, &id).unwrap().scenes.len(), 2);
    }

    #[test]
    fn excerpt_cuts_on_chars_not_bytes() {
        assert_eq!(excerpt("あいうえお\nsegunda", 3), "あい…");
        assert_eq!(excerpt("curto", 10), "curto");
    }
}
