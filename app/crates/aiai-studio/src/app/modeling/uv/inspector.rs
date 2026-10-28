//! Seção "UV" do Inspector: costuras, unwrap, empacotamento, projeções e relatório de qualidade.

use super::{UvAction, UvParams};
use super::{l, UvHost};
use aiai_3d_edit::uv::metrics::UvReport;
use aiai_3d_edit::uv::unwrap::UnwrapMethod;
use egui::{Color32, RichText};

const TEX_SIZES: [u32; 5] = [256, 512, 1024, 2048, 4096];
const WARN: Color32 = Color32::from_rgb(0xff, 0x6a, 0x5a);
const OK: Color32 = Color32::from_rgb(0x6c, 0xd0, 0x8a);

fn sub(ui: &mut egui::Ui, title: &str, open: bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).color(theme::TEXT).size(13.0)).default_open(open).show(ui, add);
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f32, speed: f64, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix).fixed_decimals(1));
    });
}

impl UvHost<'_> {
    pub(super) fn inspector_uv_impl(&mut self, ui: &mut egui::Ui) {
        self.uv_refresh_cache();
        egui::CollapsingHeader::new(RichText::new("UV").strong())
            .id_salt("mdl_uv_section")
            .default_open(true)
            .show(ui, |ui| {
                let mut act: Option<UvAction> = None;
                let p: &mut UvParams = &mut self.uv.params;

                ui.toggle_value(&mut self.uv.show_editor, l("Editor de UV", "UV editor"))
                    .on_hover_text(l("Mostra/oculta o editor 2D de UV", "Shows/hides the 2D UV editor"));
                ui.horizontal_wrapped(|ui| {
                    ui.label(l("Textura", "Texture"));
                    egui::ComboBox::from_id_salt("uv_tex_size")
                        .selected_text(format!("{} px", p.tex_size))
                        .width(80.0)
                        .show_ui(ui, |ui| {
                            for s in TEX_SIZES {
                                ui.selectable_value(&mut p.tex_size, s, format!("{s} px"));
                            }
                        });
                });

                sub(ui, l("Costuras", "Seams"), true, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(l("Marcar", "Mark")).on_hover_text(l("Marca as arestas escolhidas como costura", "Marks the selected edges as seams")).clicked() {
                            act = Some(UvAction::MarkSeam);
                        }
                        if ui.button(l("Limpar", "Clear")).on_hover_text(l("Tira a costura das arestas escolhidas", "Clears the seam on the selected edges")).clicked() {
                            act = Some(UvAction::ClearSeam);
                        }
                        if ui.button(l("Limpar todas", "Clear all")).clicked() {
                            act = Some(UvAction::ClearAllSeams);
                        }
                    });
                    if ui.button(l("Caminho mais curto", "Shortest path")).on_hover_text(l("Costura entre 2 vértices escolhidos", "Seam between 2 selected vertices")).clicked() {
                        act = Some(UvAction::SeamPath);
                    }
                    drag(ui, l("Ângulo", "Angle"), &mut p.seam_angle_deg, 0.5, 1.0..=179.0, "°");
                    if ui.button(l("Costuras por ângulo", "Seams by angle")).on_hover_text(l("Costura em toda aresta com ângulo diedral maior que o limite", "Seam on every edge whose dihedral angle is above the limit")).clicked() {
                        act = Some(UvAction::SeamsByAngle);
                    }
                    ui.separator();
                    drag(ui, l("Ângulo (personagem)", "Angle (character)"), &mut p.char_angle_deg, 0.5, 1.0..=179.0, "°");
                    if ui
                        .button(l("Automático para personagem", "Automatic for characters"))
                        .on_hover_text(l("Ângulo diedral + cortes para abrir ilhas fechadas + pontas", "Dihedral angle + cuts to open closed islands + tips"))
                        .clicked()
                    {
                        act = Some(UvAction::AutoSeamsCharacter);
                    }
                    if ui.button(l("Das arestas duras", "From sharp edges")).clicked() {
                        act = Some(UvAction::SeamsFromSharp);
                    }
                });

                sub(ui, "Unwrap", true, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut p.method, UnwrapMethod::Lscm, "LSCM");
                        ui.selectable_value(&mut p.method, UnwrapMethod::LscmArap, "LSCM + ARAP");
                    });
                    drag(ui, l("Margem", "Margin"), &mut p.margin_px, 0.1, 0.0..=64.0, " px");
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut p.correct_aspect, l("Corrigir proporção", "Correct aspect"));
                        ui.add_enabled(
                            p.correct_aspect,
                            egui::DragValue::new(&mut p.aspect).speed(0.01).range(0.1..=10.0).fixed_decimals(2).prefix("L/A "),
                        );
                    });
                    ui.checkbox(&mut p.fill_holes, l("Preencher buracos", "Fill holes"));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(l("Desdobrar", "Unwrap")).on_hover_text(l("Desdobra as ilhas definidas pelas costuras (faces escolhidas, ou tudo)", "Unwraps the islands defined by the seams (selected faces, or everything)")).clicked() {
                            act = Some(UvAction::Unwrap);
                        }
                        ui.add(egui::DragValue::new(&mut p.stretch_iters).range(1..=500).suffix(" it."));
                        if ui.button(l("Minimizar estiramento", "Minimize stretch")).clicked() {
                            act = Some(UvAction::MinimizeStretch);
                        }
                    });
                });

                sub(ui, "Empacotar", true, |ui| {
                    drag(ui, l("Margem", "Margin"), &mut p.pack_margin_px, 0.1, 0.0..=64.0, " px");
                    ui.checkbox(&mut p.pack_rotate, l("Permitir rotação (90°)", "Allow rotation (90°)"));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(l("Empacotar", "Pack")).clicked() {
                            act = Some(UvAction::Pack);
                        }
                        if ui.button(l("Normalizar densidade", "Normalize density")).on_hover_text(l("Mesma densidade de texel em todas as ilhas e reempacota", "Same texel density on every island, then repacks")).clicked() {
                            act = Some(UvAction::NormalizeDensity);
                        }
                    });
                });

                sub(ui, l("Projeções", "Projections"), false, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(l("Planar em", "Planar on"));
                        for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                            ui.selectable_value(&mut p.planar_axis, i, *n);
                        }
                        if ui.button(l("Planar", "Planar")).clicked() {
                            act = Some(UvAction::ProjectPlanar);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.checkbox(&mut p.box_cross, l("Cruz", "Cross"));
                        if ui.button(l("Caixa", "Box")).clicked() {
                            act = Some(UvAction::ProjectBox);
                        }
                        if ui.button(l("Cilindro", "Cylinder")).clicked() {
                            act = Some(UvAction::ProjectCylinder);
                        }
                        if ui.button(l("Esfera", "Sphere")).clicked() {
                            act = Some(UvAction::ProjectSphere);
                        }
                    });
                    drag(ui, l("Limite de ângulo", "Angle limit"), &mut p.smart_angle_deg, 0.5, 1.0..=88.0, "°");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Smart UV Project").clicked() {
                            act = Some(UvAction::SmartProject);
                        }
                        if ui.button(l("Da vista", "From view")).on_hover_text(l("Projeta pela câmera do viewport", "Projects through the viewport camera")).clicked() {
                            act = Some(UvAction::ProjectFromView);
                        }
                    });
                });

                sub(ui, l("Relatório", "Report"), true, |ui| {
                    match self.uv.cache.as_ref().map(|c| &c.report) {
                        Some(Ok(r)) => report_grid(ui, r),
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).color(WARN));
                        }
                        None => {
                            ui.label(RichText::new("—").color(theme::TEXT_DIM));
                        }
                    };
                });

                if let Some(err) = &self.uv.last_error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(err).color(WARN).size(12.0));
                }
                if let Some(a) = act {
                    self.run_uv_action(a);
                }
            });
    }
}

fn flag(ui: &mut egui::Ui, label: &str, n: usize) {
    ui.label(label);
    ui.label(RichText::new(n.to_string()).color(if n == 0 { OK } else { WARN }).strong());
    ui.end_row();
}

/// Tabela do `UvReport`.
pub(super) fn report_grid(ui: &mut egui::Ui, r: &UvReport) {
    egui::Grid::new("uv_report").num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
        ui.label(l("Faces / ilhas", "Faces / islands"));
        ui.label(format!("{} / {}", r.face_count, r.island_count));
        ui.end_row();
        ui.label(l("Estiramento médio", "Mean stretch"));
        ui.label(RichText::new(format!("{:.3}", r.mean_stretch)).strong());
        ui.end_row();
        ui.label(l("Estiramento máximo", "Max stretch"));
        ui.label(if r.max_stretch.is_finite() { format!("{:.2}", r.max_stretch) } else { "colapsado".to_string() });
        ui.end_row();
        flag(ui, l("Faces invertidas", "Flipped faces"), r.flipped_faces);
        flag(ui, l("Ilhas sobrepostas", "Overlapping islands"), r.overlapping_island_pairs);
        flag(ui, l("Autointerseção", "Self-intersection"), r.self_overlapping_faces);
        flag(ui, l("Faces colapsadas", "Collapsed faces"), r.degenerate_faces);
        flag(ui, l("Fora de [0,1]", "Outside [0,1]"), r.outside_unit_square);
        ui.label(l("Ocupação", "Coverage"));
        ui.label(format!("{:.0}%", r.uv_area * 100.0));
        ui.end_row();
        if let Some(d) = r.texel_density.filter(|_| !(ui.available_width() < 260.0)) {
            ui.label(l("Densidade média", "Mean density"));
            ui.label(format!("{:.0} px/u (±{:.0})", d.mean, d.std_dev));
            ui.end_row();
            ui.label(l("Densidade mín–máx", "Density min–max"));
            ui.label(format!("{:.0} – {:.0}", d.min, d.max));
            ui.end_row();
        }
    });
    // painel estreito: a densidade não cabe em duas colunas; vai empilhada (rótulo em cima, valor embaixo)
    if let Some(d) = r.texel_density.filter(|_| ui.available_width() < 260.0) {
        ui.add_space(2.0);
        ui.label(RichText::new(l("Densidade média", "Mean density")).color(theme::TEXT_DIM));
        ui.label(format!("{:.0} px/u (±{:.0})", d.mean, d.std_dev));
        ui.label(RichText::new(l("Densidade mín–máx", "Density min–max")).color(theme::TEXT_DIM));
        ui.label(format!("{:.0} – {:.0}", d.min, d.max));
    }
}

/// Cores do editor (fundo escuro como no ANIGO, nos dois temas do app).
#[allow(dead_code)]
mod theme {
    use egui::Color32;
    pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xea);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x9a, 0x9a, 0xa4);
    pub const SELECT: Color32 = Color32::from_rgb(255, 154, 46);
    pub const BORDER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x44);
}
