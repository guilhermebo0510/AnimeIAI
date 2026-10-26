//! Ferramentas funcionais do S25 (à moda do Clip Studio) que não têm módulo próprio: as opções novas da Seleção, da
//! Seleção automática, do Balde, do Conta-gotas e do Mover/Operação (um campo só, `ToolOptions::area`), o "a que
//! camadas a varinha/o balde se referem", o conta-gotas da cor da camada, o "selecionar camada" pelo clique no desenho
//! e os desenhos das sub-ferramentas novas na lista do Grupo de ferramentas.

use super::commands::{self as cmds, NodeId};
use super::selection::SelTool;
use super::ui_kit::Env;
use crate::i18n::tr;
use aiai_core::*;
use egui::{Color32, Painter, Pos2, Rect, Stroke as EStroke, Vec2};
use std::sync::Arc;

/// A que camadas a varinha (e o encolher seleção) se refere: as três sub-ferramentas da Seleção automática do Clip
/// Studio ("só a camada em edição", "todas as camadas", "camadas de referência").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum WandRef {
    #[default]
    Editing,
    All,
    /// As camadas de linha ligadas ao balde (`line_refs` das camadas de preenchimento do corte; o ícone de referência
    /// do balde no painel Camadas).
    Reference,
}

impl WandRef {
    pub(crate) const ALL: [WandRef; 3] = [WandRef::Editing, WandRef::All, WandRef::Reference];

    pub(crate) fn id(self) -> &'static str {
        match self {
            WandRef::Editing => "editing",
            WandRef::All => "all",
            WandRef::Reference => "reference",
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            WandRef::Editing => "wand.ref.editing",
            WandRef::All => "wand.ref.all",
            WandRef::Reference => "wand.ref.reference",
        }
    }
}

/// O jeito do balde (as sub-ferramentas do Balde do Clip Studio, com o nosso tinta-e-pinta como "outras camadas").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub(crate) enum FillMode {
    /// Referir outras camadas: as linhas das camadas de referência delimitam, a cor vai para a camada de
    /// preenchimento (o balde de sempre, tinta-e-pinta de anime).
    #[default]
    Others,
    /// Só a camada em edição: as cores da própria camada delimitam e a cor entra nela (pixels na raster, região na
    /// vetorial/de preenchimento).
    Editing,
    /// Cercar e preencher: um laço em volta; as regiões fechadas que ele cerca são pintadas.
    Enclose,
    /// Pintar área não preenchida: um pincel; as regiões ainda sem tinta que ele toca são pintadas.
    Unfilled,
}

impl FillMode {
    pub(crate) const ALL: [FillMode; 4] = [FillMode::Others, FillMode::Editing, FillMode::Enclose, FillMode::Unfilled];

    pub(crate) fn id(self) -> &'static str {
        match self {
            FillMode::Others => "others",
            FillMode::Editing => "editing",
            FillMode::Enclose => "enclose",
            FillMode::Unfilled => "unfilled",
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            FillMode::Others => "fillm.others",
            FillMode::Editing => "fillm.editing",
            FillMode::Enclose => "fillm.enclose",
            FillMode::Unfilled => "fillm.unfilled",
        }
    }

    /// O balde age por um gesto (laço ou pincel) e não por um clique.
    pub(crate) fn is_path(self) -> bool {
        matches!(self, FillMode::Enclose | FillMode::Unfilled)
    }
}

/// Opções novas (S25) das ferramentas de área.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AreaOpts {
    /// A que camadas a varinha e o encolher seleção se referem.
    pub wand_ref: WandRef,
    /// Varinha: fechar intervalo (px; 0 = desligado).
    pub wand_gap: f32,
    /// Varinha: dimensionamento de área (px; negativo contrai).
    pub wand_scale: f32,
    /// Caneta e borracha de seleção: diâmetro (px do canvas).
    pub sel_brush: f32,
    /// Laço magnético: raio da busca da borda (px do canvas).
    pub sel_magnet: f32,
    pub fill_mode: FillMode,
    /// Balde na camada em edição: tolerância de cor (0..255, por canal).
    pub fill_tolerance: f32,
    /// Pintar área não preenchida: diâmetro do pincel (px do canvas).
    pub fill_brush: f32,
    /// Conta-gotas: lê só a camada ativa (a cor da camada) em vez do que aparece na tela.
    pub eyedrop_layer: bool,
    /// Mão: o arrasto gira a visão (sub-ferramenta Girar, como o R do Clip Studio).
    pub hand_rotate: bool,
    /// Mover: o clique escolhe a camada desenhada sob o cursor (sub-ferramenta Selecionar camada).
    pub pick_layer: bool,
    /// Texto: o arrasto dá o centro e o raio e o texto corre em volta do círculo (Texto circular).
    pub text_circle: bool,
}

impl Default for AreaOpts {
    fn default() -> Self {
        AreaOpts {
            wand_ref: WandRef::Editing,
            wand_gap: 0.0,
            wand_scale: 0.0,
            sel_brush: 30.0,
            sel_magnet: 10.0,
            fill_mode: FillMode::Others,
            fill_tolerance: 10.0,
            fill_brush: 40.0,
            eyedrop_layer: false,
            hand_rotate: false,
            pick_layer: false,
            text_circle: false,
        }
    }
}

// ------------------------------------------------------------------------------------------------ referências

/// Camadas de referência do corte: as camadas de linha ligadas ao balde de alguma camada de preenchimento.
pub(crate) fn reference_layers(p: &Project, cut: &NodeId) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = Vec::new();
    for r in cmds::layer_rows(p, cut) {
        if let Some(l) = cmds::layer_data(p, &r.id).filter(|l| l.kind == LayerType::Fill) {
            for id in &l.line_refs {
                if p.node(id).is_some() && !out.contains(id) {
                    out.push(id.clone());
                }
            }
        }
    }
    out
}

/// Imagem que a varinha / o encolher seleção / o balde na camada leem, em px do canvas (com o movimento das camadas).
pub(crate) fn reference_image(env: &Env, cut: &NodeId, layer: &NodeId, r: WandRef) -> Result<aiai_paint::Image, String> {
    let only = match r {
        WandRef::Editing => Some(vec![layer.clone()]),
        WandRef::All => None,
        WandRef::Reference => {
            let refs = reference_layers(&env.core.project, cut);
            if refs.is_empty() {
                return Err(tr("wand.no_reference").to_owned());
            }
            Some(refs)
        }
    };
    let opts = aiai_render::RenderOptions { include_guides: false, include_reference: false, only_layers: only, background: None, camera: false, layer_motion: true };
    aiai_render::render_frame(&env.core.project, &env.core.blobs, cut, env.ed.frame, &opts).map(|o| o.image).map_err(|e| e.to_string())
}

/// Render de uma camada guardado por (documento, camada, quadro): o conta-gotas da camada e o laço magnético leem a
/// mesma imagem a cada movimento sem renderizar de novo.
#[derive(Default)]
pub(crate) struct LayerCache {
    key: Option<(u64, NodeId, u32, WandRef)>,
    img: Option<Arc<aiai_paint::Image>>,
    edges: Option<Arc<Vec<f32>>>,
}

pub(crate) fn cached_image(env: &mut Env, r: WandRef) -> Result<Arc<aiai_paint::Image>, String> {
    let cut = env.ed.active_cut(env.core).ok_or_else(|| tr("err.no_layer").to_owned())?;
    let layer = env.ed.active_layer(env.core).ok_or_else(|| tr("err.no_layer").to_owned())?;
    let key = (env.core.change_counter, layer.clone(), env.ed.frame, r);
    if env.ed.sel_state.layer_cache.key.as_ref() == Some(&key) {
        if let Some(i) = &env.ed.sel_state.layer_cache.img {
            return Ok(i.clone());
        }
    }
    let img = Arc::new(reference_image(env, &cut, &layer, r)?);
    env.ed.sel_state.layer_cache = LayerCache { key: Some(key), img: Some(img.clone()), edges: None };
    Ok(img)
}

/// Força das bordas da imagem de referência (laço magnético), guardada com ela.
pub(crate) fn cached_edges(env: &mut Env, r: WandRef) -> Result<(Arc<Vec<f32>>, u32, u32), String> {
    let img = cached_image(env, r)?;
    if let Some(e) = &env.ed.sel_state.layer_cache.edges {
        return Ok((e.clone(), img.width, img.height));
    }
    let e = Arc::new(aiai_paint::area::edge_strength(&img));
    env.ed.sel_state.layer_cache.edges = Some(e.clone());
    Ok((e, img.width, img.height))
}

/// As bordas já calculadas (para a prévia do laço magnético, que só lê).
pub(crate) fn edges_ready(env: &Env) -> Option<(Arc<Vec<f32>>, u32, u32)> {
    let c = &env.ed.sel_state.layer_cache;
    Some((c.edges.clone()?, c.img.as_ref()?.width, c.img.as_ref()?.height))
}

// ------------------------------------------------------------------------------------------------ conta-gotas

/// Conta-gotas "cor da camada": a cor da camada ativa no ponto (sem as outras camadas por cima ou por baixo); pixel
/// transparente não muda a cor.
pub(crate) fn eyedrop_layer(env: &mut Env, p: Vec2) {
    let img = match cached_image(env, WandRef::Editing) {
        Ok(i) => i,
        Err(e) => {
            env.ed.notify(e);
            return;
        }
    };
    if let Some(c) = straight_at(&img, p) {
        super::tools::set_picked_color(env, c);
    }
}

/// Cor não pré-multiplicada no pixel (`None` fora ou transparente).
pub(crate) fn straight_at(img: &aiai_paint::Image, p: Vec2) -> Option<[f32; 4]> {
    let (x, y) = (p.x.floor() as i64, p.y.floor() as i64);
    if x < 0 || y < 0 || x >= img.width as i64 || y >= img.height as i64 {
        return None;
    }
    let c = img.get(x as u32, y as u32);
    if c[3] < 1.0 / 255.0 {
        return None;
    }
    Some([(c[0] / c[3]).min(1.0), (c[1] / c[3]).min(1.0), (c[2] / c[3]).min(1.0), c[3]])
}

// ------------------------------------------------------------------------------------------------ selecionar camada

/// Camadas que dá para escolher pelo desenho, de cima para baixo: as visíveis que pintam algo no quadro.
fn pickable(p: &Project, cut: &NodeId) -> Vec<NodeId> {
    cmds::layer_rows(p, cut)
        .into_iter()
        .filter(|r| cmds::layer_data(p, &r.id).is_some_and(|l| l.visible && matches!(l.kind, LayerType::Vector | LayerType::Raster | LayerType::Fill | LayerType::Text)))
        .map(|r| r.id)
        .collect()
}

/// A camada de cima que tem desenho (alfa ≥ 10%) no ponto `c` do canvas.
pub(crate) fn layer_at(env: &Env, cut: &NodeId, c: Vec2) -> Option<NodeId> {
    let (w, h) = cmds::canvas_size(&env.core.project, cut);
    if c.x < 0.0 || c.y < 0.0 || c.x >= w as f32 || c.y >= h as f32 {
        return None;
    }
    for id in pickable(&env.core.project, cut) {
        let opts = aiai_render::RenderOptions { include_guides: false, include_reference: false, only_layers: Some(vec![id.clone()]), background: None, camera: false, layer_motion: true };
        let Ok(out) = aiai_render::render_frame(&env.core.project, &env.core.blobs, cut, env.ed.frame, &opts) else { continue };
        if out.image.get(c.x as u32, c.y as u32)[3] >= 0.1 {
            return Some(id);
        }
    }
    None
}

/// Mover > Selecionar camada: o clique faz da camada desenhada sob o cursor a camada ativa (no vazio, avisa).
pub(crate) fn pick_layer(env: &mut Env, c: Vec2) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    match layer_at(env, &cut, c) {
        Some(id) => {
            env.ed.layer = Some(id);
            env.ed.selection = None;
        }
        None => env.ed.notify(tr("move.pick_none")),
    }
}

// ------------------------------------------------------------------------------------------------ desenhos da lista

/// Miniatura das sub-ferramentas novas que não são da Seleção nem do Balde.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum AreaThumb {
    Wand(WandRef),
    Eyedrop(bool),
    Rotate,
    PickLayer,
    TextCircle,
}

pub(crate) fn paint_thumb(env: &mut Env, p: &Painter, r: Rect, a: AreaThumb) {
    let t = env.t;
    let at = |x: f32, y: f32| Pos2::new(r.left() + r.width() * x, r.top() + r.height() * y);
    match a {
        AreaThumb::Wand(w) => paint_wand_thumb(p, r, w, &t),
        AreaThumb::Eyedrop(l) => paint_eyedrop_thumb(p, r, l, &t),
        AreaThumb::Rotate => {
            // a folha girada e a seta em arco
            let c = r.center();
            let k = r.height() * 0.32;
            let (s, co) = 0.35f32.sin_cos();
            let corner = |x: f32, y: f32| c + Vec2::new(x * co - y * s, x * s + y * co) * k;
            p.add(egui::Shape::convex_polygon(vec![corner(-1.2, -0.9), corner(1.2, -0.9), corner(1.2, 0.9), corner(-1.2, 0.9)], t.bg_widget, EStroke::new(1.0, t.text)));
            let arc: Vec<Pos2> = (0..=16).map(|i| { let f = -2.6 + i as f32 / 16.0 * 1.9; c + Vec2::new(f.cos(), f.sin()) * r.height() * 0.46 }).collect();
            let tip = *arc.last().unwrap_or(&c);
            p.add(egui::Shape::line(arc, EStroke::new(1.4, t.accent)));
            p.circle_filled(tip, 2.2, t.accent);
        }
        AreaThumb::TextCircle => {
            // letras em volta de um círculo
            let c = r.center();
            let rad = r.height() * 0.36;
            p.circle_stroke(c, rad, EStroke::new(1.0, t.text_dim));
            for k in 0..7 {
                let a = -2.4 + k as f32 * 0.5;
                let q = c + Vec2::new(a.cos(), a.sin()) * (rad + 3.0);
                p.text(q, egui::Align2::CENTER_CENTER, ["A", "B", "C", "D", "E", "F", "G"][k], egui::FontId::proportional(r.height() * 0.22), t.text);
            }
        }
        AreaThumb::PickLayer => {
            // pilha de camadas com a de cima destacada e o cursor clicando no desenho
            for k in 0..3usize {
                let o = Vec2::new(0.0, -(k as f32) * r.height() * 0.16);
                let pts = vec![at(0.3, 0.72) + o, at(0.5, 0.86) + o, at(0.7, 0.72) + o, at(0.5, 0.58) + o];
                p.add(egui::Shape::convex_polygon(pts, if k == 2 { t.accent.gamma_multiply(0.7) } else { t.bg_widget }, EStroke::new(1.0, t.text)));
            }
            let c = at(0.62, 0.3);
            p.add(egui::Shape::convex_polygon(vec![c, c + Vec2::new(0.0, 10.0), c + Vec2::new(3.0, 7.5), c + Vec2::new(7.0, 7.5)], t.text, EStroke::NONE));
        }
    }
}

/// Desenho das sub-ferramentas novas da Seleção (caneta, borracha, encolher, laço magnético).
pub(crate) fn paint_select_thumb(p: &Painter, r: Rect, m: SelTool, t: &crate::theme::Tokens) {
    let at = |x: f32, y: f32| Pos2::new(r.left() + r.width() * x, r.top() + r.height() * y);
    let dash = EStroke::new(1.2, t.text);
    let wave = |y0: f32| -> Vec<Pos2> { (0..=20).map(|k| { let f = k as f32 / 20.0; at(0.15 + 0.7 * f, y0 - 0.18 * (f * std::f32::consts::TAU).sin()) }).collect() };
    match m {
        SelTool::Pen => {
            p.add(egui::Shape::line(wave(0.55), EStroke::new(r.height() * 0.28, t.accent.gamma_multiply(0.45))));
            p.extend(egui::Shape::dashed_line(&wave(0.55), dash, 3.0, 2.0));
        }
        SelTool::Erase => {
            let band = Rect::from_min_max(at(0.12, 0.25), at(0.88, 0.75));
            p.rect_filled(band, 2.0, t.accent.gamma_multiply(0.45));
            p.add(egui::Shape::line(vec![at(0.3, 0.1), at(0.5, 0.5), at(0.7, 0.9)], EStroke::new(r.height() * 0.22, t.bg_panel)));
        }
        SelTool::Shrink => {
            // um laço largo (tracejado) e o desenho dentro, com o contorno justo
            let big = Rect::from_min_max(at(0.08, 0.08), at(0.92, 0.92));
            let pts = vec![big.left_top(), big.right_top(), big.right_bottom(), big.left_bottom(), big.left_top()];
            p.extend(egui::Shape::dashed_line(&pts, EStroke::new(1.0, t.text_dim), 3.0, 3.0));
            p.circle_filled(r.center(), r.height() * 0.2, t.accent.gamma_multiply(0.7));
            p.circle_stroke(r.center(), r.height() * 0.2, dash);
            for (a, b) in [((0.2, 0.2), (0.33, 0.33)), ((0.8, 0.8), (0.67, 0.67))] {
                p.arrow(at(a.0, a.1), at(b.0, b.1) - at(a.0, a.1), EStroke::new(1.0, t.text));
            }
        }
        SelTool::Magnetic => {
            // borda (meio escuro, meio claro) e o laço colado nela
            p.rect_filled(Rect::from_min_max(at(0.1, 0.1), at(0.5, 0.9)), 0.0, t.text_dim.gamma_multiply(0.6));
            let pts = vec![at(0.5, 0.1), at(0.5, 0.9)];
            p.extend(egui::Shape::dashed_line(&pts, dash, 3.0, 2.0));
            // ímã
            let c = at(0.75, 0.45);
            p.circle_stroke(c, r.height() * 0.16, EStroke::new(2.4, t.accent));
            p.rect_filled(Rect::from_center_size(c + Vec2::new(0.0, r.height() * 0.12), Vec2::new(r.height() * 0.4, r.height() * 0.16)), 0.0, t.bg_panel);
        }
        _ => {}
    }
}

/// Desenho das três referências da varinha: uma camada, a pilha toda, a pilha com a de referência marcada.
pub(crate) fn paint_wand_thumb(p: &Painter, r: Rect, w: WandRef, t: &crate::theme::Tokens) {
    let at = |x: f32, y: f32| Pos2::new(r.left() + r.width() * x, r.top() + r.height() * y);
    let sheet = |k: usize, on: bool| {
        let o = Vec2::new(k as f32 * r.width() * 0.08, -(k as f32) * r.height() * 0.14);
        let pts = vec![at(0.18, 0.72) + o, at(0.52, 0.86) + o, at(0.82, 0.68) + o, at(0.48, 0.54) + o];
        p.add(egui::Shape::convex_polygon(pts, if on { t.accent.gamma_multiply(0.7) } else { t.bg_widget }, EStroke::new(1.0, t.text)));
    };
    match w {
        WandRef::Editing => sheet(0, true),
        WandRef::All => (0..3).for_each(|k| sheet(k, true)),
        WandRef::Reference => (0..3).for_each(|k| sheet(k, k == 1)),
    }
    // a varinha
    p.line_segment([at(0.62, 0.5), at(0.85, 0.12)], EStroke::new(1.4, t.text));
    let c = at(0.86, 0.12);
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2 + 0.4;
        p.line_segment([c, c + Vec2::new(a.cos(), a.sin()) * 4.0], EStroke::new(1.0, t.accent));
    }
}

/// Desenho dos jeitos do balde.
pub(crate) fn paint_fill_thumb(p: &Painter, r: Rect, i: u8, t: &crate::theme::Tokens) {
    let at = |x: f32, y: f32| Pos2::new(r.left() + r.width() * x, r.top() + r.height() * y);
    let ink = EStroke::new(1.2, t.text);
    let fill = t.accent.gamma_multiply(0.55);
    let rr = Rect::from_min_max(at(0.2, 0.18), at(0.8, 0.82));
    match FillMode::ALL.get(i as usize).copied().unwrap_or_default() {
        FillMode::Others => {
            // linha numa folha, cor na folha de baixo
            p.rect_filled(rr.translate(Vec2::new(3.0, 3.0)).shrink(1.0), 2.0, fill);
            p.rect_stroke(rr, 2.0, ink, egui::StrokeKind::Inside);
        }
        FillMode::Editing => {
            p.rect_filled(rr.shrink(1.5), 2.0, fill);
            p.rect_stroke(rr, 2.0, ink, egui::StrokeKind::Inside);
        }
        FillMode::Enclose => {
            // duas regiões; o laço cerca as duas e elas enchem
            let (a, b) = (Rect::from_min_max(at(0.22, 0.3), at(0.48, 0.7)), Rect::from_min_max(at(0.52, 0.3), at(0.78, 0.7)));
            for q in [a, b] {
                p.rect_filled(q.shrink(1.5), 1.0, fill);
                p.rect_stroke(q, 1.0, ink, egui::StrokeKind::Inside);
            }
            let lasso: Vec<Pos2> = (0..=24).map(|k| { let f = k as f32 / 24.0 * std::f32::consts::TAU; at(0.5 + 0.42 * f.cos(), 0.5 + 0.38 * f.sin()) }).collect();
            p.extend(egui::Shape::dashed_line(&lasso, EStroke::new(1.0, t.text), 3.0, 2.0));
        }
        FillMode::Unfilled => {
            // contorno com frestas sem tinta e o pincel passando
            p.rect_stroke(rr, 2.0, ink, egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_min_max(at(0.24, 0.22), at(0.6, 0.78)), 1.0, fill);
            p.add(egui::Shape::line(vec![at(0.55, 0.3), at(0.7, 0.5), at(0.6, 0.75)], EStroke::new(r.height() * 0.18, t.accent.gamma_multiply(0.8))));
        }
    }
}

/// Desenho das duas cores do conta-gotas: o que aparece (camadas juntas) e a cor de uma camada.
pub(crate) fn paint_eyedrop_thumb(p: &Painter, r: Rect, layer: bool, t: &crate::theme::Tokens) {
    let at = |x: f32, y: f32| Pos2::new(r.left() + r.width() * x, r.top() + r.height() * y);
    let a = Rect::from_min_max(at(0.12, 0.3), at(0.55, 0.85));
    let b = a.translate(Vec2::new(r.width() * 0.18, -r.height() * 0.18));
    p.rect_filled(a, 1.0, if layer { t.bg_widget } else { t.accent.gamma_multiply(0.5) });
    p.rect_filled(b, 1.0, if layer { t.accent } else { t.accent.gamma_multiply(0.5) });
    p.rect_stroke(b, 1.0, EStroke::new(1.0, t.text), egui::StrokeKind::Inside);
    p.line_segment([at(0.62, 0.55), at(0.9, 0.15)], EStroke::new(2.0, t.text));
    p.circle_filled(at(0.62, 0.55), 2.2, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_texts_of_the_new_modes() {
        use crate::i18n::{Lang, tr_in};
        let mut ids = std::collections::HashSet::new();
        for w in WandRef::ALL {
            assert!(ids.insert(w.id()));
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, w.label_key()), w.label_key());
            }
        }
        for f in FillMode::ALL {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, f.label_key()), f.label_key());
            }
        }
        for k in ["wand.no_reference", "move.pick_none"] {
            for l in [Lang::Pt, Lang::En] {
                assert_ne!(tr_in(l, k), k);
            }
        }
        assert!(FillMode::Enclose.is_path() && FillMode::Unfilled.is_path() && !FillMode::Others.is_path());
    }

    #[test]
    fn straight_color_from_premultiplied() {
        let mut img = aiai_paint::Image::new(2, 1);
        img.data[1] = [0.25, 0.0, 0.0, 0.5];
        assert_eq!(straight_at(&img, Vec2::new(0.5, 0.5)), None, "transparente não dá cor");
        let c = straight_at(&img, Vec2::new(1.5, 0.5)).unwrap();
        assert!((c[0] - 0.5).abs() < 1e-6 && c[3] == 0.5);
        assert_eq!(straight_at(&img, Vec2::new(3.0, 0.5)), None);
    }
}
