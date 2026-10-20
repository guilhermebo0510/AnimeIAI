//! Entrega (S24 4.8–4.12): `DELIVERY` junta a fila de exportação, os destinos, os entregáveis, a verificação final e o
//! pacote de entrega. A lógica pura (fila, nomes, verificação, zip) está em `aiai_tools::delivery`; o que exporta de fato
//! é o `EXPORT_VIDEO` ([`run_job`] cuida de cada tipo de saída, da miniatura, das cópias e do pacote).
//!
//! O estado mora em `entrega.json` ao lado do projeto. Com o app aberto o app é dono da fila (a mesma que o painel
//! mostra, com progresso por quadro): ele atende as operações com o próprio estado por [`handle_with_store`] e roda os
//! itens numa thread. Sem o app (headless), `queue_run` roda os itens da fila em sequência dentro da chamada.
//! Não muda o documento (nada de undo): só o `entrega.json`, as pastas de saída e as miniaturas.

use crate::core_tools::{request, tool_error};
use crate::export_ctl::{self, ExportControl, Phase};
use crate::protocol::BridgeError;
use aiai_core::{CoreSession, Id};
use aiai_tools::audio::{self, WavBits};
use aiai_tools::delivery::formats::{self, FormatPreset, Kind};
use aiai_tools::delivery::package::{self, PackagePlan};
use aiai_tools::delivery::preflight::{self, Level};
use aiai_tools::delivery::store::{Deliverable, JobRecord, JobResult, JobSpec, PosterInfo, Settings, Status, Store, fmt_size, matrix};
use aiai_tools::delivery::{SourceKind, SourceRef, mix, poster};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DELIVERY_ACTIONS: &[&str] = &["DELIVERY"];

/// `DELIVERY` não muda o documento (o headless não salva o projeto depois dela).
pub fn is_mutating(_action: &str) -> bool {
    false
}

pub fn handle(session: &mut CoreSession, action: &str, params: &Value) -> Result<Value, BridgeError> {
    match action {
        "DELIVERY" => delivery_headless(session, params),
        _ => Err(BridgeError::unknown_action(action)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOp {
    Formats,
    Settings,
    Preflight,
    QueueAdd,
    QueueList,
    QueueRemove,
    QueueMove,
    QueueCancel,
    QueueRetry,
    QueueClear,
    QueuePause,
    QueueResume,
    QueueRun,
    List,
    Delete,
    Package,
    Poster,
    Reexport,
}

impl DeliveryOp {
    /// Operações que mexem na fila viva (o app as atende com a fila da interface).
    pub fn is_queue(self) -> bool {
        matches!(self, DeliveryOp::QueueAdd | DeliveryOp::QueueList | DeliveryOp::QueueRemove | DeliveryOp::QueueMove | DeliveryOp::QueueCancel | DeliveryOp::QueueRetry | DeliveryOp::QueueClear | DeliveryOp::QueuePause | DeliveryOp::QueueResume | DeliveryOp::QueueRun | DeliveryOp::Reexport)
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DeliveryRequest {
    pub op: Option<DeliveryOp>,
    /// Fonte pelo atalho: um corte (com `start`/`end` opcionais), um episódio ou uma edição.
    #[serde(default)]
    pub cut: Option<Id>,
    #[serde(default)]
    pub episode: Option<Id>,
    #[serde(default)]
    pub edit: Option<Id>,
    #[serde(default)]
    pub start: Option<u32>,
    #[serde(default)]
    pub end: Option<u32>,
    /// Ou a fonte completa {kind: cut|episode|edit, id, start?, end?}.
    #[serde(default)]
    pub source: Option<SourceRef>,
    /// Matriz: várias fontes × vários formatos (`presets`).
    #[serde(default)]
    pub sources: Vec<SourceRef>,
    /// Formato (id de `formats`): tv, web, youtube, master, nvenc, prores, h265, webm, preview, png, wav, stems, contact.
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub presets: Vec<String>,
    /// Qualidade (CRF) no lugar da do formato.
    #[serde(default)]
    pub crf: Option<u32>,
    /// `preflight`: mede o loudness (padrão true; mixa a fonte inteira).
    #[serde(default)]
    pub loudness: Option<bool>,
    /// Item da fila ou entregável.
    #[serde(default)]
    pub id: Option<u64>,
    /// `queue_move`: posição na lista.
    #[serde(default)]
    pub to: Option<usize>,
    /// `settings`: campos a mudar (dest, custom_dir, name_template, auto_version, extra_dirs, package).
    #[serde(default)]
    pub settings: Option<Value>,
    /// `queue_add`/`reexport`: começa a exportar já (no app, sem esperar; no headless, a chamada espera os itens acabarem).
    #[serde(default)]
    pub run: bool,
    /// `queue_add`: quadro do pôster/miniatura (padrão o do meio).
    #[serde(default)]
    pub poster_frame: Option<u32>,
    /// `poster`: quadro (da fonte do entregável), título e legenda sobre a imagem.
    #[serde(default)]
    pub frame: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    /// `queue_run` headless: no máximo tantos itens.
    #[serde(default)]
    pub limit: Option<usize>,
    /// `list`/`queue_list`: quantos itens (os mais recentes primeiro nos entregáveis).
    #[serde(default)]
    pub count: Option<usize>,
    /// Preenchido pelo servidor MCP com as pastas onde ele pode gravar (o valor que o agente mandar é substituído): toda
    /// pasta de saída, de cópia e de pacote precisa estar dentro de uma delas. Vazio = sem confinamento (o app, pelo usuário).
    #[serde(default)]
    pub allowed_dirs: Vec<String>,
}

fn canon(p: &Path) -> PathBuf {
    let mut tail = Vec::new();
    let mut cur = p.to_path_buf();
    while !cur.exists() {
        match cur.file_name().map(|n| n.to_owned()) {
            Some(n) => {
                tail.push(n);
                cur.pop();
            }
            None => break,
        }
    }
    let mut base = std::fs::canonicalize(&cur).unwrap_or(cur);
    for n in tail.iter().rev() {
        base.push(n);
    }
    PathBuf::from(base.to_string_lossy().to_lowercase())
}

impl DeliveryRequest {
    /// `Err` se há confinamento e `p` está fora das pastas permitidas (comparação pela forma canônica do ancestral existente).
    pub fn confine(&self, p: &Path) -> Result<(), BridgeError> {
        if self.allowed_dirs.is_empty() || self.allowed_dirs.iter().any(|d| canon(p).starts_with(canon(Path::new(d)))) {
            return Ok(());
        }
        Err(BridgeError::invalid_params(format!("{} fica fora das pastas permitidas para gravar {:?}", p.display(), self.allowed_dirs)))
    }

    fn confine_spec(&self, spec: &JobSpec) -> Result<(), BridgeError> {
        self.confine(Path::new(&spec.out))?;
        spec.extra_dirs.iter().try_for_each(|d| self.confine(Path::new(d)))
    }
}

/// O que uma operação fez: a resposta, se o estado mudou (grava `entrega.json`) e se pede para rodar a fila.
pub struct Outcome {
    pub value: Value,
    pub changed: bool,
    pub run: bool,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Pasta do projeto: a Entrega grava ao lado dele.
pub fn project_dir(session: &CoreSession) -> Result<PathBuf, BridgeError> {
    session.path.clone().ok_or_else(|| BridgeError::failed("o projeto ainda não tem pasta: salve antes (a Entrega grava o estado e as saídas ao lado dele)"))
}

fn source_of(r: &DeliveryRequest) -> Result<SourceRef, BridgeError> {
    if let Some(s) = &r.source {
        return Ok(s.clone());
    }
    let n = [r.cut.is_some(), r.episode.is_some(), r.edit.is_some()].iter().filter(|b| **b).count();
    if n != 1 {
        return Err(BridgeError::invalid_params("informe a fonte: `source`, ou só um entre `cut`, `episode` e `edit`"));
    }
    let mut s = match (&r.cut, &r.episode, &r.edit) {
        (Some(c), _, _) => SourceRef::cut(c.clone()),
        (_, Some(e), _) => SourceRef::episode(e.clone()),
        (_, _, Some(e)) => SourceRef::edit(e.clone()),
        _ => unreachable!(),
    };
    if s.kind == SourceKind::Cut {
        s.start = r.start;
        s.end = r.end;
    } else if r.start.is_some() || r.end.is_some() {
        return Err(BridgeError::invalid_params("'start'/'end' só valem com 'cut'"));
    }
    Ok(s)
}

fn preset_of(id: &str) -> Result<&'static FormatPreset, BridgeError> {
    formats::preset(id).ok_or_else(|| BridgeError::invalid_params(format!("formato desconhecido: {id}; use um de {}", formats::PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", "))))
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<Value, BridgeError> {
    serde_json::to_value(v).map_err(|e| BridgeError::failed(e.to_string()))
}

fn status_str(s: Status) -> &'static str {
    match s {
        Status::Queued => "queued",
        Status::Running => "running",
        Status::Done => "done",
        Status::Failed => "failed",
        Status::Canceled => "canceled",
    }
}

pub fn job_json(j: &JobRecord) -> Value {
    json!({
        "id": j.spec.id, "status": status_str(j.status), "source": { "kind": j.spec.source.kind, "id": j.spec.source.id, "start": j.spec.source.start, "end": j.spec.source.end },
        "source_name": j.spec.source_name, "preset": j.spec.preset, "out": j.spec.out, "crf": j.spec.crf, "attempts": j.attempts, "secs": j.secs,
        "error": j.error, "resumed": j.resumed, "package": j.spec.package, "extra_dirs": j.spec.extra_dirs,
    })
}

pub fn deliverable_json(d: &Deliverable, project_dir: &Path) -> Value {
    json!({
        "id": d.id(), "source_name": d.spec.source_name, "source": d.spec.source, "preset": d.spec.preset, "files": d.result.files, "is_folder": d.result.is_sequence,
        "frames": d.result.frames, "width": d.result.width, "height": d.result.height, "fps": d.result.fps, "duration_s": d.result.duration_s,
        "size_bytes": d.result.size_bytes, "size": fmt_size(d.result.size_bytes), "lufs": d.result.lufs, "true_peak_dbtp": d.result.true_peak,
        "thumbnail": d.result.thumbnail.as_ref().map(|t| Store::thumbs_dir(project_dir).join(t).display().to_string()),
        "copies": d.result.copies, "package": d.result.package, "poster": d.poster, "warnings": d.result.warnings,
        "missing": d.missing, "finished_ms": d.finished_ms, "source_revision": d.spec.revision,
    })
}

fn formats_json() -> Value {
    let list: Vec<Value> = formats::PRESETS
        .iter()
        .map(|p| {
            let av = p.availability();
            json!({
                "id": p.id, "kind": format!("{:?}", p.kind).to_lowercase(), "codec": p.codec, "ext": p.ext, "crf": p.crf, "loudness": p.loudness, "loudness_target_lufs": p.loudness_target(),
                "audio": p.audio, "sidecar_srt": p.sidecar_srt, "height": p.height, "speed": p.speed, "available": av.is_ok(), "reason": av.err(),
            })
        })
        .collect();
    json!({ "formats": list, "name_tokens": aiai_tools::delivery::names::TOKENS, "default_template": aiai_tools::delivery::names::DEFAULT_TEMPLATE })
}

/// Aplica um merge de campos em `settings` (campo desconhecido é erro) e valida.
fn merge_settings(cur: &Settings, patch: &Value) -> Result<Settings, BridgeError> {
    let Some(o) = patch.as_object() else { return Err(BridgeError::invalid_params("'settings' deve ser um objeto")) };
    let mut v = to_json(cur)?;
    for (k, x) in o {
        v[k] = x.clone();
    }
    let s: Settings = serde_json::from_value(v).map_err(|e| BridgeError::invalid_params(format!("settings inválido: {e}")))?;
    s.validate().map_err(BridgeError::invalid_params)?;
    Ok(s)
}

fn settings_json(s: &Settings, session: &CoreSession) -> Result<Value, BridgeError> {
    let mut v = to_json(s)?;
    if let Some(dir) = &session.path {
        v["out_dir"] = json!(s.out_dir(dir, &session.project.name).display().to_string());
    }
    Ok(v)
}

/// Atende uma operação sobre um `Store` (o do arquivo, no headless; o da interface, no app).
pub fn handle_with_store(session: &CoreSession, store: &mut Store, params: &Value, now: u64) -> Result<Outcome, BridgeError> {
    let r: DeliveryRequest = request(params, "DELIVERY")?;
    let op = r.op.ok_or_else(|| BridgeError::invalid_params("DELIVERY: falta `op` (formats | settings | preflight | queue_add | queue_list | queue_remove | queue_move | queue_cancel | queue_retry | queue_clear | queue_pause | queue_resume | queue_run | list | delete | package | poster | reexport)"))?;
    let done = |value: Value| Ok(Outcome { value, changed: false, run: false });
    let changed = |value: Value| Ok(Outcome { value, changed: true, run: false });
    let need_id = || r.id.ok_or_else(|| BridgeError::invalid_params("DELIVERY: falta `id`"));
    match op {
        DeliveryOp::Formats => done(formats_json()),
        DeliveryOp::Settings => {
            if let Some(patch) = &r.settings {
                let new = merge_settings(&store.settings, patch)?;
                let dir = project_dir(session)?;
                // a pasta de saída (inclusive a de Documentos) e as extras precisam caber nas pastas permitidas
                r.confine(&new.out_dir(&dir, &session.project.name))?;
                new.extra_dirs.iter().try_for_each(|d| r.confine(Path::new(d)))?;
                store.settings = new;
                return changed(settings_json(&store.settings, session)?);
            }
            done(settings_json(&store.settings, session)?)
        }
        DeliveryOp::Preflight => {
            let dir = project_dir(session)?;
            let src = source_of(&r)?;
            let preset = preset_of(r.preset.as_deref().unwrap_or("tv"))?;
            let out_dir = store.settings.out_dir(&dir, &session.project.name);
            let confined = r.confine(&out_dir).err().map(|e| e.message);
            let rep = preflight::run(&preflight::Input { project: &session.project, blobs: &session.blobs, source: &src, preset, out_dir: &out_dir, measure_loudness: r.loudness.unwrap_or(true), folder_error: confined });
            let mut v = to_json(&rep)?;
            v["blocking"] = json!(rep.worst == Level::Fail);
            v["pass"] = json!(rep.count(Level::Pass));
            v["warn"] = json!(rep.count(Level::Warn));
            v["fail"] = json!(rep.count(Level::Fail));
            done(v)
        }
        DeliveryOp::QueueAdd => {
            let dir = project_dir(session)?;
            let pairs: Vec<(SourceRef, String)> = if r.sources.is_empty() {
                let src = source_of(&r)?;
                let ids: Vec<String> = if r.presets.is_empty() { vec![r.preset.clone().unwrap_or_else(|| "tv".into())] } else { r.presets.clone() };
                matrix(&[src], &ids)
            } else {
                let ids: Vec<String> = if r.presets.is_empty() { vec![r.preset.clone().unwrap_or_else(|| "tv".into())] } else { r.presets.clone() };
                matrix(&r.sources, &ids)
            };
            // tudo ou nada: valida cada par antes de pôr algum na fila
            let mut specs = Vec::new();
            for (src, pid) in &pairs {
                let preset = preset_of(pid)?;
                preset.availability().map_err(|e| BridgeError::failed(format!("formato {pid} indisponível: {e}")))?;
                let mut spec = store.plan(&session.project, &dir, src, preset, r.crf, now).map_err(tool_error)?;
                spec.poster_frame = r.poster_frame;
                r.confine_spec(&spec)?;
                // o nome do próximo item já conta com os que entram agora
                let id = store.add_job(spec);
                specs.push(id);
            }
            let jobs: Vec<Value> = specs.iter().filter_map(|id| store.job(*id)).map(job_json).collect();
            Ok(Outcome { value: json!({ "added": jobs, "queued": store.pending() }), changed: true, run: r.run })
        }
        DeliveryOp::QueueList => {
            let n = r.count.unwrap_or(200);
            let jobs: Vec<Value> = store.jobs.iter().take(n).map(job_json).collect();
            done(json!({ "jobs": jobs, "paused": store.paused, "queued": store.pending(), "running": store.jobs.iter().any(|j| j.status == Status::Running) }))
        }
        DeliveryOp::QueueRemove => {
            store.remove_job(need_id()?).map_err(BridgeError::invalid_params)?;
            changed(json!({ "removed": r.id }))
        }
        DeliveryOp::QueueMove => {
            let to = r.to.ok_or_else(|| BridgeError::invalid_params("queue_move: falta `to`"))?;
            store.move_job(need_id()?, to).map_err(BridgeError::invalid_params)?;
            changed(json!({ "order": store.jobs.iter().map(|j| j.spec.id).collect::<Vec<_>>() }))
        }
        DeliveryOp::QueueCancel => {
            let id = need_id()?;
            if !store.cancel_queued(id) {
                return Err(BridgeError::invalid_params(format!("o item {id} não está esperando na fila (o que já está exportando só se cancela pelo app aberto)")));
            }
            changed(json!({ "canceled": id }))
        }
        DeliveryOp::QueueRetry => {
            store.retry(need_id()?).map_err(BridgeError::invalid_params)?;
            Ok(Outcome { value: json!({ "retry": r.id, "queued": store.pending() }), changed: true, run: r.run })
        }
        DeliveryOp::QueueClear => {
            let n = store.clear_finished();
            changed(json!({ "cleared": n }))
        }
        DeliveryOp::QueuePause => {
            store.paused = true;
            changed(json!({ "paused": true }))
        }
        DeliveryOp::QueueResume => {
            store.paused = false;
            Ok(Outcome { value: json!({ "paused": false, "queued": store.pending() }), changed: true, run: true })
        }
        DeliveryOp::QueueRun => {
            store.paused = false;
            Ok(Outcome { value: json!({ "paused": false, "queued": store.pending() }), changed: true, run: true })
        }
        DeliveryOp::List => {
            let dir = project_dir(session)?;
            store.refresh_missing();
            let n = r.count.unwrap_or(100);
            let items: Vec<Value> = store.deliverables.iter().rev().take(n).map(|d| deliverable_json(d, &dir)).collect();
            done(json!({ "deliverables": items, "total": store.deliverables.len(), "out_dir": store.settings.out_dir(&dir, &session.project.name).display().to_string() }))
        }
        DeliveryOp::Delete => {
            let dir = project_dir(session)?;
            if let Some(f) = store.deliverable(need_id()?).and_then(|d| d.folder()) {
                r.confine(&f)?;
            }
            let moved = store.trash_deliverable(need_id()?, &dir, now).map_err(BridgeError::failed)?;
            changed(json!({ "deleted": r.id, "moved_to": moved.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() }))
        }
        DeliveryOp::Package => {
            let dir = project_dir(session)?;
            let id = need_id()?;
            let d = store.deliverable(id).ok_or_else(|| BridgeError::invalid_params(format!("entregável {id} não existe")))?.clone();
            if let Some(f) = d.folder() {
                r.confine(&f)?;
            }
            let (zip, entries) = build_package(session, &dir, &d, now).map_err(BridgeError::failed)?;
            if let Some(x) = store.deliverable_mut(id) {
                x.result.package = Some(zip.display().to_string());
            }
            changed(json!({ "package": zip.display().to_string(), "entries": entries }))
        }
        DeliveryOp::Poster => {
            let dir = project_dir(session)?;
            let id = need_id()?;
            let d = store.deliverable(id).ok_or_else(|| BridgeError::invalid_params(format!("entregável {id} não existe")))?.clone();
            r.confine(&Store::thumbs_dir(&dir))?;
            let frame = r.frame.unwrap_or_else(|| poster::default_frame(&session.project, &d.spec.source));
            let info = write_poster(session, &dir, id, &d.spec.source, frame, r.title.as_deref().unwrap_or(""), r.subtitle.as_deref().unwrap_or("")).map_err(BridgeError::failed)?;
            if let Some(x) = store.deliverable_mut(id) {
                x.poster = Some(info.clone());
            }
            changed(json!({ "poster": info }))
        }
        DeliveryOp::Reexport => {
            let dir = project_dir(session)?;
            let id = need_id()?;
            let d = store.deliverable(id).ok_or_else(|| BridgeError::invalid_params(format!("entregável {id} não existe")))?.clone();
            let preset = preset_of(&d.spec.preset)?;
            let mut spec = store.plan(&session.project, &dir, &d.spec.source, preset, d.spec.crf, now).map_err(tool_error)?;
            spec.poster_frame = d.spec.poster_frame;
            spec.extra_dirs = d.spec.extra_dirs.clone();
            spec.package = d.spec.package;
            r.confine_spec(&spec)?;
            let new_id = store.add_job(spec);
            Ok(Outcome { value: json!({ "added": store.job(new_id).map(job_json), "queued": store.pending() }), changed: true, run: r.run })
        }
    }
}

fn delivery_headless(session: &mut CoreSession, params: &Value) -> Result<Value, BridgeError> {
    let dir = project_dir(session);
    // sem pasta só as operações que não tocam o disco do projeto funcionam
    let (mut store, warning) = match &dir {
        Ok(d) => {
            let l = Store::load(d, now_ms());
            (l.store, l.warning)
        }
        Err(_) => (Store::default(), None),
    };
    let out = handle_with_store(session, &mut store, params, now_ms())?;
    let mut value = out.value;
    if out.changed {
        let d = dir.as_ref().map_err(|e| e.clone())?;
        store.save(d).map_err(tool_error)?;
    }
    if out.run {
        let d = dir?;
        let req = request::<DeliveryRequest>(params, "DELIVERY")?;
        let ran = run_queue(session, &mut store, &d, &req)?;
        value["ran"] = json!(ran);
    }
    if let Some(w) = warning {
        value["store_warning"] = json!(w);
    }
    Ok(value)
}

/// Roda os itens da fila em sequência (headless): grava o `entrega.json` a cada mudança de estado.
fn run_queue(session: &mut CoreSession, store: &mut Store, dir: &Path, req: &DeliveryRequest) -> Result<Vec<Value>, BridgeError> {
    let mut ran = Vec::new();
    let limit = req.limit.unwrap_or(usize::MAX);
    while ran.len() < limit {
        let Some(id) = store.next_runnable() else { break };
        store.mark_running(id);
        store.save(dir).map_err(tool_error)?;
        let spec = store.job(id).map(|j| j.spec.clone()).ok_or_else(|| BridgeError::failed("item sumiu da fila"))?;
        let t0 = std::time::Instant::now();
        // itens guardados por outra sessão também respeitam as pastas permitidas de quem manda rodar
        let allowed = req.confine_spec(&spec).map_err(|e| JobError::Failed(e.message)).and_then(|_| run_job(session, &spec, dir, None));
        match allowed {
            Ok(res) => store.finish_ok(id, res, t0.elapsed().as_secs_f32(), now_ms()),
            Err(JobError::Canceled) => store.finish_canceled(id, t0.elapsed().as_secs_f32()),
            Err(JobError::Failed(m)) => store.finish_err(id, m, t0.elapsed().as_secs_f32()),
        }
        store.save(dir).map_err(tool_error)?;
        if let Some(j) = store.job(id) {
            ran.push(job_json(j));
        }
    }
    Ok(ran)
}

// ------------------------------------------------------------------------------------------------ executar um item

#[derive(Debug, Clone, PartialEq)]
pub enum JobError {
    Canceled,
    Failed(String),
}

impl From<BridgeError> for JobError {
    fn from(e: BridgeError) -> Self {
        JobError::Failed(e.message)
    }
}

impl From<aiai_tools::ToolError> for JobError {
    fn from(e: aiai_tools::ToolError) -> Self {
        JobError::Failed(e.to_string())
    }
}

fn is_canceled(ctl: &Option<Arc<ExportControl>>, e: &BridgeError) -> bool {
    ctl.as_ref().is_some_and(|c| c.is_canceled()) || e.message.contains(export_ctl::CANCELED)
}

/// Tamanho em bytes de um arquivo ou, numa pasta, a soma dos arquivos dela.
fn size_of(p: &Path) -> u64 {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| size_of(&e.path())).sum(),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// Unidade de saída do item: o arquivo, ou a pasta (sequência de imagens e stems).
fn output_unit(preset: &FormatPreset, out: &Path) -> PathBuf {
    if matches!(preset.kind, Kind::PngSequence | Kind::Stems) { out.parent().map(Path::to_path_buf).unwrap_or_else(|| out.to_path_buf()) } else { out.to_path_buf() }
}

/// Exporta um item da fila: gera a saída do formato, a miniatura, as cópias nas pastas extras e o pacote. `ctl` (se houver)
/// recebe progresso e pede pausa/cancelamento. Falha ou cancelamento apagam o que este item criou.
pub fn run_job(session: &mut CoreSession, spec: &JobSpec, project_dir: &Path, ctl: Option<Arc<ExportControl>>) -> Result<JobResult, JobError> {
    let preset = formats::preset(&spec.preset).ok_or_else(|| JobError::Failed(format!("formato desconhecido: {}", spec.preset)))?;
    preset.availability().map_err(|e| JobError::Failed(format!("formato {} indisponível: {e}", spec.preset)))?;
    let _guard = ctl.clone().map(export_ctl::install);
    let out = PathBuf::from(&spec.out);
    let unit = output_unit(preset, &out);
    let existed = unit.exists();
    let r = run_inner(session, spec, preset, project_dir, &ctl);
    if r.is_err() && !existed {
        // nada pela metade: o arquivo (ou a pasta) que este item começou some
        let _ = if unit.is_dir() { std::fs::remove_dir_all(&unit) } else { std::fs::remove_file(&unit) };
    }
    r
}

fn run_inner(session: &mut CoreSession, spec: &JobSpec, preset: &'static FormatPreset, project_dir: &Path, ctl: &Option<Arc<ExportControl>>) -> Result<JobResult, JobError> {
    let out = PathBuf::from(&spec.out);
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| JobError::Failed(format!("criar a pasta {}: {e}", d.display())))?;
    }
    let mut res = JobResult { fps: session.project.settings.fps(), ..Default::default() };
    match preset.kind {
        Kind::Video | Kind::PngSequence => {
            let params = formats::export_params(&spec.source.params(), preset, &spec.out, spec.crf);
            let v = crate::production_tools::export_video(session, &params).map_err(|e| if is_canceled(ctl, &e) { JobError::Canceled } else { JobError::Failed(e.message) })?;
            fill_from_export(&mut res, &v, preset, &out);
        }
        Kind::Wav => {
            if let Some(c) = ctl {
                c.begin(1, Phase::Audio);
            }
            let mut buf = mix::mix_source(&session.project, &session.blobs, &spec.source)?.ok_or_else(|| JobError::Failed("a fonte não tem som para gravar".into()))?;
            if let Some(l) = preset.loudness.and_then(|l| serde_json::from_value(json!(l)).ok()) {
                audio::normalize_mix(&mut buf, l);
            }
            audio::export_wav(&buf, &out, WavBits::Pcm24)?;
            let rep = audio::loudness_report(&buf);
            res.files = vec![out.display().to_string()];
            res.duration_s = buf.duration_s();
            res.frames = (buf.duration_s() * res.fps).round() as u64;
            res.lufs = rep.integrated_lufs;
            res.true_peak = rep.true_peak_dbtp;
            if let Some(c) = ctl {
                c.frame_done();
            }
        }
        Kind::Stems => {
            let folder = output_unit(preset, &out);
            let base = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "stems".into());
            let stems = mix::stems(&session.project, &session.blobs, &spec.source)?;
            if stems.is_empty() {
                return Err(JobError::Failed("nenhuma trilha da fonte tem som para gravar".into()));
            }
            if let Some(c) = ctl {
                c.begin(stems.len() as u64, Phase::Audio);
            }
            let mut longest = 0.0f64;
            for (label, buf) in &stems {
                if ctl.as_ref().is_some_and(|c| c.checkpoint().is_err()) {
                    return Err(JobError::Canceled);
                }
                let path = folder.join(format!("{base}_{}.wav", aiai_tools::delivery::names::safe_name(label)));
                audio::export_wav(buf, &path, WavBits::Pcm24)?;
                longest = longest.max(buf.duration_s());
                res.files.push(path.display().to_string());
                if let Some(c) = ctl {
                    c.frame_done();
                }
            }
            res.is_sequence = true; // a entrega é a pasta
            res.duration_s = longest;
            res.frames = (longest * res.fps).round() as u64;
        }
        Kind::ContactSheet => {
            if let Some(c) = ctl {
                c.begin(1, Phase::Frames);
            }
            let img = poster::contact(&session.project, &session.blobs, &spec.source)?;
            img.save_png(&out).map_err(JobError::Failed)?;
            res.files = vec![out.display().to_string()];
            res.width = img.width;
            res.height = img.height;
            if let Some(c) = ctl {
                c.frame_done();
            }
        }
    }
    if ctl.as_ref().is_some_and(|c| c.is_canceled()) {
        return Err(JobError::Canceled);
    }
    if let Some(c) = ctl {
        c.set_phase(Phase::Finishing);
    }
    res.size_bytes = if res.is_sequence { res.files.iter().map(|f| size_of(Path::new(f))).sum() } else { res.files.first().map(|f| size_of(Path::new(f))).unwrap_or(0) };
    // sidecars (.srt) entram na lista depois do principal: o tamanho é só o do principal
    if !res.is_sequence {
        res.size_bytes = res.files.first().map(|f| size_of(Path::new(f))).unwrap_or(0);
    }

    // miniatura do cartão
    if matches!(preset.kind, Kind::Video | Kind::PngSequence | Kind::ContactSheet) {
        match write_thumbnail(session, project_dir, spec, preset, &out) {
            Ok(name) => res.thumbnail = Some(name),
            Err(e) => res.warnings.push(format!("miniatura: {e}")),
        }
    }
    // cópias nas pastas extras
    for d in &spec.extra_dirs {
        match copy_into(Path::new(d), &res) {
            Ok(n) => res.copies.push(format!("{} ({n} arquivo(s))", Path::new(d).display())),
            Err(e) => res.warnings.push(format!("cópia em {d}: {e}")),
        }
    }
    // pacote de entrega
    if spec.package && !matches!(preset.kind, Kind::ContactSheet) {
        match make_package(session, project_dir, spec, &res) {
            Ok((zip, poster_info)) => {
                for d in &spec.extra_dirs {
                    let _ = std::fs::create_dir_all(d);
                    if let Some(n) = zip.file_name() {
                        if let Err(e) = std::fs::copy(&zip, Path::new(d).join(n)) {
                            res.warnings.push(format!("cópia do pacote em {d}: {e}"));
                        }
                    }
                }
                res.package = Some(zip.display().to_string());
                res.poster = Some(poster_info);
            }
            Err(e) => res.warnings.push(format!("pacote de entrega: {e}")),
        }
    }
    Ok(res)
}

fn fill_from_export(res: &mut JobResult, v: &Value, preset: &FormatPreset, out: &Path) {
    let main = if preset.kind == Kind::PngSequence {
        let first = ["00001", "00000"].iter().map(|n| PathBuf::from(out.display().to_string().replace("%05d", n))).find(|p| p.exists());
        res.is_sequence = true;
        first.unwrap_or_else(|| out.to_path_buf())
    } else {
        v["path"].as_str().map(PathBuf::from).unwrap_or_else(|| out.to_path_buf())
    };
    res.files = vec![main.display().to_string()];
    if let Some(a) = v["sidecar_srts"].as_array() {
        res.files.extend(a.iter().filter_map(|x| x.as_str().map(String::from)));
    }
    res.frames = v["frames"].as_u64().unwrap_or(0);
    let (num, den) = (v["fps"][0].as_f64().unwrap_or(0.0), v["fps"][1].as_f64().unwrap_or(1.0));
    if den > 0.0 && num > 0.0 {
        res.fps = num / den;
    }
    let probe = &v["probe"];
    res.width = probe["width"].as_u64().or_else(|| v["size"][0].as_u64()).unwrap_or(0) as u32;
    res.height = probe["height"].as_u64().or_else(|| v["size"][1].as_u64()).unwrap_or(0) as u32;
    res.duration_s = probe["duration"].as_f64().filter(|d| *d > 0.0).unwrap_or(res.frames as f64 / res.fps.max(1.0));
    let lufs = v.pointer("/audio/loudness/integrated_lufs").or_else(|| v.pointer("/audio/integrated_lufs"));
    res.lufs = lufs.and_then(Value::as_f64);
    res.true_peak = v.pointer("/audio/loudness/true_peak_dbtp").and_then(Value::as_f64);
    if let Some(w) = v["warnings"].as_array() {
        res.warnings.extend(w.iter().take(10).map(|x| format!("{} ({} quadros)", x["warning"].as_str().unwrap_or("?"), x["frames"])));
    }
}

fn write_thumbnail(session: &CoreSession, project_dir: &Path, spec: &JobSpec, preset: &FormatPreset, out: &Path) -> Result<String, String> {
    let dir = Store::thumbs_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("criar {}: {e}", dir.display()))?;
    let img = if preset.kind == Kind::ContactSheet {
        poster::scale_to_width(&aiai_paint::Image::load_png(out)?, poster::THUMB_WIDTH)
    } else {
        poster::thumbnail(&session.project, &session.blobs, &spec.source, spec.poster_frame).map_err(|e| e.to_string())?
    };
    let name = format!("{}.png", spec.id);
    img.save_png(&dir.join(&name))?;
    Ok(name)
}

/// Copia os arquivos da entrega (a pasta inteira numa sequência/stems) para `dest`; devolve quantos arquivos.
fn copy_into(dest: &Path, res: &JobResult) -> Result<usize, String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("criar {}: {e}", dest.display()))?;
    let mut n = 0;
    if res.is_sequence {
        let Some(folder) = res.files.first().and_then(|f| Path::new(f).parent()) else { return Ok(0) };
        let target = dest.join(folder.file_name().unwrap_or_default());
        std::fs::create_dir_all(&target).map_err(|e| format!("criar {}: {e}", target.display()))?;
        for e in std::fs::read_dir(folder).map_err(|e| e.to_string())?.flatten() {
            if e.path().is_file() {
                std::fs::copy(e.path(), target.join(e.file_name())).map_err(|er| format!("copiar {}: {er}", e.path().display()))?;
                n += 1;
            }
        }
    } else {
        for f in &res.files {
            let p = Path::new(f);
            if let Some(name) = p.file_name() {
                std::fs::copy(p, dest.join(name)).map_err(|e| format!("copiar {}: {e}", p.display()))?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// Pôster (1280 de largura) com título e legenda opcionais; grava em `entrega-miniaturas/<id>-poster.png`.
pub fn write_poster(session: &CoreSession, project_dir: &Path, id: u64, source: &SourceRef, frame: u32, title: &str, subtitle: &str) -> Result<PosterInfo, String> {
    let dir = Store::thumbs_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("criar {}: {e}", dir.display()))?;
    let img = poster::poster(&session.project, &session.blobs, source, frame, poster::POSTER_WIDTH, title, subtitle).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{id}-poster.png"));
    img.save_png(&file)?;
    Ok(PosterInfo { file: file.display().to_string(), frame, title: title.to_owned() })
}

fn make_package(session: &CoreSession, project_dir: &Path, spec: &JobSpec, res: &JobResult) -> Result<(PathBuf, PosterInfo), String> {
    let info = write_poster(session, project_dir, spec.id, &spec.source, spec.poster_frame.unwrap_or_else(|| poster::default_frame(&session.project, &spec.source)), "", "")?;
    let fake = Deliverable { spec: spec.clone(), result: res.clone(), finished_ms: now_ms(), poster: Some(info.clone()), missing: false };
    let (zip, _) = zip_for(session, &fake, now_ms())?;
    Ok((zip, info))
}

/// Pacote de um entregável já pronto: o pôster do editor (ou um gerado agora) entra junto.
fn build_package(session: &CoreSession, project_dir: &Path, d: &Deliverable, now: u64) -> Result<(PathBuf, Vec<Value>), String> {
    let mut d = d.clone();
    if d.poster.as_ref().is_none_or(|p| !Path::new(&p.file).exists()) {
        d.poster = Some(write_poster(session, project_dir, d.id(), &d.spec.source, d.spec.poster_frame.unwrap_or_else(|| poster::default_frame(&session.project, &d.spec.source)), "", "")?);
    }
    zip_for(session, &d, now)
}

fn zip_for(session: &CoreSession, d: &Deliverable, now: u64) -> Result<(PathBuf, Vec<Value>), String> {
    let lines = package::credit_lines(&session.project, &d.spec.source).map_err(|e| e.to_string())?;
    let (date, _) = aiai_tools::delivery::names::date_time_of(now);
    let text = package::credits_text(&session.project.name, &d.spec.source_name, &date, &lines);
    let plan: PackagePlan = package::plan_for(d, text);
    let entries = package::entries_of(&plan)?;
    let main = d.main_file().ok_or("a entrega não tem arquivo")?;
    let zip = if d.result.is_sequence { d.folder().map(|f| f.with_file_name(format!("{}_pacote.zip", plan.folder))) } else { Some(Path::new(main).with_file_name(format!("{}_pacote.zip", plan.folder))) }.ok_or("sem pasta para o pacote")?;
    let rep = package::write_zip(&zip, &entries, now)?;
    Ok((zip, rep.iter().map(|r| json!({ "name": r.name, "size": r.size, "sha256": r.sha256 })).collect()))
}
