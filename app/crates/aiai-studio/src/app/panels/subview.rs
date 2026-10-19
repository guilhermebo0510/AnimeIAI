//! Sub View (visualizador de referência): imagens do disco abertas ao lado do canvas — foto de referência, settei,
//! paleta de cores de uma cena — sem entrar no projeto. Anterior/próxima entre as abertas e conta-gotas: clicar na
//! imagem pega a cor dela como cor direta.

use crate::app::ui_kit::{Env, body_ui, dim_label};
use crate::i18n::tr;
use egui::{Rect, Sense, Ui};
use std::path::PathBuf;

#[derive(Default)]
pub(crate) struct SubViewState {
    images: Vec<PathBuf>,
    current: usize,
    /// Imagem carregada: caminho, pixels RGBA8 (para o conta-gotas) e textura.
    loaded: Option<(PathBuf, [usize; 2], Vec<u8>, egui::TextureHandle)>,
    load_job: Option<crossbeam_channel::Receiver<(PathBuf, Result<([usize; 2], Vec<u8>), String>)>>,
    pick_job: Option<crossbeam_channel::Receiver<Vec<PathBuf>>>,
    pub(crate) error: Option<String>,
}

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let ctx = ui.ctx().clone();
    let st = &mut env.ed.subview;
    // diálogo de abrir (várias imagens) e carga em segundo plano
    if let Some(rx) = &st.pick_job {
        match rx.try_recv() {
            Ok(files) => {
                st.pick_job = None;
                if !files.is_empty() {
                    st.current = st.images.len();
                    st.images.extend(files);
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(_) => st.pick_job = None,
        }
    }
    let want = st.images.get(st.current).cloned();
    if let Some(rx) = &st.load_job {
        if let Ok((path, res)) = rx.try_recv() {
            st.load_job = None;
            match res {
                Ok((size, rgba)) => {
                    let ci = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let tex = ctx.load_texture("subview", ci, egui::TextureOptions::LINEAR);
                    st.loaded = Some((path, size, rgba, tex));
                    st.error = None;
                }
                Err(e) => st.error = Some(e),
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    if let Some(p) = &want {
        if st.loaded.as_ref().is_none_or(|l| &l.0 != p) && st.load_job.is_none() {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let path = p.clone();
            let _ = std::thread::Builder::new().name("aiai-subview".into()).spawn(move || {
                let r = image::open(&path).map(|i| i.to_rgba8()).map(|i| ([i.width() as usize, i.height() as usize], i.into_raw())).map_err(|e| e.to_string());
                let _ = tx.send((path, r));
            });
            st.load_job = Some(rx);
        }
    }
    // barra
    let n = st.images.len();
    let mut open = false;
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(st.pick_job.is_none(), egui::Button::new(tr("subview.open")));
        env.notes.push(("subview:open".into(), b.rect));
        open = b.clicked();
        if ui.add_enabled(n > 1, egui::Button::new("◀")).clicked() {
            st.current = (st.current + n - 1) % n.max(1);
        }
        if ui.add_enabled(n > 1, egui::Button::new("▶")).clicked() {
            st.current = (st.current + 1) % n.max(1);
        }
        if let Some(p) = &want {
            let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            ui.add(egui::Label::new(egui::RichText::new(format!("{name}  ({}/{n})", st.current + 1)).size(11.5).color(t.text_dim)).truncate());
        }
    });
    if open {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let _ = std::thread::Builder::new().name("aiai-subview-pick".into()).spawn(move || {
            let files = rfd::FileDialog::new().add_filter("Imagem", &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"]).pick_files().unwrap_or_default();
            let _ = tx.send(files);
        });
        st.pick_job = Some(rx);
    }
    if let Some(e) = &st.error {
        dim_label(ui, &t, e);
    }
    let Some((_, size, rgba, tex)) = &st.loaded else {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr("panel.subview.empty")).color(t.text_dim));
        return;
    };
    // imagem ajustada ao painel; clique = conta-gotas
    let avail = ui.available_rect_before_wrap();
    let s = (avail.width() / size[0] as f32).min((avail.height() - 18.0).max(10.0) / size[1] as f32);
    let r = Rect::from_min_size(avail.min, egui::vec2(size[0] as f32 * s, size[1] as f32 * s));
    let resp = ui.allocate_rect(r, Sense::click_and_drag());
    ui.painter().image(tex.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    env.notes.push(("subview:image".into(), r));
    if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.clicked() || resp.dragged()) {
        let (x, y) = (((p.x - r.left()) / s) as usize, ((p.y - r.top()) / s) as usize);
        if x < size[0] && y < size[1] {
            let i = (y * size[0] + x) * 4;
            let c = aiai_core::Rgba([rgba[i] as f32 / 255.0, rgba[i + 1] as f32 / 255.0, rgba[i + 2] as f32 / 255.0, 1.0]);
            env.ed.set_color_direct(c);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::Crosshair).on_hover_text(tr("subview.pick"));
}
