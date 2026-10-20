//! Tela de abertura: aparece na hora em que o processo começa e some quando a interface principal está pronta.
//!
//! Por que uma janela Win32 própria e não um overlay do eframe: a criação do `StudioApp` (fontes, abertura do projeto,
//! reprodução do diário) roda dentro do callback de criação do eframe, antes do primeiro quadro, e a janela do eframe só
//! aparece depois dele; nada do egui chega a ser desenhado nesse intervalo. A janela daqui vive numa thread própria
//! (com a sua fila de mensagens) e é desenhada com GDI, então aparece antes de a GPU ser iniciada e continua animada
//! enquanto a thread principal está ocupada.
//!
//! Conteúdo: a marca com o nome, a descrição, a versão, uma linha de estado ("Abrindo projeto…") e uma barra de espera.
//! Claro ou escuro conforme o tema do app. Nunca aparece com `--no-session`, `--screenshot` ou `--no-splash` (testes e
//! harness não ganham janela extra); `--splash` força. Não atrasa a abertura: o desenho é feito na própria thread e o
//! único tempo mínimo é o de não piscar (a tela fica no mínimo 450 ms quando aparece).

#[cfg(windows)]
use windows_impl as imp;

/// O que a tela precisa saber (já traduzido: o idioma e o tema vêm das preferências e da linha de comando).
#[derive(Debug, Clone)]
pub struct SplashInfo {
    pub dark: bool,
    pub version: String,
    pub tagline: String,
    pub status: String,
}

/// Abre a tela de abertura (retorna na hora; a janela sobe numa thread própria). Sem efeito fora do Windows.
pub fn start(info: SplashInfo) {
    imp::start(info);
}

/// Troca a linha de estado ("Abrindo projeto…"). Sem a tela aberta não faz nada.
pub fn status(text: &str) {
    imp::status(text);
}

/// A interface está pronta: a tela some (com um fade curto, e só depois do tempo mínimo).
pub fn finish() {
    imp::finish();
}

/// A tela está aberta (ainda não pediram para fechar).
pub fn is_active() -> bool {
    imp::is_active()
}

#[cfg(not(windows))]
mod imp {
    use super::SplashInfo;
    pub fn start(_: SplashInfo) {}
    pub fn status(_: &str) {}
    pub fn finish() {}
    pub fn is_active() -> bool {
        false
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::SplashInfo;
    use crate::brand;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC,
        InvalidateRect, ReleaseDC, SelectObject, SetBkMode, SetTextColor, UpdateWindow, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
        DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, FW_NORMAL,
        HBITMAP, HDC, OUT_DEFAULT_PRECIS, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetSystemMetrics, KillTimer, LoadCursorW, PostMessageW, PostQuitMessage,
        RegisterClassExW, SetLayeredWindowAttributes, SetTimer, ShowWindow, TranslateMessage, CS_NOCLOSE, IDC_APPSTARTING, LWA_ALPHA, MSG,
        SM_CXSCREEN, SM_CYSCREEN, SW_SHOWNOACTIVATE, WM_APP, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    const WM_STATUS: u32 = WM_APP + 1;
    const WM_FINISH: u32 = WM_APP + 2;
    const TIMER_ANIM: usize = 1;
    /// A tela fica no mínimo este tempo quando aparece (não pisca se o app abrir muito rápido).
    const MIN_VISIBLE: Duration = Duration::from_millis(450);
    const FADE_STEP: u8 = 40;

    /// Tamanho lógico da janela (multiplicado pela escala do sistema).
    const W: f32 = 560.0;
    const H: f32 = 330.0;

    static HWND_SPLASH: AtomicIsize = AtomicIsize::new(0);
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static FINISH_EARLY: AtomicBool = AtomicBool::new(false);
    static STATUS: Mutex<String> = Mutex::new(String::new());
    static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

    struct Palette {
        top: [u8; 3],
        bottom: [u8; 3],
        border: [u8; 3],
        dim: [u8; 3],
        text: [u8; 3],
        accent: [u8; 3],
        track: [u8; 3],
    }

    const DARK: Palette = Palette { top: [0x22, 0x23, 0x27], bottom: [0x0f, 0x10, 0x12], border: [0x3a, 0x3c, 0x45], dim: [0xa6, 0xa8, 0xb1], text: [0xdd, 0xde, 0xe2], accent: [0xf0, 0x98, 0x3c], track: [0x2e, 0x30, 0x37] };
    const LIGHT: Palette = Palette { top: [0xff, 0xff, 0xff], bottom: [0xe9, 0xeb, 0xef], border: [0xc4, 0xc7, 0xcf], dim: [0x5a, 0x5d, 0x66], text: [0x1f, 0x20, 0x24], accent: [0xc2, 0x60, 0x0c], track: [0xd8, 0xda, 0xe0] };

    /// Tudo que a janela precisa para pintar (handles GDI são inteiros, para a struct poder ficar num `static`).
    struct Window {
        w: i32,
        h: i32,
        scale: f32,
        stat_dc: isize,
        back_dc: isize,
        font: isize,
        pal: &'static Palette,
        shown_at: Instant,
        closing: bool,
        close_at: Option<Instant>,
        alpha: u8,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn rgb(c: [u8; 3]) -> COLORREF {
        c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16
    }

    pub fn is_active() -> bool {
        ACTIVE.load(Ordering::Relaxed)
    }

    pub fn start(info: SplashInfo) {
        if ACTIVE.swap(true, Ordering::SeqCst) {
            return;
        }
        *STATUS.lock().unwrap() = info.status.clone();
        let spawned = std::thread::Builder::new().name("aiai-splash".into()).spawn(move || {
            if let Err(e) = run(info) {
                tracing::warn!("tela de abertura: {e}");
            }
            ACTIVE.store(false, Ordering::SeqCst);
            HWND_SPLASH.store(0, Ordering::SeqCst);
        });
        if let Err(e) = spawned {
            tracing::warn!("tela de abertura: thread: {e}");
            ACTIVE.store(false, Ordering::SeqCst);
        }
    }

    pub fn status(text: &str) {
        if !ACTIVE.load(Ordering::Relaxed) {
            return;
        }
        *STATUS.lock().unwrap() = text.to_owned();
        let h = HWND_SPLASH.load(Ordering::SeqCst);
        if h != 0 {
            unsafe { PostMessageW(h as HWND, WM_STATUS, 0, 0) };
        }
    }

    pub fn finish() {
        if !ACTIVE.load(Ordering::Relaxed) {
            return;
        }
        FINISH_EARLY.store(true, Ordering::SeqCst);
        let h = HWND_SPLASH.load(Ordering::SeqCst);
        if h != 0 {
            unsafe { PostMessageW(h as HWND, WM_FINISH, 0, 0) };
        }
    }

    /// Seção DIB 32 bpp de cima para baixo; devolve o bitmap e o ponteiro dos pixels (BGRA).
    unsafe fn dib(dc: HDC, w: i32, h: i32) -> Option<(HBITMAP, *mut u8)> {
        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0, biClrUsed: 0, biClrImportant: 0 };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = unsafe { CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0) };
        (!bmp.is_null() && !bits.is_null()).then_some((bmp, bits as *mut u8))
    }

    /// Fundo em degradê, marca com o nome e borda de 1 px, como pixels BGRA opacos.
    fn render_background(w: u32, h: u32, scale: f32, dark: bool) -> Result<Vec<u8>, String> {
        use resvg::tiny_skia::{Pixmap, Transform};
        let pal = if dark { &DARK } else { &LIGHT };
        let mut pm = Pixmap::new(w, h).ok_or("pixmap inválido")?;
        let stride = w as usize * 4;
        for y in 0..h as usize {
            let t = y as f32 / (h - 1) as f32;
            let c: Vec<u8> = (0..3).map(|i| (pal.top[i] as f32 + (pal.bottom[i] as f32 - pal.top[i] as f32) * t).round() as u8).collect();
            for x in 0..w as usize {
                let o = y * stride + x * 4;
                pm.data_mut()[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        // a marca com o nome: 380 pt de largura, centrada, na metade de cima
        let svg = brand::wordmark_svg(if dark { crate::theme::ThemeKind::Dark } else { crate::theme::ThemeKind::Light });
        let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).map_err(|e| e.to_string())?;
        let size = tree.size();
        let target_w = 380.0 * scale;
        let k = target_w / size.width();
        let (tw, th) = (size.width() * k, size.height() * k);
        let tx = (w as f32 - tw) / 2.0;
        let ty = 70.0 * scale;
        resvg::render(&tree, Transform::from_translate(tx, ty).pre_scale(k, k), &mut pm.as_mut());
        let _ = th;
        // borda
        let mut px = pm.take();
        for x in 0..w as usize {
            for y in [0usize, h as usize - 1] {
                let o = y * stride + x * 4;
                px[o..o + 4].copy_from_slice(&[pal.border[0], pal.border[1], pal.border[2], 255]);
            }
        }
        for y in 0..h as usize {
            for x in [0usize, w as usize - 1] {
                let o = y * stride + x * 4;
                px[o..o + 4].copy_from_slice(&[pal.border[0], pal.border[1], pal.border[2], 255]);
            }
        }
        // RGBA -> BGRA (tudo opaco, então premultiplicado e direto coincidem)
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        Ok(px)
    }

    unsafe fn draw_text(dc: HDC, text: &str, rect: &mut RECT, flags: u32, color: [u8; 3]) {
        unsafe {
            SetTextColor(dc, rgb(color));
            let w = wide(text);
            DrawTextW(dc, w.as_ptr(), (w.len() - 1) as i32, rect, flags | DT_SINGLELINE | DT_NOPREFIX);
        }
    }

    fn run(info: SplashInfo) -> Result<(), String> {
        let dpi = unsafe { GetDpiForSystem() }.max(96);
        let scale = dpi as f32 / 96.0;
        let (w, h) = ((W * scale).round() as i32, (H * scale).round() as i32);
        let pal: &'static Palette = if info.dark { &DARK } else { &LIGHT };
        let bg = render_background(w as u32, h as u32, scale, info.dark)?;

        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let (stat_bmp, stat_bits) = dib(screen, w, h).ok_or("CreateDIBSection falhou")?;
            let (back_bmp, _) = dib(screen, w, h).ok_or("CreateDIBSection falhou")?;
            std::ptr::copy_nonoverlapping(bg.as_ptr(), stat_bits, bg.len());
            let stat_dc = CreateCompatibleDC(screen);
            let back_dc = CreateCompatibleDC(screen);
            ReleaseDC(std::ptr::null_mut(), screen);
            SelectObject(stat_dc, stat_bmp);
            SelectObject(back_dc, back_bmp);
            let face = wide("Segoe UI");
            let font = CreateFontW(-((14.0 * scale).round() as i32), 0, 0, 0, FW_NORMAL as i32, 0, 0, 0, DEFAULT_CHARSET as u32, OUT_DEFAULT_PRECIS as u32, CLIP_DEFAULT_PRECIS as u32, CLEARTYPE_QUALITY as u32, DEFAULT_PITCH as u32, face.as_ptr());
            SelectObject(stat_dc, font);
            SetBkMode(stat_dc, TRANSPARENT as i32);
            // partes fixas do texto: descrição abaixo da marca e versão no canto
            let s = |v: f32| (v * scale).round() as i32;
            let mut r = RECT { left: 0, top: s(70.0 + 380.0 * 128.0 / 559.3 + 22.0), right: w, bottom: s(70.0 + 380.0 * 128.0 / 559.3 + 46.0) };
            draw_text(stat_dc, &info.tagline, &mut r, DT_CENTER, pal.dim);
            let mut r = RECT { left: s(24.0), top: h - s(46.0), right: w - s(24.0), bottom: h - s(22.0) };
            draw_text(stat_dc, &info.version, &mut r, DT_RIGHT | DT_VCENTER, pal.dim);

            *WINDOW.lock().unwrap() = Some(Window { w, h, scale, stat_dc: stat_dc as isize, back_dc: back_dc as isize, font: font as isize, pal, shown_at: Instant::now(), closing: false, close_at: None, alpha: 255 });

            let hinst = GetModuleHandleW(std::ptr::null());
            let class = wide("AnimeIAISplash");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_NOCLOSE,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinst,
                hIcon: std::ptr::null_mut(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_APPSTARTING),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            if RegisterClassExW(&wc) == 0 {
                return Err("RegisterClassExW falhou".into());
            }
            let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2 - s(20.0);
            let title = wide("AnimeIAI");
            let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED, class.as_ptr(), title.as_ptr(), WS_POPUP, x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());
            if hwnd.is_null() {
                return Err("CreateWindowExW falhou".into());
            }
            round_corners(hwnd);
            SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
            HWND_SPLASH.store(hwnd as isize, Ordering::SeqCst);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            UpdateWindow(hwnd);
            SetTimer(hwnd, TIMER_ANIM, 33, None);
            // o app pode ter terminado de abrir antes de a janela existir
            if FINISH_EARLY.load(Ordering::SeqCst) {
                PostMessageW(hwnd, WM_FINISH, 0, 0);
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // liberação dos recursos GDI
            if let Some(win) = WINDOW.lock().unwrap().take() {
                DeleteDC(win.stat_dc as HDC);
                DeleteDC(win.back_dc as HDC);
                DeleteObject(win.font as _);
            }
            DeleteObject(stat_bmp as _);
            DeleteObject(back_bmp as _);
        }
        Ok(())
    }

    /// Cantos arredondados no Windows 11 (no 10 o atributo não existe e o retângulo fica como está).
    unsafe fn round_corners(hwnd: HWND) {
        use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
        let pref: i32 = DWMWCP_ROUND;
        unsafe { DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &pref as *const i32 as *const _, 4) };
    }

    unsafe fn paint(hwnd: HWND) {
        let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
        let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
        if let Some(win) = WINDOW.lock().unwrap().as_ref() {
            let (back, stat) = (win.back_dc as HDC, win.stat_dc as HDC);
            let s = |v: f32| (v * win.scale).round() as i32;
            unsafe {
                BitBlt(back, 0, 0, win.w, win.h, stat, 0, 0, SRCCOPY);
                // barra de espera: trilho e um trecho que desliza (ease in-out)
                let (tw, th) = (s(240.0), s(3.0).max(2));
                let tx = (win.w - tw) / 2;
                let ty = s(70.0 + 380.0 * 128.0 / 559.3 + 22.0 + 24.0 + 26.0);
                let track = CreateSolidBrush(rgb(win.pal.track));
                FillRect(back, &RECT { left: tx, top: ty, right: tx + tw, bottom: ty + th }, track);
                DeleteObject(track as _);
                let ms = win.shown_at.elapsed().as_millis() as f32;
                let t = (ms % 1500.0) / 1500.0;
                let e = t * t * (3.0 - 2.0 * t);
                let seg = s(90.0);
                let x0 = tx - seg + ((tw + seg) as f32 * e) as i32;
                let (a, b) = (x0.max(tx), (x0 + seg).min(tx + tw));
                if b > a {
                    let accent = CreateSolidBrush(rgb(win.pal.accent));
                    FillRect(back, &RECT { left: a, top: ty, right: b, bottom: ty + th }, accent);
                    DeleteObject(accent as _);
                }
                // linha de estado (embaixo, à esquerda)
                SelectObject(back, win.font as _);
                SetBkMode(back, TRANSPARENT as i32);
                let text = STATUS.lock().unwrap().clone();
                let mut r = RECT { left: s(24.0), top: win.h - s(46.0), right: win.w - s(150.0), bottom: win.h - s(22.0) };
                draw_text(back, &text, &mut r, DT_LEFT | DT_VCENTER | DT_END_ELLIPSIS, win.pal.text);
                BitBlt(hdc, 0, 0, win.w, win.h, back, 0, 0, SRCCOPY);
            }
        }
        unsafe { EndPaint(hwnd, &ps) };
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                unsafe { paint(hwnd) };
                0
            }
            WM_STATUS => {
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
                0
            }
            WM_FINISH => {
                if let Some(win) = WINDOW.lock().unwrap().as_mut() {
                    // respeita o tempo mínimo; o timer cuida do resto
                    win.close_at = Some(win.shown_at + MIN_VISIBLE);
                }
                0
            }
            WM_TIMER => {
                let mut destroy = false;
                if let Some(win) = WINDOW.lock().unwrap().as_mut() {
                    if !win.closing && win.close_at.is_some_and(|t| Instant::now() >= t) {
                        win.closing = true;
                    }
                    if win.closing {
                        win.alpha = win.alpha.saturating_sub(FADE_STEP);
                        unsafe { SetLayeredWindowAttributes(hwnd, 0, win.alpha, LWA_ALPHA) };
                        destroy = win.alpha == 0;
                    }
                }
                if destroy {
                    unsafe { DestroyWindow(hwnd) };
                } else {
                    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
                }
                0
            }
            WM_DESTROY => {
                unsafe {
                    KillTimer(hwnd, TIMER_ANIM);
                    PostQuitMessage(0);
                }
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }
}
