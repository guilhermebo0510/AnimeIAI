//! Caneta vetorial em formas por cliques: a curva (spline pelos pontos, forma Curva) e a linha encadeada (polilinha, forma
//! Linha quando o primeiro clique não arrasta; arrastar continua fazendo uma linha só). Cada clique põe um ponto; Enter,
//! duplo clique ou botão direito termina a forma aberta; clicar perto do PRIMEIRO ponto (raio em px da tela, com o ponto
//! destacado e o anel de fechar junto da mira) fecha a forma e termina; Esc ou Backspace tiram o último ponto e, sem
//! pontos, cancelam (como no Illustrator e no Clip Studio). A linha encaixa cada trecho na régua ativa (Shift = passos de
//! 15°); a prévia mostra o próximo trecho até o cursor.

use super::editor::*;
use super::ui_kit::Env;
use crate::workspace::ToolId;
use aiai_core::StrokeGeometry;
use egui::{Color32, Stroke, Vec2};
use std::time::Instant;

/// Raio (px da tela) em volta do primeiro ponto que fecha a forma.
pub(crate) const CLOSE_PX: f32 = 10.0;
/// Duplo clique: tempo e distância (px da tela) entre os dois cliques.
const DOUBLE_MS: u128 = 350;
const DOUBLE_PX: f32 = 6.0;
/// Clique sem arrasto (px da tela) na forma Linha: começa a linha encadeada em vez de gravar uma linha de 0 px.
pub(crate) const CLICK_PX: f32 = 4.0;

/// Gesto novo de cliques com o primeiro ponto em `c` (`line` = polilinha; senão, curva).
pub(crate) fn begin(c: Vec2, line: bool) -> Gesture {
    Gesture::PolyCurve { pts: vec![c], last_click: Instant::now(), line }
}

/// Pontos mínimos para fechar: triângulo na linha, dois pontos na curva (vira um laço).
fn min_closed(line: bool) -> usize {
    if line { 3 } else { 2 }
}

/// O cursor `c` (px do canvas) fecha a forma: perto do primeiro ponto, com pontos bastantes.
pub(crate) fn closes(pts: &[Vec2], line: bool, c: Vec2, scale: f32) -> bool {
    pts.len() >= min_closed(line) && pts.first().is_some_and(|f| (*f - c).length() * scale <= CLOSE_PX)
}

/// O ponto que um clique em `c` põe agora: a linha encaixa o trecho a partir do último ponto na régua ativa (ou a 15° com
/// Shift); a curva fica onde o cursor está.
pub(crate) fn next_point(env: &mut Env, pts: &[Vec2], line: bool, c: Vec2) -> Vec2 {
    let Some(last) = pts.last().copied().filter(|_| line) else { return c };
    if env.ed.shift {
        super::tools::shape_points(PenShape::Line, last, c, true).0[1]
    } else {
        super::rulers::snap_line_end(env, last, c)
    }
}

/// Clique (aperto) com o gesto de cliques aberto: põe ponto, fecha no primeiro, termina no duplo clique/botão direito.
pub(crate) fn press(env: &mut Env, c: Vec2, scale: f32, button: egui::PointerButton) {
    let Gesture::PolyCurve { pts, last_click, line } = &env.ed.gesture else { return };
    let (pts, line, since) = (pts.clone(), *line, last_click.elapsed().as_millis());
    match button {
        egui::PointerButton::Secondary => finish(env, false),
        egui::PointerButton::Primary => {
            if closes(&pts, line, c, scale) {
                return finish(env, true);
            }
            if since < DOUBLE_MS && pts.last().is_some_and(|l| (*l - c).length() * scale < DOUBLE_PX) {
                return finish(env, false);
            }
            let p = next_point(env, &pts, line, c);
            if let Gesture::PolyCurve { pts, last_click, .. } = &mut env.ed.gesture {
                pts.push(p);
                *last_click = Instant::now();
            }
        }
        _ => {}
    }
}

/// Grava a forma de cliques (aberta ou fechada) e encerra o gesto. Menos de 2 pontos diferentes não grava nada.
pub(crate) fn finish(env: &mut Env, closed: bool) {
    let Gesture::PolyCurve { mut pts, line, .. } = std::mem::replace(&mut env.ed.gesture, Gesture::None) else { return };
    pts.dedup_by(|a, b| (*a - *b).length() < 0.5);
    let closed = closed && pts.len() >= min_closed(line);
    if pts.len() < 2 {
        return;
    }
    // a Figura grava (camada vetorial, raster ou de preenchimento; linha, preenchimento ou os dois)
    super::figure::commit_clicks(env, pts, line, closed);
}

/// Esc/Backspace: tira o último ponto; sem pontos, cancela a forma.
pub(crate) fn back(env: &mut Env) {
    if let Gesture::PolyCurve { pts, .. } = &mut env.ed.gesture {
        pts.pop();
        if pts.is_empty() {
            env.ed.gesture = Gesture::None;
        }
    }
}

/// Teclas da forma de cliques (antes dos atalhos gerais): Enter termina, Esc/Backspace tiram o último ponto. Trocar de
/// ferramenta ou de forma da caneta no meio grava o que já foi feito (aberto).
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    let Gesture::PolyCurve { line, .. } = &env.ed.gesture else { return };
    let want: &[PenShape] = if *line { &[PenShape::Line, PenShape::Polyline] } else { &[PenShape::Curve] };
    if *env.tool != ToolId::Pen || !want.contains(&env.ed.opts.pen_shape) {
        return finish(env, false);
    }
    use egui::{Key, Modifiers};
    let (enter, backs) = ctx.input_mut(|i| {
        let enter = i.consume_key(Modifiers::NONE, Key::Enter);
        let mut n = 0;
        while i.consume_key(Modifiers::NONE, Key::Escape) || i.consume_key(Modifiers::NONE, Key::Backspace) {
            n += 1;
        }
        (enter, n)
    });
    for _ in 0..backs {
        back(env);
    }
    if enter {
        finish(env, false);
    }
}

// ------------------------------------------------------------------------------------------------ Recortar na prévia

/// Base do recorte de `layer` quando ela tem Recortar (`clip_to_below`), como o compositor acha: a camada de baixo mais
/// perto, no mesmo grupo, que não recorta (guia e referência fora da vista não contam; uma oculta que não recorta desliga
/// o recorte). `None` = a camada não recorta, ou recorta em nada (o compositor desenha sem recorte).
pub(crate) fn clip_base(p: &aiai_core::Project, layer: &super::commands::NodeId) -> Option<super::commands::NodeId> {
    clip_base_in(p, layer, false, false)
}

/// Como [`clip_base`], com guias/referências entrando quando a vista as mostra (`guides`, `reference`).
fn clip_base_in(p: &aiai_core::Project, layer: &super::commands::NodeId, guides: bool, reference: bool) -> Option<super::commands::NodeId> {
    use aiai_core::{LayerType, NodeKind};
    let n = p.node(layer)?;
    let NodeKind::Layer(l) = &n.data else { return None };
    if !l.clip_to_below {
        return None;
    }
    let sibs = &p.node(n.parent.as_ref()?)?.children;
    let i = sibs.iter().position(|c| c == layer)?;
    for id in sibs[..i].iter().rev() {
        let Some(NodeKind::Layer(b)) = p.node(id).map(|x| &x.data) else { continue };
        if (b.kind == LayerType::Guide && !guides) || (b.kind == LayerType::Reference && !reference) {
            continue;
        }
        if !b.visible {
            if b.clip_to_below {
                continue;
            }
            return None;
        }
        if !b.clip_to_below {
            return Some(id.clone());
        }
    }
    None
}

/// Alfa (no canvas) da base do recorte de `layer`, para a prévia ao vivo mostrar o traço como o render vai mostrar: numa
/// camada com Recortar, o traço só aparece sobre o desenho da camada de baixo (antes a prévia mostrava o traço inteiro e
/// ele "sumia" ao soltar, quando chegava o render recortado).
/// `guides`/`reference` = o que a vista do canvas mostra (uma camada 3D de referência só entra com a referência à mostra).
pub(crate) fn clip_alpha(p: &aiai_core::Project, blobs: &aiai_core::BlobStore, cut: &super::commands::NodeId, layer: &super::commands::NodeId, frame: u32, guides: bool, reference: bool) -> Option<aiai_paint::Image> {
    let base = clip_base_in(p, layer, guides, reference)?;
    let opts = aiai_render::RenderOptions { include_guides: guides, include_reference: reference, only_layers: Some(vec![base]), background: None, camera: false, layer_motion: true };
    aiai_render::render_frame(p, blobs, cut, frame, &opts).ok().map(|o| o.image)
}

/// Traço começando numa camada com Recortar: avisa na barra que ele só aparece sobre o desenho da camada de baixo.
pub(crate) fn clip_notice(env: &mut Env, layer: &super::commands::NodeId) {
    if clip_base(&env.core.project, layer).is_some() {
        env.ed.notify(crate::i18n::tr("msg.stroke_clipped"));
    }
}

/// Multiplica os pixels do traço (`crop` na área de `origin`, `fwd` = área → canvas) pelo alfa da base do recorte.
pub(crate) fn clip_pixels(crop: &mut aiai_paint::Image, mask: &[bool], origin: [i32; 2], fwd: Option<&aiai_paint::Affine>, base: &aiai_paint::Image) {
    let cw = crop.width as usize;
    for (i, px) in crop.data.iter_mut().enumerate() {
        if !mask.get(i).copied().unwrap_or(false) {
            continue;
        }
        let (x, y) = ((origin[0] + (i % cw) as i32) as f32 + 0.5, (origin[1] + (i / cw) as i32) as f32 + 0.5);
        let (cx, cy) = match fwd {
            Some(af) => af.apply(x, y),
            None => (x, y),
        };
        let (xi, yi) = (cx.floor() as i64, cy.floor() as i64);
        let a = if xi >= 0 && yi >= 0 && (xi as u32) < base.width && (yi as u32) < base.height { base.data[(yi as u32 * base.width + xi as u32) as usize][3] } else { 0.0 };
        for c in px.iter_mut() {
            *c *= a;
        }
    }
}

/// Prévia da forma de cliques: a forma até aqui mais o próximo trecho até o cursor (encaixado como o clique faria), ou a
/// forma fechada quando o cursor está no primeiro ponto (ponto destacado e anel de fechar junto da mira).
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf) {
    let Gesture::PolyCurve { pts, line, .. } = &env.ed.gesture else { return };
    let (pts, line) = (pts.clone(), *line);
    let c = super::commands::resolve_color(&env.core.project, &env.ed.color);
    let color = super::ui_kit::rgba_to_color32(aiai_core::Rgba([c.0[0], c.0[1], c.0[2], 1.0]));
    let cursor = env.ed.cursor.map(|q| xf.to_canvas(q));
    let closing = cursor.is_some_and(|q| closes(&pts, line, q, xf.scale));
    let mut path = pts.clone();
    match cursor {
        Some(_) if closing => path.push(pts[0]),
        Some(q) => {
            let p = next_point(env, &pts, line, q);
            path.push(p);
        }
        None => {}
    }
    let width = env.ed.opts.pen_width.max(0.1);
    if line {
        super::canvas::polyline(painter, &path, width, color, xf);
    } else {
        if closing {
            path.pop();
        }
        let st = aiai_core::Stroke { taper: None,
            id: aiai_core::Id(String::new()),
            points: path.iter().map(|p| aiai_core::StrokePoint { x: p.x, y: p.y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
            geometry: StrokeGeometry::Spline,
            tension: 0.0,
            tip: aiai_core::VectorTip::Marker,
            width,
            profile: Default::default(),
            color: env.ed.color.clone(),
            opacity: 1.0,
            closed: closing,
            aliased: false,
            color_trace: false,
            group: String::new(),
        };
        let (sp, sr) = super::tools::sampled(&st);
        super::canvas::variable_line(painter, &sp, &sr, color, xf);
    }
    let accent = env.t.accent;
    for (i, p) in pts.iter().enumerate() {
        let s = xf.to_screen(*p);
        match (i, closing) {
            (0, true) => painter.circle(s, 7.0, accent, Stroke::new(2.0, Color32::WHITE)),
            // o primeiro ponto é o alvo de fechar: um pouco maior
            (0, false) => painter.circle(s, 4.5, Color32::WHITE, Stroke::new(1.6, accent)),
            _ => painter.circle(s, 3.5, Color32::WHITE, Stroke::new(1.2, accent)),
        };
    }
    if closing {
        // anel de fechar junto da mira (como a caneta do Illustrator)
        if let Some(q) = env.ed.cursor {
            let at = q + egui::vec2(13.0, 13.0);
            painter.circle_stroke(at, 4.5, Stroke::new(3.2, Color32::from_black_alpha(170)));
            painter.circle_stroke(at, 4.5, Stroke::new(1.6, Color32::WHITE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::commands::{self as cmds, add_layer, quick_start, NodeId};
    use super::super::editor::*;
    use super::super::ui_kit::Env;
    use crate::workspace::{ToolId, WorkspaceId};
    use aiai_core::*;
    use aiai_tools::ruler::{self as rl, Ruler, RulerKind, RulerSet};
    use egui::Vec2;

    struct Doc {
        core: CoreSession,
        ed: Editor,
        icons: crate::icons::Icons,
        notes: Vec<(String, egui::Rect)>,
        tool: ToolId,
        cut: NodeId,
        line: NodeId,
        guide: NodeId,
    }

    fn doc(ruler: Option<Ruler>) -> Doc {
        let mut core = CoreSession::for_tests("pen");
        core.project.settings.width = 1920;
        core.project.settings.height = 1080;
        let cut = quick_start(&mut core, 12, None).unwrap();
        let line = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        let guide = add_layer(&mut core, &cut, LayerType::Guide, Some(&line)).unwrap();
        if let Some(r) = ruler {
            let mut set = RulerSet::default();
            set.add(r);
            cmds::run(&mut core, rl::set_command(&guide, &set, "r")).unwrap();
        }
        let mut ed = Editor::default();
        ed.cut = Some(cut.clone());
        ed.layer = Some(line.clone());
        ed.rulers.layer = Some(guide.clone());
        Doc { core, ed, icons: Default::default(), notes: Vec::new(), tool: ToolId::Pen, cut, line, guide }
    }

    impl Doc {
        fn env(&mut self) -> Env<'_> {
            Env { t: crate::theme::Tokens::dark(), core: &mut self.core, ed: &mut self.ed, icons: &mut self.icons, notes: &mut self.notes, tool: &mut self.tool, ws: WorkspaceId::Layout }
        }
        fn strokes(&self) -> Vec<Stroke> {
            let cel = cmds::exposed_cel(&self.core.project, &self.cut, &self.line, 0);
            cel.and_then(|c| cmds::cel_data(&self.core.project, &c).map(|d| d.strokes.clone())).unwrap_or_default()
        }
    }

    fn two_point() -> Ruler {
        let mut r = Ruler::preset(RulerKind::Perspective, 2, (1920, 1080), Some([960.0, 500.0]), "r1", "");
        r.vps = vec![[-1000.0, 500.0], [3000.0, 500.0]];
        r
    }

    /// Caneta com régua de 2 pontos: o traço fica depois de soltar, e todos os pontos estão na reta da partida ao ponto de fuga.
    #[test]
    fn pen_stroke_snapped_to_a_ruler_is_committed_with_exact_geometry() {
        let mut d = doc(Some(two_point()));
        let s = Vec2::new(1000.0, 800.0);
        let mut env = d.env();
        let g = super::super::tools::begin_stroke(&mut env, ToolId::Pen, s, 1.0, 1.0);
        let Gesture::Stroke(mut g) = g else { panic!("sem traço") };
        for i in 1..60 {
            let t = i as f32;
            // a mão treme para os lados e às vezes volta um pouco
            let wob = if i > 2 { (t * 0.7).sin() * 3.0 } else { 0.0 };
            let back = if i % 9 == 0 { 9.0 } else { 0.0 };
            g.push(s + Vec2::new(-t * 6.0 + back, -t * 0.9 + wob), 0.8);
        }
        super::super::tools::finish_stroke(&mut env, g, std::time::Instant::now());
        let toast = env.ed.toast.clone();
        let st = d.strokes();
        assert_eq!(st.len(), 1, "o traço some ao soltar: {toast:?}");
        let to_vp = (Vec2::new(-1000.0, 500.0) - s).normalized();
        for q in &st[0].points {
            let off = (q.x - s.x) * to_vp.y - (q.y - s.y) * to_vp.x;
            assert!(off.abs() < 1e-2, "ponto fora da reta: {q:?}");
        }
        // o traço só anda para a frente na reta (a mão que volta não dobra o traço sobre si)
        let along: Vec<f32> = st[0].points.iter().map(|q| (Vec2::new(q.x, q.y) - s).dot(to_vp)).collect();
        assert!(along.windows(2).all(|w| w[1] > w[0]), "o traço volta na reta: {along:?}");
        // e aparece no render
        let img = aiai_render::render_frame(&d.core.project, &d.core.blobs, &d.cut, 0, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image;
        let mid = s + to_vp * 150.0;
        let ink = (-3..=3).flat_map(|dy| (-3..=3).map(move |dx| (dx, dy))).map(|(dx, dy)| img.get((mid.x as i32 + dx) as u32, (mid.y as i32 + dy) as u32)[3]).fold(0.0f32, f32::max);
        assert!(ink > 0.5, "o traço não aparece no render ({ink})");
    }

    fn press(d: &mut Doc, x: f32, y: f32) {
        let mut env = d.env();
        super::press(&mut env, Vec2::new(x, y), 1.0, egui::PointerButton::Primary);
    }

    fn npts(d: &Doc) -> Option<usize> {
        match &d.ed.gesture {
            Gesture::PolyCurve { pts, .. } => Some(pts.len()),
            _ => None,
        }
    }

    /// Curva por cliques: Esc tira o último ponto (sem pontos, cancela), clicar no primeiro ponto fecha e grava a spline
    /// fechada; Enter, duplo clique e botão direito gravam aberta.
    #[test]
    fn curve_by_clicks_closes_on_the_first_point_and_esc_removes_the_last() {
        let mut d = doc(None);
        d.ed.opts.pen_shape = PenShape::Curve;
        d.ed.gesture = super::begin(Vec2::new(100.0, 100.0), false);
        press(&mut d, 200.0, 100.0);
        press(&mut d, 200.0, 200.0);
        assert_eq!(npts(&d), Some(3));
        super::back(&mut d.env());
        assert_eq!(npts(&d), Some(2), "Esc tira só o último ponto");
        press(&mut d, 150.0, 220.0);
        // perto do primeiro ponto (8 px da tela): fecha e termina
        assert!(super::closes(&[Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0)], false, Vec2::new(106.0, 105.0), 1.0));
        press(&mut d, 106.0, 105.0);
        assert!(matches!(d.ed.gesture, Gesture::None));
        let st = d.strokes();
        assert_eq!(st.len(), 1);
        assert!(st[0].closed && st[0].geometry == StrokeGeometry::Spline && st[0].points.len() == 3, "{:?}", st[0]);
        // Esc até o fim cancela sem gravar
        d.ed.gesture = super::begin(Vec2::new(300.0, 300.0), false);
        press(&mut d, 400.0, 300.0);
        super::back(&mut d.env());
        super::back(&mut d.env());
        assert!(matches!(d.ed.gesture, Gesture::None));
        assert_eq!(d.strokes().len(), 1);
        // Enter termina aberta
        d.ed.gesture = super::begin(Vec2::new(300.0, 300.0), false);
        press(&mut d, 400.0, 300.0);
        press(&mut d, 400.0, 400.0);
        super::finish(&mut d.env(), false);
        let st = d.strokes();
        assert_eq!(st.len(), 2);
        assert!(!st[1].closed && st[1].points.len() == 3);
        // duplo clique no último ponto termina aberta (sem ponto repetido)
        d.ed.gesture = super::begin(Vec2::new(500.0, 500.0), false);
        press(&mut d, 600.0, 500.0);
        press(&mut d, 601.0, 501.0);
        assert!(matches!(d.ed.gesture, Gesture::None));
        assert_eq!(d.strokes()[2].points.len(), 2);
        // botão direito termina
        d.ed.gesture = super::begin(Vec2::new(500.0, 600.0), false);
        press(&mut d, 600.0, 650.0);
        super::press(&mut d.env(), Vec2::new(700.0, 700.0), 1.0, egui::PointerButton::Secondary);
        assert!(matches!(d.ed.gesture, Gesture::None));
        assert_eq!(d.strokes().len(), 4);
    }

    /// Linha encadeada: cliques põem trechos retos ligados; clicar no primeiro ponto fecha o polígono (triângulo).
    #[test]
    fn chained_line_closes_into_a_triangle() {
        let mut d = doc(None);
        d.ed.opts.pen_shape = PenShape::Line;
        d.ed.gesture = super::begin(Vec2::new(100.0, 300.0), true);
        press(&mut d, 300.0, 300.0);
        press(&mut d, 200.0, 120.0);
        // dois pontos não fecham uma linha (precisa de 3)
        assert!(!super::closes(&[Vec2::new(100.0, 300.0), Vec2::new(300.0, 300.0)], true, Vec2::new(100.0, 300.0), 1.0));
        press(&mut d, 103.0, 297.0);
        let st = d.strokes();
        assert_eq!(st.len(), 1);
        let s = &st[0];
        assert!(s.closed && s.geometry == StrokeGeometry::Polyline, "{s:?}");
        assert_eq!(s.points.iter().map(|q| (q.x, q.y)).collect::<Vec<_>>(), vec![(100.0, 300.0), (300.0, 300.0), (200.0, 120.0)]);
    }

    /// Linha encadeada com régua de 2 pontos: cada trecho cai exatamente na reta do ponto de fuga mais perto.
    #[test]
    fn chained_line_segments_snap_to_the_ruler() {
        let mut d = doc(Some(two_point()));
        d.ed.opts.pen_shape = PenShape::Line;
        let s = Vec2::new(1000.0, 800.0);
        d.ed.gesture = super::begin(s, true);
        press(&mut d, 800.0, 775.0);
        super::finish(&mut d.env(), false);
        let st = d.strokes();
        let q = st[0].points[1];
        let to_vp = (Vec2::new(-1000.0, 500.0) - s).normalized();
        let off = (q.x - s.x) * to_vp.y - (q.y - s.y) * to_vp.x;
        assert!(off.abs() < 1e-3, "fora da reta do ponto de fuga: {q:?}");
        // a linha de arrasto usa o mesmo encaixe
        let mut env = d.env();
        let end = super::super::rulers::snap_line_end(&mut env, s, Vec2::new(800.0, 775.0));
        assert!((end - Vec2::new(q.x, q.y)).length() < 1e-3);
    }

    /// Retângulo e elipse da caneta gravam com e sem régua (a régua não mexe nas formas fechadas).
    #[test]
    fn rect_and_ellipse_commit_with_and_without_a_ruler() {
        for ruler in [None, Some(two_point())] {
            let mut d = doc(ruler);
            for shape in [PenShape::Rect, PenShape::Ellipse] {
                d.ed.opts.pen_shape = shape;
                let (pts, geo, closed) = super::super::tools::shape_points(shape, Vec2::new(100.0, 100.0), Vec2::new(300.0, 220.0), false);
                super::super::tools::commit_vector_shape(&mut d.env(), pts.clone(), geo, closed);
                let st = d.strokes();
                let s = st.last().unwrap();
                assert!(s.closed && s.points.len() == pts.len());
                assert_eq!((s.points[0].x, s.points[0].y), (pts[0].x, pts[0].y));
            }
            assert_eq!(d.strokes().len(), 2);
        }
    }

    /// Trocar o tipo da régua escolhida converte no lugar (1 → 2 pontos espelha o ponto em volta do meio), num passo de
    /// undo; sem régua, só muda o tipo da próxima.
    #[test]
    fn changing_the_type_converts_the_selected_ruler_in_one_undo_step() {
        use super::super::rulers::{convert_active, NewKind};
        let one = Ruler::preset(RulerKind::Perspective, 1, (1920, 1080), Some([500.0, 400.0]), "r1", crate::i18n::tr(NewKind::P1.key()));
        let mut d = doc(Some(one.clone()));
        let read = |d: &Doc| rl::read_layer(cmds::layer_data(&d.core.project, &d.guide).unwrap()).unwrap();
        assert!(convert_active(&mut d.env(), NewKind::P2));
        let set = read(&d);
        assert_eq!(set.items.len(), 1);
        let r = &set.items[0];
        assert_eq!(r.vps, vec![[500.0, 400.0], [1420.0, 400.0]]);
        assert_eq!((r.id.as_str(), r.color.as_str()), ("r1", one.color.as_str()));
        assert_eq!(r.name, crate::i18n::tr(NewKind::P2.key()), "o nome padrão acompanha o tipo");
        assert_eq!(d.ed.rulers.new_kind, NewKind::P2);
        d.core.undo().unwrap();
        assert_eq!(read(&d).items[0], one, "um passo de undo volta à régua de 1 ponto");
        // sem régua: nada é convertido, só muda o tipo da próxima
        cmds::run(&mut d.core, rl::set_command(&d.guide, &RulerSet::default(), "r")).unwrap();
        assert!(!convert_active(&mut d.env(), NewKind::Radial));
        assert!(read(&d).items.is_empty());
        assert_eq!(d.ed.rulers.new_kind, NewKind::Radial);
    }

    /// Camada com Recortar: a base é a de baixo que não recorta (oculta desliga), e a prévia ao vivo multiplica o traço
    /// pelo alfa dela — o traço fora do desenho de baixo não aparece nem durante o arrasto (antes "sumia" ao soltar).
    #[test]
    fn clipped_layer_preview_matches_the_render() {
        let mut d = doc(None);
        let base = d.line.clone();
        let top = add_layer(&mut d.core, &d.cut, LayerType::Vector, Some(&base)).unwrap();
        assert_eq!(super::clip_base(&d.core.project, &top), None, "sem Recortar não há base");
        cmds::set_layer_props(&mut d.core, &top, serde_json::json!({"clip_to_below": true})).unwrap();
        assert_eq!(super::clip_base(&d.core.project, &top), Some(base.clone()));
        // um traço grosso na base, de x = 100 a 300 em y = 200
        let pt = |x: f32| StrokePoint { x, y: 200.0, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None };
        let stroke = Stroke { taper: None, id: Id(String::new()), points: vec![pt(100.0), pt(300.0)], geometry: StrokeGeometry::Polyline, tension: 0.0, tip: VectorTip::Marker, width: 20.0, profile: WidthProfile::default(), color: ColorRef::default(), opacity: 1.0, closed: false, aliased: false, color_trace: false, group: String::new() };
        let cel = cmds::exposed_cel(&d.core.project, &d.cut, &base, 0).unwrap();
        cmds::run(&mut d.core, Command::InsertStrokes { cel, items: vec![aiai_core::command::IndexedStroke { index: 0, stroke }] }).unwrap();
        let alpha = super::clip_alpha(&d.core.project, &d.core.blobs, &d.cut, &top, 0, false, false).expect("alfa da base");
        assert!(alpha.get(200, 200)[3] > 0.9 && alpha.get(200, 260)[3] < 1e-3);
        // pixels do traço novo: um sobre a base, um fora
        let mut crop = aiai_paint::Image::new(2, 1);
        crop.data = vec![[0.0, 0.0, 0.0, 1.0]; 2];
        super::clip_pixels(&mut crop, &[true, true], [200, 200], None, &alpha);
        assert!(crop.data[0][3] > 0.9);
        let mut out = aiai_paint::Image::new(1, 1);
        out.data = vec![[0.0, 0.0, 0.0, 1.0]];
        super::clip_pixels(&mut out, &[true], [200, 260], None, &alpha);
        assert!(out.data[0][3] < 1e-3, "fora do desenho de baixo o traço some na prévia também");
        // base oculta (sem Recortar) desliga o recorte, como no compositor
        cmds::set_layer_props(&mut d.core, &base, serde_json::json!({"visible": false})).unwrap();
        assert_eq!(super::clip_base(&d.core.project, &top), None);
    }

    /// Caneta com régua de simetria: o traço fica como foi desenhado e a cópia espelhada entra no mesmo passo de undo.
    #[test]
    fn pen_with_a_symmetry_ruler_commits_the_mirrored_copy() {
        let sym = Ruler::preset(RulerKind::Symmetry, 2, (1920, 1080), Some([960.0, 540.0]), "r1", "");
        let mut d = doc(Some(sym));
        let mut env = d.env();
        let Gesture::Stroke(mut g) = super::super::tools::begin_stroke(&mut env, ToolId::Pen, Vec2::new(700.0, 300.0), 1.0, 1.0) else { panic!() };
        for i in 1..30 {
            g.push(Vec2::new(700.0 - i as f32 * 5.0, 300.0 + (i as f32 * 0.5).sin() * 20.0), 1.0);
        }
        super::super::tools::finish_stroke(&mut env, g, std::time::Instant::now());
        let st = d.strokes();
        assert_eq!(st.len(), 2, "o traço e a cópia");
        for (a, b) in st[0].points.iter().zip(&st[1].points) {
            assert!((a.x + b.x - 1920.0).abs() < 1e-2 && (a.y - b.y).abs() < 1e-2, "espelho no eixo vertical do meio: {a:?} {b:?}");
        }
        d.core.undo().unwrap();
        assert!(d.strokes().is_empty(), "um passo de undo tira os dois");
    }
}
