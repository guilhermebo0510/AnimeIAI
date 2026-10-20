//! Produção e revisão (S19) sobre o documento: `PRODUCTION` (tabela de cortes, etapas, sugestão automática, lint de todos
//! os quadros, proveniência, relatório), `CORRECTION` (camada vermelha + nota) e `RETAKE` (snapshot, comparação, folha
//! ANTES | DEPOIS | DIFERENÇA).
//!
//! Mesmo padrão de [`crate::preprod_tools`]: operação por `op`; as mutações passam pela `CoreSession` (undo, log,
//! proveniência). Caminhos de gravação (`out`) devem ser absolutos e sem `..`.

use crate::core_tools::{apply, request, tool_error};
use crate::protocol::BridgeError;
use aiai_core::{CoreSession, Id, LayerType, NodeKind, Stage, StageStatus};
use aiai_tools::draw::CurveSpec;
use aiai_tools::production::{self, CorrectionRequest, RetakeRequest};
use aiai_tools::{production_history as history, production_stats as stats};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const REVIEW_ACTIONS: &[&str] = &["PRODUCTION", "CORRECTION", "RETAKE"];

pub fn is_mutating(action: &str) -> bool {
    REVIEW_ACTIONS.contains(&action)
}

/// `PRODUCTION`: estado da produção.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ProductionRequest {
    /// カット表: cortes na ordem do documento com duração, as 8 etapas, correções e retakes abertos.
    CutTable,
    /// Muda o estado de uma etapa do corte.
    SetStage { cut: Id, stage: Stage, status: StageStatus },
    /// Sugere avanços de etapa pelo conteúdo do corte (nunca mexe em aprovadas/retake); `apply` grava num passo de undo.
    SuggestStages {
        cut: Id,
        #[serde(default)]
        apply: bool,
    },
    /// Lint de TODAS as camadas de preenchimento do corte em todos os quadros expostos (regiões sem cor).
    LintCut { cut: Id },
    /// Quem criou/mudou o nó; com `history` (> 0), as últimas entradas do log que tocam o nó ou a subárvore.
    Provenance {
        id: Id,
        #[serde(default)]
        history: usize,
    },
    /// Relatório de produção em Markdown (com `out`, grava o arquivo).
    Report {
        #[serde(default)]
        out: Option<String>,
    },
    /// Muda a mesma etapa de vários cortes num passo de undo.
    SetStages { cuts: Vec<Id>, stage: Stage, status: StageStatus },
    /// Prazo (`AAAA-MM-DD`), responsável e peso de vários cortes num passo de undo; texto vazio (ou peso <= 0) limpa o campo.
    SetMeta {
        cuts: Vec<Id>,
        #[serde(default)]
        due: Option<String>,
        #[serde(default)]
        assignee: Option<String>,
        #[serde(default)]
        weight: Option<f32>,
    },
    /// Histórico de status (índice derivado do log, guardado em `producao-historico.ndjson` na pasta do projeto): os
    /// eventos mais novos primeiro (`limit` padrão 100), de um corte com `cut`.
    History {
        #[serde(default)]
        cut: Option<Id>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Estatísticas da gestão (contagens por etapa/estado, matriz cena × etapa, fluxo cumulativo, burndown, vazão, tempos,
    /// taxa de retake, previsão) e os fluxos por etapa; `stuck_days` (padrão 3) define "travado".
    Stats {
        #[serde(default)]
        stuck_days: Option<f64>,
    },
    /// Análise determinística de gargalos: fila, vazão e espera por etapa e os achados (gargalo, ociosa, travados, atrasados, retakes).
    Bottlenecks {
        #[serde(default)]
        stuck_days: Option<f64>,
    },
    /// Pacote de dados para um agente analisar (estatísticas, achados, últimos eventos, notas, roteiro da análise). Com
    /// `out` (absoluto) grava o JSON. Nunca muda o projeto: devolva a análise com `note_add` (`ai: true`).
    AnalysisPackage {
        #[serde(default)]
        out: Option<String>,
        #[serde(default)]
        stuck_days: Option<f64>,
    },
    /// Os números dos gráficos em CSV (com `out` grava o arquivo).
    StatsCsv {
        #[serde(default)]
        out: Option<String>,
    },
    /// Anexa uma nota de gestão com carimbo de hora (livre ou de um corte). `ai: true` marca "gerada por IA".
    NoteAdd {
        text: String,
        #[serde(default)]
        cut: Option<Id>,
        #[serde(default)]
        ai: bool,
    },
    NoteList,
    NoteDelete { note: Id },
}

/// `CORRECTION`: correção desenhada em vermelho numa camada de guia do corte (não renderiza) + nota no documento "Produção".
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CorrectionOp {
    Add {
        cut: Id,
        /// Quadro 0-based.
        frame: u32,
        note: String,
        #[serde(default)]
        strokes: Vec<CurveSpec>,
        #[serde(default)]
        author: Option<String>,
    },
    /// Marca resolvida (ou reabre com `resolved: false`); `retake` liga ao retake que a resolveu.
    Resolve {
        correction: Id,
        #[serde(default = "yes")]
        resolved: bool,
        #[serde(default)]
        by: Option<String>,
        #[serde(default)]
        retake: Option<Id>,
    },
    List { cut: Id },
}

/// `RETAKE`: guarda o render atual do quadro como snapshot e compara depois da refeitura.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RetakeOp {
    Create {
        cut: Id,
        frame: u32,
        reason: String,
        #[serde(default)]
        author: Option<String>,
    },
    Resolve {
        retake: Id,
        #[serde(default = "yes")]
        resolved: bool,
    },
    /// Compara o snapshot com o render atual (`frame` omitido = o do retake); com `out` grava a folha
    /// ANTES | DEPOIS | DIFERENÇA (`panel_w` px por painel, padrão 480).
    Compare {
        retake: Id,
        #[serde(default)]
        frame: Option<u32>,
        #[serde(default)]
        out: Option<String>,
        #[serde(default)]
        panel_w: Option<u32>,
    },
    List,
}

fn yes() -> bool {
    true
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "PRODUCTION" => production_action(session, params),
        "CORRECTION" => correction(session, params),
        "RETAKE" => retake(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

fn production_action(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let p = &session.project;
    match request::<ProductionRequest>(params, "PRODUCTION")? {
        ProductionRequest::CutTable => {
            let rows = production::cut_table(p);
            let total: u32 = rows.iter().map(|r| r.duration).sum();
            Ok(json!({ "cuts": rows, "total_frames": total, "total_seconds": total as f64 / p.settings.fps() }))
        }
        ProductionRequest::SetStage { cut, stage, status } => {
            let cmd = production::set_stage(p, &cut, stage, status).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        ProductionRequest::SuggestStages { cut, apply: do_apply } => {
            let sug = production::suggest_stages(p, &session.blobs, &cut).map_err(tool_error)?;
            let list: Vec<Value> = sug.iter().map(|(s, st, why)| json!({ "stage": s, "status": st, "reason": why })).collect();
            if do_apply {
                if let Some(cmd) = production::apply_stage_suggestions(p, &cut, &sug).map_err(tool_error)? {
                    let mut v = apply(session, params, cmd)?;
                    v["suggestions"] = json!(list);
                    return Ok(v);
                }
            }
            Ok(json!({ "suggestions": list, "applied": false }))
        }
        ProductionRequest::LintCut { cut } => {
            let node = p.node(&cut).ok_or_else(|| BridgeError::invalid_params(format!("não encontrado: corte {cut}")))?;
            let fills: Vec<Id> = node.children.iter().filter(|c| matches!(p.node(c).map(|n| &n.data), Some(NodeKind::Layer(l)) if l.kind == LayerType::Fill)).cloned().collect();
            let mut layers = Vec::new();
            let (mut regions, mut area, mut border) = (0usize, 0usize, 0usize);
            for l in &fills {
                let s = production::lint_fill_layer(p, &session.blobs, &cut, l).map_err(tool_error)?;
                regions += s.unpainted_regions;
                area += s.unpainted_area;
                border += s.border_unpainted_regions;
                layers.push(s);
            }
            Ok(json!({ "cut": cut, "fill_layers": layers, "unpainted_regions": regions, "unpainted_area": area, "border_unpainted_regions": border, "clean": regions == 0 }))
        }
        ProductionRequest::Provenance { id, history } => {
            let info = production::provenance(p, &id).map_err(tool_error)?;
            let mut v = json!({ "provenance": info });
            if history > 0 {
                let h = production::provenance_history(p, session.log(), &id, history.min(200)).map_err(tool_error)?;
                v["history"] = serde_json::to_value(h).map_err(|e| BridgeError::failed(e.to_string()))?;
            }
            Ok(v)
        }
        ProductionRequest::Report { out } => {
            let md = production::production_report(p, &session.blobs).map_err(tool_error)?;
            match out {
                Some(o) => {
                    let path = check_abs(&o)?;
                    ensure_parent(&path)?;
                    std::fs::write(&path, md.as_bytes()).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", path.display())))?;
                    Ok(json!({ "path": path.display().to_string(), "bytes": md.len() }))
                }
                None => Ok(json!({ "markdown": md })),
            }
        }
        ProductionRequest::SetStages { cuts, stage, status } => match production::set_stage_many(p, &cuts, stage, status).map_err(tool_error)? {
            Some(cmd) => apply(session, params, cmd),
            None => Ok(json!({ "changed": 0, "note": "todos os cortes já estavam nesse estado" })),
        },
        ProductionRequest::SetMeta { cuts, due, assignee, weight } => {
            let cmd = production::set_cut_meta(p, &cuts, &production::CutMetaPatch { due, assignee, weight }).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        ProductionRequest::History { cut, limit } => {
            let (idx, warn) = history::index_for_session(session);
            let events: Vec<&history::StatusEvent> = idx.events.iter().rev().filter(|e| cut.as_ref().is_none_or(|c| &e.cut == c)).take(limit.unwrap_or(100).min(2000)).collect();
            let mut v = json!({ "total": idx.events.len(), "last_revision": idx.last_rev, "events": events });
            if let Some(w) = warn {
                v["warning"] = json!(w);
            }
            Ok(v)
        }
        ProductionRequest::Stats { stuck_days } => {
            let (a, warn) = analysis(session, stuck_days);
            let mut v = serde_json::to_value(&a).map_err(|e| BridgeError::failed(e.to_string()))?;
            if let Some(w) = warn {
                v["warning"] = json!(w);
            }
            Ok(v)
        }
        ProductionRequest::Bottlenecks { stuck_days } => {
            let (a, warn) = analysis(session, stuck_days);
            let mut v = json!({
                "flows": a.flows,
                "findings": a.findings.iter().map(|f| json!({ "finding": f, "text": f.text_pt() })).collect::<Vec<_>>(),
            });
            if let Some(w) = warn {
                v["warning"] = json!(w);
            }
            Ok(v)
        }
        ProductionRequest::AnalysisPackage { out, stuck_days } => {
            let (idx, warn) = history::index_for_session(session);
            let rows = production::cut_table(p);
            let doc = production::production_read(p).map_err(tool_error)?;
            let mut cfg = stats::StatsConfig::new(session.clock.now_ms());
            if let Some(d) = stuck_days {
                cfg.stuck_days = d.max(0.0);
            }
            let pkg = stats::analysis_package(&p.name, &rows, &idx.events, &doc.notes, &cfg);
            let mut v = json!({ "package": pkg });
            if let Some(o) = out {
                let path = check_abs(&o)?;
                ensure_parent(&path)?;
                let text = serde_json::to_string_pretty(&v["package"]).map_err(|e| BridgeError::failed(e.to_string()))?;
                std::fs::write(&path, text.as_bytes()).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", path.display())))?;
                v["path"] = json!(path.display().to_string());
            }
            if let Some(w) = warn {
                v["warning"] = json!(w);
            }
            Ok(v)
        }
        ProductionRequest::StatsCsv { out } => {
            let (a, _) = analysis(session, None);
            let csv = stats::stats_csv(&a);
            match out {
                Some(o) => {
                    let path = check_abs(&o)?;
                    ensure_parent(&path)?;
                    std::fs::write(&path, csv.as_bytes()).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", path.display())))?;
                    Ok(json!({ "path": path.display().to_string(), "bytes": csv.len() }))
                }
                None => Ok(json!({ "csv": csv })),
            }
        }
        ProductionRequest::NoteAdd { text, cut, ai } => {
            let author = params.get("author").and_then(Value::as_str).unwrap_or(crate::core_ops::DEFAULT_AUTHOR).to_owned();
            let now = session.clock.now_ms();
            let (id, cmd) = production::note_add(&session.project, &mut session.ids, now, &author, &text, cut, ai).map_err(tool_error)?;
            let mut v = apply(session, params, cmd)?;
            v["note"] = json!(id);
            v["t_ms"] = json!(now);
            Ok(v)
        }
        ProductionRequest::NoteList => {
            let doc = production::production_read(p).map_err(tool_error)?;
            Ok(json!({ "notes": doc.notes }))
        }
        ProductionRequest::NoteDelete { note } => {
            let cmd = production::note_delete(&session.project, &mut session.ids, &note).map_err(tool_error)?;
            apply(session, params, cmd)
        }
    }
}

/// Análise (estatísticas + fluxos + achados) com o histórico da sessão; o aviso é o do índice (arquivo ruim/falha ao gravar).
fn analysis(session: &CoreSession, stuck_days: Option<f64>) -> (stats::Analysis, Option<String>) {
    let (idx, warn) = history::index_for_session(session);
    let rows = production::cut_table(&session.project);
    let mut cfg = stats::StatsConfig::new(session.clock.now_ms());
    if let Some(d) = stuck_days {
        cfg.stuck_days = d.max(0.0);
    }
    (stats::analyze(&rows, &idx.events, &cfg), warn)
}

fn correction(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    match request::<CorrectionOp>(params, "CORRECTION")? {
        CorrectionOp::Add { cut, frame, note, strokes, author } => {
            let req = CorrectionRequest { cut, frame, note, strokes, author };
            let a = production::correction_add(&session.project, &mut session.ids, &req).map_err(tool_error)?;
            let mut v = apply(session, params, a.command)?;
            v["correction"] = json!(a.id);
            Ok(v)
        }
        CorrectionOp::Resolve { correction, resolved, by, retake } => {
            let by = by.unwrap_or_else(|| "agent".into());
            let cmd = production::correction_resolve(&session.project, &correction, resolved, &by, retake).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        CorrectionOp::List { cut } => {
            let list = production::corrections_for_cut(&session.project, &cut).map_err(tool_error)?;
            Ok(json!({ "cut": cut, "corrections": list }))
        }
    }
}

fn retake(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    match request::<RetakeOp>(params, "RETAKE")? {
        RetakeOp::Create { cut, frame, reason, author } => {
            let req = RetakeRequest { cut, frame, reason, author };
            let c = production::retake_create(&session.project, &mut session.ids, &mut session.blobs, &req).map_err(tool_error)?;
            let mut v = apply(session, params, c.command)?;
            v["retake"] = json!(c.id);
            Ok(v)
        }
        RetakeOp::Resolve { retake, resolved } => {
            let cmd = production::retake_resolve(&session.project, &retake, resolved).map_err(tool_error)?;
            apply(session, params, cmd)
        }
        RetakeOp::Compare { retake, frame, out, panel_w } => {
            let p = &session.project;
            let cmp = production::retake_compare(p, &session.blobs, &retake, frame).map_err(tool_error)?;
            let mut v = json!({
                "retake": retake, "changed_pixels": cmp.changed_pixels, "changed_fraction": cmp.changed_fraction,
                "mean_abs_diff": cmp.mean_abs_diff, "max_diff": cmp.max_diff,
            });
            if let Some(o) = out {
                let path = check_abs(&o)?;
                let prod = production::production_read(p).map_err(tool_error)?;
                let r = prod.retakes.iter().find(|r| r.id == retake).ok_or_else(|| BridgeError::invalid_params(format!("não encontrado: retake {retake}")))?;
                let sheet = production::retake_compare_sheet(&cmp, r, panel_w.unwrap_or(480).clamp(120, 1200));
                ensure_parent(&path)?;
                sheet.save_png(&path).map_err(|e| BridgeError::failed(format!("gravar {}: {e}", path.display())))?;
                v["path"] = json!(path.display().to_string());
            }
            Ok(v)
        }
        RetakeOp::List => {
            let prod = production::production_read(&session.project).map_err(tool_error)?;
            Ok(json!({ "retakes": prod.retakes }))
        }
    }
}

fn check_abs(path: &str) -> Result<PathBuf, BridgeError> {
    let p = Path::new(path);
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(BridgeError::invalid_params("'out' deve ser um caminho absoluto sem '..'"));
    }
    Ok(p.to_path_buf())
}

fn ensure_parent(p: &Path) -> Result<(), BridgeError> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| BridgeError::failed(format!("criar {}: {e}", dir.display())))?;
    }
    Ok(())
}
