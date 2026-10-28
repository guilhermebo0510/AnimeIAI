//! Prévia de intercalação de um quadro pela bridge (R16 §8): `INBETWEEN_PREVIEW`, `INBETWEEN_ACCEPT`,
//! `INBETWEEN_DISCARD` e o render com a prévia por cima (`RENDER with_preview`). A proposta fica presa à sessão que a
//! calculou (app ou headless): aceitar, descartar e renderizar usam a mesma sessão, sem cair em outra.

use crate::core_ops::{DEFAULT_AUTHOR, obj, opt_str, opt_u64};
use crate::core_tools::{request, tool_error};
use crate::protocol::{BridgeError, code};
use crate::params_check::check_keys;
use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::inbetween_preview::{self as ibp, InbetweenPreviewRequest, InbetweenProposal};
use serde_json::{Value, json};

pub const ACTIONS: &[&str] = &["INBETWEEN_PREVIEW", "INBETWEEN_ACCEPT", "INBETWEEN_DISCARD"];

/// Cor da prévia (diferente das cores da mesa de luz).
pub const PREVIEW_RGB: [f32; 3] = [0.88, 0.25, 0.98];

/// Resumo da proposta para o MCP: pares por `Stroke.id`, avisos, recusas e contagens (o candidato fica na sessão).
pub fn proposal_json(p: &Project, prop: &InbetweenProposal) -> Value {
    let scope = ibp::accept_scope(p, prop).ok();
    let layer_name = p.node(&prop.layer).map(|n| n.name.clone()).unwrap_or_default();
    // a mesma regra da interface e do aceite (inclui camada ou pasta travada)
    let blocked = ibp::blocked_reason(p, prop);
    json!({
        "proposal_id": prop.id,
        "generation": prop.generation,
        "revision": prop.revision,
        "cut": prop.cut,
        "layer": prop.layer,
        "frame": prop.frame,
        "frame_a": prop.frame_a,
        "frame_b": prop.frame_b,
        "key_a": prop.key_a,
        "key_b": prop.key_b,
        "t": prop.t,
        "stale": prop.stale,
        "acceptable": blocked.is_none(),
        "blocked_reason": blocked,
        "accept_label": scope.as_ref().map(|s| s.button_text(&layer_name)),
        "options": { "motion": prop.options.motion, "bend": prop.options.bend, "max_cost": prop.options.max_cost },
        "manual_pairs": prop.manual_pairs,
        "pairs": prop.pairs,
        "approximations": prop.approximations,
        "refusals": prop.refusals,
        "notes": prop.notes,
        "counts": {
            "strokes_a": prop.strokes_a.len(),
            "strokes_b": prop.strokes_b.len(),
            "pairs": prop.pairs.len(),
            "candidate_strokes": prop.candidate.len(),
            "candidate_points": prop.candidate.iter().map(|s| s.points.len()).sum::<usize>(),
            "approximations": prop.approximations.len(),
            "refusals": prop.refusals.len(),
        },
    })
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "INBETWEEN_PREVIEW" => {
            if params.get("pairs").is_some() || params.get("options").is_some() {
                return Err(BridgeError::invalid_params("INBETWEEN_PREVIEW: pares por índice ('pairs'/'options') não são aceitos; use 'manual_pairs' com os ids dos traços [[id_em_A, id_em_B], …]"));
            }
            let r: InbetweenPreviewRequest = request(params, action)?;
            let t0 = std::time::Instant::now();
            let prop = ibp::preview(session, &r).map_err(tool_error)?;
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let mut v = proposal_json(&session.project, &prop);
            // E1: tempo do cálculo síncrono (prévia inteira, dentro da sessão)
            v["compute_ms"] = json!(ms);
            Ok(v)
        }
        "INBETWEEN_ACCEPT" => {
            check_keys(params, action, &["proposal_id", "author"])?;
            let p = obj(params)?;
            let id = opt_u64(&p, "proposal_id")?.ok_or_else(|| BridgeError::invalid_params("INBETWEEN_ACCEPT exige 'proposal_id'"))?;
            let author = opt_str(&p, "author")?.unwrap_or(DEFAULT_AUTHOR).to_string();
            let acc = ibp::accept(session, id, &author).map_err(tool_error)?;
            Ok(json!({
                "revision": acc.outcome.revision,
                "description": acc.outcome.description,
                "created": acc.outcome.created,
                "affected": acc.outcome.affected,
                "cel": acc.cel,
                "number": acc.number,
                "frames": acc.frames,
            }))
        }
        "INBETWEEN_DISCARD" => {
            check_keys(params, action, &["proposal_id"])?;
            let p = obj(params)?;
            let id = opt_u64(&p, "proposal_id")?.ok_or_else(|| BridgeError::invalid_params("INBETWEEN_DISCARD exige 'proposal_id'"))?;
            ibp::discard(session, id).map_err(tool_error)?;
            Ok(json!({ "ok": true, "proposal_id": id }))
        }
        other => Err(BridgeError::unknown_action(other)),
    }
}

/// Proposta viva com este id para o render (sem recalcular; inválida = erro explícito).
pub fn proposal_for_render(session: &mut CoreSession, id: u64) -> Result<InbetweenProposal, BridgeError> {
    if let Some(why) = ibp::invalidate_if_changed(session) {
        return Err(BridgeError::new(code::STALE_REVISION, format!("a prévia {id} não vale mais: {why}")));
    }
    match ibp::current(session) {
        Some(p) if p.id == id => Ok(p.clone()),
        Some(p) => Err(BridgeError::new(code::STALE_REVISION, format!("a prévia {id} não é a atual ({})", p.id))),
        None => Err(BridgeError::invalid_params(format!("prévia {id} não encontrada nesta sessão (descartada, aceita ou a sessão foi reaberta)"))),
    }
}

/// Quadro com a prévia por cima (R16 REQ-M03): o desenho do quadro como está, mais o candidato na cor da prévia, no
/// lugar exato da camada (movimento da camada no quadro, transformação identidade do candidato). Transitório: nada é
/// gravado (cel, WAL, revisão).
pub fn render_with_preview(project: &Project, blobs: &BlobStore, prop: &InbetweenProposal, opts: &RenderOptions) -> Result<(aiai_paint::Image, Vec<String>), BridgeError> {
    let base = render_frame(project, blobs, &prop.cut, prop.frame, opts).map_err(|e| BridgeError::invalid_params(e.to_string()))?;
    let mut warnings = base.warnings;
    let mut image = base.image;
    // cópia de trabalho: um cel novo só com o candidato, na cor da prévia, exposto no quadro da proposta
    let mut tmp = project.clone();
    let cel_id = Id(format!("__inbetween_preview_{}", prop.id));
    let number = u32::MAX - 1;
    let mut cel = CelData::new(number);
    cel.role = CelRole::Inbetween;
    cel.strokes = prop
        .candidate
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.color = ColorRef::Direct { rgba: Rgba([PREVIEW_RGB[0], PREVIEW_RGB[1], PREVIEW_RGB[2], 1.0]) };
            s.opacity = 1.0;
            s
        })
        .collect();
    let node = Node { id: cel_id.clone(), name: "prévia".into(), parent: Some(prop.layer.clone()), children: Vec::new(), data: NodeKind::Cel(cel), provenance: Default::default() };
    tmp.nodes.insert(cel_id.clone(), node);
    if let Some(l) = tmp.nodes.get_mut(&prop.layer) {
        l.children.push(cel_id);
        if let NodeKind::Layer(ld) = &mut l.data {
            ld.visible = true;
            ld.opacity = 1.0;
        }
    }
    if let Some(NodeKind::Cut(c)) = tmp.nodes.get_mut(&prop.cut).map(|n| &mut n.data) {
        if let Some(t) = c.xsheet.tracks.iter_mut().find(|t| t.layer.as_ref() == Some(&prop.layer)) {
            t.cells.insert(prop.frame, CellRef::Cel { number });
        }
    }
    let mut only = opts.clone();
    only.only_layers = Some(vec![prop.layer.clone()]);
    only.background = None;
    let over = render_frame(&tmp, blobs, &prop.cut, prop.frame, &only).map_err(|e| BridgeError::failed(format!("prévia: {e}")))?;
    warnings.extend(over.warnings);
    if (over.image.width, over.image.height) != (image.width, image.height) {
        return Err(BridgeError::failed("prévia: tamanhos diferentes entre o quadro e a sobreposição"));
    }
    image.draw_over(&over.image, 0, 0, 1.0);
    Ok((image, warnings))
}
