//! Painéis da Modelagem (S24 3D, fase 12): Objetos (o documento, adicionar primitivas, abrir um modelo editável do
//! projeto, salvar e enviar), Propriedades do objeto (nome, transformação, cor toon, sombreamento, malha) e
//! Modificadores (pilha espelho/subdivisão/array com ligar, subir, descer e tirar). Toda mudança passa pelo histórico da
//! Modelagem (Ctrl+Z).

use crate::app::modeling::{self as mdl, EditMode};
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_3d_edit::doc::{ModifierSpec, Primitive, Shading};
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
                        Ok(i) => env.ed.modeling.active = Some(i),
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
                        if let Some(x) = crate::app::panels::commit_on_release(ui, egui::Id::new(("mdl_xf", a, field, axis)), v[axis], |ui, x| ui.add(egui::DragValue::new(x).speed(speed).max_decimals(3))) {
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
            for k in ["mirror", "subsurf", "array"] {
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
