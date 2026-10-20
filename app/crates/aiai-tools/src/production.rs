//! S19 — produção e revisão: tabela de cortes (カット表), estado por etapa, correções (camada vermelha), retakes
//! comparáveis, proveniência e relatório de produção.
//!
//! O estado estruturado de correções e retakes mora num documento Notes raiz chamado "Produção" com
//! `schema: "production"` ([`ProductionDoc`]). Os snapshots dos retakes são PNG em blobs, e cada um é referenciado
//! por um **asset** (`AssetType::Texture`, `source: "retake"`), porque `Project::referenced_blobs` só enxerga blobs de
//! cels e assets — um blob citado apenas no JSON de um documento seria coletado como lixo (ver relatório: mudança
//! sugerida no núcleo).

use crate::context::*;
use crate::draw::CurveSpec;
use crate::fill::lint;
use crate::fx2d::{default_font, heavy_font};
use crate::preprod::{create_doc_command, doc_content, draw_text, draw_text_centered, fill_rect, rect_outline, replace_doc_command};
use aiai_core::command::CellEdit;
use aiai_core::*;
use aiai_paint::{Affine, Image, transform_image};
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const ALL_STAGES: [Stage; 8] = [Stage::Layout, Stage::Genga, Stage::Douga, Stage::Shiage, Stage::Background, Stage::Satsuei, Stage::Color, Stage::Sound];

pub fn stage_label(s: Stage) -> &'static str {
    match s {
        Stage::Layout => "LO",
        Stage::Genga => "Genga",
        Stage::Douga => "Douga",
        Stage::Shiage => "Shiage",
        Stage::Background => "BG",
        Stage::Satsuei => "Satsuei",
        Stage::Color => "Cor",
        Stage::Sound => "Som",
    }
}

pub fn status_label(s: StageStatus) -> &'static str {
    match s {
        StageStatus::NotStarted => "não iniciado",
        StageStatus::InProgress => "em andamento",
        StageStatus::Review => "para revisão",
        StageStatus::Approved => "aprovado",
        StageStatus::Retake => "retake",
    }
}

fn status_short(s: StageStatus) -> &'static str {
    match s {
        StageStatus::NotStarted => "-",
        StageStatus::InProgress => "and.",
        StageStatus::Review => "rev.",
        StageStatus::Approved => "ok",
        StageStatus::Retake => "RETAKE",
    }
}

// ================================================================ documento de produção

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Correction {
    pub id: Id,
    pub cut: Id,
    /// Quadro 0-based.
    pub frame: u32,
    pub note: String,
    #[serde(default)]
    pub resolved: bool,
    pub author: String,
    /// Camada de correção e cel com os traços vermelhos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cel: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
    /// Retake que resolveu a correção.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retake: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Retake {
    pub id: Id,
    pub cut: Id,
    pub frame: u32,
    pub reason: String,
    /// Revisão do projeto no momento do snapshot.
    pub revision: u64,
    /// Asset que mantém o blob vivo.
    pub snapshot_asset: Id,
    pub snapshot: BlobHash,
    pub author: String,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProductionDoc {
    #[serde(default = "prod_schema")]
    pub schema: String,
    #[serde(default)]
    pub corrections: Vec<Correction>,
    #[serde(default)]
    pub retakes: Vec<Retake>,
    /// Notas da gestão (do usuário ou do agente), com carimbo de hora; a análise por IA devolve as dela aqui.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ProdNote>,
}

/// Nota de gestão do documento "Produção": livre ou ligada a um corte; `ai` marca o que veio da análise de um agente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProdNote {
    pub id: Id,
    /// Instante da nota (ms desde a época Unix).
    pub t_ms: u64,
    pub author: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<Id>,
    /// Gerada por IA (nunca aplica mudanças: é só texto para a gestão ler).
    #[serde(default)]
    pub ai: bool,
}

fn prod_schema() -> String {
    "production".into()
}

impl Default for ProductionDoc {
    fn default() -> Self {
        ProductionDoc { schema: prod_schema(), corrections: Vec::new(), retakes: Vec::new(), notes: Vec::new() }
    }
}

pub const PRODUCTION_DOC_NAME: &str = "Produção";

/// Documento de produção do projeto (se já existe).
pub fn find_production_doc(p: &Project) -> Option<Id> {
    p.roots.documents.iter().find(|id| match p.node(id).map(|n| &n.data) {
        Some(NodeKind::Document(d)) => d.doc == DocumentType::Notes && d.content.get("schema").and_then(|v| v.as_str()) == Some("production"),
        _ => false,
    }).cloned()
}

pub fn production_read(p: &Project) -> ToolResult<ProductionDoc> {
    match find_production_doc(p) {
        Some(id) => doc_content(p, &id, DocumentType::Notes),
        None => Ok(ProductionDoc::default()),
    }
}

/// Comando que grava `doc`: `ReplaceData` se o documento existe, senão cria o documento raiz "Produção".
fn commit_production(p: &Project, idgen: &mut IdGen, doc: &ProductionDoc) -> ToolResult<Command> {
    match find_production_doc(p) {
        Some(id) => replace_doc_command(p, &id, doc),
        None => create_doc_command(&idgen.next(), None, PRODUCTION_DOC_NAME, DocumentType::Notes, doc),
    }
}

// ================================================================ tabela de cortes

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CutRow {
    pub cut: Id,
    pub number: u32,
    pub name: String,
    pub episode: Option<Id>,
    pub scene: Option<Id>,
    pub scene_name: String,
    pub duration: u32,
    /// Todas as 8 etapas (ausente no corte = não iniciado).
    pub stages: BTreeMap<Stage, StageStatus>,
    pub open_corrections: usize,
    /// Retakes do corte (todos) e os ainda abertos.
    pub retakes: usize,
    pub open_retakes: usize,
    /// Gestão: prazo (`AAAA-MM-DD`), responsável e peso do corte (opcionais).
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub weight: Option<f32>,
    /// Notas do corte (campo `notes` do `CutData`).
    #[serde(default)]
    pub notes: String,
    /// PNG do quadro 0 (só em [`cut_table_with_thumbs`]).
    pub thumbnail: Option<BlobHash>,
}

/// Cortes na ordem do documento (episódio → cena → corte).
pub fn cut_table(p: &Project) -> Vec<CutRow> {
    let prod = production_read(p).unwrap_or_default();
    let mut rows = Vec::new();
    for ep in &p.roots.episodes {
        let Some(epn) = p.node(ep) else { continue };
        for sc in &epn.children {
            let Some(scn) = p.node(sc) else { continue };
            if !matches!(scn.data, NodeKind::Scene(_)) {
                continue;
            }
            for cid in &scn.children {
                let Some(cn) = p.node(cid) else { continue };
                let NodeKind::Cut(cd) = &cn.data else { continue };
                let stages = ALL_STAGES.iter().map(|s| (*s, cd.stages.get(s).copied().unwrap_or_default())).collect();
                let rts: Vec<&Retake> = prod.retakes.iter().filter(|r| &r.cut == cid).collect();
                rows.push(CutRow {
                    cut: cid.clone(),
                    number: cd.number,
                    name: cn.name.clone(),
                    episode: Some(ep.clone()),
                    scene: Some(sc.clone()),
                    scene_name: scn.name.clone(),
                    duration: cd.duration,
                    stages,
                    open_corrections: prod.corrections.iter().filter(|c| &c.cut == cid && !c.resolved).count(),
                    retakes: rts.len(),
                    open_retakes: rts.iter().filter(|r| !r.resolved).count(),
                    due: cd.due.clone(),
                    assignee: cd.assignee.clone(),
                    weight: cd.weight,
                    notes: cd.notes.clone(),
                    thumbnail: None,
                });
            }
        }
    }
    rows
}

fn white_render_opts() -> RenderOptions {
    RenderOptions { include_guides: false, include_reference: false, only_layers: None, background: Some([1.0, 1.0, 1.0]), camera: true, layer_motion: true }
}

/// Render do quadro `frame` do corte reduzido para `max_w` (mantendo a proporção) sobre fundo branco.
pub fn cut_thumbnail(p: &Project, blobs: &BlobStore, cut: &Id, frame: u32, max_w: u32) -> ToolResult<Image> {
    let out = render_frame(p, blobs, cut, frame, &white_render_opts()).map_err(|e| ToolError::Render(e.to_string()))?;
    let s = (max_w as f32 / out.image.width as f32).min(1.0);
    let (w, h) = (((out.image.width as f32 * s).round() as u32).max(1), ((out.image.height as f32 * s).round() as u32).max(1));
    Ok(transform_image(&out.image, Affine::from_trs(0.0, 0.0, s, 0.0, [0.0, 0.0]), w, h))
}

/// Como [`cut_table`], com miniaturas PNG guardadas em `blobs` (cache; não entram no documento).
pub fn cut_table_with_thumbs(p: &Project, blobs: &mut BlobStore, max_w: u32) -> ToolResult<Vec<CutRow>> {
    let mut rows = cut_table(p);
    for r in &mut rows {
        let img = cut_thumbnail(p, blobs, &r.cut, 0, max_w)?;
        r.thumbnail = Some(blobs.put(img.encode_png().map_err(ToolError::Render)?));
    }
    Ok(rows)
}

/// Muda o estado de uma etapa do corte (merge patch em `stages`).
pub fn set_stage(p: &Project, cut: &Id, stage: Stage, status: StageStatus) -> ToolResult<Command> {
    cut_data(p, cut)?;
    let s = serde_json::to_value(stage).map_err(|e| ToolError::Invalid(e.to_string()))?;
    let st = serde_json::to_value(status).map_err(|e| ToolError::Invalid(e.to_string()))?;
    let key = s.as_str().ok_or_else(|| ToolError::Invalid("etapa".into()))?.to_string();
    Ok(Command::SetProps { id: cut.clone(), name: None, data: json!({ "stages": { key: st } }) })
}

/// Muda a mesma etapa de vários cortes num único comando (um passo de undo). Cortes já no estado pedido são pulados.
pub fn set_stage_many(p: &Project, cuts: &[Id], stage: Stage, status: StageStatus) -> ToolResult<Option<Command>> {
    let mut cmds = Vec::new();
    for c in cuts {
        let cd = cut_data(p, c)?;
        if cd.stages.get(&stage).copied().unwrap_or_default() != status {
            cmds.push(set_stage(p, c, stage, status)?);
        }
    }
    Ok(match cmds.len() {
        0 => None,
        1 => cmds.pop(),
        n => Some(Command::Batch { commands: cmds, label: Some(format!("Etapa {} → {} em {n} cortes", stage_label(stage), status_label(status))) }),
    })
}

/// Pedido de mudança de gestão (prazo, responsável, peso) em vários cortes. Texto vazio (ou peso <= 0) limpa o campo;
/// `None` deixa como está.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CutMetaPatch {
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub weight: Option<f32>,
}

/// Comando que aplica `patch` a todos os `cuts` (um passo de undo).
pub fn set_cut_meta(p: &Project, cuts: &[Id], patch: &CutMetaPatch) -> ToolResult<Command> {
    let mut data = serde_json::Map::new();
    if let Some(d) = &patch.due {
        let d = d.trim();
        if !d.is_empty() && crate::production_history::parse_date(d).is_none() {
            return Err(ToolError::Invalid(format!("prazo deve ser AAAA-MM-DD: {d}")));
        }
        data.insert("due".into(), if d.is_empty() { serde_json::Value::Null } else { json!(d) });
    }
    if let Some(a) = &patch.assignee {
        let a = a.trim();
        data.insert("assignee".into(), if a.is_empty() { serde_json::Value::Null } else { json!(a) });
    }
    if let Some(w) = patch.weight {
        if w.is_nan() || w > 1000.0 {
            return Err(ToolError::Invalid(format!("peso fora do intervalo: {w}")));
        }
        data.insert("weight".into(), if w <= 0.0 { serde_json::Value::Null } else { json!(w) });
    }
    if data.is_empty() {
        return Err(ToolError::Invalid("nada a mudar: informe due, assignee e/ou weight".into()));
    }
    let mut cmds = Vec::new();
    for c in cuts {
        cut_data(p, c)?;
        cmds.push(Command::SetProps { id: c.clone(), name: None, data: serde_json::Value::Object(data.clone()) });
    }
    if cmds.is_empty() {
        return Err(ToolError::Invalid("nenhum corte informado".into()));
    }
    Ok(if cmds.len() == 1 { cmds.remove(0) } else { Command::Batch { label: Some(format!("Gestão de {} cortes", cmds.len())), commands: cmds } })
}

/// Anexa uma nota de gestão ao documento "Produção" (cria o documento se preciso).
pub fn note_add(p: &Project, idgen: &mut IdGen, now_ms: u64, author: &str, text: &str, cut: Option<Id>, ai: bool) -> ToolResult<(Id, Command)> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ToolError::Invalid("nota vazia".into()));
    }
    if let Some(c) = &cut {
        cut_data(p, c)?;
    }
    let mut doc = production_read(p)?;
    let id = idgen.next();
    doc.notes.push(ProdNote { id: id.clone(), t_ms: now_ms, author: author.to_owned(), text: text.to_owned(), cut, ai });
    Ok((id, commit_production(p, idgen, &doc)?))
}

/// Remove uma nota de gestão.
pub fn note_delete(p: &Project, idgen: &mut IdGen, note: &Id) -> ToolResult<Command> {
    let mut doc = production_read(p)?;
    let before = doc.notes.len();
    doc.notes.retain(|n| &n.id != note);
    if doc.notes.len() == before {
        return Err(ToolError::NotFound(note.to_string()));
    }
    commit_production(p, idgen, &doc)
}

// ================================================================ estados automáticos

fn rank(s: StageStatus) -> Option<u8> {
    match s {
        StageStatus::NotStarted => Some(0),
        StageStatus::InProgress => Some(1),
        StageStatus::Review => Some(2),
        StageStatus::Approved | StageStatus::Retake => None,
    }
}

fn cel_has_content(c: &CelData) -> bool {
    !c.strokes.is_empty() || !c.tiles.is_empty() || !c.fills.is_empty() || c.image.is_some() || !c.text.is_empty()
}

/// Camadas (com dados) da subárvore do corte.
fn cut_layers<'a>(p: &'a Project, cut: &Id) -> Vec<(&'a Node, &'a LayerData)> {
    p.subtree(cut)
        .iter()
        .filter_map(|id| p.node(id))
        .filter_map(|n| match &n.data {
            NodeKind::Layer(l) => Some((n, l)),
            _ => None,
        })
        .collect()
}

fn layer_cels<'a>(p: &'a Project, layer: &Node) -> Vec<&'a CelData> {
    layer.children.iter().filter_map(|c| p.node(c)).filter_map(|n| if let NodeKind::Cel(c) = &n.data { Some(c) } else { None }).collect()
}

/// Um quadro por cel distinto exposto pela camada (o primeiro em que aparece).
pub fn exposed_cel_frames(p: &Project, cut: &CutData, layer: &Id) -> Vec<(u32, Id)> {
    let mut frames = cut.xsheet.change_frames(cut.duration);
    frames.insert(0, 0);
    frames.sort_unstable();
    frames.dedup();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for f in frames {
        if let Some(cel) = exposed_cel(p, cut, layer, f) {
            if seen.insert(cel.id.clone()) {
                out.push((f, cel.id.clone()));
            }
        }
    }
    out
}

/// Resultado do lint de uma camada de preenchimento ao longo dos cels expostos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FillLintSummary {
    pub layer: Id,
    pub layer_name: String,
    pub frames_checked: usize,
    pub unpainted_regions: usize,
    pub unpainted_area: usize,
    pub painted_regions: usize,
    /// Regiões cortadas pela borda do quadro sem tinta (fora a maior, que é o "fora"): aviso para conferir no olho.
    #[serde(default)]
    pub border_unpainted_regions: usize,
}

pub fn lint_fill_layer(p: &Project, blobs: &BlobStore, cut_id: &Id, layer: &Id) -> ToolResult<FillLintSummary> {
    let cut = cut_data(p, cut_id)?;
    let name = node(p, layer)?.name.clone();
    let mut s = FillLintSummary { layer: layer.clone(), layer_name: name, frames_checked: 0, unpainted_regions: 0, unpainted_area: 0, painted_regions: 0, border_unpainted_regions: 0 };
    for (frame, _) in exposed_cel_frames(p, cut, layer) {
        let rep = lint(p, blobs, cut_id, frame, layer, 3.0, 30)?;
        s.frames_checked += 1;
        s.unpainted_regions += rep.unpainted.len();
        s.unpainted_area += rep.unpainted.iter().map(|r| r.area).sum::<usize>();
        s.painted_regions += rep.painted;
        s.border_unpainted_regions += rep.border_unpainted.len();
    }
    Ok(s)
}

/// Sugere avanços de etapa a partir do conteúdo do corte. Só devolve o que avança o estado atual (nunca mexe em
/// etapas aprovadas ou em retake).
pub fn suggest_stages(p: &Project, blobs: &BlobStore, cut_id: &Id) -> ToolResult<Vec<(Stage, StageStatus, String)>> {
    let cut = cut_data(p, cut_id)?;
    let layers = cut_layers(p, cut_id);
    let mut out: Vec<(Stage, StageStatus, String)> = Vec::new();
    let with_role = |role: LayerRole| -> Vec<(&Node, &LayerData)> { layers.iter().filter(|(_, l)| l.role == role).cloned().collect() };
    let strokes_in = |ls: &[(&Node, &LayerData)]| -> (usize, usize) {
        let mut cels = 0;
        let mut strokes = 0;
        for (n, _) in ls {
            for c in layer_cels(p, n) {
                if cel_has_content(c) {
                    cels += 1;
                    strokes += c.strokes.len();
                }
            }
        }
        (cels, strokes)
    };
    let lo = with_role(LayerRole::Layout);
    let (c, s) = strokes_in(&lo);
    if c > 0 {
        out.push((Stage::Layout, StageStatus::InProgress, format!("{c} cel(s) de layout com conteúdo ({s} traços)")));
    }
    let ge = with_role(LayerRole::Genga);
    let (c, s) = strokes_in(&ge);
    if c > 0 {
        out.push((Stage::Genga, StageStatus::InProgress, format!("{c} cel(s)-chave de genga com traços ({s} traços)")));
    }
    let dou = with_role(LayerRole::Douga);
    let (c, s) = strokes_in(&dou);
    if c > 0 {
        out.push((Stage::Douga, StageStatus::InProgress, format!("{c} cel(s) de douga com traços ({s} traços)")));
    }
    // shiage: camadas de preenchimento
    let fills: Vec<&(&Node, &LayerData)> = layers.iter().filter(|(_, l)| l.kind == LayerType::Fill).collect();
    if !fills.is_empty() {
        let any_fill = fills.iter().any(|(n, _)| layer_cels(p, n).iter().any(|c| !c.fills.is_empty()));
        let mut summaries = Vec::new();
        let mut lint_err = None;
        for (n, l) in &fills {
            if l.line_refs.is_empty() {
                continue;
            }
            match lint_fill_layer(p, blobs, cut_id, &n.id) {
                Ok(s) => summaries.push(s),
                Err(e) => lint_err = Some(format!("{}: {e}", n.name)),
            }
        }
        let checked: usize = summaries.iter().map(|s| s.frames_checked).sum();
        let unpainted: usize = summaries.iter().map(|s| s.unpainted_regions).sum();
        let painted: usize = summaries.iter().map(|s| s.painted_regions).sum();
        if lint_err.is_none() && checked > 0 && unpainted == 0 && painted > 0 {
            out.push((Stage::Shiage, StageStatus::Review, format!("lint: 0 regiões sem pintar em {checked} cel(s) exposto(s) ({painted} regiões pintadas)")));
        } else if any_fill {
            let why = match lint_err {
                Some(e) => format!("lint não concluído ({e})"),
                None => format!("{unpainted} região(ões) sem pintar em {checked} cel(s)"),
            };
            out.push((Stage::Shiage, StageStatus::InProgress, format!("há preenchimentos; {why}")));
        }
    }
    let bg: Vec<(&Node, &LayerData)> = layers.iter().filter(|(_, l)| matches!(l.role, LayerRole::Background | LayerRole::Book)).cloned().collect();
    let (c, _) = strokes_in(&bg);
    if c > 0 {
        out.push((Stage::Background, StageStatus::InProgress, format!("{c} cel(s) de BG/BOOK com conteúdo")));
    }
    let fx = layers.iter().filter(|(_, l)| matches!(l.kind, LayerType::Effect | LayerType::Adjustment | LayerType::Particles)).count();
    let cam = !cut.camera.keys.is_empty() || cut.camera.zoom != 1.0 || cut.camera.center.is_some();
    if fx > 0 || cam {
        out.push((Stage::Satsuei, StageStatus::InProgress, format!("{fx} camada(s) de efeito/ajuste/partículas{}", if cam { ", câmera animada" } else { "" })));
    }
    if !cut.audio.is_empty() {
        out.push((Stage::Sound, StageStatus::InProgress, format!("{} clipe(s) de áudio no corte", cut.audio.len())));
    }
    out.retain(|(st, new, _)| {
        let cur = cut.stages.get(st).copied().unwrap_or_default();
        match (rank(cur), rank(*new)) {
            (Some(a), Some(b)) => b > a,
            _ => false,
        }
    });
    Ok(out)
}

/// Aplica as sugestões num único Batch (None se não há nada a mudar).
pub fn apply_stage_suggestions(p: &Project, cut: &Id, suggestions: &[(Stage, StageStatus, String)]) -> ToolResult<Option<Command>> {
    if suggestions.is_empty() {
        return Ok(None);
    }
    let cmds = suggestions.iter().map(|(s, st, _)| set_stage(p, cut, *s, *st)).collect::<ToolResult<Vec<_>>>()?;
    Ok(Some(Command::Batch { commands: cmds, label: Some("Estados automáticos das etapas".into()) }))
}

// ================================================================ correções

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CorrectionRequest {
    pub cut: Id,
    /// Quadro 0-based.
    pub frame: u32,
    pub note: String,
    /// Traços da correção (sempre gravados em vermelho).
    #[serde(default)]
    pub strokes: Vec<CurveSpec>,
    /// Quem registra (padrão "agent").
    #[serde(default)]
    pub author: Option<String>,
}

pub struct CorrectionAdded {
    pub id: Id,
    pub command: Command,
}

pub const CORRECTION_LAYER: &str = "correção";
pub const CORRECTION_RED: &str = "#ff2a2a";

fn correction_layer(p: &Project, cut: &Id) -> Option<Id> {
    p.node(cut)?.children.iter().find(|c| matches!(p.node(c).map(|n| &n.data), Some(NodeKind::Layer(l)) if l.role == LayerRole::Correction)).cloned()
}

/// Registra uma correção: garante a camada Guide (papel Correction, não renderiza) com trilha no timesheet, cria o cel
/// do quadro com os traços vermelhos e grava `{id, quadro, nota, resolvida: false, autor}` no documento "Produção".
pub fn correction_add(p: &Project, idgen: &mut IdGen, req: &CorrectionRequest) -> ToolResult<CorrectionAdded> {
    let cut = cut_data(p, &req.cut)?;
    if req.frame >= cut.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora da duração {}", req.frame, cut.duration)));
    }
    let red = ColorRef::Direct { rgba: Rgba::from_hex(CORRECTION_RED).expect("cor fixa") };
    let mut strokes = Vec::new();
    for c in &req.strokes {
        let mut s = c.to_stroke()?;
        s.color = red.clone();
        strokes.push(s);
    }
    let mut cmds = Vec::new();
    let existing_layer = correction_layer(p, &req.cut);
    let (layer_id, track_id, track_cells): (Id, Id, BTreeMap<u32, CellRef>) = match &existing_layer {
        Some(l) => match cut.xsheet.track_for_layer(l) {
            Some(t) => (l.clone(), t.id.clone(), t.cells.clone()),
            None => {
                let tid = idgen.next();
                cmds.push(Command::AddTrack { cut: req.cut.clone(), index: None, track: Track::new(tid.clone(), "COR", TrackKind::Drawing, Some(l.clone())) });
                (l.clone(), tid, BTreeMap::new())
            }
        },
        None => {
            let lid = idgen.next();
            let mut ld = LayerData::new(LayerType::Guide);
            ld.role = LayerRole::Correction;
            cmds.push(Command::CreateNode { id: Some(lid.clone()), parent: Some(req.cut.clone()), index: None, name: CORRECTION_LAYER.into(), data: NodeKind::Layer(ld) });
            let tid = idgen.next();
            cmds.push(Command::AddTrack { cut: req.cut.clone(), index: None, track: Track::new(tid.clone(), "COR", TrackKind::Drawing, Some(lid.clone())) });
            (lid, tid, BTreeMap::new())
        }
    };
    let number = req.frame + 1;
    let existing_cel = existing_layer.as_ref().and_then(|l| p.cel_by_number(l, number));
    let cel_id = match existing_cel {
        Some(cn) => {
            if !strokes.is_empty() {
                let NodeKind::Cel(cd) = &cn.data else { unreachable!() };
                let items = strokes.into_iter().enumerate().map(|(i, s)| aiai_core::command::IndexedStroke { index: cd.strokes.len() + i, stroke: s }).collect();
                cmds.push(Command::InsertStrokes { cel: cn.id.clone(), items });
            }
            cn.id.clone()
        }
        None => {
            let id = idgen.next();
            let mut cel = CelData::new(number);
            cel.strokes = strokes;
            cmds.push(Command::CreateNode { id: Some(id.clone()), parent: Some(layer_id.clone()), index: None, name: format!("cor{number}"), data: NodeKind::Cel(cel) });
            id
        }
    };
    // exposição: o cel aparece só no quadro da correção (o seguinte volta a vazio, salvo se já houver entrada)
    let mut cells = Vec::new();
    if track_cells.get(&req.frame) != Some(&CellRef::Cel { number }) {
        cells.push(CellEdit { frame: req.frame, cell: Some(CellRef::Cel { number }) });
    }
    if req.frame + 1 < cut.duration && !track_cells.contains_key(&(req.frame + 1)) {
        cells.push(CellEdit { frame: req.frame + 1, cell: Some(CellRef::Empty) });
    }
    if !cells.is_empty() {
        cmds.push(Command::SetCells { cut: req.cut.clone(), track: track_id, cells });
    }
    let mut prod = production_read(p)?;
    let id = idgen.next();
    prod.corrections.push(Correction {
        id: id.clone(),
        cut: req.cut.clone(),
        frame: req.frame,
        note: req.note.clone(),
        resolved: false,
        author: req.author.clone().unwrap_or_else(|| "agent".into()),
        layer: Some(layer_id),
        cel: Some(cel_id),
        resolved_by: None,
        retake: None,
    });
    cmds.push(commit_production(p, idgen, &prod)?);
    Ok(CorrectionAdded { id, command: Command::Batch { commands: cmds, label: Some(format!("Correção no quadro {}", req.frame + 1)) } })
}

/// Marca a correção como resolvida (ou reabre com `resolved = false`); `retake` opcional liga ao retake.
pub fn correction_resolve(p: &Project, correction: &Id, resolved: bool, by: &str, retake: Option<Id>) -> ToolResult<Command> {
    let doc_id = find_production_doc(p).ok_or_else(|| ToolError::NotFound("documento de produção".into()))?;
    let mut prod = production_read(p)?;
    let c = prod.corrections.iter_mut().find(|c| &c.id == correction).ok_or_else(|| ToolError::NotFound(format!("correção {correction}")))?;
    c.resolved = resolved;
    c.resolved_by = resolved.then(|| by.to_string());
    c.retake = if resolved { retake.clone() } else { None };
    if let Some(r) = &retake {
        if let Some(rt) = prod.retakes.iter_mut().find(|r2| &r2.id == r) {
            rt.correction = Some(correction.clone());
        }
    }
    replace_doc_command(p, &doc_id, &prod)
}

pub fn corrections_for_cut(p: &Project, cut: &Id) -> ToolResult<Vec<Correction>> {
    Ok(production_read(p)?.corrections.into_iter().filter(|c| &c.cut == cut).collect())
}

// ================================================================ retakes

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RetakeRequest {
    pub cut: Id,
    pub frame: u32,
    pub reason: String,
    #[serde(default)]
    pub author: Option<String>,
}

pub struct RetakeCreated {
    pub id: Id,
    pub command: Command,
}

/// Cria o retake: guarda o render atual do quadro (PNG sobre branco) como snapshot, com motivo e revisão.
pub fn retake_create(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &RetakeRequest) -> ToolResult<RetakeCreated> {
    let cut = cut_data(p, &req.cut)?;
    if req.frame >= cut.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora da duração {}", req.frame, cut.duration)));
    }
    let out = render_frame(p, blobs, &req.cut, req.frame, &white_render_opts()).map_err(|e| ToolError::Render(e.to_string()))?;
    let hash = blobs.put(out.image.encode_png().map_err(ToolError::Render)?);
    let rid = idgen.next();
    let asset_id = idgen.next();
    let cut_name = node(p, &req.cut)?.name.clone();
    let asset = Command::CreateNode {
        id: Some(asset_id.clone()),
        parent: None,
        index: None,
        name: format!("Retake {cut_name} q{}", req.frame + 1),
        data: NodeKind::Asset(AssetData { asset: AssetType::Texture, blob: Some(hash.clone()), params: json!({"retake": rid.0, "cut": req.cut.0, "frame": req.frame}), source: "retake".into() }),
    };
    let mut prod = production_read(p)?;
    prod.retakes.push(Retake {
        id: rid.clone(),
        cut: req.cut.clone(),
        frame: req.frame,
        reason: req.reason.clone(),
        revision: p.revision,
        snapshot_asset: asset_id,
        snapshot: hash,
        author: req.author.clone().unwrap_or_else(|| "agent".into()),
        resolved: false,
        correction: None,
    });
    let commit = commit_production(p, idgen, &prod)?;
    Ok(RetakeCreated { id: rid, command: Command::Batch { commands: vec![asset, commit], label: Some(format!("Retake {cut_name}")) } })
}

/// Marca o retake como resolvido (a refeitura foi aprovada).
pub fn retake_resolve(p: &Project, retake: &Id, resolved: bool) -> ToolResult<Command> {
    let doc_id = find_production_doc(p).ok_or_else(|| ToolError::NotFound("documento de produção".into()))?;
    let mut prod = production_read(p)?;
    prod.retakes.iter_mut().find(|r| &r.id == retake).ok_or_else(|| ToolError::NotFound(format!("retake {retake}")))?.resolved = resolved;
    replace_doc_command(p, &doc_id, &prod)
}

pub struct RetakeCompare {
    pub before: Image,
    pub after: Image,
    /// Mapa de calor da diferença absoluta sobre o "depois" esmaecido.
    pub diff: Image,
    pub changed_pixels: usize,
    pub changed_fraction: f32,
    pub mean_abs_diff: f32,
    pub max_diff: f32,
}

const DIFF_THRESHOLD: f32 = 0.04;

pub fn diff_images(before: &Image, after: &Image) -> ToolResult<(Image, usize, f32, f32)> {
    if before.width != after.width || before.height != after.height {
        return Err(ToolError::Invalid(format!("tamanhos diferentes: {}x{} vs {}x{}", before.width, before.height, after.width, after.height)));
    }
    let mut diff = Image::new(after.width, after.height);
    let (mut changed, mut sum, mut max) = (0usize, 0.0f64, 0.0f32);
    for (i, (b, a)) in before.data.iter().zip(&after.data).enumerate() {
        // as imagens são achatadas sobre branco (alfa 1); compara RGB
        let d = (0..3).map(|c| (b[c] - a[c]).abs()).fold(0.0f32, f32::max);
        sum += d as f64;
        max = max.max(d);
        let lum = 0.3 * a[0] + 0.59 * a[1] + 0.11 * a[2];
        let base = 0.78 + 0.22 * lum;
        let mut px = [base, base, base, 1.0];
        if d > DIFF_THRESHOLD {
            changed += 1;
            let t = (d * 2.5).min(1.0);
            let heat = [1.0, 0.85 * t, 0.0];
            let o = 0.55 + 0.45 * t;
            for c in 0..3 {
                px[c] = px[c] * (1.0 - o) + heat[c] * o;
            }
        }
        diff.data[i] = px;
    }
    Ok((diff, changed, (sum / before.data.len().max(1) as f64) as f32, max))
}

/// Compara o snapshot do retake com o render atual do quadro `frame` (omitido = o quadro do retake).
pub fn retake_compare(p: &Project, blobs: &BlobStore, retake: &Id, frame: Option<u32>) -> ToolResult<RetakeCompare> {
    let prod = production_read(p)?;
    let r = prod.retakes.iter().find(|r| &r.id == retake).ok_or_else(|| ToolError::NotFound(format!("retake {retake}")))?;
    let bytes = blobs.get(&r.snapshot).ok_or_else(|| ToolError::NotFound(format!("blob do snapshot {}", r.snapshot)))?;
    let before = Image::decode(&bytes).map_err(ToolError::Render)?;
    let after = render_frame(p, blobs, &r.cut, frame.unwrap_or(r.frame), &white_render_opts()).map_err(|e| ToolError::Render(e.to_string()))?.image;
    let (diff, changed, mean, max) = diff_images(&before, &after)?;
    let n = before.data.len().max(1);
    Ok(RetakeCompare { before, after, diff, changed_pixels: changed, changed_fraction: changed as f32 / n as f32, mean_abs_diff: mean, max_diff: max })
}

/// Folha lado a lado ANTES | DEPOIS | DIFERENÇA, com legendas (cada painel reduzido a `panel_w`).
pub fn retake_compare_sheet(cmp: &RetakeCompare, retake: &Retake, panel_w: u32) -> Image {
    let font = default_font();
    let bold = heavy_font();
    let s = panel_w as f32 / cmp.before.width as f32;
    let ph = (cmp.before.height as f32 * s).round() as u32;
    let (gap, top, bottom) = (16u32, 70u32, 44u32);
    let w = panel_w * 3 + gap * 4;
    let h = top + ph + bottom;
    let mut sheet = Image::filled(w, h, [0.97, 0.97, 0.98, 1.0]);
    let scale = |img: &Image| transform_image(img, Affine::from_trs(0.0, 0.0, s, 0.0, [0.0, 0.0]), panel_w, ph);
    let labels = [
        format!("ANTES  (retake, rev. {})", retake.revision),
        "DEPOIS  (estado atual)".to_string(),
        format!("DIFERENÇA  ({:.2}% dos pixels)", cmp.changed_fraction * 100.0),
    ];
    for (i, (img, label)) in [&cmp.before, &cmp.after, &cmp.diff].into_iter().zip(&labels).enumerate() {
        let x = (gap + i as u32 * (panel_w + gap)) as i32;
        draw_text(&mut sheet, bold, 22.0, label, x as f32, 36.0, [0.1, 0.12, 0.2, 1.0]);
        sheet.draw_over(&scale(img), x, top as i32, 1.0);
        rect_outline(&mut sheet, x, top as i32, panel_w as i32, ph as i32, 2, [0.1, 0.12, 0.2, 1.0]);
    }
    fill_rect(&mut sheet, 0, 0, w as i32, 6, [0.14, 0.17, 0.26, 1.0]);
    draw_text(&mut sheet, font, 18.0, &format!("Motivo: {}", retake.reason), gap as f32, (top + ph + 10) as f32, [0.25, 0.27, 0.33, 1.0]);
    let note = format!("máx. {:.2}  média {:.4}", cmp.max_diff, cmp.mean_abs_diff);
    draw_text_centered(&mut sheet, font, 16.0, &note, (w - gap - panel_w / 2) as f32, (top + ph + 12) as f32, [0.42, 0.44, 0.50, 1.0]);
    sheet
}

// ================================================================ proveniência

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceInfo {
    pub id: Id,
    pub name: String,
    pub kind: String,
    pub created_by: String,
    pub modified_by: String,
    pub revision: u64,
}

pub fn provenance(p: &Project, id: &Id) -> ToolResult<ProvenanceInfo> {
    let n = node(p, id)?;
    Ok(ProvenanceInfo {
        id: n.id.clone(),
        name: n.name.clone(),
        kind: n.data.type_name().into(),
        created_by: n.provenance.created_by.clone(),
        modified_by: n.provenance.modified_by.clone(),
        revision: n.provenance.revision,
    })
}

/// Entradas do log do núcleo que tocam o nó ou a subárvore dele (mais recentes por último; no máximo `limit`).
pub fn provenance_history(p: &Project, log: &[LogEntry], id: &Id, limit: usize) -> ToolResult<Vec<LogEntry>> {
    node(p, id)?;
    let ids: Vec<String> = p.subtree(id).into_iter().take(400).map(|i| i.0).collect();
    let mut hits: Vec<LogEntry> = log
        .iter()
        .filter(|e| e.op == "apply")
        .filter(|e| match &e.command {
            Some(c) => serde_json::to_string(c).map(|s| ids.iter().any(|i| s.contains(&format!("\"{i}\"")))).unwrap_or(false),
            None => false,
        })
        .cloned()
        .collect();
    if hits.len() > limit {
        hits.drain(..hits.len() - limit);
    }
    Ok(hits)
}

// ================================================================ relatório

/// Relatório de produção em Markdown: progresso por etapa, tabela de cortes, bloqueios, correções abertas, retakes,
/// regiões não pintadas (lint) e lacunas.
pub fn production_report(p: &Project, blobs: &BlobStore) -> ToolResult<String> {
    let rows = cut_table(p);
    let prod = production_read(p)?;
    let fps = p.settings.fps();
    let total_frames: u32 = rows.iter().map(|r| r.duration).sum();
    let mut md = String::new();
    let _ = writeln!(md, "# Relatório de produção — {}\n", p.name);
    let _ = writeln!(
        md,
        "Revisão do projeto: {} · Cortes: {} · Duração total: {} quadros ({:.1} s a {:.3} fps)\n",
        p.revision,
        rows.len(),
        total_frames,
        total_frames as f64 / fps,
        fps
    );
    let _ = writeln!(md, "## Progresso por etapa\n");
    let _ = writeln!(md, "| Etapa | Não iniciado | Em andamento | Para revisão | Aprovado | Retake | Aprovado % | Revisão+aprovado % |");
    let _ = writeln!(md, "|---|---:|---:|---:|---:|---:|---:|---:|");
    for st in ALL_STAGES {
        let count = |s: StageStatus| rows.iter().filter(|r| r.stages.get(&st) == Some(&s)).count();
        let n = rows.len().max(1) as f32;
        let (ni, ip, rv, ap, rt) = (count(StageStatus::NotStarted), count(StageStatus::InProgress), count(StageStatus::Review), count(StageStatus::Approved), count(StageStatus::Retake));
        let _ = writeln!(md, "| {} | {ni} | {ip} | {rv} | {ap} | {rt} | {:.0}% | {:.0}% |", stage_label(st), ap as f32 / n * 100.0, (ap + rv) as f32 / n * 100.0);
    }
    let _ = writeln!(md, "\n## Tabela de cortes (カット表)\n");
    let mut head = String::from("| Corte | Cena | Quadros |");
    let mut sep = String::from("|---|---|---:|");
    for st in ALL_STAGES {
        let _ = write!(head, " {} |", stage_label(st));
        sep.push_str("---|");
    }
    head.push_str(" Correções abertas | Retakes |");
    sep.push_str("---:|---:|");
    let _ = writeln!(md, "{head}\n{sep}");
    for r in &rows {
        let mut line = format!("| {} ({}) | {} | {} |", r.number, r.name, r.scene_name, r.duration);
        for st in ALL_STAGES {
            let _ = write!(line, " {} |", status_short(r.stages[&st]));
        }
        let _ = write!(line, " {} | {}/{} |", r.open_corrections, r.open_retakes, r.retakes);
        let _ = writeln!(md, "{line}");
    }
    let _ = writeln!(md, "\nLegenda: `-` não iniciado, `and.` em andamento, `rev.` para revisão, `ok` aprovado, `RETAKE` retake. Retakes mostrados como abertos/total.");
    // bloqueados
    let _ = writeln!(md, "\n## Cortes bloqueados\n");
    let mut any = false;
    for r in &rows {
        let retake_stages: Vec<&str> = ALL_STAGES.iter().filter(|s| r.stages[s] == StageStatus::Retake).map(|s| stage_label(*s)).collect();
        if !retake_stages.is_empty() || r.open_corrections > 0 || r.open_retakes > 0 {
            any = true;
            let mut why = Vec::new();
            if !retake_stages.is_empty() {
                why.push(format!("etapa(s) em retake: {}", retake_stages.join(", ")));
            }
            if r.open_corrections > 0 {
                why.push(format!("{} correção(ões) aberta(s)", r.open_corrections));
            }
            if r.open_retakes > 0 {
                why.push(format!("{} retake(s) aberto(s)", r.open_retakes));
            }
            let _ = writeln!(md, "- Corte {} ({}): {}", r.number, r.name, why.join("; "));
        }
    }
    if !any {
        let _ = writeln!(md, "Nenhum corte bloqueado.");
    }
    // correções
    let cut_label = |id: &Id| rows.iter().find(|r| &r.cut == id).map(|r| format!("corte {} ({})", r.number, r.name)).unwrap_or_else(|| id.to_string());
    let open: Vec<&Correction> = prod.corrections.iter().filter(|c| !c.resolved).collect();
    let _ = writeln!(md, "\n## Correções abertas ({} de {})\n", open.len(), prod.corrections.len());
    if open.is_empty() {
        let _ = writeln!(md, "Nenhuma correção aberta.");
    }
    for c in &open {
        let _ = writeln!(md, "- {}, quadro {}: {} (por {})", cut_label(&c.cut), c.frame + 1, c.note, c.author);
    }
    let done: Vec<&Correction> = prod.corrections.iter().filter(|c| c.resolved).collect();
    if !done.is_empty() {
        let _ = writeln!(md, "\nResolvidas:\n");
        for c in &done {
            let _ = writeln!(md, "- {}, quadro {}: {} (resolvida por {}{})", cut_label(&c.cut), c.frame + 1, c.note, c.resolved_by.as_deref().unwrap_or("?"), c.retake.as_ref().map(|r| format!(", retake {r}")).unwrap_or_default());
        }
    }
    // retakes
    let _ = writeln!(md, "\n## Retakes ({})\n", prod.retakes.len());
    if prod.retakes.is_empty() {
        let _ = writeln!(md, "Nenhum retake registrado.");
    }
    for r in &prod.retakes {
        let mut extra = String::new();
        if let Ok(cmp) = retake_compare(p, blobs, &r.id, None) {
            let _ = write!(extra, "; diferença atual {:.2}% dos pixels", cmp.changed_fraction * 100.0);
        }
        let _ = writeln!(md, "- {} quadro {} (rev. {}), {}: {}{}", cut_label(&r.cut), r.frame + 1, r.revision, if r.resolved { "resolvido" } else { "aberto" }, r.reason, extra);
    }
    // lint
    let _ = writeln!(md, "\n## Regiões não pintadas (lint de shiage)\n");
    let mut lint_lines = Vec::new();
    let mut gaps = Vec::new();
    for r in &rows {
        let layers = cut_layers(p, &r.cut);
        let fills: Vec<&(&Node, &LayerData)> = layers.iter().filter(|(_, l)| l.kind == LayerType::Fill).collect();
        if fills.is_empty() {
            continue;
        }
        for (n, l) in fills {
            if l.line_refs.is_empty() {
                gaps.push(format!("Corte {}: camada de preenchimento \"{}\" sem `line_refs` (lint impossível).", r.number, n.name));
                continue;
            }
            match lint_fill_layer(p, blobs, &r.cut, &n.id) {
                Ok(s) => lint_lines.push(format!(
                    "- Corte {} / {}: {} região(ões) sem pintar ({} px²) em {} cel(s) exposto(s); {} pintada(s){}",
                    r.number, s.layer_name, s.unpainted_regions, s.unpainted_area, s.frames_checked, s.painted_regions,
                    if s.border_unpainted_regions > 0 { format!("; {} na borda do quadro sem tinta (conferir no olho)", s.border_unpainted_regions) } else { String::new() }
                )),
                Err(e) => gaps.push(format!("Corte {}: lint falhou em \"{}\": {e}", r.number, n.name)),
            }
        }
    }
    if lint_lines.is_empty() {
        let _ = writeln!(md, "Nenhuma camada de preenchimento para conferir.");
    }
    for l in &lint_lines {
        let _ = writeln!(md, "{l}");
    }
    // lacunas
    for r in &rows {
        let cd = cut_data(p, &r.cut)?;
        let layers = cut_layers(p, &r.cut);
        let real: usize = layers.iter().filter(|(_, l)| !l.kind.is_non_rendering()).count();
        if real == 0 {
            gaps.push(format!("Corte {}: sem camadas de conteúdo (só referência/guia ou vazio).", r.number));
        }
        if cd.dialogue_links.is_empty() && cd.storyboard_panel.is_some() {
            gaps.push(format!("Corte {}: nasceu de um painel mas não tem falas vinculadas.", r.number));
        }
        if cd.xsheet.tracks.is_empty() && real > 0 {
            gaps.push(format!("Corte {}: sem trilhas no timesheet.", r.number));
        }
    }
    let _ = writeln!(md, "\n## Lacunas detectadas\n");
    if gaps.is_empty() {
        let _ = writeln!(md, "Nenhuma lacuna detectada.");
    }
    for g in &gaps {
        let _ = writeln!(md, "- {g}");
    }
    Ok(md)
}
