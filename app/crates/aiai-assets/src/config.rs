//! Onde fica a biblioteca. A raiz é `Documentos\AnimeIAI` (a Documentos do sistema, OneDrive incluído) com uma pasta por
//! tipo; `AIAI_LIBRARY_DIR` troca a raiz (testes e capturas de tela). Sem sessão (`--no-session`) e sem a variável
//! nada é lido nem gravado: `root()` devolve `None`.

use crate::kind::Kind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static USER_FOLDERS: AtomicBool = AtomicBool::new(false);
static CHANGES: AtomicU64 = AtomicU64::new(0);

/// Liga ou desliga o uso das pastas do usuário em `Documentos` (o app liga fora do `--no-session`; o MCP em modo
/// headless liga; os testes não ligam e portanto nunca tocam em `Documentos`).
pub fn enable_user_folders(on: bool) {
    USER_FOLDERS.store(on, Ordering::SeqCst);
}

/// Raiz da biblioteca ou `None` (biblioteca desligada).
pub fn root() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("AIAI_LIBRARY_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    if USER_FOLDERS.load(Ordering::SeqCst) { aiai_paint::userdirs::animeiai_dir() } else { None }
}

/// Pasta de um tipo dentro da raiz.
pub fn kind_dir(root: &Path, kind: Kind) -> PathBuf {
    root.join(kind.folder())
}

/// Muda a cada gravação na biblioteca (por qualquer caminho: painel, bridge, MCP); o painel relê a pasta quando vê o
/// número mudar.
pub fn change_counter() -> u64 {
    CHANGES.load(Ordering::SeqCst)
}

pub fn bump() {
    CHANGES.fetch_add(1, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_and_override_by_env() {
        // os testes do crate não ligam as pastas do usuário: sem a variável, a biblioteca está desligada
        if std::env::var_os("AIAI_LIBRARY_DIR").is_none() {
            assert!(root().is_none());
        }
        assert_eq!(kind_dir(Path::new("x"), Kind::Model3d), Path::new("x").join("Modelos 3D"));
        let n = change_counter();
        bump();
        assert_eq!(change_counter(), n + 1);
    }
}
