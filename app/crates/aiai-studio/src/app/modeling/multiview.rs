//! Multivista do viewport (como no Blender), vinda do ANIGO (`anigo-studio/src/app/multiview.rs`, commit bc5e83af,
//! mesmo autor): 1, 2 (lado a lado ou empilhadas) ou 4 vistas, cada uma com a sua câmera (perspectiva, ortográfica
//! livre ou alinhada: frente/costas/direita/esquerda/topo/base).
//!
//! A câmera da vista **ativa** fica em `ModelingState::cam` (edição, atalhos e botões continuam usando ela); as das
//! outras ficam em [`MultiView::cams`] e entram em `cam` só enquanto a vista delas é desenhada. A ativa é a que está
//! sob o ponteiro (sem botão apertado) e só ela recebe entrada; as outras só desenham.
//!
//! Mudanças em relação ao ANIGO: cores do `Tokens`, rótulos pelo i18n do AnimeIAI, sem salvar o estado em disco.

use super::camera::{OrbitCamera, ViewPreset};
use egui::{pos2, Rect, Sense, Stroke};

pub(crate) const MAX_VIEWS: usize = super::workbench::MAX_VIEW_SLOTS;
/// Espaço entre vistas vizinhas (pontos lógicos): a linha divisória.
const GAP: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ViewLayout {
    /// Uma vista (a ativa) ocupa a área toda.
    #[default]
    Single,
    /// Duas lado a lado.
    SplitV,
    /// Duas empilhadas.
    SplitH,
    /// Quatro (2×2).
    Quad,
}

impl ViewLayout {
    pub(crate) const ALL: [ViewLayout; 4] = [ViewLayout::Single, ViewLayout::SplitV, ViewLayout::SplitH, ViewLayout::Quad];

    pub(crate) fn count(self) -> usize {
        match self {
            ViewLayout::Single => 1,
            ViewLayout::SplitV | ViewLayout::SplitH => 2,
            ViewLayout::Quad => 4,
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            ViewLayout::Single => "single",
            ViewLayout::SplitV => "split_v",
            ViewLayout::SplitH => "split_h",
            ViewLayout::Quad => "quad",
        }
    }

    pub(crate) fn from_key(k: &str) -> Option<ViewLayout> {
        Self::ALL.into_iter().find(|l| l.key() == k)
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            ViewLayout::Single => "mview.layout.single",
            ViewLayout::SplitV => "mview.layout.split_v",
            ViewLayout::SplitH => "mview.layout.split_h",
            ViewLayout::Quad => "mview.layout.quad",
        }
    }

    /// Divide `area` nos retângulos das vistas, na ordem dos slots (esq→dir, cima→baixo). `ratio` = posição das
    /// divisórias [vertical, horizontal] como fração da largura/altura.
    pub(crate) fn split(self, area: Rect, ratio: [f32; 2]) -> Vec<Rect> {
        let (l, t, r, b) = (area.left(), area.top(), area.right(), area.bottom());
        let (mx, my) = (area.left() + area.width() * ratio[0], area.top() + area.height() * ratio[1]);
        let h = GAP * 0.5;
        let rc = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
        match self {
            ViewLayout::Single => vec![area],
            ViewLayout::SplitV => vec![rc(l, t, mx - h, b), rc(mx + h, t, r, b)],
            ViewLayout::SplitH => vec![rc(l, t, r, my - h), rc(l, my + h, r, b)],
            ViewLayout::Quad => vec![rc(l, t, mx - h, my - h), rc(mx + h, t, r, my - h), rc(l, my + h, mx - h, b), rc(mx + h, my + h, r, b)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewKind {
    Perspective,
    Ortho,
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl ViewKind {
    pub(crate) const ALL: [ViewKind; 8] = [ViewKind::Perspective, ViewKind::Ortho, ViewKind::Front, ViewKind::Back, ViewKind::Right, ViewKind::Left, ViewKind::Top, ViewKind::Bottom];

    pub(crate) fn key(self) -> &'static str {
        match self {
            ViewKind::Perspective => "perspective",
            ViewKind::Ortho => "ortho",
            ViewKind::Front => "front",
            ViewKind::Back => "back",
            ViewKind::Right => "right",
            ViewKind::Left => "left",
            ViewKind::Top => "top",
            ViewKind::Bottom => "bottom",
        }
    }

    pub(crate) fn from_key(k: &str) -> Option<ViewKind> {
        Self::ALL.into_iter().find(|v| v.key() == k)
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            ViewKind::Perspective => "mview.kind.perspective",
            ViewKind::Ortho => "mview.kind.ortho",
            ViewKind::Front => "mview.kind.front",
            ViewKind::Back => "mview.kind.back",
            ViewKind::Right => "mview.kind.right",
            ViewKind::Left => "mview.kind.left",
            ViewKind::Top => "mview.kind.top",
            ViewKind::Bottom => "mview.kind.bottom",
        }
    }

    /// O tipo de vista que a câmera mostra agora (alinhada só se ortográfica e no ângulo exato).
    pub(crate) fn of(cam: &OrbitCamera) -> ViewKind {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        if !cam.orthographic {
            return ViewKind::Perspective;
        }
        const EPS: f32 = 0.02;
        let yaw = cam.yaw.rem_euclid(TAU);
        let near = |a: f32, b: f32| (a - b).abs() < EPS || (a - b).abs() > TAU - EPS;
        if cam.pitch > 1.5 {
            return ViewKind::Top;
        }
        if cam.pitch < -1.5 {
            return ViewKind::Bottom;
        }
        if cam.pitch.abs() < EPS {
            if near(yaw, 0.0) {
                return ViewKind::Front;
            }
            if near(yaw, PI) {
                return ViewKind::Back;
            }
            if near(yaw, FRAC_PI_2) {
                return ViewKind::Right;
            }
            if near(yaw, TAU - FRAC_PI_2) {
                return ViewKind::Left;
            }
        }
        ViewKind::Ortho
    }

    pub(crate) fn apply(self, cam: &mut OrbitCamera) {
        let preset = match self {
            ViewKind::Perspective => {
                cam.orthographic = false;
                return;
            }
            ViewKind::Ortho => {
                cam.orthographic = true;
                return;
            }
            ViewKind::Front => ViewPreset::Front,
            ViewKind::Back => ViewPreset::Back,
            ViewKind::Right => ViewPreset::Right,
            ViewKind::Left => ViewPreset::Left,
            ViewKind::Top => ViewPreset::Top,
            ViewKind::Bottom => ViewPreset::Bottom,
        };
        cam.orthographic = true;
        cam.set_preset(preset);
    }

    /// Vistas de partida do quadro 2×2 (como o Blender): perspectiva, frente, direita, topo.
    pub(crate) fn default_for_slot(slot: usize) -> ViewKind {
        match slot {
            1 => ViewKind::Front,
            2 => ViewKind::Right,
            3 => ViewKind::Top,
            _ => ViewKind::Perspective,
        }
    }
}

pub(crate) struct MultiView {
    pub layout: ViewLayout,
    /// O arranjo para onde "maximizar" volta.
    pub restore: ViewLayout,
    pub cams: [OrbitCamera; MAX_VIEWS],
    /// A vista já ganhou a câmera de partida.
    pub seeded: [bool; MAX_VIEWS],
    pub active: usize,
    pub rects: [Rect; MAX_VIEWS],
    pub ratio: [f32; 2],
}

impl Default for MultiView {
    fn default() -> Self {
        MultiView { layout: ViewLayout::Single, restore: ViewLayout::Quad, cams: [OrbitCamera::default(); MAX_VIEWS], seeded: [true, false, false, false], active: 0, rects: [Rect::NOTHING; MAX_VIEWS], ratio: [0.5, 0.5] }
    }
}

impl MultiView {
    pub(crate) fn visible(&self) -> Vec<usize> {
        match self.layout {
            ViewLayout::Single => vec![self.active],
            l => (0..l.count()).collect(),
        }
    }

    pub(crate) fn slot_at(&self, pos: egui::Pos2) -> Option<usize> {
        self.visible().into_iter().find(|&s| self.rects[s].contains(pos))
    }

    /// Uma vista ⇄ o último arranjo de várias (Ctrl+Espaço do Blender).
    pub(crate) fn toggle_maximize(&mut self, active_cam: &mut OrbitCamera) {
        if self.layout == ViewLayout::Single {
            self.set_layout(active_cam, self.restore);
        } else {
            self.set_layout(active_cam, ViewLayout::Single);
        }
    }

    pub(crate) fn set_layout(&mut self, active_cam: &mut OrbitCamera, layout: ViewLayout) {
        if self.layout != ViewLayout::Single && layout == ViewLayout::Single {
            self.restore = self.layout;
        }
        if layout != ViewLayout::Single {
            self.restore = layout;
        }
        self.layout = layout;
        if layout != ViewLayout::Single && self.active >= layout.count() {
            self.set_active(active_cam, 0);
        }
        self.seed(active_cam);
    }

    /// As vistas que aparecem pela primeira vez partem da câmera ativa, no tipo de partida do slot.
    pub(crate) fn seed(&mut self, active_cam: &OrbitCamera) {
        for s in self.visible() {
            if !self.seeded[s] {
                let mut cam = *active_cam;
                let kind = ViewKind::default_for_slot(s);
                if matches!(kind, ViewKind::Top | ViewKind::Bottom) {
                    cam.yaw = 0.0;
                }
                kind.apply(&mut cam);
                self.cams[s] = cam;
                self.seeded[s] = true;
            }
        }
    }

    pub(crate) fn set_active(&mut self, active_cam: &mut OrbitCamera, slot: usize) {
        let slot = slot.min(MAX_VIEWS - 1);
        if slot == self.active {
            return;
        }
        self.cams[self.active] = *active_cam;
        *active_cam = self.cams[slot];
        self.active = slot;
    }

    /// A vista sob o ponteiro vira a ativa (sem botão apertado, sem gesto em andamento, sem menu aberto).
    pub(crate) fn follow_pointer(&mut self, active_cam: &mut OrbitCamera, ctx: &egui::Context, busy: bool) {
        if self.layout == ViewLayout::Single || busy || ctx.any_popup_open() {
            return;
        }
        if ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        if let Some(slot) = ctx.input(|i| i.pointer.hover_pos()).and_then(|p| self.slot_at(p)) {
            self.set_active(active_cam, slot);
        }
    }

    pub(crate) fn pane_rects(&self, area: Rect) -> Vec<Rect> {
        self.layout.split(area, self.ratio)
    }

    /// Divisórias arrastáveis (duplo clique centraliza; no 2×2 o cruzamento move as duas).
    pub(crate) fn dividers_ui(&mut self, ui: &mut egui::Ui, area: Rect, t: &crate::theme::Tokens, notes: &mut Vec<(String, Rect)>) {
        const HIT: f32 = 8.0;
        let layout = self.layout;
        if layout == ViewLayout::Single || area.width() < 1.0 || area.height() < 1.0 {
            return;
        }
        let vertical = matches!(layout, ViewLayout::SplitV | ViewLayout::Quad);
        let horizontal = matches!(layout, ViewLayout::SplitH | ViewLayout::Quad);
        let x = area.left() + area.width() * self.ratio[0];
        let y = area.top() + area.height() * self.ratio[1];
        let id = ui.id().with("mview_dividers");
        let line = |on: bool| Stroke::new(if on { 2.5 } else { 1.0 }, if on { t.accent } else { t.border });
        let mut new = self.ratio;
        let mut cursor = None;
        if vertical {
            let r = Rect::from_min_max(pos2(x - HIT * 0.5, area.top()), pos2(x + HIT * 0.5, area.bottom()));
            notes.push(("mdl:divider.v".into(), r));
            let resp = ui.interact(r, id.with("v"), Sense::click_and_drag());
            let on = resp.hovered() || resp.dragged();
            if on {
                cursor = Some(egui::CursorIcon::ResizeHorizontal);
            }
            ui.painter().vline(x, area.y_range(), line(on));
            if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                new[0] = fraction(p.x, area.left(), area.width());
            }
            if resp.double_clicked() {
                new[0] = 0.5;
            }
        }
        if horizontal {
            let r = Rect::from_min_max(pos2(area.left(), y - HIT * 0.5), pos2(area.right(), y + HIT * 0.5));
            notes.push(("mdl:divider.h".into(), r));
            let resp = ui.interact(r, id.with("h"), Sense::click_and_drag());
            let on = resp.hovered() || resp.dragged();
            if on {
                cursor = Some(egui::CursorIcon::ResizeVertical);
            }
            ui.painter().hline(area.x_range(), y, line(on));
            if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                new[1] = fraction(p.y, area.top(), area.height());
            }
            if resp.double_clicked() {
                new[1] = 0.5;
            }
        }
        if layout == ViewLayout::Quad {
            let r = Rect::from_center_size(pos2(x, y), egui::vec2(HIT * 2.5, HIT * 2.5));
            let resp = ui.interact(r, id.with("c"), Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                cursor = Some(egui::CursorIcon::Move);
                ui.painter().circle_filled(pos2(x, y), 4.0, t.accent);
            }
            if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
                new[0] = fraction(p.x, area.left(), area.width());
                new[1] = fraction(p.y, area.top(), area.height());
            }
            if resp.double_clicked() {
                new = [0.5, 0.5];
            }
        }
        if let Some(c) = cursor {
            ui.ctx().set_cursor_icon(c);
        }
        self.ratio = new;
    }
}

/// Fração da divisória, com as vistas nunca menores que 10% da área.
fn fraction(p: f32, start: f32, len: f32) -> f32 {
    ((p - start) / len.max(1.0)).clamp(0.1, 0.9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_split_the_area_in_slot_order() {
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        assert_eq!(ViewLayout::Single.split(area, [0.5, 0.5]), vec![area]);
        let q = ViewLayout::Quad.split(area, [0.5, 0.5]);
        assert_eq!(q.len(), 4);
        assert!(q[0].center().x < q[1].center().x && q[0].center().y < q[2].center().y);
        let v = ViewLayout::SplitV.split(area, [0.25, 0.5]);
        assert!((v[0].width() - 49.0).abs() < 0.01, "{:?}", v[0]);
        for l in ViewLayout::ALL {
            assert_eq!(ViewLayout::from_key(l.key()), Some(l));
            assert_eq!(l.split(area, [0.5, 0.5]).len(), l.count());
        }
    }

    #[test]
    fn view_kinds_round_trip_through_the_camera() {
        for k in ViewKind::ALL {
            let mut c = OrbitCamera::default();
            k.apply(&mut c);
            if k != ViewKind::Ortho {
                assert_eq!(ViewKind::of(&c), k, "{k:?}");
            }
            assert_eq!(ViewKind::from_key(k.key()), Some(k));
        }
    }

    #[test]
    fn quad_seeds_front_right_top_and_swaps_cameras() {
        let mut mv = MultiView::default();
        let mut cam = OrbitCamera { yaw: 0.6, pitch: 0.3, ..OrbitCamera::default() };
        mv.set_layout(&mut cam, ViewLayout::Quad);
        assert_eq!(ViewKind::of(&mv.cams[1]), ViewKind::Front);
        assert_eq!(ViewKind::of(&mv.cams[2]), ViewKind::Right);
        assert_eq!(ViewKind::of(&mv.cams[3]), ViewKind::Top);
        let persp = cam;
        mv.set_active(&mut cam, 2);
        assert_eq!(ViewKind::of(&cam), ViewKind::Right);
        assert_eq!(mv.cams[0], persp, "a câmera da vista que saiu fica guardada");
        mv.toggle_maximize(&mut cam);
        assert_eq!((mv.layout, mv.active), (ViewLayout::Single, 2));
        mv.toggle_maximize(&mut cam);
        assert_eq!(mv.layout, ViewLayout::Quad);
    }
}
