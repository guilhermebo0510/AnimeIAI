//! Reprodução do workspace Áudio: um fio próprio toca um trecho de um buffer em blocos pela saída `waveOut` do winmm
//! (já no `windows-sys` do app; nenhuma dependência nova). Cada bloco passa pelo rack compartilhado (`Rack::process_block`,
//! só os efeitos com `realtime()`; a UI troca parâmetros e o A/B durante a reprodução) e, quando o dispositivo termina de
//! tocá-lo, alimenta o `LoudnessMeter` — os medidores mostram o que se ouve, não o que está na fila.
//!
//! O cabeçote vem da posição do dispositivo (`waveOutGetPosition`) mapeada de volta para o quadro da fonte. Sem saída de
//! som (`muted`: testes, captura, `--no-session`, ou nenhum dispositivo), o tempo corre pelo relógio e tudo o mais é igual.

use aiai_audio::AudioBuf;
use aiai_audio::fx::Rack;
use aiai_audio::meters::LoudnessMeter;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Quadros por bloco (21 ms a 48 kHz) e blocos na fila do dispositivo (~130 ms de latência).
pub(crate) const BLOCK: usize = 1024;
const QUEUE: usize = 6;
/// Pontos guardados do histórico de loudness (a cada 100 ms: 2 min).
pub(crate) const HISTORY: usize = 1200;

/// Rack compartilhado entre a UI e o fio: a UI troca a cadeia e sobe `version`; o fio copia, zera o estado e segue.
#[derive(Default)]
pub(crate) struct SharedRack {
    pub version: u64,
    pub rack: Rack,
}

/// Leitura dos medidores (cópia para a UI).
#[derive(Clone, Debug, Default)]
pub(crate) struct MeterSnap {
    pub momentary: Option<f64>,
    pub short_term: Option<f64>,
    pub integrated: Option<f64>,
    pub lra: Option<f64>,
    pub true_peak: Option<f64>,
    /// Pico de amostra (linear) do último bloco tocado, por canal (1 ou 2).
    pub peak: Vec<f32>,
    /// Curto prazo (LUFS) a cada 100 ms, o mais novo no fim.
    pub history: Vec<f32>,
    pub seconds: f64,
    /// Posições do rack puladas por não rodarem em tempo real.
    pub skipped: usize,
}

/// Alimenta um `LoudnessMeter` e mantém a leitura pronta (usado pelo fio e pelo medidor da linha do tempo).
pub(crate) struct MeterFeed {
    pub meter: LoudnessMeter,
    pub snap: MeterSnap,
    next_hist: f64,
}

impl MeterFeed {
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        MeterFeed { meter: LoudnessMeter::new(rate, channels.max(1)), snap: MeterSnap { peak: vec![0.0; channels.max(1)], ..Default::default() }, next_hist: 0.1 }
    }

    pub(crate) fn push(&mut self, data: &[f32], channels: usize) {
        let nch = channels.max(1);
        self.meter.push(data);
        let mut pk = vec![0.0f32; nch];
        for fr in data.chunks_exact(nch) {
            for (c, v) in fr.iter().enumerate() {
                pk[c] = pk[c].max(v.abs());
            }
        }
        let m = &self.meter;
        let s = &mut self.snap;
        s.peak = pk;
        s.momentary = m.momentary_lufs();
        s.short_term = m.short_term_lufs();
        s.integrated = m.integrated_lufs();
        s.lra = m.lra_lu();
        s.true_peak = m.true_peak_dbtp();
        s.seconds = m.duration_s();
        while s.seconds >= self.next_hist {
            self.next_hist += 0.1;
            s.history.push(s.short_term.or(s.momentary).unwrap_or(-70.0).max(-70.0) as f32);
            if s.history.len() > HISTORY {
                let cut = s.history.len() - HISTORY;
                s.history.drain(..cut);
            }
        }
    }
}

/// O que tocar.
pub(crate) struct PlayRequest {
    pub buf: Arc<AudioBuf>,
    pub start: usize,
    pub end: usize,
    pub looping: bool,
    pub rack: Option<Arc<Mutex<SharedRack>>>,
    /// Identifica a fonte (asset, prévia renderizada, mixagem do corte) para a UI saber de quem é o cabeçote.
    pub key: String,
    pub label: String,
}

struct Shared {
    stop: AtomicBool,
    done: AtomicBool,
    /// Quadro da fonte que soa agora.
    pos: AtomicU64,
    meter: Mutex<MeterSnap>,
    error: Mutex<Option<String>>,
}

pub(crate) struct Playing {
    shared: Arc<Shared>,
    pub key: String,
    pub label: String,
    pub rate: u32,
}

#[derive(Default)]
pub(crate) struct Player {
    cur: Option<Playing>,
    /// Sem saída de som: o tempo corre pelo relógio.
    pub muted: bool,
    /// Última leitura dos medidores (fica depois de parar).
    pub last_meter: MeterSnap,
    pub last_label: String,
    pub last_error: Option<String>,
    /// Quantas vezes tocou (estado para a bridge e testes).
    pub starts: u32,
}

impl Player {
    pub(crate) fn play(&mut self, req: PlayRequest) {
        self.stop();
        let n = req.buf.frames();
        let (start, end) = (req.start.min(n), req.end.min(n));
        if end <= start {
            return;
        }
        let shared = Arc::new(Shared { stop: AtomicBool::new(false), done: AtomicBool::new(false), pos: AtomicU64::new(start as u64), meter: Mutex::new(MeterSnap::default()), error: Mutex::new(None) });
        let (sh, buf, rack, looping, muted) = (shared.clone(), req.buf.clone(), req.rack.clone(), req.looping, self.muted);
        let rate = req.buf.rate;
        let spawned = std::thread::Builder::new().name("aiai-audio-play".into()).spawn(move || run(sh, buf, start, end, looping, rack, muted));
        if spawned.is_ok() {
            self.starts += 1;
            self.last_label = req.label.clone();
            self.cur = Some(Playing { shared, key: req.key, label: req.label, rate });
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(p) = self.cur.take() {
            p.shared.stop.store(true, Ordering::Relaxed);
            if let Ok(m) = p.shared.meter.lock() {
                self.last_meter = m.clone();
            }
        }
    }

    /// Chamado a cada quadro: recolhe o fio que terminou e copia os medidores.
    pub(crate) fn tick(&mut self) {
        let Some(p) = &self.cur else { return };
        if let Ok(m) = p.shared.meter.lock() {
            self.last_meter = m.clone();
        }
        if let Ok(mut e) = p.shared.error.lock() {
            if let Some(e) = e.take() {
                self.last_error = Some(e);
            }
        }
        if p.shared.done.load(Ordering::Relaxed) {
            self.cur = None;
        }
    }

    pub(crate) fn playing(&self) -> Option<&Playing> {
        self.cur.as_ref().filter(|p| !p.shared.done.load(Ordering::Relaxed))
    }

    pub(crate) fn is_playing(&self, key: &str) -> bool {
        self.playing().is_some_and(|p| p.key == key)
    }

    /// Quadro da fonte que soa agora (se tocando).
    pub(crate) fn position(&self) -> Option<usize> {
        self.playing().map(|p| p.shared.pos.load(Ordering::Relaxed) as usize)
    }
}

/// Um bloco na fila do dispositivo: de onde veio na fonte, onde começa na saída e o áudio processado (para o medidor).
struct InFlight {
    slot: usize,
    src: usize,
    out_start: u64,
    frames: usize,
    data: Vec<f32>,
}

fn run(sh: Arc<Shared>, buf: Arc<AudioBuf>, start: usize, end: usize, looping: bool, rack: Option<Arc<Mutex<SharedRack>>>, muted: bool) {
    let nch = buf.channels().clamp(1, 2);
    let rate = buf.rate.max(1);
    let mut dev = if muted {
        None
    } else {
        match out::Device::open(rate, BLOCK, QUEUE) {
            Ok(d) => Some(d),
            Err(e) => {
                if let Ok(mut x) = sh.error.lock() {
                    *x = Some(e);
                }
                None
            }
        }
    };
    let mut feed = MeterFeed::new(rate, nch);
    let mut local: Option<(u64, Rack)> = None;
    let mut cursor = start;
    let mut written: u64 = 0;
    let mut flight: VecDeque<InFlight> = VecDeque::new();
    let mut src_done = false;
    let clock = Instant::now();
    let mut skipped = 0usize;
    loop {
        if sh.stop.load(Ordering::Relaxed) {
            break;
        }
        // posição tocada (quadros de saída)
        let played = match &dev {
            Some(d) => d.position().unwrap_or(0),
            None => (clock.elapsed().as_secs_f64() * rate as f64) as u64,
        };
        // blocos que o dispositivo terminou: medidor e vaga livre
        while let Some(f) = flight.front() {
            let finished = match &dev {
                Some(d) => d.is_done(f.slot),
                None => played >= f.out_start + f.frames as u64,
            };
            if !finished {
                break;
            }
            let f = flight.pop_front().unwrap_or_else(|| unreachable!());
            feed.push(&f.data, nch);
        }
        if let Some(f) = flight.iter().find(|f| played < f.out_start + f.frames as u64).or(flight.back()) {
            let off = played.saturating_sub(f.out_start).min(f.frames as u64) as usize;
            let mut pos = f.src + off;
            if pos >= end {
                pos = if looping { start + (pos - end) % (end - start).max(1) } else { end };
            }
            sh.pos.store(pos as u64, Ordering::Relaxed);
        }
        if let Ok(mut m) = sh.meter.lock() {
            *m = feed.snap.clone();
            m.skipped = skipped;
        }
        if src_done && flight.is_empty() {
            break;
        }
        // encher a fila
        let mut wrote = false;
        while !src_done && flight.len() < QUEUE {
            let slot = match &dev {
                Some(d) => match (0..QUEUE).find(|s| !flight.iter().any(|f| f.slot == *s) && d.is_free(*s)) {
                    Some(s) => s,
                    None => break,
                },
                None => 0,
            };
            // um bloco inteiro: no laço, continua do início do trecho dentro do mesmo bloco (sem buraco nem deriva do
            // cabeçote); sem laço, o último bloco é curto (o dispositivo completa com silêncio)
            let src = cursor;
            let mut data = Vec::with_capacity(BLOCK * nch);
            let mut n = 0;
            while n < BLOCK {
                if cursor >= end {
                    if looping {
                        cursor = start;
                    } else {
                        src_done = true;
                        break;
                    }
                }
                for c in 0..nch {
                    data.push(buf.ch[c][cursor]);
                }
                cursor += 1;
                n += 1;
            }
            if cursor >= end && !looping {
                src_done = true;
            }
            if let Some(r) = &rack {
                if let Ok(g) = r.lock() {
                    if local.as_ref().is_none_or(|(v, _)| *v != g.version) {
                        let mut copy = g.rack.clone();
                        copy.reset();
                        local = Some((g.version, copy));
                    }
                }
                if let Some((_, rk)) = &mut local {
                    skipped = rk.process_block(&mut data, nch, rate);
                    for v in &mut data {
                        if !v.is_finite() {
                            *v = 0.0;
                        }
                    }
                }
            }
            if n == 0 {
                break;
            }
            if let Some(d) = &mut dev {
                d.write(slot, &data, nch);
            }
            // o dispositivo toca o buffer inteiro (BLOCK quadros) mesmo quando o último bloco é curto
            let out_frames = if dev.is_some() { BLOCK } else { n };
            flight.push_back(InFlight { slot, src, out_start: written, frames: out_frames, data });
            written += out_frames as u64;
            wrote = true;
        }
        if !wrote {
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    drop(dev);
    if let Ok(mut m) = sh.meter.lock() {
        *m = feed.snap.clone();
        m.skipped = skipped;
    }
    sh.done.store(true, Ordering::Relaxed);
}

#[cfg(windows)]
mod out {
    use windows_sys::Win32::Media::Audio::{HWAVEOUT, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, WHDR_DONE, waveOutClose, waveOutGetPosition, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite};
    use windows_sys::Win32::Media::{MMTIME, TIME_SAMPLES};

    /// Cabeçalho com alinhamento de 8 (o `WAVEHDR` do winmm é empacotado; o campo de flags fica alinhado assim).
    #[repr(C, align(8))]
    struct Hdr(WAVEHDR);

    /// Saída `waveOut` estéreo PCM 16 bits com `n` buffers fixos de `block` quadros.
    pub(super) struct Device {
        h: HWAVEOUT,
        hdrs: Vec<Box<Hdr>>,
        data: Vec<Vec<i16>>,
        used: Vec<bool>,
        block: usize,
    }

    // SAFETY: o handle e os buffers só são usados pelo fio de reprodução que os criou.
    unsafe impl Send for Device {}

    impl Device {
        pub(super) fn open(rate: u32, block: usize, n: usize) -> Result<Device, String> {
            let fmt = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 2, nSamplesPerSec: rate, nAvgBytesPerSec: rate * 4, nBlockAlign: 4, wBitsPerSample: 16, cbSize: 0 };
            let mut h: HWAVEOUT = std::ptr::null_mut();
            // SAFETY: formato válido; sem callback (CALLBACK_NULL = 0); o handle volta em `h`.
            let r = unsafe { waveOutOpen(&mut h, WAVE_MAPPER, &fmt, 0, 0, 0) };
            if r != 0 || h.is_null() {
                return Err(format!("waveOutOpen falhou (MMRESULT {r})"));
            }
            let mut d = Device { h, hdrs: Vec::new(), data: (0..n).map(|_| vec![0i16; block * 2]).collect(), used: vec![false; n], block };
            for i in 0..n {
                let mut hdr = WAVEHDR::default();
                hdr.lpData = d.data[i].as_mut_ptr().cast();
                hdr.dwBufferLength = (block * 4) as u32;
                let mut b = Box::new(Hdr(hdr));
                // SAFETY: cabeçalho e buffer vivem enquanto o dispositivo existir (caixa e Vec fixos).
                let r = unsafe { waveOutPrepareHeader(d.h, &mut b.0, std::mem::size_of::<WAVEHDR>() as u32) };
                if r != 0 {
                    return Err(format!("waveOutPrepareHeader falhou (MMRESULT {r})"));
                }
                d.hdrs.push(b);
            }
            Ok(d)
        }

        fn flags(&self, i: usize) -> u32 {
            let p: *const WAVEHDR = &self.hdrs[i].0;
            // SAFETY: leitura volátil do campo que o driver escreve; o cabeçalho está alinhado (Hdr) e vivo.
            unsafe { std::ptr::read_volatile(std::ptr::addr_of!((*p).dwFlags)) }
        }

        pub(super) fn is_free(&self, i: usize) -> bool {
            !self.used[i] || self.flags(i) & WHDR_DONE != 0
        }

        pub(super) fn is_done(&self, i: usize) -> bool {
            self.used[i] && self.flags(i) & WHDR_DONE != 0
        }

        /// Escreve um bloco (entrelaçado, `nch` canais) no buffer `i`; mono vira estéreo; o resto do buffer fica em silêncio.
        pub(super) fn write(&mut self, i: usize, data: &[f32], nch: usize) {
            let frames = (data.len() / nch.max(1)).min(self.block);
            let out = &mut self.data[i];
            for f in 0..self.block {
                let (l, r) = if f < frames {
                    let l = data[f * nch];
                    (l, if nch > 1 { data[f * nch + 1] } else { l })
                } else {
                    (0.0, 0.0)
                };
                out[f * 2] = (l.clamp(-1.0, 1.0) * 32767.0) as i16;
                out[f * 2 + 1] = (r.clamp(-1.0, 1.0) * 32767.0) as i16;
            }
            let hdr = &mut self.hdrs[i].0;
            hdr.dwFlags &= !WHDR_DONE;
            self.used[i] = true;
            // SAFETY: cabeçalho preparado, buffer vivo e não está na fila (is_free conferiu).
            unsafe {
                waveOutWrite(self.h, hdr, std::mem::size_of::<WAVEHDR>() as u32);
            }
        }

        /// Quadros já tocados desde a abertura.
        pub(super) fn position(&self) -> Option<u64> {
            let mut t = MMTIME { wType: TIME_SAMPLES, ..Default::default() };
            // SAFETY: estrutura local com o tamanho certo.
            let r = unsafe { waveOutGetPosition(self.h, &mut t, std::mem::size_of::<MMTIME>() as u32) };
            // SAFETY: com TIME_SAMPLES o driver preenche `sample` (o tipo volta igual quando suportado).
            (r == 0 && t.wType == TIME_SAMPLES).then(|| unsafe { t.u.sample } as u64)
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            // SAFETY: reset devolve todos os buffers; depois de despreparar, o handle fecha.
            unsafe {
                waveOutReset(self.h);
                for b in &mut self.hdrs {
                    waveOutUnprepareHeader(self.h, &mut b.0, std::mem::size_of::<WAVEHDR>() as u32);
                }
                waveOutClose(self.h);
            }
        }
    }
}

#[cfg(not(windows))]
mod out {
    pub(super) struct Device;
    impl Device {
        pub(super) fn open(_rate: u32, _block: usize, _n: usize) -> Result<Device, String> {
            Err("sem saída de som nesta plataforma".into())
        }
        pub(super) fn is_free(&self, _i: usize) -> bool {
            true
        }
        pub(super) fn is_done(&self, _i: usize) -> bool {
            true
        }
        pub(super) fn write(&mut self, _i: usize, _data: &[f32], _nch: usize) {}
        pub(super) fn position(&self) -> Option<u64> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_audio::fx::{Amplify, AudioFx};

    fn sine(rate: u32, secs: f64, amp: f32) -> AudioBuf {
        let n = (rate as f64 * secs) as usize;
        AudioBuf::mono(rate, (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin()).collect())
    }

    #[test]
    fn muted_playback_runs_by_the_clock_and_meters_what_was_played() {
        let mut p = Player { muted: true, ..Default::default() };
        let buf = Arc::new(sine(8000, 0.6, 0.5));
        p.play(PlayRequest { buf: buf.clone(), start: 0, end: buf.frames(), looping: false, rack: None, key: "a".into(), label: "a".into() });
        assert!(p.is_playing("a"));
        let t0 = Instant::now();
        let mut last = 0;
        while p.playing().is_some() {
            p.tick();
            if let Some(pos) = p.position() {
                assert!(pos >= last, "cabeçote não volta sem laço");
                last = pos;
            }
            assert!(t0.elapsed().as_secs() < 10, "não terminou");
            std::thread::sleep(Duration::from_millis(10));
        }
        p.tick();
        assert!(t0.elapsed().as_secs_f64() >= 0.5, "tocou na velocidade do relógio");
        let m = &p.last_meter;
        assert!((m.seconds - 0.6).abs() < 0.2, "mediu o que tocou: {}", m.seconds);
        // seno de pico 0,5 = -6 dBFS de pico
        assert!(m.true_peak.is_some_and(|tp| (tp + 6.0).abs() < 0.6), "{:?}", m.true_peak);
        assert!(m.momentary.is_some());
    }

    #[test]
    fn the_shared_rack_processes_blocks_and_changes_live() {
        let mut p = Player { muted: true, ..Default::default() };
        // 48 kHz: a fila do dispositivo (6 blocos) fica em ~130 ms e a troca do rack aparece logo
        let buf = Arc::new(sine(48_000, 3.0, 0.25));
        let mut rack = Rack::new();
        rack.push(AudioFx::Amplify(Amplify { gain_db: 6.0, allow_clipping: true }));
        let shared = Arc::new(Mutex::new(SharedRack { version: 1, rack }));
        p.play(PlayRequest { buf: buf.clone(), start: 0, end: buf.frames(), looping: true, rack: Some(shared.clone()), key: "b".into(), label: "b".into() });
        let wait = |p: &mut Player, secs: f64| {
            let t = Instant::now();
            while t.elapsed().as_secs_f64() < secs {
                p.tick();
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&mut p, 0.4);
        let pk = p.last_meter.peak.first().copied().unwrap_or(0.0);
        assert!((pk - 0.5).abs() < 0.05, "+6 dB no bloco: {pk}");
        // A/B: desliga o rack durante a reprodução
        if let Ok(mut g) = shared.lock() {
            g.rack.bypass = true;
            g.version += 1;
        }
        wait(&mut p, 0.5);
        let pk = p.last_meter.peak.first().copied().unwrap_or(0.0);
        assert!((pk - 0.25).abs() < 0.05, "sem rack: {pk}");
        assert!(p.playing().is_some(), "laço continua");
        p.stop();
        assert!(p.playing().is_none());
    }
}
