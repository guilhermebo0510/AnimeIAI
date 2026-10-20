//! Réguas no canvas (S24 2.1–2.2): as réguas da camada de guia (`aiai_tools::ruler`) desenhadas por cima do desenho (só
//! visão: nunca entram no render nem na exportação), a ferramenta Régua (`U`: arrastar no vazio cria uma régua do tipo
//! escolhido, arrastar uma alça edita: pontos de fuga, horizonte, centro, ângulo, cantos e meio da grade; Alt = só o
//! ponto), o encaixe dos traços da caneta e do pincel na régua ativa (a direção é escolhida pelo primeiro trecho, como no
//! Clip Studio, e o traço segue a reta pelo ponto de partida; na simetria o traço é repetido nas cópias) e os atalhos
//! (`Ctrl+;` mostra/oculta, `Ctrl+Shift+;` liga/desliga o encaixe, `Shift+U` passa para a régua seguinte).
//! Cada mudança grava o conjunto inteiro em `params.rulers` da camada (um passo de undo).

use super::commands::{self as cmds, NodeId};
use super::editor::*;
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::workspace::ToolId;
use aiai_core::*;
use aiai_tools::ruler::{self as rl, Handle, Ruler, RulerKind, RulerSet};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

/// Tipo da régua nova (ferramenta Régua e menu + do painel): a perspectiva aparece como 1, 2 e 3 pontos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewKind {
    P1,
    P2,
    P3,
    Parallel,
    Radial,
    Symmetry,
    Grid,
}

impl NewKind {
    pub(crate) const ALL: [NewKind; 7] = [NewKind::P1, NewKind::P2, NewKind::P3, NewKind::Parallel, NewKind::Radial, NewKind::Symmetry, NewKind::Grid];

    pub(crate) fn key(self) -> &'static str {
        match self {
            NewKind::P1 => "ruler.kind.p1",
            NewKind::P2 => "ruler.kind.p2",
            NewKind::P3 => "ruler.kind.p3",
            NewKind::Parallel => "ruler.kind.parallel",
            NewKind::Radial => "ruler.kind.radial",
            NewKind::Symmetry => "ruler.kind.symmetry",
            NewKind::Grid => "ruler.kind.grid",
        }
    }

    /// Nome curto estável (notas de UI da bridge).
    pub(crate) fn id(self) -> &'static str {
        self.key().rsplit('.').next().unwrap_or("")
    }

    pub(crate) fn kind_points(self) -> (RulerKind, usize) {
        match self {
            NewKind::P1 => (RulerKind::Perspective, 1),
            NewKind::P2 => (RulerKind::Perspective, 2),
            NewKind::P3 => (RulerKind::Perspective, 3),
            NewKind::Parallel => (RulerKind::Parallel, 2),
            NewKind::Radial => (RulerKind::Radial, 2),
            NewKind::Symmetry => (RulerKind::Symmetry, 2),
            NewKind::Grid => (RulerKind::Grid, 2),
        }
    }

    pub(crate) fn of(r: &Ruler) -> NewKind {
        match (r.kind, r.vps.len()) {
            (RulerKind::Perspective, 1) => NewKind::P1,
            (RulerKind::Perspective, 3) => NewKind::P3,
            (RulerKind::Perspective, _) => NewKind::P2,
            (RulerKind::Parallel, _) => NewKind::Parallel,
            (RulerKind::Radial, _) => NewKind::Radial,
            (RulerKind::Symmetry, _) => NewKind::Symmetry,
            (RulerKind::Grid, _) => NewKind::Grid,
        }
    }
}

/// Estado das réguas no editor (não vai para o documento).
pub(crate) struct RulerState {
    /// Linhas das réguas à mostra (`Ctrl+;`).
    pub show: bool,
    /// Encaixe geral (`Ctrl+Shift+;`); cada régua ainda tem o seu.
    pub snap: bool,
    pub new_kind: NewKind,
    /// Camada de guia das réguas escolhida por último (vale quando a camada ativa não é de guia).
    pub layer: Option<NodeId>,
    /// Régua em arrasto (ou sendo criada): a prévia aparece no lugar da gravada até soltar.
    pub preview: Option<(NodeId, Ruler)>,
    /// Alça sob o cursor (destaque).
    pub hover: Option<Handle>,
}

impl Default for RulerState {
    fn default() -> Self {
        RulerState { show: true, snap: true, new_kind: NewKind::P2, layer: None, preview: None, hover: None }
    }
}

/// Encaixe de um traço em andamento (pontos em px do canvas, antes de irem ao espaço do cel).
pub(crate) struct Snap {
    pub ruler: Ruler,
    pub start: Vec2,
    /// Direção escolhida pelo primeiro trecho; `None` até o traço andar `decide` px.
    pub dir: Option<Vec2>,
    pub decided: bool,
    /// Distância (px do canvas) que o primeiro trecho precisa andar para escolher a direção.
    pub decide: f32,
}

fn v(p: rl::P) -> Vec2 {
    Vec2::new(p[0], p[1])
}
fn a(p: Vec2) -> rl::P {
    [p.x, p.y]
}

impl Snap {
    /// O ponto do traço depois do encaixe.
    pub(crate) fn filter(&mut self, p: Vec2) -> Vec2 {
        if self.ruler.kind == RulerKind::Symmetry {
            return p;
        }
        if !self.decided {
            if (p - self.start).length() < self.decide {
                return self.start;
            }
            self.dir = rl::pick_dir(&self.ruler, a(self.start), a(p - self.start)).map(v);
            self.decided = true;
        }
        match self.dir {
            Some(d) => v(rl::project(a(self.start), a(d), a(p))),
            None => p,
        }
    }
}

// ------------------------------------------------------------------------------------------------ camada e leitura

fn guide_layers(p: &Project, cut: &NodeId) -> Vec<NodeId> {
    cmds::layer_rows(p, cut).into_iter().filter(|r| cmds::layer_data(p, &r.id).is_some_and(|l| l.kind == LayerType::Guide)).map(|r| r.id).collect()
}

/// Camadas de guia do corte ativo (para o seletor do painel).
pub(crate) fn guide_layers_of(env: &mut Env) -> Vec<NodeId> {
    match env.ed.active_cut(env.core) {
        Some(cut) => guide_layers(&env.core.project, &cut),
        None => Vec::new(),
    }
}

/// Camada de guia das réguas: a ativa quando é de guia; senão a escolhida por último (se ainda está no corte); senão a
/// primeira de guia com réguas; senão a primeira de guia.
pub(crate) fn ruler_layer(env: &mut Env) -> Option<NodeId> {
    let cut = env.ed.active_cut(env.core)?;
    let p = &env.core.project;
    let guides = guide_layers(p, &cut);
    if let Some(act) = env.ed.layer.clone().filter(|l| guides.contains(l)) {
        env.ed.rulers.layer = Some(act.clone());
        return Some(act);
    }
    if let Some(l) = env.ed.rulers.layer.clone().filter(|l| guides.contains(l)) {
        return Some(l);
    }
    let with = guides.iter().find(|g| cmds::layer_data(p, g).and_then(|l| rl::read_layer(l).ok()).is_some_and(|s| !s.items.is_empty()));
    with.or(guides.first()).cloned()
}

/// Réguas gravadas na camada (erro de formato vira texto, para o painel mostrar).
pub(crate) fn layer_set(p: &Project, layer: &NodeId) -> Result<RulerSet, String> {
    match cmds::layer_data(p, layer) {
        Some(l) => rl::read_layer(l),
        None => Err(tr("err.no_layer").to_owned()),
    }
}

/// A camada das réguas, criando uma camada de guia "Réguas" no topo do corte quando não há nenhuma.
pub(crate) fn ensure_layer(env: &mut Env) -> Option<NodeId> {
    if let Some(l) = ruler_layer(env) {
        return Some(l);
    }
    let cut = env.ed.active_cut(env.core)?;
    let top = cmds::layer_rows(&env.core.project, &cut).first().map(|r| r.id.clone());
    match cmds::add_layer(env.core, &cut, LayerType::Guide, top.as_ref()) {
        Ok(id) => {
            let cmd = Command::SetProps { id: id.clone(), name: Some(tr("ruler.layer_name").to_owned()), data: serde_json::Value::Null };
            let _ = cmds::run(env.core, cmd);
            env.ed.rulers.layer = Some(id.clone());
            Some(id)
        }
        Err(e) => {
            env.ed.notify(e);
            None
        }
    }
}

/// Grava o conjunto na camada (um passo de undo). Camada travada é recusada com aviso.
pub(crate) fn write(env: &mut Env, layer: &NodeId, set: &RulerSet, label_key: &str) -> bool {
    if cmds::layer_data(&env.core.project, layer).is_some_and(|l| l.locked) {
        env.ed.notify(tr("err.layer_locked"));
        return false;
    }
    if let Err(e) = set.validate() {
        env.ed.notify(e);
        return false;
    }
    env.ed.apply(env.core, rl::set_command(layer, set, tr(label_key)))
}

/// Régua nova do tipo `kind` posta em volta de `at` (ou do meio), com o nome traduzido; grava e ativa.
pub(crate) fn create(env: &mut Env, kind: NewKind, at: Option<Vec2>) -> Option<String> {
    let layer = ensure_layer(env)?;
    let cut = env.ed.active_cut(env.core)?;
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let mut set = match layer_set(&env.core.project, &layer) {
        Ok(s) => s,
        Err(e) => {
            env.ed.notify(e);
            return None;
        }
    };
    let (k, n) = kind.kind_points();
    let r = Ruler::preset(k, n, canvas, at.map(a), &set.next_id(), tr(kind.key()));
    let id = set.add(r);
    write(env, &layer, &set, "ruler.undo.create").then_some(id)
}

/// Muda a régua ativa da camada das réguas com `f` e grava (um passo de undo).
pub(crate) fn edit_active(env: &mut Env, f: impl FnOnce(&mut Ruler, (u32, u32))) {
    let (Some(layer), Some(cut)) = (ruler_layer(env), env.ed.active_cut(env.core)) else { return };
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let Ok(mut set) = layer_set(&env.core.project, &layer) else { return };
    let Some(id) = set.active_ruler().map(|r| r.id.clone()) else { return };
    let r = set.get_mut(&id).expect("existe");
    let before = r.clone();
    f(r, canvas);
    if *r != before {
        write(env, &layer, &set, "ruler.undo.edit");
    }
}

/// Régua ativa da camada das réguas, quando ela encaixa traços agora (encaixe geral e da régua ligados, régua e camada à
/// mostra).
pub(crate) fn snap_ruler(env: &mut Env) -> Option<Ruler> {
    if !env.ed.rulers.snap || !env.ed.rulers.show {
        return None;
    }
    let layer = ruler_layer(env)?;
    let l = cmds::layer_data(&env.core.project, &layer)?;
    if !l.visible {
        return None;
    }
    let set = rl::read_layer(l).ok()?;
    set.active_ruler().filter(|r| r.visible && r.snap).cloned()
}

/// Encaixe do traço que começa em `p` (px do canvas), para a caneta e o pincel.
pub(crate) fn stroke_snap(env: &mut Env, tool: ToolId, p: Vec2, scale: f32) -> Option<Snap> {
    if !matches!(tool, ToolId::Pen | ToolId::Brush) {
        return None;
    }
    let ruler = snap_ruler(env)?;
    Some(Snap { ruler, start: p, dir: None, decided: false, decide: 6.0 / scale.max(1e-3) })
}

/// Ponta da linha da caneta (forma Linha) encaixada na régua ativa: a direção mais perto do arrasto atual.
pub(crate) fn snap_line_end(env: &mut Env, start: Vec2, end: Vec2) -> Vec2 {
    if env.ed.opts.pen_shape != PenShape::Line || env.ed.shift {
        return end;
    }
    match snap_ruler(env).filter(|r| r.kind != RulerKind::Symmetry) {
        Some(r) => match rl::pick_dir(&r, a(start), a(end - start)) {
            Some(d) => v(rl::project(a(start), d, a(end))),
            None => end,
        },
        None => end,
    }
}

/// Cópias da simetria de pontos no espaço do cel do traço (`fwd`/`inv` = colocação da camada).
pub(crate) fn mirrored(g: &StrokeGesture, pts: &[Vec2]) -> Vec<Vec<Vec2>> {
    let Some(s) = g.snap.as_ref().filter(|s| s.ruler.kind == RulerKind::Symmetry) else { return Vec::new() };
    let to_cel = |p: Vec2| match &g.inv {
        Some(m) => {
            let (x, y) = m.apply(p.x, p.y);
            Vec2::new(x, y)
        }
        None => p,
    };
    rl::symmetry_maps(&s.ruler).iter().map(|m| pts.iter().map(|p| to_cel(v(rl::apply_map(m, a(g.to_canvas(*p)))))).collect()).collect()
}

// ------------------------------------------------------------------------------------------------ ferramenta

/// Comprimento da alça de ângulo: 70 pontos da tela, em px do canvas.
fn arm(xf: &Xf) -> f32 {
    70.0 / xf.scale.max(1e-3)
}

fn active_for_tool(env: &mut Env) -> Option<(NodeId, RulerSet, (u32, u32))> {
    let layer = ruler_layer(env)?;
    let cut = env.ed.active_cut(env.core)?;
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let set = layer_set(&env.core.project, &layer).ok()?;
    Some((layer, set, canvas))
}

/// Clique da ferramenta Régua: numa alça da régua ativa = arrasto da alça; no vazio = criação por arrasto.
pub(crate) fn press(env: &mut Env, c: Vec2, xf: &Xf) -> Gesture {
    if let Some((layer, set, canvas)) = active_for_tool(env) {
        if let Some(r) = set.active_ruler().filter(|r| r.visible).cloned() {
            if let Some(h) = rl::hit_handle(&r, canvas, arm(xf), a(c), 9.0 / xf.scale.max(1e-3)) {
                if r.locked {
                    env.ed.notify(tr("ruler.locked"));
                    return Gesture::None;
                }
                env.ed.rulers.preview = Some((layer.clone(), r.clone()));
                return Gesture::RulerDrag { layer, orig: Box::new(r), handle: h, start: c };
            }
        }
    }
    Gesture::RulerCreate { start: c, cur: c }
}

/// Régua que o arrasto de criação de `start` até `cur` faria (px do canvas).
pub(crate) fn creation_preview(env: &mut Env, start: Vec2, cur: Vec2) -> Option<Ruler> {
    let cut = env.ed.active_cut(env.core)?;
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let (w, h) = (canvas.0 as f32, canvas.1 as f32);
    let kind = env.ed.rulers.new_kind;
    let (k, n) = kind.kind_points();
    let mut r = Ruler::preset(k, n, canvas, Some(a(start)), "new", tr(kind.key()));
    let d = cur - start;
    let moved = d.length() > 1.0;
    match k {
        RulerKind::Perspective if n >= 2 => {
            // arrastar abre os pontos de fuga (pelo menos 0,6 da largura para cada lado)
            let spread = (d.length() * 2.5).max(0.6 * w);
            r.vps[0] = [start.x - spread, start.y];
            r.vps[1] = [start.x + spread, start.y];
            if n == 3 {
                r.vps[2] = [start.x, start.y + spread.max(1.5 * h)];
            }
        }
        RulerKind::Parallel | RulerKind::Symmetry if moved => {
            let mut ang = d.y.atan2(d.x).to_degrees();
            if k == RulerKind::Symmetry || k == RulerKind::Parallel {
                ang = ang.rem_euclid(180.0);
            }
            r.angle = ang;
        }
        RulerKind::Grid if moved && d.x.abs() > 4.0 && d.y.abs() > 4.0 => {
            // a caixa do arrasto vira um pedaço de chão: lado de cima mais estreito
            let (x0, y0, x1, y1) = (start.x.min(cur.x), start.y.min(cur.y), start.x.max(cur.x), start.y.max(cur.y));
            let inset = (x1 - x0) * 0.18;
            r.quad = [[x0 + inset, y0], [x1 - inset, y0], [x1, y1], [x0, y1]];
        }
        _ => {}
    }
    Some(r)
}

pub(crate) fn drag_move(env: &mut Env, c: Vec2) {
    let alt = env.ed.alt;
    if let Gesture::RulerDrag { layer, orig, handle, start } = &env.ed.gesture {
        let r = rl::drag(orig, *handle, a(*start), a(c), alt);
        env.ed.rulers.preview = Some((layer.clone(), r));
    }
}

/// Soltou a alça: grava a régua como ficou (nada muda = nada gravado).
pub(crate) fn drag_release(env: &mut Env, layer: NodeId, orig: Ruler) {
    let Some((_, r)) = env.ed.rulers.preview.take() else { return };
    if r == orig {
        return;
    }
    let Ok(mut set) = layer_set(&env.core.project, &layer) else { return };
    if let Some(slot) = set.get_mut(&r.id) {
        *slot = r;
        write(env, &layer, &set, "ruler.undo.edit");
    }
}

/// Soltou o arrasto de criação: arrasto de verdade cria; clique simples cria só quando a camada não tem régua nenhuma
/// (com réguas, um clique perdido perto de uma alça não deve criar outra).
pub(crate) fn create_release(env: &mut Env, start: Vec2, cur: Vec2, scale: f32) {
    let dragged = (cur - start).length() * scale >= 8.0;
    let empty = active_for_tool(env).is_none_or(|(_, s, _)| s.items.is_empty());
    if !dragged && !empty {
        return;
    }
    let Some(mut r) = creation_preview(env, start, cur) else { return };
    let Some(layer) = ensure_layer(env) else { return };
    let mut set = match layer_set(&env.core.project, &layer) {
        Ok(s) => s,
        Err(e) => {
            env.ed.notify(e);
            return;
        }
    };
    r.id = set.next_id();
    set.add(r);
    write(env, &layer, &set, "ruler.undo.create");
}

/// Atalhos das réguas (rodam com as teclas do editor).
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    use crate::keymap::Action as A;
    let keys = env.ed.keys.clone();
    let tool_ruler = *env.tool == ToolId::Ruler;
    let (snap, show, next, del) = ctx.input_mut(|i| {
        let mut hit = |x: A| keys.get(x).is_some_and(|c| c.take(i));
        let (s, sh, n) = (hit(A::RulerSnap), hit(A::RulersShow), hit(A::RulerNext));
        let d = tool_ruler && (i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace));
        (s, sh, n, d)
    });
    if snap {
        env.ed.rulers.snap = !env.ed.rulers.snap;
        env.ed.notify(tr(if env.ed.rulers.snap { "ruler.snap_on" } else { "ruler.snap_off" }));
    }
    if show {
        env.ed.rulers.show = !env.ed.rulers.show;
    }
    if next {
        if let Some(layer) = ruler_layer(env) {
            if let Ok(mut set) = layer_set(&env.core.project, &layer) {
                if set.items.len() > 1 {
                    set.cycle();
                    write(env, &layer, &set, "ruler.undo.activate");
                }
            }
        }
    }
    if del {
        delete_active(env);
    }
}

pub(crate) fn delete_active(env: &mut Env) {
    let Some(layer) = ruler_layer(env) else { return };
    let Ok(mut set) = layer_set(&env.core.project, &layer) else { return };
    let Some(id) = set.active_ruler().map(|r| r.id.clone()) else { return };
    set.remove(&id);
    write(env, &layer, &set, "ruler.undo.delete");
}

/// "Converter em traços": as linhas da régua ativa viram traços no cel da camada de guia no quadro atual.
pub(crate) fn bake_active(env: &mut Env) {
    let (Some(layer), Some(cut)) = (ruler_layer(env), env.ed.active_cut(env.core)) else { return };
    let Ok(set) = layer_set(&env.core.project, &layer) else { return };
    let Some(r) = set.active_ruler().cloned() else { return };
    if cmds::layer_data(&env.core.project, &layer).is_some_and(|l| l.locked) {
        env.ed.notify(tr("err.layer_locked"));
        return;
    }
    let (frame, pref) = (env.ed.frame, env.ed.cel_number);
    let planned = cmds::plan_with_prep(env.core, &cut, &layer, frame, pref, vec![], |p, _| rl::bake_command(p, &cut, frame, &layer, &r).map_err(|e| e.to_string()));
    match planned {
        Ok(cmd) => {
            if env.ed.apply(env.core, cmd) && !env.ed.show_guides {
                env.ed.show_guides = true;
            }
        }
        Err(e) => env.ed.notify(e),
    }
}

/// Grade de chão coerente com a perspectiva ativa (2 ou 3 pontos): nova régua de grade, ativa.
pub(crate) fn floor_grid(env: &mut Env) {
    let (Some(layer), Some(cut)) = (ruler_layer(env), env.ed.active_cut(env.core)) else { return };
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let Ok(mut set) = layer_set(&env.core.project, &layer) else { return };
    let Some(q) = set.active_ruler().and_then(|r| rl::floor_quad(r, canvas)) else {
        env.ed.notify(tr("ruler.floor_needs_two"));
        return;
    };
    let mut g = Ruler::preset(RulerKind::Grid, 2, canvas, None, &set.next_id(), tr("ruler.floor_name"));
    g.quad = q;
    g.extend = 12;
    set.add(g);
    write(env, &layer, &set, "ruler.undo.create");
}

// ------------------------------------------------------------------------------------------------ desenho

fn color_of(r: &Ruler, k: f32) -> Color32 {
    let c = r.rgba().0;
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgba_unmultiplied(q(c[0]), q(c[1]), q(c[2]), q(c[3] * k))
}

/// Linhas das réguas (todas as camadas de guia visíveis do corte) e, com a ferramenta Régua, as alças da régua ativa.
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, rect: Rect, xf: &Xf, cut: &NodeId, canvas: (u32, u32)) {
    // arrasto cancelado (Esc, soltou fora): a prévia não fica no lugar da régua gravada
    if !matches!(env.ed.gesture, Gesture::RulerDrag { .. }) {
        env.ed.rulers.preview = None;
    }
    let creating = match &env.ed.gesture {
        Gesture::RulerCreate { start, cur } => Some((*start, *cur)),
        _ => None,
    };
    if !env.ed.rulers.show && creating.is_none() {
        return;
    }
    // recorte = o que a área do canvas mostra, em px do canvas
    let ends = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].map(|p| xf.to_canvas(p));
    let clip = [ends.iter().map(|e| e.x).fold(f32::MAX, f32::min), ends.iter().map(|e| e.y).fold(f32::MAX, f32::min), ends.iter().map(|e| e.x).fold(f32::MIN, f32::max), ends.iter().map(|e| e.y).fold(f32::MIN, f32::max)];
    let main = ruler_layer(env);
    let tool = *env.tool == ToolId::Ruler;
    let preview = env.ed.rulers.preview.clone();
    let mut active: Option<Ruler> = None;
    for layer in guide_layers(&env.core.project, cut) {
        let Some(l) = cmds::layer_data(&env.core.project, &layer) else { continue };
        if !l.visible || !env.ed.rulers.show {
            continue;
        }
        let Ok(set) = rl::read_layer(l) else { continue };
        let act_id = set.active_ruler().map(|r| r.id.clone());
        for r in &set.items {
            let r = match &preview {
                Some((pl, pr)) if *pl == layer && pr.id == r.id => pr,
                _ => r,
            };
            if !r.visible {
                continue;
            }
            let is_active = Some(&layer) == main.as_ref() && act_id.as_deref() == Some(r.id.as_str());
            if is_active {
                active = Some(r.clone());
            }
            paint_lines(painter, xf, r, canvas, clip, if is_active { 1.0 } else { 0.55 });
        }
    }
    if let Some((s, c)) = creating {
        if let Some(r) = creation_preview(env, s, c) {
            paint_lines(painter, xf, &r, canvas, clip, 1.0);
        }
    }
    // alças da régua ativa (ferramenta Régua)
    env.ed.rulers.hover = None;
    if let (true, Some(r)) = (tool, active) {
        let hover = env.ed.cursor.filter(|p| rect.contains(*p) && matches!(env.ed.gesture, Gesture::None)).and_then(|p| rl::hit_handle(&r, canvas, arm(xf), a(xf.to_canvas(p)), 9.0 / xf.scale.max(1e-3)));
        env.ed.rulers.hover = hover;
        let accent = env.t.accent;
        for (h, q) in rl::handles(&r, canvas, arm(xf)) {
            let p = xf.to_screen(v(q));
            let hot = hover == Some(h) || matches!(&env.ed.gesture, Gesture::RulerDrag { handle, .. } if *handle == h);
            let fill = if hot { accent } else { Color32::WHITE };
            let line = Stroke::new(1.5, if r.locked { Color32::GRAY } else { accent });
            if h == Handle::Angle {
                if let Some(cq) = rl::handles(&r, canvas, arm(xf)).iter().find(|x| x.0 == Handle::Center).map(|x| x.1) {
                    painter.line_segment([xf.to_screen(v(cq)), p], Stroke::new(1.0, accent));
                }
            }
            if !rect.contains(p) {
                // ponto de fuga fora da vista: marcador na borda apontando para ele
                if let Handle::Vp(i) = h {
                    let ctr = rect.center();
                    let d = (p - ctr).normalized();
                    let t = edge_t(rect.shrink(10.0), ctr, d);
                    let e = ctr + d * t;
                    let n = egui::vec2(-d.y, d.x);
                    painter.add(egui::Shape::convex_polygon(vec![e + d * 8.0, e - d * 4.0 + n * 6.0, e - d * 4.0 - n * 6.0], fill, line));
                    painter.text(e - d * 14.0, egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(10.0), accent);
                    env.note(format!("ruler:handle:{}", h.key()), Rect::from_center_size(e, egui::vec2(16.0, 16.0)));
                }
                continue;
            }
            match h {
                Handle::Vp(i) => {
                    painter.circle(p, 6.5, fill, line);
                    painter.text(p + egui::vec2(9.0, -9.0), egui::Align2::LEFT_BOTTOM, format!("{}", i + 1), egui::FontId::proportional(10.5), accent);
                }
                Handle::Angle => {
                    let s = 5.5;
                    painter.add(egui::Shape::convex_polygon(vec![p + egui::vec2(0.0, -s), p + egui::vec2(s, 0.0), p + egui::vec2(0.0, s), p + egui::vec2(-s, 0.0)], fill, line));
                }
                _ => {
                    painter.rect(Rect::from_center_size(p, egui::vec2(10.0, 10.0)), 1.0, fill, line, egui::StrokeKind::Inside);
                }
            }
            env.note(format!("ruler:handle:{}", h.key()), Rect::from_center_size(p, egui::vec2(16.0, 16.0)));
        }
    }
    // traço encaixando: a reta escolhida (fina) e as cópias da simetria
    if let Gesture::Stroke(g) = &env.ed.gesture {
        if let Some(s) = &g.snap {
            if let Some(d) = s.dir {
                if let Some((p, q)) = rl::clip_param(a(s.start), a(d), -1e8, 1e8, clip) {
                    painter.add(egui::Shape::dashed_line(&[xf.to_screen(v(p)), xf.to_screen(v(q))], Stroke::new(1.0, color_of(&s.ruler, 0.9)), 6.0, 4.0));
                }
            }
            let pts: Vec<Vec2> = g.samples.iter().map(|x| x.p).collect();
            for copy in mirrored(g, &pts) {
                let sp: Vec<Pos2> = copy.iter().map(|p| xf.to_screen(g.to_canvas(*p))).collect();
                if sp.len() >= 2 {
                    painter.add(egui::Shape::line(sp, Stroke::new(1.5, color_of(&s.ruler, 1.0))));
                }
            }
        }
    }
}

/// Distância do centro até a borda do retângulo na direção `d`.
fn edge_t(r: Rect, c: Pos2, d: Vec2) -> f32 {
    let tx = if d.x > 1e-6 { (r.right() - c.x) / d.x } else if d.x < -1e-6 { (r.left() - c.x) / d.x } else { f32::MAX };
    let ty = if d.y > 1e-6 { (r.bottom() - c.y) / d.y } else if d.y < -1e-6 { (r.top() - c.y) / d.y } else { f32::MAX };
    tx.min(ty).max(0.0)
}

fn paint_lines(painter: &egui::Painter, xf: &Xf, r: &Ruler, canvas: (u32, u32), clip: [f32; 4], k: f32) {
    let thin = Stroke::new(1.0, color_of(r, k * 0.75));
    let strong = Stroke::new(1.8, color_of(r, k));
    for l in rl::lines(r, canvas, clip) {
        let seg = [xf.to_screen(v(l.a)), xf.to_screen(v(l.b))];
        if l.strong && r.kind == RulerKind::Perspective {
            // horizonte tracejado
            painter.add(egui::Shape::dashed_line(&seg, strong, 10.0, 5.0));
        } else {
            painter.line_segment(seg, if l.strong { strong } else { thin });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_point() -> Ruler {
        let mut r = Ruler::preset(RulerKind::Perspective, 2, (1920, 1080), Some([960.0, 500.0]), "r1", "");
        r.vps = vec![[-1000.0, 500.0], [3000.0, 500.0]];
        r
    }

    /// O traço espera o primeiro trecho (6 px da tela), escolhe a direção do ponto de fuga e todos os pontos depois
    /// caem na reta que vai da partida até ele.
    #[test]
    fn a_stroke_waits_for_its_first_segment_then_follows_the_vanishing_line() {
        let r = two_point();
        let s = Vec2::new(1000.0, 800.0);
        let mut snap = Snap { ruler: r.clone(), start: s, dir: None, decided: false, decide: 6.0 };
        assert_eq!(snap.filter(Vec2::new(1002.0, 799.0)), s, "antes de decidir fica na partida");
        let p = snap.filter(Vec2::new(990.0, 797.0));
        let to_vp1 = (Vec2::new(-1000.0, 500.0) - s).normalized();
        assert!(snap.dir.is_some_and(|d| d.dot(to_vp1).abs() > 0.9999));
        // a mão desvia: o ponto continua na reta
        for q in [p, snap.filter(Vec2::new(800.0, 790.0)), snap.filter(Vec2::new(600.0, 640.0))] {
            let off = (q - s).x * to_vp1.y - (q - s).y * to_vp1.x;
            assert!(off.abs() < 1e-2, "fora da reta: {q:?}");
        }
        // simetria não mexe no ponto
        let sym = Ruler::preset(RulerKind::Symmetry, 2, (1920, 1080), None, "r2", "");
        let mut ss = Snap { ruler: sym, start: s, dir: None, decided: false, decide: 6.0 };
        assert_eq!(ss.filter(Vec2::new(1234.0, 567.0)), Vec2::new(1234.0, 567.0));
    }

    #[test]
    fn new_kinds_round_trip_through_the_ruler() {
        for k in NewKind::ALL {
            let (kind, n) = k.kind_points();
            let r = Ruler::preset(kind, n, (1920, 1080), None, "r1", "");
            assert_eq!(NewKind::of(&r), k);
            r.validate().unwrap();
        }
    }
}
