//! Janelas soltas do dock e painel maximizado, a parte sem egui: o que vai para o `layouts.json` (`SavedFloat`), onde
//! uma janela guardada reaparece (`place`: o monitor de antes se ele ainda existe, senão trazida para a área visível
//! de outro), a lista de monitores do sistema e a máquina de estados do maximizar (`Maximized`).
//!
//! Decisões:
//! - Janela solta (Janela > Destacar painel) faz parte do layout do workspace: posição, tamanho e monitor são gravados
//!   e voltam ao abrir o app. Cada janela tem a sua árvore do dock (um painel, um grupo de abas, ou o que o usuário
//!   arrumar dentro dela). Fechar a janela devolve os painéis ao lugar de onde saíram (`FloatHome`).
//! - Maximizar NÃO é gravado: é um estado de visão passageiro (como o ` do Premiere/After Effects). A árvore do layout
//!   não muda ao maximizar, então o layout gravado é sempre o normal; reabrir o app mostra o layout normal.
//!
//! Coordenadas das janelas: pixels físicos da área de trabalho virtual do sistema (os do Win32 e do winit; o app é
//! DPI-aware por monitor). Posição = canto externo (com a moldura do sistema); tamanho = área cliente.

use crate::layout::{PanelKind, SavedNode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Retângulo inteiro em pixels físicos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// Área da interseção (0 se não se tocam).
    pub fn overlap(&self, o: &IRect) -> i64 {
        let w = (self.right().min(o.right()) - self.x.max(o.x)).max(0) as i64;
        let h = (self.bottom().min(o.bottom()) - self.y.max(o.y)).max(0) as i64;
        w * h
    }
}

/// Monitor em que a janela estava: nome do dispositivo (`\\.\DISPLAY2`) e a área de trabalho dele (sem a barra de
/// tarefas) naquele momento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedMonitor {
    pub name: String,
    pub work: IRect,
}

/// Onde a janela estava no layout principal, para voltar ao fechar: aba junto de `anchor`, ou numa linha/coluna ao
/// lado do bloco vizinho (`block`: os painéis dele; `after` = depois dele), com a fração `frac` do espaço. O bloco é
/// achado de novo como o menor pedaço da árvore que tem todos os painéis dele ainda presentes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FloatHome {
    Tab {
        anchor: String,
        /// Posição entre as abas.
        #[serde(default)]
        index: usize,
    },
    Row { block: Vec<String>, after: bool, frac: f32 },
    Col { block: Vec<String>, after: bool, frac: f32 },
}

/// Uma janela solta guardada no layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedFloat {
    /// A árvore do dock dentro da janela.
    pub tree: SavedNode,
    /// Canto externo, pixels físicos (`None` = centralizada no monitor).
    #[serde(default)]
    pub pos: Option<[i32; 2]>,
    /// Área cliente, pixels físicos.
    pub size: [i32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<SavedMonitor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<FloatHome>,
}

impl SavedFloat {
    /// Os painéis da janela (pela chave guardada; chaves desconhecidas ficam de fora).
    pub fn panels(&self) -> Vec<PanelKind> {
        fn walk(n: &SavedNode, out: &mut Vec<PanelKind>) {
            match n {
                SavedNode::Pane { panel, .. } => out.extend(PanelKind::from_key(panel)),
                SavedNode::Tabs { children, .. } | SavedNode::Grid { children, .. } => children.iter().for_each(|c| walk(c, out)),
                SavedNode::Row { children, .. } | SavedNode::Col { children, .. } => children.iter().for_each(|c| walk(&c.node, out)),
            }
        }
        let mut out = Vec::new();
        walk(&self.tree, &mut out);
        out
    }
}

/// Um monitor do sistema agora.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub rect: IRect,
    /// Área de trabalho (sem a barra de tarefas).
    pub work: IRect,
    /// Escala do sistema (1.0 = 96 dpi).
    pub scale: f32,
    pub primary: bool,
}

/// Onde a janela vai aparecer.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub pos: [i32; 2],
    pub size: [i32; 2],
    /// Índice em `monitors`.
    pub monitor: usize,
    /// A janela foi trazida/encolhida para caber (o monitor dela sumiu ou ela estava fora da tela).
    pub moved: bool,
}

/// Altura reservada à barra de título do sistema (em 96 dpi) ao trazer uma janela para dentro da área visível.
pub const CHROME_H: i32 = 40;
/// Menor janela solta (área cliente, em 96 dpi).
pub const MIN_W: i32 = 240;
pub const MIN_H: i32 = 160;

/// O monitor que contém a maior parte do retângulo (ou `None` se não toca nenhum).
pub fn monitor_of(r: &IRect, monitors: &[Monitor]) -> Option<usize> {
    monitors.iter().enumerate().map(|(i, m)| (i, m.rect.overlap(r))).filter(|(_, a)| *a > 0).max_by_key(|(_, a)| *a).map(|(i, _)| i)
}

/// Escolhe o monitor e a posição de uma janela guardada. Ordem: o monitor de antes (mesmo nome e mesma área); o monitor
/// que contém a maior parte da janela; o de mesmo nome (a resolução mudou); o principal. A janela fica onde estava se a
/// barra de título dela continua alcançável; senão é encolhida para caber e trazida para dentro da área de trabalho do
/// monitor escolhido. Sem monitores conhecidos devolve `None` (fica como está).
/// `whole`: a janela tem de caber inteira na área de trabalho (janela nova); sem ele basta a barra de título alcançável
/// (janela que o usuário deixou meio para fora fica onde ele pôs).
pub fn place(pos: Option<[i32; 2]>, size: [i32; 2], saved: Option<&SavedMonitor>, monitors: &[Monitor], whole: bool) -> Option<Placement> {
    if monitors.is_empty() {
        return None;
    }
    let win = pos.map(|p| IRect::new(p[0], p[1], size[0].max(1), size[1].max(1) + CHROME_H));
    let same = saved.and_then(|s| monitors.iter().position(|m| m.name == s.name && m.work == s.work));
    let by_area = win.as_ref().and_then(|w| {
        monitors.iter().enumerate().map(|(i, m)| (i, m.work.overlap(w))).filter(|(_, a)| *a > 0).max_by_key(|(_, a)| *a).map(|(i, _)| i)
    });
    let by_name = saved.and_then(|s| monitors.iter().position(|m| m.name == s.name));
    let primary = monitors.iter().position(|m| m.primary).unwrap_or(0);
    let mi = same.or(by_area).or(by_name).unwrap_or(primary);
    let m = &monitors[mi];
    let wa = m.work;
    let chrome = (CHROME_H as f32 * m.scale).round() as i32;
    let (min_w, min_h) = ((MIN_W as f32 * m.scale) as i32, (MIN_H as f32 * m.scale) as i32);
    let w = size[0].max(min_w).min(wa.w.max(min_w));
    let h = size[1].max(min_h).min((wa.h - chrome).max(min_h));
    let mut moved = w != size[0].max(min_w) || h != size[1].max(min_h);
    let p = match pos {
        None => [wa.x + (wa.w - w) / 2, wa.y + (wa.h - h - chrome).max(0) / 2],
        Some(p) => {
            // barra de título alcançável: pelo menos min(metade da largura, 200 px) dela dentro de alguma área de trabalho
            let strip = IRect::new(p[0], p[1], w, chrome);
            let need = (w / 2).min((200.0 * m.scale) as i32).max(1) as i64 * chrome.max(1) as i64;
            let reachable = if whole {
                let full = IRect::new(p[0], p[1], w, h + chrome);
                full.overlap(&wa) == full.w as i64 * full.h as i64
            } else {
                monitors.iter().any(|mm| mm.work.overlap(&strip) >= need)
            };
            if reachable && !moved {
                p
            } else {
                moved = true;
                let x = p[0].clamp(wa.x, (wa.right() - w).max(wa.x));
                let y = p[1].clamp(wa.y, (wa.bottom() - h - chrome).max(wa.y));
                [x, y]
            }
        }
    };
    Some(Placement { pos: p, size: [w, h], monitor: mi, moved })
}

// ------------------------------------------------------------------------------------------ monitores do sistema

/// Os monitores conectados agora (pixels físicos). Fora do Windows: lista vazia (as janelas ficam onde o sistema
/// puser; o egui não expõe a lista de monitores).
pub fn monitors() -> Vec<Monitor> {
    sys::monitors()
}

/// Faz da janela solta (título `child`, deste processo) uma janela "possuída" pela principal (título `owner`), como os
/// painéis soltos do Premiere/Clip Studio: fica sempre na frente da principal (não se perde atrás dela ao clicar na
/// principal) e minimiza junto com ela; continua livre para ir a outro monitor. O egui não expõe o dono da janela no
/// `ViewportBuilder`, então é feito pelo sistema depois que a janela existe (chamar da thread da interface: só as
/// janelas dela são consideradas). Devolve se achou as duas janelas.
pub fn set_owner(child: &str, owner: &str) -> bool {
    sys::set_owner(child, owner)
}

#[cfg(windows)]
mod sys {
    use super::{IRect, Monitor};
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

    fn irect(r: &RECT) -> IRect {
        IRect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
    }

    unsafe extern "system" fn each(hmon: HMONITOR, _hdc: HDC, _clip: *mut RECT, data: LPARAM) -> windows_sys::core::BOOL {
        // SAFETY: `data` é o `&mut Vec<Monitor>` passado por `monitors` logo abaixo, vivo durante a enumeração.
        let out = unsafe { &mut *(data as *mut Vec<Monitor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `info` é um MONITORINFOEXW com cbSize certo; a API aceita o ponteiro como MONITORINFO.
        if unsafe { GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO) } == 0 {
            return 1;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        // SAFETY: ponteiros para variáveis locais; falha = 96 dpi.
        let scale = if unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } == 0 { dx as f32 / 96.0 } else { 1.0 };
        let len = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
        out.push(Monitor {
            name: String::from_utf16_lossy(&info.szDevice[..len]),
            rect: irect(&info.monitorInfo.rcMonitor),
            work: irect(&info.monitorInfo.rcWork),
            scale,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
        1
    }

    unsafe extern "system" fn each_window(h: windows_sys::Win32::Foundation::HWND, data: LPARAM) -> windows_sys::core::BOOL {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, InternalGetWindowText};
        // SAFETY: `data` é o `&mut Vec<(HWND, String)>` de `set_owner`, vivo durante a enumeração.
        let out = unsafe { &mut *(data as *mut Vec<(windows_sys::Win32::Foundation::HWND, String)>) };
        let mut pid = 0u32;
        // SAFETY: `h` veio do sistema; ponteiros para variáveis locais.
        let thread = unsafe { GetWindowThreadProcessId(h, &mut pid) };
        // só as janelas desta thread (as do winit): ler o título de janela de outra thread manda WM_GETTEXT e espera
        // aquela thread (a do wgpu, a do IME) — se ela estiver esperando por esta, o app trava
        if thread == unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() } {
            let mut buf = [0u16; 512];
            // InternalGetWindowText lê o título guardado sem mandar WM_GETTEXT (nada de reentrar no winit)
            let n = unsafe { InternalGetWindowText(h, buf.as_mut_ptr(), buf.len() as i32) };
            out.push((h, String::from_utf16_lossy(&buf[..n.max(0) as usize])));
        }
        1
    }

    pub fn set_owner(child: &str, owner: &str) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GWLP_HWNDPARENT, SetWindowLongPtrW};
        let mut wins: Vec<(windows_sys::Win32::Foundation::HWND, String)> = Vec::new();
        // SAFETY: o callback só usa `wins` durante a chamada (síncrona).
        unsafe { EnumWindows(Some(each_window), &mut wins as *mut _ as LPARAM) };
        let find = |t: &str| wins.iter().find(|(_, w)| w == t).map(|(h, _)| *h);
        match (find(child), find(owner)) {
            (Some(c), Some(o)) if c != o => {
                // SAFETY: duas janelas deste processo; GWLP_HWNDPARENT numa janela de topo troca o dono.
                unsafe { SetWindowLongPtrW(c, GWLP_HWNDPARENT, o as isize) };
                true
            }
            _ => false,
        }
    }

    pub fn monitors() -> Vec<Monitor> {
        let mut out: Vec<Monitor> = Vec::new();
        // SAFETY: o callback só usa `out` durante a chamada (síncrona).
        unsafe {
            EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut out as *mut Vec<Monitor> as LPARAM);
        }
        out
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn monitors() -> Vec<super::Monitor> {
        Vec::new()
    }
    pub fn set_owner(_child: &str, _owner: &str) -> bool {
        false
    }
}

// ------------------------------------------------------------------------------------------ maximizar

/// Onde um dock é desenhado: a janela principal ou uma janela solta (pelo id da sessão).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockHost {
    Main,
    Float(u64),
}

impl DockHost {
    /// Nome do viewport no `GET_UI_STATE` (`root` ou `float:N`).
    pub fn key(self) -> String {
        match self {
            DockHost::Main => "root".to_owned(),
            DockHost::Float(id) => format!("float:{id}"),
        }
    }
}

/// Painel maximizado por (layout do workspace, dock). Não é gravado (ver o topo do arquivo). O painel é guardado pelo
/// tipo, não pelo tile: refazer a árvore (variante, layout aplicado) não deixa um id velho apontando para outro painel.
#[derive(Debug, Default, Clone)]
pub struct Maximized {
    map: HashMap<(String, DockHost), PanelKind>,
}

impl Maximized {
    pub fn get(&self, slot: &str, host: DockHost) -> Option<PanelKind> {
        self.map.get(&(slot.to_owned(), host)).copied()
    }

    /// Maximiza `kind`; se ele já é o maximizado, restaura. Outro painel maximizado no mesmo dock dá lugar a este.
    /// Devolve se ficou maximizado.
    pub fn toggle(&mut self, slot: &str, host: DockHost, kind: PanelKind) -> bool {
        let k = (slot.to_owned(), host);
        if self.map.get(&k) == Some(&kind) {
            self.map.remove(&k);
            false
        } else {
            self.map.insert(k, kind);
            true
        }
    }

    /// Volta ao layout normal. Devolve se havia algo maximizado.
    pub fn restore(&mut self, slot: &str, host: DockHost) -> bool {
        self.map.remove(&(slot.to_owned(), host)).is_some()
    }

    /// O maximizado que não está mais visível neste dock (fechado, destacado, layout trocado) sai.
    pub fn validate(&mut self, slot: &str, host: DockHost, visible: impl Fn(PanelKind) -> bool) {
        let k = (slot.to_owned(), host);
        if self.map.get(&k).is_some_and(|p| !visible(*p)) {
            self.map.remove(&k);
        }
    }

    /// Esquece um dock (a janela solta fechou).
    pub fn forget(&mut self, host: DockHost) {
        self.map.retain(|(_, h), _| *h != host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32, w: i32, h: i32, primary: bool) -> Monitor {
        Monitor { name: name.into(), rect: IRect::new(x, 0, w, h), work: IRect::new(x, 0, w, h - 48), scale: 1.0, primary }
    }

    fn two() -> Vec<Monitor> {
        vec![mon(r"\\.\DISPLAY1", 0, 1920, 1080, true), mon(r"\\.\DISPLAY2", 1920, 2560, 1440, false)]
    }

    #[test]
    fn a_window_on_a_present_monitor_stays_where_it_was() {
        let ms = two();
        let saved = SavedMonitor { name: r"\\.\DISPLAY2".into(), work: ms[1].work };
        let p = place(Some([2100, 200]), [900, 600], Some(&saved), &ms, false).unwrap();
        assert_eq!((p.pos, p.size, p.monitor, p.moved), ([2100, 200], [900, 600], 1, false));
    }

    #[test]
    fn a_missing_monitor_brings_the_window_into_the_visible_area() {
        // estava no segundo monitor, que foi desligado
        let ms = vec![mon(r"\\.\DISPLAY1", 0, 1920, 1080, true)];
        let saved = SavedMonitor { name: r"\\.\DISPLAY2".into(), work: IRect::new(1920, 0, 2560, 1392) };
        let p = place(Some([3000, 300]), [1200, 700], Some(&saved), &ms, false).unwrap();
        assert!(p.moved);
        assert_eq!(p.monitor, 0);
        assert_eq!(p.size, [1200, 700]);
        let wa = ms[0].work;
        assert!(p.pos[0] >= wa.x && p.pos[0] + p.size[0] <= wa.right(), "{:?}", p.pos);
        assert!(p.pos[1] >= wa.y && p.pos[1] + p.size[1] + CHROME_H <= wa.bottom(), "{:?}", p.pos);
    }

    #[test]
    fn a_window_bigger_than_the_screen_shrinks_and_a_missing_position_centers() {
        let ms = vec![mon(r"\\.\DISPLAY1", 0, 1366, 768, true)];
        let p = place(Some([-5000, -5000]), [2560, 1400], None, &ms, false).unwrap();
        assert!(p.moved);
        assert_eq!(p.size, [1366, 768 - 48 - CHROME_H]);
        assert_eq!(p.pos, [0, 0]);
        let c = place(None, [600, 400], None, &ms, false).unwrap();
        assert_eq!(c.pos, [(1366 - 600) / 2, (768 - 48 - 400 - CHROME_H) / 2]);
        assert_eq!(place(Some([10, 10]), [600, 400], None, &[], false), None, "sem monitores conhecidos fica como está");
    }

    #[test]
    fn a_window_half_off_screen_but_with_a_reachable_title_bar_is_kept() {
        let ms = two();
        // metade para fora à direita do segundo monitor: a barra de título ainda tem mais de 200 px dentro
        let p = place(Some([1920 + 2560 - 400, 100]), [900, 600], None, &ms, false).unwrap();
        assert!(!p.moved);
        // só 50 px da barra dentro: trazida de volta
        let q = place(Some([1920 + 2560 - 50, 100]), [900, 600], None, &ms, false).unwrap();
        assert!(q.moved);
        assert_eq!(q.pos[0] + q.size[0], 1920 + 2560);
    }

    #[test]
    fn a_new_window_must_fit_whole_on_its_monitor() {
        let ms = vec![mon(r"\.\DISPLAY1", 0, 1920, 1080, true)];
        // painel embaixo da janela principal: a janela nova desceria para fora da tela
        let p = place(Some([100, 823]), [1843, 400], None, &ms, true).unwrap();
        assert!(p.moved);
        assert_eq!(p.pos, [1920 - 1843, 1032 - 400 - CHROME_H], "{:?}", p.pos);
        // a mesma janela guardada (o usuário a deixou assim) fica, porque a barra de título está na tela
        assert!(!place(Some([100, 823]), [1843, 400], None, &ms, false).unwrap().moved);
    }

    #[test]
    fn saved_float_round_trips_through_json_and_lists_its_panels() {
        let f = SavedFloat {
            tree: SavedNode::Tabs { children: vec![SavedNode::Pane { panel: "layers".into(), hidden: false }, SavedNode::Pane { panel: "timeline".into(), hidden: false }], active: Some(1), hidden: false },
            pos: Some([2000, 120]),
            size: [800, 500],
            monitor: Some(SavedMonitor { name: r"\\.\DISPLAY2".into(), work: IRect::new(1920, 0, 2560, 1392) }),
            home: Some(FloatHome::Row { block: vec!["canvas".into(), "layers".into()], after: true, frac: 0.25 }),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"type\":\"row\"") && json.contains("DISPLAY2"), "{json}");
        let back: SavedFloat = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.panels(), vec![PanelKind::Layers, PanelKind::Timeline]);
        // campos opcionais ausentes (arquivo mínimo) entram como None
        let min: SavedFloat = serde_json::from_str(r#"{"tree":{"type":"pane","panel":"canvas"},"size":[640,480]}"#).unwrap();
        assert_eq!((min.pos, min.monitor, min.home), (None, None, None));
    }

    #[test]
    fn maximize_toggles_restores_and_drops_a_panel_that_left() {
        let mut m = Maximized::default();
        let main = DockHost::Main;
        assert_eq!(m.get("animacao", main), None);
        assert!(m.toggle("animacao", main, PanelKind::Timeline));
        assert_eq!(m.get("animacao", main), Some(PanelKind::Timeline));
        // cada layout e cada dock têm o seu
        assert_eq!(m.get("pintura", main), None);
        assert_eq!(m.get("animacao", DockHost::Float(1)), None);
        // outro painel toma o lugar; o mesmo de novo restaura
        assert!(m.toggle("animacao", main, PanelKind::Layers));
        assert!(!m.toggle("animacao", main, PanelKind::Layers));
        assert_eq!(m.get("animacao", main), None);
        // painel que saiu do dock (fechado/destacado) deixa de estar maximizado
        m.toggle("animacao", main, PanelKind::Timeline);
        m.validate("animacao", main, |k| k != PanelKind::Timeline);
        assert_eq!(m.get("animacao", main), None);
        m.toggle("animacao", main, PanelKind::Timeline);
        m.validate("animacao", main, |_| true);
        assert_eq!(m.get("animacao", main), Some(PanelKind::Timeline));
        assert!(m.restore("animacao", main));
        assert!(!m.restore("animacao", main));
        m.toggle("x", DockHost::Float(3), PanelKind::Canvas);
        m.forget(DockHost::Float(3));
        assert_eq!(m.get("x", DockHost::Float(3)), None);
    }
}
