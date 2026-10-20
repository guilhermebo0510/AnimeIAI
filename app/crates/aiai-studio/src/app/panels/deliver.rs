//! Workspace Entrega (S24 4.8–4.12): estado e execução da fila de exportação, painel Formatos (predefinições, com a matriz
//! "uma fonte × vários formatos") e o pedido nas Propriedades. Os outros painéis (Fila, Destinos, Entregáveis, Pôster,
//! Verificação final) estão em [`super::deliver_panels`].
//!
//! A fila e os entregáveis moram em `entrega.json` ao lado do projeto (`aiai_tools::delivery::store`): reabrir o projeto
//! devolve a fila parada com o que faltava. Cada item roda a ação `EXPORT_VIDEO` (ou o WAV, os stems, a folha de contato)
//! pelo mesmo `aiai_bridge::delivery_tools::run_job` que o MCP usa, numa thread com uma CÓPIA da sessão: a exportação só lê
//! o documento, então a interface segue respondendo. O progresso por quadro, a pausa e o cancelamento vêm do
//! `ExportControl` que a thread recebe. Um item por vez.

use crate::app::actions::{self, DocKind};
use crate::app::editor::Editor;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_bridge::delivery_tools::{self, JobError};
use aiai_bridge::export_ctl::ExportControl;
use aiai_core::{CoreSession, Project};
use aiai_tools::delivery::formats::{self, FormatPreset, PRESETS};
use aiai_tools::delivery::preflight::{self, Level, Report};
use aiai_tools::delivery::store::{JobResult, Store};
use aiai_tools::delivery::SourceRef;
use egui::{FontId, Rect, Sense, Ui};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Pedido da verificação final: a fonte, o formato e a revisão do documento (mudou o documento, a verificação envelhece).
pub(crate) type PfKey = (SourceRef, &'static str, u64);

/// O estado da Entrega que está no disco, aberto para o projeto atual.
pub(crate) struct Loaded {
    pub dir: PathBuf,
    pub store: Store,
    /// Mudou desde a última gravação.
    pub dirty: bool,
}

/// O item que está exportando agora.
pub(crate) struct Running {
    pub id: u64,
    pub ctl: Arc<ExportControl>,
    rx: crossbeam_channel::Receiver<(u64, Result<JobResult, JobError>)>,
    pub started: Instant,
}

#[derive(Default)]
pub(crate) struct Preflight {
    /// Última verificação concluída e para qual pedido.
    pub report: Option<(PfKey, Report)>,
    running: Option<(PfKey, crossbeam_channel::Receiver<Report>)>,
    /// A verificação foi pedida pelo botão Exportar: ao terminar sem falha o pedido entra na fila.
    pub pending_export: bool,
    /// A verificação achou falha e espera "Exportar mesmo assim" ou o cancelamento.
    pub blocked: bool,
}

impl Preflight {
    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }
}

/// Seleção de pasta pelo diálogo nativo (numa thread: o diálogo não trava a interface).
pub(crate) enum PickFor {
    Custom,
    Extra,
}

/// Editor de pôster do entregável selecionado.
#[derive(Default)]
pub(crate) struct PosterEd {
    /// Entregável a que o editor se refere.
    pub id: Option<u64>,
    pub frame: u32,
    pub title: String,
    pub subtitle: String,
    /// Prévia (1280 reduzido) e a que foi pedida por último.
    pub tex: Option<egui::TextureHandle>,
    pub shown: Option<(u64, u32, String, String)>,
    pub asked: Option<(u64, u32, String, String)>,
    pub changed_at: Option<Instant>,
    pub rx: Option<crossbeam_channel::Receiver<Result<aiai_paint::Image, String>>>,
}

pub(crate) struct DeliverState {
    /// Formato escolhido (índice em [`PRESETS`]).
    pub preset: usize,
    pub source: Option<SourceRef>,
    pub crf: Option<u32>,
    /// Formatos marcados para exportar de uma vez (matriz com a fonte escolhida).
    pub matrix: BTreeSet<&'static str>,
    /// Faz a verificação final antes de enfileirar.
    pub verify_first: bool,
    pub loaded: Option<Loaded>,
    pub running: Option<Running>,
    pub pf: Preflight,
    /// Entregável selecionado.
    pub sel: Option<u64>,
    pub confirm_delete: Option<u64>,
    pub poster: PosterEd,
    /// Miniaturas (por caminho): textura e o instante em que o arquivo foi lido.
    pub thumbs: HashMap<String, Option<egui::TextureHandle>>,
    pub picker: Option<(PickFor, crossbeam_channel::Receiver<Option<PathBuf>>)>,
    /// Arrasto de um item da fila: id e linha de origem.
    pub drag: Option<u64>,
    /// Texto do modelo de nome em edição (o que vale está em `store.settings`).
    pub template_buf: Option<String>,
}

impl Default for DeliverState {
    fn default() -> Self {
        DeliverState {
            preset: 0,
            source: None,
            crf: None,
            matrix: BTreeSet::new(),
            verify_first: true,
            loaded: None,
            running: None,
            pf: Preflight::default(),
            sel: None,
            confirm_delete: None,
            poster: PosterEd::default(),
            thumbs: HashMap::new(),
            picker: None,
            drag: None,
            template_buf: None,
        }
    }
}

impl DeliverState {
    pub(crate) fn preset(&self) -> &'static FormatPreset {
        &PRESETS[self.preset.min(PRESETS.len() - 1)]
    }

    pub(crate) fn store(&self) -> Option<&Store> {
        self.loaded.as_ref().map(|l| &l.store)
    }

    /// Dá acesso mutável ao estado e marca para gravar.
    pub(crate) fn store_mut(&mut self) -> Option<&mut Store> {
        self.loaded.as_mut().map(|l| {
            l.dirty = true;
            &mut l.store
        })
    }

    pub(crate) fn key(&self, p: &Project) -> Option<PfKey> {
        Some((self.source.clone()?, self.preset().id, p.revision))
    }
}

/// Todas as fontes possíveis: edições, episódios e cortes, com o rótulo para a lista.
pub(crate) fn sources(p: &Project) -> Vec<(SourceRef, String)> {
    let mut out: Vec<(SourceRef, String)> = actions::docs_of(p, DocKind::Edit).into_iter().map(|(id, n)| (SourceRef::edit(id), format!("{}: {n}", tr("deliver.src.edit")))).collect();
    for ep in &p.roots.episodes {
        if let Some(n) = p.node(ep) {
            out.push((SourceRef::episode(ep.clone()), format!("{}: {}", tr("deliver.src.episode"), n.name)));
        }
    }
    for (cut, code, scene, _) in super::edit::project_cuts(p) {
        out.push((SourceRef::cut(cut), format!("{}: {code} · {scene}", tr("deliver.src.cut"))));
    }
    out
}

/// Nome de arquivo seguro (usado também pelo Áudio para nomear WAVs).
pub(crate) use aiai_tools::delivery::names::safe_name;

/// Nome do formato no idioma ativo.
pub(crate) fn preset_label(id: &str) -> String {
    formats::preset(id).map(|p| tr(p.key).to_owned()).unwrap_or_else(|| id.to_owned())
}

pub(crate) fn now_ms() -> u64 {
    delivery_tools::now_ms()
}

/// Abre um arquivo ou pasta com o programa padrão do sistema.
pub(crate) fn open_path(p: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let r = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(p).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    r.map(|_| ()).map_err(|e| format!("{}: {e}", p.display()))
}

/// Mostra o arquivo na pasta (seleciona no Explorador); pasta abre direto.
pub(crate) fn reveal(p: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let r = if p.is_dir() { std::process::Command::new("explorer").arg(p).spawn() } else { std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn() };
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(if p.is_dir() { p } else { p.parent().unwrap_or(p) }).spawn();
    r.map(|_| ()).map_err(|e| format!("{}: {e}", p.display()))
}

// ------------------------------------------------------------------------------------------------ execução da fila

/// Chamado a cada quadro pelo app (de qualquer workspace): abre o estado do projeto, recolhe a exportação pronta, começa
/// a próxima da fila, recolhe a verificação final, a prévia do pôster e a pasta escolhida, e grava `entrega.json`.
pub(crate) fn tick(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    ensure_loaded(core, ed);
    poll_running(ed, ctx);
    start_next(core, ed, ctx);
    poll_preflight(core, ed, ctx);
    poll_picker(ed);
    super::deliver_panels::poll_poster(ed, ctx);
    let st = &mut ed.deliver;
    if let Some(l) = st.loaded.as_mut().filter(|l| l.dirty) {
        l.dirty = false;
        if let Err(e) = l.store.save(&l.dir) {
            ed.toast = Some(format!("{}: {e}", tr("deliver.save_failed")));
        }
    }
    if ed.deliver.running.is_some() || ed.deliver.pf.is_running() {
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn ensure_loaded(core: &CoreSession, ed: &mut Editor) {
    let st = &mut ed.deliver;
    if st.loaded.as_ref().map(|l| &l.dir) == core.path.as_ref() {
        return;
    }
    // o projeto mudou (outro aberto, ou salvo pela primeira vez): o que exportava não vale mais para o estado novo
    if let Some(r) = st.running.take() {
        r.ctl.cancel();
    }
    st.pf = Preflight::default();
    st.sel = None;
    st.thumbs.clear();
    st.poster = PosterEd::default();
    st.loaded = None;
    let Some(dir) = core.path.clone() else { return };
    let l = Store::load(&dir, now_ms());
    let mut store = l.store;
    // só grava se reabrir mudou algo (item que rodava voltou para a fila): abrir um projeto não cria `entrega.json`
    let before = store.clone();
    store.on_reopen();
    let resumed = store.pending();
    let dirty = store != before;
    st.loaded = Some(Loaded { dir, store, dirty });
    if let Some(w) = l.warning {
        ed.toast = Some(format!("{}: {w}", tr("deliver.state_unreadable")));
    } else if resumed > 0 {
        ed.toast = Some(tr("deliver.resumed_toast").replace("{0}", &resumed.to_string()));
    }
}

fn poll_running(ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.deliver;
    let Some(run) = &st.running else { return };
    let (id, secs) = (run.id, run.started.elapsed().as_secs_f32());
    let res = match run.rx.try_recv() {
        Ok((_, r)) => r,
        Err(crossbeam_channel::TryRecvError::Empty) => return,
        Err(crossbeam_channel::TryRecvError::Disconnected) => Err(JobError::Failed(tr("deliver.lost").to_owned())),
    };
    st.running = None;
    let name = st.store().and_then(|s| s.job(id)).map(|j| Path::new(&j.spec.out).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_default();
    let mut toast = String::new();
    if let Some(store) = st.store_mut() {
        match res {
            Ok(r) => {
                let warns = r.warnings.len();
                store.finish_ok(id, r, secs, now_ms());
                store.refresh_missing();
                toast = format!("{}: {name}", tr("deliver.done"));
                if warns > 0 {
                    toast.push_str(&format!(" ({warns} {})", tr("deliver.warnings")));
                }
            }
            Err(JobError::Canceled) => {
                store.finish_canceled(id, secs);
                toast = format!("{}: {name}", tr("deliver.status.canceled"));
            }
            Err(JobError::Failed(m)) => {
                toast = format!("{}: {}", tr("deliver.failed"), actions::excerpt(&m, 160));
                store.finish_err(id, m, secs);
            }
        }
    }
    st.thumbs.clear();
    if !toast.is_empty() {
        ed.toast = Some(toast);
    }
    ctx.request_repaint();
}

fn start_next(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.deliver;
    if st.running.is_some() {
        return;
    }
    let Some(l) = st.loaded.as_mut() else { return };
    let Some(id) = l.store.next_runnable() else { return };
    l.store.mark_running(id);
    l.dirty = true;
    let Some(spec) = l.store.job(id).map(|j| j.spec.clone()) else { return };
    let dir = l.dir.clone();
    // cópia da sessão: a exportação só lê o documento
    let mut copy = CoreSession::new(core.project.clone());
    copy.blobs = core.blobs.clone();
    copy.path = core.path.clone();
    let ctl = ExportControl::new();
    let (ctl2, (tx, rx)) = (ctl.clone(), crossbeam_channel::bounded(1));
    let spawned = std::thread::Builder::new().name("aiai-export".into()).spawn(move || {
        let r = delivery_tools::run_job(&mut copy, &spec, &dir, Some(ctl2));
        let _ = tx.send((id, r));
    });
    match spawned {
        Ok(_) => {
            st.running = Some(Running { id, ctl, rx, started: Instant::now() });
            ctx.request_repaint();
        }
        Err(e) => {
            if let Some(s) = st.store_mut() {
                s.finish_err(id, format!("não consegui iniciar a thread de exportação: {e}"), 0.0);
            }
        }
    }
}

/// Pausa ou retoma a fila: o item que exporta para (e continua de onde parou) e nada novo começa enquanto estiver parada.
pub(crate) fn set_paused(st: &mut DeliverState, on: bool) {
    if let Some(s) = st.store_mut() {
        s.paused = on;
    }
    if let Some(r) = &st.running {
        r.ctl.set_paused(on);
    }
}

/// Cancela o que exporta agora (o arquivo parcial é apagado) ou, se ainda espera, tira da fila de espera.
pub(crate) fn cancel_job(st: &mut DeliverState, id: u64) {
    if st.running.as_ref().is_some_and(|r| r.id == id) {
        if let Some(r) = &st.running {
            r.ctl.cancel();
        }
    } else if let Some(s) = st.store_mut() {
        s.cancel_queued(id);
    }
}

// ------------------------------------------------------------------------------------------------ pedido e verificação

/// Põe na fila o pedido atual (o formato escolhido ou, com a matriz marcada, cada formato marcado) e devolve quantos.
pub(crate) fn enqueue_now(core: &CoreSession, ed: &mut Editor) -> usize {
    let st = &mut ed.deliver;
    let Some(src) = st.source.clone() else {
        ed.toast = Some(tr("deliver.no_source").to_owned());
        return 0;
    };
    let ids: Vec<&'static str> = if st.matrix.is_empty() { vec![st.preset().id] } else { PRESETS.iter().map(|p| p.id).filter(|id| st.matrix.contains(id)).collect() };
    // o ajuste manual de qualidade vale só para um formato (cada formato tem a sua escala de CRF)
    let crf = if ids.len() == 1 { st.crf } else { None };
    let Some(l) = st.loaded.as_mut() else {
        ed.toast = Some(tr("board.save_first").to_owned());
        return 0;
    };
    let mut n = 0;
    let mut err = None;
    for id in ids {
        let Some(preset) = formats::preset(id) else { continue };
        if let Err(e) = preset.availability() {
            err = Some(format!("{}: {e}", tr(preset.key)));
            continue;
        }
        match l.store.plan(&core.project, &l.dir, &src, preset, crf, now_ms()) {
            Ok(spec) => {
                l.store.add_job(spec);
                n += 1;
            }
            Err(e) => err = Some(e.to_string()),
        }
    }
    l.dirty = true;
    st.pf.blocked = false;
    if let Some(e) = err {
        ed.toast = Some(e);
    }
    n
}

/// Botão Exportar: com a verificação ligada, mede antes; sem falha o pedido entra na fila, com falha espera a decisão.
pub(crate) fn request_export(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.deliver;
    if st.source.is_none() {
        ed.toast = Some(tr("deliver.no_source").to_owned());
        return;
    }
    if st.loaded.is_none() {
        ed.toast = Some(tr("board.save_first").to_owned());
        return;
    }
    if !st.verify_first {
        enqueue_now(core, ed);
        return;
    }
    let key = st.key(&core.project);
    match (&st.pf.report, &key) {
        (Some((k, rep)), Some(cur)) if k == cur => {
            if rep.worst == Level::Fail {
                st.pf.blocked = true;
            } else {
                enqueue_now(core, ed);
            }
        }
        _ => {
            st.pf.pending_export = true;
            start_preflight(core, ed, ctx);
        }
    }
}

/// Começa a verificação final do pedido atual numa thread (mixa o som e mede: demora).
pub(crate) fn start_preflight(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.deliver;
    let (Some(l), Some(key), Some(src)) = (st.loaded.as_ref(), st.key(&core.project), st.source.clone()) else { return };
    if st.pf.running.is_some() {
        return;
    }
    let out_dir = l.store.settings.out_dir(&l.dir, &core.project.name);
    let preset = st.preset();
    let (project, blobs) = (core.project.clone(), core.blobs.clone());
    let (tx, rx) = crossbeam_channel::bounded(1);
    let ctx2 = ctx.clone();
    let spawned = std::thread::Builder::new().name("aiai-preflight".into()).spawn(move || {
        let rep = preflight::run(&preflight::Input { project: &project, blobs: &blobs, source: &src, preset, out_dir: &out_dir, measure_loudness: true, folder_error: None });
        let _ = tx.send(rep);
        ctx2.request_repaint();
    });
    if spawned.is_ok() {
        st.pf.running = Some((key, rx));
    }
}

fn poll_preflight(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    let st = &mut ed.deliver;
    let Some((key, rx)) = &st.pf.running else { return };
    let rep = match rx.try_recv() {
        Ok(r) => r,
        Err(crossbeam_channel::TryRecvError::Empty) => return,
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            st.pf.running = None;
            st.pf.pending_export = false;
            ed.toast = Some(tr("deliver.pf.lost").to_owned());
            return;
        }
    };
    let key = key.clone();
    st.pf.running = None;
    let worst = rep.worst;
    st.pf.report = Some((key, rep));
    if std::mem::take(&mut st.pf.pending_export) {
        if worst == Level::Fail {
            st.pf.blocked = true;
        } else {
            enqueue_now(core, ed);
        }
    }
    ctx.request_repaint();
}

fn poll_picker(ed: &mut Editor) {
    let st = &mut ed.deliver;
    let Some((kind, rx)) = &st.picker else { return };
    let picked = match rx.try_recv() {
        Ok(p) => p,
        Err(crossbeam_channel::TryRecvError::Empty) => return,
        Err(_) => None,
    };
    let kind = match kind {
        PickFor::Custom => 0,
        PickFor::Extra => 1,
    };
    st.picker = None;
    let Some(dir) = picked else { return };
    let text = dir.display().to_string();
    if let Some(s) = st.store_mut() {
        if kind == 0 {
            s.settings.dest = aiai_tools::delivery::store::Dest::Custom;
            s.settings.custom_dir = text;
        } else if !s.settings.extra_dirs.contains(&text) {
            s.settings.extra_dirs.push(text);
        }
    }
}

/// Abre o diálogo de pasta (thread); o resultado entra em `poll_picker`.
pub(crate) fn pick_folder(st: &mut DeliverState, kind: PickFor, start: Option<PathBuf>) {
    if st.picker.is_some() {
        return;
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let title = tr("deliver.dest.pick").to_owned();
    let spawned = std::thread::Builder::new().name("aiai-folder-dialog".into()).spawn(move || {
        let mut d = rfd::FileDialog::new().set_title(title);
        if let Some(dir) = start.filter(|d| d.is_dir()) {
            d = d.set_directory(dir);
        }
        let _ = tx.send(d.pick_folder());
    });
    if spawned.is_ok() {
        st.picker = Some((kind, rx));
    }
}

// ------------------------------------------------------------------------------------------------ ponte do MCP

/// `DELIVERY` com o app aberto: a fila e os destinos são os da interface (o MCP vê e mexe no que o painel mostra).
pub(crate) fn bridge(core: &CoreSession, ed: &mut Editor, ctx: &egui::Context, params: &Value) -> Result<Value, aiai_bridge::BridgeError> {
    use aiai_bridge::delivery_tools::{DeliveryOp, DeliveryRequest, handle_with_store};
    let req: DeliveryRequest = serde_json::from_value(params.clone()).map_err(|e| aiai_bridge::BridgeError::invalid_params(format!("DELIVERY: parâmetros inválidos: {e}")))?;
    let op = req.op;
    // o item que exporta agora só o app consegue cancelar
    if op == Some(DeliveryOp::QueueCancel) {
        if let Some(id) = req.id.filter(|id| ed.deliver.running.as_ref().is_some_and(|r| r.id == *id)) {
            cancel_job(&mut ed.deliver, id);
            return Ok(json!({ "canceled": id, "was_running": true }));
        }
    }
    let Some(l) = ed.deliver.loaded.as_mut() else {
        // sem pasta de projeto só as operações que não tocam o estado respondem (formatos, verificação sem destino)
        let mut tmp = Store::default();
        return handle_with_store(core, &mut tmp, params, now_ms()).map(|o| o.value);
    };
    let out = handle_with_store(core, &mut l.store, params, now_ms())?;
    if out.changed {
        l.dirty = true;
    }
    if out.run {
        set_paused(&mut ed.deliver, false);
        ctx.request_repaint();
    }
    match op {
        Some(DeliveryOp::QueuePause) => set_paused(&mut ed.deliver, true),
        Some(DeliveryOp::QueueResume | DeliveryOp::QueueRun) => set_paused(&mut ed.deliver, false),
        _ => {}
    }
    ed.deliver.pf.blocked = false;
    Ok(out.value)
}

// ------------------------------------------------------------------------------------------------ Predefinições (formatos)

pub(crate) fn presets(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    ui.spacing_mut().item_spacing.y = 3.0;
    egui::ScrollArea::vertical().id_salt("deliver_presets").auto_shrink([false, false]).show(ui, |ui| {
        for (i, p) in PRESETS.iter().enumerate() {
            let avail = p.availability();
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), Sense::click());
            env.note(format!("deliver:preset:{i}"), rect);
            let on = env.ed.deliver.preset == i;
            let painter = ui.painter();
            if on {
                painter.rect_filled(rect, 4.0, t.accent_dim);
                painter.rect_filled(Rect::from_min_size(rect.left_top() + egui::vec2(0.0, 4.0), egui::vec2(3.0, 38.0)), 1.5, t.accent);
            } else if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            let ink = if avail.is_ok() { t.text } else { t.text_dim.gamma_multiply(0.7) };
            let clip = painter.with_clip_rect(Rect::from_min_max(rect.min + egui::vec2(8.0, 0.0), rect.max - egui::vec2(26.0, 0.0)));
            clip.text(rect.left_top() + egui::vec2(10.0, 6.0), egui::Align2::LEFT_TOP, tr(p.key), FontId::proportional(13.0), ink);
            let sub = match &avail {
                Ok(()) => tr(p.tip).to_owned(),
                Err(e) => format!("{}: {e}", tr("deliver.unavailable")),
            };
            let mut job = egui::text::LayoutJob::simple(sub.clone(), FontId::proportional(11.0), if avail.is_ok() { t.text_dim } else { t.danger }, (rect.width() - 40.0).max(20.0));
            job.wrap.max_rows = 1;
            let g = clip.layout_job(job);
            clip.galley(rect.left_top() + egui::vec2(10.0, 25.0), g, t.text_dim);
            // caixa da matriz: marca vários formatos para exportar de uma vez
            let box_rect = Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), egui::vec2(16.0, 16.0));
            let checked = env.ed.deliver.matrix.contains(p.id);
            let hov = ui.rect_contains_pointer(box_rect);
            let painter = ui.painter();
            painter.rect(box_rect, 3.0, if checked { t.accent } else { t.bg_widget }, egui::Stroke::new(1.0, if hov { t.accent } else { t.border }), egui::StrokeKind::Inside);
            if checked {
                let c = box_rect.center();
                let stroke = egui::Stroke::new(2.0, t.bg_panel);
                painter.line_segment([c + egui::vec2(-4.0, 0.5), c + egui::vec2(-1.0, 3.5)], stroke);
                painter.line_segment([c + egui::vec2(-1.0, 3.5), c + egui::vec2(4.5, -3.5)], stroke);
            }
            let clicked = resp.on_hover_text(sub).clicked();
            if clicked {
                let pos = ui.input(|i| i.pointer.interact_pos());
                if pos.is_some_and(|p| box_rect.expand(3.0).contains(p)) {
                    if avail.is_ok() && !env.ed.deliver.matrix.remove(p.id) {
                        env.ed.deliver.matrix.insert(p.id);
                    }
                } else {
                    env.ed.deliver.preset = i;
                    env.ed.deliver.crf = None;
                }
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------ Propriedades (o pedido)

fn loudness_text(p: &FormatPreset) -> &'static str {
    match p.loudness {
        Some("arib") => "ARIB TR-B32 · −24 LKFS",
        Some("ebu") => "EBU R128 · −23 LUFS",
        Some("streaming") => "−14 LUFS",
        _ => "",
    }
}

pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let all = sources(&env.core.project);
    {
        let st = &mut env.ed.deliver;
        if st.source.as_ref().is_none_or(|s| !all.iter().any(|(x, _)| x == s)) {
            st.source = all.first().map(|(s, _)| s.clone());
        }
    }
    let preset = env.ed.deliver.preset();
    section(ui, &t, "deliver.request");
    ui.label(egui::RichText::new(tr(preset.key)).strong());
    dim_label(ui, &t, tr("deliver.source"));
    let current = env.ed.deliver.source.as_ref().and_then(|s| all.iter().find(|(x, _)| x == s)).map(|(_, l)| l.clone()).unwrap_or_else(|| tr("deliver.no_source").to_owned());
    let mut pick = None;
    let r = egui::ComboBox::from_id_salt("deliver_src").width((ui.available_width() - 8.0).max(80.0)).selected_text(current).show_ui(ui, |ui| {
        for (s, l) in &all {
            if ui.selectable_label(env.ed.deliver.source.as_ref() == Some(s), l).clicked() {
                pick = Some(s.clone());
            }
        }
    });
    env.note("deliver:source", r.response.rect);
    if let Some(s) = pick {
        env.ed.deliver.source = Some(s);
    }
    // o nome que o arquivo vai ter (modelo + versão livre) e a pasta
    let planned = env.ed.deliver.source.clone().and_then(|src| {
        let l = env.ed.deliver.loaded.as_ref()?;
        l.store.plan(&env.core.project, &l.dir, &src, preset, None, now_ms()).ok()
    });
    match (&planned, env.ed.deliver.loaded.as_ref()) {
        (Some(spec), _) => {
            dim_label(ui, &t, tr("deliver.will_save"));
            let out = Path::new(&spec.out);
            ui.label(egui::RichText::new(out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).size(12.0));
            dim_label(ui, &t, &format!("{} {}", tr("deliver.folder"), out.parent().map(|p| p.display().to_string()).unwrap_or_default()));
        }
        (None, Some(_)) => dim_label(ui, &t, tr("deliver.template_bad")),
        (None, None) => dim_label(ui, &t, tr("board.save_first")),
    }
    let (w, h, fps) = (env.core.project.settings.width, env.core.project.settings.height, env.core.project.settings.fps());
    egui::Grid::new("deliver_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("deliver.codec"));
        ui.label(if preset.codec.is_empty() { preset.ext } else { preset.codec });
        ui.end_row();
        if matches!(preset.kind, aiai_tools::delivery::formats::Kind::Video | aiai_tools::delivery::formats::Kind::PngSequence) {
            dim_label(ui, &t, tr("deliver.resolution"));
            let shown_h = preset.height.filter(|x| *x < h).unwrap_or(h);
            let shown_w = if shown_h == h { w } else { (w as f64 * shown_h as f64 / h as f64).round() as u32 & !1 };
            ui.label(format!("{shown_w}×{shown_h} · {fps:.3} fps"));
            ui.end_row();
        }
        if let Some(def) = preset.crf {
            dim_label(ui, &t, tr("deliver.crf"));
            let mut v = env.ed.deliver.crf.unwrap_or(def);
            let r = ui.add(egui::DragValue::new(&mut v).range(0..=51).speed(0.1));
            env.note("deliver:crf", r.rect);
            if v != env.ed.deliver.crf.unwrap_or(def) {
                env.ed.deliver.crf = Some(v);
            }
            ui.end_row();
        }
        dim_label(ui, &t, tr("deliver.loudness"));
        ui.label(if !preset.audio { tr("deliver.no_audio") } else if preset.loudness.is_none() { tr("deliver.loudness_raw") } else { loudness_text(preset) });
        ui.end_row();
        dim_label(ui, &t, tr("deliver.srt"));
        ui.label(tr(if preset.sidecar_srt { "deliver.yes" } else { "deliver.no" }));
        ui.end_row();
        if let Some(src) = &env.ed.deliver.source {
            if let Ok(frames) = src.total_frames(&env.core.project) {
                dim_label(ui, &t, tr("deliver.estimate"));
                let est = preset.estimate_bytes(frames as u64, fps, w, h, env.core.project.settings.audio_sample_rate);
                ui.label(format!("~{}", aiai_tools::delivery::store::fmt_size(est)));
                ui.end_row();
            }
        }
    });
    if let Err(e) = preset.availability() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("{}: {e}", tr("deliver.unavailable"))).color(t.danger).size(12.0));
    }
    ui.add_space(6.0);
    let n = env.ed.deliver.matrix.len();
    let label = if n > 1 { tr("deliver.export_n").replace("{0}", &n.to_string()) } else { tr("deliver.export").to_owned() };
    let usable = env.ed.deliver.source.is_some() && env.ed.deliver.loaded.is_some() && (preset.availability().is_ok() || n > 0);
    let b = ui.add_enabled(usable, egui::Button::new(label));
    env.note("deliver:props:export", b.rect);
    if b.on_hover_text(tr("deliver.export.tip")).clicked() {
        let ctx = ui.ctx().clone();
        request_export(env.core, env.ed, &ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, tr_in};
    use aiai_tools::delivery::store::Status;

    #[test]
    fn preset_keys_exist_in_both_languages() {
        for p in PRESETS {
            for k in [p.key, p.tip] {
                assert_ne!(tr_in(Lang::En, k), k, "{k}");
                assert_ne!(tr_in(Lang::Pt, k), k, "{k}");
            }
        }
    }

    #[test]
    fn every_preflight_code_has_text_in_both_languages() {
        for c in preflight::CODES {
            let k = format!("deliver.pf.{c}");
            assert_ne!(tr_in(Lang::En, &k), k, "{k}");
            assert_ne!(tr_in(Lang::Pt, &k), k, "{k}");
            // os dois idiomas usam os mesmos {n}
            for n in 0..5 {
                let ph = format!("{{{n}}}");
                assert_eq!(tr_in(Lang::En, &k).contains(&ph), tr_in(Lang::Pt, &k).contains(&ph), "{k} {ph}");
            }
        }
        for k in ["queued", "resumed", "running", "paused", "done", "failed", "canceled"] {
            let key = format!("deliver.status.{k}");
            assert_ne!(tr_in(Lang::En, &key), key);
            assert_ne!(tr_in(Lang::Pt, &key), key);
        }
    }

    fn session_with_cut(frames: u32) -> (CoreSession, SourceRef) {
        let mut core = CoreSession::for_tests("t");
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = crate::app::commands::quick_start(&mut core, frames, None).unwrap();
        (core, SourceRef::cut(cut))
    }

    fn state_in(dir: &Path) -> (CoreSession, Editor) {
        let (mut core, src) = session_with_cut(6);
        core.path = Some(dir.join("t.aiai"));
        std::fs::create_dir_all(core.path.as_ref().unwrap()).unwrap();
        let mut ed = Editor::default();
        ed.deliver.source = Some(src);
        (core, ed)
    }

    #[test]
    fn the_queue_persists_resumes_and_runs_one_at_a_time() {
        let dir = std::env::temp_dir().join(format!("aiai_deliver_st_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (core, mut ed) = state_in(&dir);
        let ctx = egui::Context::default();
        tick(&core, &mut ed, &ctx);
        assert!(ed.deliver.loaded.is_some(), "o projeto com pasta abre o estado");
        // matriz: dois formatos de áudio/imagem que não dependem do ffmpeg
        ed.deliver.matrix.insert("contact");
        ed.deliver.matrix.insert("wav");
        ed.deliver.verify_first = false;
        request_export(&core, &mut ed, &ctx);
        assert_eq!(ed.deliver.store().unwrap().jobs.len(), 2, "uma fonte × dois formatos");
        // pausar antes de começar: nada roda
        set_paused(&mut ed.deliver, true);
        tick(&core, &mut ed, &ctx);
        assert!(ed.deliver.running.is_none());
        assert!(dir.join("t.aiai").join("entrega.json").is_file(), "gravou ao mudar");
        // o app fecha e abre de novo: a fila volta parada
        let (core2, mut ed2) = state_in(&dir);
        tick(&core2, &mut ed2, &ctx);
        let s = ed2.deliver.store().unwrap();
        assert_eq!(s.pending(), 2);
        assert!(s.paused, "reabrir deixa a fila parada");
        // retomar: roda um por vez até acabar
        set_paused(&mut ed2.deliver, false);
        let t0 = Instant::now();
        while ed2.deliver.store().unwrap().jobs.iter().any(|j| matches!(j.status, Status::Queued | Status::Running)) {
            tick(&core2, &mut ed2, &ctx);
            assert!(ed2.deliver.store().unwrap().jobs.iter().filter(|j| j.status == Status::Running).count() <= 1);
            assert!(t0.elapsed() < Duration::from_secs(60), "a fila não terminou");
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = ed2.deliver.store().unwrap();
        // o corte de teste não tem som: o WAV falha com o motivo; a folha de contato conclui
        let by = |id: &str| s.jobs.iter().find(|j| j.spec.preset == id).unwrap();
        assert_eq!(by("contact").status, Status::Done);
        assert_eq!(by("wav").status, Status::Failed);
        assert!(by("wav").error.as_deref().unwrap().contains("não tem som"));
        assert_eq!(s.deliverables.len(), 1);
        assert!(s.deliverables[0].result.thumbnail.is_some() && !s.deliverables[0].missing);
        // tentar de novo (depois de consertar) coloca de volta no fim
        let id = by("wav").spec.id;
        ed2.deliver.store_mut().unwrap().retry(id).unwrap();
        assert_eq!(ed2.deliver.store().unwrap().pending(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn export_with_verification_waits_for_the_report_and_blocks_on_failure() {
        let dir = std::env::temp_dir().join(format!("aiai_deliver_pf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut core, mut ed) = state_in(&dir);
        let ctx = egui::Context::default();
        tick(&core, &mut ed, &ctx);
        // formato que não depende do ffmpeg
        ed.deliver.preset = PRESETS.iter().position(|p| p.id == "contact").unwrap();
        request_export(&core, &mut ed, &ctx);
        assert!(ed.deliver.pf.pending_export, "primeiro verifica");
        let t0 = Instant::now();
        while ed.deliver.pf.is_running() {
            tick(&core, &mut ed, &ctx);
            assert!(t0.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(ed.deliver.store().unwrap().jobs.len(), 1, "sem falha o pedido entrou na fila");
        // largura ímpar: falha; o pedido espera "exportar mesmo assim" e não entra sozinho
        core.project.settings.width = 65;
        ed.deliver.preset = PRESETS.iter().position(|p| p.id == "web").unwrap();
        if formats::preset("web").unwrap().availability().is_ok() {
            request_export(&core, &mut ed, &ctx);
            let t0 = Instant::now();
            while ed.deliver.pf.is_running() {
                tick(&core, &mut ed, &ctx);
                assert!(t0.elapsed() < Duration::from_secs(30));
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(ed.deliver.pf.blocked, "falha segura o pedido");
            assert_eq!(ed.deliver.store().unwrap().jobs.len(), 1);
            // exportar mesmo assim entra na fila (o motor recusa quando for rodar, com o motivo)
            assert_eq!(enqueue_now(&core, &mut ed), 1);
            assert!(!ed.deliver.pf.blocked);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_project_without_a_folder_cannot_export() {
        let (core, src) = session_with_cut(6);
        let mut ed = Editor::default();
        ed.deliver.source = Some(src);
        let ctx = egui::Context::default();
        tick(&core, &mut ed, &ctx);
        assert!(ed.deliver.loaded.is_none());
        request_export(&core, &mut ed, &ctx);
        assert_eq!(ed.toast.as_deref(), Some(tr("board.save_first")));
    }

    #[test]
    fn the_mcp_bridge_drives_the_same_queue_the_panel_shows() {
        let dir = std::env::temp_dir().join(format!("aiai_deliver_br_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (core, mut ed) = state_in(&dir);
        let ctx = egui::Context::default();
        tick(&core, &mut ed, &ctx);
        let cut = ed.deliver.source.clone().unwrap().id;
        let r = bridge(&core, &mut ed, &ctx, &json!({"op": "queue_add", "cut": cut, "preset": "contact", "run": true})).unwrap();
        assert_eq!(r["queued"], 1);
        assert_eq!(ed.deliver.store().unwrap().jobs.len(), 1, "o painel enxerga o item que o agente pôs");
        let t0 = Instant::now();
        while ed.deliver.store().unwrap().jobs[0].status != Status::Done {
            tick(&core, &mut ed, &ctx);
            assert!(t0.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(20));
        }
        let l = bridge(&core, &mut ed, &ctx, &json!({"op": "list"})).unwrap();
        assert_eq!(l["total"], 1);
        // pausar e cancelar pelo agente
        bridge(&core, &mut ed, &ctx, &json!({"op": "queue_pause"})).unwrap();
        assert!(ed.deliver.store().unwrap().paused);
        let _ = std::fs::remove_dir_all(dir);
    }
}
