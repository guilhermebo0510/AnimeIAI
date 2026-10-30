//! Copiloto de traço na interface (protótipo): caixas de sugestão no canvas, fila única, fantasma magenta, Tab/Esc.
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
    /// Semente da geração ("outra versão" troca).
    pub seed: u64,
    /// Caixa de foco: a única automática do corte, que vai para onde você desenha. As outras são fixas (criadas
    /// com + ou fixadas a partir do foco) e ficam onde estão.
    pub auto: bool,
    /// Camada de onde a caixa lê o desenho (a do traço que a criou ou moveu). Trocar de camada, ou um pincel que cria
    /// uma camada sozinho, não muda o que a caixa lê.
    pub layer: Option<Id>,
    /// Somar o prompt geral ao desta caixa (desligue numa caixa sobre a mão quando o geral descreve o rosto).
    pub use_global: bool,
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
    /// Passos do modelo (0 = os da mudança escolhida).
    pub steps: u32,
    /// Quanto a sugestão pode mudar o desenho (0 = só limpar … 1 = criar a partir do esboço).
    pub change: f32,
    /// Resolução de geração em px (0 = automática: o tamanho da janela de contexto, de 384 a 512).
    pub res: u32,
    pub opacity: f32,
    pub pause_s: f32,
    /// Força do ímã (0 = desligado).
    pub magnet: f32,
    /// Cor da caixa e do fantasma (opacas; a do fantasma recebe a opacidade `opacity`).
    pub box_color: Color32,
    pub ghost_color: Color32,
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
    /// Desfazer/refazer já vistos (`Session::history_moves`): mudou, os fantasmas caem.
    seen_moves: u64,
    /// Mudanças no documento já vistas (`Session::change_counter`): outra edição (transformar, preencher, colar…)
    /// põe as caixas ligadas do corte na fila.
    seen_change: u64,
    /// Onde aceitar: na própria camada ou numa camada limpa acima.
    pub target: cp::AcceptTarget,
    /// Tirar o foco do teclado no próximo quadro (depois do Tab que aceitou).
    drop_focus: bool,
    /// Pedido ao dock: mostrar (true) ou esconder (false) o painel Copiloto.
    pub panel_req: Option<bool>,
    /// Um campo de texto tinha o teclado no quadro anterior (lido em `keys`).
    typing: bool,
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
            steps: 0,
            change: cp::CHANGE_DEFAULT,
            res: 0,
            opacity: 0.25,
            pause_s: 0.4,
            magnet: 0.7,
            box_color: BOX_COLOR,
            ghost_color: GHOST_COLOR,
            replace_rough: true,
            ghosts: Vec::new(),
            dirty: Vec::new(),
            busy: None,
            tx: None,
            rx: None,
            new_box_mode: false,
            drag: None,
            last: String::new(),
            seen_moves: 0,
            seen_change: 0,
            target: cp::AcceptTarget::Same,
            drop_focus: false,
            panel_req: None,
            typing: false,
        }
    }
}

/// Cores padrão da caixa e do fantasma: fora das cores de trabalho do genga e do douga (preto = linha, vermelho e azul =
/// sombra e luz), para nunca se confundir com o desenho. Trocáveis em Ajustes.
pub(crate) const BOX_COLOR: Color32 = Color32::from_rgb(22, 170, 80);
pub(crate) const GHOST_COLOR: Color32 = Color32::from_rgb(214, 36, 160);

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

/// A caixa volta para a fila (o fantasma dela fica velho até chegar o novo). Só caixas ligadas.
fn requeue(c: &mut CopilotUi, id: u32) {
    if !c.boxes.iter().any(|b| b.id == id && b.state == BoxState::On) {
        return;
    }
    for g in c.ghosts.iter_mut().filter(|g| g.box_id == id) {
        g.stale = true;
    }
    // já esperando: só atualiza a hora (não entra duas vezes)
    c.dirty.retain(|d| d.0 != id);
    c.dirty.push((id, Instant::now()));
}

/// Todas as caixas ligadas do corte voltam para a fila (mudou um ajuste ou o prompt geral).
fn requeue_cut(c: &mut CopilotUi, cut: Option<&Id>) {
    let ids: Vec<u32> = c.boxes.iter().filter(|b| Some(&b.cut) == cut).map(|b| b.id).collect();
    for id in ids {
        requeue(c, id);
    }
}

/// Liga/desliga. Ao ligar, o serviço começa a carregar os modelos (sem esperar). Ao desligar, os fantasmas e a fila
/// somem (a geração em andamento termina no serviço, mas o resultado é descartado).
pub(crate) fn set_on(env: &mut Env, on: bool) {
    env.ed.copilot.on = on;
    env.ed.copilot.panel_req = Some(on);
    env.ed.copilot.seen_change = env.core.change_counter;
    env.ed.copilot.seen_moves = env.core.history_moves;
    if !on {
        let c = &mut env.ed.copilot;
        c.ghosts.clear();
        c.dirty.clear();
        c.new_box_mode = false;
        c.drag = None;
    }
    if on {
        if let Err(e) = cp::start_service() {
            env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
            env.ed.copilot.on = false;
        }
    }
}

/// Um traço terminou no corte: as caixas onde ele caiu entram na fila. Fora de todas, a caixa de FOCO (no máximo
/// uma por corte) vai até ele (nasce na primeira vez); nunca nasce uma caixa por traço.
pub(crate) fn on_stroke(env: &mut Env, cut: &Id, bbox: [f32; 4], traced: bool, make_box: bool) {
    // esta mudança já foi tratada aqui (o `tick` não precisa refazer o corte todo)
    env.ed.copilot.seen_change = env.core.change_counter;
    let layer = env.ed.layer.clone();
    let c = &mut env.ed.copilot;
    if !c.on || !(c.improve || c.complete) || traced {
        return;
    }
    let (cx, cy) = (bbox[0] + bbox[2] / 2.0, bbox[1] + bbox[3] / 2.0);
    // caixas sobrepostas: o traço refaz só a MENOR que o contém (a mais específica), nunca várias de uma vez
    let hit: Vec<u32> = c
        .boxes
        .iter()
        .filter(|b| &b.cut == cut && cx >= b.rect[0] && cx <= b.rect[0] + b.rect[2] && cy >= b.rect[1] && cy <= b.rect[1] + b.rect[3])
        .min_by(|a, b| (a.rect[2] * a.rect[3]).total_cmp(&(b.rect[2] * b.rect[3])))
        .map(|b| b.id)
        .into_iter()
        .collect();
    let ids = if make_box && hit.is_empty() {
        // fora das caixas: o foco vai até o traço, no tamanho que já tem (pelo menos 256 e cobrindo o traço)
        let min = bbox[2].max(bbox[3]).max(256.0);
        match c.boxes.iter_mut().find(|b| &b.cut == cut && b.auto) {
            Some(b) => {
                b.layer = layer.clone();
                let (w, h) = (b.rect[2].max(bbox[2] + 32.0).max(256.0), b.rect[3].max(bbox[3] + 32.0).max(256.0));
                b.rect = [cx - w / 2.0, cy - h / 2.0, w, h];
                let id = b.id;
                // o fantasma era de outro lugar
                c.ghosts.retain(|g| g.box_id != id);
                vec![id]
            }
            None => {
                let id = c.next_box;
                c.next_box += 1;
                c.boxes.push(CBox { id, cut: cut.clone(), rect: [cx - min / 2.0, cy - min / 2.0, min, min], prompt: String::new(), state: BoxState::On, seed: 1, auto: true, layer: layer.clone(), use_global: true });
                vec![id]
            }
        }
    } else {
        hit
    };
    for id in ids {
        requeue(c, id);
    }
}

/// Uma vez por quadro: recebe o que ficou pronto e manda a próxima caixa quando a vez chegar.
pub(crate) fn tick(env: &mut Env, ctx: &egui::Context) {
    if std::mem::take(&mut env.ed.copilot.drop_focus) {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    // o que ficou pronto (recebe tudo primeiro, depois mexe no estado)
    let mut done = Vec::new();
    if let Some(rx) = env.ed.copilot.rx.as_ref() {
        while let Ok(x) = rx.try_recv() {
            done.push(x);
        }
    }
    let moves = env.core.history_moves;
    let change = env.core.change_counter;
    if moves != env.ed.copilot.seen_moves {
        // desfazer/refazer: os fantasmas foram calculados sobre um desenho que não existe mais (não gera de novo sozinho)
        env.ed.copilot.seen_moves = moves;
        env.ed.copilot.seen_change = change;
        env.ed.copilot.ghosts.clear();
    } else if change != env.ed.copilot.seen_change {
        // outra edição (transformar, preencher, colar, apagar camada…): as caixas ligadas do corte geram de novo
        env.ed.copilot.seen_change = change;
        if env.ed.copilot.on {
            let cut = env.ed.cut.clone();
            requeue_cut(&mut env.ed.copilot, cut.as_ref());
        }
    }
    for (id, r) in done {
        env.ed.copilot.busy = None;
        // desligado: o que chegar é descartado (o painel fechado não deixa fantasma para trás)
        if !env.ed.copilot.on {
            continue;
        }
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
    let Some(layer) = b.layer.clone().filter(|l| env.core.project.node(l).is_some()).or_else(|| env.ed.layer.clone()) else { return };
    let c = &env.ed.copilot;
    let mut prompt = b.prompt.clone();
    // o da caixa primeiro (pesa mais: diz o que há NESTA caixa); o geral depois, como contexto do todo
    if c.global_on && b.use_global && !c.global_prompt.trim().is_empty() {
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
        steps: (c.steps > 0).then_some(c.steps),
        res: (c.res > 0).then_some(c.res),
        change: Some(c.change),
        strength: None,
        control_end: None,
        cfg: None,
        seed: Some(b.seed),
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

/// O traço do fantasma da caixa `id` cai (pelo meio) dentro de uma caixa MENOR do mesmo corte, ligada ou pausada, que tem
/// o próprio fantasma: na sobreposição vale o da caixa menor (o do outro não aparece nem entra ao aceitar).
fn shadowed(c: &CopilotUi, id: u32, st: &Stroke) -> bool {
    let Some(me) = c.boxes.iter().find(|b| b.id == id) else { return false };
    let Some(m) = st.points.get(st.points.len() / 2) else { return false };
    let area = me.rect[2] * me.rect[3];
    c.boxes.iter().any(|b| {
        b.id != id
            && b.cut == me.cut
            && b.state != BoxState::Off
            && b.rect[2] * b.rect[3] < area
            && m.x >= b.rect[0]
            && m.x <= b.rect[0] + b.rect[2]
            && m.y >= b.rect[1]
            && m.y <= b.rect[1] + b.rect[3]
            && c.ghosts.iter().any(|g| g.box_id == b.id)
    })
}

/// A sugestão de um fantasma sem os traços que outra caixa menor cobre.
fn visible_suggestion(c: &CopilotUi, g: &Ghost) -> Suggestion {
    let mut s = g.s.clone();
    s.improve.retain(|st| !shadowed(c, g.box_id, st));
    s.complete.retain(|st| !shadowed(c, g.box_id, st));
    s
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
        let gc = c.ghost_color;
        let col = Color32::from_rgba_unmultiplied(gc.r(), gc.g(), gc.b(), a);
        for st in g.s.improve.iter().chain(&g.s.complete) {
            if !shadowed(c, g.box_id, st) {
                ghost_line(painter, xf, st, col);
            }
        }
    }
    if !c.on {
        return;
    }
    for b in c.boxes.iter().filter(|b| &b.cut == cut) {
        let r = rect_of(b, xf);
        let col = match b.state {
            BoxState::On => c.box_color,
            BoxState::Paused => c.box_color.gamma_multiply(0.55),
            BoxState::Off => c.box_color.gamma_multiply(0.25),
        };
        if b.auto {
            // foco: tracejada (segue o desenho); fixas: contínuas
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.0, col), 6.0, 4.0));
        } else {
            painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Middle);
        }
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
        painter.text(tag.left_center() + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}{st}{mark}", if b.auto { crate::i18n::tr("copilot.focus").to_string() } else { b.id.to_string() }), egui::FontId::proportional(11.0), Color32::WHITE);
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
                let layer = env.ed.layer.clone();
                let c = &mut env.ed.copilot;
                c.boxes.push(CBox { id, cut, rect: r, prompt: String::new(), state: BoxState::On, seed: 1, auto: false, layer, use_global: true });
                c.dirty.push((id, Instant::now()));
            }
        }
        env.ed.copilot.new_box_mode = false;
    } else if let Drag::Move(id, _) | Drag::Resize(id, _) = d {
        // caixa mudou de lugar: o fantasma dela fica velho e ela volta para a fila
        requeue(&mut env.ed.copilot, id);
    }
    true
}

/// Aceita os fantasmas do quadro (todos, ou só Melhorar/Completar) na camada de cada sugestão: um passo de desfazer
/// por caixa.
pub(crate) fn accept(env: &mut Env, which: AcceptWhich) -> usize {
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    // o que cada fantasma mostra (sem o que uma caixa menor cobre) é o que entra
    let shown: Vec<Suggestion> = env.ed.copilot.ghosts.iter().map(|g| visible_suggestion(&env.ed.copilot, g)).collect();
    let ghosts: Vec<Ghost> = std::mem::take(&mut env.ed.copilot.ghosts);
    let mut keep = Vec::new();
    let mut n = 0;
    for (g, vis) in ghosts.into_iter().zip(shown) {
        if Some(&g.s.cut) != cut.as_ref() || g.s.frame != frame {
            keep.push(g);
            continue;
        }
        match cp::accept_command_to(&env.core.project, &vis, which, None, env.ed.copilot.target, env.ed.copilot.replace_rough, &mut env.core.ids) {
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
    // várias caixas aceitas de uma vez (Tab) = um passo só de desfazer: o Ctrl+Z volta tudo, inclusive a camada limpa
    // criada pela primeira delas
    if n >= 2 {
        env.core.merge_last(n, &format!("Copiloto: aceitar {n} caixas"));
    }
    // o próprio aceitar mudou o documento: não é motivo para gerar de novo
    env.ed.copilot.seen_change = env.core.change_counter;
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
    // havia um campo de texto com o teclado no quadro ANTERIOR? (no quadro do Tab o egui já passou o foco para o
    // próximo campo, inclusive nos painéis desenhados antes do canvas, então o estado de agora não serve)
    let typing = std::mem::replace(&mut env.ed.copilot.typing, ui.ctx().egui_wants_keyboard_input());
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    if !env.ed.copilot.ghosts.iter().any(|g| Some(&g.s.cut) == cut.as_ref() && g.s.frame == frame) {
        return false;
    }
    // digitando num campo (o prompt): Tab e Esc são do campo
    if typing {
        return false;
    }
    let (tab, esc) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::Tab), i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)));
    if tab {
        // o egui também anda o foco com Tab: cancela, e no próximo quadro ninguém fica com o foco (o Ctrl+Z seguinte
        // tem de desfazer o aceitar, não o texto de um campo do painel)
        ui.ctx().memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        env.ed.copilot.drop_focus = true;
        let n = accept(env, AcceptWhich::All);
        env.ed.notify(format!("{} ({n})", crate::i18n::tr("copilot.accepted")));
    } else if esc {
        discard(env);
    }
    tab || esc
}

/// Corpo do painel Copiloto (no dock, coluna própria à direita; nunca por cima do desenho): estado, o que sugerir e quanto
/// mudar, prompt geral, caixas (cada uma com estado, tamanho de geração e prompt), aceitar e, fechados, os ajustes finos.
pub(crate) fn panel(env: &mut Env, ui: &mut egui::Ui) {
    use crate::app::ui_kit::section;
    use crate::icons::IconId;
    use crate::i18n::tr;
    let state = cp::service_state();
    let cur_cut = env.ed.cut.clone();
    let t = env.t;
    // onde cada caixa vai gerar (janela de contexto e tamanho de geração), para mostrar antes de gerar
    let res = env.ed.copilot.res;
    // o foco primeiro, depois as fixas
    let mut plans: Vec<(u32, bool, [f32; 4], Option<([u32; 4], [u32; 2])>)> = env
        .ed
        .copilot
        .boxes
        .iter()
        .filter(|b| Some(&b.cut) == cur_cut.as_ref())
        .map(|b| (b.id, b.auto, b.rect, cp::plan(&env.core.project, &b.cut, b.rect, None, (res > 0).then_some(res)).ok()))
        .collect();
    plans.sort_by_key(|p| !p.1);
    if !env.ed.copilot.on {
        // desligado: o painel explica e liga
        ui.add_space(6.0);
        ui.label(RichText::new(tr("copilot.off")).size(12.0).color(t.text_dim));
        ui.add_space(4.0);
        if ui.button(RichText::new(tr("copilot.turn_on")).strong()).clicked() {
            set_on(env, true);
        }
        return;
    }

    ui.spacing_mut().item_spacing.y = 5.0;
    let mut act: Option<Option<AcceptWhich>> = None;
    let mut again = false;

    // --- estado: serviço, fila e a última geração numa linha só
    {
        let c = &env.ed.copilot;
        let (dot, svc) = match state {
            "pronto" if c.busy.is_some() => (t.st_progress, tr("copilot.svc.busy").to_string()),
            "pronto" => (t.st_ok, tr("copilot.svc.ready").to_string()),
            "carregando" => (t.st_progress, tr("copilot.svc.loading").to_string()),
            "gerando" => (t.st_progress, tr("copilot.svc.busy").to_string()),
            _ => (t.text_dim, tr("copilot.svc.stopped").to_string()),
        };
        let busy = c.busy.map(|(id, at)| format!(" · {} {id} · {:.0} s", tr("copilot.generating"), at.elapsed().as_secs_f32())).unwrap_or_default();
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            ui.label(RichText::new(format!("{svc}{busy}")).size(12.0).color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{} {}", tr("copilot.queue"), c.dirty.len())).size(11.0).color(t.text_dim));
            });
        });
        if !c.last.is_empty() {
            ui.label(RichText::new(format!("{}: {}", tr("copilot.last"), c.last)).size(11.0).color(t.text_dim));
        }
    }

    // --- o que sugerir e quanto mudar
    section(ui, &t, "copilot.sec.suggest");
    {
        let c = &mut env.ed.copilot;
        ui.horizontal(|ui| {
            again |= ui.toggle_value(&mut c.improve, tr("copilot.improve")).on_hover_text(tr("copilot.improve.tip")).changed();
            again |= ui.toggle_value(&mut c.complete, tr("copilot.complete")).on_hover_text(tr("copilot.complete.tip")).changed();
        });
    }
    let mut notes: Vec<(String, Rect)> = Vec::new();
    {
        let c = &mut env.ed.copilot;
        ui.label(RichText::new(tr("copilot.change")).size(12.0).color(t.text_dim)).on_hover_text(tr("copilot.change.tip"));
        let mut v = c.change * 100.0;
        let w = (ui.available_width() - 64.0).max(80.0);
        ui.spacing_mut().slider_width = w;
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).max_decimals(0)).on_hover_text(tr("copilot.change.tip"));
        // o trilho (sem a caixa do número), para quem clica numa posição
        notes.push(("copilot:change".into(), Rect::from_min_size(r.rect.min, Vec2::new(w, r.rect.height()))));
        if r.changed() {
            c.change = v / 100.0;
            again = true;
        }
    }

    // --- prompt geral: largo, duas linhas; vale ao sair do campo
    section(ui, &t, "copilot.sec.prompt");
    {
        let c = &mut env.ed.copilot;
        let r = ui.add(egui::TextEdit::multiline(&mut c.global_prompt).desired_rows(2).desired_width(f32::INFINITY).hint_text(tr("copilot.global.hint")));
        notes.push(("copilot:global".into(), r.rect));
        again |= prompt_done(ui, &r, &mut c.global_prompt) && c.global_on;
        again |= ui.checkbox(&mut c.global_on, RichText::new(tr("copilot.global.all")).size(12.0)).changed();
    }

    // --- caixas
    ui.horizontal(|ui| {
        section_inline(ui, &t, tr("copilot.sec.boxes"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if env.icon_button_sized(ui, IconId::Play, "copilot.on_all", false, true, "copilot:on_all", 22.0, false).clicked() {
                let c = &mut env.ed.copilot;
                for b in c.boxes.iter_mut().filter(|b| Some(&b.cut) == cur_cut.as_ref()) {
                    b.state = BoxState::On;
                }
                again = true;
            }
            if env.icon_button_sized(ui, IconId::Pause, "copilot.pause_all", false, true, "copilot:pause_all", 22.0, false).clicked() {
                let c = &mut env.ed.copilot;
                for b in c.boxes.iter_mut().filter(|b| b.state == BoxState::On) {
                    b.state = BoxState::Paused;
                }
                c.dirty.clear();
            }
            let nb = env.ed.copilot.new_box_mode;
            if env.icon_button_sized(ui, IconId::Plus, "copilot.new_box.tip", nb, true, "copilot:new_box", 22.0, false).clicked() {
                env.ed.copilot.new_box_mode = !nb;
            }
        });
    });
    if plans.is_empty() {
        ui.label(RichText::new(tr("copilot.boxes.empty")).size(11.5).color(t.text_dim));
    }
    let mut remove = None;
    let mut redo_box = Vec::new();
    for (id, auto, rect_b, plan) in &plans {
        let (id, auto) = (*id, *auto);
        egui::Frame::new().stroke(egui::Stroke::new(1.0, t.border)).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let badge = if auto { tr("copilot.focus").to_string() } else { id.to_string() };
                let (r, resp) = ui.allocate_exact_size(Vec2::new(if auto { 40.0 } else { 22.0 }, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 3.0, env.ed.copilot.box_color);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, badge, egui::FontId::proportional(11.5), Color32::WHITE);
                if auto {
                    resp.on_hover_text(tr("copilot.focus.tip"));
                }
                let st = env.ed.copilot.boxes.iter().find(|b| b.id == id).map(|b| b.state).unwrap_or(BoxState::Off);
                for (s, icon, key, name) in [
                    (BoxState::On, IconId::Play, "copilot.box.state.on", "on"),
                    (BoxState::Paused, IconId::Pause, "copilot.box.state.paused", "paused"),
                    (BoxState::Off, IconId::EyeOff, "copilot.box.state.off", "off"),
                ] {
                    if env.icon_button_sized(ui, icon, key, st == s, true, &format!("copilot:box:{id}:{name}"), 22.0, false).clicked() && st != s {
                        let c = &mut env.ed.copilot;
                        if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                            b.state = s;
                        }
                        match s {
                            BoxState::On => redo_box.push(id),
                            // desligada: sem fantasma e fora da fila
                            BoxState::Off => {
                                c.ghosts.retain(|g| g.box_id != id);
                                c.dirty.retain(|d| d.0 != id);
                            }
                            BoxState::Paused => c.dirty.retain(|d| d.0 != id),
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if env.icon_button_sized(ui, IconId::Trash, "copilot.box.delete", false, true, &format!("copilot:box:{id}:delete"), 22.0, false).clicked() {
                        remove = Some(id);
                    }
                    if auto && env.icon_button_sized(ui, IconId::Pin, "copilot.box.pin", false, true, &format!("copilot:box:{id}:pin"), 22.0, false).clicked() {
                        // fixar: a caixa fica aqui; o próximo traço fora das caixas cria um foco novo
                        if let Some(b) = env.ed.copilot.boxes.iter_mut().find(|b| b.id == id) {
                            b.auto = false;
                        }
                    }
                    if env.icon_button_sized(ui, IconId::Refresh, "copilot.box.again", false, true, &format!("copilot:box:{id}:again"), 22.0, false).clicked() {
                        let c = &mut env.ed.copilot;
                        if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                            b.seed = b.seed.wrapping_add(1);
                            b.state = BoxState::On;
                        }
                        redo_box.push(id);
                    }
                });
            });
            // tamanho da caixa → tamanho da geração, e o aviso quando a escala atrapalha
            if let Some((win, gsz)) = plan {
                let z = cp::gen_zoom(*win, *gsz);
                let txt = format!("{:.0} × {:.0} px  →  {} {} × {}  ({:.1}×)", rect_b[2], rect_b[3], tr("copilot.box.gen"), gsz[0], gsz[1], z);
                let (col, tip) = if z < 0.6 {
                    (t.st_retake, Some("copilot.zoom.down"))
                } else if z > 1.6 {
                    (t.st_review, Some("copilot.zoom.up"))
                } else {
                    (t.text_dim, None)
                };
                let r = ui.label(RichText::new(txt).size(11.0).color(col));
                r.on_hover_text(format!("{}{}", tr("copilot.box.gen.tip"), tip.map(|k| format!("\n\n{}", tr(k))).unwrap_or_default()));
            }
            let c = &mut env.ed.copilot;
            if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                let r = ui.add(egui::TextEdit::multiline(&mut b.prompt).desired_rows(2).desired_width(f32::INFINITY).hint_text(tr("copilot.box.hint")));
                notes.push((format!("copilot:box:{id}:prompt"), r.rect));
                if prompt_done(ui, &r, &mut b.prompt) {
                    redo_box.push(id);
                }
                if ui.checkbox(&mut b.use_global, RichText::new(tr("copilot.box.use_global")).size(11.5)).on_hover_text(tr("copilot.box.use_global.tip")).changed() {
                    redo_box.push(id);
                }
            }
        });
    }
    {
        let c = &mut env.ed.copilot;
        if let Some(id) = remove {
            c.boxes.retain(|b| b.id != id);
            c.ghosts.retain(|g| g.box_id != id);
            c.dirty.retain(|d| d.0 != id);
        }
        for id in redo_box {
            requeue(c, id);
        }
    }

    // --- aceitar
    section(ui, &t, "copilot.sec.accept");
    let has = env.ed.copilot.ghosts.iter().any(|g| Some(&g.s.cut) == cur_cut.as_ref());
    ui.horizontal(|ui| {
        for (icon, key, name, w) in [
            (IconId::Check, "copilot.accept_all", "copilot:accept:all", Some(AcceptWhich::All)),
            (IconId::ToolWand, "copilot.accept_improve", "copilot:accept:improve", Some(AcceptWhich::Improve)),
            (IconId::Plus, "copilot.accept_complete", "copilot:accept:complete", Some(AcceptWhich::Complete)),
            (IconId::Close, "copilot.discard", "copilot:discard", None),
        ] {
            if env.icon_button_sized(ui, icon, key, false, has, name, 26.0, false).clicked() {
                act = Some(w);
            }
        }
        ui.add_space(6.0);
        let c = &mut env.ed.copilot;
        let label = |x: cp::AcceptTarget| tr(if x == cp::AcceptTarget::Clean { "copilot.target.clean" } else { "copilot.target.same" }).to_string();
        let tr_rect = egui::ComboBox::from_id_salt("copilot_target").width(ui.available_width().max(90.0)).selected_text(label(c.target)).show_ui(ui, |ui| {
            for x in [cp::AcceptTarget::Same, cp::AcceptTarget::Clean] {
                let r = ui.selectable_value(&mut c.target, x, label(x));
                notes.push((format!("copilot:target:{}", if x == cp::AcceptTarget::Clean { "clean" } else { "same" }), r.rect));
            }
        })
        .response
        .on_hover_text(tr("copilot.target.tip"))
        .rect;
        notes.push(("copilot:target".into(), tr_rect));
    });
    {
        let c = &mut env.ed.copilot;
        ui.add_enabled_ui(c.target == cp::AcceptTarget::Same, |ui| {
            ui.checkbox(&mut c.replace_rough, RichText::new(tr("copilot.replace")).size(12.0)).on_hover_text(tr("copilot.replace.tip"));
        });
    }

    // --- ajustes finos (fechados por padrão)
    egui::CollapsingHeader::new(RichText::new(tr("copilot.sec.adjust")).size(12.0).color(t.text_dim)).id_salt("copilot_adjust").default_open(false).show(ui, |ui| {
        let c = &mut env.ed.copilot;
        egui::Grid::new("copilot_adjust_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new(tr("copilot.steps")).size(12.0)).on_hover_text(tr("copilot.steps.tip"));
            let auto = tr("copilot.steps.auto").to_string();
            again |= ui.add(egui::Slider::new(&mut c.steps, 0..=12).custom_formatter(move |n, _| if n < 0.5 { auto.clone() } else { format!("{n:.0}") })).changed();
            ui.end_row();
            ui.label(RichText::new(tr("copilot.res")).size(12.0)).on_hover_text(tr("copilot.res.tip"));
            let label = |r: u32| if r == 0 { tr("copilot.res.auto").to_string() } else { format!("{r} px") };
            egui::ComboBox::from_id_salt("copilot_res").width(120.0).selected_text(label(c.res)).show_ui(ui, |ui| {
                for r in [0, 384, 512, 640, 768] {
                    again |= ui.selectable_value(&mut c.res, r, label(r)).changed();
                }
            });
            ui.end_row();
            ui.label(RichText::new(tr("copilot.opacity")).size(12.0));
            let mut v = c.opacity * 100.0;
            if ui.add(egui::Slider::new(&mut v, 10.0..=60.0).suffix(" %").max_decimals(0)).changed() {
                c.opacity = v / 100.0;
            }
            ui.end_row();
            ui.label(RichText::new(tr("copilot.magnet")).size(12.0)).on_hover_text(tr("copilot.magnet.tip"));
            let mut v = c.magnet * 100.0;
            if ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %").max_decimals(0)).changed() {
                c.magnet = v / 100.0;
            }
            ui.end_row();
            ui.label(RichText::new(tr("copilot.pause")).size(12.0)).on_hover_text(tr("copilot.pause.tip"));
            ui.add(egui::Slider::new(&mut c.pause_s, 0.2..=2.0).suffix(" s"));
            ui.end_row();
            ui.label(RichText::new(tr("copilot.colors")).size(12.0)).on_hover_text(tr("copilot.colors.tip"));
            ui.horizontal(|ui| {
                ui.color_edit_button_srgba(&mut c.ghost_color).on_hover_text(tr("copilot.color.ghost"));
                ui.color_edit_button_srgba(&mut c.box_color).on_hover_text(tr("copilot.color.box"));
                if ui.small_button(tr("copilot.colors.reset")).clicked() {
                    c.ghost_color = GHOST_COLOR;
                    c.box_color = BOX_COLOR;
                }
            });
            ui.end_row();
        });
    });
    if again {
        requeue_cut(&mut env.ed.copilot, cur_cut.as_ref());
    }
    for (k, r) in notes {
        env.note(&k, r);
    }
    match act {
        Some(Some(w)) => {
            accept(env, w);
        }
        Some(None) => {
            discard(env);
        }
        None => {}
    }
}

/// Campo de prompt: quebra de linha não tem sentido num prompt (o texto quebra sozinho na tela), então Enter aplica
/// (sai do campo) e qualquer quebra vira espaço. `true` = terminou de editar (Enter ou clicou fora).
fn prompt_done(ui: &egui::Ui, r: &egui::Response, text: &mut String) -> bool {
    // o campo de várias linhas consome o Enter (vira uma quebra): a quebra é o sinal de Enter
    let mut enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if text.contains(['\r', '\n']) {
        *text = text.replace(['\r', '\n'], " ").trim().to_string();
        enter = true;
    }
    if enter {
        r.surrender_focus();
    }
    enter || r.lost_focus()
}

/// Título de seção sem a linha (para dividir a linha com botões).
fn section_inline(ui: &mut egui::Ui, t: &crate::theme::Tokens, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(t.text_dim).strong());
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
        "steps": c.steps, "res": c.res, "change": c.change,
        "service": cp::service_state(),
        "busy": c.busy.map(|b| b.0), "queue": c.dirty.iter().map(|d| d.0).collect::<Vec<_>>(),
        "boxes": c.boxes.iter().map(|b| serde_json::json!({"id": b.id, "rect": b.rect, "state": format!("{:?}", b.state), "prompt": b.prompt, "auto": b.auto, "seed": b.seed, "layer": b.layer, "use_global": b.use_global})).collect::<Vec<_>>(),
        "ghosts": c.ghosts.iter().map(|g| serde_json::json!({"box": g.box_id, "id": g.s.id, "stale": g.stale, "improve": pts(&g.s.improve), "complete": pts(&g.s.complete),
            "window": g.s.window, "gen_size": g.s.gen_size, "ms": g.s.ms})).collect::<Vec<_>>(),
        "target": c.target, "new_box_mode": c.new_box_mode,
        "last": c.last,
    })
}
