//! Biblioteca de recursos no app (S24 2.7–2.8): o estado dos painéis Biblioteca e Prévia (índice da pasta, filtros,
//! seleção, metadados em edição, miniaturas e prévias feitas numa thread de fundo, janela de importar) e as ações que
//! passam pela ação `LIBRARY` da bridge (a mesma do MCP): usar no projeto, salvar do projeto, arrastar para o canvas.
//!
//! A pasta `Documentos\AnimeIAI\{Modelos 3D, Materiais, Texturas, Paletas, Poses, Pincéis}` é a verdade e o índice é
//! reconstruído quando alguém grava (painel, bridge, MCP: contador de mudanças do `aiai-assets`), quando a janela volta ao
//! foco e quando o usuário pede. Sem sessão (`--no-session`) e sem `AIAI_LIBRARY_DIR` nada é lido nem gravado.

use super::actions;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_assets::import::{ImportOpts, Imported};
use aiai_assets::index::{Entry, Index, Query, Sort};
use aiai_assets::kind::{IMAGE_EXTS, ext_of};
use aiai_assets::palette::Palette;
use aiai_assets::{Kind, store};
use aiai_core::BlobHash;
use crossbeam_channel::{Receiver, Sender};
use egui::{ColorImage, Rect, TextureHandle, TextureOptions};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Lado das miniaturas feitas (a grade mostra menor ou maior por escala).
pub(crate) const THUMB_PX: u32 = 192;
/// Maior lado da imagem da prévia (textura grande é reduzida para a tela).
const PREVIEW_MAX_SIDE: u32 = 2048;

/// O que se arrasta da grade (para o canvas).
#[derive(Clone, Debug)]
pub(crate) struct LibDrag {
    pub id: String,
}

/// Campos de metadados em edição de um recurso (texto como o usuário digita).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Draft {
    pub id: String,
    pub name: String,
    pub tags: String,
    pub author: String,
    pub license: String,
    pub source: String,
    pub description: String,
}

impl Draft {
    pub(crate) fn of(e: &Entry) -> Draft {
        Draft { id: e.id.clone(), name: e.env.name.clone(), tags: e.env.tags.join(", "), author: e.env.author.name.clone(), license: e.env.license_text(), source: e.env.source_text(), description: e.env.description.clone() }
    }
}

/// Janela de importar: arquivos escolhidos e o que o usuário informa para todos.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImportDialog {
    pub files: Vec<PathBuf>,
    pub as_material: bool,
    pub license: String,
    pub author: String,
    pub source: String,
    pub tags: String,
}

/// Estado da prévia do recurso selecionado.
#[derive(Default)]
pub(crate) enum ModelState {
    #[default]
    None,
    Loading,
    Ready(Arc<aiai_3d_model::Model>, BlobHash),
    Failed(String),
}

#[derive(Default)]
pub(crate) struct Preview {
    /// Recurso a que a prévia pertence.
    pub id: String,
    pub model: ModelState,
    /// Último quadro do turntable.
    pub frame: Option<TextureHandle>,
    pub pending: Option<(Receiver<Result<Vec<u8>, String>>, (u32, u32))>,
    pub started: Option<Instant>,
    pub paused: bool,
    pub yaw: f32,
    /// Imagem da textura/material (repete para o teste 3×3).
    pub image: Option<TextureHandle>,
    pub image_loading: bool,
    pub image_err: Option<String>,
    pub tile3: bool,
    pub model_err: Option<String>,
}

enum Slot {
    Pending,
    Ready(TextureHandle),
    /// Tipo sem imagem (paleta, pose) ou falha: a grade desenha o ícone/amostras.
    None,
}

enum Job {
    Thumb { key: String, entry: Box<Entry> },
    Image { id: String, path: PathBuf },
    Model { id: String, path: PathBuf },
    Import { root: PathBuf, files: Vec<PathBuf>, opts: ImportOpts },
    Extract { root: PathBuf, path: PathBuf, name: String },
}

enum Done {
    Thumb { key: String, res: Result<Option<ColorImage>, String> },
    Skipped(String),
    Image { id: String, res: Result<ColorImage, String> },
    Model { id: String, res: Result<(Arc<aiai_3d_model::Model>, BlobHash), String> },
    Import(Vec<(String, Result<Vec<Imported>, String>)>),
    Extracted(Result<Imported, String>),
}

struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Done>,
}

fn color_image(t: &aiai_assets::thumb::Thumb) -> ColorImage {
    ColorImage::from_rgba_premultiplied([t.w as usize, t.h as usize], &t.rgba)
}

fn run_job(job: Job, visible: &Mutex<HashSet<String>>) -> Done {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let guard = |name: &str, f: &mut dyn FnMut() -> Done| catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        tracing::warn!("biblioteca: {name} entrou em pânico");
        Done::Extracted(Err(format!("{name}: erro interno")))
    });
    match job {
        Job::Thumb { key, entry } => {
            if !visible.lock().map(|v| v.contains(&key)).unwrap_or(true) {
                return Done::Skipped(key);
            }
            catch_unwind(AssertUnwindSafe(|| aiai_assets::thumb::thumbnail(&entry, THUMB_PX))).map_or_else(
                |_| Done::Thumb { key: key.clone(), res: Err("a miniatura entrou em pânico".into()) },
                |r| Done::Thumb { key: key.clone(), res: r.map(|o| o.map(|t| color_image(&t))) },
            )
        }
        Job::Image { id, path } => {
            let mut f = || {
                let res = image::open(&path).map_err(|e| format!("{}: {e}", path.display())).map(|img| {
                    let img = if img.width() > PREVIEW_MAX_SIDE || img.height() > PREVIEW_MAX_SIDE { img.thumbnail(PREVIEW_MAX_SIDE, PREVIEW_MAX_SIDE) } else { img };
                    let rgba = img.to_rgba8();
                    ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
                });
                Done::Image { id: id.clone(), res }
            };
            guard("prévia da imagem", &mut f)
        }
        Job::Model { id, path } => {
            let mut f = || Done::Model { id: id.clone(), res: aiai_assets::thumb::load_model(&path) };
            guard("prévia do modelo", &mut f)
        }
        Job::Import { root, files, opts } => {
            let _ = store::ensure_folders(&root);
            let mut out = Vec::new();
            for f in files {
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let r = catch_unwind(AssertUnwindSafe(|| aiai_assets::import::import_file(&root, &f, &opts))).unwrap_or_else(|_| Err(format!("{name}: erro interno ao importar")));
                out.push((name, r));
            }
            aiai_assets::bump();
            Done::Import(out)
        }
        Job::Extract { root, path, name } => {
            let mut f = || {
                let r = image::open(&path).map_err(|e| format!("{}: {e}", path.display())).and_then(|img| {
                    let mut pal = aiai_assets::palette::extract_from_image(&img.to_rgba8(), 8);
                    pal.name = format!("{name} (paleta)");
                    let opts = ImportOpts { source: format!("extraída de: {name}"), tags: vec!["extraída".into()], ..Default::default() };
                    aiai_assets::import::add_palette(&root, &pal, &opts, &name)
                });
                Done::Extracted(r)
            };
            guard("extração de paleta", &mut f)
        }
    }
}

pub(crate) struct LibraryState {
    pub index: Index,
    root: Option<PathBuf>,
    scanned: bool,
    seen_counter: u64,
    seen_brushes: u64,
    was_focused: bool,
    last_pass: u64,
    pub kind: Option<Kind>,
    pub tag: Option<String>,
    pub query: String,
    pub sort: Sort,
    /// Tamanho das miniaturas na grade (px).
    pub size: f32,
    pub selected: Option<String>,
    pub draft: Draft,
    pub preview: Preview,
    pub import: Option<ImportDialog>,
    /// Aviso curto no rodapé da grade (resultado de importar, erro de ação).
    pub msg: Option<String>,
    pub busy: usize,
    thumbs: HashMap<String, Slot>,
    visible: Arc<Mutex<HashSet<String>>>,
    worker: Option<Worker>,
    /// Retângulo do painel Biblioteca no último desenho e a passada em que foi desenhado (arquivos soltos na janela).
    pub drop_rect: Option<(Rect, u64)>,
    /// O painel Pincéis precisa reler a pasta (a biblioteca mudou em `Pincéis`).
    pub brush_rescan: bool,
    /// Id do recurso que ainda deve ser selecionado quando o índice o trouxer (importado agora).
    want_select: Option<String>,
    pub show_problems: bool,
}

impl Default for LibraryState {
    fn default() -> Self {
        LibraryState {
            index: Index::default(),
            root: None,
            scanned: false,
            seen_counter: u64::MAX,
            seen_brushes: u64::MAX,
            was_focused: true,
            last_pass: u64::MAX,
            kind: None,
            tag: None,
            query: String::new(),
            sort: Sort::Name,
            size: 88.0,
            selected: None,
            draft: Draft::default(),
            preview: Preview::default(),
            import: None,
            msg: None,
            busy: 0,
            thumbs: HashMap::new(),
            visible: Arc::new(Mutex::new(HashSet::new())),
            worker: None,
            drop_rect: None,
            brush_rescan: false,
            want_select: None,
            show_problems: false,
        }
    }
}

fn thumb_key(e: &Entry) -> String {
    format!("{}:{}:{}", e.id, e.mtime, e.bytes)
}

impl LibraryState {
    /// Solta a prévia (turntable, imagem) e, com `thumbs`, as miniaturas da grade (orçamento de VRAM, `gpu_owners`).
    pub(crate) fn release_gpu(&mut self, thumbs: bool) -> bool {
        let mut had = self.preview.frame.take().is_some() | self.preview.image.take().is_some();
        if thumbs && !self.thumbs.is_empty() {
            self.thumbs.clear();
            had = true;
        }
        had
    }

    /// A biblioteca está ligada (há uma pasta raiz)?
    pub(crate) fn enabled(&self) -> bool {
        self.root.is_some()
    }

    pub(crate) fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Uma vez por quadro: reavalia a raiz, relê a pasta se algo mudou e recolhe o que a thread de fundo terminou.
    pub(crate) fn sync(&mut self, ctx: &egui::Context, brush_version: u64) {
        let pass = ctx.cumulative_pass_nr();
        if self.last_pass == pass {
            return;
        }
        self.last_pass = pass;
        let root = aiai_assets::root();
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let mut scan = !self.scanned || root != self.root || aiai_assets::change_counter() != self.seen_counter || brush_version != self.seen_brushes || (focused && !self.was_focused);
        self.was_focused = focused;
        if root != self.root {
            self.root = root;
            self.thumbs.clear();
            self.selected = None;
            scan = true;
        }
        if scan {
            self.rescan();
            self.seen_brushes = brush_version;
        }
        self.drain(ctx);
    }

    /// Relê a pasta (botão "Reler" e mudanças vistas por `sync`).
    pub(crate) fn rescan(&mut self) {
        self.scanned = true;
        self.seen_counter = aiai_assets::change_counter();
        let before = self.brush_sig();
        self.index = match &self.root {
            Some(r) => Index::scan(r),
            None => Index::default(),
        };
        if self.brush_sig() != before {
            self.brush_rescan = true;
        }
        if let Some(id) = self.want_select.take() {
            if self.index.get(&id).is_some() {
                self.select(Some(id));
            } else {
                self.want_select = Some(id);
            }
        }
        // seleção que sumiu
        if self.selected.as_ref().is_some_and(|s| self.index.get(s).is_none()) {
            self.select(None);
        } else if let Some(s) = self.selected.clone() {
            // a pasta mudou: o rascunho acompanha o que está no disco, a menos que o usuário esteja digitando no recurso
            if let Some(e) = self.index.get(&s) {
                if self.draft.id != s {
                    self.draft = Draft::of(e);
                }
            }
        }
        let live: HashSet<String> = self.index.entries.iter().map(thumb_key).collect();
        self.thumbs.retain(|k, _| live.contains(k));
    }

    fn brush_sig(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for e in self.index.entries.iter().filter(|e| e.kind == Kind::Brush) {
            (&e.id, e.mtime, e.bytes).hash(&mut h);
        }
        h.finish()
    }

    pub(crate) fn select(&mut self, id: Option<String>) {
        if self.selected == id {
            return;
        }
        self.selected = id.clone();
        self.preview = Preview { tile3: true, ..Default::default() };
        self.draft = id.as_deref().and_then(|i| self.index.get(i)).map(Draft::of).unwrap_or_default();
    }

    pub(crate) fn selected_entry(&self) -> Option<&Entry> {
        self.selected.as_deref().and_then(|i| self.index.get(i))
    }

    /// Consulta atual (filtros da árvore + busca + ordem).
    pub(crate) fn query(&self) -> Query {
        Query { kind: self.kind, text: self.query.clone(), tags: self.tag.iter().cloned().collect(), sort: self.sort }
    }

    fn worker(&mut self, ctx: &egui::Context) -> &Worker {
        if self.worker.is_none() {
            let (tx, jrx) = crossbeam_channel::unbounded::<Job>();
            let (dtx, rx) = crossbeam_channel::unbounded::<Done>();
            let (c, vis) = (ctx.clone(), Arc::clone(&self.visible));
            let spawned = std::thread::Builder::new().name("aiai-library".into()).spawn(move || {
                while let Ok(job) = jrx.recv() {
                    let done = run_job(job, &vis);
                    if dtx.send(done).is_err() {
                        break;
                    }
                    c.request_repaint();
                }
            });
            if let Err(e) = spawned {
                tracing::warn!("biblioteca: thread de fundo não abriu ({e})");
            }
            self.worker = Some(Worker { tx, rx });
        }
        self.worker.as_ref().expect("worker")
    }

    fn drain(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.worker else { return };
        let done: Vec<Done> = w.rx.try_iter().collect();
        for d in done {
            match d {
                Done::Thumb { key, res } => match res {
                    Ok(Some(img)) => {
                        let tex = ctx.load_texture(format!("lib-thumb-{key}"), img, TextureOptions::LINEAR);
                        self.thumbs.insert(key, Slot::Ready(tex));
                    }
                    Ok(None) => {
                        self.thumbs.insert(key, Slot::None);
                    }
                    Err(e) => {
                        tracing::warn!("biblioteca: miniatura: {e}");
                        self.thumbs.insert(key, Slot::None);
                    }
                },
                Done::Skipped(key) => {
                    self.thumbs.remove(&key);
                }
                Done::Image { id, res } => {
                    if self.preview.id == id {
                        self.preview.image_loading = false;
                        match res {
                            Ok(img) => {
                                let opts = TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::LINEAR };
                                self.preview.image = Some(ctx.load_texture(format!("lib-image-{id}"), img, opts));
                            }
                            Err(e) => self.preview.image_err = Some(e),
                        }
                    }
                }
                Done::Model { id, res } => {
                    if self.preview.id == id {
                        self.preview.model = match res {
                            Ok((m, h)) => ModelState::Ready(m, h),
                            Err(e) => ModelState::Failed(e),
                        };
                    }
                }
                Done::Import(results) => {
                    self.busy = self.busy.saturating_sub(1);
                    let (mut ok, mut warns, mut errs) = (0usize, Vec::new(), Vec::new());
                    let mut first: Option<String> = None;
                    for (name, r) in results {
                        match r {
                            Ok(items) => {
                                for i in items {
                                    if i.path.as_os_str().is_empty() {
                                        warns.extend(i.warnings);
                                        continue;
                                    }
                                    ok += 1;
                                    first.get_or_insert(i.id);
                                    warns.extend(i.warnings.into_iter().map(|w| format!("{name}: {w}")));
                                }
                            }
                            Err(e) => errs.push(e),
                        }
                    }
                    let mut m = tr("lib.imported").replace("{n}", &ok.to_string());
                    if !errs.is_empty() {
                        m.push_str(&format!(" · {}: {}", tr("lib.import_failed"), errs.join("; ")));
                    }
                    if !warns.is_empty() {
                        m.push_str(&format!(" · {}", warns.join("; ")));
                    }
                    self.msg = Some(m);
                    self.want_select = first;
                    self.rescan();
                }
                Done::Extracted(r) => match r {
                    Ok(i) => {
                        self.msg = Some(tr("lib.palette_extracted").replace("{name}", &i.name));
                        self.want_select = Some(i.id);
                        aiai_assets::bump();
                        self.rescan();
                    }
                    Err(e) => self.msg = Some(e),
                },
            }
        }
    }

    /// Pede uma miniatura (uma vez por chave) se a célula está visível; devolve a textura quando pronta.
    pub(crate) fn thumb(&mut self, ctx: &egui::Context, e: &Entry) -> Option<TextureHandle> {
        let key = thumb_key(e);
        if let Ok(mut v) = self.visible.lock() {
            v.insert(key.clone());
        }
        match self.thumbs.get(&key) {
            Some(Slot::Ready(t)) => return Some(t.clone()),
            Some(Slot::Pending | Slot::None) => return None,
            None => {}
        }
        let wants_image = match e.kind {
            Kind::Texture | Kind::Material => IMAGE_EXTS.contains(&ext_of(&e.path).as_str()),
            Kind::Model3d | Kind::Brush => true,
            Kind::Palette | Kind::Pose => false,
        };
        if !wants_image {
            self.thumbs.insert(key, Slot::None);
            return None;
        }
        self.thumbs.insert(key.clone(), Slot::Pending);
        let job = Job::Thumb { key: key.clone(), entry: Box::new(e.clone()) };
        if self.worker(ctx).tx.send(job).is_err() {
            self.thumbs.insert(key, Slot::None);
        }
        None
    }

    /// Começou o quadro: só as miniaturas pedidas daqui em diante contam como visíveis.
    pub(crate) fn begin_visible(&mut self) {
        if let Ok(mut v) = self.visible.lock() {
            v.clear();
        }
    }

    /// A miniatura ainda está sendo feita? (a grade mostra o ícone até lá.)
    pub(crate) fn thumb_pending(&self, e: &Entry) -> bool {
        matches!(self.thumbs.get(&thumb_key(e)), Some(Slot::Pending))
    }

    /// Prepara a prévia do selecionado: carrega a imagem ou o modelo uma vez.
    pub(crate) fn ensure_preview(&mut self, ctx: &egui::Context) {
        let Some(e) = self.selected_entry().cloned() else { return };
        if self.preview.id != e.id {
            self.preview = Preview { id: e.id.clone(), tile3: true, ..Default::default() };
        }
        match e.kind {
            Kind::Texture | Kind::Material if self.preview.image.is_none() && !self.preview.image_loading && self.preview.image_err.is_none() && IMAGE_EXTS.contains(&ext_of(&e.path).as_str()) => {
                self.preview.image_loading = true;
                let _ = self.worker(ctx).tx.send(Job::Image { id: e.id.clone(), path: e.path.clone() });
            }
            Kind::Model3d if matches!(self.preview.model, ModelState::None) => {
                self.preview.model = ModelState::Loading;
                let _ = self.worker(ctx).tx.send(Job::Model { id: e.id.clone(), path: e.path.clone() });
            }
            _ => {}
        }
    }

    /// Abre a janela de importar com estes arquivos (os de pincéis vão direto para o importador de pincéis).
    pub(crate) fn begin_import(&mut self, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        let dlg = self.import.get_or_insert_with(|| ImportDialog { license: String::new(), ..Default::default() });
        for f in files {
            if !dlg.files.contains(&f) {
                dlg.files.push(f);
            }
        }
    }

    /// Importa de verdade (thread de fundo); `dlg` vem da janela de importar.
    pub(crate) fn run_import(&mut self, ctx: &egui::Context, dlg: ImportDialog) {
        let Some(root) = self.root.clone() else {
            self.msg = Some(tr("lib.disabled").to_owned());
            return;
        };
        let opts = ImportOpts {
            author: dlg.author,
            license: dlg.license,
            source: dlg.source,
            tags: aiai_assets::parse_tags(&dlg.tags),
            as_kind: dlg.as_material.then_some(Kind::Material),
            name: None,
            folder: String::new(),
        };
        self.busy += 1;
        self.msg = Some(tr("lib.importing").to_owned());
        if self.worker(ctx).tx.send(Job::Import { root, files: dlg.files, opts }).is_err() {
            self.busy = self.busy.saturating_sub(1);
        }
    }

    /// Extrai as cores dominantes da textura selecionada para uma paleta nova da biblioteca.
    pub(crate) fn extract_palette(&mut self, ctx: &egui::Context, e: &Entry) {
        let Some(root) = self.root.clone() else { return };
        let _ = store::ensure_folders(&root);
        let _ = self.worker(ctx).tx.send(Job::Extract { root, path: e.path.clone(), name: e.env.name.clone() });
    }

    /// A pasta mudou por uma ação do painel (já gravada): relê já.
    pub(crate) fn changed(&mut self) {
        aiai_assets::bump();
        self.rescan();
    }

    /// Soltar arquivos do sistema na janela: valem para a biblioteca quando o ponteiro está sobre o painel dela.
    pub(crate) fn accepts_drop(&self, ctx: &egui::Context) -> bool {
        let pos = ctx.input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()));
        let now = ctx.cumulative_pass_nr();
        matches!((self.drop_rect, pos), (Some((r, pass)), Some(p)) if now.saturating_sub(pass) <= 2 && r.contains(p))
    }
}

// ================================================================================================ ações com o documento

/// Usa o recurso no projeto pela ação `LIBRARY` (um passo de desfazer). `layer` = cria a camada no corte ativo (modelo 3D,
/// textura); senão copia só para o projeto. Atualiza a seleção do editor e avisa.
pub(crate) fn use_entry(env: &mut Env, id: &str, layer: bool) {
    let Some(e) = env.ed.library.index.get(id).cloned() else { return };
    let cut = env.ed.active_cut(env.core);
    let mut p = json!({ "op": "use", "id": id });
    let toon = e.kind == Kind::Material && !IMAGE_EXTS.contains(&ext_of(&e.path).as_str());
    if toon {
        // material toon: as cores vão para a camada 3D ativa; sem ela, para a única camada 3D do corte
        let is_3d = |env: &Env, l: &aiai_core::Id| aiai_tools::model3d::layer_params(&env.core.project, l).is_ok();
        let target = env.ed.layer.clone().filter(|l| is_3d(env, l)).or_else(|| {
            let rows = super::commands::layer_rows(&env.core.project, cut.as_ref()?);
            let mut only = rows.into_iter().map(|r| r.id).filter(|l| is_3d(env, l));
            only.next().filter(|_| only.next().is_none())
        });
        match target {
            Some(l) => p["layer_id"] = json!(l),
            None => {
                env.ed.notify(tr("lib.pick_3d_layer"));
                return;
            }
        }
    } else if matches!(e.kind, Kind::Model3d | Kind::Texture | Kind::Material) {
        if layer {
            let Some(cut) = cut else {
                env.ed.notify(tr("err.no_cut"));
                return;
            };
            p["cut"] = json!(cut);
            p["mode"] = json!("layer");
        } else {
            p["mode"] = json!("asset");
        }
    }
    match actions::run(env.core, "LIBRARY", p) {
        Ok(v) => {
            if let Some(l) = v.get("layer").and_then(Value::as_str) {
                env.ed.layer = Some(aiai_core::Id(l.to_owned()));
                env.ed.cel_number = None;
                env.ed.show_reference = true;
            }
            if let Some(pl) = v.get("palette").and_then(Value::as_str) {
                env.ed.palette = Some(aiai_core::Id(pl.to_owned()));
            }
            if e.kind == Kind::Brush {
                select_brush(env, &e, &v);
            }
            env.ed.notify(tr("lib.used").replace("{name}", &e.env.name));
        }
        Err(m) => env.ed.notify(m),
    }
}

/// Seleciona o pincel da biblioteca na ferramenta (relê a pasta de pincéis antes, para o pincel recém-importado entrar).
fn select_brush(env: &mut Env, e: &Entry, _v: &Value) {
    env.ed.brushes.rescan();
    match env.ed.brushes.find_id(&e.id) {
        Some(i) => {
            env.ed.opts.brush_preset = i;
            env.ed.opts.brush_custom = None;
        }
        None => env.ed.notify(tr("lib.brush_missing")),
    }
}

/// Salva na biblioteca uma paleta ou um modelo 3D do projeto.
pub(crate) fn save_from_project(env: &mut Env, from: &str, node: &aiai_core::Id) {
    let p = match from {
        // paleta feita pelo usuário: autor e licença do perfil dele (as mesmas dos pincéis)
        "palette" => json!({ "op": "save", "from": "palette", "palette": node, "asset_author": env.ed.brushes.profile_author().name, "license": env.ed.brushes.profile_license() }),
        _ => json!({ "op": "save", "from": "model", "asset": node }),
    };
    let ctx_root = env.ed.library.root().map(Path::to_path_buf);
    if ctx_root.is_none() {
        env.ed.library.msg = Some(tr("lib.disabled").to_owned());
        return;
    }
    match actions::run(env.core, "LIBRARY", p) {
        Ok(v) => {
            let name = v.pointer("/saved/name").and_then(Value::as_str).unwrap_or("").to_owned();
            if let Some(id) = v.pointer("/saved/id").and_then(Value::as_str) {
                env.ed.library.want_select = Some(id.to_owned());
            }
            env.ed.library.msg = Some(tr("lib.saved").replace("{name}", &name));
            env.ed.library.changed();
        }
        Err(m) => env.ed.library.msg = Some(m),
    }
}

/// Atualiza o conteúdo de uma paleta da biblioteca com uma paleta do projeto (nova versão, o manifesto continua).
pub(crate) fn update_palette_from_project(env: &mut Env, id: &str, node: &aiai_core::Id) {
    let Some(e) = env.ed.library.index.get(id).cloned() else { return };
    let Some(pd) = super::commands::palette_data(&env.core.project, node).cloned() else { return };
    if pd.colors.is_empty() {
        env.ed.library.msg = Some(tr("lib.palette_empty").to_owned());
        return;
    }
    let pal = aiai_tools::library::palette_of(&e.env.name, &pd);
    match replace_palette_payload(&e, &pal) {
        Ok(()) => {
            env.ed.library.msg = Some(tr("lib.palette_updated").replace("{name}", &e.env.name));
            env.ed.library.changed();
        }
        Err(m) => env.ed.library.msg = Some(m),
    }
}

fn replace_palette_payload(e: &Entry, pal: &Palette) -> Result<(), String> {
    let mut env = e.env.clone();
    env.payload = pal.to_payload();
    env.touch();
    // versão: incrementa o último número (1.0.0 -> 1.0.1)
    let mut parts: Vec<u64> = env.version.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    if parts.is_empty() {
        parts = vec![1, 0, 0];
    }
    if let Some(l) = parts.last_mut() {
        *l += 1;
    }
    env.version = parts.iter().map(u64::to_string).collect::<Vec<_>>().join(".");
    aiai_assets::store::write_envelope(&e.storage, &e.path, &env)
}

/// Soltar um recurso arrastado da biblioteca no canvas: modelo = camada 3D, textura = camada de referência, paleta entra
/// no documento.
pub(crate) fn drop_target(env: &mut Env, ui: &egui::Ui, resp: &egui::Response) {
    if resp.dnd_hover_payload::<LibDrag>().is_some() {
        ui.painter().rect_stroke(resp.rect.shrink(2.0), 4.0, egui::Stroke::new(2.0, env.t.accent), egui::StrokeKind::Inside);
    }
    if let Some(d) = resp.dnd_release_payload::<LibDrag>() {
        use_entry(env, &d.id, true);
    }
}

/// Abre a pasta no Explorador (a do tipo, ou a raiz).
pub(crate) fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    if let Err(e) = r {
        tracing::warn!("não abriu a pasta {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-studio-lib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn draft_follows_the_entry_and_selection_resets_the_preview() {
        let root = tmp("draft");
        std::fs::create_dir_all(root.join("Texturas")).unwrap();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])).save(root.join("Texturas").join("papel.png")).unwrap();
        let mut st = LibraryState::default();
        st.root = Some(root.clone());
        st.rescan();
        assert_eq!(st.index.entries.len(), 1);
        let id = st.index.entries[0].id.clone();
        st.select(Some(id.clone()));
        assert_eq!(st.draft.name, "papel");
        assert!(st.preview.tile3, "a textura abre com o teste 3x3 ligado");
        // a seleção que some da pasta é solta
        std::fs::remove_file(root.join("Texturas").join("papel.png")).unwrap();
        st.rescan();
        assert!(st.selected.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn thumbnails_and_import_run_on_the_worker_thread() {
        let (root, src) = (tmp("worker"), tmp("worker-src"));
        let png = src.join("grao.png");
        image::RgbaImage::from_pixel(300, 200, image::Rgba([90, 120, 30, 255])).save(&png).unwrap();
        let ctx = egui::Context::default();
        let mut st = LibraryState::default();
        st.root = Some(root.clone());
        st.rescan();
        // importar pela thread
        st.run_import(&ctx, ImportDialog { files: vec![png], license: "CC0-1.0".into(), tags: "teste".into(), ..Default::default() });
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            st.drain(&ctx);
            if st.busy == 0 {
                break;
            }
        }
        assert_eq!(st.busy, 0, "importar terminou");
        assert_eq!(st.index.entries.len(), 1, "{:?}", st.msg);
        assert!(st.selected.is_some(), "o importado fica selecionado");
        // miniatura
        let e = st.index.entries[0].clone();
        st.begin_visible();
        assert!(st.thumb(&ctx, &e).is_none());
        let mut tex = None;
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            st.drain(&ctx);
            st.begin_visible();
            tex = st.thumb(&ctx, &e);
            if tex.is_some() {
                break;
            }
        }
        let t = tex.expect("miniatura pronta");
        assert_eq!(t.size(), [192, 128]);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&src);
    }

    #[test]
    fn palette_update_bumps_the_version_and_keeps_the_manifest() {
        let root = tmp("palupd");
        let pal = Palette { name: "Cores".into(), colors: vec![aiai_assets::palette::parse_loose_json(&json!(["#ff0000"])).unwrap().colors.remove(0)], warnings: vec![] };
        let imp = aiai_assets::import::add_palette(&root, &pal, &ImportOpts { license: "CC0-1.0".into(), author: "Ana".into(), ..Default::default() }, "teste").unwrap();
        let idx = Index::scan(&root);
        let e = idx.get(&imp.id).unwrap().clone();
        let newer = aiai_assets::palette::parse_loose_json(&json!(["#00ff00", "#0000ff"])).unwrap();
        replace_palette_payload(&e, &newer).unwrap();
        let after = Index::scan(&root);
        let n = after.get(&imp.id).unwrap();
        assert_eq!((n.env.version.as_str(), n.stats.colors, n.env.author.name.as_str()), ("1.0.1", Some(2), "Ana"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
