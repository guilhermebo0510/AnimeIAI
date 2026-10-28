//! Caneta nativa do Windows (auditoria 4.1c–f). O winit já entrega cada amostra do histórico do ponteiro (as que o
//! Windows junta entre dois quadros, a mais de 60 Hz) como um toque, mas com a pressão do instante atual repetida em
//! todas, e sem inclinação nem botões da caneta. Aqui a janela principal ganha uma subclasse que olha as mensagens
//! `WM_POINTER*` da caneta ANTES do winit, lê `GetPointerPenInfoHistory` (pressão, inclinação X/Y, borracha, botão
//! lateral de cada amostra, na mesma ordem e posição das que o winit vai mandar) e passa a mensagem adiante sem mexer
//! nela. O canvas casa cada `PointerMoved` com a amostra nativa da mesma posição.
//!
//! Sem caneta (mouse, toque, a fila da bridge), a fila fica vazia e nada muda. WinTab (driver próprio de algumas
//! mesas, para quem desliga o Windows Ink) não entra: as mesas atuais entregam tudo pelo Windows Ink.

/// Uma amostra da caneta, em px físicos da área cliente da janela principal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenSample {
    pub x: f32,
    pub y: f32,
    /// 0..1 (o Windows entrega 0..1024).
    pub pressure: f32,
    /// Inclinação em graus (-90..90), X para a direita e Y para baixo.
    pub tilt: [f32; 2],
    /// Ponta de borracha (caneta virada) ou botão de borracha apertado.
    pub eraser: bool,
    /// Botão lateral (barril) apertado.
    pub barrel: bool,
    pub down: bool,
    pub up: bool,
    pub in_contact: bool,
}

use std::collections::VecDeque;
use std::sync::Mutex;

static QUEUE: Mutex<VecDeque<PenSample>> = Mutex::new(VecDeque::new());
/// Mais que isso sem ninguém ler (canvas fechado, caneta sobre os painéis): as mais velhas saem.
const MAX_QUEUE: usize = 4096;

fn push(s: PenSample) {
    if let Ok(mut q) = QUEUE.lock() {
        if q.len() >= MAX_QUEUE {
            q.pop_front();
        }
        q.push_back(s);
    }
}

/// Tira da fila todas as amostras que chegaram (as mais velhas primeiro).
pub fn drain() -> Vec<PenSample> {
    QUEUE.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}

/// Para os testes: põe uma amostra na fila como se tivesse vindo da janela.
pub fn inject_for_test(s: PenSample) {
    push(s);
}

/// Casa a posição de um `PointerMoved` (px físicos da área cliente) com a amostra nativa da mesma posição: devolve a
/// amostra e quantas da frente da fila (mais velhas) ficaram para trás. Tolerância de 0,75 px (o winit arredonda a
/// parte inteira e soma a fração da mesma conta).
pub fn match_at(samples: &[PenSample], x: f32, y: f32) -> Option<usize> {
    samples.iter().position(|s| (s.x - x).abs() <= 0.75 && (s.y - y).abs() <= 0.75)
}

#[cfg(windows)]
mod sys {
    use super::{PenSample, push};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::Input::Pointer::{
        GetPointerDeviceRects, GetPointerPenInfo, GetPointerPenInfoHistory, GetPointerType, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT, POINTER_FLAG_UP,
        POINTER_PEN_INFO,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{PEN_FLAG_BARREL, PEN_FLAG_ERASER, PEN_FLAG_INVERTED, PT_PEN, WM_POINTERDOWN, WM_POINTERUP, WM_POINTERUPDATE};

    const SUBCLASS_ID: usize = 0x41_49_41_49; // "AIAI"

    pub fn install(hwnd: isize) {
        unsafe {
            SetWindowSubclass(hwnd as HWND, Some(proc_), SUBCLASS_ID, 0);
        }
    }

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
        if msg == WM_POINTERDOWN || msg == WM_POINTERUPDATE || msg == WM_POINTERUP {
            let id = (wparam & 0xffff) as u32;
            unsafe { read_pen(hwnd, id) };
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    unsafe fn read_pen(hwnd: HWND, id: u32) {
        let mut kind = 0;
        if unsafe { GetPointerType(id, &mut kind) } == 0 || kind != PT_PEN {
            return;
        }
        let mut cur: POINTER_PEN_INFO = Default::default();
        if unsafe { GetPointerPenInfo(id, &mut cur) } == 0 {
            return;
        }
        let mut n = cur.pointerInfo.historyCount.max(1);
        let mut hist: Vec<POINTER_PEN_INFO> = vec![Default::default(); n as usize];
        if unsafe { GetPointerPenInfoHistory(id, &mut n, hist.as_mut_ptr()) } == 0 {
            hist = vec![cur];
        } else {
            hist.truncate(n as usize);
        }
        let mut origin = POINT { x: 0, y: 0 };
        unsafe { ClientToScreen(hwnd, &mut origin) };
        // o histórico vem do mais novo para o mais velho; a fila vai do mais velho para o mais novo (como o winit)
        for pi in hist.iter().rev() {
            let info = &pi.pointerInfo;
            let (mut dev, mut disp) = (RECT { left: 0, top: 0, right: 0, bottom: 0 }, RECT { left: 0, top: 0, right: 0, bottom: 0 });
            // mesma conta do winit (himétrico -> px da tela, com fração), para as posições casarem
            let (sx, sy) = if unsafe { GetPointerDeviceRects(info.sourceDevice, &mut dev, &mut disp) } != 0 && dev.right > dev.left && dev.bottom > dev.top {
                let rx = (disp.right - disp.left) as f64 / (dev.right - dev.left) as f64;
                let ry = (disp.bottom - disp.top) as f64 / (dev.bottom - dev.top) as f64;
                (disp.left as f64 + info.ptHimetricLocation.x as f64 * rx, disp.top as f64 + info.ptHimetricLocation.y as f64 * ry)
            } else {
                (info.ptPixelLocation.x as f64, info.ptPixelLocation.y as f64)
            };
            push(PenSample {
                x: (sx - origin.x as f64) as f32,
                y: (sy - origin.y as f64) as f32,
                pressure: (pi.pressure as f32 / 1024.0).clamp(0.0, 1.0),
                tilt: [pi.tiltX as f32, pi.tiltY as f32],
                eraser: pi.penFlags & (PEN_FLAG_ERASER | PEN_FLAG_INVERTED) != 0,
                barrel: pi.penFlags & PEN_FLAG_BARREL != 0,
                down: info.pointerFlags & POINTER_FLAG_DOWN != 0,
                up: info.pointerFlags & POINTER_FLAG_UP != 0,
                in_contact: info.pointerFlags & POINTER_FLAG_INCONTACT != 0,
            });
        }
    }
}

/// Liga a leitura nativa da caneta na janela principal (Windows; nos outros sistemas não faz nada).
pub fn install(window: &impl raw_window_handle::HasWindowHandle) {
    #[cfg(windows)]
    if let Ok(h) = window.window_handle() {
        if let raw_window_handle::RawWindowHandle::Win32(w) = h.as_raw() {
            sys::install(w.hwnd.get());
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}
