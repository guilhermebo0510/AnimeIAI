//! Entrada sintética da bridge (`INPUT`): mouse, roda, teclado, texto e caneta simulada (com pressão) injetados DENTRO do egui
//! pelo gancho `raw_input_hook`, sem tocar no cursor nem no teclado do sistema (os eventos nunca saem da janela do AnimeIAI).
//! Coordenadas em pontos lógicos da janela (as mesmas dos retângulos de `GET_UI_STATE`).
//!
//! Eventos (`events`, em ordem; cada um é um objeto com `kind`):
//! `mouse_move {x,y}`, `mouse_down|mouse_up {x,y,button?: primary|secondary|middle}`, `wheel {dx,dy}`,
//! `key {key, pressed?, modifiers?: [ctrl|shift|alt]}` (sem `pressed` = aperta e solta), `text {text}`,
//! `pen {x,y,pressure,phase: down|move|up, t_ms?}` e `wait {frames}`. `modifiers` também vale para mouse/wheel.
//! `t_ms` (opcional) = quando o evento da caneta entra, em ms desde o `down`: a caneta anda no ritmo real (o pincel vê a
//! velocidade do traço, como uma mesa digitalizadora que manda um ponto a cada 8 ms); sem `t_ms`, entra o quanto antes.
//!
//! Escalonamento: movimentos consecutivos (`mouse_move`, `pen move`) entram juntos no mesmo quadro (como um dispositivo de
//! alta taxa); qualquer outro evento ocupa um quadro e nunca divide o quadro com um movimento anterior — assim o egui vê o
//! ponteiro sobre o alvo um quadro ANTES do clique. A caneta vira `PointerMoved`/`PointerButton` (posição) mais
//! `Event::Touch` com `force` = pressão (o canvas lê a pressão dos eventos de toque).

use egui::{Event, Modifiers, PointerButton, Pos2, TouchPhase, Vec2};
use serde_json::{Map, Value};
use std::collections::VecDeque;

pub(crate) const MAX_EVENTS: usize = 2000;
const MAX_QUEUE: usize = 8000;
const MAX_MOVES_PER_FRAME: usize = 64;
const PEN_ID: u64 = 1;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Move(Pos2, Modifiers),
    Button { pos: Pos2, button: PointerButton, pressed: bool, mods: Modifiers },
    Wheel { delta: Vec2, mods: Modifiers },
    Key { key: egui::Key, pressed: bool, mods: Modifiers },
    Text(String),
    Pen { pos: Pos2, pressure: f32, phase: PenPhase, at: Option<f32> },
    Wait(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PenPhase {
    Down,
    Move,
    Up,
}

impl Step {
    fn is_move(&self) -> bool {
        matches!(self, Step::Move(..) | Step::Pen { phase: PenPhase::Move, .. })
    }
}

#[derive(Default)]
pub(crate) struct InputQueue {
    steps: VecDeque<Step>,
    wait: u32,
    /// Quadros em que algo foi injetado (diagnóstico).
    pub(crate) injected_frames: u64,
    /// Modificadores atuais do teclado injetado (para emitir `ModifiersChanged` só quando mudam).
    mods: Modifiers,
    /// Quando a caneta encostou (o `t_ms` dos eventos seguintes conta daqui).
    pen_t0: Option<std::time::Instant>,
    /// Atribuição da entrada deste quadro (R16 REQ-P03).
    attr: Attribution,
}

/// Quem emitiu os eventos que DISPARAM ação neste quadro (botão do ponteiro, roda, tecla, texto, toque que começa ou
/// termina). Movimento não dispara nada: clique de verdade com movimento injetado no mesmo quadro não é da bridge.
/// O gesto (botão apertado até soltar) junta as fontes de todos os quadros dele: apertar injetado e soltar de verdade
/// = `Mixed`.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Attribution {
    /// Eventos de ponteiro que disparam ação neste quadro (botão, roda, toque que começa/termina): (injetado, plataforma).
    pointer: (bool, bool),
    /// Teclado neste quadro (tecla, texto, colar/copiar/recortar): (injetado, plataforma).
    keys: (bool, bool),
    /// Enter/Espaço APERTADOS neste quadro (o que aciona um botão em foco no egui): (injetado, plataforma). Uma tecla
    /// alheia no mesmo quadro não entra (CX12 C12-01).
    activate: (bool, bool),
    /// Gesto de cada botão do ponteiro (primário, secundário, meio, extra 1 e 2), em andamento ou terminado neste quadro:
    /// (injetado, plataforma) dos eventos DESSE botão e se ele ainda está apertado. Só eventos de ponteiro entram: uma
    /// tecla no meio do gesto não muda a origem do clique (CX10 C10-01); e um botão não contamina a origem de outro
    /// (CX16 C16-10: o secundário apertado na plataforma não faz o clique primário injetado virar "misto").
    gestures: [(Option<(bool, bool)>, bool); 5],
}

fn button_index(b: egui::PointerButton) -> usize {
    match b {
        egui::PointerButton::Primary => 0,
        egui::PointerButton::Secondary => 1,
        egui::PointerButton::Middle => 2,
        egui::PointerButton::Extra1 => 3,
        egui::PointerButton::Extra2 => 4,
    }
}

/// Fonte de um gesto: os dois lados no mesmo gesto (apertar injetado, soltar de verdade) = `Mixed`.
fn gesture_source(g: (bool, bool)) -> aiai_core::InputSource {
    if g == (true, true) { aiai_core::InputSource::Mixed } else { one(g) }
}

fn is_pointer_trigger(e: &Event) -> bool {
    matches!(e, Event::PointerButton { .. } | Event::MouseWheel { .. } | Event::Touch { phase: TouchPhase::Start | TouchPhase::End | TouchPhase::Cancel, .. })
}

fn is_key_trigger(e: &Event) -> bool {
    matches!(e, Event::Key { .. } | Event::Text(_) | Event::Paste(_) | Event::Copy | Event::Cut)
}

fn is_activation(e: &Event) -> bool {
    matches!(e, Event::Key { key: egui::Key::Enter | egui::Key::Space, pressed: true, .. })
}

/// Fonte de um tipo de evento: só injetado = bridge; só plataforma = plataforma; os dois no mesmo quadro = não dá para
/// saber qual acionou (desconhecida, nunca chute).
fn one(flags: (bool, bool)) -> aiai_core::InputSource {
    use aiai_core::InputSource as S;
    match flags {
        (true, false) => S::BridgeInjected,
        (true, true) => S::UnknownLegacy,
        _ => S::UiPlatformUnattributed,
    }
}

impl Attribution {
    /// Abre um quadro novo: eventos da plataforma (os que sobraram depois do filtro) e os injetados.
    fn frame(&mut self, platform: &[Event], injected: &[Event]) {
        self.pointer = (injected.iter().any(is_pointer_trigger), platform.iter().any(is_pointer_trigger));
        self.keys = (injected.iter().any(is_key_trigger), platform.iter().any(is_key_trigger));
        self.activate = (injected.iter().any(is_activation), platform.iter().any(is_activation));
        for slot in self.gestures.iter_mut() {
            if !slot.1 {
                slot.0 = None;
            }
        }
        for (evs, inj) in [(platform, false), (injected, true)] {
            for e in evs {
                if let Event::PointerButton { button, pressed, .. } = e {
                    let slot = &mut self.gestures[button_index(*button)];
                    let g = slot.0.get_or_insert((false, false));
                    if inj { g.0 = true } else { g.1 = true }
                    slot.1 = *pressed;
                }
            }
        }
    }

    /// Fonte de uma ação disparada por um botão específico (o Aceitar usa o primário): a do gesto desse botão; sem
    /// gesto dele neste quadro, a dos eventos de ponteiro do quadro.
    pub(crate) fn button_source(&self, b: egui::PointerButton) -> aiai_core::InputSource {
        match self.gestures[button_index(b)].0 {
            Some(g) => gesture_source(g),
            None => one(self.pointer),
        }
    }

    /// Fonte de uma ação do ponteiro sem saber o botão: a dos gestos em andamento quando todos concordam; gestos de
    /// botões com fontes diferentes = não dá para saber qual acionou (desconhecida, sem afirmar mistura).
    pub(crate) fn pointer_source(&self) -> aiai_core::InputSource {
        let mut it = self.gestures.iter().filter_map(|s| s.0).map(gesture_source);
        match it.next() {
            None => one(self.pointer),
            Some(first) if it.all(|x| x == first) => first,
            Some(_) => aiai_core::InputSource::UnknownLegacy,
        }
    }

    /// Fonte de uma ação disparada pelo teclado.
    pub(crate) fn key_source(&self) -> aiai_core::InputSource {
        one(self.keys)
    }

    /// Fonte de um botão acionado por Enter/Espaço (só esses eventos contam; sem nenhum, desconhecida).
    pub(crate) fn activation_source(&self) -> aiai_core::InputSource {
        if self.activate == (false, false) { aiai_core::InputSource::UnknownLegacy } else { one(self.activate) }
    }

    /// Fonte das ações deste quadro quando não se sabe se vieram do ponteiro ou do teclado: a do único tipo de entrada
    /// que disparou algo; os dois com fontes diferentes = desconhecida (nunca "humano", nunca chute).
    pub(crate) fn source(&self) -> aiai_core::InputSource {
        let ptr = self.gestures.iter().any(|s| s.0.is_some()) || self.pointer.0 || self.pointer.1;
        let key = self.keys.0 || self.keys.1;
        match (ptr, key) {
            (true, false) => self.pointer_source(),
            (false, true) => self.key_source(),
            (false, false) => aiai_core::InputSource::UiPlatformUnattributed,
            (true, true) if self.pointer_source() == self.key_source() => self.key_source(),
            (true, true) => aiai_core::InputSource::UnknownLegacy,
        }
    }
}

impl InputQueue {
    pub(crate) fn pending(&self) -> usize {
        self.steps.len() + self.wait as usize
    }

    /// Fonte das ações deste quadro (vai para `CoreSession::source` antes da interface rodar).
    pub(crate) fn frame_source(&self) -> aiai_core::InputSource {
        self.attr.source()
    }

    /// Fonte de um clique do botão primário neste quadro (o Aceitar da barra usa esta; CX16 C16-10).
    pub(crate) fn primary_source(&self) -> aiai_core::InputSource {
        self.attr.button_source(egui::PointerButton::Primary)
    }

    /// Fonte de um botão acionado por Enter/Espaço neste quadro.
    pub(crate) fn activation_source(&self) -> aiai_core::InputSource {
        self.attr.activation_source()
    }

    /// Enfileira passos; recusa se a fila ficaria grande demais.
    pub(crate) fn push(&mut self, steps: Vec<Step>) -> Result<(), String> {
        if self.steps.len() + steps.len() > MAX_QUEUE {
            return Err(format!("fila de entrada cheia ({} pendentes); espere terminar (GET_UI_STATE.input_pending)", self.steps.len()));
        }
        self.steps.extend(steps);
        Ok(())
    }

    /// Eventos do próximo quadro (chamado em `raw_input_hook`).
    pub(crate) fn next_events(&mut self, raw: &mut egui::RawInput) {
        // com entrada injetada em andamento (fila ou caneta encostada), o mouse de verdade não entra no canvas: senão
        // quem mexe no mouse do sistema ao mesmo tempo (outro programa na frente) puxa o traço injetado
        if self.pen_t0.is_some() || !self.steps.is_empty() {
            raw.events.retain(|e| !matches!(e, Event::PointerMoved(_) | Event::MouseMoved(_) | Event::PointerButton { .. } | Event::PointerGone | Event::Touch { .. }));
        }
        if self.wait > 0 {
            self.wait -= 1;
            self.attr.frame(&raw.events, &[]);
            return;
        }
        let before = raw.events.len();
        let mut moves = 0;
        while let Some(front) = self.steps.front() {
            // caneta com ritmo: o evento espera a hora dele
            if let (Step::Pen { at: Some(ms), .. }, Some(t0)) = (front, self.pen_t0) {
                if t0.elapsed().as_secs_f32() * 1000.0 < *ms {
                    break;
                }
            }
            if front.is_move() {
                if moves >= MAX_MOVES_PER_FRAME {
                    break;
                }
                let step = self.steps.pop_front().expect("há passo");
                emit(raw, step);
                moves += 1;
                continue;
            }
            if moves > 0 {
                break; // o clique/tecla espera o quadro seguinte
            }
            let step = self.steps.pop_front().expect("há passo");
            match step {
                Step::Pen { phase: PenPhase::Down, .. } => self.pen_t0 = Some(std::time::Instant::now()),
                Step::Pen { phase: PenPhase::Up, .. } => self.pen_t0 = None,
                _ => {}
            }
            if let Step::Wait(n) = step {
                self.wait = n.saturating_sub(1);
            } else {
                emit(raw, step);
            }
            break;
        }
        self.attr.frame(&raw.events[..before], &raw.events[before..]);
        if raw.events.len() > before {
            self.injected_frames += 1;
            // o estado de modificadores acompanha o que os eventos injetados declaram (o egui 0.36 lê `ModifiersChanged`)
            let fresh: Vec<Event> = raw.events.drain(before..).collect();
            for e in fresh {
                let em = match &e {
                    Event::PointerButton { modifiers, .. } | Event::MouseWheel { modifiers, .. } | Event::Key { modifiers, .. } => Some(*modifiers),
                    _ => None,
                };
                if let Some(em) = em {
                    if em != self.mods {
                        raw.events.push(Event::ModifiersChanged(em));
                        self.mods = em;
                    }
                }
                raw.events.push(e);
            }
        }
    }
}

fn emit(raw: &mut egui::RawInput, step: Step) {
    match step {
        Step::Move(p, _) => {
            raw.events.push(Event::PointerMoved(p));
        }
        Step::Button { pos, button, pressed, mods } => {
            raw.events.push(Event::PointerMoved(pos));
            raw.events.push(Event::PointerButton { pos, button, pressed, modifiers: mods });
        }
        Step::Wheel { delta, mods } => {
            raw.events.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase: TouchPhase::Move, modifiers: mods });
        }
        Step::Key { key, pressed, mods } => {
            raw.events.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: mods });
        }
        Step::Text(t) => raw.events.push(Event::Text(t)),
        Step::Pen { pos, pressure, phase, .. } => {
            let touch = |phase| Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(PEN_ID), phase, pos, force: Some(pressure) };
            match phase {
                PenPhase::Down => {
                    raw.events.push(Event::PointerMoved(pos));
                    raw.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
                    raw.events.push(touch(TouchPhase::Start));
                }
                PenPhase::Move => {
                    raw.events.push(Event::PointerMoved(pos));
                    raw.events.push(touch(TouchPhase::Move));
                }
                PenPhase::Up => {
                    raw.events.push(touch(TouchPhase::End));
                    raw.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
                }
            }
        }
        Step::Wait(_) => {}
    }
}

fn num(m: &Map<String, Value>, k: &str, i: usize) -> Result<f32, String> {
    m.get(k)
        .and_then(Value::as_f64)
        .filter(|f| f.is_finite() && f.abs() < 100_000.0)
        .map(|f| f as f32)
        .ok_or_else(|| format!("events[{i}]: '{k}' é obrigatório e deve ser um número"))
}

fn pos(m: &Map<String, Value>, i: usize) -> Result<Pos2, String> {
    Ok(Pos2::new(num(m, "x", i)?, num(m, "y", i)?))
}

fn button(m: &Map<String, Value>, i: usize) -> Result<PointerButton, String> {
    match m.get("button").and_then(Value::as_str).unwrap_or("primary") {
        "primary" | "left" => Ok(PointerButton::Primary),
        "secondary" | "right" => Ok(PointerButton::Secondary),
        "middle" => Ok(PointerButton::Middle),
        other => Err(format!("events[{i}]: button '{other}' inválido (primary|secondary|middle)")),
    }
}

fn mods(m: &Map<String, Value>, i: usize) -> Result<Modifiers, String> {
    let mut out = Modifiers::NONE;
    match m.get("modifiers") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for x in list {
                match x.as_str().map(str::to_ascii_lowercase).as_deref() {
                    Some("ctrl") | Some("control") | Some("command") => {
                        out.ctrl = true;
                        out.command = true;
                    }
                    Some("shift") => out.shift = true,
                    Some("alt") => out.alt = true,
                    _ => return Err(format!("events[{i}]: modifiers aceita ctrl|shift|alt")),
                }
            }
        }
        Some(_) => return Err(format!("events[{i}]: 'modifiers' deve ser uma lista")),
    }
    Ok(out)
}

fn key_from(name: &str) -> Option<egui::Key> {
    if let Some(k) = egui::Key::from_name(name) {
        return Some(k);
    }
    let mut c = name.chars();
    let first = c.next()?;
    let cap: String = first.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect();
    egui::Key::from_name(&cap)
}

/// Converte a lista `events` do pedido em passos.
pub(crate) fn parse(events: &[Value]) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        let m = ev.as_object().ok_or_else(|| format!("events[{i}] deve ser um objeto"))?;
        let kind = m.get("kind").and_then(Value::as_str).ok_or_else(|| format!("events[{i}].kind é obrigatório"))?;
        match kind {
            "mouse_move" => out.push(Step::Move(pos(m, i)?, mods(m, i)?)),
            "mouse_down" | "mouse_up" => out.push(Step::Button { pos: pos(m, i)?, button: button(m, i)?, pressed: kind == "mouse_down", mods: mods(m, i)? }),
            "wheel" => out.push(Step::Wheel { delta: Vec2::new(num(m, "dx", i).unwrap_or(0.0), num(m, "dy", i)?), mods: mods(m, i)? }),
            "key" => {
                let name = m.get("key").and_then(Value::as_str).ok_or_else(|| format!("events[{i}]: 'key' é obrigatório (texto)"))?;
                let key = key_from(name).ok_or_else(|| format!("events[{i}]: tecla desconhecida: {name}"))?;
                let mods = mods(m, i)?;
                match m.get("pressed").and_then(Value::as_bool) {
                    Some(p) => out.push(Step::Key { key, pressed: p, mods }),
                    None => {
                        out.push(Step::Key { key, pressed: true, mods });
                        out.push(Step::Key { key, pressed: false, mods });
                    }
                }
            }
            "text" => {
                let t = m.get("text").and_then(Value::as_str).filter(|t| !t.is_empty() && t.chars().count() <= 1000);
                out.push(Step::Text(t.ok_or_else(|| format!("events[{i}]: 'text' deve ter de 1 a 1000 caracteres"))?.to_string()));
            }
            "pen" => {
                let pressure = num(m, "pressure", i)?;
                if !(0.0..=1.0).contains(&pressure) {
                    return Err(format!("events[{i}]: pressure fora de 0..1"));
                }
                let phase = match m.get("phase").and_then(Value::as_str) {
                    Some("down") => PenPhase::Down,
                    Some("move") => PenPhase::Move,
                    Some("up") => PenPhase::Up,
                    _ => return Err(format!("events[{i}]: phase deve ser down|move|up")),
                };
                let at = match m.get("t_ms") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_f64().filter(|t| t.is_finite() && (0.0..=600_000.0).contains(t)).ok_or_else(|| format!("events[{i}]: t_ms deve ser 0..600000"))? as f32),
                };
                out.push(Step::Pen { pos: pos(m, i)?, pressure, phase, at });
            }
            "wait" => {
                let n = m.get("frames").and_then(Value::as_u64).filter(|n| *n <= 600).ok_or_else(|| format!("events[{i}]: wait.frames deve ser 0..600"))?;
                out.push(Step::Wait(n as u32));
            }
            other => return Err(format!("events[{i}]: kind '{other}' desconhecido (mouse_move, mouse_down, mouse_up, wheel, key, text, pen, wait)")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Roda a fila quadro a quadro; devolve os eventos de cada quadro.
    fn frames(events: Value) -> Vec<Vec<Event>> {
        let mut q = InputQueue::default();
        q.push(parse(events.as_array().unwrap()).unwrap()).unwrap();
        let mut all = Vec::new();
        for _ in 0..500 {
            let mut raw = egui::RawInput::default();
            q.next_events(&mut raw);
            all.push(raw.events);
            if q.pending() == 0 {
                break;
            }
        }
        all.retain(|f| !f.is_empty());
        all
    }

    /// Fonte de cada quadro (R16 AT-15): `real` = eventos da plataforma que entram antes da fila em cada quadro.
    fn sources(events: Value, real: &[(usize, Event)]) -> Vec<aiai_core::InputSource> {
        let mut q = InputQueue::default();
        q.push(parse(events.as_array().unwrap()).unwrap()).unwrap();
        let mut out = Vec::new();
        for k in 0..8 {
            let mut raw = egui::RawInput::default();
            raw.events.extend(real.iter().filter(|(f, _)| *f == k).map(|(_, e)| e.clone()));
            q.next_events(&mut raw);
            out.push(q.frame_source());
        }
        out
    }

    #[test]
    fn frame_source_links_actions_to_the_events_that_fired_them() {
        use aiai_core::InputSource as S;
        let click = json!([{"kind":"mouse_move","x":10,"y":20},{"kind":"mouse_down","x":10,"y":20},{"kind":"mouse_up","x":10,"y":20}]);
        // clique injetado: o quadro do soltar (onde o botão dispara) é da bridge
        let s = sources(click.clone(), &[]);
        assert_eq!((s[1], s[2]), (S::BridgeInjected, S::BridgeInjected), "{s:?}");
        // sem injeção: plataforma, nunca "humano"
        let press = |pressed| Event::PointerButton { pos: Pos2::new(5.0, 5.0), button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let s = sources(json!([{"kind":"wait","frames":1}]), &[(2, press(true)), (3, press(false))]);
        assert_eq!((s[2], s[3]), (S::UiPlatformUnattributed, S::UiPlatformUnattributed), "{s:?}");
        // tecla de verdade com movimento injetado no mesmo quadro: não é da bridge
        let key = Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let s = sources(json!([{"kind":"mouse_move","x":1,"y":1}]), &[(0, key)]);
        assert_eq!(s[0], S::UiPlatformUnattributed, "{s:?}");
        // CX10 C10-01: clique injetado com uma tecla de verdade no quadro do soltar: o clique continua da bridge
        let key2 = Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let mut q = InputQueue::default();
        q.push(parse(click.as_array().unwrap()).unwrap()).unwrap();
        let mut last = None;
        for k in 0..3 {
            let mut raw = egui::RawInput::default();
            if k == 2 {
                raw.events.push(key2.clone());
            }
            q.next_events(&mut raw);
            last = Some((q.attr.pointer_source(), q.attr.key_source(), q.frame_source()));
        }
        assert_eq!(last, Some((S::BridgeInjected, S::UiPlatformUnattributed, S::UnknownLegacy)), "clique da bridge; tecla da plataforma; o quadro sozinho não diz");
        // CX12 C12-01: Enter injetado (aciona o botão em foco) com a soltura de outra tecla da plataforma no mesmo quadro
        let release_a = Event::Key { key: egui::Key::A, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        let mut q = InputQueue::default();
        q.push(parse(json!([{"kind":"key","key":"Enter","pressed":true}]).as_array().unwrap()).unwrap()).unwrap();
        let mut raw = egui::RawInput::default();
        raw.events.push(release_a);
        q.next_events(&mut raw);
        assert_eq!(q.activation_source(), S::BridgeInjected, "só o Enter acionou");
        // Enter da plataforma e Enter injetado no mesmo quadro: não dá para saber qual acionou
        let enter = Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let mut q = InputQueue::default();
        q.push(parse(json!([{"kind":"key","key":"Enter","pressed":true}]).as_array().unwrap()).unwrap()).unwrap();
        let mut raw = egui::RawInput::default();
        raw.events.push(enter);
        q.next_events(&mut raw);
        assert_eq!(q.activation_source(), S::UnknownLegacy);
        // gesto que começa injetado e termina na plataforma: misto
        let s = sources(json!([{"kind":"mouse_move","x":10,"y":20},{"kind":"mouse_down","x":10,"y":20}]), &[(3, press(false))]);
        assert_eq!(s[1], S::BridgeInjected, "{s:?}");
        assert_eq!(s[3], S::Mixed, "{s:?}");
    }

    #[test]
    fn each_mouse_button_keeps_its_own_source() {
        // CX16 C16-10
        use aiai_core::InputSource as S;
        use egui::PointerButton::{Primary, Secondary};
        let btn = |button, pressed| Event::PointerButton { pos: Pos2::new(1.0, 1.0), button, pressed, modifiers: Modifiers::NONE };
        // a plataforma aperta o secundário (fica apertado); no quadro seguinte a bridge clica o primário
        let mut a = Attribution::default();
        a.frame(&[btn(Secondary, true)], &[]);
        a.frame(&[], &[btn(Primary, true), btn(Primary, false)]);
        assert_eq!(a.button_source(Primary), S::BridgeInjected, "o secundário da plataforma não contamina o clique primário");
        assert_eq!(a.pointer_source(), S::UnknownLegacy, "sem saber o botão, gestos de fontes diferentes = desconhecida");
        // soltura de outro botão (plataforma) no meio de um gesto primário injetado
        let mut a = Attribution::default();
        a.frame(&[], &[btn(Primary, true)]);
        a.frame(&[btn(Secondary, false)], &[]);
        a.frame(&[], &[btn(Primary, false)]);
        assert_eq!(a.button_source(Primary), S::BridgeInjected);
        // o mesmo botão apertado pela bridge e solto na plataforma: misto de verdade
        let mut a = Attribution::default();
        a.frame(&[], &[btn(Primary, true)]);
        a.frame(&[btn(Primary, false)], &[]);
        assert_eq!(a.button_source(Primary), S::Mixed);
        assert_eq!(a.pointer_source(), S::Mixed);
    }

    #[test]
    fn click_waits_one_frame_after_the_move_and_press_release_are_separate() {
        let f = frames(json!([{"kind":"mouse_move","x":10,"y":20},{"kind":"mouse_down","x":10,"y":20},{"kind":"mouse_up","x":10,"y":20}]));
        assert_eq!(f.len(), 3, "move | down | up em quadros distintos: {f:?}");
        assert!(matches!(f[0][0], Event::PointerMoved(p) if p == Pos2::new(10.0, 20.0)));
        assert!(f[1].iter().any(|e| matches!(e, Event::PointerButton { pressed: true, button: PointerButton::Primary, .. })));
        assert!(f[2].iter().any(|e| matches!(e, Event::PointerButton { pressed: false, .. })));
    }

    #[test]
    fn consecutive_moves_share_a_frame_and_pen_carries_pressure() {
        let f = frames(json!([
            {"kind":"pen","x":1,"y":1,"pressure":0.2,"phase":"down"},
            {"kind":"pen","x":2,"y":2,"pressure":0.5,"phase":"move"},
            {"kind":"pen","x":3,"y":3,"pressure":0.9,"phase":"move"},
            {"kind":"pen","x":3,"y":3,"pressure":0.0,"phase":"up"}]));
        assert_eq!(f.len(), 3, "down | 2 moves juntos | up: {f:?}");
        let forces: Vec<f32> = f[1].iter().filter_map(|e| if let Event::Touch { force, .. } = e { *force } else { None }).collect();
        assert_eq!(forces, [0.5, 0.9]);
        assert!(f[0].iter().any(|e| matches!(e, Event::Touch { phase: TouchPhase::Start, force: Some(p), .. } if (*p - 0.2).abs() < 1e-6)));
        assert!(f[2].iter().any(|e| matches!(e, Event::Touch { phase: TouchPhase::End, .. })));
    }

    #[test]
    fn paced_pen_waits_for_its_time() {
        let mut q = InputQueue::default();
        q.push(parse(json!([
            {"kind":"pen","x":1,"y":1,"pressure":1.0,"phase":"down"},
            {"kind":"pen","x":2,"y":1,"pressure":1.0,"phase":"move","t_ms":0},
            {"kind":"pen","x":3,"y":1,"pressure":1.0,"phase":"move","t_ms":40},
            {"kind":"pen","x":3,"y":1,"pressure":1.0,"phase":"up","t_ms":48}]).as_array().unwrap()).unwrap()).unwrap();
        let t0 = std::time::Instant::now();
        let mut moved_at = Vec::new();
        while q.pending() > 0 && t0.elapsed().as_secs_f32() < 2.0 {
            let mut raw = egui::RawInput::default();
            q.next_events(&mut raw);
            for e in &raw.events {
                if let Event::Touch { phase: TouchPhase::Move, pos, .. } = e {
                    moved_at.push((pos.x, t0.elapsed().as_secs_f32()));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(moved_at.len(), 2, "{moved_at:?}");
        assert!(moved_at[1].1 - moved_at[0].1 >= 0.035, "o segundo movimento esperou ~40 ms: {moved_at:?}");
        assert!(parse(json!([{"kind":"pen","x":1,"y":1,"pressure":1.0,"phase":"move","t_ms":-5}]).as_array().unwrap()).is_err());
    }

    #[test]
    fn the_real_mouse_does_not_pull_an_injected_stroke() {
        let mut q = InputQueue::default();
        q.push(parse(json!([
            {"kind":"pen","x":1,"y":1,"pressure":1.0,"phase":"down"},
            {"kind":"pen","x":2,"y":1,"pressure":1.0,"phase":"move","t_ms":30}]).as_array().unwrap()).unwrap()).unwrap();
        let real = || Event::PointerMoved(Pos2::new(500.0, 500.0));
        let mut seen_real = false;
        for _ in 0..200 {
            let mut raw = egui::RawInput::default();
            raw.events.push(real());
            q.next_events(&mut raw);
            seen_real |= raw.events.iter().any(|e| matches!(e, Event::PointerMoved(p) if p.x == 500.0));
            if q.pending() == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!seen_real, "o mouse do sistema ficou fora enquanto a caneta injetada está encostada");
        // sem injeção, o mouse de verdade passa
        let mut raw = egui::RawInput::default();
        raw.events.push(real());
        let mut idle = InputQueue::default();
        idle.next_events(&mut raw);
        assert_eq!(raw.events.len(), 1);
    }

    #[test]
    fn keys_text_wheel_wait_and_modifiers() {
        let f = frames(json!([
            {"kind":"key","key":"z","modifiers":["ctrl"]},
            {"kind":"wait","frames":3},
            {"kind":"text","text":"olá"},
            {"kind":"wheel","dx":0,"dy":-40}]));
        let n: usize = f.iter().flatten().filter(|e| !matches!(e, Event::ModifiersChanged(_))).count();
        assert_eq!(n, 4, "press, release, texto, roda: {f:?}");
        assert!(matches!(&f[0][0], Event::ModifiersChanged(m) if m.ctrl && m.command), "o estado dos modificadores muda antes da tecla: {f:?}");
        assert!(matches!(&f[0][1], Event::Key { key: egui::Key::Z, pressed: true, modifiers, .. } if modifiers.ctrl && modifiers.command));
        assert!(f.iter().flatten().any(|e| matches!(e, Event::ModifiersChanged(m) if *m == egui::Modifiers::NONE)), "e volta a nenhum depois");
        assert!(f.iter().flatten().any(|e| matches!(e, Event::Text(t) if t == "olá")));
        assert!(f.iter().flatten().any(|e| matches!(e, Event::MouseWheel { delta, .. } if *delta == Vec2::new(0.0, -40.0))));
    }

    #[test]
    fn invalid_events_are_rejected_with_the_index() {
        for bad in [
            json!([{"kind":"warp"}]),
            json!([{"kind":"mouse_move","x":1}]),
            json!([{"kind":"mouse_down","x":1,"y":1,"button":"x"}]),
            json!([{"kind":"key","key":"NopeKey"}]),
            json!([{"kind":"key","key":"a","modifiers":["super"]}]),
            json!([{"kind":"pen","x":1,"y":1,"pressure":2.0,"phase":"down"}]),
            json!([{"kind":"pen","x":1,"y":1,"pressure":0.5,"phase":"hover"}]),
            json!([{"kind":"wait","frames":100000}]),
            json!([{"kind":"text","text":""}]),
            json!(["x"]),
        ] {
            let e = parse(bad.as_array().unwrap()).unwrap_err();
            assert!(e.contains("events[0]"), "{bad}: {e}");
        }
    }

    #[test]
    fn queue_is_bounded() {
        let mut q = InputQueue::default();
        let many = vec![Step::Wait(1); MAX_QUEUE];
        q.push(many).unwrap();
        assert!(q.push(vec![Step::Wait(1)]).unwrap_err().contains("cheia"));
    }
}
