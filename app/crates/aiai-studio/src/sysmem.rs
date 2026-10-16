//! Memória residente do processo (barra de status).

/// Working set do processo em bytes (Windows: `GetProcessMemoryInfo`). `None` em outras plataformas ou em falha.
#[cfg(windows)]
pub fn process_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: estrutura própria zerada com `cb` correto; o pseudo-handle do processo atual não precisa ser fechado.
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            Some(c.WorkingSetSize as u64)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
pub fn process_memory_bytes() -> Option<u64> {
    None
}

/// "184 MB" / "1.5 GB".
pub fn format_bytes(b: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = b as f64 / MB;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(184 * 1024 * 1024), "184 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
    }

    #[cfg(windows)]
    #[test]
    fn reads_own_memory() {
        let m = process_memory_bytes().expect("GetProcessMemoryInfo");
        assert!(m > 1024 * 1024, "{m}");
    }
}
