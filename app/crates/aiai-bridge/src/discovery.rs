//! Arquivo de descoberta da bridge: porta efêmera + token por sessão, com permissões restritas.
//!
//! Local (ver `docs/native/pesquisa/mcp_bridge_nativa.md`, D7):
//! * `AIAI_BRIDGE_DISCOVERY` (arquivo) sobrescreve tudo;
//! * Windows: `%LOCALAPPDATA%\AnimeIAI\bridge\bridge.json` (por usuário, fora de pastas sincronizadas);
//! * Linux/macOS: `$XDG_RUNTIME_DIR/animeiai/bridge.json` (tmpfs 0700 do usuário), senão
//!   `~/.local/share/animeiai/bridge/bridge.json`, senão `<tmp>/aiai-bridge/bridge.json`.
//!
//! Escrita atômica: arquivo temporário criado com `create_new` + modo 0600 (Unix) e `rename`.
//! Leitura: em Unix recusa arquivo legível por grupo/outros.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const MAX_DISCOVERY_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Discovery {
    /// `127.0.0.1:<porta>`
    pub addr: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub started_at: u64,
    pub version: String,
    pub protocol: u32,
}

/// Caminho padrão do arquivo de descoberta (não cria nada).
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("AIAI_BRIDGE_DISCOVERY").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    resolve_default(
        std::env::var_os("LOCALAPPDATA"),
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
        cfg!(windows),
    )
}

fn non_empty(v: Option<std::ffi::OsString>) -> Option<PathBuf> {
    v.filter(|p| !p.is_empty()).map(PathBuf::from)
}

fn resolve_default(
    localappdata: Option<std::ffi::OsString>,
    xdg_runtime: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    windows: bool,
) -> PathBuf {
    if windows {
        if let Some(base) = non_empty(localappdata) {
            return base.join("AnimeIAI").join("bridge").join("bridge.json");
        }
    } else if let Some(rt) = non_empty(xdg_runtime).filter(|p| p.is_absolute() || p.has_root()) {
        return rt.join("animeiai").join("bridge.json");
    }
    if let Some(home) = non_empty(home) {
        return home.join(".local").join("share").join("animeiai").join("bridge").join("bridge.json");
    }
    std::env::temp_dir().join("aiai-bridge").join("bridge.json")
}

/// Grava o arquivo de descoberta de forma atômica e com permissão restrita.
pub fn write(path: &Path, d: &Discovery) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    create_private_dir(dir)?;
    let tmp = dir.join(format!(
        "{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("bridge.json"),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp); // sobra de um processo com o mesmo pid
    let body = serde_json::to_vec_pretty(d).map_err(io::Error::other)?;
    let result = (|| {
        let mut f = open_private_new(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
        drop(f);
        // Windows: tira a ACL herdada (SYSTEM, Administradores, outras contas do perfil) e deixa só o usuário atual.
        // Melhor esforço: se falhar, vale a ACL por usuário de %LOCALAPPDATA% (a que o arquivo já herdou).
        #[cfg(windows)]
        let _ = restrict_to_current_user(&tmp);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Lê (e valida) o arquivo de descoberta.
pub fn read(path: &Path) -> io::Result<Discovery> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_DISCOVERY_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "arquivo de descoberta inválido"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} é legível por outros usuários (esperado 0600); recusado", path.display()),
            ));
        }
    }
    let mut raw = String::new();
    std::fs::File::open(path)?.take(MAX_DISCOVERY_BYTES).read_to_string(&mut raw)?;
    let d: Discovery = serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    if d.token.is_empty() || !d.addr.starts_with("127.0.0.1:") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "descoberta sem token ou fora do loopback"));
    }
    Ok(d)
}

/// Remove o arquivo só se ainda for desta instância (outra instância pode tê-lo sobrescrito).
pub fn remove_if_owner(path: &Path, pid: u32, token: &str) {
    if let Ok(d) = read(path) {
        if d.pid == pid && d.token == token {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Windows: `icacls /inheritance:r` + concede controle total SÓ ao SID do usuário atual (o rename preserva a ACL).
#[cfg(windows)]
fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    use std::process::{Command, Stdio};
    let who = Command::new("whoami").args(["/user", "/fo", "csv", "/nh"]).stderr(Stdio::null()).output()?;
    let text = String::from_utf8_lossy(&who.stdout);
    // "DOMINIO\usuario","S-1-5-21-..."
    let sid = text.trim().rsplit(',').next().map(|s| s.trim().trim_matches('"')).filter(|s| s.starts_with("S-1-")).ok_or_else(|| io::Error::other("SID do usuário não encontrado"))?;
    let status = Command::new("icacls").arg(path).args(["/inheritance:r", "/grant:r", &format!("*{sid}:F")]).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("icacls falhou"))
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn open_private_new(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn open_private_new(path: &Path) -> io::Result<std::fs::File> {
    // Windows: sem `mode`; o arquivo herda a ACL da pasta (por usuário em %LOCALAPPDATA%).
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

