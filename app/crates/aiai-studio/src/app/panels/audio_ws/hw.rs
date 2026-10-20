//! Detecção de hardware do painel Voz sintética: a GPU em uso (adaptador do wgpu, que o app já abre), a VRAM dedicada de
//! cada adaptador de vídeo (o Windows guarda o tamanho no registro, na classe de adaptadores de vídeo), a RAM
//! (`GlobalMemoryStatusEx`) e o espaço livre em disco. Só leitura local: nada de rede nem de processo externo.
//!
//! O veredito (`hq_verdict`) compara com os requisitos do motor opcional de alta qualidade (`HQ_REQ`).

use std::path::{Path, PathBuf};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub(crate) fn gib(b: u64) -> f64 {
    b as f64 / GIB
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GpuAdapter {
    pub name: String,
    pub vram_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HwInfo {
    /// Adaptador que o wgpu abriu (nome, tipo, backend, driver).
    pub gpu_name: Option<String>,
    pub gpu_kind: Option<String>,
    pub backend: Option<String>,
    pub driver: Option<String>,
    /// Adaptadores de vídeo do registro, com a VRAM dedicada.
    pub adapters: Vec<GpuAdapter>,
    pub ram_total: Option<u64>,
    pub ram_free: Option<u64>,
    /// (pasta, espaço livre) — o disco do sistema e a pasta onde o motor opcional seria instalado.
    pub disks: Vec<(PathBuf, Option<u64>)>,
    pub probed: bool,
}

impl HwInfo {
    /// Guarda o que o wgpu disse do adaptador em uso (chamado pelo app quando a janela abre).
    pub(crate) fn set_gpu(&mut self, name: &str, kind: &str, backend: &str, driver: &str) {
        self.gpu_name = Some(name.to_owned());
        self.gpu_kind = Some(kind.to_owned());
        self.backend = Some(backend.to_owned());
        self.driver = (!driver.trim().is_empty()).then(|| driver.to_owned());
    }

    /// Lê registro, memória e discos (rápido; o painel chama uma vez e no botão "Atualizar").
    pub(crate) fn probe(&mut self) {
        self.adapters = video_adapters();
        let (t, f) = ram();
        self.ram_total = t;
        self.ram_free = f;
        let mut disks = vec![PathBuf::from("C:\\")];
        let dir = hq_install_dir();
        if !disks.iter().any(|d| same_drive(d, &dir)) {
            disks.push(dir);
        }
        self.disks = disks.into_iter().map(|d| {
            let free = aiai_core::persist::free_space(&d);
            (d, free)
        }).collect();
        self.probed = true;
    }

    /// VRAM do adaptador em uso: o do registro com o mesmo nome do wgpu; sem nome, o maior.
    pub(crate) fn vram_active(&self) -> Option<u64> {
        if let Some(n) = &self.gpu_name {
            let n = n.to_lowercase();
            if let Some(a) = self.adapters.iter().find(|a| {
                let m = a.name.to_lowercase();
                !m.is_empty() && (m.contains(&n) || n.contains(&m))
            }) {
                return a.vram_bytes;
            }
        }
        self.adapters.iter().filter_map(|a| a.vram_bytes).max()
    }

    /// Espaço livre no disco da pasta de instalação do motor opcional.
    pub(crate) fn install_free(&self) -> Option<u64> {
        let dir = hq_install_dir();
        self.disks.iter().find(|(d, _)| same_drive(d, &dir)).and_then(|(_, f)| *f)
    }
}

fn same_drive(a: &Path, b: &Path) -> bool {
    let root = |p: &Path| p.components().next().map(|c| c.as_os_str().to_string_lossy().to_uppercase());
    root(a) == root(b)
}

/// Pasta onde o motor opcional ficaria: `sidecars/style-bert-vits2` na raiz do repositório (ao lado de `sidecars/kokoro`).
pub(crate) fn hq_install_dir() -> PathBuf {
    aiai_audio::kokoro::find_script()
        .and_then(|s| s.parent().and_then(|d| d.parent()).and_then(|d| d.parent()).map(|r| r.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("C:\\AnimeIAI"))
        .join("sidecars")
        .join("style-bert-vits2")
}

/// Requisitos do motor opcional de alta qualidade (Style-Bert-VITS2 JP-Extra com PyTorch e um modelo de voz).
pub(crate) struct HqReq {
    pub vram_min: f64,
    pub vram_rec: f64,
    pub ram_min: f64,
    pub disk_need: f64,
}

pub(crate) const HQ_REQ: HqReq = HqReq { vram_min: 4.0, vram_rec: 12.0, ram_min: 8.0, disk_need: 15.0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// GPU com a VRAM recomendada, RAM e disco suficientes.
    Good,
    /// Roda, abaixo do recomendado (VRAM entre o mínimo e o recomendado).
    Fits,
    /// Só em CPU (lento): VRAM abaixo do mínimo ou desconhecida.
    CpuOnly,
    /// Falta disco ou RAM: não dá para instalar como está.
    Blocked,
}

impl Verdict {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Verdict::Good => "audio.tts.verdict.good",
            Verdict::Fits => "audio.tts.verdict.fits",
            Verdict::CpuOnly => "audio.tts.verdict.cpu",
            Verdict::Blocked => "audio.tts.verdict.blocked",
        }
    }
}

/// Motivo de um veredito: chave i18n + números (GB) para o texto.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reason {
    VramUnknown,
    VramLow(f64),
    VramBelowRec(f64),
    VramOk(f64),
    RamLow(f64),
    DiskLow(f64),
    DiskOk(f64),
}

pub(crate) fn hq_verdict(vram: Option<u64>, ram_total: Option<u64>, disk_free: Option<u64>, req: &HqReq) -> (Verdict, Vec<Reason>) {
    let mut reasons = Vec::new();
    let mut v = match vram.map(gib) {
        None => {
            reasons.push(Reason::VramUnknown);
            Verdict::CpuOnly
        }
        Some(g) if g < req.vram_min => {
            reasons.push(Reason::VramLow(g));
            Verdict::CpuOnly
        }
        Some(g) if g < req.vram_rec => {
            reasons.push(Reason::VramBelowRec(g));
            Verdict::Fits
        }
        Some(g) => {
            reasons.push(Reason::VramOk(g));
            Verdict::Good
        }
    };
    // um pente de "8 GB" aparece como ~7,9 GiB (memória reservada): 5 % de folga
    if let Some(r) = ram_total.map(gib).filter(|r| *r < req.ram_min * 0.95) {
        reasons.push(Reason::RamLow(r));
        v = Verdict::Blocked;
    }
    match disk_free.map(gib) {
        Some(d) if d < req.disk_need => {
            reasons.push(Reason::DiskLow(d));
            v = Verdict::Blocked;
        }
        Some(d) => reasons.push(Reason::DiskOk(d)),
        None => {}
    }
    (v, reasons)
}

// ------------------------------------------------------------------------------------------------ Windows

#[cfg(windows)]
fn ram() -> (Option<u64>, Option<u64>) {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page: u64,
        avail_page: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_ext_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buf: *mut MemoryStatusEx) -> i32;
    }
    let mut m = MemoryStatusEx { length: std::mem::size_of::<MemoryStatusEx>() as u32, memory_load: 0, total_phys: 0, avail_phys: 0, total_page: 0, avail_page: 0, total_virtual: 0, avail_virtual: 0, avail_ext_virtual: 0 };
    // SAFETY: estrutura com o tamanho certo em `length`, ponteiro para um local.
    let ok = unsafe { GlobalMemoryStatusEx(&mut m) };
    if ok != 0 { (Some(m.total_phys), Some(m.avail_phys)) } else { (None, None) }
}

#[cfg(not(windows))]
fn ram() -> (Option<u64>, Option<u64>) {
    (None, None)
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Adaptadores de vídeo da classe `{4d36e968-...}` do registro: `DriverDesc` e `HardwareInformation.qwMemorySize`
/// (QWORD, placas com mais de 4 GB) ou `HardwareInformation.MemorySize` (DWORD/binário, as antigas).
#[cfg(windows)]
fn video_adapters() -> Vec<GpuAdapter> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(hkey: isize, sub: *const u16, value: *const u16, flags: u32, ty: *mut u32, data: *mut core::ffi::c_void, len: *mut u32) -> i32;
    }
    const HKLM: isize = 0x8000_0002u32 as i32 as isize;
    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    const RRF_RT_ANY: u32 = 0x0000_ffff;
    let read = |sub: &[u16], name: &str, flags: u32, buf: &mut [u8]| -> Option<usize> {
        let n = wide(name);
        let mut len = buf.len() as u32;
        let mut ty = 0u32;
        // SAFETY: strings terminadas em zero, buffer com o tamanho informado em `len`.
        let r = unsafe { RegGetValueW(HKLM, sub.as_ptr(), n.as_ptr(), flags, &mut ty, buf.as_mut_ptr().cast(), &mut len) };
        (r == 0).then_some(len as usize)
    };
    let mut out: Vec<GpuAdapter> = Vec::new();
    for i in 0..16 {
        let sub = wide(&format!("SYSTEM\\CurrentControlSet\\Control\\Class\\{{4d36e968-e325-11ce-bfc1-08002be10318}}\\{i:04}"));
        let mut b = [0u8; 512];
        let Some(n) = read(&sub, "DriverDesc", RRF_RT_REG_SZ, &mut b) else { continue };
        let units: Vec<u16> = b[..n].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
        let name = String::from_utf16_lossy(&units).trim().to_owned();
        let mut m = [0u8; 16];
        let vram = match read(&sub, "HardwareInformation.qwMemorySize", RRF_RT_ANY, &mut m) {
            Some(8) => Some(u64::from_le_bytes(m[..8].try_into().unwrap_or([0; 8]))),
            _ => match read(&sub, "HardwareInformation.MemorySize", RRF_RT_ANY, &mut m) {
                Some(8) => Some(u64::from_le_bytes(m[..8].try_into().unwrap_or([0; 8]))),
                Some(4) => Some(u32::from_le_bytes(m[..4].try_into().unwrap_or([0; 4])) as u64),
                _ => None,
            },
        }
        .filter(|&v| v > 0);
        if !name.is_empty() && !out.iter().any(|a| a.name == name && a.vram_bytes == vram) {
            out.push(GpuAdapter { name, vram_bytes: vram });
        }
    }
    out
}

#[cfg(not(windows))]
fn video_adapters() -> Vec<GpuAdapter> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: u64 = 1024 * 1024 * 1024;

    #[test]
    fn verdict_follows_vram_ram_and_disk() {
        let r = &HQ_REQ;
        assert_eq!(hq_verdict(Some(16 * G), Some(32 * G), Some(100 * G), r).0, Verdict::Good);
        assert_eq!(hq_verdict(Some(6 * G), Some(16 * G), Some(100 * G), r).0, Verdict::Fits);
        let (v, why) = hq_verdict(Some(2 * G), Some(16 * G), Some(100 * G), r);
        assert_eq!(v, Verdict::CpuOnly);
        assert!(matches!(why[0], Reason::VramLow(g) if (g - 2.0).abs() < 1e-9));
        assert_eq!(hq_verdict(None, Some(16 * G), Some(100 * G), r).0, Verdict::CpuOnly);
        // disco cheio bloqueia mesmo com GPU boa (o C: desta máquina vive cheio)
        let (v, why) = hq_verdict(Some(24 * G), Some(32 * G), Some(3 * G), r);
        assert_eq!(v, Verdict::Blocked);
        assert!(why.iter().any(|w| matches!(w, Reason::DiskLow(_))));
        assert_eq!(hq_verdict(Some(24 * G), Some(4 * G), Some(100 * G), r).0, Verdict::Blocked);
        // 8 GB nominais (7,9 GiB vistos pelo sistema) não bloqueiam
        assert_eq!(hq_verdict(Some(16 * G), Some(79 * G / 10), Some(100 * G), r).0, Verdict::Good);
    }

    #[test]
    fn active_vram_matches_the_wgpu_adapter_by_name() {
        let mut h = HwInfo { adapters: vec![GpuAdapter { name: "Intel(R) UHD Graphics".into(), vram_bytes: Some(G) }, GpuAdapter { name: "NVIDIA GeForce RTX 3060".into(), vram_bytes: Some(12 * G) }], ..Default::default() };
        assert_eq!(h.vram_active(), Some(12 * G), "sem nome do wgpu: o maior");
        h.set_gpu("Intel(R) UHD Graphics", "IntegratedGpu", "Dx12", "");
        assert_eq!(h.vram_active(), Some(G));
        assert!(h.driver.is_none());
    }

    #[test]
    fn probe_reads_this_machine_without_failing() {
        let mut h = HwInfo::default();
        h.probe();
        assert!(h.probed);
        assert!(!h.disks.is_empty());
        #[cfg(windows)]
        assert!(h.ram_total.is_some_and(|t| t > G / 2), "RAM: {:?}", h.ram_total);
    }
}
