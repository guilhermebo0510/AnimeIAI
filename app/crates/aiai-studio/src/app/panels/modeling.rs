//! Painéis da Modelagem (S24 3D): Objetos (o documento, adicionar primitivas, abrir um modelo editável do projeto,
//! salvar e enviar), Propriedades (no modo Edição, os Operadores do ANIGO e o "Ajustar última operação"; nome,
//! transformação, cor toon, sombreamento, malha) e Modificadores (a pilha com os 9 tipos vendorizados do ANIGO: espelho,
//! subdivisão, array, espessura, soldar, separar arestas, reduzir, deslocar e triangular). Toda mudança passa pelo
//! histórico da Modelagem (Ctrl+Z).

use crate::app::modeling::keys::MAction;
use crate::app::modeling::{self as mdl, EditMode};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_3d_edit::doc::{ModifierSpec, Primitive, Shading};
use aiai_3d_edit::ops::catalog::{OpKind, OpParams};
use aiai_core::model3d::hex_rgb;
use egui::{Color32, Rect, RichText, Ui};

fn c32(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8)
}

fn rgb(c: Color32) -> [f32; 3] {
    [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0]
}

/// Painel Objetos.
pub(crate) fn outliner(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("mdl_outliner").show(ui, |ui| {
        // documento
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("mdl.doc_name"));
            let id = egui::Id::new("mdl_doc_name");
            let cur = env.ed.modeling.doc.name.clone();
            let (r, done) = crate::app::panels::text_commit(ui, id, &cur, 150.0);
            env.note("mdl:doc_name", r.rect);
            if let Some(n) = done.filter(|n| !n.trim().is_empty() && *n != cur) {
                let _ = env.ed.modeling.edit(|d| {
                    d.name = n.trim().to_owned();
                    Ok(())
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            let r = ui.button(tr("mdl.new"));
            env.note("mdl:new", r.rect);
            if r.clicked() {
                env.ed.modeling.new_doc(tr("mdl.new_name"));
            }
            let models = aiai_tools::modeling::editable_assets(&env.core.project);
            let r = ui.add_enabled_ui(!models.is_empty(), |ui| {
                ui.menu_button(tr("mdl.open"), |ui| {
                    for (id, name, objs) in &models {
                        if ui.button(format!("{name} ({objs})")).clicked() {
                            mdl::open_asset(env, id);
                            ui.close();
                        }
                    }
                })
            });
            env.note("mdl:open", r.response.rect);
        });
        ui.add_space(4.0);
        section(ui, &t, "mdl.add");
        ui.horizontal_wrapped(|ui| {
            for k in Primitive::ALL {
                let r = ui.button(tr(&format!("mdl.prim.{}", k.key())));
                env.note(format!("mdl:add.{}", k.key()), r.rect);
                if r.clicked() {
                    if let Err(e) = env.ed.modeling.add_primitive(k) {
                        env.ed.notify(e);
                    }
                }
            }
        });
        ui.add_space(4.0);
        section(ui, &t, "mdl.objects");
        if env.ed.modeling.doc.objects.is_empty() {
            dim_label(ui, &t, tr("mdl.no_objects"));
        }
        let n = env.ed.modeling.doc.objects.len();
        for i in 0..n {
            let (name, visible, color) = {
                let o = &env.ed.modeling.doc.objects[i];
                (o.name.clone(), o.visible, o.color)
            };
            ui.horizontal(|ui| {
                let eye = env.icon_button(ui, if visible { crate::icons::IconId::Eye } else { crate::icons::IconId::EyeOff }, "mdl.visible", false, true, &format!("mdl:obj_eye.{i}"));
                if eye.clicked() {
                    let _ = env.ed.modeling.edit(|d| {
                        d.objects[i].visible = !visible;
                        Ok(())
                    });
                }
                let (sw, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(sw, 2.0, c32(color));
                let active = env.ed.modeling.active == Some(i);
                let r = ui.selectable_label(active, &name);
                env.note(format!("mdl:obj.{i}"), r.rect);
                if r.clicked() {
                    let ms = &mut env.ed.modeling;
                    if ms.active != Some(i) {
                        ms.active = Some(i);
                        ms.sel.clear();
                        ms.touch();
                    }
                }
            });
        }
        if let Some(a) = env.ed.modeling.active {
            ui.horizontal(|ui| {
                let r = ui.button(tr("mdl.duplicate"));
                env.note("mdl:duplicate", r.rect);
                if r.clicked() {
                    let r = env.ed.modeling.edit(|d| {
                        let mut o = d.objects[a].clone();
                        o.name = d.unique_name(&o.name);
                        o.transform.pos[0] += 1.2;
                        d.objects.push(o);
                        Ok(d.objects.len() - 1)
                    });
                    match r {
                        Ok(i) => {
                            env.ed.modeling.active = Some(i);
                            env.ed.modeling.touch();
                        }
                        Err(e) => env.ed.notify(e),
                    }
                }
                let r = ui.button(tr("mdl.delete_object"));
                env.note("mdl:delete_object", r.rect);
                if r.clicked() {
                    let ms = &mut env.ed.modeling;
                    let _ = ms.edit(|d| {
                        d.objects.remove(a);
                        Ok(())
                    });
                    ms.active = None;
                    ms.mode = EditMode::Object;
                    ms.touch();
                }
            });
        }
        ui.add_space(6.0);
        section(ui, &t, "mdl.output");
        ui.horizontal_wrapped(|ui| {
            let r = ui.button(tr("mdl.save"));
            env.note("mdl:save2", r.rect);
            if r.on_hover_text(tr("mdl.save.tip")).clicked() {
                mdl::save_to_library(env);
            }
            let r = ui.button(tr("mdl.send"));
            env.note("mdl:send2", r.rect);
            if r.on_hover_text(tr("mdl.send.tip")).clicked() {
                mdl::send_to_layout(env);
            }
        });
        let asset = env.ed.modeling.asset.clone().and_then(|a| env.core.project.node(&a).map(|n| n.name.clone()));
        dim_label(ui, &t, &match asset {
            Some(n) => format!("{}: {n}", tr("mdl.linked")),
            None => tr("mdl.not_linked").to_owned(),
        });
        if !env.ed.modeling.user_files {
            dim_label(ui, &t, tr("mdl.no_session"));
        }
    });
}

/// Escultura: os 20 pincéis, raio, força, somar/tirar, simetria, dyntopo e as ferramentas de malha inteira.
fn sculpt_panel(env: &mut Env, ui: &mut Ui) {
    use crate::app::modeling::sculpt::{self as sc, SCULPT_BRUSHES};
    let t = env.t;
    section(ui, &t, "sculpt.title");
    let active = env.ed.modeling.sculpt.active;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
        for k in SCULPT_BRUSHES {
            let r = ui.selectable_label(active == k, sc::brush_label(k));
            env.note(format!("sculpt:brush.{}", sc::brush_key(k)), r.rect);
            if r.clicked() {
                // pincéis de ação única (inverter/limpar máscara) agem na hora, sem traço
                match k {
                    aiai_3d_edit::sculpt::brush::BrushKind::MaskInvert => {
                        let r = sc::mask_all(&mut env.ed.modeling, true).map(|_| ());
                        report(env, r);
                    }
                    aiai_3d_edit::sculpt::brush::BrushKind::MaskClear => {
                        let r = sc::mask_all(&mut env.ed.modeling, false).map(|_| ());
                        report(env, r);
                    }
                    _ => env.ed.modeling.sculpt.active = k,
                }
            }
        }
    });
    let ss = &mut env.ed.modeling.sculpt;
    egui::Grid::new("sculpt_brush").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        let slot = ss.active_slot_mut();
        dim_label(ui, &t, tr("sculpt.radius"));
        ui.add(egui::Slider::new(&mut slot.radius_px, sc::RADIUS_MIN_PX..=sc::RADIUS_MAX_PX).logarithmic(true).suffix(" px")).on_hover_text(tr("sculpt.radius.tip"));
        ui.end_row();
        dim_label(ui, &t, tr("sculpt.strength"));
        ui.add(egui::Slider::new(&mut slot.brush.strength, 0.0..=sc::STRENGTH_MAX).max_decimals(2));
        ui.end_row();
        dim_label(ui, &t, tr("sculpt.subtract"));
        ui.checkbox(&mut slot.brush.direction_invert, "").on_hover_text(tr("sculpt.subtract.tip"));
        ui.end_row();
        dim_label(ui, &t, tr("sculpt.symmetry"));
        ui.horizontal(|ui| {
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                ui.toggle_value(&mut ss.symmetry.mirror[i], *a);
            }
        });
        ui.end_row();
        dim_label(ui, &t, tr("sculpt.dyntopo"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut ss.dyntopo, "").on_hover_text(tr("sculpt.dyntopo.tip"));
            ui.add_enabled(ss.dyntopo, egui::DragValue::new(&mut ss.dyntopo_detail_px).range(2.0..=80.0).suffix(" px")).on_hover_text(tr("sculpt.dyntopo.detail"));
        });
        ui.end_row();
    });
    dim_label(ui, &t, tr("sculpt.keys"));
    ui.separator();
    section(ui, &t, "sculpt.whole");
    let mut run: Option<u8> = None;
    ui.horizontal_wrapped(|ui| {
        let r = ui.button(tr("sculpt.smooth_all"));
        env.notes.push(("sculpt:smooth_all".into(), r.rect));
        if r.clicked() {
            run = Some(0);
        }
        let ss = &mut env.ed.modeling.sculpt;
        ui.add(egui::DragValue::new(&mut ss.smooth_all_iters).range(1..=32).suffix("×"));
    });
    ui.horizontal_wrapped(|ui| {
        let r = ui.button(tr("sculpt.voxel"));
        env.notes.push(("sculpt:voxel".into(), r.rect));
        if r.clicked() {
            run = Some(1);
        }
        let ss = &mut env.ed.modeling.sculpt;
        let auto = env.ed.modeling.active.and_then(|a| env.ed.modeling.doc.objects.get(a)).map(|o| sc::suggest_voxel_size(&o.mesh)).unwrap_or(0.05);
        let mut v = if ss.voxel_size > 0.0 { ss.voxel_size } else { auto };
        if ui.add(egui::DragValue::new(&mut v).range(0.0005..=1.0).speed(0.0005).max_decimals(4)).on_hover_text(tr("sculpt.voxel.size")).changed() {
            ss.voxel_size = v;
        }
        ui.checkbox(&mut ss.voxel_shade_smooth, tr("sculpt.voxel.smooth"));
    });
    ui.horizontal_wrapped(|ui| {
        let r = ui.button(tr("sculpt.symmetrize"));
        env.notes.push(("sculpt:symmetrize".into(), r.rect));
        if r.clicked() {
            run = Some(2);
        }
        let ss = &mut env.ed.modeling.sculpt;
        for (i, a) in ["+X → −X", "+Y → −Y", "+Z → −Z"].iter().enumerate() {
            ui.selectable_value(&mut ss.symmetrize_axis, i, *a);
        }
    });
    match run {
        Some(0) => {
            let n = env.ed.modeling.sculpt.smooth_all_iters;
            let r = sc::smooth_all(&mut env.ed.modeling, n, 0.5);
            report(env, r);
        }
        Some(1) => {
            let r = sc::voxel_remesh(&mut env.ed.modeling).map(|_| ());
            report(env, r);
        }
        Some(2) => {
            let ax = env.ed.modeling.sculpt.symmetrize_axis;
            let r = sc::symmetrize(&mut env.ed.modeling, ax, true).map(|_| ());
            report(env, r);
        }
        _ => {}
    }
    let ss = &env.ed.modeling.sculpt;
    if let Some(e) = &ss.error {
        ui.colored_label(t.danger, e);
    } else if let Some(s) = &ss.status {
        dim_label(ui, &t, s);
    }
    ui.separator();
}

fn report(env: &mut Env, r: Result<(), String>) {
    match r {
        Ok(()) => env.ed.modeling.sculpt.error = None,
        Err(e) => {
            env.ed.notify(&e);
            env.ed.modeling.sculpt.error = Some(e);
        }
    }
}

/// O editor de UV enxerga o estado da Modelagem pelo `UvHost` (a câmera da vista ativa vai junto, para "da vista").
fn uv_host<R>(env: &mut Env, f: impl FnOnce(&mut mdl::uv::UvHost) -> R) -> R {
    let ms = &mut env.ed.modeling;
    let mut uv = std::mem::take(&mut ms.uv);
    let camera = ms.cam;
    let r = {
        let mut h = mdl::uv::UvHost { modeling: ms, uv: &mut uv, camera, notes: env.notes };
        f(&mut h)
    };
    env.ed.modeling.uv = uv;
    r
}

/// Normais personalizadas (sombra cel do rosto): a partir do centro, da vista ou de outro objeto, média, suavizar.
fn normals_section(env: &mut Env, ui: &mut Ui) {
    use crate::app::modeling::normals::{self as nm, NormalOp};
    use crate::app::modeling::uv::l;
    egui::CollapsingHeader::new(RichText::new(l("Normais (sombra cel)", "Normals (cel shading)")).strong()).id_salt("mdl_normals").default_open(false).show(ui, |ui| {
        let t = env.t;
        dim_label(ui, &t, l("Nas faces escolhidas (sem seleção: a malha toda).", "On the selected faces (no selection: the whole mesh)."));
        let a = env.ed.modeling.active;
        let names: Vec<(usize, String)> = env.ed.modeling.doc.objects.iter().enumerate().filter(|(i, _)| Some(*i) != a).map(|(i, o)| (i, o.name.clone())).collect();
        let nu = &mut env.ed.modeling.normals_ui;
        ui.horizontal(|ui| {
            ui.label(l("Força", "Amount"));
            ui.add(egui::Slider::new(&mut nu.factor, 0.0..=1.0).max_decimals(2));
        });
        ui.horizontal(|ui| {
            ui.label(l("Origem", "Source"));
            let cur = nu.source.and_then(|s| names.iter().find(|(i, _)| *i == s)).map(|(_, n)| n.clone()).unwrap_or_else(|| "—".into());
            egui::ComboBox::from_id_salt("mdl_normals_src").selected_text(cur).show_ui(ui, |ui| {
                for (i, n) in &names {
                    ui.selectable_value(&mut nu.source, Some(*i), n);
                }
            });
        });
        let mut run = None;
        ui.horizontal_wrapped(|ui| {
            for op in NormalOp::ALL {
                let r = ui.button(op.label()).on_hover_text(op.tip());
                env.notes.push((format!("mdl:normals.{}", op.key()), r.rect));
                if r.clicked() {
                    run = Some(op);
                }
            }
        });
        if let Some(op) = run {
            let st = env.ed.modeling.normals_ui;
            match nm::run(&mut env.ed.modeling, op, st) {
                Ok(n) => env.ed.notify(format!("{}: {n}", op.label())),
                Err(e) => env.ed.notify(e),
            }
        }
    });
}

/// Painel Editor de UV.
pub(crate) fn uv_editor(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 4.0);
    let ui = &mut child;
    if env.ed.modeling.mode != EditMode::Edit || env.ed.modeling.active.is_none() {
        dim_label(ui, &t, tr("panel.model_uv.empty"));
        return;
    }
    env.note("mdl:uv_editor", body);
    uv_host(env, |h| h.uv_editor_panel(ui));
}

/// Painel Propriedades do objeto.
pub(crate) fn properties(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(a) = env.ed.modeling.active.filter(|a| *a < env.ed.modeling.doc.objects.len()) else {
        dim_label(ui, &t, tr("mdl.need_object"));
        return;
    };
    egui::ScrollArea::vertical().id_salt("mdl_props").show(ui, |ui| {
        // auditoria UX-06: os parâmetros da operação que acabou de rodar (ex.: distância da extrusão) vêm primeiro; o
        // catálogo de operações é um bloco que dá para recolher (o estado fica guardado)
        if env.ed.modeling.mode == EditMode::Sculpt {
            sculpt_panel(env, ui);
        }
        if env.ed.modeling.mode == EditMode::Edit {
            adjust_last(env, ui);
            egui::CollapsingHeader::new(egui::RichText::new(tr("mdl.ops_catalog")).strong()).id_salt("mdl_ops_catalog").default_open(true).show(ui, |ui| operators(env, ui));
            uv_host(env, |h| h.inspector_uv(ui));
            normals_section(env, ui);
        }
        let o = env.ed.modeling.doc.objects[a].clone();
        section(ui, &t, "mdl.object");
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("mdl.name"));
            let (r, done) = crate::app::panels::text_commit(ui, egui::Id::new(("mdl_obj_name", a)), &o.name, 150.0);
            env.note("mdl:obj_name", r.rect);
            if let Some(n) = done.filter(|n| !n.trim().is_empty() && *n != o.name) {
                let _ = env.ed.modeling.edit(|d| {
                    d.objects[a].name = n.trim().to_owned();
                    Ok(())
                });
            }
        });
        egui::Grid::new(("mdl_xf", a)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (key, field, speed) in [("m3d.pos", 0usize, 0.01f64), ("m3d.rot", 1, 0.5), ("m3d.scale", 2, 0.01)] {
                dim_label(ui, &t, tr(key));
                let v = [o.transform.pos, o.transform.rot_deg, o.transform.scale][field];
                ui.horizontal(|ui| {
                    for axis in 0..3 {
                        // eixo nomeado e na cor do gizmo (auditoria UX-07): a cor nunca é o único sinal
                        let (name, col) = [("X", egui::Color32::from_rgb(230, 90, 90)), ("Y", egui::Color32::from_rgb(110, 200, 110)), ("Z", egui::Color32::from_rgb(100, 150, 240))][axis];
                        ui.label(egui::RichText::new(name).strong().color(col).size(11.0));
                        let unit = match field {
                            0 => " m",
                            1 => "°",
                            _ => "",
                        };
                        if let Some(x) = crate::app::panels::commit_on_release(ui, egui::Id::new(("mdl_xf", a, field, axis)), v[axis], |ui, x| ui.add(egui::DragValue::new(x).speed(speed).max_decimals(3).suffix(unit))) {
                            if field == 2 && x.abs() < 1e-4 {
                                continue;
                            }
                            let _ = env.ed.modeling.edit(|d| {
                                let tr = &mut d.objects[a].transform;
                                match field {
                                    0 => tr.pos[axis] = x,
                                    1 => tr.rot_deg[axis] = x,
                                    _ => tr.scale[axis] = x,
                                }
                                Ok(())
                            });
                        }
                    }
                });
                ui.end_row();
            }
        });
        section(ui, &t, "mdl.material");
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("mdl.color"));
            let mut c = c32(o.color);
            let r = ui.color_edit_button_srgba(&mut c);
            env.note("mdl:color", r.rect);
            // um passo de desfazer quando o seletor fecha (não a cada arrasto dentro dele)
            if r.changed() {
                env.ed.modeling.doc.objects[a].color = rgb(c);
                env.ed.modeling.touch();
                let key = egui::Id::new(("mdl_color_before", a));
                if ui.data(|dm| dm.get_temp::<[f32; 3]>(key)).is_none() {
                    ui.data_mut(|dm| dm.insert_temp(key, o.color));
                }
            }
            let before: Option<[f32; 3]> = ui.data(|dm| dm.get_temp(egui::Id::new(("mdl_color_before", a))));
            let idle = !r.changed() && !ui.input(|i| i.pointer.any_down());
            if let Some(b) = before.filter(|_| idle) {
                ui.data_mut(|dm| dm.remove_temp::<[f32; 3]>(egui::Id::new(("mdl_color_before", a))));
                let now = env.ed.modeling.doc.objects[a].color;
                env.ed.modeling.doc.objects[a].color = b;
                let _ = env.ed.modeling.edit(|d| {
                    d.objects[a].color = now;
                    Ok(())
                });
            }
            ui.label(RichText::new(hex_rgb(env.ed.modeling.doc.objects[a].color)).monospace().size(11.0).color(t.text_dim));
        });
        ui.horizontal_wrapped(|ui| {
            for (i, pc) in aiai_3d_edit::doc::COLORS.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                ui.painter().rect_filled(r, 3.0, c32(*pc));
                env.note(format!("mdl:swatch.{i}"), r);
                if resp.on_hover_text(hex_rgb(*pc)).clicked() {
                    let _ = env.ed.modeling.edit(|d| {
                        d.objects[a].color = *pc;
                        Ok(())
                    });
                }
            }
        });
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr("mdl.shading"));
            for s in Shading::ALL {
                let r = ui.selectable_label(o.shading == s, tr(&format!("mdl.shading.{}", s.key())));
                env.note(format!("mdl:shading.{}", s.key()), r.rect);
                if r.clicked() && o.shading != s {
                    let _ = env.ed.modeling.edit(|d| {
                        d.objects[a].shading = s;
                        Ok(())
                    });
                }
            }
        });
        section(ui, &t, "mdl.mesh");
        let ev = o.evaluated();
        let (ef, et) = match &ev {
            Ok(m) => (m.face_count(), m.triangle_count()),
            Err(_) => (0, 0),
        };
        dim_label(ui, &t, &format!("{} {} · {} {} · {} {} ({} {})", o.mesh.vert_count(), tr("mdl.verts"), o.mesh.face_count(), tr("mdl.faces"), ef, tr("mdl.faces_eval"), et, tr("m3d.tris")));
        if let Err(e) = ev {
            ui.label(RichText::new(e.to_string()).color(t.danger).size(11.5));
        }
        let r = ui.button(tr("mdl.merge_near")).on_hover_text(tr("mdl.merge_near.tip"));
        env.note("mdl:merge_near", r.rect);
        if r.clicked() {
            mdl::merge_near(env);
        }
        if env.ed.modeling.mode == EditMode::Edit {
            let s = &env.ed.modeling.sel;
            dim_label(ui, &t, &format!("{}: {} {} · {} {} · {} {}", tr("mdl.selection"), s.verts.len(), tr("mdl.verts"), s.edges.len(), tr("mdl.edges"), s.faces.len(), tr("mdl.faces")));
        }
    });
}

/// Painel Modificadores.
pub(crate) fn modifiers(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    let Some(a) = env.ed.modeling.active.filter(|a| *a < env.ed.modeling.doc.objects.len()) else {
        dim_label(ui, &t, tr("mdl.need_object"));
        return;
    };
    egui::ScrollArea::vertical().id_salt("mdl_mods").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            dim_label(ui, &t, tr("mdl.mod.add"));
            for k in ModifierSpec::KEYS {
                let r = ui.button(tr(&format!("mdl.mod.{k}")));
                env.note(format!("mdl:mod_add.{k}"), r.rect);
                if r.clicked() {
                    let r = env.ed.modeling.edit(|d| {
                        let m = ModifierSpec::new(k).ok_or("modificador desconhecido")?;
                        d.objects[a].modifiers.push(m);
                        d.objects[a].evaluated().map_err(|e| e.to_string())?;
                        Ok(())
                    });
                    if let Err(e) = r {
                        env.ed.notify(e);
                    }
                }
            }
        });
        let mods = env.ed.modeling.doc.objects[a].modifiers.clone();
        if mods.is_empty() {
            dim_label(ui, &t, tr("mdl.mod.none"));
        }
        let n = mods.len();
        for (i, m) in mods.iter().enumerate() {
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut on = m.enabled();
                    let r = ui.checkbox(&mut on, RichText::new(tr(&format!("mdl.mod.{}", m.key()))).strong());
                    env.note(format!("mdl:mod.{i}"), r.rect);
                    if r.changed() {
                        let _ = env.ed.modeling.edit(|d| {
                            d.objects[a].modifiers[i].set_enabled(on);
                            Ok(())
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if env.icon_button(ui, crate::icons::IconId::Trash, "mdl.mod.remove", false, true, &format!("mdl:mod_remove.{i}")).clicked() {
                            let _ = env.ed.modeling.edit(|d| {
                                d.objects[a].modifiers.remove(i);
                                Ok(())
                            });
                        }
                        if env.icon_button(ui, crate::icons::IconId::ArrowDown, "mdl.mod.down", false, i + 1 < n, &format!("mdl:mod_down.{i}")).clicked() {
                            let _ = env.ed.modeling.edit(|d| {
                                d.objects[a].modifiers.swap(i, i + 1);
                                Ok(())
                            });
                        }
                        if env.icon_button(ui, crate::icons::IconId::ArrowUp, "mdl.mod.up", false, i > 0, &format!("mdl:mod_up.{i}")).clicked() {
                            let _ = env.ed.modeling.edit(|d| {
                                d.objects[a].modifiers.swap(i, i - 1);
                                Ok(())
                            });
                        }
                    });
                });
                let mut changed: Option<ModifierSpec> = None;
                match m.clone() {
                    ModifierSpec::Mirror { mut axes, merge, enabled } => {
                        ui.horizontal(|ui| {
                            dim_label(ui, &t, tr("mdl.mod.axes"));
                            for (k, l) in ["X", "Y", "Z"].iter().enumerate() {
                                if ui.checkbox(&mut axes[k], *l).changed() {
                                    changed = Some(ModifierSpec::Mirror { axes, merge, enabled });
                                }
                            }
                        });
                    }
                    ModifierSpec::Subsurf { levels, enabled } => {
                        ui.horizontal(|ui| {
                            dim_label(ui, &t, tr("mdl.mod.levels"));
                            let mut l = levels;
                            if ui.add(egui::Slider::new(&mut l, 1..=3)).changed() && l != levels {
                                changed = Some(ModifierSpec::Subsurf { levels: l, enabled });
                            }
                        });
                    }
                    ModifierSpec::Array { count, offset, enabled } => {
                        ui.horizontal(|ui| {
                            dim_label(ui, &t, tr("mdl.mod.count"));
                            if let Some(c) = crate::app::panels::commit_on_release(ui, egui::Id::new(("mdl_arr_count", a, i)), count, |ui, c| ui.add(egui::DragValue::new(c).range(1..=100))) {
                                changed = Some(ModifierSpec::Array { count: c, offset, enabled });
                            }
                        });
                        ui.horizontal(|ui| {
                            dim_label(ui, &t, tr("mdl.mod.offset"));
                            for k in 0..3 {
                                if let Some(x) = crate::app::panels::commit_on_release(ui, egui::Id::new(("mdl_arr_off", a, i, k)), offset[k], |ui, x| ui.add(egui::DragValue::new(x).speed(0.01).max_decimals(2))) {
                                    let mut o2 = offset;
                                    o2[k] = x;
                                    changed = Some(ModifierSpec::Array { count, offset: o2, enabled });
                                }
                            }
                        });
                    }
                    other => {
                        if let Some(nm) = modifier_params(ui, &t, a, i, &other) {
                            changed = Some(nm);
                        }
                    }
                }
                if let Some(nm) = changed {
                    let r = env.ed.modeling.edit(|d| {
                        nm.validate()?;
                        d.objects[a].modifiers[i] = nm;
                        d.objects[a].evaluated().map_err(|e| e.to_string())?;
                        Ok(())
                    });
                    if let Err(e) = r {
                        env.ed.notify(e);
                    }
                }
            });
        }
    });
}

/// Campos dos modificadores vendorizados do ANIGO (espessura, soldar, separar arestas, reduzir, deslocar, triangular).
fn modifier_params(ui: &mut Ui, t: &crate::theme::Tokens, a: usize, i: usize, m: &ModifierSpec) -> Option<ModifierSpec> {
    let mut out = None;
    let num = |ui: &mut Ui, key: &str, salt: &str, v: f32, speed: f64, lo: f32, hi: f32| -> Option<f32> {
        ui.horizontal(|ui| {
            dim_label(ui, t, tr(key));
            crate::app::panels::commit_on_release(ui, egui::Id::new(("mdl_mod_f", a, i, salt.to_owned())), v, |ui, x| ui.add(egui::DragValue::new(x).speed(speed).range(lo..=hi).max_decimals(3)))
        })
        .inner
    };
    match m.clone() {
        ModifierSpec::Solidify { thickness, offset, even, rim, enabled } => {
            if let Some(v) = num(ui, "mdl.mod.thickness", "th", thickness, 0.002, -10.0, 10.0).filter(|v| v.abs() >= 1e-4) {
                out = Some(ModifierSpec::Solidify { thickness: v, offset, even, rim, enabled });
            }
            if let Some(v) = num(ui, "mdl.mod.shell_offset", "off", offset, 0.02, -1.0, 1.0) {
                out = Some(ModifierSpec::Solidify { thickness, offset: v, even, rim, enabled });
            }
            ui.horizontal(|ui| {
                let (mut e, mut r) = (even, rim);
                let c1 = ui.checkbox(&mut e, tr("mdl.mod.even")).changed();
                let c2 = ui.checkbox(&mut r, tr("mdl.mod.rim")).changed();
                if c1 || c2 {
                    out = Some(ModifierSpec::Solidify { thickness, offset, even: e, rim: r, enabled });
                }
            });
        }
        ModifierSpec::Weld { distance, enabled } => {
            if let Some(v) = num(ui, "mdl.mod.distance", "d", distance, 0.0005, 0.0, 10.0) {
                out = Some(ModifierSpec::Weld { distance: v, enabled });
            }
        }
        ModifierSpec::EdgeSplit { angle_deg, sharp, enabled } => {
            if let Some(v) = num(ui, "mdl.mod.angle", "ang", angle_deg, 0.5, 0.0, 180.0) {
                out = Some(ModifierSpec::EdgeSplit { angle_deg: v, sharp, enabled });
            }
            let mut s = sharp;
            if ui.checkbox(&mut s, tr("mdl.mod.sharp")).changed() {
                out = Some(ModifierSpec::EdgeSplit { angle_deg, sharp: s, enabled });
            }
        }
        ModifierSpec::Decimate { ratio, enabled } => {
            if let Some(v) = num(ui, "mdl.mod.ratio", "r", ratio, 0.01, 0.01, 1.0) {
                out = Some(ModifierSpec::Decimate { ratio: v, enabled });
            }
        }
        ModifierSpec::Displace { strength, noise_scale, seed, axis, enabled } => {
            if let Some(v) = num(ui, "mdl.mod.strength", "s", strength, 0.005, -10.0, 10.0) {
                out = Some(ModifierSpec::Displace { strength: v, noise_scale, seed, axis, enabled });
            }
            if let Some(v) = num(ui, "mdl.mod.noise_scale", "n", noise_scale, 0.01, 0.0, 100.0) {
                out = Some(ModifierSpec::Displace { strength, noise_scale: v, seed, axis, enabled });
            }
            ui.horizontal(|ui| {
                dim_label(ui, t, tr("mdl.mod.direction"));
                for (val, label) in [(None, tr("mdl.mod.normal")), (Some(0), "X"), (Some(1), "Y"), (Some(2), "Z")] {
                    if ui.selectable_label(axis == val, label).clicked() && axis != val {
                        out = Some(ModifierSpec::Displace { strength, noise_scale, seed, axis: val, enabled });
                    }
                }
            });
            ui.horizontal(|ui| {
                dim_label(ui, t, tr("mdl.mod.seed"));
                if let Some(v) = crate::app::panels::commit_on_release(ui, egui::Id::new(("mdl_mod_seed", a, i)), seed, |ui, x| ui.add(egui::DragValue::new(x).range(0..=9999))) {
                    out = Some(ModifierSpec::Displace { strength, noise_scale, seed: v, axis, enabled });
                }
            });
        }
        _ => {}
    }
    out
}

/// Grupos de operadores (a ordem e os grupos do Inspector do ANIGO) com a ação de atalho, quando há.
const OP_GROUPS: &[(&str, &[(OpKind, Option<MAction>)])] = &[
    (
        "mdl.ops.geometry",
        &[
            (OpKind::Extrude, Some(MAction::Extrude)),
            (OpKind::ExtrudeIndividual, None),
            (OpKind::Inset, Some(MAction::Inset)),
            (OpKind::InsetIndividual, None),
            (OpKind::Bevel, Some(MAction::Bevel)),
            (OpKind::LoopCut, Some(MAction::LoopCut)),
            (OpKind::Bisect, Some(MAction::Bisect)),
            (OpKind::Bridge, None),
            (OpKind::Subdivide, None),
            (OpKind::Poke, None),
        ],
    ),
    (
        "mdl.ops.remove",
        &[
            (OpKind::DeleteVerts, Some(MAction::Delete)),
            (OpKind::DeleteEdges, None),
            (OpKind::DeleteFaces, None),
            (OpKind::DeleteOnlyFaces, None),
            (OpKind::MergeCenter, Some(MAction::Merge)),
            (OpKind::MergeByDistance, Some(MAction::MergeNear)),
            (OpKind::DissolveVerts, Some(MAction::Dissolve)),
            (OpKind::DissolveEdges, None),
            (OpKind::DissolveFaces, None),
            (OpKind::LimitedDissolve, None),
            (OpKind::FillHole, Some(MAction::FillHole)),
            (OpKind::GridFill, None),
        ],
    ),
    (
        "mdl.ops.faces",
        &[
            (OpKind::Triangulate, Some(MAction::Triangulate)),
            (OpKind::TrisToQuads, None),
            (OpKind::FlipNormals, None),
            (OpKind::RecalcOutside, Some(MAction::RecalcNormals)),
            (OpKind::HideSelected, Some(MAction::Hide)),
            (OpKind::RevealAll, Some(MAction::Reveal)),
        ],
    ),
    ("mdl.ops.verts", &[(OpKind::Smooth, None), (OpKind::Relax, None)]),
    ("mdl.ops.edge_attrs", &[(OpKind::MarkSharp, None), (OpKind::ClearSharp, None), (OpKind::MarkSeam, None), (OpKind::ClearSeam, None), (OpKind::SetCrease, None)]),
];

/// Seção Operadores (modo Edição): um botão por operador do catálogo, com a tecla na dica.
fn operators(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    section(ui, &t, "mdl.ops.title");
    let keys = env.ed.modeling.keys.clone();
    for (title, ops) in OP_GROUPS {
        ui.label(RichText::new(tr(title)).color(t.text_dim).size(11.5));
        ui.horizontal_wrapped(|ui| {
            for (k, act) in ops.iter() {
                let label = tr(&mdl::op_label_key(*k)).to_owned();
                let tip = match act.map(|a| keys.label(a)).filter(|s| !s.is_empty()) {
                    Some(key) => format!("{label} ({key})"),
                    None => label.clone(),
                };
                let r = ui.small_button(label).on_hover_text(tip);
                env.note(format!("mdl:op.{}", k.key()), r.rect);
                if r.clicked() {
                    mdl::run(env, *k);
                }
            }
        });
    }
}

/// Painel "Ajustar última operação" (Redo Panel): mudar um parâmetro refaz a operação.
fn adjust_last(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(last) = env.ed.modeling.last_op_valid() else { return };
    let kind = last.kind;
    if !kind.has_params() {
        return;
    }
    let mut p: OpParams = last.params.clone();
    let mut changed = false;
    ui.add_space(4.0);
    let title = ui.label(RichText::new(format!("{}: {}", tr("mdl.ops.adjust"), tr(&mdl::op_label_key(kind)))).strong().color(t.accent));
    env.note("mdl:adjust", title.rect);
    let mut dv = |ui: &mut Ui, key: &str, v: &mut f32, speed: f64| {
        ui.horizontal(|ui| {
            dim_label(ui, &t, tr(key));
            let r = ui.add(egui::DragValue::new(v).speed(speed).max_decimals(3));
            changed |= r.changed();
        });
    };
    match kind {
        OpKind::Extrude => dv(ui, "mdl.p.distance", &mut p.distance, 0.01),
        OpKind::Inset | OpKind::InsetIndividual => {
            dv(ui, "mdl.p.thickness", &mut p.thickness, 0.005);
            dv(ui, "mdl.p.depth", &mut p.depth, 0.005);
        }
        OpKind::Bevel => dv(ui, "mdl.p.width", &mut p.width, 0.005),
        OpKind::MergeByDistance => dv(ui, "mdl.p.merge_distance", &mut p.merge_distance, 0.0005),
        OpKind::LimitedDissolve => dv(ui, "mdl.p.angle", &mut p.dissolve_angle_deg, 0.5),
        OpKind::Smooth | OpKind::Relax => dv(ui, "mdl.p.factor", &mut p.factor, 0.01),
        OpKind::Subdivide => dv(ui, "mdl.p.smoothness", &mut p.subdiv_smooth, 0.01),
        OpKind::Poke => dv(ui, "mdl.p.offset", &mut p.poke_offset, 0.01),
        OpKind::Bisect => dv(ui, "mdl.p.plane_offset", &mut p.plane_offset, 0.01),
        _ => {}
    }
    drop(dv);
    match kind {
        OpKind::Inset => {
            changed |= ui.checkbox(&mut p.outset, tr("mdl.p.outset")).changed();
            changed |= ui.checkbox(&mut p.even, tr("mdl.p.even")).changed();
        }
        OpKind::Bevel => {
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.segments"));
                changed |= ui.add(egui::DragValue::new(&mut p.segments).range(1..=32)).changed();
            });
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.profile"));
                changed |= ui.add(egui::Slider::new(&mut p.profile, 0.0..=1.0)).changed();
            });
        }
        OpKind::LoopCut => {
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.cuts"));
                let r = ui.add(egui::DragValue::new(&mut p.cuts).range(1..=64));
                env.note("mdl:adjust.cuts", r.rect);
                changed |= r.changed();
            });
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.slide"));
                changed |= ui.add(egui::Slider::new(&mut p.slide, -0.95..=0.95)).changed();
            });
        }
        OpKind::Smooth | OpKind::Relax => {
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.iterations"));
                changed |= ui.add(egui::DragValue::new(&mut p.iterations).range(1..=100)).changed();
            });
            changed |= ui.checkbox(&mut p.lock_boundary, tr("mdl.p.lock_boundary")).changed();
        }
        OpKind::Subdivide => {
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.cuts"));
                changed |= ui.add(egui::DragValue::new(&mut p.subdiv_cuts).range(1..=16)).changed();
            });
        }
        OpKind::SetCrease => {
            ui.horizontal(|ui| {
                dim_label(ui, &t, tr("mdl.p.crease"));
                changed |= ui.add(egui::Slider::new(&mut p.crease, 0.0..=1.0)).changed();
            });
        }
        OpKind::Bisect => {
            ui.horizontal(|ui| {
                for (i, n) in ["X", "Y", "Z"].iter().enumerate() {
                    changed |= ui.selectable_value(&mut p.plane_axis, i, *n).changed();
                }
            });
            changed |= ui.checkbox(&mut p.clear_inner, tr("mdl.p.clear_inner")).changed();
            changed |= ui.checkbox(&mut p.clear_outer, tr("mdl.p.clear_outer")).changed();
            changed |= ui.checkbox(&mut p.fill, tr("mdl.p.fill")).changed();
        }
        _ => {}
    }
    if changed {
        if let Err(e) = env.ed.modeling.redo_last_with(p) {
            env.ed.notify(e);
        }
    }
}
