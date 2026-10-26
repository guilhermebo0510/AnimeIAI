//! Ferramenta Degradê no canvas (S25, à moda do Clip Studio). As sub-ferramentas são degradês prontos: cor principal →
//! transparente, principal → secundária, listra, listra com a secundária (vivos: seguem as cores em uso), os do programa,
//! céus, o degradê do painel Cores e os degradês guardados nas paletas; a aba "Degradê livre" põe pontos de cor.
//!
//! - Arrastar de A até B pinta o degradê (Shift = ângulo em passos, "Ajustar ângulo" das opções); forma linear, radial,
//!   angular ou losango; repetição prender/repetir/espelhar; opacidade; modo de mistura; inverter.
//! - Onde criar: "Na camada de edição" pinta pixels na camada raster ativa (só dentro da seleção; sem raster ativa, uma
//!   nova logo acima, como o pincel); "Camada de degradê" cria uma camada de degradê EDITÁVEL acima da ativa. Com uma
//!   camada de degradê ativa, as duas pontas aparecem no canvas: arrastar uma ponta move só ela (mantém as cores);
//!   arrastar fora das pontas refaz o degradê da camada com o pronto escolhido; o painel Propriedades edita as paradas.
//! - Degradê livre: clique põe um ponto com a cor de desenho, arrastar move, Delete tira o escolhido, Enter aplica (na
//!   raster ativa, ou numa raster nova com "Camada de degradê"), Esc limpa. A prévia mostra o campo de cor ao vivo.
//!
//! O resultado sai de `aiai_tools::grad_tool` (o mesmo pedido de `aiai_gradient`).

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::paint::builtin_gradients;
use aiai_core::{BlendMode, ColorRef, Gradient, GradientExtend, GradientShape, LayerType};
use aiai_paint::brush::GradientKind;
use aiai_tools::grad_tool::{COLOR_PRESETS, gradient_action, gradient_region, is_gradient_layer, preset_gradient, raster_command, set_layer_gradient, sky_presets};
use aiai_tools::paint::GradientRequest;
use egui::{Color32, Stroke, Ui, Vec2};

/// Degradê escolhido (sub-ferramenta).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GradPick {
    /// Pronto que segue as cores em uso (chave de `COLOR_PRESETS`).
    Color(&'static str),
    /// Pronto do programa ou céu, pelo nome.
    Named(&'static str),
    /// Degradê guardado numa paleta (paleta, id do degradê).
    Palette(NodeId, aiai_core::Id),
    /// O degradê do painel Cores.
    Current,
    /// Degradê livre (pontos de cor).
    Freeform,
}

/// Estado e opções da ferramenta.
pub(crate) struct GradTool {
    pub pick: GradPick,
    pub shape: GradientShape,
    pub extend: GradientExtend,
    pub opacity: f32,
    pub blend: BlendMode,
    /// Onde criar: camada de degradê editável (senão, pixels na camada de edição).
    pub editable: bool,
    /// Shift: passos de ângulo (graus).
    pub angle_step: f32,
    pub reverse: bool,
    /// Degradê livre: pontos (px do canvas) e cores.
    pub free: Vec<(Vec2, ColorRef)>,
    pub free_sel: Option<usize>,
}

impl Default for GradTool {
    fn default() -> Self {
        GradTool {
            pick: GradPick::Color("main_to_transparent"),
            shape: GradientShape::Linear,
            extend: GradientExtend::Pad,
            opacity: 1.0,
            blend: BlendMode::Normal,
            editable: false,
            angle_step: 45.0,
            reverse: false,
            free: Vec::new(),
            free_sel: None,
        }
    }
}

/// Raio (px da tela) para pegar uma ponta ou um ponto do degradê livre.
const GRAB_PX: f32 = 9.0;

/// Sub-ferramentas fixas da aba de prontos (as das paletas entram por `palette_picks`).
pub(crate) fn fixed_picks() -> Vec<GradPick> {
    let mut v: Vec<GradPick> = COLOR_PRESETS.iter().map(|(k, _)| GradPick::Color(k)).collect();
    v.extend(sky_presets().into_iter().map(|(n, _)| GradPick::Named(n)));
    v.extend(builtin_gradients().into_iter().map(|(n, _)| GradPick::Named(n)));
    v.push(GradPick::Current);
    v
}

/// Degradês das paletas do projeto (a lista acompanha as paletas).
pub(crate) fn palette_picks(env: &Env) -> Vec<GradPick> {
    let p = &env.core.project;
    p.roots.palettes.iter().filter_map(|id| cmds::palette_data(p, id).map(|pd| (id, pd))).flat_map(|(id, pd)| pd.gradients.iter().map(move |g| GradPick::Palette(id.clone(), g.id.clone()))).collect()
}

/// Chave de texto dos prontos com nome (o nome é o do `aiai_tools`, em pt; a chave i18n é ASCII).
const NAMED_KEYS: [(&str, &str); 14] = [
    ("Preto → branco", "black_white"),
    ("Pôr do sol", "sunset"),
    ("Céu de dia", "day_sky"),
    ("Ouro", "gold"),
    ("Prata", "silver"),
    ("Arco-íris", "rainbow"),
    ("Mahō shōjo", "maho_shojo"),
    ("Neon", "neon"),
    ("Fogo", "fire"),
    ("Cor → transparente", "white_transparent"),
    ("Brilho radial", "radial_glow"),
    ("Céu noturno", "night_sky"),
    ("Fim de tarde", "evening_sky"),
    ("Céu azul", "blue_sky"),
];

/// Chave i18n do pronto com nome.
pub(crate) fn named_key(n: &str) -> String {
    let slug = NAMED_KEYS.iter().find(|(name, _)| *name == n).map(|(_, k)| *k).unwrap_or("unknown");
    format!("grad.named.{slug}")
}

/// Chave estável da sub-ferramenta.
pub(crate) fn pick_key(p: &GradPick) -> String {
    match p {
        GradPick::Color(k) => format!("grad:{k}"),
        GradPick::Named(n) => format!("grad:{}", n.to_lowercase()),
        GradPick::Palette(pal, id) => format!("grad:pal:{pal}:{id}"),
        GradPick::Current => "grad:current".into(),
        GradPick::Freeform => "grad:freeform".into(),
    }
}

/// Nome da sub-ferramenta.
pub(crate) fn pick_label(env: &Env, p: &GradPick) -> String {
    match p {
        GradPick::Color(k) => tr(&format!("grad.preset.{k}")).to_owned(),
        GradPick::Named(n) => tr(&named_key(n)).to_owned(),
        GradPick::Palette(pal, id) => cmds::palette_data(&env.core.project, pal).and_then(|pd| pd.gradients.iter().find(|g| g.id == *id)).map(|g| g.name.clone()).unwrap_or_else(|| tr("grad.pick.missing").to_owned()),
        GradPick::Current => tr("grad.pick.current").to_owned(),
        GradPick::Freeform => tr("grad.pick.freeform").to_owned(),
    }
}

/// O degradê da sub-ferramenta (sem a geometria), com as cores em uso. `None` = degradê livre ou degradê sumido.
pub(crate) fn pick_gradient(env: &Env, p: &GradPick) -> Option<Gradient> {
    let (main, sub) = super::figure::main_sub(env);
    match p {
        GradPick::Color(k) => preset_gradient(k, &main, &sub),
        GradPick::Named(n) => preset_gradient(n, &main, &sub),
        GradPick::Palette(pal, id) => cmds::palette_data(&env.core.project, pal).and_then(|pd| pd.gradients.iter().find(|g| g.id == *id)).map(|g| g.gradient.clone()),
        GradPick::Current => Some(env.ed.grad.gradient.clone()),
        GradPick::Freeform => None,
    }
}

/// O degradê em uso (com Inverter).
fn current(env: &Env) -> Option<Gradient> {
    let g = pick_gradient(env, &env.ed.grad_tool.pick)?;
    Some(if env.ed.grad_tool.reverse { g.reversed() } else { g })
}

fn kind_of(s: GradientShape) -> GradientKind {
    match s {
        GradientShape::Linear => GradientKind::Linear,
        GradientShape::Radial => GradientKind::Radial,
        GradientShape::Angular => GradientKind::Angular,
        GradientShape::Diamond => GradientKind::Diamond,
    }
}

/// Camada de degradê ativa: (camada, pontas).
fn active_grad_layer(env: &mut Env) -> Option<(NodeId, [[f32; 2]; 2])> {
    let cut = env.ed.active_cut(env.core)?;
    let layer = env.ed.active_layer(env.core)?;
    let l = cmds::layer_data(&env.core.project, &layer)?;
    if !is_gradient_layer(l) {
        return None;
    }
    let (_, _, reg) = gradient_region(&env.core.project, &cut, &layer, env.ed.frame)?;
    Some((layer, reg.gradient.as_ref()?.absolute?))
}

// ------------------------------------------------------------------------------------------------ entrada

/// Aperto com a ferramenta Degradê.
pub(crate) fn press(env: &mut Env, c: Vec2, scale: f32) {
    if env.ed.grad_tool.pick == GradPick::Freeform {
        let near = env.ed.grad_tool.free.iter().position(|(p, _)| (*p - c).length() * scale <= GRAB_PX);
        let idx = match near {
            Some(i) => i,
            None => {
                env.ed.grad_tool.free.push((c, env.ed.color.clone()));
                env.ed.grad_tool.free.len() - 1
            }
        };
        env.ed.grad_tool.free_sel = Some(idx);
        env.ed.gesture = Gesture::GradFree { idx };
        return;
    }
    // pontas da camada de degradê ativa
    if let Some((_, ends)) = active_grad_layer(env) {
        let hit = (0..2).find(|i| (Vec2::new(ends[*i][0], ends[*i][1]) - c).length() * scale <= GRAB_PX);
        if let Some(i) = hit {
            let other = ends[1 - i];
            env.ed.gesture = Gesture::GradDrag { start: Vec2::new(other[0], other[1]), cur: c, handle: Some(i as u8) };
            return;
        }
    }
    env.ed.gesture = Gesture::GradDrag { start: c, cur: c, handle: None };
}

/// Movimento com o gesto do degradê. `true` = era dele.
pub(crate) fn moved(env: &mut Env, c: Vec2) -> bool {
    let (shift, step) = (env.ed.shift, env.ed.grad_tool.angle_step);
    match &mut env.ed.gesture {
        Gesture::GradDrag { start, cur, .. } => {
            *cur = if shift { super::figure::snap_angle(*start, c, step) } else { c };
            true
        }
        Gesture::GradFree { idx } => {
            let i = *idx;
            if let Some(p) = env.ed.grad_tool.free.get_mut(i) {
                p.0 = c;
            }
            true
        }
        _ => false,
    }
}

/// Soltar o arrasto (`cur` já com o encaixe do Shift).
pub(crate) fn release(env: &mut Env, start: Vec2, cur: Vec2, handle: Option<u8>, scale: f32) {
    if (cur - start).length() * scale < 3.0 {
        if handle.is_none() {
            env.ed.notify(tr("grad.hint.drag"));
        }
        return;
    }
    match handle {
        Some(i) => move_end(env, start, cur, i),
        None => apply_drag(env, start, cur),
    }
}

/// Move uma ponta da camada de degradê ativa (as cores ficam).
fn move_end(env: &mut Env, fixed: Vec2, moved: Vec2, i: u8) {
    let (Some(cut), Some(layer)) = (env.ed.active_cut(env.core), env.ed.active_layer(env.core)) else { return };
    let Some((_, _, reg)) = gradient_region(&env.core.project, &cut, &layer, env.ed.frame) else { return };
    let Some(mut g) = reg.gradient.clone() else { return };
    let (f, m) = ([fixed.x, fixed.y], [moved.x, moved.y]);
    g.absolute = Some(if i == 0 { [m, f] } else { [f, m] });
    match set_layer_gradient(&env.core.project, &cut, &layer, env.ed.frame, g) {
        Ok(c) => {
            env.ed.apply(env.core, c);
        }
        Err(e) => env.ed.notify(e.to_string()),
    }
}

/// Pedido do degradê da ferramenta de `a` até `b` na camada `layer`.
fn request(env: &Env, cut: &NodeId, layer: &NodeId, a: Vec2, b: Vec2, g: Option<Gradient>) -> GradientRequest {
    let o = &env.ed.grad_tool;
    GradientRequest {
        cut: cut.clone(),
        frame: env.ed.frame,
        layer: layer.clone(),
        kind: kind_of(o.shape),
        from: [a.x, a.y],
        to: [b.x, b.y],
        stops: Vec::new(),
        blend: o.blend,
        opacity: o.opacity.clamp(0.0, 1.0),
        gradient: g,
        preset: None,
        main_color: None,
        sub_color: None,
        extend: Some(o.extend),
        editable: o.editable,
        selection: None,
        freeform: Vec::new(),
    }
}

/// Camada raster para pintar: a ativa, ou uma nova logo acima (com aviso), como o pincel.
fn raster_target(env: &mut Env) -> Option<super::tools::Target> {
    let cut = env.ed.active_cut(env.core)?;
    let active = env.ed.active_layer(env.core);
    if !active.as_ref().and_then(|l| cmds::layer_data(&env.core.project, l)).is_some_and(|l| l.kind == LayerType::Raster) {
        match cmds::add_layer(env.core, &cut, LayerType::Raster, active.as_ref()) {
            Ok(id) => {
                let name = env.core.project.node(&id).map(|n| n.name.clone()).unwrap_or_default();
                env.ed.layer = Some(id);
                env.ed.cel_number = None;
                env.ed.notify(format!("{} \u{201c}{name}\u{201d}", tr("msg.auto_raster_layer")));
            }
            Err(e) => {
                env.ed.notify(e);
                return None;
            }
        }
    }
    super::tools::target(env, &[LayerType::Raster], "err.need_raster")
}

/// Roda o pedido: camada de degradê (nova ou a ativa refeita) ou pixels na raster.
fn run(env: &mut Env, mut req: GradientRequest) {
    let mask = super::tools::paint_clip_mask(env);
    if req.editable {
        let Some(cut) = env.ed.active_cut(env.core) else { return };
        if let Some(l) = env.ed.active_layer(env.core) {
            if cmds::layer_data(&env.core.project, &l).is_some_and(|d| d.locked && is_gradient_layer(d)) {
                return env.ed.notify(tr("err.layer_locked"));
            }
            req.layer = l;
        } else {
            req.layer = cut;
        }
        let core = &mut *env.core;
        let plan = gradient_action(&core.project, &mut core.blobs, &mut core.ids, &req, mask.as_ref());
        match plan {
            Ok(plan) => {
                if env.ed.apply(env.core, plan.command) {
                    if let Some(l) = plan.layer {
                        env.ed.layer = Some(l);
                        env.ed.cel_number = None;
                    }
                }
            }
            Err(e) => env.ed.notify(e.to_string()),
        }
        return;
    }
    let Some(t) = raster_target(env) else { return };
    req.layer = t.layer.clone();
    let planned = cmds::plan_with_prep(env.core, &t.cut, &t.layer, t.frame, env.ed.cel_number, vec![], |p, blobs| raster_command(p, blobs, &req, mask.as_ref()).map_err(|e| e.to_string()));
    match planned {
        Ok(c) => {
            env.ed.apply(env.core, c);
        }
        Err(e) => env.ed.notify(e),
    }
}

fn apply_drag(env: &mut Env, a: Vec2, b: Vec2) {
    let Some(cut) = env.ed.active_cut(env.core) else { return env.ed.notify(tr("err.no_cut")) };
    let Some(g) = current(env) else { return env.ed.notify(tr("grad.pick.missing")) };
    let req = request(env, &cut, &cut, a, b, Some(g));
    run(env, req);
}

/// Aplica o degradê livre (Enter) e limpa os pontos.
pub(crate) fn apply_freeform(env: &mut Env) {
    if env.ed.grad_tool.free.is_empty() {
        return env.ed.notify(tr("grad.free.empty"));
    }
    let Some(cut) = env.ed.active_cut(env.core) else { return env.ed.notify(tr("err.no_cut")) };
    let mut req = request(env, &cut, &cut, Vec2::ZERO, Vec2::new(1.0, 0.0), None);
    req.freeform = env.ed.grad_tool.free.iter().map(|(p, c)| aiai_tools::freeform::FreeformPoint { at: [p.x, p.y], color: c.clone() }).collect();
    req.extend = None;
    let before = env.core.change_counter;
    run(env, req);
    if env.core.change_counter != before {
        env.ed.grad_tool.free.clear();
        env.ed.grad_tool.free_sel = None;
    }
}

/// Teclas do degradê livre (antes dos atalhos gerais): Enter aplica, Esc limpa, Delete/Backspace tiram o ponto escolhido.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    if *env.tool != ToolId::Gradient || env.ed.grad_tool.pick != GradPick::Freeform || env.ed.grad_tool.free.is_empty() {
        return;
    }
    use egui::{Key, Modifiers};
    let (enter, esc, del) = ctx.input_mut(|i| {
        (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape), i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace))
    });
    let st = &mut env.ed.grad_tool;
    if del {
        if let Some(i) = st.free_sel.take().filter(|i| *i < st.free.len()) {
            st.free.remove(i);
        }
    }
    if esc {
        st.free.clear();
        st.free_sel = None;
    }
    if enter {
        apply_freeform(env);
    }
}

// ------------------------------------------------------------------------------------------------ prévia

/// Malha de `nx`×`ny` quadrados sobre o canvas com a cor de `f` em cada vértice (prévia barata do campo de cor).
fn field_mesh(painter: &egui::Painter, xf: &Xf, canvas: (u32, u32), alpha: f32, f: impl Fn([f32; 2]) -> [f32; 4]) {
    let (nx, ny) = (48u32, 28u32);
    let (w, h) = (canvas.0 as f32, canvas.1 as f32);
    let mut mesh = egui::Mesh::default();
    for j in 0..=ny {
        for i in 0..=nx {
            let p = Vec2::new(w * i as f32 / nx as f32, h * j as f32 / ny as f32);
            let c = f([p.x, p.y]);
            let col = super::ui_kit::rgba_to_color32(aiai_core::Rgba([c[0], c[1], c[2], (c[3] * alpha).clamp(0.0, 1.0)]));
            mesh.colored_vertex(xf.to_screen(p), col);
        }
    }
    let row = nx + 1;
    for j in 0..ny {
        for i in 0..nx {
            let k = j * row + i;
            mesh.add_triangle(k, k + 1, k + row);
            mesh.add_triangle(k + 1, k + row + 1, k + row);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn handle_dot(painter: &egui::Painter, p: egui::Pos2, accent: Color32, filled: Color32) {
    painter.circle(p, 6.0, filled, Stroke::new(2.0, Color32::WHITE));
    painter.circle_stroke(p, 7.5, Stroke::new(1.2, accent));
}

/// Desenho da ferramenta por cima do canvas: a prévia do arrasto, as pontas da camada de degradê ativa, os pontos e o
/// campo do degradê livre.
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &NodeId) {
    if *env.tool != ToolId::Gradient {
        return;
    }
    let canvas = cmds::canvas_size(&env.core.project, cut);
    let accent = env.t.accent;
    let p = &env.core.project;
    let resolve = |c: &ColorRef| cmds::resolve_color(p, c).0;
    if env.ed.grad_tool.pick == GradPick::Freeform {
        let pts: Vec<([f32; 2], [f32; 4])> = env.ed.grad_tool.free.iter().map(|(q, c)| ([q.x, q.y], resolve(c))).collect();
        if !pts.is_empty() {
            let field = aiai_tools::freeform::FreeField::new(&pts, (canvas.0 as f32).hypot(canvas.1 as f32));
            field_mesh(painter, xf, canvas, 0.9 * env.ed.grad_tool.opacity, |q| field.sample(q));
        }
        for (i, (q, c)) in pts.iter().enumerate() {
            let fill = super::ui_kit::rgba_to_color32(aiai_core::Rgba(*c));
            let s = xf.to_screen(Vec2::new(q[0], q[1]));
            handle_dot(painter, s, if env.ed.grad_tool.free_sel == Some(i) { accent } else { Color32::from_black_alpha(160) }, fill);
        }
        return;
    }
    let drag = match &env.ed.gesture {
        Gesture::GradDrag { start, cur, handle } => Some((*start, *cur, *handle)),
        _ => None,
    };
    let ends = match drag {
        Some((s, c, Some(0))) => Some((c, s)),
        Some((s, c, _)) => Some((s, c)),
        None => active_grad_layer(env).map(|(_, e)| (Vec2::new(e[0][0], e[0][1]), Vec2::new(e[1][0], e[1][1]))),
    };
    let Some((a, b)) = ends else { return };
    // prévia do degradê durante o arrasto (meio transparente, sem a seleção)
    if let Some((_, _, handle)) = drag {
        let g = match handle {
            Some(_) => {
                let cut = cut.clone();
                env.ed.layer.clone().and_then(|l| gradient_region(&env.core.project, &cut, &l, env.ed.frame).and_then(|(_, _, r)| r.gradient.clone()))
            }
            None => current(env).map(|g| Gradient { shape: env.ed.grad_tool.shape, extend: env.ed.grad_tool.extend, ..g }),
        };
        if let Some(g) = g {
            let g = Gradient { absolute: Some([[a.x, a.y], [b.x, b.y]]), ..g };
            let p = &env.core.project;
            let r = g.resolve(|c| cmds::resolve_color(p, c).0);
            let m = r.mapper([0.0, 0.0, canvas.0 as f32, canvas.1 as f32]);
            field_mesh(painter, xf, canvas, 0.65 * env.ed.grad_tool.opacity, |q| r.sample(&m, q));
        }
    }
    let (sa, sb) = (xf.to_screen(a), xf.to_screen(b));
    painter.line_segment([sa, sb], Stroke::new(3.0, Color32::from_black_alpha(150)));
    painter.line_segment([sa, sb], Stroke::new(1.5, Color32::WHITE));
    handle_dot(painter, sa, accent, accent);
    handle_dot(painter, sb, accent, Color32::from_gray(40));
}

// ------------------------------------------------------------------------------------------------ propriedades

fn shape_key(s: GradientShape) -> &'static str {
    match s {
        GradientShape::Linear => "grad.shape.linear",
        GradientShape::Radial => "grad.shape.radial",
        GradientShape::Angular => "grad.shape.angular",
        GradientShape::Diamond => "grad.shape.diamond",
    }
}

fn extend_key(e: GradientExtend) -> &'static str {
    match e {
        GradientExtend::Pad => "grad.extend.pad",
        GradientExtend::Repeat => "grad.extend.repeat",
        GradientExtend::Reflect => "grad.extend.reflect",
    }
}

/// Linhas do painel Propriedades da ferramenta.
pub(crate) fn prop_rows(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let free = env.ed.grad_tool.pick == GradPick::Freeform;
    // onde criar (o mais usado vem primeiro: o painel é baixo)
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(tr("grad.tool.where")).size(12.0));
        for (v, key, note) in [(false, "grad.tool.where.edit", "grad:tool:where:edit"), (true, if free { "grad.tool.where.new_raster" } else { "grad.tool.where.layer" }, "grad:tool:where:layer")] {
            let r = ui.selectable_label(env.ed.grad_tool.editable == v, tr(key));
            env.note(note, r.rect);
            if r.clicked() {
                env.ed.grad_tool.editable = v;
            }
        }
    });
    // amostra do degradê em uso
    if !free {
        if let Some(g) = current(env) {
            let r = super::panels::gradient_edit::swatch(env, ui, &g, egui::vec2(ui.available_width().min(240.0), 16.0), false);
            env.note("grad:tool:swatch", r.rect);
        }
        ui.horizontal_wrapped(|ui| {
            for s in [GradientShape::Linear, GradientShape::Radial, GradientShape::Angular, GradientShape::Diamond] {
                let r = ui.selectable_label(env.ed.grad_tool.shape == s, tr(shape_key(s)));
                env.note(format!("grad:tool:shape:{}", shape_key(s)), r.rect);
                if r.clicked() {
                    env.ed.grad_tool.shape = s;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("grad.tool.extend")).size(12.0));
            let o = &mut env.ed.grad_tool;
            egui::ComboBox::from_id_salt("grad_tool_extend").selected_text(tr(extend_key(o.extend))).show_ui(ui, |ui| {
                for e in [GradientExtend::Pad, GradientExtend::Repeat, GradientExtend::Reflect] {
                    ui.selectable_value(&mut o.extend, e, tr(extend_key(e)));
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("grad.tool.angle_step")).size(12.0)).on_hover_text(tr("grad.tool.angle_step.tip"));
            ui.add(egui::DragValue::new(&mut env.ed.grad_tool.angle_step).range(1.0..=90.0).speed(0.2).max_decimals(0).suffix("°"));
            ui.checkbox(&mut env.ed.grad_tool.reverse, tr("grad.tool.reverse"));
        });
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("grad.tool.opacity")).size(12.0));
        let mut v = env.ed.grad_tool.opacity * 100.0;
        if ui.add(egui::Slider::new(&mut v, 1.0..=100.0).max_decimals(0).suffix("%").trailing_fill(true)).changed() {
            env.ed.grad_tool.opacity = (v / 100.0).clamp(0.01, 1.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("grad.tool.blend")).size(12.0));
        let o = &mut env.ed.grad_tool;
        egui::ComboBox::from_id_salt("grad_tool_blend").selected_text(tr(cmds::blend_key(o.blend))).show_ui(ui, |ui| {
            for b in cmds::BLEND_MODES.into_iter().filter(|b| *b != BlendMode::PassThrough) {
                ui.selectable_value(&mut o.blend, b, tr(cmds::blend_key(b)));
            }
        });
    });
    if free {
        let n = env.ed.grad_tool.free.len();
        ui.label(egui::RichText::new(format!("{}: {n}", tr("grad.free.points"))).size(12.0));
        ui.horizontal(|ui| {
            let a = ui.add_enabled(n > 0, egui::Button::new(tr("grad.free.apply")));
            env.note("grad:free:apply", a.rect);
            if a.clicked() {
                apply_freeform(env);
            }
            let c = ui.add_enabled(n > 0, egui::Button::new(tr("grad.free.clear")));
            env.note("grad:free:clear", c.rect);
            if c.clicked() {
                env.ed.grad_tool.free.clear();
                env.ed.grad_tool.free_sel = None;
            }
        });
        // cor do ponto escolhido = a cor de desenho (clique no ponto e troque a cor)
        if let Some(i) = env.ed.grad_tool.free_sel.filter(|i| *i < n) {
            let b = ui.button(tr("grad.free.recolor"));
            env.note("grad:free:recolor", b.rect);
            if b.on_hover_text(tr("grad.free.recolor.tip")).clicked() {
                env.ed.grad_tool.free[i].1 = env.ed.color.clone();
            }
        }
        ui.label(egui::RichText::new(tr("grad.hint.free")).size(11.5).color(t.text_dim));
        return;
    }
    // camada de degradê ativa: as paradas dela no editor de sempre (um passo de desfazer por gesto)
    if let Some((layer, _)) = active_grad_layer(env) {
        let Some(cut) = env.ed.cut.clone() else { return };
        ui.separator();
        ui.label(egui::RichText::new(tr("grad.layer.title")).size(12.0).strong());
        if let Some(g) = gradient_region(&env.core.project, &cut, &layer, env.ed.frame).and_then(|(_, _, r)| r.gradient.clone()) {
            let id = egui::Id::new(("grad_layer_edit", layer.0.as_str()));
            let edited = super::panels::gradient_edit::buffered(ui, id, &g, |ui, v| {
                super::panels::gradient_edit::editor(env, ui, "grad_layer", v);
            });
            if let Some(ng) = edited {
                match set_layer_gradient(&env.core.project, &cut, &layer, env.ed.frame, ng) {
                    Ok(c) => {
                        env.ed.apply(env.core, c);
                    }
                    Err(e) => env.ed.notify(e.to_string()),
                }
            }
        }
        ui.label(egui::RichText::new(tr("grad.hint.layer")).size(11.5).color(t.text_dim));
    } else {
        ui.label(egui::RichText::new(tr("grad.hint.drag")).size(11.5).color(t.text_dim));
    }
}

// ------------------------------------------------------------------------------------------------ miniaturas

/// Miniatura da sub-ferramenta: a faixa do degradê (com as cores em uso), ou os pontos do degradê livre.
pub(crate) fn paint_thumb(env: &Env, p: &egui::Painter, r: egui::Rect, pick: &GradPick) {
    match pick_gradient(env, pick) {
        Some(g) => {
            let pr = &env.core.project;
            let rg = g.resolve(|c| cmds::resolve_color(pr, c).0);
            // a faixa sempre da esquerda para a direita (as listras repetem 4 vezes)
            let rep = matches!(g.extend, GradientExtend::Repeat | GradientExtend::Reflect);
            if rep {
                let n = 96;
                let mut mesh = egui::Mesh::default();
                for i in 0..=n {
                    let f = i as f32 / n as f32;
                    let c = super::ui_kit::rgba_to_color32(aiai_core::Rgba(rg.color_at(f * 4.0)));
                    let x = r.left() + r.width() * f;
                    mesh.colored_vertex(egui::pos2(x, r.top()), c);
                    mesh.colored_vertex(egui::pos2(x, r.bottom()), c);
                }
                for i in 0..n as u32 {
                    mesh.add_triangle(2 * i, 2 * i + 1, 2 * i + 2);
                    mesh.add_triangle(2 * i + 1, 2 * i + 3, 2 * i + 2);
                }
                p.rect_filled(r, 2.0, Color32::from_gray(170));
                p.add(egui::Shape::mesh(mesh));
            } else {
                super::panels::gradient_edit::paint_strip(p, r, &rg);
            }
            p.rect_stroke(r, 2.0, Stroke::new(1.0, env.t.border), egui::StrokeKind::Inside);
        }
        None if *pick == GradPick::Freeform => {
            let cols = [Color32::from_rgb(240, 90, 120), Color32::from_rgb(255, 210, 90), Color32::from_rgb(90, 170, 240)];
            let pts = [(0.15, 0.3), (0.5, 0.75), (0.85, 0.35)];
            let mut mesh = egui::Mesh::default();
            let (nx, ny) = (16, 4);
            let field = aiai_tools::freeform::FreeField::new(&pts.iter().zip(cols).map(|((x, y), c)| ([*x * 100.0, *y * 25.0], [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0])).collect::<Vec<_>>(), 103.0);
            for j in 0..=ny {
                for i in 0..=nx {
                    let (fx, fy) = (i as f32 / nx as f32, j as f32 / ny as f32);
                    let c = field.sample([fx * 100.0, fy * 25.0]);
                    mesh.colored_vertex(egui::pos2(r.left() + r.width() * fx, r.top() + r.height() * fy), super::ui_kit::rgba_to_color32(aiai_core::Rgba(c)));
                }
            }
            for j in 0..ny as u32 {
                for i in 0..nx as u32 {
                    let k = j * (nx as u32 + 1) + i;
                    mesh.add_triangle(k, k + 1, k + nx as u32 + 1);
                    mesh.add_triangle(k + 1, k + nx as u32 + 2, k + nx as u32 + 1);
                }
            }
            p.add(egui::Shape::mesh(mesh));
            for ((x, y), c) in pts.iter().zip(cols) {
                p.circle(egui::pos2(r.left() + r.width() * x, r.top() + r.height() * y), 3.5, c, Stroke::new(1.2, Color32::WHITE));
            }
        }
        None => {
            p.rect_stroke(r, 2.0, Stroke::new(1.0, env.t.text_dim), egui::StrokeKind::Inside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::commands::{add_layer, quick_start};
    use crate::workspace::WorkspaceId;
    use aiai_core::CoreSession;

    struct Doc {
        core: CoreSession,
        ed: Editor,
        icons: crate::icons::Icons,
        notes: Vec<(String, egui::Rect)>,
        tool: ToolId,
        cut: NodeId,
    }

    impl Doc {
        fn new() -> (Doc, NodeId) {
            let mut core = CoreSession::for_tests("grad");
            core.project.settings.width = 200;
            core.project.settings.height = 100;
            let cut = quick_start(&mut core, 4, None).unwrap();
            let l = add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
            let mut ed = Editor::default();
            ed.cut = Some(cut.clone());
            ed.layer = Some(l.clone());
            ed.color = ColorRef::Direct { rgba: aiai_core::Rgba([1.0, 0.0, 0.0, 1.0]) };
            (Doc { core, ed, icons: Default::default(), notes: Vec::new(), tool: ToolId::Gradient, cut }, l)
        }
        fn env(&mut self) -> Env<'_> {
            Env { t: crate::theme::Tokens::dark(), core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut self.notes, tool: &mut self.tool, ws: WorkspaceId::Pintura }
        }
        fn img(&self) -> aiai_paint::Image {
            aiai_render::render_frame(&self.core.project, &self.core.blobs, &self.cut, 0, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image
        }
    }

    /// Arrasto na camada de edição: pixels do "principal → transparente"; na camada de degradê: camada nova, depois a
    /// ponta arrastada move só ela.
    #[test]
    fn drag_paints_pixels_or_creates_and_edits_a_gradient_layer() {
        let (mut d, ras) = Doc::new();
        let mut env = d.env();
        press(&mut env, Vec2::new(0.0, 50.0), 1.0);
        assert!(moved(&mut env, Vec2::new(200.0, 50.0)));
        env.ed.gesture = Gesture::None;
        release(&mut env, Vec2::new(0.0, 50.0), Vec2::new(200.0, 50.0), None, 1.0);
        let img = d.img();
        assert!(img.get(2, 50)[0] > 0.95 && img.get(2, 50)[3] > 0.95 && img.get(197, 50)[3] < 0.1, "vermelho → transparente");
        assert!(cmds::layer_data(&d.core.project, &ras).is_some());
        // camada de degradê editável
        d.ed.grad_tool.editable = true;
        d.ed.grad_tool.pick = GradPick::Named("Céu noturno");
        let mut env = d.env();
        release(&mut env, Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0), None, 1.0);
        let gl = d.ed.layer.clone().unwrap();
        assert_ne!(gl, ras);
        assert!(is_gradient_layer(cmds::layer_data(&d.core.project, &gl).unwrap()));
        // pega a ponta de baixo e leva para a direita
        let mut env = d.env();
        press(&mut env, Vec2::new(101.0, 99.0), 1.0);
        assert!(matches!(env.ed.gesture, Gesture::GradDrag { handle: Some(1), .. }));
        let Gesture::GradDrag { start, .. } = std::mem::replace(&mut env.ed.gesture, Gesture::None) else { unreachable!() };
        release(&mut env, start, Vec2::new(190.0, 0.0), Some(1), 1.0);
        let (_, _, reg) = gradient_region(&d.core.project, &d.cut, &gl, 0).unwrap();
        assert_eq!(reg.gradient.as_ref().unwrap().absolute, Some([[100.0, 0.0], [190.0, 0.0]]));
    }

    /// Degradê livre: dois cliques põem pontos, Enter (aplicar) pinta e limpa.
    #[test]
    fn freeform_points_then_apply() {
        let (mut d, _) = Doc::new();
        d.ed.grad_tool.pick = GradPick::Freeform;
        let mut env = d.env();
        press(&mut env, Vec2::new(10.0, 50.0), 1.0);
        env.ed.gesture = Gesture::None;
        env.ed.color = ColorRef::Direct { rgba: aiai_core::Rgba([0.0, 0.0, 1.0, 1.0]) };
        press(&mut env, Vec2::new(190.0, 50.0), 1.0);
        env.ed.gesture = Gesture::None;
        // clicar perto de um ponto escolhe (não cria)
        press(&mut env, Vec2::new(12.0, 51.0), 1.0);
        env.ed.gesture = Gesture::None;
        assert_eq!((env.ed.grad_tool.free.len(), env.ed.grad_tool.free_sel), (2, Some(0)));
        apply_freeform(&mut env);
        assert!(env.ed.grad_tool.free.is_empty());
        let img = d.img();
        assert!(img.get(10, 50)[0] > 0.9 && img.get(190, 50)[2] > 0.9);
    }

    #[test]
    fn every_pick_has_a_name_and_a_unique_key() {
        use crate::i18n::{Lang, tr_in};
        let picks: Vec<GradPick> = fixed_picks().into_iter().chain([GradPick::Freeform]).collect();
        let keys: std::collections::HashSet<String> = picks.iter().map(pick_key).collect();
        assert_eq!(keys.len(), picks.len());
        for p in &picks {
            let k = match p {
                GradPick::Color(k) => format!("grad.preset.{k}"),
                GradPick::Named(n) => named_key(n),
                GradPick::Current => "grad.pick.current".into(),
                GradPick::Freeform => "grad.pick.freeform".into(),
                GradPick::Palette(..) => continue,
            };
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, &k), k, "{k} sem texto em {l:?}");
            }
        }
    }
}
