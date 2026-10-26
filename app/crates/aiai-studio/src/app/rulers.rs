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
use aiai_tools::ruler::{self as rl, FigureShape, Handle, Ruler, RulerKind, RulerSet};
use egui::{Color32, Pos2, Rect, Stroke, Vec2};

/// Tipo da régua nova (ferramenta Régua, sub-ferramentas da prateleira e menu + do painel), na ordem do Clip Studio:
/// linear, curva, figura (retângulo, elipse, polígono), caneta de régua, especiais (paralelas, radial, concêntricos),
/// guia, perspectiva de 1, 2 e 3 pontos, simetria e grade em perspectiva.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewKind {
    Linear,
    Curve,
    FigRect,
    FigEllipse,
    FigPolygon,
    /// Caneta de régua: desenha à mão e o caminho vira uma régua curva.
    RulerPen,
    Parallel,
    Radial,
    Concentric,
    Guide,
    P1,
    P2,
    P3,
    Symmetry,
    Grid,
}

impl NewKind {
    pub(crate) const ALL: [NewKind; 15] = [
        NewKind::Linear,
        NewKind::Curve,
        NewKind::FigRect,
        NewKind::FigEllipse,
        NewKind::FigPolygon,
        NewKind::RulerPen,
        NewKind::Parallel,
        NewKind::Radial,
        NewKind::Concentric,
        NewKind::Guide,
        NewKind::P1,
        NewKind::P2,
        NewKind::P3,
        NewKind::Symmetry,
        NewKind::Grid,
    ];

    /// Os tipos que uma régua pode ter (a caneta de régua é só um jeito de criar a curva).
    pub(crate) fn types() -> impl Iterator<Item = NewKind> {
        NewKind::ALL.into_iter().filter(|k| *k != NewKind::RulerPen)
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            NewKind::Linear => "ruler.kind.linear",
            NewKind::Curve => "ruler.kind.curve",
            NewKind::FigRect => "ruler.kind.fig_rect",
            NewKind::FigEllipse => "ruler.kind.fig_ellipse",
            NewKind::FigPolygon => "ruler.kind.fig_polygon",
            NewKind::RulerPen => "ruler.kind.pen",
            NewKind::Parallel => "ruler.kind.parallel",
            NewKind::Radial => "ruler.kind.radial",
            NewKind::Concentric => "ruler.kind.concentric",
            NewKind::Guide => "ruler.kind.guide",
            NewKind::P1 => "ruler.kind.p1",
            NewKind::P2 => "ruler.kind.p2",
            NewKind::P3 => "ruler.kind.p3",
            NewKind::Symmetry => "ruler.kind.symmetry",
            NewKind::Grid => "ruler.kind.grid",
        }
    }

    /// Nome curto estável (notas de UI da bridge, chave da sub-ferramenta).
    pub(crate) fn id(self) -> &'static str {
        self.key().rsplit('.').next().unwrap_or("")
    }

    pub(crate) fn kind_points(self) -> (RulerKind, usize) {
        match self {
            NewKind::Linear => (RulerKind::Linear, 2),
            NewKind::Curve | NewKind::RulerPen => (RulerKind::Curve, 2),
            NewKind::FigRect | NewKind::FigEllipse | NewKind::FigPolygon => (RulerKind::Figure, 2),
            NewKind::Concentric => (RulerKind::Concentric, 2),
            NewKind::Guide => (RulerKind::Guide, 2),
            NewKind::P1 => (RulerKind::Perspective, 1),
            NewKind::P2 => (RulerKind::Perspective, 2),
            NewKind::P3 => (RulerKind::Perspective, 3),
            NewKind::Parallel => (RulerKind::Parallel, 2),
            NewKind::Radial => (RulerKind::Radial, 2),
            NewKind::Symmetry => (RulerKind::Symmetry, 2),
            NewKind::Grid => (RulerKind::Grid, 2),
        }
    }

    /// Figura da régua de figura deste tipo.
    pub(crate) fn shape(self) -> Option<FigureShape> {
        match self {
            NewKind::FigRect => Some(FigureShape::Rect),
            NewKind::FigEllipse => Some(FigureShape::Ellipse),
            NewKind::FigPolygon => Some(FigureShape::Polygon),
            _ => None,
        }
    }

    /// Régua nova deste tipo em volta de `at` (ou do meio do canvas).
    pub(crate) fn make(self, canvas: (u32, u32), at: Option<rl::P>, id: &str, name: &str) -> Ruler {
        let (k, n) = self.kind_points();
        let mut r = Ruler::preset(k, n, canvas, at, id, name);
        self.apply_shape(&mut r);
        r
    }

    /// Põe a figura do tipo na régua (polígono regular: caixa quadrada e pelo menos 3 lados).
    pub(crate) fn apply_shape(self, r: &mut Ruler) {
        if let Some(s) = self.shape() {
            r.shape = s;
            if s == FigureShape::Polygon {
                let m = r.size[0].min(r.size[1]);
                r.size = [m, m];
                if r.count < 3 {
                    r.count = 5;
                }
            }
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
            (RulerKind::Linear, _) => NewKind::Linear,
            (RulerKind::Curve, _) => NewKind::Curve,
            (RulerKind::Figure, _) => match r.shape {
                FigureShape::Rect => NewKind::FigRect,
                FigureShape::Ellipse => NewKind::FigEllipse,
                FigureShape::Polygon => NewKind::FigPolygon,
            },
            (RulerKind::Concentric, _) => NewKind::Concentric,
            (RulerKind::Guide, _) => NewKind::Guide,
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
    /// Régua curva sendo posta por cliques (px do canvas): Enter ou duplo clique termina, Esc cancela.
    pub pending: Vec<Vec2>,
    /// Último clique da curva (duplo clique).
    pub last_click: Option<(std::time::Instant, Vec2)>,
    /// Caminho da caneta de régua em andamento (px do canvas).
    pub pen_path: Vec<Vec2>,
}

impl Default for RulerState {
    fn default() -> Self {
        RulerState { show: true, snap: true, new_kind: NewKind::P2, layer: None, preview: None, hover: None, pending: Vec::new(), last_click: None, pen_path: Vec::new() }
    }
}

/// Encaixe numa régua objeto (linear, curva, figura, guia): o caminho e onde o traço está nele.
pub(crate) struct ObjSnap {
    pub track: rl::Track,
    /// Parâmetro (comprimento de arco) do último ponto encaixado.
    pub s: f32,
    /// Último ponto da mão (px do canvas).
    pub last: Vec2,
    /// Folga da janela de continuidade (px do canvas): o traço só procura o ponto mais perto até aqui além do que a
    /// mão andou, para não saltar para outra parte do caminho que passa perto.
    pub slack: f32,
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
    /// Caneta (vetor): o traço só avança na reta (a mão que volta não dobra o traço sobre si); o pincel vai e volta.
    pub forward: bool,
    /// Quanto o traço já avançou na reta desde a partida (px do canvas, no sentido do primeiro trecho).
    pub reach: f32,
    /// Régua objeto: o traço vai para o caminho dela (no lugar da reta pela partida).
    pub object: Option<ObjSnap>,
}

fn v(p: rl::P) -> Vec2 {
    Vec2::new(p[0], p[1])
}
fn a(p: Vec2) -> rl::P {
    [p.x, p.y]
}

impl Snap {
    pub(crate) fn new(ruler: Ruler, start: Vec2, decide: f32, forward: bool) -> Snap {
        Snap { ruler, start, dir: None, decided: false, decide, forward, reach: 0.0, object: None }
    }

    /// Encaixe na régua objeto `ruler` (caminho `track`, partida no parâmetro `s`).
    pub(crate) fn object(ruler: Ruler, start: Vec2, track: rl::Track, s: f32, slack: f32) -> Snap {
        let mut x = Snap::new(ruler, start, 0.0, false);
        x.object = Some(ObjSnap { track, s, last: start, slack });
        x
    }

    /// O ponto do traço depois do encaixe.
    pub(crate) fn filter(&mut self, p: Vec2) -> Vec2 {
        if let Some(o) = self.object.as_mut() {
            // o ponto mais perto do caminho, perto de onde o traço já está (não pula para outra parte da curva)
            let win = (p - o.last).length() * 2.0 + o.slack;
            let (q, s) = o.track.nearest_near(a(p), o.s, win);
            o.s = s;
            o.last = p;
            return v(q);
        }
        match self.ruler.kind {
            RulerKind::Symmetry => return p,
            RulerKind::Concentric => return v(rl::concentric_project(&self.ruler, a(self.start), a(p))),
            _ => {}
        }
        if !self.decided {
            if (p - self.start).length() < self.decide {
                return self.start;
            }
            // a direção aponta para onde o primeiro trecho foi
            self.dir = rl::pick_dir(&self.ruler, a(self.start), a(p - self.start)).map(|d| if (p - self.start).dot(v(d)) < 0.0 { -v(d) } else { v(d) });
            self.decided = true;
        }
        match self.dir {
            Some(d) if self.forward => {
                // ponto exato na reta (partida + d·t), sem o arredondamento da projeção, e só para a frente
                self.reach = self.reach.max((p - self.start).dot(d));
                self.start + d * self.reach
            }
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

/// Converte a régua ativa (a escolhida) para o tipo `k` no lugar (`rl::convert`: posição, horizonte e cor ficam onde fazem
/// sentido), num passo de undo; o nome padrão do tipo antigo vira o do novo. Também passa a ser o tipo das réguas novas.
/// `false` = não há régua ativa (nada mudou).
pub(crate) fn convert_active(env: &mut Env, k: NewKind) -> bool {
    env.ed.rulers.new_kind = k;
    if active_ruler(env).is_none() {
        return false;
    }
    edit_active(env, |x, canvas| {
        let old = NewKind::of(x);
        if old == k {
            return;
        }
        let (kind, n) = k.kind_points();
        let mut c = rl::convert(x, kind, n, canvas);
        if x.name == tr(old.key()) || x.name == rl::default_name(x.kind, x.vps.len()) {
            c.name = tr(k.key()).to_owned();
        }
        *x = c;
    });
    true
}

/// A régua ativa (escolhida) da camada das réguas.
pub(crate) fn active_ruler(env: &mut Env) -> Option<Ruler> {
    let layer = ruler_layer(env)?;
    layer_set(&env.core.project, &layer).ok()?.active_ruler().cloned()
}

/// As réguas da camada das réguas, quando encaixam traços agora (encaixe geral ligado, réguas e camada à mostra). Cada
/// régua ainda tem o seu olho e o seu encaixe (filtrados por quem usa).
fn snap_set(env: &mut Env) -> Option<RulerSet> {
    if !env.ed.rulers.snap || !env.ed.rulers.show {
        return None;
    }
    let layer = ruler_layer(env)?;
    let l = cmds::layer_data(&env.core.project, &layer)?;
    if !l.visible {
        return None;
    }
    rl::read_layer(l).ok()
}

/// Régua "de campo" (perspectiva, paralelas, radial, concêntricos, grade, simetria) que encaixa em qualquer lugar: a
/// ativa, quando é de campo; senão a primeira de campo com olho e encaixe ligados.
fn field_ruler(set: &RulerSet) -> Option<Ruler> {
    let ok = |r: &&Ruler| !r.kind.is_object() && r.visible && r.snap;
    set.active_ruler().filter(ok).or_else(|| set.items.iter().find(ok)).cloned()
}

/// Régua de campo que encaixa traços agora (linha da caneta).
pub(crate) fn snap_ruler(env: &mut Env) -> Option<Ruler> {
    field_ruler(&snap_set(env)?)
}

/// Encaixe do traço que começa em `p` (px do canvas), para a caneta, o pincel e a borracha (como no Clip Studio). A
/// régua objeto (linear, curva, figura, guia) a até `SNAP_PX` px da tela da partida ganha (a mais perto); senão a de
/// campo.
pub(crate) fn stroke_snap(env: &mut Env, tool: ToolId, p: Vec2, scale: f32) -> Option<Snap> {
    if !matches!(tool, ToolId::Pen | ToolId::Brush | ToolId::Eraser) {
        return None;
    }
    // borracha com um pincel do grupo borracha: só encaixa o que pede (Borracha de encaixe, limpa-tipos), como no Clip Studio
    if tool == ToolId::Eraser && env.ed.opts.eraser_use_brush && !super::tools::stroke_preset(env, true).snap {
        return None;
    }
    let set = snap_set(env)?;
    let scale = scale.max(1e-3);
    if let Some((i, track, s)) = rl::pick_object(&set.items, a(p), rl::SNAP_PX / scale) {
        return Some(Snap::object(set.items[i].clone(), p, track, s, 12.0 / scale));
    }
    let ruler = field_ruler(&set)?;
    Some(Snap::new(ruler, p, 6.0 / scale, tool == ToolId::Pen))
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

/// Clique da ferramenta Régua: numa alça da régua ativa = arrasto da alça; no vazio = criação por arrasto (ou um
/// ponto da régua curva, que é posta por cliques).
pub(crate) fn press(env: &mut Env, c: Vec2, xf: &Xf) -> Gesture {
    let curve = env.ed.rulers.new_kind == NewKind::Curve;
    // curva em andamento: cada clique é mais um ponto (as alças da régua ativa não atrapalham)
    if curve && !env.ed.rulers.pending.is_empty() {
        return curve_click(env, c, xf.scale);
    }
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
    if curve {
        return curve_click(env, c, xf.scale);
    }
    env.ed.rulers.pen_path = vec![c];
    Gesture::RulerCreate { start: c, cur: c }
}

/// Clique da régua curva: mais um ponto; o segundo clique de um duplo clique termina a curva.
fn curve_click(env: &mut Env, c: Vec2, scale: f32) -> Gesture {
    let st = &mut env.ed.rulers;
    let double = st.last_click.is_some_and(|(t, q)| t.elapsed().as_millis() < 450 && (q - c).length() * scale < 6.0);
    st.last_click = Some((std::time::Instant::now(), c));
    if double {
        st.last_click = None;
        finish_curve(env);
    } else {
        st.pending.push(c);
    }
    Gesture::None
}

/// Termina a régua curva posta por cliques (Enter, duplo clique, troca de ferramenta): com 2 pontos ou mais vira régua.
pub(crate) fn finish_curve(env: &mut Env) {
    let pts = std::mem::take(&mut env.ed.rulers.pending);
    if pts.is_empty() {
        return;
    }
    if pts.len() < 2 {
        env.ed.notify(tr("ruler.curve_need_two"));
        return;
    }
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let mut r = NewKind::Curve.make(canvas, None, "new", tr(NewKind::Curve.key()));
    r.pts = pts.into_iter().map(a).collect();
    add_new(env, r);
}

/// Grava a régua nova `r` na camada das réguas (criando a camada se preciso) e a ativa. Régua inválida é avisada.
fn add_new(env: &mut Env, mut r: Ruler) {
    if let Err(e) = r.validate() {
        env.ed.notify(e);
        return;
    }
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

/// Arrasto de criação: anda a ponta e, na caneta de régua, guarda o caminho da mão.
pub(crate) fn create_move(env: &mut Env, c: Vec2) {
    if let Gesture::RulerCreate { cur, .. } = &mut env.ed.gesture {
        *cur = c;
    }
    let st = &mut env.ed.rulers;
    if st.new_kind == NewKind::RulerPen && st.pen_path.last().is_none_or(|l| (*l - c).length() > 0.3) {
        st.pen_path.push(c);
    }
}

/// Régua que o arrasto de criação de `start` até `cur` faria (px do canvas; `scale` = zoom da vista).
pub(crate) fn creation_preview(env: &mut Env, start: Vec2, cur: Vec2, scale: f32) -> Option<Ruler> {
    let cut = env.ed.active_cut(env.core)?;
    let canvas = cmds::canvas_size(&env.core.project, &cut);
    let (w, h) = (canvas.0 as f32, canvas.1 as f32);
    let kind = env.ed.rulers.new_kind;
    let (k, n) = kind.kind_points();
    let mut r = kind.make(canvas, Some(a(start)), "new", tr(kind.key()));
    let d = cur - start;
    let moved = d.length() * scale.max(1e-3) > 2.0;
    let shift = env.ed.shift;
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
        // linear: o segmento do arrasto (Shift: a cada 15°)
        RulerKind::Linear if moved => {
            let mut e = cur;
            if shift {
                let step = std::f32::consts::PI / 12.0;
                let ang = (d.y.atan2(d.x) / step).round() * step;
                e = start + Vec2::new(ang.cos(), ang.sin()) * d.length();
            }
            r.pts = vec![a(start), a(e)];
        }
        // caneta de régua: o caminho da mão simplificado (tolerância de 2 px da tela)
        RulerKind::Curve => {
            let path: Vec<rl::P> = env.ed.rulers.pen_path.iter().map(|p| a(*p)).collect();
            if let Some(pts) = rl::simplify(&path, 2.0 / scale.max(1e-3)) {
                r.pts = pts;
            }
        }
        // figura: retângulo e elipse na caixa do arrasto (Shift: quadrado/círculo); polígono do centro até um vértice
        RulerKind::Figure if moved => {
            if r.shape == FigureShape::Polygon {
                r.center = a(start);
                r.size = [d.length().max(0.5); 2];
                r.angle = d.y.atan2(d.x).to_degrees() + 90.0;
            } else {
                let mut e = d;
                if shift {
                    let m = d.x.abs().max(d.y.abs());
                    e = Vec2::new(m * d.x.signum(), m * d.y.signum());
                }
                r.center = a(start + e * 0.5);
                r.size = [(e.x.abs() * 0.5).max(0.5), (e.y.abs() * 0.5).max(0.5)];
                r.angle = 0.0;
            }
        }
        // concêntricos: do centro, a elipse da caixa (arrasto quase reto = círculo)
        RulerKind::Concentric if moved => {
            let (ax, ay) = (d.x.abs(), d.y.abs());
            r.size = if ax.min(ay) * scale < 6.0 { [d.length(); 2] } else { [ax, ay] };
            r.angle = 0.0;
        }
        // guia: horizontal ou vertical conforme o arrasto, pelo ponto do clique
        RulerKind::Guide => r.angle = if d.y.abs() > d.x.abs() && moved { 90.0 } else { 0.0 },
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
/// (com réguas, um clique perdido perto de uma alça não deve criar outra). A guia nasce também de um clique (como no
/// Clip Studio); a caneta de régua precisa de um caminho que ande.
pub(crate) fn create_release(env: &mut Env, start: Vec2, cur: Vec2, scale: f32) {
    let kind = env.ed.rulers.new_kind;
    let pen = kind == NewKind::RulerPen;
    let dragged = if pen {
        env.ed.rulers.pen_path.windows(2).map(|w| (w[1] - w[0]).length()).sum::<f32>() * scale >= 8.0
    } else {
        (cur - start).length() * scale >= 8.0
    };
    let empty = active_for_tool(env).is_none_or(|(_, s, _)| s.items.is_empty());
    let click_ok = !pen && (empty || kind == NewKind::Guide);
    if !dragged && !click_ok {
        env.ed.rulers.pen_path.clear();
        return;
    }
    let r = creation_preview(env, start, cur, scale);
    env.ed.rulers.pen_path.clear();
    if let Some(r) = r {
        add_new(env, r);
    }
}

/// Atalhos das réguas (rodam com as teclas do editor).
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    use crate::keymap::Action as A;
    let keys = env.ed.keys.clone();
    let tool_ruler = *env.tool == ToolId::Ruler;
    // régua curva sendo posta por cliques: Enter termina, Esc cancela, Backspace tira o último ponto
    let placing = tool_ruler && !env.ed.rulers.pending.is_empty();
    let (snap, show, next, del, enter, esc, back) = ctx.input_mut(|i| {
        let mut hit = |x: A| keys.get(x).is_some_and(|c| c.take(i));
        let (s, sh, n) = (hit(A::RulerSnap), hit(A::RulersShow), hit(A::RulerNext));
        let none = egui::Modifiers::NONE;
        let (en, es, bk) = if placing { (i.consume_key(none, egui::Key::Enter), i.consume_key(none, egui::Key::Escape), i.consume_key(none, egui::Key::Backspace)) } else { (false, false, false) };
        let d = tool_ruler && !placing && (i.consume_key(none, egui::Key::Delete) || i.consume_key(none, egui::Key::Backspace));
        (s, sh, n, d, en, es, bk)
    });
    if enter {
        finish_curve(env);
    }
    if esc {
        env.ed.rulers.pending.clear();
    }
    if back {
        env.ed.rulers.pending.pop();
    }
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
    // curva por cliques largada (trocou de ferramenta ou de tipo): termina com o que tem
    if !env.ed.rulers.pending.is_empty() && (*env.tool != ToolId::Ruler || env.ed.rulers.new_kind != NewKind::Curve) {
        finish_curve(env);
    }
    let creating = match &env.ed.gesture {
        Gesture::RulerCreate { start, cur } => Some((*start, *cur)),
        _ => None,
    };
    if !env.ed.rulers.show && creating.is_none() && env.ed.rulers.pending.is_empty() {
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
        if let Some(r) = creation_preview(env, s, c, xf.scale) {
            paint_lines(painter, xf, &r, canvas, clip, 1.0);
        }
        // caneta de régua: o caminho da mão, fino, por baixo da curva que ele vira
        if env.ed.rulers.new_kind == NewKind::RulerPen && env.ed.rulers.pen_path.len() >= 2 {
            let sp: Vec<Pos2> = env.ed.rulers.pen_path.iter().map(|p| xf.to_screen(*p)).collect();
            painter.add(egui::Shape::line(sp, Stroke::new(1.0, Color32::from_rgba_unmultiplied(155, 107, 255, 110))));
        }
    }
    // régua curva sendo posta: a curva pelos pontos e pelo cursor, e os pontos
    if !env.ed.rulers.pending.is_empty() {
        let mut pts: Vec<rl::P> = env.ed.rulers.pending.iter().map(|p| a(*p)).collect();
        if let Some(cur) = env.ed.cursor.filter(|p| rect.contains(*p)) {
            pts.push(a(xf.to_canvas(cur)));
        }
        let sp: Vec<Pos2> = rl::spline(&pts, false, 24).into_iter().map(|p| xf.to_screen(v(p))).collect();
        let col = Color32::from_rgb(155, 107, 255);
        if sp.len() >= 2 {
            painter.add(egui::Shape::line(sp, Stroke::new(1.8, col)));
        }
        for p in &env.ed.rulers.pending {
            painter.rect(Rect::from_center_size(xf.to_screen(*p), egui::vec2(7.0, 7.0)), 1.0, Color32::WHITE, Stroke::new(1.5, col), egui::StrokeKind::Inside);
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
                Handle::Point(_) => {
                    painter.circle(p, 5.0, fill, line);
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
            // régua objeto: o caminho em que o traço está, mais forte; concêntricos: a elipse da partida
            let path: Option<Vec<rl::P>> = match (&s.object, s.ruler.kind) {
                (Some(o), _) => Some(o.track.pts.clone()),
                (None, RulerKind::Concentric) => Some(rl::concentric_ring(&s.ruler, a(s.start))),
                _ => None,
            };
            if let Some(path) = path {
                for w in path.windows(2) {
                    if let Some((p, q)) = rl::clip_param(w[0], [w[1][0] - w[0][0], w[1][1] - w[0][1]], 0.0, 1.0, clip) {
                        painter.line_segment([xf.to_screen(v(p)), xf.to_screen(v(q))], Stroke::new(2.2, color_of(&s.ruler, 1.0)));
                    }
                }
            }
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
        let mut snap = Snap::new(r.clone(), s, 6.0, false);
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
        let mut ss = Snap::new(sym, s, 6.0, true);
        assert_eq!(ss.filter(Vec2::new(1234.0, 567.0)), Vec2::new(1234.0, 567.0));
    }

    #[test]
    fn new_kinds_round_trip_through_the_ruler() {
        for k in NewKind::types() {
            let r = k.make((1920, 1080), None, "r1", "");
            assert_eq!(NewKind::of(&r), k);
            r.validate().unwrap();
            assert!(!crate::i18n::tr(k.key()).starts_with("ruler."), "{k:?} sem texto");
        }
        assert_eq!(NewKind::of(&NewKind::RulerPen.make((1920, 1080), None, "r1", "")), NewKind::Curve);
        let ids: std::collections::HashSet<_> = NewKind::ALL.iter().map(|k| k.id()).collect();
        assert_eq!(ids.len(), NewKind::ALL.len());
    }

    /// Régua objeto: os pontos do traço vão para o caminho (linear: o segmento, parando nas pontas); concêntricos: a
    /// elipse da partida.
    #[test]
    fn object_and_concentric_snaps_put_every_point_on_the_ruler() {
        let mut lin = NewKind::Linear.make((1920, 1080), None, "r1", "");
        lin.pts = vec![[100.0, 100.0], [600.0, 100.0]];
        let (_, track, s0) = rl::pick_object(std::slice::from_ref(&lin), [150.0, 130.0], rl::SNAP_PX).unwrap();
        let mut snap = Snap::object(lin, Vec2::new(150.0, 130.0), track, s0, 12.0);
        for (i, q) in [Vec2::new(150.0, 130.0), Vec2::new(300.0, 140.0), Vec2::new(500.0, 70.0), Vec2::new(900.0, 120.0)].into_iter().enumerate() {
            let p = snap.filter(q);
            assert!((p.y - 100.0).abs() < 1e-3, "{i}: {p:?}");
            assert!(p.x <= 600.0 + 1e-3, "passou da ponta: {p:?}");
        }
        let mut c = NewKind::Concentric.make((1920, 1080), Some([500.0, 500.0]), "r2", "");
        c.size = [100.0, 100.0];
        let mut cs = Snap::new(c, Vec2::new(560.0, 500.0), 6.0, true);
        for q in [Vec2::new(500.0, 300.0), Vec2::new(380.0, 520.0)] {
            assert!(((cs.filter(q) - Vec2::new(500.0, 500.0)).length() - 60.0).abs() < 1e-3);
        }
    }

    /// No app: com uma régua linear e uma perspectiva, o traço do pincel que começa perto da linear vai para ela; longe
    /// dela segue a perspectiva; a borracha também encaixa.
    #[test]
    fn strokes_pick_the_nearby_object_ruler_before_the_field_ruler() {
        use super::super::commands::{add_layer, quick_start};
        let mut core = CoreSession::for_tests("rulers");
        core.project.settings.width = 1920;
        core.project.settings.height = 1080;
        let cut = quick_start(&mut core, 12, None).unwrap();
        let guide = add_layer(&mut core, &cut, LayerType::Guide, None).unwrap();
        let mut set = RulerSet::default();
        set.add(two_point());
        let mut lin = NewKind::Linear.make((1920, 1080), None, "", "");
        lin.pts = vec![[200.0, 900.0], [900.0, 900.0]];
        set.add(lin);
        cmds::run(&mut core, rl::set_command(&guide, &set, "r")).unwrap();
        let mut ed = Editor::default();
        ed.cut = Some(cut.clone());
        ed.rulers.layer = Some(guide.clone());
        let (mut icons, mut notes, mut tool) = (Default::default(), Vec::new(), ToolId::Brush);
        let mut env = Env { t: crate::theme::Tokens::dark(), core: &mut core, ed: &mut ed, icons: &mut icons, notes: &mut notes, tool: &mut tool, ws: crate::workspace::WorkspaceId::Layout };
        // a 30 px da tela (zoom 1): linear
        let near = stroke_snap(&mut env, ToolId::Brush, Vec2::new(400.0, 930.0), 1.0).unwrap();
        assert_eq!(near.ruler.kind, RulerKind::Linear);
        // com zoom 2 os mesmos 30 px do canvas são 60 da tela: perspectiva
        let zoomed = stroke_snap(&mut env, ToolId::Brush, Vec2::new(400.0, 930.0), 2.0).unwrap();
        assert_eq!(zoomed.ruler.kind, RulerKind::Perspective);
        assert_eq!(stroke_snap(&mut env, ToolId::Eraser, Vec2::new(400.0, 905.0), 1.0).map(|s| s.ruler.kind), Some(RulerKind::Linear));
        assert!(stroke_snap(&mut env, ToolId::Fill, Vec2::new(400.0, 905.0), 1.0).is_none());
        // encaixe geral desligado: nada
        env.ed.rulers.snap = false;
        assert!(stroke_snap(&mut env, ToolId::Pen, Vec2::new(400.0, 905.0), 1.0).is_none());
    }
}
