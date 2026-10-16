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
            let (mut regions, mut area) = (0usize, 0usize);
            for l in &fills {
                let s = production::lint_fill_layer(p, &session.blobs, &cut, l).map_err(tool_error)?;
                regions += s.unpainted_regions;
                area += s.unpainted_area;
                layers.push(s);
            }
            Ok(json!({ "cut": cut, "fill_layers": layers, "unpainted_regions": regions, "unpainted_area": area, "clean": regions == 0 }))
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
    }
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
