//! Criar pincel e importar pincéis (painel Ferramenta, pincel).
//!
//! - Criar: parte de um pincel da biblioteca; ponta redonda, de forma ou de imagem (PNG/JPG escolhido pelo usuário),
//!   textura procedural ou de imagem, todos os ajustes do motor e uma prévia desenhada pelo próprio motor a cada
//!   mudança. Salva na biblioteca do usuário, na categoria escolhida (ou numa nova).
//! - Importar: lê pincéis do AnimeIAI (`.aiaibrush` e `.aiaibrushes`, sem perda: autor, licença e origem ficam) e
//!   pacotes de outros programas (`aiai_paint::brush_import`: GIMP, Photoshop .abr, Procreate, Krita, imagens), mostra o
//!   que achou com a miniatura do motor e importa os marcados. Os de terceiros viram `.aiaibrush` em
//!   `Pincéis\Importados\<pacote>` com a origem e a licença "desconhecida" (a do autor do pacote original), e o arquivo
//!   original é guardado. As imagens das pontas e texturas entram no projeto quando o pincel é usado.
//! - Exportar: `.aiaibrush` de um pincel ou `.aiaibrushes` de uma categoria, confirmando autor e licença antes.

use crate::app::brushes::{CATEGORIES, ExportMeta, cat_label, cat_of, imported_from_kind, sanitize};
use crate::app::ui_kit::Env;
use crate::i18n::tr;
use crate::prefs::BrushAuthorPrefs;
use aiai_paint::brush::{BrushAuthor, BrushPreset, BrushTexture, BrushTip, TextureKind, TextureMode};
use aiai_paint::brush_import::{ImportedBrush, ImportedTip};
use aiai_paint::brush_pack::{self, BrushFile, Package};
use aiai_paint::mask::Mask;

pub(crate) struct Maker {
    pub preset: BrushPreset,
    pub name: String,
    pub cat: String,
    /// Autor, licença, fonte e etiquetas (vírgulas) do pincel que está sendo criado.
    author: BrushAuthor,
    license: String,
    source: String,
    tags: String,
    /// Máscaras ("AIM1") da ponta e da textura de imagem escolhidas aqui (vão para a biblioteca ao salvar).
    tip_bytes: Option<Vec<u8>>,
    tex_bytes: Option<Vec<u8>>,
    preview: Option<egui::TextureHandle>,
    preview_of: Option<BrushPreset>,
    error: Option<String>,
    /// Categoria aberta dos ajustes (as mesmas das configurações avançadas da ferramenta, S25).
    cat_ui: super::tool_detail::Cat,
}

pub(crate) struct Importer {
    files: Vec<String>,
    errors: Vec<String>,
    /// Categoria e pacote (subpasta) dos pincéis de outros programas.
    cat: String,
    pack: String,
    /// Licença dos pincéis de outros programas ("desconhecida" até o usuário saber).
    license: String,
    items: Vec<ImportItem>,
    /// Arquivos de terceiros lidos (nome, bytes): o original é guardado se algum pincel dele for importado.
    originals: Vec<(String, Vec<u8>)>,
}

/// De onde vem o pincel a importar.
enum Kind {
    /// Formato de outro programa (vira `.aiaibrush` com a origem registrada).
    Third(ImportedBrush, String),
    /// `.aiaibrush`/`.aiaibrushes` (sem perda); `pack` = nome do pacote, se veio de um.
    Native(Box<BrushFile>, Option<String>),
}

/// Situação do pincel nativo frente à biblioteca.
#[derive(Clone, Copy, PartialEq)]
enum Have {
    New,
    /// Já instalado com a mesma versão ou mais nova.
    Same,
    /// A versão do arquivo é mais nova que a instalada.
    Newer,
}

struct ImportItem {
    kind: Kind,
    have: Have,
    /// Versão mais nova que a instalada: substituir (ou manter os dois).
    replace: bool,
    on: bool,
    name: String,
    thumb: Option<egui::TextureHandle>,
    /// A imagem da ponta (as pontas de imagem).
    tip: Option<egui::TextureHandle>,
}

/// Janela "Exportar": o que exportar e o autor/licença a confirmar.
pub(crate) struct ExportDialog {
    target: ExportTarget,
    name: String,
    author: BrushAuthor,
    license: String,
    attribution: String,
    /// Licença desconhecida (pincel de outro programa): só exporta com o aceite de que pode redistribuir.
    needs_accept: bool,
    accept: bool,
    error: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) enum ExportTarget {
    Brush(usize),
    Category(String),
}

/// Abre o criador a partir do pincel `base` da biblioteca (ou do pincel em uso).
pub(crate) fn open_maker(env: &mut Env, base: Option<usize>) {
    let o = &env.ed.opts;
    let idx = base.unwrap_or(o.brush_preset);
    let src = match base {
        Some(i) => env.ed.brushes.get(i).cloned(),
        None => o.brush_custom.clone().or_else(|| env.ed.brushes.get(o.brush_preset).cloned()),
    };
    let mut p = src.unwrap_or_else(|| BrushPreset::round("Pincel", 24.0, 0.9));
    if base.is_none() {
        p.size = o.brush_size;
        p.opacity = o.brush_opacity;
    }
    let lib = &env.ed.brushes;
    let base_user = lib.items.get(idx).is_some_and(|b| b.user);
    let profile = lib.profile_author();
    // pincel derivado: leva a licença e as etiquetas do pincel de origem e diz de quem era; o autor é o do perfil
    let license = if base_user && !p.license.trim().is_empty() { p.license.clone() } else { lib.profile_license() };
    let mut source = p.source.clone();
    if base_user && source.is_empty() && !p.author.name.trim().is_empty() && p.author.name != profile.name {
        source = tr("brushes.based_on").replace("{name}", &p.name).replace("{author}", &p.author.name);
    }
    let tags = p.tags.join(", ");
    let p = env.ed.brushes.with_masks(&p);
    let name = format!("{} 2", p.name);
    let cat = match cat_of(&p).as_str() {
        "importados" => "meus".to_owned(),
        c => c.to_owned(),
    };
    env.ed.brush_panel.maker = Some(Maker { preset: p, name, cat, author: profile, license, source, tags, tip_bytes: None, tex_bytes: None, preview: None, preview_of: None, error: None, cat_ui: Default::default() });
}

/// Escolhe arquivos de pincel e lê o que tiverem.
pub(crate) fn pick_import(env: &mut Env, ctx: &egui::Context) {
    let files = rfd::FileDialog::new()
        .set_title(tr("brushes.import"))
        .add_filter(tr("brushes.import.filter"), &["aiaibrush", "aiaibrushes", "abr", "brush", "brushset", "bundle", "gbr", "gih", "png", "jpg", "jpeg", "webp", "sut"])
        .pick_files()
        .unwrap_or_default();
    import_files(env, ctx, files);
}

/// Lê os arquivos de pincel e abre a janela de importar (a Biblioteca entrega os pincéis que chegam a ela por aqui).
pub(crate) fn import_files(env: &mut Env, ctx: &egui::Context, files: Vec<std::path::PathBuf>) {
    if files.is_empty() {
        return;
    }
    let mut imp = Importer { files: Vec::new(), errors: Vec::new(), cat: "importados".to_owned(), pack: String::new(), license: brush_pack::UNKNOWN_LICENSE.to_owned(), items: Vec::new(), originals: Vec::new() };
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = name.rsplit_once('.').map(|(a, _)| a.to_owned()).unwrap_or_else(|| name.clone());
        let bytes = match std::fs::read(&f) {
            Ok(b) => b,
            Err(e) => {
                imp.errors.push(format!("{name}: {e}"));
                continue;
            }
        };
        let native = name.to_lowercase().ends_with(".aiaibrush") || name.to_lowercase().ends_with(".aiaibrushes");
        if native {
            match brush_pack::read(&bytes) {
                Ok(Package::Brush(b)) => {
                    imp.files.push(format!("{name}: 1 · {}", author_license_line(&b.preset)));
                    imp.items.push(native_item(env, *b, None));
                }
                Ok(Package::Pack(pk)) => {
                    let m = &pk.manifest;
                    let who = if m.author.name.is_empty() { tr("brushes.author_none").to_owned() } else { m.author.name.clone() };
                    imp.files.push(tr("brushes.import.pack_line").replace("{name}", &m.name).replace("{n}", &pk.brushes.len().to_string()).replace("{author}", &who).replace("{license}", &license_with_link(&m.license)));
                    for b in pk.brushes {
                        imp.items.push(native_item(env, b, Some(m.name.clone())));
                    }
                }
                Err(e) => imp.errors.push(format!("{name}: {e}")),
            }
            continue;
        }
        if imp.pack.is_empty() {
            imp.pack = stem.clone();
        }
        match aiai_paint::brush_import::import_brushes(&name, &bytes) {
            Ok(v) => {
                imp.files.push(format!("{name}: {}", v.len()));
                imp.items.extend(v.into_iter().map(|b| ImportItem { name: b.name.clone(), kind: Kind::Third(b, name.clone()), have: Have::New, replace: false, on: true, thumb: None, tip: None }));
                imp.originals.push((name, bytes));
            }
            Err(e) => imp.errors.push(format!("{name}: {e}")),
        }
    }
    // miniaturas pelo motor (a ponta já está na memória)
    let t = env.t;
    let presets: Vec<BrushPreset> = imp.items.iter().map(|it| item_preset(it, "importados")).collect();
    for ((it, th), p) in imp.items.iter_mut().zip(super::properties::brush_thumbnails(ctx, &presets, t.bg_widget, t.text)).zip(&presets) {
        it.thumb = Some(th);
        let mask = match (&it.kind, &p.tip) {
            (Kind::Third(b, _), _) => match &b.tip {
                ImportedTip::Mask(m) => Some(m),
                ImportedTip::Round { .. } => None,
            },
            (_, BrushTip::Image { mask: Some(m), .. }) => Some(m),
            _ => None,
        };
        if let Some(m) = mask {
            // tinta na cor do texto sobre o fundo do cartão
            let px: Vec<egui::Color32> = m.data.iter().map(|&a| t.bg_widget.lerp_to_gamma(t.text, a as f32 / 255.0)).collect();
            let ci = egui::ColorImage::new([m.width as usize, m.height as usize], px);
            it.tip = Some(ctx.load_texture("import-tip", ci, egui::TextureOptions::LINEAR));
        }
    }
    env.ed.brush_panel.importer = Some(imp);
}

/// "Autor · licença" de um pincel (a janela de importar mostra antes de instalar).
fn author_license_line(p: &BrushPreset) -> String {
    let who = if p.author.name.is_empty() { tr("brushes.author_none").to_owned() } else { p.author.name.clone() };
    format!("{who} · {}", p.license)
}

fn license_with_link(l: &brush_pack::License) -> String {
    match l.link() {
        Some(u) => format!("{} ({u})", l.to_text()),
        None => l.to_text(),
    }
}

/// Pincel nativo para a lista de importar, com a situação frente à biblioteca (novo, já instalado, versão mais nova).
fn native_item(env: &Env, b: BrushFile, pack: Option<String>) -> ImportItem {
    let have = match env.ed.brushes.find_id(&b.preset.id) {
        None => Have::New,
        Some(i) => {
            let cur = &env.ed.brushes.items[i].preset.version;
            if brush_pack::version_cmp(&b.preset.version, cur) == std::cmp::Ordering::Greater { Have::Newer } else { Have::Same }
        }
    };
    ImportItem { name: b.preset.name.clone(), kind: Kind::Native(Box::new(b), pack), have, replace: true, on: have != Have::Same, thumb: None, tip: None }
}

/// O preset do item para a miniatura (e, nos de terceiros, o que será salvo).
fn item_preset(it: &ImportItem, cat: &str) -> BrushPreset {
    match &it.kind {
        Kind::Third(b, _) => preset_of(b, &it.name, cat).0,
        Kind::Native(f, _) => f.preset.clone(),
    }
}

/// Pincel importado → preset (com as máscaras na memória) e as máscaras codificadas para a biblioteca.
fn preset_of(b: &ImportedBrush, name: &str, cat: &str) -> (BrushPreset, Option<Vec<u8>>, Option<Vec<u8>>) {
    let mut p = BrushPreset::round(name, b.size.unwrap_or(40.0).clamp(1.0, 600.0), 0.9);
    p.category = cat.to_owned();
    p.source = b.source.clone();
    p.size_pressure_min = 0.3;
    let mut tip_bytes = None;
    match &b.tip {
        ImportedTip::Mask(m) => {
            let bytes = m.encode();
            p.tip = BrushTip::Image { blob: aiai_core::BlobHash::of(&bytes), mask: Some(m.clone()) };
            // carimbo de imagem: 25% do diâmetro (o padrão do Photoshop) quando o arquivo não diz
            p.spacing = 0.25;
            tip_bytes = Some(bytes);
        }
        ImportedTip::Round { hardness, roundness } => p.tip = BrushTip::Round { hardness: *hardness, roundness: *roundness, angle: 0.0 },
    }
    if let Some(s) = b.spacing {
        p.spacing = s.clamp(0.01, 5.0);
    }
    let mut tex_bytes = None;
    if let Some(t) = &b.texture {
        let bytes = t.encode();
        p.texture = Some(texture_image(aiai_core::BlobHash::of(&bytes), t.clone()));
        tex_bytes = Some(bytes);
    }
    (p, tip_bytes, tex_bytes)
}

fn texture_image(hash: aiai_core::BlobHash, m: Mask) -> BrushTexture {
    BrushTexture { kind: TextureKind::Paper, scale: 1.0, depth: 0.6, mode: TextureMode::Multiply, seed: 0, invert: false, rotation: 0.0, image: Some(hash), image_mask: Some(std::sync::Arc::new(m)) }
}

/// Categorias para escolher: as de fábrica e as que os pincéis do usuário já usam.
fn category_choices(env: &Env) -> Vec<String> {
    let mut v: Vec<String> = CATEGORIES.iter().map(|(k, _)| (*k).to_owned()).collect();
    for b in &env.ed.brushes.items {
        let c = cat_of(&b.preset);
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}

fn category_field(ui: &mut egui::Ui, id: &str, cat: &mut String, choices: &[String]) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt((id, "combo")).selected_text(cat_label(cat)).width(150.0).show_ui(ui, |ui| {
            for c in choices {
                ui.selectable_value(cat, c.clone(), cat_label(c));
            }
        });
        ui.add(egui::TextEdit::singleline(cat).hint_text(tr("brushes.new_category")).desired_width(120.0));
    });
}

/// Janelas abertas do criador e do importador (chamado pelo painel a cada quadro).
pub(crate) fn windows(env: &mut Env, ctx: &egui::Context) {
    // mais de um painel pode chamar (Pincéis e Biblioteca no mesmo workspace): as janelas são desenhadas uma vez por passada
    static LAST_PASS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);
    if LAST_PASS.swap(ctx.cumulative_pass_nr(), std::sync::atomic::Ordering::Relaxed) == ctx.cumulative_pass_nr() {
        return;
    }
    maker_window(env, ctx);
    importer_window(env, ctx);
    export_window(env, ctx);
}

fn maker_window(env: &mut Env, ctx: &egui::Context) {
    let Some(mut m) = env.ed.brush_panel.maker.take() else { return };
    let choices = category_choices(env);
    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    // prévia pelo motor quando o preset mudou
    if m.preview_of.as_ref() != Some(&m.preset) {
        let t = env.t;
        let mut q = m.preset.clone();
        q.size = q.size.clamp(2.0, 60.0);
        m.preview = preview_texture(ctx, &q, t.bg_widget, t.text);
        m.preview_of = Some(m.preset.clone());
    }
    egui::Window::new(tr("brushes.maker")).id(egui::Id::new("brush_maker")).open(&mut open).default_width(560.0).resizable(true).collapsible(false).show(ctx, |ui| {
        if let Some(tex) = &m.preview {
            let w = ui.available_width().min(560.0);
            let r = ui.add(egui::Image::new((tex.id(), egui::vec2(w, w * 150.0 / 560.0))));
            env.note("maker:preview", r.rect);
        }
        egui::Grid::new("maker_head").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(tr("brushes.name"));
            let r = ui.add(egui::TextEdit::singleline(&mut m.name).desired_width(260.0));
            env.note("maker:name", r.rect);
            ui.end_row();
            ui.label(tr("brushes.category"));
            category_field(ui, "maker_cat", &mut m.cat, &choices);
            ui.end_row();
            ui.label(tr("brushes.author"));
            let r = ui.add(egui::TextEdit::singleline(&mut m.author.name).desired_width(260.0));
            env.note("maker:author", r.rect);
            ui.end_row();
            ui.label(tr("brushes.author_contact"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut m.author.email).hint_text("e-mail").desired_width(128.0));
                ui.add(egui::TextEdit::singleline(&mut m.author.url).hint_text("https://").desired_width(128.0));
            });
            ui.end_row();
            ui.label(tr("brushes.license"));
            let r = license_field(ui, "maker_license", &mut m.license);
            env.note("maker:license", r);
            ui.end_row();
            ui.label(tr("brushes.source"));
            let r = ui.add(egui::TextEdit::singleline(&mut m.source).hint_text(tr("brushes.source.hint")).desired_width(260.0));
            env.note("maker:source", r.rect);
            ui.end_row();
            ui.label(tr("brushes.tags"));
            let r = ui.add(egui::TextEdit::singleline(&mut m.tags).hint_text(tr("brushes.tags.hint")).desired_width(260.0));
            env.note("maker:tags", r.rect);
            ui.end_row();
            // ponta de imagem
            ui.label(tr("maker.tip_image"));
            ui.horizontal(|ui| {
                let b = ui.button(tr("maker.pick_image"));
                env.note("maker:tip_image", b.rect);
                if b.on_hover_text(tr("maker.pick_image.tip")).clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Imagem", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                        match std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| aiai_paint::brush_import::tip_from_image(&b)) {
                            Ok(mask) => set_tip_image(&mut m, mask),
                            Err(e) => m.error = Some(e),
                        }
                    }
                }
                if let BrushTip::Image { mask: Some(mask), .. } = &m.preset.tip {
                    ui.label(format!("{}×{}", mask.width, mask.height));
                    if ui.button(tr("maker.invert")).clicked() {
                        let mut inv = mask.clone();
                        inv.data.iter_mut().for_each(|v| *v = 255 - *v);
                        set_tip_image(&mut m, inv);
                    }
                }
            });
            ui.end_row();
            // textura
            ui.label(tr("maker.texture"));
            ui.horizontal(|ui| {
                let cur = match &m.preset.texture {
                    None => tr("maker.tex_none").to_owned(),
                    Some(t) if t.image.is_some() => tr("maker.tex_image").to_owned(),
                    Some(t) => tr(tex_key(t.kind)).to_owned(),
                };
                egui::ComboBox::from_id_salt("maker_tex").selected_text(cur).show_ui(ui, |ui| {
                    if ui.selectable_label(m.preset.texture.is_none(), tr("maker.tex_none")).clicked() {
                        m.preset.texture = None;
                        m.tex_bytes = None;
                    }
                    for k in [TextureKind::Paper, TextureKind::Canvas, TextureKind::Noise, TextureKind::Cells, TextureKind::Grain] {
                        let sel = m.preset.texture.as_ref().is_some_and(|t| t.image.is_none() && t.kind == k);
                        if ui.selectable_label(sel, tr(tex_key(k))).clicked() {
                            m.preset.texture = Some(BrushTexture { kind: k, scale: 4.0, depth: 0.6, mode: TextureMode::Multiply, seed: 7, invert: false, rotation: 0.0, image: None, image_mask: None });
                            m.tex_bytes = None;
                        }
                    }
                });
                let b = ui.button(tr("maker.pick_texture"));
                env.note("maker:texture_image", b.rect);
                if b.clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Imagem", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                        match std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| aiai_paint::brush_import::texture_from_image(&b)) {
                            Ok(t) => {
                                let bytes = t.encode();
                                m.preset.texture = Some(texture_image(aiai_core::BlobHash::of(&bytes), t));
                                m.tex_bytes = Some(bytes);
                            }
                            Err(e) => m.error = Some(e),
                        }
                    }
                }
            });
            ui.end_row();
            if let Some(t) = &mut m.preset.texture {
                ui.label(tr("maker.tex_mode"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut t.mode, TextureMode::Multiply, tr("maker.tex_multiply"));
                    ui.selectable_value(&mut t.mode, TextureMode::Subtract, tr("maker.tex_subtract"));
                    ui.checkbox(&mut t.invert, tr("maker.invert"));
                });
                ui.end_row();
            }
        });
        ui.separator();
        // a janela cresceu com autor/licença/fonte/etiquetas: a lista de ajustes encolhe em telas baixas (o Salvar fica à vista)
        let grid_h = (ctx.content_rect().height() - 640.0).clamp(100.0, 300.0);
        // os ajustes do motor nas categorias das configurações avançadas (tamanho e opacidade estão em Tamanho e Tinta)
        {
            let mut e = super::tool_detail::Edit { p: &mut m.preset, o: None, t: env.t, icons: env.icons, notes: env.notes, compact: false, salt: "maker" };
            super::tool_detail::categories_ui(ui, &mut e, &mut m.cat_ui, None, grid_h);
        }
        if let Some(e) = &m.error {
            ui.colored_label(env.t.danger, e);
        }
        ui.separator();
        ui.horizontal(|ui| {
            let ok = ui.add_enabled(!m.name.trim().is_empty() && !m.cat.trim().is_empty(), egui::Button::new(tr("maker.save")));
            env.note("maker:save", ok.rect);
            save = ok.clicked();
            cancel = ui.button(tr("confirm.cancel")).clicked();
        });
    });
    if save {
        match save_maker(env, &m) {
            Ok(()) => return,
            Err(e) => m.error = Some(e),
        }
    }
    if open && !cancel {
        env.ed.brush_panel.maker = Some(m);
    }
}

fn set_tip_image(m: &mut Maker, mask: Mask) {
    let bytes = mask.encode();
    m.preset.tip = BrushTip::Image { blob: aiai_core::BlobHash::of(&bytes), mask: Some(mask) };
    if m.preset.spacing < 0.1 {
        m.preset.spacing = 0.15;
    }
    m.tip_bytes = Some(bytes);
}

fn save_maker(env: &mut Env, m: &Maker) -> Result<(), String> {
    let lib = &mut env.ed.brushes;
    // imagens novas desta sessão do criador; as que vieram do pincel base já estão na biblioteca (ou no projeto)
    for b in [&m.tip_bytes, &m.tex_bytes].into_iter().flatten() {
        lib.put_asset(b.clone())?;
    }
    if let BrushTip::Image { mask: Some(mask), blob } = &m.preset.tip {
        if m.tip_bytes.is_none() && !lib_has(env, blob) {
            env.ed.brushes.put_asset(mask.encode())?;
        }
    }
    let mut p = m.preset.clone();
    p.author = BrushAuthor { name: m.author.name.trim().to_owned(), email: m.author.email.trim().to_owned(), url: m.author.url.trim().to_owned() };
    p.license = m.license.trim().to_owned();
    p.source = m.source.trim().to_owned();
    p.tags = parse_tags(&m.tags);
    p.validate()?;
    let i = env.ed.brushes.add_user(p, &m.name, &m.cat)?;
    // os últimos valores digitados viram o perfil (preferências): valem para os próximos pincéis e exportações
    env.ed.brushes.set_author(BrushAuthorPrefs { name: m.author.name.trim().to_owned(), email: m.author.email.trim().to_owned(), url: m.author.url.trim().to_owned(), license: m.license.trim().to_owned() });
    env.ed.opts.brush_preset = i;
    env.ed.opts.brush_custom = None;
    if let Some(b) = env.ed.brushes.get(i) {
        env.ed.opts.brush_size = b.size;
        env.ed.opts.brush_opacity = b.opacity;
    }
    env.ed.brush_panel.cat = Some(m.cat.trim().to_owned());
    Ok(())
}

/// Etiquetas separadas por vírgula (sem repetir, sem vazias).
fn parse_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for t in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !v.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            v.push(t.chars().take(64).collect());
        }
    }
    v
}

/// Licença: escolher uma conhecida (SPDX, a padrão, a "desconhecida") ou escrever o texto. Devolve o retângulo (nota).
fn license_field(ui: &mut egui::Ui, id: &str, license: &mut String) -> egui::Rect {
    let mut rect = ui.min_rect();
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(license).desired_width(170.0).hint_text(brush_pack::DEFAULT_LICENSE));
        rect = r.rect;
        egui::ComboBox::from_id_salt((id, "pick")).selected_text(tr("brushes.license.pick")).width(110.0).show_ui(ui, |ui| {
            if ui.selectable_label(false, tr("license.proprietary")).clicked() {
                *license = brush_pack::DEFAULT_LICENSE.to_owned();
            }
            for k in brush_pack::KNOWN_SPDX {
                if ui.selectable_label(license.trim().eq_ignore_ascii_case(k), k).clicked() {
                    *license = k.to_owned();
                }
            }
            if ui.selectable_label(false, tr("license.unknown")).clicked() {
                *license = brush_pack::UNKNOWN_LICENSE.to_owned();
            }
        });
    })
    .response
    .on_hover_text(tr("brushes.license.tip"));
    rect
}

fn lib_has(env: &Env, h: &aiai_core::BlobHash) -> bool {
    env.ed.brushes.has_asset(h)
}

fn importer_window(env: &mut Env, ctx: &egui::Context) {
    let Some(mut imp) = env.ed.brush_panel.importer.take() else { return };
    let choices = category_choices(env);
    let mut open = true;
    let mut go = false;
    let mut cancel = false;
    let third = imp.items.iter().any(|i| matches!(i.kind, Kind::Third(..)));
    egui::Window::new(tr("brushes.import")).id(egui::Id::new("brush_import")).open(&mut open).default_width(560.0).resizable(true).collapsible(false).show(ctx, |ui| {
        for f in &imp.files {
            ui.label(f);
        }
        for e in &imp.errors {
            ui.colored_label(env.t.danger, e);
        }
        if third {
            ui.label(egui::RichText::new(tr("brushes.import.license")).size(11.5).color(env.t.text_dim));
            egui::Grid::new("import_third").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(tr("brushes.category"));
                category_field(ui, "import_cat", &mut imp.cat, &choices);
                ui.end_row();
                ui.label(tr("brushes.import.pack"));
                let r = ui.add(egui::TextEdit::singleline(&mut imp.pack).desired_width(270.0));
                env.note("import:pack", r.rect);
                ui.end_row();
                ui.label(tr("brushes.import.license3"));
                let r = license_field(ui, "import_license", &mut imp.license);
                env.note("import:license", r);
                ui.end_row();
            });
        }
        ui.horizontal(|ui| {
            if ui.button(tr("brushes.import.all")).clicked() {
                imp.items.iter_mut().for_each(|i| i.on = true);
            }
            if ui.button(tr("brushes.import.none")).clicked() {
                imp.items.iter_mut().for_each(|i| i.on = false);
            }
        });
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (k, it) in imp.items.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let r = ui.checkbox(&mut it.on, "");
                    env.note(format!("import:item:{k}"), r.rect);
                    let side = 28.0;
                    match &it.tip {
                        Some(t) => {
                            let k = side / t.size_vec2().max_elem().max(1.0);
                            let (r, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                            ui.painter().image(t.id(), egui::Rect::from_center_size(r.center(), t.size_vec2() * k), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                        None => {
                            ui.add_space(side + ui.spacing().item_spacing.x);
                        }
                    }
                    if let Some(t) = &it.thumb {
                        ui.add(egui::Image::new((t.id(), egui::vec2(120.0, 24.0))));
                    }
                    ui.add(egui::TextEdit::singleline(&mut it.name).desired_width(170.0));
                    match &it.kind {
                        Kind::Third(b, _) => {
                            let detail = match &b.tip {
                                ImportedTip::Mask(m) => format!("{}×{}", m.width, m.height),
                                ImportedTip::Round { .. } => tr("tipshape.round").to_owned(),
                            };
                            let tex = if b.texture.is_some() { format!(" · {}", tr("maker.texture")) } else { String::new() };
                            ui.label(egui::RichText::new(format!("{detail}{tex}")).size(11.0).color(env.t.text_dim)).on_hover_text(&b.source);
                        }
                        Kind::Native(f, _) => {
                            let line = format!("{} · v{}", author_license_line(&f.preset), f.preset.version);
                            ui.label(egui::RichText::new(line).size(11.0).color(env.t.text_dim)).on_hover_text(license_with_link(&f.manifest.license));
                        }
                    }
                });
                // pincel nativo: a situação frente à biblioteca
                if matches!(it.kind, Kind::Native(..)) && it.have != Have::New {
                    ui.horizontal(|ui| {
                        ui.add_space(30.0);
                        if it.have == Have::Same {
                            ui.label(egui::RichText::new(tr("brushes.import.state_same")).size(11.0).color(env.t.text_dim));
                        } else {
                            ui.label(egui::RichText::new(tr("brushes.import.state_newer")).size(11.0).color(env.t.text_dim));
                            ui.selectable_value(&mut it.replace, true, tr("brushes.import.replace"));
                            ui.selectable_value(&mut it.replace, false, tr("brushes.import.keep_both"));
                        }
                    });
                }
            }
        });
        ui.separator();
        let n = imp.items.iter().filter(|i| i.on).count();
        ui.horizontal(|ui| {
            let ok = n > 0 && (!third || !imp.cat.trim().is_empty());
            let b = ui.add_enabled(ok, egui::Button::new(format!("{} ({n})", tr("brushes.import.go"))));
            env.note("import:go", b.rect);
            go = b.clicked();
            cancel = ui.button(tr("confirm.cancel")).clicked();
        });
    });
    if go {
        run_import(env, &imp);
        return;
    }
    if open && !cancel {
        env.ed.brush_panel.importer = Some(imp);
    }
}

/// Instala os pincéis marcados: os de terceiros viram `.aiaibrush` em `<categoria>\<pacote>` com a origem e a licença
/// escolhida (o arquivo original fica em `_Originais`); os nativos entram como estão (autor, licença, origem e id).
fn run_import(env: &mut Env, imp: &Importer) {
    let cat = if imp.cat.trim().is_empty() { "importados".to_owned() } else { imp.cat.trim().to_owned() };
    let pack = if imp.pack.trim().is_empty() { "Importados".to_owned() } else { imp.pack.trim().to_owned() };
    let license = if imp.license.trim().is_empty() { brush_pack::UNKNOWN_LICENSE.to_owned() } else { imp.license.trim().to_owned() };
    let (mut done, mut skipped, mut last) = (0usize, 0usize, None);
    let mut kept: std::collections::HashSet<String> = Default::default();
    for it in imp.items.iter().filter(|i| i.on) {
        let r: Result<usize, String> = match &it.kind {
            Kind::Third(b, file) => (|| {
                let (mut p, tip, tex) = preset_of(b, &it.name, &cat);
                for bytes in [tip, tex].into_iter().flatten() {
                    env.ed.brushes.put_asset(bytes)?;
                }
                p.license = license.clone();
                p.tags = vec![pack.clone()];
                let lib = &env.ed.brushes;
                let mut f = BrushFile::from_preset(&p, None, &|h| lib.asset_bytes(h))?;
                f.manifest.source.imported_from = imported_from_kind(&b.source).to_owned();
                if kept.insert(file.clone()) {
                    if let Some((n, bytes)) = imp.originals.iter().find(|(n, _)| n == file) {
                        if let Err(e) = env.ed.brushes.keep_original(n, bytes) {
                            tracing::warn!("original do pincel não guardado: {e}");
                        }
                    }
                }
                env.ed.brushes.install_file(f, Some(&pack))
            })(),
            Kind::Native(f, from_pack) => {
                let mut f = (**f).clone();
                match (it.have, env.ed.brushes.find_id(&f.preset.id)) {
                    (Have::Same, _) => {
                        skipped += 1;
                        continue;
                    }
                    (Have::Newer, Some(i)) if it.replace => {
                        if let Err(e) = env.ed.brushes.remove_user(i) {
                            env.ed.notify(format!("{}: {e}", it.name));
                            continue;
                        }
                    }
                    (Have::Newer, Some(_)) => {
                        // manter os dois: a cópia importada ganha id próprio
                        f.preset.id = brush_pack::new_id();
                        f.manifest.id = f.preset.id.clone();
                    }
                    _ => {}
                }
                f.preset.name = it.name.trim().to_owned();
                env.ed.brushes.install_file(f, from_pack.as_deref())
            }
        };
        match r {
            Ok(i) => {
                done += 1;
                last = Some(i);
            }
            Err(e) => env.ed.notify(format!("{}: {e}", it.name)),
        }
    }
    if let Some(i) = last {
        env.ed.brush_panel.cat = Some(env.ed.brushes.get(i).map(cat_of).unwrap_or(cat));
        env.ed.opts.brush_preset = i;
        env.ed.opts.brush_custom = None;
        if let Some(b) = env.ed.brushes.get(i) {
            env.ed.opts.brush_size = b.size;
            env.ed.opts.brush_opacity = b.opacity;
        }
    }
    env.ed.notify(tr("brushes.import.done").replace("{n}", &done.to_string()).replace("{m}", &skipped.to_string()));
}

// ------------------------------------------------------------------------------------------------ exportar

/// Abre a janela de exportar um pincel ou uma categoria; autor e licença vêm do pincel (ou do perfil do usuário).
pub(crate) fn open_export(env: &mut Env, target: ExportTarget) {
    let lib = &env.ed.brushes;
    let (name, author, license, attribution, unknown) = match &target {
        ExportTarget::Brush(i) => {
            let b = lib.items.get(*i);
            let p = b.map(|b| &b.preset);
            // pincel de outro programa sem autor registrado: não vira autoria do usuário
            let imported = b.and_then(|b| b.manifest.as_ref()).is_some_and(|m| !m.source.imported_from.is_empty());
            let author = p.map(|p| p.author.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| if imported { BrushAuthor::default() } else { lib.profile_author() });
            let license = p.map(|p| p.license.clone()).filter(|l| !l.trim().is_empty()).unwrap_or_else(|| lib.profile_license());
            let attr = b.and_then(|b| b.manifest.as_ref()).and_then(|m| m.license.attribution.clone()).unwrap_or_default();
            (p.map(|p| p.name.clone()).unwrap_or_default(), author, license, attr, lib.license_unknown(*i))
        }
        ExportTarget::Category(c) => {
            let unknown = lib.items.iter().enumerate().any(|(i, b)| b.user && cat_of(&b.preset) == *c && lib.license_unknown(i));
            (cat_label(c), lib.profile_author(), lib.profile_license(), String::new(), unknown)
        }
    };
    env.ed.brush_panel.export = Some(ExportDialog { target, name, author, license, attribution, needs_accept: unknown, accept: false, error: None });
}

fn export_window(env: &mut Env, ctx: &egui::Context) {
    let Some(mut d) = env.ed.brush_panel.export.take() else { return };
    let mut open = true;
    let mut go = false;
    let mut cancel = false;
    let pack = matches!(d.target, ExportTarget::Category(_));
    egui::Window::new(tr(if pack { "brushes.export_cat.title" } else { "brushes.export.title" })).id(egui::Id::new("brush_export")).open(&mut open).default_width(480.0).resizable(false).collapsible(false).show(ctx, |ui| {
        ui.label(egui::RichText::new(tr("brushes.export.hint")).size(11.5).color(env.t.text_dim));
        egui::Grid::new("export_grid").num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
            ui.label(tr(if pack { "brushes.export.pack_name" } else { "brushes.name" }));
            let r = ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(280.0));
            env.note("export:name", r.rect);
            ui.end_row();
            ui.label(tr("brushes.author"));
            let r = ui.add(egui::TextEdit::singleline(&mut d.author.name).desired_width(280.0));
            env.note("export:author", r.rect);
            ui.end_row();
            ui.label(tr("brushes.author_contact"));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut d.author.email).hint_text("e-mail").desired_width(138.0));
                ui.add(egui::TextEdit::singleline(&mut d.author.url).hint_text("https://").desired_width(138.0));
            });
            ui.end_row();
            ui.label(tr("brushes.license"));
            let r = license_field(ui, "export_license", &mut d.license);
            env.note("export:license", r);
            ui.end_row();
            ui.label(tr("brushes.export.attribution"));
            ui.add(egui::TextEdit::singleline(&mut d.attribution).desired_width(280.0));
            ui.end_row();
        });
        if d.needs_accept {
            ui.add_space(4.0);
            ui.colored_label(env.t.danger, tr("brushes.export.unknown_license"));
            let r = ui.checkbox(&mut d.accept, tr("brushes.export.accept"));
            env.note("export:accept", r.rect);
        }
        if let Some(e) = &d.error {
            ui.colored_label(env.t.danger, e);
        }
        ui.separator();
        ui.horizontal(|ui| {
            let ready = !d.name.trim().is_empty() && !d.license.trim().is_empty() && (!d.needs_accept || d.accept);
            let b = ui.add_enabled(ready, egui::Button::new(tr("brushes.export.go")));
            env.note("export:go", b.rect);
            go = b.clicked();
            cancel = ui.button(tr("confirm.cancel")).clicked();
        });
    });
    if go {
        match run_export(env, &d) {
            Ok(true) => return,
            Ok(false) => {} // o usuário desistiu do diálogo de salvar: a janela continua
            Err(e) => d.error = Some(e),
        }
    }
    if open && !cancel {
        env.ed.brush_panel.export = Some(d);
    }
}

/// Pergunta onde salvar e grava. `Ok(false)` = o usuário cancelou o diálogo do sistema.
fn run_export(env: &mut Env, d: &ExportDialog) -> Result<bool, String> {
    let meta = ExportMeta { author: BrushAuthor { name: d.author.name.trim().to_owned(), email: d.author.email.trim().to_owned(), url: d.author.url.trim().to_owned() }, license: d.license.trim().to_owned(), attribution: d.attribution.trim().to_owned() };
    let (bytes, ext, n) = match &d.target {
        ExportTarget::Brush(i) => (env.ed.brushes.export_brush(*i, &meta)?, brush_pack::EXT_BRUSH, 1),
        ExportTarget::Category(c) => {
            let (b, n) = env.ed.brushes.export_category(c, d.name.trim(), &meta)?;
            (b, brush_pack::EXT_PACK, n)
        }
    };
    let Some(path) = rfd::FileDialog::new().set_title(tr("brushes.export.go")).set_file_name(format!("{}.{ext}", sanitize(&d.name))).add_filter(ext, &[ext]).save_file() else { return Ok(false) };
    let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) { path } else { path.with_extension(ext) };
    aiai_core::persist::write_file_atomic(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    // os valores confirmados viram o perfil (preferências) para a próxima exportação
    env.ed.brushes.set_author(BrushAuthorPrefs { name: meta.author.name.clone(), email: meta.author.email.clone(), url: meta.author.url.clone(), license: meta.license.clone() });
    env.ed.notify(tr("brushes.export.done").replace("{path}", &path.display().to_string()).replace("{n}", &n.to_string()));
    Ok(true)
}

fn tex_key(k: TextureKind) -> &'static str {
    match k {
        TextureKind::Paper => "maker.tex_paper",
        TextureKind::Canvas => "maker.tex_canvas",
        TextureKind::Noise => "maker.tex_noise",
        TextureKind::Cells => "maker.tex_cells",
        TextureKind::Grain => "maker.tex_grain",
    }
}

/// Prévia do criador: um S com pressão subindo e descendo, pintado pelo motor.
fn preview_texture(ctx: &egui::Context, p: &BrushPreset, bg: egui::Color32, ink: egui::Color32) -> Option<egui::TextureHandle> {
    use aiai_paint::brush::{BrushPoint, paint_stroke};
    let (w, h) = (560u32, 150u32);
    let c = |c: egui::Color32| [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0];
    let mut img = aiai_paint::Image::filled(w, h, c(bg));
    let path: Vec<BrushPoint> = (0..120)
        .map(|k| {
            let t = k as f32 / 119.0;
            BrushPoint { x: 30.0 + t * (w as f32 - 60.0), y: h as f32 / 2.0 + 38.0 * (t * std::f32::consts::TAU).sin(), pressure: 0.15 + 0.85 * (t * std::f32::consts::PI).sin(), tilt: None }
        })
        .collect();
    paint_stroke(&mut img, p, &path, c(ink), 11);
    let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img.to_rgba8());
    Some(ctx.load_texture("brush-maker-preview", ci, egui::TextureOptions::LINEAR))
}
