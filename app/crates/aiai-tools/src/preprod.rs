//! S16 — pré-produção: roteiro (JA + tradução PT), beats, storyboard/e-conte, animatic e geração de cortes.
//!
//! Esquemas (tipos Rust serializados em `DocumentData.content`):
//! - **Roteiro** (`DocumentType::Script`): [`ScriptDoc`] — cenas com elementos (ação, fala, parentético, transição, nota).
//!   Cada elemento guarda o texto original, a tradução, emoção e direção, e a estimativa de segundos.
//! - **Beats** (`DocumentType::Notes`, `schema: "beats"`): [`BeatBoard`] — escolha de projeto: um documento Notes
//!   próprio (e não um campo do roteiro) para que a estrutura possa ser editada sem tocar nas falas e para que um
//!   projeto possa ter vários quadros de beats (versões do outline).
//! - **Storyboard / e-conte** (`DocumentType::Storyboard`): [`StoryboardDoc`] — painéis; o desenho de cada painel é um
//!   Cut pequeno (480x270) filho do documento, com camada vetorial "rascunho".
//!
//! Toda ferramenta lê o projeto e devolve **comandos**; quem chama aplica pela sessão (undo, log, proveniência).

use crate::context::*;
use crate::fx2d::{default_font, heavy_font};
use aiai_core::*;
use aiai_paint::poly::fill_polygons;
use aiai_paint::{Affine, Image, transform_image};
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ================================================================ utilidades de documento

pub(crate) fn doc_content<T: DeserializeOwned>(p: &Project, id: &Id, want: DocumentType) -> ToolResult<T> {
    match &node(p, id)?.data {
        NodeKind::Document(d) if d.doc == want => {
            serde_json::from_value(d.content.clone()).map_err(|e| ToolError::Invalid(format!("conteúdo do documento {id} inválido: {e}")))
        }
        NodeKind::Document(d) => Err(ToolError::Invalid(format!("{id} é documento {:?}, esperado {want:?}", d.doc))),
        other => Err(ToolError::Invalid(format!("{id} é {}, não documento", other.type_name()))),
    }
}

pub(crate) fn replace_doc_command<T: Serialize>(p: &Project, id: &Id, content: &T) -> ToolResult<Command> {
    let NodeKind::Document(d) = &node(p, id)?.data else {
        return Err(ToolError::Invalid(format!("{id} não é documento")));
    };
    let mut d = d.clone();
    d.content = serde_json::to_value(content).map_err(|e| ToolError::Invalid(e.to_string()))?;
    Ok(Command::ReplaceData { id: id.clone(), data: NodeKind::Document(d) })
}

pub(crate) fn create_doc_command<T: Serialize>(id: &Id, parent: Option<&Id>, name: &str, doc: DocumentType, content: &T) -> ToolResult<Command> {
    let content = serde_json::to_value(content).map_err(|e| ToolError::Invalid(e.to_string()))?;
    Ok(Command::CreateNode { id: Some(id.clone()), parent: parent.cloned(), index: None, name: name.into(), data: NodeKind::Document(DocumentData { doc, content }) })
}

/// Resultado de uma criação de documento.
#[derive(Debug, Clone)]
pub struct Created {
    pub id: Id,
    pub command: Command,
}

// ================================================================ roteiro

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    #[default]
    Action,
    Dialogue,
    Parenthetical,
    Transition,
    Note,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ScriptElement {
    pub id: Id,
    #[serde(default)]
    pub kind: ElementKind,
    /// Personagem que fala (nó Character do projeto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<Id>,
    /// Nome do falante quando não há personagem correspondente no projeto (importação de Fountain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// Texto original (idioma do roteiro, ex.: JA).
    #[serde(default)]
    pub text: String,
    /// Tradução (idioma de tradução, ex.: PT-BR).
    #[serde(default)]
    pub translation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Duração estimada explícita; `None` = estimar pelo texto (ver [`estimate_timing`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est_seconds: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ScriptScene {
    pub id: Id,
    /// Cabeçalho "INT. LUGAR — HORA".
    #[serde(default)]
    pub heading: String,
    #[serde(default)]
    pub elements: Vec<ScriptElement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScriptDoc {
    #[serde(default)]
    pub title: String,
    #[serde(default = "d_ja")]
    pub language: String,
    #[serde(default = "d_pt")]
    pub translation_language: String,
    #[serde(default)]
    pub scenes: Vec<ScriptScene>,
}

fn d_ja() -> String {
    "ja".into()
}
fn d_pt() -> String {
    "pt-BR".into()
}

impl Default for ScriptDoc {
    fn default() -> Self {
        ScriptDoc { title: String::new(), language: d_ja(), translation_language: d_pt(), scenes: Vec::new() }
    }
}

impl ScriptDoc {
    pub fn element(&self, id: &Id) -> Option<&ScriptElement> {
        self.scenes.iter().flat_map(|s| &s.elements).find(|e| &e.id == id)
    }
    pub fn scene(&self, id: &Id) -> Option<&ScriptScene> {
        self.scenes.iter().find(|s| &s.id == id)
    }
    /// Todos os ids (cenas e elementos) zerados — para comparar roteiros ignorando ids.
    pub fn without_ids(&self) -> ScriptDoc {
        let mut d = self.clone();
        for s in &mut d.scenes {
            s.id = Id(String::new());
            for e in &mut s.elements {
                e.id = Id(String::new());
            }
        }
        d
    }
}

/// Elemento sem id (entrada das ferramentas).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ElementSpec {
    #[serde(default)]
    pub kind: ElementKind,
    #[serde(default)]
    pub character: Option<Id>,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub translation: String,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub est_seconds: Option<f32>,
}

impl ElementSpec {
    pub fn into_element(self, id: Id) -> ScriptElement {
        ScriptElement {
            id,
            kind: self.kind,
            character: self.character.filter(|c| !c.0.is_empty()),
            speaker: self.speaker.filter(|s| !s.is_empty()),
            text: self.text,
            translation: self.translation,
            emotion: self.emotion.filter(|s| !s.is_empty()),
            direction: self.direction.filter(|s| !s.is_empty()),
            est_seconds: self.est_seconds,
        }
    }
    pub fn action(text: &str, translation: &str) -> Self {
        ElementSpec { kind: ElementKind::Action, text: text.into(), translation: translation.into(), ..Default::default() }
    }
    pub fn dialogue(character: &Id, text: &str, translation: &str, emotion: Option<&str>, direction: Option<&str>) -> Self {
        ElementSpec {
            kind: ElementKind::Dialogue,
            character: Some(character.clone()),
            text: text.into(),
            translation: translation.into(),
            emotion: emotion.map(String::from),
            direction: direction.map(String::from),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct SceneSpec {
    pub heading: String,
    #[serde(default)]
    pub elements: Vec<ElementSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScriptCreateRequest {
    /// Nome do documento (raiz "Documentos").
    pub name: String,
    #[serde(default)]
    pub title: String,
    /// Idioma original; omitido = `dialogue_language` do projeto.
    #[serde(default)]
    pub language: Option<String>,
    /// Idioma da tradução; omitido = `subtitle_language` do projeto.
    #[serde(default)]
    pub translation_language: Option<String>,
    #[serde(default)]
    pub scenes: Vec<SceneSpec>,
}

pub struct ScriptCreated {
    pub id: Id,
    pub script: ScriptDoc,
    pub command: Command,
}

fn build_scenes(idgen: &mut IdGen, specs: Vec<SceneSpec>) -> Vec<ScriptScene> {
    specs
        .into_iter()
        .map(|s| ScriptScene { id: idgen.next(), heading: s.heading, elements: s.elements.into_iter().map(|e| e.into_element(idgen.next())).collect() })
        .collect()
}

pub fn script_create(p: &Project, idgen: &mut IdGen, req: ScriptCreateRequest) -> ToolResult<ScriptCreated> {
    let script = ScriptDoc {
        title: req.title,
        language: req.language.unwrap_or_else(|| p.settings.dialogue_language.clone()),
        translation_language: req.translation_language.unwrap_or_else(|| p.settings.subtitle_language.clone()),
        scenes: build_scenes(idgen, req.scenes),
    };
    script_create_from_doc(idgen, &req.name, script)
}

/// Cria o documento a partir de um [`ScriptDoc`] já montado (ex.: vindo de [`fountain_to_script`]).
pub fn script_create_from_doc(idgen: &mut IdGen, name: &str, script: ScriptDoc) -> ToolResult<ScriptCreated> {
    let id = idgen.next();
    let command = create_doc_command(&id, None, name, DocumentType::Script, &script)?;
    Ok(ScriptCreated { id, script, command })
}

pub fn script_read(p: &Project, doc: &Id) -> ToolResult<ScriptDoc> {
    doc_content(p, doc, DocumentType::Script)
}

/// Campos a mudar num elemento existente (omitido = mantém; texto vazio em emoção/direção limpa).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ElementPatch {
    #[serde(default)]
    pub kind: Option<ElementKind>,
    /// Id vazio limpa o personagem.
    #[serde(default)]
    pub character: Option<Id>,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub translation: Option<String>,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub est_seconds: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ScriptOp {
    SetMeta {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        translation_language: Option<String>,
    },
    InsertScene {
        #[serde(default)]
        index: Option<usize>,
        heading: String,
    },
    UpdateScene { scene: Id, heading: String },
    DeleteScene { scene: Id },
    MoveScene { scene: Id, index: usize },
    InsertElement {
        scene: Id,
        #[serde(default)]
        index: Option<usize>,
        element: ElementSpec,
    },
    UpdateElement { element: Id, patch: ElementPatch },
    DeleteElement { element: Id },
    /// Move o elemento para a cena `scene` (pode ser a mesma) na posição `index` (omitido = fim).
    MoveElement {
        element: Id,
        scene: Id,
        #[serde(default)]
        index: Option<usize>,
    },
}

pub struct ScriptEdited {
    /// Um único `ReplaceData` (um passo de undo).
    pub command: Command,
    /// Ids criados por `insert_*`, na ordem das operações.
    pub created: Vec<Id>,
    pub script: ScriptDoc,
}

fn find_element(doc: &ScriptDoc, id: &Id) -> Option<(usize, usize)> {
    doc.scenes.iter().enumerate().find_map(|(si, s)| s.elements.iter().position(|e| &e.id == id).map(|ei| (si, ei)))
}

fn scene_index(doc: &ScriptDoc, id: &Id) -> ToolResult<usize> {
    doc.scenes.iter().position(|s| &s.id == id).ok_or_else(|| ToolError::NotFound(format!("cena {id}")))
}

pub fn apply_script_ops(doc: &mut ScriptDoc, idgen: &mut IdGen, ops: Vec<ScriptOp>) -> ToolResult<Vec<Id>> {
    let mut created = Vec::new();
    for op in ops {
        match op {
            ScriptOp::SetMeta { title, language, translation_language } => {
                if let Some(t) = title {
                    doc.title = t;
                }
                if let Some(l) = language {
                    doc.language = l;
                }
                if let Some(t) = translation_language {
                    doc.translation_language = t;
                }
            }
            ScriptOp::InsertScene { index, heading } => {
                let id = idgen.next();
                let at = index.unwrap_or(doc.scenes.len()).min(doc.scenes.len());
                doc.scenes.insert(at, ScriptScene { id: id.clone(), heading, elements: Vec::new() });
                created.push(id);
            }
            ScriptOp::UpdateScene { scene, heading } => {
                let i = scene_index(doc, &scene)?;
                doc.scenes[i].heading = heading;
            }
            ScriptOp::DeleteScene { scene } => {
                let i = scene_index(doc, &scene)?;
                doc.scenes.remove(i);
            }
            ScriptOp::MoveScene { scene, index } => {
                let i = scene_index(doc, &scene)?;
                let s = doc.scenes.remove(i);
                let at = index.min(doc.scenes.len());
                doc.scenes.insert(at, s);
            }
            ScriptOp::InsertElement { scene, index, element } => {
                let si = scene_index(doc, &scene)?;
                let id = idgen.next();
                let els = &mut doc.scenes[si].elements;
                let at = index.unwrap_or(els.len()).min(els.len());
                els.insert(at, element.into_element(id.clone()));
                created.push(id);
            }
            ScriptOp::UpdateElement { element, patch } => {
                let (si, ei) = find_element(doc, &element).ok_or_else(|| ToolError::NotFound(format!("elemento {element}")))?;
                let e = &mut doc.scenes[si].elements[ei];
                if let Some(k) = patch.kind {
                    e.kind = k;
                }
                if let Some(c) = patch.character {
                    e.character = if c.0.is_empty() { None } else { Some(c) };
                }
                if let Some(s) = patch.speaker {
                    e.speaker = if s.is_empty() { None } else { Some(s) };
                }
                if let Some(t) = patch.text {
                    e.text = t;
                }
                if let Some(t) = patch.translation {
                    e.translation = t;
                }
                if let Some(t) = patch.emotion {
                    e.emotion = if t.is_empty() { None } else { Some(t) };
                }
                if let Some(t) = patch.direction {
                    e.direction = if t.is_empty() { None } else { Some(t) };
                }
                if let Some(s) = patch.est_seconds {
                    e.est_seconds = if s > 0.0 { Some(s) } else { None };
                }
            }
            ScriptOp::DeleteElement { element } => {
                let (si, ei) = find_element(doc, &element).ok_or_else(|| ToolError::NotFound(format!("elemento {element}")))?;
                doc.scenes[si].elements.remove(ei);
            }
            ScriptOp::MoveElement { element, scene, index } => {
                let (si, ei) = find_element(doc, &element).ok_or_else(|| ToolError::NotFound(format!("elemento {element}")))?;
                let target = scene_index(doc, &scene)?;
                let e = doc.scenes[si].elements.remove(ei);
                let els = &mut doc.scenes[target].elements;
                let at = index.unwrap_or(els.len()).min(els.len());
                els.insert(at, e);
            }
        }
    }
    Ok(created)
}

/// Edita o roteiro (inserir/atualizar/apagar/mover cenas e elementos por id) e devolve UM `ReplaceData`.
pub fn script_edit(p: &Project, idgen: &mut IdGen, doc_id: &Id, ops: Vec<ScriptOp>) -> ToolResult<ScriptEdited> {
    let mut doc = script_read(p, doc_id)?;
    let created = apply_script_ops(&mut doc, idgen, ops)?;
    let command = replace_doc_command(p, doc_id, &doc)?;
    Ok(ScriptEdited { command, created, script: doc })
}

// ---------------------------------------------------------------- tempo

fn is_small_kana(c: char) -> bool {
    "ゃゅょぁぃぅぇぉゎャュョァィゥェォヮ".contains(c)
}

/// Contagem heurística de moras: kana = 1 (exceto kana pequeno, que funde com o anterior; ッ, ン e ー contam 1),
/// kanji ≈ 2 (leitura on média), sequência latina/dígitos ≈ 1 mora a cada 2 caracteres, pontuação = 0.
pub fn count_mora(text: &str) -> usize {
    let mut n = 0usize;
    let mut run = 0usize;
    let flush = |n: &mut usize, run: &mut usize| {
        *n += (*run).div_ceil(2);
        *run = 0;
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            run += 1;
            continue;
        }
        flush(&mut n, &mut run);
        let u = c as u32;
        if is_small_kana(c) {
            continue;
        }
        if (0x3041..=0x3096).contains(&u) || (0x30A1..=0x30FA).contains(&u) || c == 'ー' {
            n += 1;
        } else if (0x4E00..=0x9FFF).contains(&u) || (0x3400..=0x4DBF).contains(&u) || c == '々' {
            n += 2;
        } else if (0xFF10..=0xFF19).contains(&u) || (0xFF21..=0xFF3A).contains(&u) || (0xFF41..=0xFF5A).contains(&u) {
            n += 1;
        }
    }
    flush(&mut n, &mut run);
    n
}

pub const MORA_PER_SECOND: f32 = 7.5;
pub const WORDS_PER_SECOND: f32 = 2.5;
pub const DEFAULT_ACTION_SECONDS: f32 = 2.0;

/// Duração de um elemento: explícita se houver; senão fala = moras/7,5 (ja) ou palavras/2,5; ação = 2 s;
/// parentético, transição e nota = 0.
pub fn element_seconds(language: &str, e: &ScriptElement) -> f32 {
    if let Some(s) = e.est_seconds {
        return s;
    }
    match e.kind {
        ElementKind::Action => DEFAULT_ACTION_SECONDS,
        ElementKind::Dialogue => {
            if e.text.trim().is_empty() {
                return 0.0;
            }
            let s = if language.to_ascii_lowercase().starts_with("ja") {
                count_mora(&e.text) as f32 / MORA_PER_SECOND
            } else {
                e.text.split_whitespace().count() as f32 / WORDS_PER_SECOND
            };
            s.max(0.3)
        }
        _ => 0.0,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ElementTiming {
    pub element: Id,
    pub seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneTiming {
    pub scene: Id,
    pub heading: String,
    pub seconds: f32,
    pub elements: Vec<ElementTiming>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TimingReport {
    pub total_seconds: f32,
    pub scenes: Vec<SceneTiming>,
}

pub fn estimate_timing(script: &ScriptDoc) -> TimingReport {
    let mut total = 0.0;
    let mut scenes = Vec::new();
    for s in &script.scenes {
        let elements: Vec<ElementTiming> = s.elements.iter().map(|e| ElementTiming { element: e.id.clone(), seconds: element_seconds(&script.language, e) }).collect();
        let seconds: f32 = elements.iter().map(|e| e.seconds).sum();
        total += seconds;
        scenes.push(SceneTiming { scene: s.id.clone(), heading: s.heading.clone(), seconds, elements });
    }
    TimingReport { total_seconds: total, scenes }
}

/// Grava a estimativa em `est_seconds` dos elementos que ainda não têm (um `ReplaceData`).
pub fn script_apply_timing(p: &Project, doc_id: &Id) -> ToolResult<Command> {
    let mut doc = script_read(p, doc_id)?;
    let lang = doc.language.clone();
    for s in &mut doc.scenes {
        for e in &mut s.elements {
            if e.est_seconds.is_none() && matches!(e.kind, ElementKind::Action | ElementKind::Dialogue) {
                e.est_seconds = Some(element_seconds(&lang, e));
            }
        }
    }
    replace_doc_command(p, doc_id, &doc)
}

// ---------------------------------------------------------------- Fountain

const HEADING_PREFIXES: [&str; 6] = ["INT./EXT.", "INT/EXT.", "INT.", "EXT.", "EST.", "I/E."];

fn has_heading_prefix(s: &str) -> bool {
    let up = s.trim_start().to_uppercase();
    HEADING_PREFIXES.iter().any(|k| up.starts_with(k))
}

fn uppercase_like(s: &str) -> bool {
    let mut has_up = false;
    for c in s.chars() {
        if c.is_lowercase() {
            return false;
        }
        if c.is_uppercase() {
            has_up = true;
        }
    }
    has_up
}

fn esc_meta(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unesc_meta(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn meta_lines(e: &ScriptElement) -> Vec<String> {
    let mut v = Vec::new();
    if !e.translation.is_empty() {
        v.push(format!("[[tr: {}]]", esc_meta(&e.translation)));
    }
    if let Some(x) = &e.emotion {
        v.push(format!("[[emotion: {}]]", esc_meta(x)));
    }
    if let Some(x) = &e.direction {
        v.push(format!("[[direction: {}]]", esc_meta(x)));
    }
    if let Some(s) = e.est_seconds {
        v.push(format!("[[sec: {s}]]"));
    }
    v
}

fn speaker_name(p: &Project, e: &ScriptElement) -> Option<String> {
    if let Some(c) = &e.character {
        if let Some(n) = p.node(c) {
            return Some(n.name.clone());
        }
    }
    e.speaker.clone()
}

fn speaker_key(e: &ScriptElement) -> Option<String> {
    e.character.as_ref().map(|c| format!("id:{c}")).or_else(|| e.speaker.as_ref().map(|s| format!("name:{s}")))
}

fn character_line(name: &str) -> String {
    let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii() && !c.is_lowercase() && !matches!(c, '(' | ')' | '^' | '@')) && uppercase_like(name);
    if plain { name.to_string() } else { format!("@{name}") }
}

/// Exporta para Fountain. Extensões: `[[tr: …]]` (tradução), `[[emotion: …]]`, `[[direction: …]]`, `[[sec: …]]`
/// como notas dentro do bloco do elemento; nomes com minúsculas/não-ASCII usam o prefixo `@`.
pub fn script_to_fountain(p: &Project, doc: &ScriptDoc) -> String {
    let mut out = String::new();
    out.push_str(&format!("Title: {}\nLanguage: {}\nTranslation: {}\n\n", doc.title, doc.language, doc.translation_language));
    for scene in &doc.scenes {
        if !scene.heading.is_empty() {
            if has_heading_prefix(&scene.heading) {
                out.push_str(&scene.heading);
            } else {
                out.push('.');
                out.push_str(&scene.heading);
            }
            out.push_str("\n\n");
        }
        let els = &scene.elements;
        let mut i = 0;
        while i < els.len() {
            let e = &els[i];
            match e.kind {
                ElementKind::Action => {
                    let mut lines: Vec<String> = e.text.lines().map(String::from).collect();
                    if lines.is_empty() {
                        lines.push(String::new());
                    }
                    let first = &lines[0];
                    let risky = uppercase_like(first)
                        || has_heading_prefix(first)
                        || first.starts_with(['@', '#', '.', '>', '~', '=', '[', '(', '!'])
                        || first.trim().is_empty();
                    if risky {
                        lines[0] = format!("!{}", lines[0]);
                    }
                    out.push_str(&lines.join("\n"));
                    for m in meta_lines(e) {
                        out.push('\n');
                        out.push_str(&m);
                    }
                    out.push_str("\n\n");
                }
                ElementKind::Dialogue | ElementKind::Parenthetical => {
                    // parentético imediatamente antes de uma fala do mesmo falante entra no bloco da fala
                    let merge = e.kind == ElementKind::Parenthetical
                        && speaker_key(e).is_some()
                        && els.get(i + 1).is_some_and(|n| n.kind == ElementKind::Dialogue && speaker_key(n) == speaker_key(e));
                    if merge {
                        let d = &els[i + 1];
                        out.push_str(&character_line(&speaker_name(p, d).unwrap_or_default()));
                        out.push_str(&format!("\n({})\n", e.text));
                        out.push_str(&d.text);
                        for m in meta_lines(d) {
                            out.push('\n');
                            out.push_str(&m);
                        }
                        out.push_str("\n\n");
                        i += 2;
                        continue;
                    }
                    if e.kind == ElementKind::Parenthetical {
                        out.push_str(&format!("({})", e.text));
                        for m in meta_lines(e) {
                            out.push('\n');
                            out.push_str(&m);
                        }
                        out.push_str("\n\n");
                    } else {
                        out.push_str(&character_line(&speaker_name(p, e).unwrap_or_default()));
                        out.push('\n');
                        out.push_str(&e.text);
                        for m in meta_lines(e) {
                            out.push('\n');
                            out.push_str(&m);
                        }
                        out.push_str("\n\n");
                    }
                }
                ElementKind::Transition => {
                    out.push_str(&format!("> {}", e.text));
                    for m in meta_lines(e) {
                        out.push('\n');
                        out.push_str(&m);
                    }
                    out.push_str("\n\n");
                }
                ElementKind::Note => {
                    out.push_str(&format!("[[{}]]", esc_meta(&e.text)));
                    out.push_str("\n\n");
                }
            }
            i += 1;
        }
    }
    out
}

struct Meta {
    translation: Option<String>,
    emotion: Option<String>,
    direction: Option<String>,
    est: Option<f32>,
}

fn parse_meta_line(line: &str) -> Option<(&'static str, String)> {
    let inner = line.trim().strip_prefix("[[")?.strip_suffix("]]")?;
    for key in ["tr", "emotion", "direction", "sec"] {
        if let Some(v) = inner.strip_prefix(key).and_then(|r| r.strip_prefix(':')) {
            return Some((key, unesc_meta(v.trim_start())));
        }
    }
    None
}

fn extract_meta(lines: Vec<String>) -> (Vec<String>, Meta) {
    let mut meta = Meta { translation: None, emotion: None, direction: None, est: None };
    let mut rest = Vec::new();
    for l in lines {
        match parse_meta_line(&l) {
            Some(("tr", v)) => meta.translation = Some(v),
            Some(("emotion", v)) => meta.emotion = Some(v),
            Some(("direction", v)) => meta.direction = Some(v),
            Some(("sec", v)) => meta.est = v.trim().parse().ok(),
            _ => rest.push(l),
        }
    }
    (rest, meta)
}

fn resolve_speaker(p: &Project, name: &str) -> (Option<Id>, Option<String>) {
    let strip = |s: &str| -> String {
        match s.rfind('(') {
            Some(i) if s.trim_end().ends_with(')') => s[..i].trim().to_string(),
            _ => s.trim().to_string(),
        }
    };
    let candidates = [name.trim().to_string(), strip(name)];
    for cand in &candidates {
        for id in &p.roots.characters {
            if let Some(n) = p.node(id) {
                if n.name.to_lowercase() == cand.to_lowercase() {
                    return (Some(id.clone()), None);
                }
            }
        }
    }
    (None, Some(name.trim().to_string()))
}

fn is_character_line(line: &str) -> bool {
    if let Some(r) = line.strip_prefix('@') {
        return !r.trim().is_empty();
    }
    let t = line.trim_end_matches('^').trim();
    uppercase_like(t) && !has_heading_prefix(t) && !t.starts_with(['!', '#', '=', '>', '.', '['])
}

/// Importa Fountain (ver extensões em [`script_to_fountain`]). Nomes de personagem são casados com os personagens
/// do projeto (sem diferenciar maiúsculas); sem correspondência ficam em `speaker`.
pub fn fountain_to_script(p: &Project, idgen: &mut IdGen, src: &str) -> ScriptDoc {
    let text = src.replace("\r\n", "\n");
    let mut paras: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for l in text.lines() {
        if l.trim().is_empty() {
            if !cur.is_empty() {
                paras.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(l.trim_end().to_string());
        }
    }
    if !cur.is_empty() {
        paras.push(cur);
    }
    let mut doc = ScriptDoc { title: String::new(), language: p.settings.dialogue_language.clone(), translation_language: p.settings.subtitle_language.clone(), scenes: Vec::new() };
    let mut start = 0;
    if let Some(first) = paras.first() {
        let keys = ["title", "credit", "author", "authors", "source", "draft date", "contact", "language", "translation", "notes", "copyright"];
        let is_kv = |l: &str| l.split_once(':').is_some_and(|(k, _)| keys.contains(&k.trim().to_lowercase().as_str()));
        if is_kv(&first[0]) {
            for l in first {
                if let Some((k, v)) = l.split_once(':') {
                    match k.trim().to_lowercase().as_str() {
                        "title" => doc.title = v.trim().to_string(),
                        "language" => doc.language = v.trim().to_string(),
                        "translation" => doc.translation_language = v.trim().to_string(),
                        _ => {}
                    }
                }
            }
            start = 1;
        }
    }
    let mut scene: Option<ScriptScene> = None;
    let push = |doc: &mut ScriptDoc, scene: &mut Option<ScriptScene>, el: ScriptElement, idgen: &mut IdGen| {
        if scene.is_none() {
            *scene = Some(ScriptScene { id: idgen.next(), heading: String::new(), elements: Vec::new() });
        }
        let _ = &doc;
        scene.as_mut().unwrap().elements.push(el);
    };
    for para in paras.into_iter().skip(start) {
        let first = para[0].clone();
        if first.starts_with("===") || first.starts_with('#') || first.starts_with('=') {
            continue;
        }
        let forced_heading = first.starts_with('.') && !first.starts_with("..");
        if forced_heading || has_heading_prefix(&first) {
            if let Some(s) = scene.take() {
                doc.scenes.push(s);
            }
            let heading = if forced_heading { first[1..].trim().to_string() } else { first.trim().to_string() };
            scene = Some(ScriptScene { id: idgen.next(), heading, elements: Vec::new() });
            if para.len() > 1 {
                let (rest, meta) = extract_meta(para[1..].to_vec());
                if !rest.is_empty() {
                    let el = ScriptElement {
                        id: idgen.next(),
                        kind: ElementKind::Action,
                        text: rest.join("\n"),
                        translation: meta.translation.unwrap_or_default(),
                        emotion: meta.emotion,
                        direction: meta.direction,
                        est_seconds: meta.est,
                        ..Default::default()
                    };
                    push(&mut doc, &mut scene, el, idgen);
                }
            }
            continue;
        }
        // nota solta: [[texto]] que não é meta
        if para.len() == 1 && first.starts_with("[[") && first.ends_with("]]") && parse_meta_line(&first).is_none() {
            let el = ScriptElement { id: idgen.next(), kind: ElementKind::Note, text: unesc_meta(&first[2..first.len() - 2]), ..Default::default() };
            push(&mut doc, &mut scene, el, idgen);
            continue;
        }
        if first.starts_with('>') && !first.trim_end().ends_with('<') {
            let (_, meta) = extract_meta(para[1..].to_vec());
            let el = ScriptElement {
                id: idgen.next(),
                kind: ElementKind::Transition,
                text: first[1..].trim().to_string(),
                translation: meta.translation.unwrap_or_default(),
                emotion: meta.emotion,
                direction: meta.direction,
                est_seconds: meta.est,
                ..Default::default()
            };
            push(&mut doc, &mut scene, el, idgen);
            continue;
        }
        if first.starts_with('!') {
            let mut lines = para.clone();
            lines[0] = first[1..].to_string();
            let (rest, meta) = extract_meta(lines);
            let el = ScriptElement {
                id: idgen.next(),
                kind: ElementKind::Action,
                text: rest.join("\n"),
                translation: meta.translation.unwrap_or_default(),
                emotion: meta.emotion,
                direction: meta.direction,
                est_seconds: meta.est,
                ..Default::default()
            };
            push(&mut doc, &mut scene, el, idgen);
            continue;
        }
        if is_character_line(&first) && para.len() >= 2 {
            let name = first.strip_prefix('@').unwrap_or(&first).trim_end_matches('^').trim().to_string();
            let (character, speaker) = resolve_speaker(p, &name);
            let (body, meta) = extract_meta(para[1..].to_vec());
            let mut text_lines = Vec::new();
            let mut parens_before = Vec::new();
            let mut parens_after = Vec::new();
            for l in body {
                let is_paren = l.trim_start().starts_with('(') && l.trim_end().ends_with(')');
                if is_paren && text_lines.is_empty() {
                    parens_before.push(l.trim()[1..l.trim().len() - 1].to_string());
                } else if is_paren {
                    parens_after.push(l.trim()[1..l.trim().len() - 1].to_string());
                } else {
                    text_lines.push(l);
                }
            }
            for t in parens_before {
                let el = ScriptElement { id: idgen.next(), kind: ElementKind::Parenthetical, character: character.clone(), speaker: speaker.clone(), text: t, ..Default::default() };
                push(&mut doc, &mut scene, el, idgen);
            }
            let el = ScriptElement {
                id: idgen.next(),
                kind: ElementKind::Dialogue,
                character: character.clone(),
                speaker: speaker.clone(),
                text: text_lines.join("\n"),
                translation: meta.translation.unwrap_or_default(),
                emotion: meta.emotion,
                direction: meta.direction,
                est_seconds: meta.est,
            };
            push(&mut doc, &mut scene, el, idgen);
            for t in parens_after {
                let el = ScriptElement { id: idgen.next(), kind: ElementKind::Parenthetical, text: t, ..Default::default() };
                push(&mut doc, &mut scene, el, idgen);
            }
            continue;
        }
        let (rest, meta) = extract_meta(para.clone());
        if rest.len() == 1 && rest[0].starts_with('(') && rest[0].ends_with(')') {
            let el = ScriptElement {
                id: idgen.next(),
                kind: ElementKind::Parenthetical,
                text: rest[0][1..rest[0].len() - 1].to_string(),
                translation: meta.translation.unwrap_or_default(),
                emotion: meta.emotion,
                direction: meta.direction,
                est_seconds: meta.est,
                ..Default::default()
            };
            push(&mut doc, &mut scene, el, idgen);
            continue;
        }
        if rest.len() == 1 && uppercase_like(&rest[0]) && rest[0].trim_end().ends_with("TO:") {
            let el = ScriptElement { id: idgen.next(), kind: ElementKind::Transition, text: rest[0].trim().to_string(), ..Default::default() };
            push(&mut doc, &mut scene, el, idgen);
            continue;
        }
        let el = ScriptElement {
            id: idgen.next(),
            kind: ElementKind::Action,
            text: rest.join("\n"),
            translation: meta.translation.unwrap_or_default(),
            emotion: meta.emotion,
            direction: meta.direction,
            est_seconds: meta.est,
            ..Default::default()
        };
        push(&mut doc, &mut scene, el, idgen);
    }
    if let Some(s) = scene.take() {
        doc.scenes.push(s);
    }
    doc
}

// ================================================================ beats

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Beat {
    pub id: Id,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub target_seconds: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Act {
    pub name: String,
    #[serde(default)]
    pub beats: Vec<Beat>,
}

/// Quadro de beats (cartões em colunas por ato). Guardado num documento Notes com `schema: "beats"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BeatBoard {
    #[serde(default = "beats_schema")]
    pub schema: String,
    #[serde(default)]
    pub acts: Vec<Act>,
}

fn beats_schema() -> String {
    "beats".into()
}

impl Default for BeatBoard {
    fn default() -> Self {
        BeatBoard { schema: beats_schema(), acts: Vec::new() }
    }
}

impl BeatBoard {
    pub fn total_target_seconds(&self) -> f32 {
        self.acts.iter().flat_map(|a| &a.beats).map(|b| b.target_seconds).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BeatOp {
    AddAct { name: String },
    AddBeat { act: usize, title: String, #[serde(default)] summary: String, #[serde(default)] target_seconds: f32 },
    UpdateBeat { beat: Id, #[serde(default)] title: Option<String>, #[serde(default)] summary: Option<String>, #[serde(default)] target_seconds: Option<f32> },
    DeleteBeat { beat: Id },
    /// Move o cartão para o ato `act` na posição `index` (omitido = fim).
    MoveBeat { beat: Id, act: usize, #[serde(default)] index: Option<usize> },
}

pub fn beats_create(idgen: &mut IdGen, name: &str, board: BeatBoard) -> ToolResult<Created> {
    let id = idgen.next();
    let command = create_doc_command(&id, None, name, DocumentType::Notes, &board)?;
    Ok(Created { id, command })
}

pub fn beats_read(p: &Project, doc: &Id) -> ToolResult<BeatBoard> {
    let b: BeatBoard = doc_content(p, doc, DocumentType::Notes)?;
    if b.schema != "beats" {
        return Err(ToolError::Invalid(format!("{doc} não é um quadro de beats (schema \"{}\")", b.schema)));
    }
    Ok(b)
}

pub fn beats_edit(p: &Project, idgen: &mut IdGen, doc: &Id, ops: Vec<BeatOp>) -> ToolResult<(Command, BeatBoard)> {
    let mut board = beats_read(p, doc)?;
    for op in ops {
        match op {
            BeatOp::AddAct { name } => board.acts.push(Act { name, beats: Vec::new() }),
            BeatOp::AddBeat { act, title, summary, target_seconds } => {
                let a = board.acts.get_mut(act).ok_or_else(|| ToolError::NotFound(format!("ato {act}")))?;
                a.beats.push(Beat { id: idgen.next(), title, summary, target_seconds });
            }
            BeatOp::UpdateBeat { beat, title, summary, target_seconds } => {
                let b = board.acts.iter_mut().flat_map(|a| a.beats.iter_mut()).find(|b| b.id == beat).ok_or_else(|| ToolError::NotFound(format!("beat {beat}")))?;
                if let Some(t) = title {
                    b.title = t;
                }
                if let Some(s) = summary {
                    b.summary = s;
                }
                if let Some(t) = target_seconds {
                    b.target_seconds = t;
                }
            }
            BeatOp::DeleteBeat { beat } => {
                let mut found = false;
                for a in &mut board.acts {
                    if let Some(i) = a.beats.iter().position(|b| b.id == beat) {
                        a.beats.remove(i);
                        found = true;
                    }
                }
                if !found {
                    return Err(ToolError::NotFound(format!("beat {beat}")));
                }
            }
            BeatOp::MoveBeat { beat, act, index } => {
                if act >= board.acts.len() {
                    return Err(ToolError::NotFound(format!("ato {act}")));
                }
                let mut taken = None;
                for a in &mut board.acts {
                    if let Some(i) = a.beats.iter().position(|b| b.id == beat) {
                        taken = Some(a.beats.remove(i));
                    }
                }
                let b = taken.ok_or_else(|| ToolError::NotFound(format!("beat {beat}")))?;
                let list = &mut board.acts[act].beats;
                let at = index.unwrap_or(list.len()).min(list.len());
                list.insert(at, b);
            }
        }
    }
    let cmd = replace_doc_command(p, doc, &board)?;
    Ok((cmd, board))
}

// ================================================================ storyboard

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum Shot {
    Establishing,
    Wide,
    Full,
    #[default]
    Medium,
    MediumClose,
    Close,
    ExtremeClose,
    Detail,
    OverShoulder,
}

impl Shot {
    /// Sigla usada na folha de e-conte.
    pub fn code(&self) -> &'static str {
        match self {
            Shot::Establishing => "EST",
            Shot::Wide => "LS",
            Shot::Full => "FS",
            Shot::Medium => "MS",
            Shot::MediumClose => "MCU",
            Shot::Close => "CU",
            Shot::ExtremeClose => "ECU",
            Shot::Detail => "DET",
            Shot::OverShoulder => "OTS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum Angle {
    #[default]
    Eye,
    High,
    Low,
    Dutch,
    Bird,
    Worm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CameraMoveKind {
    Pan,
    Tilt,
    ZoomIn,
    ZoomOut,
    Track,
    Shake,
    None,
}

/// Quadro de câmera: centro normalizado (0..1 do canvas) e zoom (1 = quadro inteiro).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraFrame {
    pub center: [f32; 2],
    pub zoom: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraMoveSpec {
    pub kind: CameraMoveKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<CameraFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<CameraFrame>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Panel {
    pub id: Id,
    /// Número do corte (カット番号).
    pub cut_number: u32,
    #[serde(default)]
    pub shot: Shot,
    #[serde(default)]
    pub angle: Angle,
    #[serde(default)]
    pub action: String,
    /// Falas do roteiro (ids de elementos).
    #[serde(default)]
    pub dialogue_refs: Vec<Id>,
    #[serde(default)]
    pub dialogue_text: String,
    #[serde(default)]
    pub sfx: String,
    pub seconds: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_move: Option<CameraMoveSpec>,
    /// Cut (filho do documento) onde o painel é desenhado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing_cut: Option<Id>,
    /// Cena do roteiro a que o painel pertence (agrupa painéis em cenas ao gerar cortes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StoryboardDoc {
    #[serde(default)]
    pub title: String,
    /// Tamanho do canvas dos painéis.
    #[serde(default = "d_panel_canvas")]
    pub canvas: [u32; 2],
    #[serde(default)]
    pub panels: Vec<Panel>,
}

fn d_panel_canvas() -> [u32; 2] {
    [480, 270]
}

impl Default for StoryboardDoc {
    fn default() -> Self {
        StoryboardDoc { title: String::new(), canvas: d_panel_canvas(), panels: Vec::new() }
    }
}

impl StoryboardDoc {
    pub fn panel(&self, id: &Id) -> Option<&Panel> {
        self.panels.iter().find(|p| &p.id == id)
    }
    pub fn total_seconds(&self) -> f32 {
        self.panels.iter().map(|p| p.seconds).sum()
    }
}

pub fn storyboard_read(p: &Project, doc: &Id) -> ToolResult<StoryboardDoc> {
    doc_content(p, doc, DocumentType::Storyboard)
}

pub fn storyboard_create(idgen: &mut IdGen, name: &str, title: &str, canvas: Option<[u32; 2]>) -> ToolResult<Created> {
    let id = idgen.next();
    let sb = StoryboardDoc { title: title.into(), canvas: canvas.unwrap_or_else(d_panel_canvas), panels: Vec::new() };
    let command = create_doc_command(&id, None, name, DocumentType::Storyboard, &sb)?;
    Ok(Created { id, command })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PanelSpec {
    /// Omitido = último número + 1.
    #[serde(default)]
    pub cut_number: Option<u32>,
    #[serde(default)]
    pub shot: Shot,
    #[serde(default)]
    pub angle: Angle,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub dialogue_refs: Vec<Id>,
    #[serde(default)]
    pub dialogue_text: String,
    #[serde(default)]
    pub sfx: String,
    #[serde(default = "d_seconds")]
    pub seconds: f32,
    #[serde(default)]
    pub camera_move: Option<CameraMoveSpec>,
    #[serde(default)]
    pub scene: Option<Id>,
}

fn d_seconds() -> f32 {
    3.0
}

impl Default for PanelSpec {
    fn default() -> Self {
        PanelSpec {
            cut_number: None,
            shot: Shot::default(),
            angle: Angle::default(),
            action: String::new(),
            dialogue_refs: Vec::new(),
            dialogue_text: String::new(),
            sfx: String::new(),
            seconds: d_seconds(),
            camera_move: None,
            scene: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PanelAddRequest {
    pub storyboard: Id,
    /// Posição na lista; omitido = no fim.
    #[serde(default)]
    pub index: Option<usize>,
    pub spec: PanelSpec,
}

/// Onde o agente desenha o painel (use `draw::draw_command` com `cut`, `layer`, quadro 0).
#[derive(Debug, Clone)]
pub struct PanelDrawing {
    pub cut: Id,
    pub layer: Id,
    pub cel: Id,
}

pub struct PanelAdded {
    pub panel: Id,
    pub drawing: PanelDrawing,
    pub command: Command,
}

pub const SKETCH_LAYER: &str = "rascunho";

fn validate_seconds(s: f32) -> ToolResult<()> {
    if s.is_finite() && s > 0.0 && s <= 600.0 { Ok(()) } else { Err(ToolError::Invalid(format!("segundos fora de (0, 600]: {s}"))) }
}

/// Adiciona um painel: cria o Cut de desenho (canvas do storyboard, camada vetorial "rascunho" com um cel) como filho
/// do documento e atualiza a lista de painéis — tudo num único Batch.
pub fn storyboard_panel_add(p: &Project, idgen: &mut IdGen, req: &PanelAddRequest) -> ToolResult<PanelAdded> {
    let mut sb = storyboard_read(p, &req.storyboard)?;
    validate_seconds(req.spec.seconds)?;
    let number = req.spec.cut_number.unwrap_or_else(|| sb.panels.iter().map(|x| x.cut_number).max().unwrap_or(0) + 1);
    let (panel_id, cut_id, layer_id, cel_id) = (idgen.next(), idgen.next(), idgen.next(), idgen.next());
    let s = &req.spec;
    let panel = Panel {
        id: panel_id.clone(),
        cut_number: number,
        shot: s.shot,
        angle: s.angle,
        action: s.action.clone(),
        dialogue_refs: s.dialogue_refs.clone(),
        dialogue_text: s.dialogue_text.clone(),
        sfx: s.sfx.clone(),
        seconds: s.seconds,
        camera_move: s.camera_move.clone(),
        drawing_cut: Some(cut_id.clone()),
        scene: s.scene.clone(),
    };
    let at = req.index.unwrap_or(sb.panels.len()).min(sb.panels.len());
    sb.panels.insert(at, panel);
    let mut cd = CutData::new(1);
    cd.number = number;
    cd.canvas = Some(sb.canvas);
    let mut cmds = vec![
        Command::CreateNode { id: Some(cut_id.clone()), parent: Some(req.storyboard.clone()), index: None, name: format!("Painel {number}"), data: NodeKind::Cut(cd) },
        Command::CreateNode { id: Some(layer_id.clone()), parent: Some(cut_id.clone()), index: None, name: SKETCH_LAYER.into(), data: NodeKind::Layer(LayerData::new(LayerType::Vector)) },
        Command::CreateNode { id: Some(cel_id.clone()), parent: Some(layer_id.clone()), index: None, name: "rascunho1".into(), data: NodeKind::Cel(CelData::new(1)) },
    ];
    cmds.push(replace_doc_command(p, &req.storyboard, &sb)?);
    Ok(PanelAdded {
        panel: panel_id,
        drawing: PanelDrawing { cut: cut_id, layer: layer_id, cel: cel_id },
        command: Command::Batch { commands: cmds, label: Some(format!("Painel {number}")) },
    })
}

/// Cut, camada "rascunho" e cel do painel (para desenhar com as ferramentas de desenho).
pub fn panel_drawing(p: &Project, storyboard: &Id, panel: &Id) -> ToolResult<PanelDrawing> {
    let sb = storyboard_read(p, storyboard)?;
    let pn = sb.panel(panel).ok_or_else(|| ToolError::NotFound(format!("painel {panel}")))?;
    let cut = pn.drawing_cut.clone().ok_or_else(|| ToolError::Invalid("painel sem corte de desenho".into()))?;
    let cn = node(p, &cut)?;
    let layer = cn
        .children
        .iter()
        .find(|c| p.node(c).is_some_and(|n| n.name == SKETCH_LAYER && matches!(n.data, NodeKind::Layer(_))))
        .ok_or_else(|| ToolError::NotFound("camada rascunho".into()))?
        .clone();
    let cel = p.node(&layer).and_then(|l| l.children.first().cloned()).ok_or_else(|| ToolError::NotFound("cel do rascunho".into()))?;
    Ok(PanelDrawing { cut, layer, cel })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct PanelPatch {
    #[serde(default)]
    pub cut_number: Option<u32>,
    #[serde(default)]
    pub shot: Option<Shot>,
    #[serde(default)]
    pub angle: Option<Angle>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub dialogue_refs: Option<Vec<Id>>,
    #[serde(default)]
    pub dialogue_text: Option<String>,
    #[serde(default)]
    pub sfx: Option<String>,
    #[serde(default)]
    pub seconds: Option<f32>,
    #[serde(default)]
    pub camera_move: Option<CameraMoveSpec>,
    #[serde(default)]
    pub clear_camera: bool,
    #[serde(default)]
    pub scene: Option<Id>,
}

pub fn storyboard_panel_update(p: &Project, storyboard: &Id, panel: &Id, patch: &PanelPatch) -> ToolResult<Command> {
    let mut sb = storyboard_read(p, storyboard)?;
    let pn = sb.panels.iter_mut().find(|x| &x.id == panel).ok_or_else(|| ToolError::NotFound(format!("painel {panel}")))?;
    if let Some(n) = patch.cut_number {
        pn.cut_number = n;
    }
    if let Some(v) = patch.shot {
        pn.shot = v;
    }
    if let Some(v) = patch.angle {
        pn.angle = v;
    }
    if let Some(v) = &patch.action {
        pn.action = v.clone();
    }
    if let Some(v) = &patch.dialogue_refs {
        pn.dialogue_refs = v.clone();
    }
    if let Some(v) = &patch.dialogue_text {
        pn.dialogue_text = v.clone();
    }
    if let Some(v) = &patch.sfx {
        pn.sfx = v.clone();
    }
    if let Some(s) = patch.seconds {
        validate_seconds(s)?;
        pn.seconds = s;
    }
    if patch.clear_camera {
        pn.camera_move = None;
    }
    if let Some(c) = &patch.camera_move {
        pn.camera_move = Some(c.clone());
    }
    if let Some(s) = &patch.scene {
        pn.scene = if s.0.is_empty() { None } else { Some(s.clone()) };
    }
    replace_doc_command(p, storyboard, &sb)
}

/// Apaga o painel e o corte de desenho dele.
pub fn storyboard_panel_delete(p: &Project, storyboard: &Id, panel: &Id) -> ToolResult<Command> {
    let mut sb = storyboard_read(p, storyboard)?;
    let i = sb.panels.iter().position(|x| &x.id == panel).ok_or_else(|| ToolError::NotFound(format!("painel {panel}")))?;
    let removed = sb.panels.remove(i);
    let mut cmds = vec![replace_doc_command(p, storyboard, &sb)?];
    if let Some(c) = removed.drawing_cut {
        if p.node(&c).is_some() {
            cmds.push(Command::DeleteNode { id: c });
        }
    }
    Ok(Command::Batch { commands: cmds, label: Some(format!("Apagar painel {}", removed.cut_number)) })
}

/// Move o painel para a posição `index` da lista final.
pub fn storyboard_panel_move(p: &Project, storyboard: &Id, panel: &Id, index: usize) -> ToolResult<Command> {
    let mut sb = storyboard_read(p, storyboard)?;
    let i = sb.panels.iter().position(|x| &x.id == panel).ok_or_else(|| ToolError::NotFound(format!("painel {panel}")))?;
    let pn = sb.panels.remove(i);
    let at = index.min(sb.panels.len());
    sb.panels.insert(at, pn);
    replace_doc_command(p, storyboard, &sb)
}

/// Renumera os cortes 1..N na ordem atual dos painéis (`step` = 1 → 1,2,3; 5 → 5,10,15).
pub fn storyboard_renumber(p: &Project, storyboard: &Id, step: u32) -> ToolResult<Command> {
    let mut sb = storyboard_read(p, storyboard)?;
    let step = step.max(1);
    for (i, pn) in sb.panels.iter_mut().enumerate() {
        pn.cut_number = (i as u32 + 1) * step;
    }
    replace_doc_command(p, storyboard, &sb)
}

// ---------------------------------------------------------------- desenho de texto e formas (folha, animatic)

/// Linha de texto como contornos: origem da caneta em x = 0 e topo da caixa (baseline = 0,88 × corpo) em y = 0.
///
/// Os avanços vêm direto das métricas da fonte (em = `size`). `aiai_paint::text::text_outline` não serve aqui: no modo
/// horizontal ele avança com `PxScale` (escala pela altura ascendente+descendente) mas desenha o glifo na escala do em,
/// então a linha sai comprimida (~0,69×) e os espaços somem — ver o relatório da S16.
struct LaidText {
    contours: Vec<Vec<[f32; 2]>>,
    width: f32,
}

fn flatten_quad(out: &mut Vec<[f32; 2]>, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2]) {
    for k in 1..=8 {
        let t = k as f32 / 8.0;
        let u = 1.0 - t;
        out.push([u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0], u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1]]);
    }
}

fn flatten_cubic(out: &mut Vec<[f32; 2]>, p: [[f32; 2]; 4]) {
    for k in 1..=10 {
        let t = k as f32 / 10.0;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push([a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0], a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1]]);
    }
}

fn lay_text(font_bytes: &[u8], size: f32, s: &str) -> Option<LaidText> {
    use ab_glyph::{Font, FontRef, OutlineCurve};
    let font = FontRef::try_from_slice(font_bytes).ok()?;
    let k = size / font.units_per_em().unwrap_or(1000.0);
    let baseline = size * 0.88;
    let mut pen = 0.0f32;
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    for ch in s.chars() {
        let id = font.glyph_id(ch);
        if let Some(outline) = font.outline(id) {
            let tp = |p: ab_glyph::Point| [pen + p.x * k, baseline - p.y * k];
            let mut cur: Vec<[f32; 2]> = Vec::new();
            let mut last: Option<[f32; 2]> = None;
            for c in &outline.curves {
                let (start, end) = match c {
                    OutlineCurve::Line(a, b) => (tp(*a), tp(*b)),
                    OutlineCurve::Quad(a, _, c2) => (tp(*a), tp(*c2)),
                    OutlineCurve::Cubic(a, _, _, d) => (tp(*a), tp(*d)),
                };
                let continuous = last.is_some_and(|l| (l[0] - start[0]).abs() < 1e-3 && (l[1] - start[1]).abs() < 1e-3);
                if !continuous {
                    if cur.len() >= 3 {
                        contours.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(start);
                }
                match c {
                    OutlineCurve::Line(_, _) => cur.push(end),
                    OutlineCurve::Quad(a, b, c2) => flatten_quad(&mut cur, tp(*a), tp(*b), tp(*c2)),
                    OutlineCurve::Cubic(a, b, c2, d) => flatten_cubic(&mut cur, [tp(*a), tp(*b), tp(*c2), tp(*d)]),
                }
                last = Some(end);
            }
            if cur.len() >= 3 {
                contours.push(cur);
            }
        }
        pen += font.h_advance_unscaled(id) * k;
    }
    if s.trim().is_empty() {
        return None;
    }
    Some(LaidText { contours, width: pen })
}

pub(crate) fn text_width(font: &[u8], size: f32, s: &str) -> f32 {
    use ab_glyph::{Font, FontRef};
    let Ok(f) = FontRef::try_from_slice(font) else { return 0.0 };
    let k = size / f.units_per_em().unwrap_or(1000.0);
    s.chars().map(|c| f.h_advance_unscaled(f.glyph_id(c)) * k).sum()
}

fn is_cjk(c: char) -> bool {
    c as u32 >= 0x2E80
}

/// Quebra por palavras (separadas por espaço) e por caractere dentro de trechos CJK, para caber em `max_w`.
pub(crate) fn wrap_text(font: &[u8], size: f32, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        // (pedaço, vem depois de espaço)
        let mut tokens: Vec<(String, bool)> = Vec::new();
        let mut word = String::new();
        let mut sep = false;
        let flush = |word: &mut String, sep: &mut bool, tokens: &mut Vec<(String, bool)>| {
            if !word.is_empty() {
                tokens.push((std::mem::take(word), *sep));
                *sep = false;
            }
        };
        for c in para.chars() {
            if c.is_whitespace() {
                flush(&mut word, &mut sep, &mut tokens);
                sep = true;
            } else if is_cjk(c) {
                flush(&mut word, &mut sep, &mut tokens);
                tokens.push((c.to_string(), sep));
                sep = false;
            } else {
                word.push(c);
            }
        }
        flush(&mut word, &mut sep, &mut tokens);
        let mut cur = String::new();
        for (t, space) in tokens {
            let joiner = if cur.is_empty() || !space { "" } else { " " };
            let cand = format!("{cur}{joiner}{t}");
            if cur.is_empty() || text_width(font, size, &cand) <= max_w {
                cur = cand;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = t;
            }
        }
        lines.push(cur);
    }
    lines
}

fn blend_cov(img: &mut Image, cov: &aiai_paint::poly::PolyCoverage, color: [f32; 4]) {
    let premul = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
    for y in 0..cov.height {
        for x in 0..cov.width {
            let c = cov.cov[(y * cov.width + x) as usize];
            if c > 0.0 {
                let (px, py) = (cov.x0 as u32 + x, cov.y0 as u32 + y);
                if px < img.width && py < img.height {
                    let i = img.idx(px, py);
                    Image::over(&mut img.data[i], premul, c);
                }
            }
        }
    }
}

pub(crate) fn fill_poly(img: &mut Image, contours: &[Vec<[f32; 2]>], color: [f32; 4]) {
    if let Some(cov) = fill_polygons(contours, false, img.width, img.height) {
        blend_cov(img, &cov, color);
    }
}

pub(crate) fn fill_rect(img: &mut Image, x: i32, y: i32, w: i32, h: i32, color: [f32; 4]) {
    let premul = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
    for yy in y.max(0)..(y + h).min(img.height as i32) {
        for xx in x.max(0)..(x + w).min(img.width as i32) {
            let i = img.idx(xx as u32, yy as u32);
            Image::over(&mut img.data[i], premul, 1.0);
        }
    }
}

pub(crate) fn rect_outline(img: &mut Image, x: i32, y: i32, w: i32, h: i32, t: i32, color: [f32; 4]) {
    fill_rect(img, x, y, w, t, color);
    fill_rect(img, x, y + h - t, w, t, color);
    fill_rect(img, x, y, t, h, color);
    fill_rect(img, x + w - t, y, t, h, color);
}

/// Desenha uma linha de texto com o topo da caixa em `y` e a tinta começando em `x`. Devolve a largura.
pub(crate) fn draw_text(img: &mut Image, font: &[u8], size: f32, s: &str, x: f32, y: f32, color: [f32; 4]) -> f32 {
    let Some(l) = lay_text(font, size, s) else { return 0.0 };
    let moved: Vec<Vec<[f32; 2]>> = l.contours.iter().map(|c| c.iter().map(|q| [q[0] + x, q[1] + y]).collect()).collect();
    fill_poly(img, &moved, color);
    l.width
}

pub(crate) fn draw_text_centered(img: &mut Image, font: &[u8], size: f32, s: &str, cx: f32, y: f32, color: [f32; 4]) {
    let w = text_width(font, size, s);
    draw_text(img, font, size, s, cx - w / 2.0, y, color);
}

pub(crate) fn draw_line(img: &mut Image, a: [f32; 2], b: [f32; 2], width: f32, color: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return;
    }
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    let quad = vec![[a[0] + nx, a[1] + ny], [b[0] + nx, b[1] + ny], [b[0] - nx, b[1] - ny], [a[0] - nx, a[1] - ny]];
    fill_poly(img, &[quad], color);
}

pub(crate) fn draw_arrow(img: &mut Image, a: [f32; 2], b: [f32; 2], width: f32, color: [f32; 4]) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = (width * 5.0).min(len * 0.6);
    let base = [b[0] - ux * head, b[1] - uy * head];
    draw_line(img, a, base, width, color);
    let (px, py) = (-uy * head * 0.5, ux * head * 0.5);
    fill_poly(img, &[vec![b, [base[0] + px, base[1] + py], [base[0] - px, base[1] - py]]], color);
}

// ---------------------------------------------------------------- render de painéis

fn render_panel_native(p: &Project, blobs: &BlobStore, panel: &Panel, bg: Option<[f32; 3]>) -> ToolResult<Option<Image>> {
    let Some(cut) = &panel.drawing_cut else { return Ok(None) };
    let opts = RenderOptions { include_guides: false, include_reference: false, only_layers: None, background: bg, camera: false };
    let out = render_frame(p, blobs, cut, 0, &opts).map_err(|e| ToolError::Render(e.to_string()))?;
    Ok(Some(out.image))
}

/// Imagem do painel encaixada (contain) em `w × h` sobre `bg`; sem desenho, só o fundo. `bg` `None` = transparente.
pub fn panel_image(p: &Project, blobs: &BlobStore, panel: &Panel, w: u32, h: u32, bg: Option<[f32; 3]>) -> ToolResult<Image> {
    let mut out = match bg {
        Some(c) => Image::filled(w, h, [c[0], c[1], c[2], 1.0]),
        None => Image::new(w, h),
    };
    if let Some(src) = render_panel_native(p, blobs, panel, bg)? {
        let s = (w as f32 / src.width as f32).min(h as f32 / src.height as f32);
        let (ox, oy) = ((w as f32 - src.width as f32 * s) / 2.0, (h as f32 - src.height as f32 * s) / 2.0);
        let scaled = transform_image(&src, Affine::from_trs(ox, oy, s, 0.0, [0.0, 0.0]), w, h);
        out.draw_over(&scaled, 0, 0, 1.0);
    }
    Ok(out)
}

// ---------------------------------------------------------------- folha de e-conte

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SheetOptions {
    /// Largura final em pixels (≤ 1600); padrão 1400.
    #[serde(default = "d_sheet_w")]
    pub width: u32,
}

fn d_sheet_w() -> u32 {
    1400
}

impl Default for SheetOptions {
    fn default() -> Self {
        SheetOptions { width: d_sheet_w() }
    }
}

const INK: [f32; 4] = [0.10, 0.11, 0.14, 1.0];
const GREY: [f32; 4] = [0.42, 0.44, 0.50, 1.0];
const RULE: [f32; 4] = [0.62, 0.64, 0.68, 1.0];
const HEAD_BG: [f32; 4] = [0.14, 0.17, 0.26, 1.0];
const SFX_COL: [f32; 4] = [0.74, 0.20, 0.12, 1.0];
const CAM_FROM: [f32; 4] = [0.92, 0.45, 0.10, 1.0];
const CAM_TO: [f32; 4] = [0.14, 0.45, 0.92, 1.0];

fn camera_label(c: &CameraMoveSpec) -> &'static str {
    match c.kind {
        CameraMoveKind::Pan => "PAN",
        CameraMoveKind::Tilt => "TILT",
        CameraMoveKind::ZoomIn => "ZOOM IN",
        CameraMoveKind::ZoomOut => "ZOOM OUT",
        CameraMoveKind::Track => "TRACK",
        CameraMoveKind::Shake => "SHAKE",
        CameraMoveKind::None => "FIX",
    }
}

fn angle_label(a: Angle) -> &'static str {
    match a {
        Angle::Eye => "eye",
        Angle::High => "high",
        Angle::Low => "low",
        Angle::Dutch => "dutch",
        Angle::Bird => "bird",
        Angle::Worm => "worm",
    }
}

fn draw_camera_overlay(img: &mut Image, cam: &CameraMoveSpec, ox: i32, oy: i32, w: i32, h: i32) {
    let rect_of = |f: &CameraFrame| -> (i32, i32, i32, i32, [f32; 2]) {
        let z = f.zoom.max(0.05);
        let (rw, rh) = (w as f32 / z, h as f32 / z);
        let (cx, cy) = (ox as f32 + f.center[0] * w as f32, oy as f32 + f.center[1] * h as f32);
        ((cx - rw / 2.0) as i32, (cy - rh / 2.0) as i32, rw as i32, rh as i32, [cx, cy])
    };
    let clip = |x: i32, y: i32, rw: i32, rh: i32| -> (i32, i32, i32, i32) {
        let (x0, y0) = (x.max(ox), y.max(oy));
        let (x1, y1) = ((x + rw).min(ox + w), (y + rh).min(oy + h));
        (x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
    };
    let mut centers = Vec::new();
    for (f, col) in [(&cam.from, CAM_FROM), (&cam.to, CAM_TO)] {
        if let Some(f) = f {
            let (x, y, rw, rh, c) = rect_of(f);
            let (x, y, rw, rh) = clip(x, y, rw, rh);
            rect_outline(img, x, y, rw, rh, 3, col);
            centers.push(c);
        }
    }
    if centers.len() == 2 && matches!(cam.kind, CameraMoveKind::Pan | CameraMoveKind::Tilt | CameraMoveKind::Track) {
        draw_arrow(img, centers[0], centers[1], 5.0, [0.95, 0.15, 0.15, 0.95]);
    }
}

/// Folha de e-conte: colunas Nº do corte | imagem do painel (com quadro/seta de câmera) | ação | fala/SE | segundos.
pub fn storyboard_export_sheet(p: &Project, blobs: &BlobStore, storyboard: &Id, opts: &SheetOptions) -> ToolResult<Image> {
    let sb = storyboard_read(p, storyboard)?;
    if sb.panels.is_empty() {
        return Err(ToolError::Invalid("storyboard sem painéis".into()));
    }
    let width = opts.width.clamp(400, 1600);
    let base_w: u32 = 1400;
    let font = default_font();
    let bold = heavy_font();
    // colunas (x, largura)
    let (no_x, no_w) = (30i32, 84i32);
    let (img_x, img_w) = (124i32, 520i32);
    let (act_x, act_w) = (662i32, 322i32);
    let (dia_x, dia_w) = (1000i32, 288i32);
    let (sec_x, sec_w) = (1304i32, 66i32);
    let img_h = (img_w as f32 * sb.canvas[1] as f32 / sb.canvas[0] as f32).round() as i32;
    let pad = 12i32;
    let title_h = 78i32;
    let head_h = 40i32;
    let body_sz = 19.0f32;
    let lh = 27i32;

    struct Row {
        h: i32,
        act_lines: Vec<String>,
        dia_lines: Vec<String>,
    }
    let mut rows = Vec::new();
    for pn in &sb.panels {
        let act_lines = wrap_text(font, body_sz, &pn.action, act_w as f32);
        let dia_lines = wrap_text(font, body_sz, &pn.dialogue_text, dia_w as f32);
        let act_h = lh * (act_lines.iter().filter(|l| !l.is_empty()).count() as i32 + 1 + i32::from(pn.camera_move.is_some()));
        let sfx_n = if pn.sfx.is_empty() { 0 } else { wrap_text(font, body_sz, &format!("SE: {}", pn.sfx), dia_w as f32).len() as i32 + 1 };
        let dia_h = lh * dia_lines.iter().filter(|l| !l.is_empty()).count() as i32 + sfx_n * lh;
        let h = img_h.max(act_h).max(dia_h) + pad * 2;
        rows.push(Row { h, act_lines, dia_lines });
    }
    let total_h = title_h + head_h + rows.iter().map(|r| r.h).sum::<i32>() + 30;
    let mut img = Image::filled(base_w, total_h as u32, [1.0, 1.0, 1.0, 1.0]);
    // título
    fill_rect(&mut img, 0, 0, base_w as i32, title_h, HEAD_BG);
    let title = if sb.title.is_empty() { p.name.clone() } else { sb.title.clone() };
    draw_text(&mut img, bold, 32.0, &format!("絵コンテ  {title}"), 30.0, 16.0, [1.0, 1.0, 1.0, 1.0]);
    let summary = format!("{} painéis · {:.1} s", sb.panels.len(), sb.total_seconds());
    let sw = text_width(font, 20.0, &summary);
    draw_text(&mut img, font, 20.0, &summary, base_w as f32 - 30.0 - sw, 26.0, [0.8, 0.85, 1.0, 1.0]);
    // cabeçalho das colunas
    let hy = title_h;
    fill_rect(&mut img, 0, hy, base_w as i32, head_h, [0.90, 0.92, 0.96, 1.0]);
    for (label, x, w) in [("CUT", no_x, no_w), ("画 / IMAGEM", img_x, img_w), ("内容 / AÇÃO", act_x, act_w), ("台詞・SE / FALA", dia_x, dia_w), ("秒 / SEG", sec_x, sec_w)] {
        draw_text_centered(&mut img, bold, 17.0, label, (x + w / 2) as f32, (hy + 10) as f32, INK);
    }
    fill_rect(&mut img, 0, hy + head_h - 2, base_w as i32, 2, INK);
    // linhas
    let mut y = title_h + head_h;
    let mut elapsed = 0.0f32;
    for (pn, row) in sb.panels.iter().zip(&rows) {
        let top = y + pad;
        // número
        draw_text_centered(&mut img, bold, 34.0, &pn.cut_number.to_string(), (no_x + no_w / 2) as f32, top as f32 + 6.0, INK);
        draw_text_centered(&mut img, font, 15.0, &format!("{} {}", pn.shot.code(), angle_label(pn.angle)), (no_x + no_w / 2) as f32, top as f32 + 58.0, GREY);
        // imagem
        let pi = panel_image(p, blobs, pn, img_w as u32, img_h as u32, Some([1.0, 1.0, 1.0]))?;
        img.draw_over(&pi, img_x, top, 1.0);
        if let Some(cam) = &pn.camera_move {
            draw_camera_overlay(&mut img, cam, img_x, top, img_w, img_h);
        }
        rect_outline(&mut img, img_x, top, img_w, img_h, 2, INK);
        // ação
        let mut ty = top as f32;
        for l in &row.act_lines {
            draw_text(&mut img, font, body_sz, l, act_x as f32, ty, INK);
            ty += lh as f32;
        }
        if let Some(cam) = &pn.camera_move {
            ty += 6.0;
            draw_text(&mut img, bold, 16.0, &format!("CAM: {}", camera_label(cam)), act_x as f32, ty, CAM_TO);
        }
        // fala / SE
        let mut ty = top as f32;
        for l in &row.dia_lines {
            draw_text(&mut img, font, body_sz, l, dia_x as f32, ty, INK);
            ty += lh as f32;
        }
        if !pn.sfx.is_empty() {
            ty += 8.0;
            for l in wrap_text(font, body_sz, &format!("SE: {}", pn.sfx), dia_w as f32) {
                draw_text(&mut img, font, body_sz, &l, dia_x as f32, ty, SFX_COL);
                ty += lh as f32;
            }
        }
        // segundos
        draw_text_centered(&mut img, bold, 26.0, &format!("{:.1}", pn.seconds), (sec_x + sec_w / 2) as f32, top as f32 + 6.0, INK);
        let m = (elapsed / 60.0).floor() as u32;
        draw_text_centered(&mut img, font, 14.0, &format!("{m}:{:04.1}", elapsed - m as f32 * 60.0), (sec_x + sec_w / 2) as f32, top as f32 + 44.0, GREY);
        elapsed += pn.seconds;
        y += row.h;
        fill_rect(&mut img, 0, y - 1, base_w as i32, 1, RULE);
    }
    draw_text(&mut img, font, 14.0, "Quadro de câmera: laranja = início, azul = fim · seta vermelha = direção do movimento", 30.0, y as f32 + 4.0, GREY);
    // divisórias verticais
    for x in [no_x + no_w + 4, img_x + img_w + 8, act_x + act_w + 8, dia_x + dia_w + 8] {
        fill_rect(&mut img, x, title_h + head_h, 1, y - (title_h + head_h), RULE);
    }
    if width != base_w {
        let s = width as f32 / base_w as f32;
        let (nw, nh) = (width, (total_h as f32 * s).round() as u32);
        img = transform_image(&img, Affine::from_trs(0.0, 0.0, s, 0.0, [0.0, 0.0]), nw, nh);
    }
    Ok(img)
}

// ---------------------------------------------------------------- animatic

/// Quadros de cada painel no animatic: `(painel, quadro inicial, duração em quadros)`. A duração é arredondada por
/// painel (mínimo 1 quadro) e o início é a soma das anteriores.
pub fn animatic_frames(p: &Project, _blobs: &BlobStore, storyboard: &Id, fps: f64) -> ToolResult<Vec<(Id, u32, u32)>> {
    let sb = storyboard_read(p, storyboard)?;
    if !(fps.is_finite() && fps > 0.0) {
        return Err(ToolError::Invalid("fps inválido".into()));
    }
    let mut start = 0u32;
    let mut out = Vec::new();
    for pn in &sb.panels {
        let dur = ((pn.seconds as f64 * fps).round() as u32).max(1);
        out.push((pn.id.clone(), start, dur));
        start += dur;
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct AnimaticOptions {
    /// Omitido = fps do projeto.
    #[serde(default)]
    pub fps: Option<f64>,
    /// Legenda (fala do painel) sobre a imagem — "silêncio com legenda" até a S17.
    #[serde(default)]
    pub captions: bool,
    /// Número do corte no canto.
    #[serde(default)]
    pub cut_numbers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnimaticReport {
    pub frames: u64,
    pub fps: f64,
    pub seconds: f64,
    pub panels: Vec<(Id, u32, u32)>,
}

pub fn render_animatic(p: &Project, blobs: &BlobStore, storyboard: &Id, out_path: &std::path::Path, w: u32, h: u32) -> ToolResult<AnimaticReport> {
    render_animatic_opts(p, blobs, storyboard, out_path, w, h, &AnimaticOptions::default())
}

/// Renderiza o animatic em MP4 (H.264, sem áudio): cada painel escalado para `w × h` e mantido pela duração.
pub fn render_animatic_opts(p: &Project, blobs: &BlobStore, storyboard: &Id, out_path: &std::path::Path, w: u32, h: u32, opts: &AnimaticOptions) -> ToolResult<AnimaticReport> {
    let sb = storyboard_read(p, storyboard)?;
    let fps = opts.fps.unwrap_or_else(|| p.settings.fps());
    let plan = animatic_frames(p, blobs, storyboard, fps)?;
    if plan.is_empty() {
        return Err(ToolError::Invalid("storyboard sem painéis".into()));
    }
    let (fps_num, fps_den) = if opts.fps.is_some() { ((fps * 1000.0).round() as u32, 1000) } else { (p.settings.fps_num, p.settings.fps_den) };
    let eopts = aiai_media::EncodeOptions { width: w, height: h, fps_num, fps_den, codec: aiai_media::VideoCodec::H264, crf: 20, audio: None, burn_subtitles: None, background: [1.0, 1.0, 1.0] };
    let mut enc = aiai_media::Encoder::start(out_path, &eopts).map_err(ToolError::Render)?;
    let font = default_font();
    for (pn, (_, _, dur)) in sb.panels.iter().zip(&plan) {
        let mut frame = panel_image(p, blobs, pn, w, h, Some([1.0, 1.0, 1.0]))?;
        if opts.cut_numbers {
            fill_rect(&mut frame, 8, 8, 70, 34, [0.1, 0.1, 0.15, 0.8]);
            draw_text(&mut frame, font, 22.0, &format!("C{:03}", pn.cut_number), 16.0, 12.0, [1.0, 1.0, 1.0, 1.0]);
        }
        if opts.captions && !pn.dialogue_text.trim().is_empty() {
            let size = (h as f32 * 0.055).max(12.0);
            let lines = wrap_text(font, size, &pn.dialogue_text, w as f32 * 0.86);
            let bar_h = (lines.len() as f32 * size * 1.4 + size * 0.6) as i32;
            fill_rect(&mut frame, 0, h as i32 - bar_h, w as i32, bar_h, [0.0, 0.0, 0.0, 0.6]);
            let mut y = (h as i32 - bar_h) as f32 + size * 0.3;
            for l in &lines {
                draw_text_centered(&mut frame, font, size, l, w as f32 / 2.0, y, [1.0, 1.0, 1.0, 1.0]);
                y += size * 1.4;
            }
        }
        for _ in 0..*dur {
            enc.push(&frame).map_err(ToolError::Render)?;
        }
    }
    let rep = enc.finish().map_err(ToolError::Render)?;
    Ok(AnimaticReport { frames: rep.frames, fps, seconds: rep.frames as f64 / fps, panels: plan })
}

// ---------------------------------------------------------------- cortes a partir do storyboard

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CutsFromStoryboardRequest {
    pub storyboard: Id,
    /// Episódio que recebe as cenas.
    pub episode: Id,
    /// `true`: uma cena por grupo de painéis consecutivos com a mesma `scene`; `false`: uma única cena.
    #[serde(default)]
    pub scene_per_panel_group: bool,
    /// Roteiro opcional: nomeia as cenas com o cabeçalho da cena correspondente.
    #[serde(default)]
    pub script: Option<Id>,
    /// Quadros por segundo para converter segundos em quadros; omitido = fps do projeto.
    #[serde(default)]
    pub fps: Option<f64>,
}

pub struct CutsFromStoryboard {
    /// Um único Batch (um passo de undo).
    pub command: Command,
    /// (painel, corte criado).
    pub cuts: Vec<(Id, Id)>,
    pub scenes: Vec<Id>,
}

fn camera_from_move(cam: &CameraMoveSpec, duration: u32, canvas: (u32, u32)) -> CameraData {
    let mut out = CameraData::default();
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    if let Some(f) = &cam.from {
        out.center = Some([f.center[0] * cw, f.center[1] * ch]);
        out.zoom = f.zoom;
    }
    if let (Some(a), Some(b)) = (&cam.from, &cam.to) {
        let last = duration.saturating_sub(1);
        let key = |v0: f32, v1: f32| {
            vec![aiai_core::keys::Key { frame: 0, value: v0, ease: aiai_core::keys::Ease::EaseInOut }, aiai_core::keys::Key { frame: last.max(1), value: v1, ease: aiai_core::keys::Ease::Linear }]
        };
        if duration > 1 {
            out.keys.0.insert("center_x".into(), key(a.center[0] * cw, b.center[0] * cw));
            out.keys.0.insert("center_y".into(), key(a.center[1] * ch, b.center[1] * ch));
            out.keys.0.insert("zoom".into(), key(a.zoom, b.zoom));
        }
    }
    out
}

/// Gera Scene/Cut a partir dos painéis: número = `cut_number`, duração = segundos × fps, falas vinculadas, painel de
/// origem, nota = ação, câmera inicial (e chaves, se houver `to`) e uma camada de **referência** com o desenho do painel
/// escalado ao tamanho do projeto (para o layout decalcar).
pub fn cuts_from_storyboard(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &CutsFromStoryboardRequest) -> ToolResult<CutsFromStoryboard> {
    let sb = storyboard_read(p, &req.storyboard)?;
    if sb.panels.is_empty() {
        return Err(ToolError::Invalid("storyboard sem painéis".into()));
    }
    let ep = node(p, &req.episode)?;
    if !matches!(ep.data, NodeKind::Episode(_)) {
        return Err(ToolError::Invalid(format!("{} não é episódio", req.episode)));
    }
    let script = match &req.script {
        Some(s) => Some(script_read(p, s)?),
        None => None,
    };
    let fps = req.fps.unwrap_or_else(|| p.settings.fps());
    let (pw, ph) = (p.settings.width, p.settings.height);
    let first_scene_no = ep.children.iter().filter(|c| matches!(p.node(c).map(|n| &n.data), Some(NodeKind::Scene(_)))).count() as u32;
    // grupos de painéis consecutivos
    let mut groups: Vec<Vec<&Panel>> = Vec::new();
    for pn in &sb.panels {
        if req.scene_per_panel_group {
            if let Some(g) = groups.last_mut() {
                if g[0].scene == pn.scene {
                    g.push(pn);
                    continue;
                }
            }
        } else if let Some(g) = groups.last_mut() {
            g.push(pn);
            continue;
        }
        groups.push(vec![pn]);
    }
    let mut cmds = Vec::new();
    let mut cuts = Vec::new();
    let mut scenes = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let scene_id = idgen.next();
        let heading = g[0].scene.as_ref().and_then(|sid| script.as_ref().and_then(|s| s.scene(sid)).map(|s| s.heading.clone()));
        let number = first_scene_no + gi as u32 + 1;
        let name = heading.clone().filter(|h| !h.is_empty()).unwrap_or_else(|| if req.scene_per_panel_group { format!("Cena {number}") } else { "Storyboard".into() });
        cmds.push(Command::CreateNode {
            id: Some(scene_id.clone()),
            parent: Some(req.episode.clone()),
            index: None,
            name,
            data: NodeKind::Scene(SceneData { number, description: heading.unwrap_or_default() }),
        });
        scenes.push(scene_id.clone());
        for pn in g {
            let duration = ((pn.seconds as f64 * fps).round() as u32).max(1);
            let mut cd = CutData::new(duration);
            cd.number = pn.cut_number;
            cd.notes = pn.action.clone();
            cd.dialogue_links = pn.dialogue_refs.clone();
            cd.storyboard_panel = Some(pn.id.clone());
            if let Some(cam) = &pn.camera_move {
                cd.camera = camera_from_move(cam, duration, (pw, ph));
            }
            let cut_id = idgen.next();
            cmds.push(Command::CreateNode { id: Some(cut_id.clone()), parent: Some(scene_id.clone()), index: None, name: format!("C{:03}", pn.cut_number), data: NodeKind::Cut(cd) });
            if pn.drawing_cut.is_some() {
                // desenho do painel sem fundo (só traços), escalado ao tamanho do projeto
                let img = panel_image(p, blobs, pn, pw, ph, None)?;
                let png = img.encode_png().map_err(ToolError::Render)?;
                let hash = blobs.put(png);
                let layer_id = idgen.next();
                let mut ld = LayerData::new(LayerType::Reference);
                ld.opacity = 0.6;
                cmds.push(Command::CreateNode { id: Some(layer_id.clone()), parent: Some(cut_id.clone()), index: None, name: "storyboard (referência)".into(), data: NodeKind::Layer(ld) });
                let mut cel = CelData::new(1);
                cel.image = Some(hash);
                cmds.push(Command::CreateNode { id: Some(idgen.next()), parent: Some(layer_id), index: None, name: "ref1".into(), data: NodeKind::Cel(cel) });
            }
            cuts.push((pn.id.clone(), cut_id));
        }
    }
    Ok(CutsFromStoryboard { command: Command::Batch { commands: cmds, label: Some(format!("Cortes do storyboard ({} painéis)", sb.panels.len())) }, cuts, scenes })
}

/// Valor JSON bruto do conteúdo de qualquer documento (para a bridge/MCP listarem sem conhecer o esquema).
pub fn document_content(p: &Project, id: &Id) -> ToolResult<Value> {
    match &node(p, id)?.data {
        NodeKind::Document(d) => Ok(d.content.clone()),
        other => Err(ToolError::Invalid(format!("{id} é {}, não documento", other.type_name()))),
    }
}
