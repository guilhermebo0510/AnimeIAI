//! Copiloto de traço na interface (protótipo): caixas de sugestão no canvas, fila única, fantasma azul, Tab/Esc.
//!
//! O gerador roda num processo separado (aiai_tools::copilot); aqui a interface só prepara o contexto (rápido) e
//! manda para uma thread que espera o serviço. Nunca duas caixas gerando ao mesmo tempo: a caixa onde se acabou de
//! desenhar passa na frente; as outras esperam. Nada entra no documento até aceitar (um passo de desfazer).

use super::editor::Xf;
use super::ui_kit::Env;
use aiai_core::{Id, Stroke};
use aiai_tools::copilot::{self as cp, AcceptWhich, Suggestion, SuggestRequest};
use egui::{Color32, Pos2, Rect, RichText, Vec2};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

/// Estado de uma caixa: ligada (entra na fila), pausada (guarda o fantasma, não gera), desligada (nada).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BoxState {
    On,
    Paused,
    Off,
}

#[derive(Clone, Debug)]
pub(crate) struct CBox {
    pub id: u32,
    pub cut: Id,
    /// [x, y, largura, altura] no canvas.
    pub rect: [f32; 4],
    pub prompt: String,
    pub state: BoxState,
}

/// O fantasma de uma caixa.
pub(crate) struct Ghost {
    pub box_id: u32,
    pub s: Suggestion,
    /// Desenhou-se na caixa depois: fica mais claro até chegar o novo.
    pub stale: bool,
}

/// Arrasto de caixa no canvas.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Criando: canto de partida.
    New(Vec2),
    /// Movendo: caixa e deslocamento do ponteiro até o canto.
    Move(u32, Vec2),
    /// Redimensionando pelo canto oposto fixo.
    Resize(u32, Vec2),
}

struct Job {
    box_id: u32,
    prep: cp::Prepared,
}

pub(crate) struct CopilotUi {
    pub on: bool,
    pub improve: bool,
    pub complete: bool,
    pub boxes: Vec<CBox>,
    next_box: u32,
    pub global_prompt: String,
    pub global_on: bool,
    pub steps: u32,
    pub strength: f32,
    pub opacity: f32,
    pub pause_s: f32,
    /// Força do ímã (0 = desligado).
    pub magnet: f32,
    /// Aceitar Melhorar substitui os traços do rascunho que a versão limpa cobre (desligado: mantém o rascunho).
    pub replace_rough: bool,
    pub ghosts: Vec<Ghost>,
    /// Caixas esperando a vez: (caixa, quando mudou por último).
    dirty: Vec<(u32, Instant)>,
    /// Pedido em andamento: (caixa, desde quando).
    busy: Option<(u32, Instant)>,
    tx: Option<Sender<Job>>,
    rx: Option<Receiver<(u32, Result<Suggestion, String>)>>,
    pub new_box_mode: bool,
    drag: Option<Drag>,
    pub last: String,
}

impl Default for CopilotUi {
    fn default() -> Self {
        Self {
            on: false,
            improve: true,
            complete: true,
            boxes: Vec::new(),
            next_box: 1,
            global_prompt: String::new(),
            global_on: true,
            steps: 4,
            strength: 0.8,
            opacity: 0.25,
            pause_s: 0.4,
            magnet: 0.7,
            replace_rough: true,
            ghosts: Vec::new(),
            dirty: Vec::new(),
            busy: None,
            tx: None,
            rx: None,
            new_box_mode: false,
            drag: None,
            last: String::new(),
        }
    }
}

const BOX_COLOR: Color32 = Color32::from_rgb(225, 40, 40);
const GHOST: [u8; 3] = [40, 120, 255];

fn worker(c: &mut CopilotUi) -> &Sender<Job> {
    if c.tx.is_none() {
        let (tx, jobs) = channel::<Job>();
        let (done, rx) = channel();
        std::thread::Builder::new()
            .name("copiloto".into())
            .spawn(move || {
                for j in jobs {
                    let r = cp::finish(j.prep).map_err(|e| e.to_string());
                    if done.send((j.box_id, r)).is_err() {
                        break;
                    }
                }
            })
            .expect("thread do copiloto");
        c.tx = Some(tx);
        c.rx = Some(rx);
    }
    c.tx.as_ref().unwrap()
}

/// Liga/desliga; ao ligar, o serviço começa a carregar os modelos (sem esperar).
pub(crate) fn set_on(env: &mut Env, on: bool) {
    env.ed.copilot.on = on;
    if on {
        if let Err(e) = cp::start_service() {
            env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
            env.ed.copilot.on = false;
        }
    }
}

/// Um traço terminou na camada `layer` do corte: a caixa onde ele caiu entra na fila (sem caixa, nasce uma em volta).
pub(crate) fn on_stroke(env: &mut Env, cut: &Id, bbox: [f32; 4], traced: bool) {
    let c = &mut env.ed.copilot;
    if !c.on || !(c.improve || c.complete) || traced {
        return;
    }
    let (cx, cy) = (bbox[0] + bbox[2] / 2.0, bbox[1] + bbox[3] / 2.0);
    let hit: Vec<u32> = c.boxes.iter().filter(|b| &b.cut == cut && cx >= b.rect[0] && cx <= b.rect[0] + b.rect[2] && cy >= b.rect[1] && cy <= b.rect[1] + b.rect[3]).map(|b| b.id).collect();
    let ids = if hit.is_empty() && !c.boxes.iter().any(|b| &b.cut == cut) {
        // sem caixa neste corte: uma automática em volta do traço (pelo menos 256 px)
        let side = bbox[2].max(bbox[3]).max(256.0);
        let id = c.next_box;
        c.next_box += 1;
        c.boxes.push(CBox { id, cut: cut.clone(), rect: [cx - side / 2.0, cy - side / 2.0, side, side], prompt: String::new(), state: BoxState::On });
        vec![id]
    } else {
        hit
    };
    for id in ids {
        if c.boxes.iter().any(|b| b.id == id && b.state == BoxState::On) {
            for g in c.ghosts.iter_mut().filter(|g| g.box_id == id) {
                g.stale = true;
            }
            // já esperando: só atualiza a hora (não entra duas vezes)
            c.dirty.retain(|d| d.0 != id);
            c.dirty.push((id, Instant::now()));
        }
    }
}

/// Uma vez por quadro: recebe o que ficou pronto e manda a próxima caixa quando a vez chegar.
pub(crate) fn tick(env: &mut Env, ctx: &egui::Context) {
    // o que ficou pronto (recebe tudo primeiro, depois mexe no estado)
    let mut done = Vec::new();
    if let Some(rx) = env.ed.copilot.rx.as_ref() {
        while let Ok(x) = rx.try_recv() {
            done.push(x);
        }
    }
    for (id, r) in done {
        env.ed.copilot.busy = None;
        match r {
            Ok(s) => {
                env.ed.copilot.last = format!("{:.1} s · {} + {}", s.ms as f32 / 1000.0, s.improve.len(), s.complete.len());
                env.ed.copilot.ghosts.retain(|g| g.box_id != id);
                env.ed.copilot.ghosts.push(Ghost { box_id: id, s, stale: false });
            }
            Err(e) => {
                env.ed.copilot.last = e.clone();
                env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
            }
        }
    }
    let c = &env.ed.copilot;
    if !c.on {
        return;
    }
    if c.busy.is_some() || !c.dirty.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    if c.busy.is_some() {
        return;
    }
    // a vez: a caixa que mudou por último, depois da pausa
    let pause = std::time::Duration::from_secs_f32(c.pause_s);
    let Some(&(id, at)) = c.dirty.iter().max_by_key(|d| d.1) else { return };
    if at.elapsed() < pause {
        return;
    }
    env.ed.copilot.dirty.retain(|d| d.0 != id);
    let Some(b) = env.ed.copilot.boxes.iter().find(|b| b.id == id).cloned() else { return };
    let Some(layer) = env.ed.layer.clone() else { return };
    let c = &env.ed.copilot;
    let mut prompt = b.prompt.clone();
    if c.global_on && !c.global_prompt.trim().is_empty() {
        prompt = if prompt.is_empty() { c.global_prompt.clone() } else { format!("{prompt}, {}", c.global_prompt) };
    }
    let req = SuggestRequest {
        cut: b.cut.clone(),
        frame: env.ed.frame,
        layer,
        rect: b.rect,
        prompt,
        improve: c.improve,
        complete: c.complete,
        steps: Some(c.steps),
        res: None,
        strength: Some(c.strength),
        seed: None,
        context: None,
        tip: env.ed.opts.pen_tip,
        color: env.ed.color.clone(),
    };
    match cp::prepare(&env.core.project, &env.core.blobs, &req) {
        Ok(prep) => {
            let tx = worker(&mut env.ed.copilot).clone();
            if tx.send(Job { box_id: id, prep }).is_ok() {
                env.ed.copilot.busy = Some((id, Instant::now()));
            }
        }
        Err(e) => env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title"))),
    }
}

fn rect_of(b: &CBox, xf: &Xf) -> Rect {
    Rect::from_two_pos(xf.to_screen(Vec2::new(b.rect[0], b.rect[1])), xf.to_screen(Vec2::new(b.rect[0] + b.rect[2], b.rect[1] + b.rect[3])))
}

/// Caixas e fantasmas por cima do canvas (só visão).
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &Id) {
    let c = &env.ed.copilot;
    if !c.on && c.ghosts.is_empty() {
        return;
    }
    let frame = env.ed.frame;
    for g in c.ghosts.iter().filter(|g| &g.s.cut == cut && g.s.frame == frame) {
        let a = (c.opacity * if g.stale { 0.45 } else { 1.0 } * 255.0) as u8;
        let col = Color32::from_rgba_unmultiplied(GHOST[0], GHOST[1], GHOST[2], a);
        for st in g.s.improve.iter().chain(&g.s.complete) {
            ghost_line(painter, xf, st, col);
        }
    }
    if !c.on {
        return;
    }
    for b in c.boxes.iter().filter(|b| &b.cut == cut) {
        let r = rect_of(b, xf);
        let col = match b.state {
            BoxState::On => BOX_COLOR,
            BoxState::Paused => BOX_COLOR.gamma_multiply(0.55),
            BoxState::Off => BOX_COLOR.gamma_multiply(0.25),
        };
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Middle);
        // alça de mover (com o número e o estado) e as de redimensionar nos cantos
        let tag = Rect::from_min_size(r.min - Vec2::new(0.0, 16.0), Vec2::new(54.0, 16.0));
        painter.rect_filled(tag, 2.0, col);
        let busy = c.busy.is_some_and(|x| x.0 == b.id);
        let waiting = c.dirty.iter().any(|d| d.0 == b.id);
        let mark = if busy { "…" } else if waiting { "·" } else { "" };
        let st = match b.state {
            BoxState::On => "",
            BoxState::Paused => " ⏸",
            BoxState::Off => " ✕",
        };
        painter.text(tag.left_center() + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}{st}{mark}", b.id), egui::FontId::proportional(11.0), Color32::WHITE);
        for k in [r.right_bottom(), r.left_top(), r.right_top(), r.left_bottom()] {
            painter.rect_filled(Rect::from_center_size(k, Vec2::splat(6.0)), 0.0, col);
        }
    }
}

fn ghost_line(painter: &egui::Painter, xf: &Xf, st: &Stroke, col: Color32) {
    let smp = aiai_paint::sample_stroke(st);
    if smp.len() < 2 {
        return;
    }
    let w = (smp.iter().map(|q| q.r).sum::<f32>() / smp.len() as f32 * 2.0 * xf.scale).max(1.0);
    painter.add(egui::Shape::line(smp.iter().map(|q| xf.to_screen(Vec2::new(q.x, q.y))).collect(), egui::Stroke::new(w, col)));
}

/// Clique no canvas: alças das caixas (mover, redimensionar, estado) e o modo "nova caixa". `true` = consumido.
pub(crate) fn press(env: &mut Env, xf: &Xf, pos: Pos2) -> bool {
    let c = &mut env.ed.copilot;
    if !c.on {
        return false;
    }
    let cv = xf.to_canvas(pos);
    if c.new_box_mode {
        c.drag = Some(Drag::New(cv));
        return true;
    }
    let Some(cut) = env.ed.cut.clone() else { return false };
    let c = &mut env.ed.copilot;
    for b in c.boxes.iter().filter(|b| b.cut == cut) {
        let r = rect_of(b, xf);
        let tag = Rect::from_min_size(r.min - Vec2::new(0.0, 16.0), Vec2::new(54.0, 16.0));
        if tag.contains(pos) {
            c.drag = Some(Drag::Move(b.id, cv - Vec2::new(b.rect[0], b.rect[1])));
            return true;
        }
        for (k, opp) in [(r.right_bottom(), r.left_top()), (r.left_top(), r.right_bottom()), (r.right_top(), r.left_bottom()), (r.left_bottom(), r.right_top())] {
            if (k - pos).length() <= 7.0 {
                c.drag = Some(Drag::Resize(b.id, xf.to_canvas(opp)));
                return true;
            }
        }
    }
    false
}

pub(crate) fn drag(env: &mut Env, xf: &Xf, pos: Pos2) -> bool {
    let c = &mut env.ed.copilot;
    let Some(d) = c.drag else { return false };
    let cv = xf.to_canvas(pos);
    match d {
        Drag::New(_) => {}
        Drag::Move(id, off) => {
            if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                b.rect[0] = cv.x - off.x;
                b.rect[1] = cv.y - off.y;
            }
        }
        Drag::Resize(id, fixed) => {
            if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                b.rect = [fixed.x.min(cv.x), fixed.y.min(cv.y), (cv.x - fixed.x).abs().max(16.0), (cv.y - fixed.y).abs().max(16.0)];
            }
        }
    }
    true
}

pub(crate) fn release(env: &mut Env, xf: &Xf, pos: Pos2) -> bool {
    let Some(d) = env.ed.copilot.drag.take() else { return false };
    let cv = xf.to_canvas(pos);
    if let Drag::New(a) = d {
        let r = [a.x.min(cv.x), a.y.min(cv.y), (cv.x - a.x).abs(), (cv.y - a.y).abs()];
        if r[2] >= 16.0 && r[3] >= 16.0 {
            if let Some(cut) = env.ed.cut.clone() {
                let c = &mut env.ed.copilot;
                let id = c.next_box;
                c.next_box += 1;
                c.boxes.push(CBox { id, cut, rect: r, prompt: String::new(), state: BoxState::On });
                c.dirty.push((id, Instant::now()));
            }
        }
        env.ed.copilot.new_box_mode = false;
    } else if let Drag::Move(id, _) | Drag::Resize(id, _) = d {
        // caixa mudou de lugar: o fantasma dela fica velho e ela volta para a fila
        let c = &mut env.ed.copilot;
        for g in c.ghosts.iter_mut().filter(|g| g.box_id == id) {
            g.stale = true;
        }
        if c.boxes.iter().any(|b| b.id == id && b.state == BoxState::On) {
            c.dirty.retain(|x| x.0 != id);
            c.dirty.push((id, Instant::now()));
        }
    }
    true
}

/// Aceita os fantasmas do quadro (todos, ou só Melhorar/Completar) na camada de cada sugestão: um passo de desfazer
/// por caixa.
pub(crate) fn accept(env: &mut Env, which: AcceptWhich) -> usize {
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    let ghosts: Vec<Ghost> = std::mem::take(&mut env.ed.copilot.ghosts);
    let mut keep = Vec::new();
    let mut n = 0;
    for g in ghosts {
        if Some(&g.s.cut) != cut.as_ref() || g.s.frame != frame {
            keep.push(g);
            continue;
        }
        match cp::accept_command(&env.core.project, &g.s, which, None, env.ed.copilot.replace_rough) {
            Ok(cmd) => match super::commands::run(env.core, cmd) {
                Ok(_) => n += 1,
                Err(e) => env.ed.notify(e),
            },
            Err(e) => {
                env.ed.notify(e.to_string());
                keep.push(g);
            }
        }
    }
    env.ed.copilot.ghosts = keep;
    n
}

/// Descarta os fantasmas do quadro.
pub(crate) fn discard(env: &mut Env) -> usize {
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    let before = env.ed.copilot.ghosts.len();
    env.ed.copilot.ghosts.retain(|g| Some(&g.s.cut) != cut.as_ref() || g.s.frame != frame);
    before - env.ed.copilot.ghosts.len()
}

/// Tab aceita tudo, Esc descarta (só com fantasma à vista e sem gesto em andamento). `true` = consumiu a tecla.
pub(crate) fn keys(env: &mut Env, ui: &egui::Ui) -> bool {
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    if !env.ed.copilot.ghosts.iter().any(|g| Some(&g.s.cut) == cut.as_ref() && g.s.frame == frame) {
        return false;
    }
    let (tab, esc) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::Tab), i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)));
    if tab {
        let n = accept(env, AcceptWhich::All);
        env.ed.notify(format!("{} ({n})", crate::i18n::tr("copilot.accepted")));
    } else if esc {
        discard(env);
    }
    tab || esc
}

/// Janela do copiloto sobre o canvas (só com ele ligado).
pub(crate) fn window(env: &mut Env, ctx: &egui::Context, rect: Rect) {
    if !env.ed.copilot.on {
        return;
    }
    use crate::i18n::tr;
    let mut open = true;
    let state = cp::service_state();
    let shown = egui::Window::new(tr("copilot.title"))
        .id(egui::Id::new("copilot_window"))
        .open(&mut open)
        .default_pos(rect.right_top() + Vec2::new(-300.0, 8.0))
        .default_width(280.0)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            let c = &mut env.ed.copilot;
            let svc = match state {
                "pronto" => tr("copilot.svc.ready"),
                "carregando" => tr("copilot.svc.loading"),
                "gerando" => tr("copilot.svc.busy"),
                _ => tr("copilot.svc.stopped"),
            };
            let fila = c.dirty.len();
            let busy = c.busy.map(|(id, t)| format!(" · {} {id} ({:.0} s)", tr("copilot.generating"), t.elapsed().as_secs_f32())).unwrap_or_default();
            ui.label(RichText::new(format!("{svc}{busy} · {} {fila}", tr("copilot.queue"))).size(11.5));
            if !c.last.is_empty() {
                ui.label(RichText::new(format!("{}: {}", tr("copilot.last"), c.last)).size(11.0));
            }
            ui.horizontal(|ui| {
                ui.toggle_value(&mut c.improve, tr("copilot.improve")).on_hover_text(tr("copilot.improve.tip"));
                ui.toggle_value(&mut c.complete, tr("copilot.complete")).on_hover_text(tr("copilot.complete.tip"));
            });
            ui.checkbox(&mut c.replace_rough, tr("copilot.replace")).on_hover_text(tr("copilot.replace.tip"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut c.global_on, tr("copilot.global"));
                ui.add(egui::TextEdit::singleline(&mut c.global_prompt).desired_width(170.0).hint_text(tr("copilot.global.hint")));
            });
            ui.horizontal(|ui| {
                ui.label(tr("copilot.steps"));
                ui.add(egui::Slider::new(&mut c.steps, 2..=12));
            });
            ui.horizontal(|ui| {
                ui.label(tr("copilot.opacity"));
                let mut v = c.opacity * 100.0;
                if ui.add(egui::Slider::new(&mut v, 10.0..=60.0).suffix(" %").max_decimals(0)).changed() {
                    c.opacity = v / 100.0;
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("copilot.magnet")).on_hover_text(tr("copilot.magnet.tip"));
                let mut v = c.magnet * 100.0;
                if ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %").max_decimals(0)).changed() {
                    c.magnet = v / 100.0;
                }
            });
            ui.horizontal(|ui| {
                ui.label(tr("copilot.pause"));
                ui.add(egui::Slider::new(&mut c.pause_s, 0.2..=2.0).suffix(" s"));
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.selectable_label(c.new_box_mode, tr("copilot.new_box")).on_hover_text(tr("copilot.new_box.tip")).clicked() {
                    c.new_box_mode = !c.new_box_mode;
                }
                if ui.button(tr("copilot.pause_all")).clicked() {
                    for b in c.boxes.iter_mut() {
                        if b.state == BoxState::On {
                            b.state = BoxState::Paused;
                        }
                    }
                    c.dirty.clear();
                }
            });
            let mut remove = None;
            for b in c.boxes.iter_mut() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}", b.id)).strong().color(BOX_COLOR));
                    for (s, key) in [(BoxState::On, "copilot.box.on"), (BoxState::Paused, "copilot.box.paused"), (BoxState::Off, "copilot.box.off")] {
                        if ui.selectable_label(b.state == s, tr(key)).clicked() {
                            b.state = s;
                        }
                    }
                    if ui.small_button("🗑").on_hover_text(tr("copilot.box.delete")).clicked() {
                        remove = Some(b.id);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut b.prompt).desired_width(250.0).hint_text(tr("copilot.box.hint")));
            }
            if let Some(id) = remove {
                c.boxes.retain(|b| b.id != id);
                c.ghosts.retain(|g| g.box_id != id);
                c.dirty.retain(|d| d.0 != id);
            }
            ui.separator();
            let has = !c.ghosts.is_empty();
            let mut act = None;
            ui.add_enabled_ui(has, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr("copilot.accept_all")).clicked() {
                        act = Some(Some(AcceptWhich::All));
                    }
                    if ui.button(tr("copilot.accept_improve")).clicked() {
                        act = Some(Some(AcceptWhich::Improve));
                    }
                    if ui.button(tr("copilot.accept_complete")).clicked() {
                        act = Some(Some(AcceptWhich::Complete));
                    }
                    if ui.button(tr("copilot.discard")).clicked() {
                        act = Some(None);
                    }
                });
            });
            act
        });
    match shown.and_then(|r| r.inner.flatten()) {
        Some(Some(w)) => {
            accept(env, w);
        }
        Some(None) => {
            discard(env);
        }
        None => {}
    }
    if !open {
        env.ed.copilot.on = false;
    }
}

// ------------------------------------------------------------------------------------------------ ímã

/// Uma linha fantasma que o ímã conhece: de qual sugestão, de qual lista e qual traço, com os pontos densos (canvas).
struct MLine {
    box_id: u32,
    sid: u64,
    list: u8,
    idx: usize,
    pts: Vec<Vec2>,
    radii: Vec<f32>,
    /// Primeira e última amostra puxadas por este traço.
    hit: Option<(usize, usize)>,
}

/// Ímã do traço em andamento: puxa a caneta para o fantasma mais perto e guarda o trecho coberto.
pub(crate) struct Magnet {
    lines: Vec<MLine>,
    radius: f32,
    strength: f32,
    pulled: usize,
    total: usize,
}

/// Ímã para um traço de caneta que começa agora (só com fantasma à vista e força > 0). `scale` = px de tela por px do
/// canvas: o raio de 12 px de tela vira canvas.
pub(crate) fn magnet_for(env: &Env, scale: f32) -> Option<Magnet> {
    let c = &env.ed.copilot;
    if !c.on || c.magnet <= 0.0 {
        return None;
    }
    let (cut, frame) = (env.ed.cut.clone()?, env.ed.frame);
    let mut lines = Vec::new();
    for g in c.ghosts.iter().filter(|g| g.s.cut == cut && g.s.frame == frame) {
        for (list, v) in [(0u8, &g.s.improve), (1u8, &g.s.complete)] {
            for (idx, st) in v.iter().enumerate() {
                let smp = aiai_paint::sample_stroke(st);
                if smp.len() >= 2 {
                    lines.push(MLine { box_id: g.box_id, sid: g.s.id, list, idx, pts: smp.iter().map(|q| Vec2::new(q.x, q.y)).collect(), radii: smp.iter().map(|q| q.r).collect(), hit: None });
                }
            }
        }
    }
    (!lines.is_empty()).then(|| Magnet { lines, radius: 12.0 / scale.max(1e-3), strength: c.magnet, pulled: 0, total: 0 })
}

impl Magnet {
    /// O ponto (canvas) puxado para a linha fantasma mais perto, com força que cai com a distância.
    pub(crate) fn pull(&mut self, p: Vec2) -> Vec2 {
        self.total += 1;
        let mut best: Option<(f32, usize, usize, Vec2)> = None;
        for (li, l) in self.lines.iter().enumerate() {
            for k in 0..l.pts.len() - 1 {
                let (a, b) = (l.pts[k], l.pts[k + 1]);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let q = a + ab * t;
                let d = (p - q).length();
                if d <= self.radius && best.is_none_or(|x| d < x.0) {
                    best = Some((d, li, k, q));
                }
            }
        }
        let Some((d, li, k, q)) = best else { return p };
        let l = &mut self.lines[li];
        l.hit = Some(match l.hit {
            Some((a, b)) => (a.min(k), b.max(k + 1)),
            None => (k, k + 1),
        });
        self.pulled += 1;
        let u = d / self.radius;
        let w = self.strength * (1.0 - u * u).powi(2);
        p + (q - p) * w
    }
}

/// Fim do traço com ímã: o trecho de fantasma coberto sai (o traço desenhado entrou no lugar dele); o resto fica.
/// `true` = o traço seguiu o fantasma quase todo (então não pede sugestão nova).
pub(crate) fn magnet_done(env: &mut Env, m: Magnet) -> bool {
    let mostly = m.total > 0 && m.pulled * 10 >= m.total * 7;
    for l in m.lines.iter().filter(|l| l.hit.is_some()) {
        let (a, b) = l.hit.unwrap();
        let Some(g) = env.ed.copilot.ghosts.iter_mut().find(|g| g.box_id == l.box_id && g.s.id == l.sid) else { continue };
        let v = if l.list == 0 { &mut g.s.improve } else { &mut g.s.complete };
        let Some(old) = v.get(l.idx).cloned() else { continue };
        // as sobras antes e depois do trecho coberto viram fantasmas menores (com folga de 2 amostras)
        let mut rest = Vec::new();
        for (from, to) in [(0usize, a.saturating_sub(2)), ((b + 2).min(l.pts.len()), l.pts.len())] {
            if to > from + 2 {
                let mut s = old.clone();
                let w = s.width.max(0.1);
                s.points = (from..to)
                    .step_by(2)
                    .chain(std::iter::once(to - 1))
                    .map(|k| aiai_core::StrokePoint { x: l.pts[k].x, y: l.pts[k].y, pressure: 1.0, tilt: None, t: None, corner: false, width: (2.0 * l.radii[k] / w).max(0.05), id: 0, handle: None })
                    .collect();
                s.points.dedup_by(|x, y| (x.x - y.x).abs() < 1e-3 && (x.y - y.y).abs() < 1e-3);
                s.closed = false;
                if s.points.len() >= 2 {
                    rest.push(s);
                }
            }
        }
        // o traço coberto vira as sobras (marcado vazio para sair; as sobras vão para o fim da lista)
        v[l.idx].points.clear();
        v.extend(rest);
    }
    for g in env.ed.copilot.ghosts.iter_mut() {
        g.s.improve.retain(|s| !s.points.is_empty());
        g.s.complete.retain(|s| !s.points.is_empty());
    }
    env.ed.copilot.ghosts.retain(|g| !g.s.improve.is_empty() || !g.s.complete.is_empty());
    mostly
}

/// Estado para a ponte (GET_UI_STATE): caixas, fila e fantasmas (com uma amostra de pontos de cada traço).
pub(crate) fn state_json(c: &CopilotUi) -> serde_json::Value {
    let pts = |v: &Vec<Stroke>| v.iter().map(|s| s.points.iter().map(|q| [q.x.round(), q.y.round()]).collect::<Vec<_>>()).collect::<Vec<_>>();
    serde_json::json!({
        "on": c.on, "improve": c.improve, "complete": c.complete, "magnet": c.magnet, "replace_rough": c.replace_rough,
        "service": cp::service_state(),
        "busy": c.busy.map(|b| b.0), "queue": c.dirty.iter().map(|d| d.0).collect::<Vec<_>>(),
        "boxes": c.boxes.iter().map(|b| serde_json::json!({"id": b.id, "rect": b.rect, "state": format!("{:?}", b.state), "prompt": b.prompt})).collect::<Vec<_>>(),
        "ghosts": c.ghosts.iter().map(|g| serde_json::json!({"box": g.box_id, "id": g.s.id, "stale": g.stale, "improve": pts(&g.s.improve), "complete": pts(&g.s.complete)})).collect::<Vec<_>>(),
        "last": c.last,
    })
}
