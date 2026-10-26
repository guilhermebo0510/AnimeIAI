//! Painel Réguas (S24 2.1–2.2): as réguas da camada de guia ativa (ou da escolhida aqui): criar pelo menu `+`
//! (perspectiva de 1, 2 e 3 pontos, paralelas, radial, simetria, grade em perspectiva), ativar, mostrar/ocultar, encaixe,
//! travar, renomear, apagar, duplicar, "Grade de chão" a partir da perspectiva e "Converter em traços"; embaixo, as
//! propriedades da régua ativa com os campos numéricos (pontos de fuga, nível do olho, ângulo, espaçamento, cantos...).
//! Também as opções da ferramenta Régua (`tool_options`). Toda mudança grava o conjunto (um passo de undo).

use crate::app::commands as cmds;
use crate::app::rulers::{self, NewKind};
use crate::app::ui_kit::{Env, body_ui, color32_to_rgba, dim_label, rgba_to_color32, section};
use crate::i18n::tr;
use crate::icons::IconId;
use crate::keymap::Action;
use aiai_tools::ruler::{self as rl, Ruler, RulerKind};
use egui::{Id, Rect, RichText, Ui};

pub(crate) fn show(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 8.0);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("rulers_panel").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 5.0;
        // encaixe e visibilidade gerais
        ui.horizontal_wrapped(|ui| {
            let mut show = env.ed.rulers.show;
            let r = ui.checkbox(&mut show, tr("ruler.show")).on_hover_text(format!("{} ({})", tr("ruler.show_tip"), env.ed.keys.label(Action::RulersShow)));
            env.note("rulers:show", r.rect);
            env.ed.rulers.show = show;
            let mut snap = env.ed.rulers.snap;
            let r = ui.checkbox(&mut snap, tr("ruler.snap")).on_hover_text(format!("{} ({})", tr("ruler.snap_tip_s25"), env.ed.keys.label(Action::RulerSnap)));
            env.note("rulers:snap", r.rect);
            env.ed.rulers.snap = snap;
        });
        if env.ed.active_cut(env.core).is_none() {
            dim_label(ui, &t, tr("panel.rulers.empty"));
            return;
        }
        // camada de guia das réguas
        let guides = rulers::guide_layers_of(env);
        let layer = rulers::ruler_layer(env);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("ruler.layer")).size(12.0).color(t.text_dim));
            let name = |id: &cmds::NodeId| env.core.project.node(id).map(|n| n.name.clone()).unwrap_or_default();
            let cur = layer.as_ref().map(name).unwrap_or_else(|| tr("ruler.no_layer").to_owned());
            let mut pick = None;
            let r = egui::ComboBox::from_id_salt("ruler_layer").selected_text(cur).width(130.0).show_ui(ui, |ui| {
                for g in &guides {
                    if ui.selectable_label(Some(g) == layer.as_ref(), name(g)).clicked() {
                        pick = Some(g.clone());
                    }
                }
            });
            env.note("rulers:layer", r.response.rect);
            if let Some(g) = pick {
                env.ed.rulers.layer = Some(g.clone());
                // a camada ativa de guia manda: escolher outra aqui também a ativa
                if env.ed.layer.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).is_some_and(|l| l.kind == aiai_core::LayerType::Guide) {
                    env.ed.layer = Some(g);
                }
            }
        });
        // + (tipos), duplicar, apagar
        ui.horizontal_wrapped(|ui| {
            let mut new = None;
            let r = ui.menu_button(RichText::new(format!("+ {}", tr("ruler.new"))), |ui| {
                for k in NewKind::types() {
                    let b = ui.button(tr(k.key()));
                    env.note(format!("rulers:new.{}", k.id()), b.rect);
                    if b.clicked() {
                        new = Some(k);
                        ui.close();
                    }
                }
            });
            env.note("rulers:new", r.response.rect);
            if let Some(k) = new {
                env.ed.rulers.new_kind = k;
                rulers::create(env, k, None);
            }
            let has = layer.as_ref().and_then(|l| rulers::layer_set(&env.core.project, l).ok()).is_some_and(|s| !s.items.is_empty());
            let b = ui.add_enabled(has, egui::Button::new(tr("ruler.duplicate")));
            env.note("rulers:duplicate", b.rect);
            if b.clicked() {
                duplicate(env);
            }
            if env.icon_button(ui, IconId::Trash, "ruler.delete", false, has, "rulers:delete").clicked() {
                rulers::delete_active(env);
            }
        });
        let Some(layer) = layer else {
            dim_label(ui, &t, tr("ruler.no_layer_hint"));
            return;
        };
        let set = match rulers::layer_set(&env.core.project, &layer) {
            Ok(s) => s,
            Err(e) => {
                // formato quebrado: diz o que é (nada some em silêncio)
                ui.label(RichText::new(e).color(t.danger));
                return;
            }
        };
        if set.items.is_empty() {
            dim_label(ui, &t, tr("ruler.empty_hint"));
            return;
        }
        // lista: olho, encaixe, cadeado, cor, nome (clique ativa)
        let active = set.active_ruler().map(|r| r.id.clone());
        let mut new_set = set.clone();
        let mut changed: Option<&str> = None;
        for (i, r) in set.items.iter().enumerate() {
            ui.horizontal(|ui| {
                let vis = r.visible;
                if env.icon_button_sized(ui, if vis { IconId::Eye } else { IconId::EyeOff }, "ruler.visible", false, true, &format!("rulers:{}:visible", r.id), 20.0, false).clicked() {
                    new_set.items[i].visible = !vis;
                    changed = Some("ruler.undo.edit");
                }
                if env.icon_button_sized(ui, IconId::Snap, "ruler.snap_one", r.snap, true, &format!("rulers:{}:snap", r.id), 20.0, false).clicked() {
                    new_set.items[i].snap = !r.snap;
                    changed = Some("ruler.undo.edit");
                }
                if env.icon_button_sized(ui, if r.locked { IconId::Lock } else { IconId::Unlock }, "ruler.lock", r.locked, true, &format!("rulers:{}:lock", r.id), 20.0, false).clicked() {
                    new_set.items[i].locked = !r.locked;
                    changed = Some("ruler.undo.edit");
                }
                let (sw, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                ui.painter().rect_filled(sw, 2.0, rgba_to_color32(r.rgba()));
                let label = format!("{}  ·  {}", r.name, tr(NewKind::of(r).key()));
                let b = ui.selectable_label(active.as_deref() == Some(r.id.as_str()), label);
                env.note(format!("rulers:{}:select", r.id), b.rect);
                if b.clicked() && active.as_deref() != Some(r.id.as_str()) {
                    new_set.active = Some(r.id.clone());
                    changed = Some("ruler.undo.activate");
                }
            });
        }
        if let Some(label) = changed {
            rulers::write(env, &layer, &new_set, label);
            return;
        }
        let Some(r) = set.active_ruler().cloned() else { return };
        ui.add_space(4.0);
        section(ui, &t, "ruler.props");
        properties(env, ui, &r);
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let can_floor = r.kind == RulerKind::Perspective && r.vps.len() >= 2;
            let b = ui.add_enabled(can_floor, egui::Button::new(tr("ruler.floor"))).on_hover_text(tr("ruler.floor_tip"));
            env.note("rulers:floor", b.rect);
            if b.clicked() {
                rulers::floor_grid(env);
            }
            let b = ui.button(tr("ruler.bake")).on_hover_text(tr("ruler.bake_tip"));
            env.note("rulers:bake", b.rect);
            if b.clicked() {
                rulers::bake_active(env);
            }
        });
        ui.label(RichText::new(tr("ruler.view_only")).size(11.0).color(t.text_dim));
    });
}

fn duplicate(env: &mut Env) {
    let Some(layer) = rulers::ruler_layer(env) else { return };
    let Ok(mut set) = rulers::layer_set(&env.core.project, &layer) else { return };
    let Some(mut r) = set.active_ruler().cloned() else { return };
    r.id = set.next_id();
    r.name = format!("{} ({})", r.name, tr("ruler.copy"));
    set.add(r);
    rulers::write(env, &layer, &set, "ruler.undo.create");
}

/// Campo numérico que grava ao soltar (um passo de undo por ajuste).
fn num(ui: &mut Ui, id: Id, cur: f32, speed: f64, range: std::ops::RangeInclusive<f32>, suffix: &str) -> Option<f32> {
    let suffix = suffix.to_owned();
    super::commit_on_release(ui, id, cur, |ui, v| ui.add(egui::DragValue::new(v).speed(speed).range(range).max_decimals(1).suffix(suffix)))
}

fn num_u(ui: &mut Ui, id: Id, cur: u32, range: std::ops::RangeInclusive<u32>) -> Option<u32> {
    super::commit_on_release(ui, id, cur, |ui, v| ui.add(egui::DragValue::new(v).speed(0.2).range(range)))
}

/// Propriedades da régua ativa: nome, tipo, cor e os campos do tipo.
fn properties(env: &mut Env, ui: &mut Ui, r: &Ruler) {
    let t = env.t;
    let base = Id::new(("ruler_props", r.id.clone()));
    let mut edit: Option<Box<dyn FnOnce(&mut Ruler, (u32, u32))>> = None;
    let mut convert = None;
    egui::Grid::new(("ruler_grid", r.id.clone())).num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
        ui.label(tr("ruler.name"));
        let (resp, name) = super::text_commit(ui, base.with("name"), &r.name, 150.0);
        env.note("rulers:name", resp.rect);
        if let Some(n) = name {
            edit = Some(Box::new(move |x, _| x.name = n));
        }
        ui.end_row();
        ui.label(tr("ruler.type"));
        let cur = NewKind::of(r);
        let mut pick = None;
        let resp = egui::ComboBox::from_id_salt(("ruler_type", r.id.clone())).selected_text(tr(cur.key())).show_ui(ui, |ui| {
            for k in NewKind::types() {
                if ui.selectable_label(k == cur, tr(k.key())).clicked() {
                    pick = Some(k);
                }
            }
        });
        env.note("rulers:type", resp.response.rect);
        // tipo novo: a régua é convertida no lugar (posição, horizonte e cor ficam onde fazem sentido)
        convert = pick.filter(|k| *k != cur);
        ui.end_row();
        ui.label(tr("ruler.color"));
        let mut c = rgba_to_color32(r.rgba());
        let resp = ui.color_edit_button_srgba(&mut c);
        env.note("rulers:color", resp.rect);
        if resp.changed() {
            let hex = color32_to_rgba(c).to_hex();
            edit = Some(Box::new(move |x, _| x.color = hex));
        }
        ui.end_row();
        let canvas = env.ed.cut.as_ref().map(|c| cmds::canvas_size(&env.core.project, c)).unwrap_or((1920, 1080));
        match r.kind {
            RulerKind::Perspective => {
                for i in 0..r.vps.len() {
                    ui.label(format!("{} {}", tr("ruler.vp"), i + 1));
                    ui.horizontal(|ui| {
                        let p = r.vps[i];
                        if let Some(x) = num(ui, base.with(("vx", i)), p[0], 2.0, -1e6..=1e6, "") {
                            edit = Some(Box::new(move |r, _| r.vps[i][0] = x));
                        }
                        if let Some(y) = num(ui, base.with(("vy", i)), p[1], 2.0, -1e6..=1e6, "") {
                            edit = Some(Box::new(move |r, _| r.vps[i][1] = y));
                        }
                    });
                    ui.end_row();
                }
                if let Some(y) = rl::eye_level(r, canvas) {
                    ui.label(tr("ruler.eye_level")).on_hover_text(tr("ruler.eye_level_tip"));
                    if let Some(ny) = num(ui, base.with("eye"), y, 1.0, -1e6..=1e6, " px") {
                        edit = Some(Box::new(move |r, c| rl::set_eye_level(r, c, ny)));
                    }
                    ui.end_row();
                }
                if r.vps.len() == 1 {
                    ui.label(tr("ruler.horizon_angle"));
                    if let Some(a) = num(ui, base.with("hang"), r.horizon_angle, 0.5, -90.0..=90.0, "°") {
                        edit = Some(Box::new(move |r, _| r.horizon_angle = a));
                    }
                    ui.end_row();
                }
                ui.label(tr("ruler.density"));
                if let Some(d) = num_u(ui, base.with("dens"), r.density, 2..=256) {
                    edit = Some(Box::new(move |r, _| r.density = d));
                }
                ui.end_row();
            }
            RulerKind::Parallel | RulerKind::Radial | RulerKind::Symmetry => {
                ui.label(tr(if r.kind == RulerKind::Parallel { "ruler.origin" } else { "ruler.center" }));
                ui.horizontal(|ui| {
                    if let Some(x) = num(ui, base.with("cx"), r.center[0], 1.0, -1e6..=1e6, "") {
                        edit = Some(Box::new(move |r, _| r.center[0] = x));
                    }
                    if let Some(y) = num(ui, base.with("cy"), r.center[1], 1.0, -1e6..=1e6, "") {
                        edit = Some(Box::new(move |r, _| r.center[1] = y));
                    }
                });
                ui.end_row();
                ui.label(tr("ruler.angle"));
                if let Some(a) = num(ui, base.with("ang"), r.angle, 0.5, -360.0..=360.0, "°") {
                    edit = Some(Box::new(move |r, _| r.angle = a));
                }
                ui.end_row();
                match r.kind {
                    RulerKind::Parallel => {
                        ui.label(tr("ruler.spacing"));
                        if let Some(s) = num(ui, base.with("sp"), r.spacing, 0.5, 1.0..=10000.0, " px") {
                            edit = Some(Box::new(move |r, _| r.spacing = s));
                        }
                        ui.end_row();
                    }
                    RulerKind::Radial => {
                        ui.label(tr("ruler.rays"));
                        if let Some(n) = num_u(ui, base.with("cnt"), r.count, 2..=720) {
                            edit = Some(Box::new(move |r, _| r.count = n));
                        }
                        ui.end_row();
                    }
                    _ => {
                        ui.label(tr("ruler.axes"));
                        if let Some(n) = num_u(ui, base.with("cnt"), r.count, 1..=64) {
                            edit = Some(Box::new(move |r, _| r.count = n));
                        }
                        ui.end_row();
                        ui.label(tr("ruler.mirror"));
                        let mut m = r.mirror;
                        let resp = ui.checkbox(&mut m, "").on_hover_text(tr("ruler.mirror_tip"));
                        env.note("rulers:mirror", resp.rect);
                        if resp.changed() {
                            edit = Some(Box::new(move |r, _| {
                                r.mirror = m;
                                if !m && r.count < 2 {
                                    r.count = 2;
                                }
                            }));
                        }
                        ui.end_row();
                    }
                }
            }
            RulerKind::Linear => {
                for (i, key) in ["ruler.point_a", "ruler.point_b"].iter().enumerate() {
                    ui.label(tr(key));
                    ui.horizontal(|ui| {
                        let p = r.pts.get(i).copied().unwrap_or_default();
                        if let Some(x) = num(ui, base.with(("px", i)), p[0], 1.0, -1e6..=1e6, "") {
                            edit = Some(Box::new(move |r, _| r.pts[i][0] = x));
                        }
                        if let Some(y) = num(ui, base.with(("py", i)), p[1], 1.0, -1e6..=1e6, "") {
                            edit = Some(Box::new(move |r, _| r.pts[i][1] = y));
                        }
                    });
                    ui.end_row();
                }
                if r.pts.len() == 2 {
                    ui.label(tr("ruler.length"));
                    let d = [r.pts[1][0] - r.pts[0][0], r.pts[1][1] - r.pts[0][1]];
                    ui.label(format!("{:.1} px  ·  {:.1}°", d[0].hypot(d[1]), d[1].atan2(d[0]).to_degrees()));
                    ui.end_row();
                }
            }
            RulerKind::Curve => {
                ui.label(tr("ruler.points"));
                ui.label(format!("{}", r.pts.len()));
                ui.end_row();
                ui.label(tr("ruler.closed"));
                let mut c = r.closed;
                let resp = ui.add_enabled(r.pts.len() >= 3, egui::Checkbox::new(&mut c, "")).on_hover_text(tr("ruler.closed_tip"));
                env.note("rulers:closed", resp.rect);
                if resp.changed() {
                    edit = Some(Box::new(move |r, _| r.closed = c));
                }
                ui.end_row();
            }
            RulerKind::Figure | RulerKind::Concentric => {
                ui.label(tr("ruler.center"));
                ui.horizontal(|ui| {
                    if let Some(x) = num(ui, base.with("cx"), r.center[0], 1.0, -1e6..=1e6, "") {
                        edit = Some(Box::new(move |r, _| r.center[0] = x));
                    }
                    if let Some(y) = num(ui, base.with("cy"), r.center[1], 1.0, -1e6..=1e6, "") {
                        edit = Some(Box::new(move |r, _| r.center[1] = y));
                    }
                });
                ui.end_row();
                // largura × altura (a régua guarda os semi-eixos)
                ui.label(tr("ruler.size"));
                ui.horizontal(|ui| {
                    if let Some(w) = num(ui, base.with("sw"), r.size[0] * 2.0, 1.0, 1.0..=1e6, "") {
                        edit = Some(Box::new(move |r, _| r.size[0] = w / 2.0));
                    }
                    if let Some(h) = num(ui, base.with("sh"), r.size[1] * 2.0, 1.0, 1.0..=1e6, "") {
                        edit = Some(Box::new(move |r, _| r.size[1] = h / 2.0));
                    }
                });
                ui.end_row();
                ui.label(tr("ruler.angle"));
                if let Some(a) = num(ui, base.with("ang"), r.angle, 0.5, -360.0..=360.0, "°") {
                    edit = Some(Box::new(move |r, _| r.angle = a));
                }
                ui.end_row();
                if r.kind == RulerKind::Figure && r.shape == rl::FigureShape::Polygon {
                    ui.label(tr("ruler.sides"));
                    if let Some(n) = num_u(ui, base.with("sides"), r.count, 3..=64) {
                        edit = Some(Box::new(move |r, _| r.count = n));
                    }
                    ui.end_row();
                }
                if r.kind == RulerKind::Concentric {
                    ui.label(tr("ruler.spacing")).on_hover_text(tr("ruler.ring_spacing_tip"));
                    if let Some(sp) = num(ui, base.with("sp"), r.spacing, 0.5, 1.0..=10000.0, " px") {
                        edit = Some(Box::new(move |r, _| r.spacing = sp));
                    }
                    ui.end_row();
                }
            }
            RulerKind::Guide => {
                let vertical = (r.angle.rem_euclid(180.0) - 90.0).abs() < 45.0;
                ui.label(tr("ruler.orientation"));
                ui.horizontal(|ui| {
                    for (vert, key) in [(false, "ruler.horizontal"), (true, "ruler.vertical")] {
                        let b = ui.selectable_label(vertical == vert, tr(key));
                        env.note(format!("rulers:guide.{}", if vert { "v" } else { "h" }), b.rect);
                        if b.clicked() && vertical != vert {
                            edit = Some(Box::new(move |r, _| r.angle = if vert { 90.0 } else { 0.0 }));
                        }
                    }
                });
                ui.end_row();
                ui.label(tr("ruler.position"));
                let i = if vertical { 0 } else { 1 };
                if let Some(x) = num(ui, base.with("gpos"), r.center[i], 1.0, -1e6..=1e6, " px") {
                    edit = Some(Box::new(move |r, _| r.center[i] = x));
                }
                ui.end_row();
            }
            RulerKind::Grid => {
                for (i, key) in ["ruler.corner_tl", "ruler.corner_tr", "ruler.corner_br", "ruler.corner_bl"].iter().enumerate() {
                    ui.label(tr(key));
                    ui.horizontal(|ui| {
                        let p = r.quad[i];
                        if let Some(x) = num(ui, base.with(("qx", i)), p[0], 1.0, -1e6..=1e6, "") {
                            edit = Some(Box::new(move |r, _| r.quad[i][0] = x));
                        }
                        if let Some(y) = num(ui, base.with(("qy", i)), p[1], 1.0, -1e6..=1e6, "") {
                            edit = Some(Box::new(move |r, _| r.quad[i][1] = y));
                        }
                    });
                    ui.end_row();
                }
                ui.label(tr("ruler.cells"));
                ui.horizontal(|ui| {
                    if let Some(u) = num_u(ui, base.with("cu"), r.cells[0], 1..=200) {
                        edit = Some(Box::new(move |r, _| r.cells[0] = u));
                    }
                    if let Some(v) = num_u(ui, base.with("cv"), r.cells[1], 1..=200) {
                        edit = Some(Box::new(move |r, _| r.cells[1] = v));
                    }
                });
                ui.end_row();
                ui.label(tr("ruler.extend")).on_hover_text(tr("ruler.extend_tip"));
                if let Some(e) = num_u(ui, base.with("ext"), r.extend, 0..=100) {
                    edit = Some(Box::new(move |r, _| r.extend = e));
                }
                ui.end_row();
            }
        }
    });
    if r.locked {
        dim_label(ui, &t, tr("ruler.locked"));
    }
    if let Some(f) = edit {
        rulers::edit_active(env, f);
    }
    if let Some(k) = convert {
        rulers::convert_active(env, k);
    }
}

/// Opções da ferramenta Régua (barra do canvas e painel Ferramenta): o tipo da próxima régua e o encaixe.
pub(crate) fn tool_options(env: &mut Env, ui: &mut Ui, compact: bool) {
    let t = env.t;
    // com uma régua escolhida, o tipo é o dela e trocar converte a régua no lugar; sem régua, é o tipo da próxima
    let active = rulers::active_ruler(env);
    let cur = active.as_ref().map(NewKind::of).unwrap_or(env.ed.rulers.new_kind);
    let mut pick = None;
    let r = egui::ComboBox::from_id_salt(("ruler_new_kind", compact)).selected_text(tr(cur.key())).show_ui(ui, |ui| {
        for k in NewKind::types() {
            if ui.selectable_label(k == cur, tr(k.key())).clicked() {
                pick = Some(k);
            }
        }
    });
    env.note("opt:ruler_kind", r.response.rect);
    r.response.on_hover_text(tr(if active.is_some() { "ruler.kind_convert_tip" } else { "ruler.new_kind_tip" }));
    if let Some(k) = pick.filter(|k| *k != cur) {
        rulers::convert_active(env, k);
    } else if let Some(k) = pick {
        env.ed.rulers.new_kind = k;
    }
    let mut snap = env.ed.rulers.snap;
    let r = ui.checkbox(&mut snap, tr("ruler.snap")).on_hover_text(format!("{} ({})", tr("ruler.snap_tip_s25"), env.ed.keys.label(Action::RulerSnap)));
    env.note("opt:ruler_snap", r.rect);
    env.ed.rulers.snap = snap;
    // como criar o tipo escolhido (curva por cliques, caneta de régua à mão, figura, guia)
    let how = match env.ed.rulers.new_kind {
        NewKind::Curve => Some("ruler.curve_hint"),
        NewKind::RulerPen => Some("ruler.pen_hint"),
        NewKind::FigRect | NewKind::FigEllipse | NewKind::FigPolygon => Some("ruler.figure_hint"),
        NewKind::Guide => Some("ruler.guide_hint"),
        _ => None,
    };
    if let Some(k) = how {
        if compact {
            r.on_hover_text(tr(k));
        } else {
            dim_label(ui, &t, tr(k));
        }
    }
    if !compact {
        dim_label(ui, &t, tr("ruler.tool_hint"));
    }
}
