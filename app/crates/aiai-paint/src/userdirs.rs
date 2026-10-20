//! Pastas do usuário que o app mostra no Explorador: `Documentos\AnimeIAI\...` (Fontes, Pincéis). A pasta Documentos
//! do Windows é localizada e pode estar no OneDrive, então ela vem do sistema (`SHGetKnownFolderPath`), nunca de um
//! caminho montado à mão; fora do Windows vale `~/Documents`.

use std::path::PathBuf;

/// Pasta Documentos do usuário (a do sistema: segue OneDrive e o nome no idioma do Windows).
pub fn documents_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(p) = known_folder_documents() {
            return Some(p);
        }
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).filter(|v| !v.is_empty()).map(PathBuf::from)?;
    ["Documentos", "Documents"].iter().map(|n| home.join(n)).find(|p| p.is_dir()).or_else(|| Some(home.join("Documents")))
}

/// `Documentos\AnimeIAI`.
pub fn animeiai_dir() -> Option<PathBuf> {
    documents_dir().map(|d| d.join("AnimeIAI"))
}

#[cfg(windows)]
fn known_folder_documents() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    #[repr(C)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(id: *const Guid, flags: u32, token: *mut core::ffi::c_void, out: *mut *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(p: *mut core::ffi::c_void);
    }
    // FOLDERID_Documents
    let id = Guid { d1: 0xFDD3_9AD0, d2: 0x238F, d3: 0x46AF, d4: [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7] };
    let mut out: *mut u16 = core::ptr::null_mut();
    // SAFETY: chamada de sistema com GUID válido; o ponteiro devolvido é lido até o NUL e liberado com CoTaskMemFree.
    unsafe {
        let hr = SHGetKnownFolderPath(&id, 0, core::ptr::null_mut(), &mut out);
        if hr != 0 || out.is_null() {
            if !out.is_null() {
                CoTaskMemFree(out.cast());
            }
            return None;
        }
        let mut n = 0usize;
        while *out.add(n) != 0 {
            n += 1;
        }
        let p = PathBuf::from(std::ffi::OsString::from_wide(core::slice::from_raw_parts(out, n)));
        CoTaskMemFree(out.cast());
        (!p.as_os_str().is_empty()).then_some(p)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn documents_dir_exists_on_this_machine() {
        let d = super::documents_dir().expect("pasta Documentos");
        assert!(d.is_absolute(), "{d:?}");
    }
}
