//! Criar pincel e importar pincéis (painel Ferramenta, pincel).
//!
//! - Criar: parte de um pincel da biblioteca; ponta redonda, de forma ou de imagem (PNG/JPG escolhido pelo usuário),
//!   textura procedural ou de imagem, todos os ajustes do motor e uma prévia desenhada pelo próprio motor a cada
//!   mudança. Salva na biblioteca do usuário, na categoria escolhida (ou numa nova).
//! - Importar: lê pacotes de outros programas (`aiai_paint::brush_import`: GIMP, Photoshop .abr, Procreate, Krita,
//!   imagens), mostra o que achou com a miniatura do motor e importa os marcados numa categoria. As imagens das
//!   pontas e texturas ficam na biblioteca do usuário e entram no projeto quando o pincel é usado.

use crate::app::brushes::{CATEGORIES, cat_label, cat_of};
use crate::app::ui_kit::Env;
use crate::i18n::tr;
use aiai_paint::brush::{BrushPreset, BrushTexture, BrushTip, TextureKind, TextureMode};
use aiai_paint::brush_import::{ImportedBrush, ImportedTip};
use aiai_paint::mask::Mask;

pub(crate) struct Maker {
    pub preset: BrushPreset,
    pub name: String,
    pub cat: String,
    /// Máscaras ("AIM1") da ponta e da textura de imagem escolhidas aqui (vão para a biblioteca ao salvar).
    tip_bytes: Option<Vec<u8>>,
    tex_bytes: Option<Vec<u8>>,
    preview: Option<egui::TextureHandle>,
    preview_of: Option<BrushPreset>,
    error: Option<String>,
}

pub(crate) struct Importer {
    files: Vec<String>,
    errors: Vec<String>,
    cat: String,
    items: Vec<ImportItem>,
}

struct ImportItem {
    brush: ImportedBrush,
    on: bool,
    name: String,
    thumb: Option<egui::TextureHandle>,
    /// A imagem da ponta (as pontas de imagem).
    tip: Option<egui::TextureHandle>,
}

/// Abre o criador a partir do pincel `base` da biblioteca (ou do pincel em uso).
pub(crate) fn open_maker(env: &mut Env, base: Option<usize>) {
    let o = &env.ed.opts;
    let src = match base {
        Some(i) => env.ed.brushes.get(i).cloned(),
        None => o.brush_custom.clone().or_else(|| env.ed.brushes.get(o.brush_preset).cloned()),
    };
    let mut p = src.unwrap_or_else(|| BrushPreset::round("Pincel", 24.0, 0.9));
    if base.is_none() {
        p.size = o.brush_size;
        p.opacity = o.brush_opacity;
    }
    let p = env.ed.brushes.with_masks(&p);
    let name = format!("{} 2", p.name);
    let cat = match cat_of(&p).as_str() {
        "importados" => "meus".to_owned(),
        c => c.to_owned(),
    };
    env.ed.brush_panel.maker = Some(Maker { preset: p, name, cat, tip_bytes: None, tex_bytes: None, preview: None, preview_of: None, error: None });
}

/// Escolhe arquivos de pincel e lê o que tiverem.
pub(crate) fn pick_import(env: &mut Env, ctx: &egui::Context) {
    let files = rfd::FileDialog::new()
        .set_title(tr("brushes.import"))
        .add_filter(tr("brushes.import.filter"), &["abr", "brush", "brushset", "bundle", "gbr", "gih", "png", "jpg", "jpeg", "webp", "sut"])
        .pick_files()
        .unwrap_or_default();
    if files.is_empty() {
        return;
    }
    let mut imp = Importer { files: Vec::new(), errors: Vec::new(), cat: String::new(), items: Vec::new() };
    for f in files {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if imp.cat.is_empty() {
            imp.cat = name.rsplit_once('.').map(|(a, _)| a.to_owned()).unwrap_or_else(|| name.clone());
        }
        match std::fs::read(&f).map_err(|e| e.to_string()).and_then(|b| aiai_paint::brush_import::import_brushes(&name, &b)) {
            Ok(v) => {
                imp.files.push(format!("{name}: {}", v.len()));
                imp.items.extend(v.into_iter().map(|b| ImportItem { name: b.name.clone(), brush: b, on: true, thumb: None, tip: None }));
            }
            Err(e) => imp.errors.push(format!("{name}: {e}")),
        }
    }
    // miniaturas pelo motor (a ponta já está na memória)
    let t = env.t;
    let presets: Vec<BrushPreset> = imp.items.iter().map(|it| preset_of(&it.brush, &it.name, "importados").0).collect();
    for (it, th) in imp.items.iter_mut().zip(super::properties::brush_thumbnails(ctx, &presets, t.bg_widget, t.text)) {
        it.thumb = Some(th);
        if let ImportedTip::Mask(m) = &it.brush.tip {
            // tinta na cor do texto sobre o fundo do cartão
            let px: Vec<egui::Color32> = m.data.iter().map(|&a| t.bg_widget.lerp_to_gamma(t.text, a as f32 / 255.0)).collect();
            let ci = egui::ColorImage::new([m.width as usize, m.height as usize], px);
            it.tip = Some(ctx.load_texture("import-tip", ci, egui::TextureOptions::LINEAR));
        }
    }
    env.ed.brush_panel.importer = Some(imp);
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
    BrushTexture { kind: TextureKind::Paper, scale: 1.0, depth: 0.6, mode: TextureMode::Multiply, seed: 0, invert: false, image: Some(hash), image_mask: Some(std::sync::Arc::new(m)) }
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
    maker_window(env, ctx);
    importer_window(env, ctx);
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
            ui.label(tr("brush.size"));
            ui.add(egui::Slider::new(&mut m.preset.size, 1.0..=600.0).logarithmic(true).max_decimals(1));
            ui.end_row();
            ui.label(tr("brush.opacity"));
            ui.add(egui::Slider::new(&mut m.preset.opacity, 0.0..=1.0).max_decimals(2));
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
                            m.preset.texture = Some(BrushTexture { kind: k, scale: 4.0, depth: 0.6, mode: TextureMode::Multiply, seed: 7, invert: false, image: None, image_mask: None });
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
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            super::properties::preset_grid(ui, &mut m.preset, true, None, "maker");
        });
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
    let p = m.preset.clone();
    p.validate()?;
    let i = env.ed.brushes.add_user(p, &m.name, &m.cat)?;
    env.ed.opts.brush_preset = i;
    env.ed.opts.brush_custom = None;
    if let Some(b) = env.ed.brushes.get(i) {
        env.ed.opts.brush_size = b.size;
        env.ed.opts.brush_opacity = b.opacity;
    }
    env.ed.brush_panel.cat = Some(m.cat.trim().to_owned());
    Ok(())
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
    egui::Window::new(tr("brushes.import")).id(egui::Id::new("brush_import")).open(&mut open).default_width(520.0).resizable(true).collapsible(false).show(ctx, |ui| {
        for f in &imp.files {
            ui.label(f);
        }
        for e in &imp.errors {
            ui.colored_label(env.t.danger, e);
        }
        ui.label(egui::RichText::new(tr("brushes.import.license")).size(11.5).color(env.t.text_dim));
        ui.horizontal(|ui| {
            ui.label(tr("brushes.category"));
            category_field(ui, "import_cat", &mut imp.cat, &choices);
        });
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
                    ui.add(egui::TextEdit::singleline(&mut it.name).desired_width(180.0));
                    let detail = match &it.brush.tip {
                        ImportedTip::Mask(m) => format!("{}×{}", m.width, m.height),
                        ImportedTip::Round { .. } => tr("tipshape.round").to_owned(),
                    };
                    let tex = if it.brush.texture.is_some() { format!(" · {}", tr("maker.texture")) } else { String::new() };
                    ui.label(egui::RichText::new(format!("{detail}{tex}")).size(11.0).color(env.t.text_dim)).on_hover_text(&it.brush.source);
                });
            }
        });
        ui.separator();
        let n = imp.items.iter().filter(|i| i.on).count();
        ui.horizontal(|ui| {
            let b = ui.add_enabled(n > 0 && !imp.cat.trim().is_empty(), egui::Button::new(format!("{} ({n})", tr("brushes.import.go"))));
            env.note("import:go", b.rect);
            go = b.clicked();
            cancel = ui.button(tr("confirm.cancel")).clicked();
        });
    });
    if go {
        let cat = imp.cat.trim().to_owned();
        let mut last = None;
        for it in imp.items.iter().filter(|i| i.on) {
            let (p, tip, tex) = preset_of(&it.brush, &it.name, &cat);
            let r = (|| -> Result<usize, String> {
                for b in [tip, tex].into_iter().flatten() {
                    env.ed.brushes.put_asset(b)?;
                }
                env.ed.brushes.add_user(p, &it.name, &cat)
            })();
            match r {
                Ok(i) => last = Some(i),
                Err(e) => env.ed.notify(format!("{}: {e}", it.name)),
            }
        }
        if let Some(i) = last {
            env.ed.brush_panel.cat = Some(cat);
            env.ed.opts.brush_preset = i;
            env.ed.opts.brush_custom = None;
            if let Some(b) = env.ed.brushes.get(i) {
                env.ed.opts.brush_size = b.size;
                env.ed.opts.brush_opacity = b.opacity;
            }
        }
        return;
    }
    if open && !cancel {
        env.ed.brush_panel.importer = Some(imp);
    }
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
            BrushPoint { x: 30.0 + t * (w as f32 - 60.0), y: h as f32 / 2.0 + 38.0 * (t * std::f32::consts::TAU).sin(), pressure: 0.15 + 0.85 * (t * std::f32::consts::PI).sin() }
        })
        .collect();
    paint_stroke(&mut img, p, &path, c(ink), 11);
    let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img.to_rgba8());
    Some(ctx.load_texture("brush-maker-preview", ci, egui::TextureOptions::LINEAR))
}
