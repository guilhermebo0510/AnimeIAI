//! Entrada sintética da bridge (`INPUT`): mouse, roda, teclado, texto e caneta simulada (com pressão) injetados DENTRO do egui
//! pelo gancho `raw_input_hook`, sem tocar no cursor nem no teclado do sistema (os eventos nunca saem da janela do AnimeIAI).
//! Coordenadas em pontos lógicos da janela (as mesmas dos retângulos de `GET_UI_STATE`).
//!
//! Eventos (`events`, em ordem; cada um é um objeto com `kind`):
//! `mouse_move {x,y}`, `mouse_down|mouse_up {x,y,button?: primary|secondary|middle}`, `wheel {dx,dy}`,
//! `key {key, pressed?, modifiers?: [ctrl|shift|alt]}` (sem `pressed` = aperta e solta), `text {text}`,
//! `pen {x,y,pressure,phase: down|move|up}` e `wait {frames}`. `modifiers` também vale para mouse/wheel.
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
    Pen { pos: Pos2, pressure: f32, phase: PenPhase },
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
}

impl InputQueue {
    pub(crate) fn pending(&self) -> usize {
        self.steps.len() + self.wait as usize
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
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let before = raw.events.len();
        let mut moves = 0;
        while let Some(front) = self.steps.front() {
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
            if let Step::Wait(n) = step {
                self.wait = n.saturating_sub(1);
            } else {
                emit(raw, step);
            }
            break;
        }
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
        Step::Pen { pos, pressure, phase } => {
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
                out.push(Step::Pen { pos: pos(m, i)?, pressure, phase });
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
