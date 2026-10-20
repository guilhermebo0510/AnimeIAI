//! Trava "projeto aberto em outro lugar" da pasta `.aiai`: `project.lock` com pid, máquina, início e um token desta
//! abertura. Duas instâncias (app + MCP headless) anexando ao mesmo `wal.ndjson` intercalam o diário e quebram a
//! recuperação; a trava avisa antes. É de cortesia (pasta de rede/nuvem não garante nada) e quem decide assumir é o
//! usuário. Trava de processo que morreu (queda) é "velha" e é substituída sem perguntar.

use crate::error::{CoreError, CoreResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const LOCK_FILE: &str = "project.lock";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub host: String,
    /// Início desta abertura (ms desde a época Unix).
    pub started_ms: u64,
    #[serde(default)]
    pub exe: String,
    /// Identifica a abertura (um processo pode reabrir a mesma pasta; só quem gravou o token apaga o arquivo).
    #[serde(default)]
    pub token: String,
}

impl LockInfo {
    fn mine() -> Self {
        let started_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        LockInfo { pid: std::process::id(), host: host_name(), started_ms, exe, token: uuid::Uuid::now_v7().to_string() }
    }

    /// Texto curto para mensagens: `PID 1234 em PC-SALA (aiai-studio.exe), desde 2026-10-19 14:03 UTC`.
    pub fn describe(&self) -> String {
        let (y, mo, d, h, mi, _) = crate::persist::utc_parts(self.started_ms);
        let exe = if self.exe.is_empty() { String::new() } else { format!(" ({})", self.exe) };
        format!("PID {} em {}{exe}, desde {y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC", self.pid, if self.host.is_empty() { "?" } else { &self.host })
    }
}

/// Nome desta máquina (a trava de outra máquina não dá para conferir pelo pid).
pub fn host_name() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq)]
pub enum LockState {
    Free,
    /// Deste processo (outra sessão dele ou reabertura).
    Mine(LockInfo),
    /// Processo morto (ou pid reaproveitado por outro programa): sobra de uma queda.
    Stale(LockInfo),
    /// Outro processo vivo nesta máquina, ou outra máquina (não dá para conferir: vale como viva).
    Held(LockInfo),
}

/// Lê a trava da pasta. Ausente = `None`; ilegível (gravação interrompida, edição à mão) = trava sem dono (pid 0), que
/// [`inspect`] trata como velha.
pub fn read(dir: &Path) -> CoreResult<Option<LockInfo>> {
    match crate::persist::retry_io(|| std::fs::read(dir.join(LOCK_FILE))) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).unwrap_or_default())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::Io(format!("{}: {e}", dir.join(LOCK_FILE).display()))),
    }
}

pub fn inspect(dir: &Path) -> CoreResult<LockState> {
    let Some(info) = read(dir)? else { return Ok(LockState::Free) };
    if info.pid == 0 {
        return Ok(LockState::Stale(info));
    }
    if info.host != host_name() {
        return Ok(LockState::Held(info));
    }
    if info.pid == std::process::id() {
        return Ok(LockState::Mine(info));
    }
    Ok(if process_alive(info.pid, info.started_ms) { LockState::Held(info) } else { LockState::Stale(info) })
}

/// Trava gravada por esta sessão. Soltar (drop) apaga o arquivo se ele ainda é desta abertura (o token confere):
/// reabrir a mesma pasta no mesmo processo grava um token novo, e a trava antiga, ao sair, não apaga a nova.
#[derive(Debug)]
pub struct ProjectLock {
    dir: PathBuf,
    info: LockInfo,
}

impl ProjectLock {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// Antes de gravar: a trava ainda é desta sessão (ou deste processo)? Outro processo assumiu = erro (a gravação não
    /// acontece). Arquivo sumido (apagado por fora) = grava a nossa de novo.
    pub fn verify(&self) -> CoreResult<()> {
        match read(&self.dir)? {
            Some(i) if i.token == self.info.token || (i.pid == std::process::id() && i.host == host_name()) => Ok(()),
            None => write_lock(&self.dir, &self.info),
            Some(i) => Err(CoreError::Locked(format!("outro processo assumiu o projeto ({})", i.describe()))),
        }
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        // sem como devolver erro aqui: se não der para apagar, a próxima abertura vê o pid morto e trata como velha
        if read(&self.dir).ok().flatten().is_some_and(|i| i.token == self.info.token) {
            let _ = std::fs::remove_file(self.dir.join(LOCK_FILE));
        }
    }
}

pub struct Acquired {
    pub lock: ProjectLock,
    /// Trava anterior substituída: velha (processo morto) ou assumida de um processo vivo (`take_over`).
    pub replaced: Option<LockInfo>,
}

/// Trava a pasta para esta sessão. Livre, deste processo ou velha: grava a nossa. De outro processo vivo: devolve
/// `Err(dono)` sem tocar em nada, a menos que `take_over` (o usuário escolheu assumir).
pub fn acquire(dir: &Path, take_over: bool) -> CoreResult<Result<Acquired, LockInfo>> {
    let replaced = match inspect(dir)? {
        LockState::Free | LockState::Mine(_) => None,
        LockState::Stale(i) => Some(i),
        LockState::Held(i) if take_over => Some(i),
        LockState::Held(i) => return Ok(Err(i)),
    };
    let info = LockInfo::mine();
    write_lock(dir, &info)?;
    Ok(Ok(Acquired { lock: ProjectLock { dir: dir.to_path_buf(), info }, replaced }))
}

fn write_lock(dir: &Path, info: &LockInfo) -> CoreResult<()> {
    let body = serde_json::to_string_pretty(info)?;
    crate::persist::write_file_atomic(&dir.join(LOCK_FILE), body.as_bytes())
}

/// O processo `pid` existe e é o mesmo que gravou a trava em `started_ms`?
#[cfg(windows)]
fn process_alive(pid: u32, started_ms: u64) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(h: *mut c_void, code: *mut u32) -> i32;
        fn GetProcessTimes(h: *mut c_void, creation: *mut u64, exit: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
        fn CloseHandle(h: *mut c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    // SAFETY: chamadas Win32 com ponteiros para locais; o handle é fechado antes de sair.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            // acesso negado = existe (de outro usuário/elevado); parâmetro inválido = não existe
            return std::io::Error::last_os_error().raw_os_error() == Some(5);
        }
        let mut code = 0u32;
        let running = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE;
        let (mut c, mut e, mut k, mut u) = (0u64, 0u64, 0u64, 0u64);
        // FILETIME (100 ns desde 1601) -> ms desde 1970
        let created_ms = (GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0).then(|| c.saturating_sub(116_444_736_000_000_000) / 10_000);
        CloseHandle(h);
        // pid reaproveitado: o processo com esse número nasceu depois da trava
        running && !created_ms.is_some_and(|c| c > started_ms + 2_000)
    }
}

#[cfg(not(windows))]
fn process_alive(pid: u32, _started_ms: u64) -> bool {
    if cfg!(target_os = "linux") { Path::new(&format!("/proc/{pid}")).exists() } else { true }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-lock-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, info: &LockInfo) {
        std::fs::write(dir.join(LOCK_FILE), serde_json::to_string(info).unwrap()).unwrap();
    }

    #[test]
    fn stale_lock_of_dead_process_is_replaced_and_removed_on_drop() {
        let dir = tmp("stale");
        // pid que não existe (os pids do Windows são múltiplos de 4; este é enorme e ímpar)
        let dead = LockInfo { pid: 0x7fff_fff1, host: host_name(), started_ms: 1, exe: "x.exe".into(), token: "velho".into() };
        put(&dir, &dead);
        assert_eq!(inspect(&dir).unwrap(), LockState::Stale(dead.clone()));
        let got = acquire(&dir, false).unwrap().expect("velha não bloqueia");
        assert_eq!(got.replaced, Some(dead));
        assert_eq!(read(&dir).unwrap().unwrap().pid, std::process::id());
        got.lock.verify().unwrap();
        drop(got);
        assert!(!dir.join(LOCK_FILE).exists(), "fechar limpo apaga a trava");
        // ilegível também é velha
        std::fs::write(dir.join(LOCK_FILE), b"{\"pid\": 12").unwrap();
        assert!(matches!(inspect(&dir).unwrap(), LockState::Stale(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn live_lock_of_other_process_blocks_until_take_over() {
        let dir = tmp("live");
        // um processo vivo de verdade, com pid diferente do nosso
        let other = live_other_pid();
        let held = LockInfo { pid: other, host: host_name(), started_ms: u64::MAX / 4, exe: "aiai-mcp.exe".into(), token: "outro".into() };
        put(&dir, &held);
        assert_eq!(inspect(&dir).unwrap(), LockState::Held(held.clone()));
        assert_eq!(acquire(&dir, false).unwrap().err(), Some(held.clone()), "viva: não toca no arquivo");
        assert_eq!(read(&dir).unwrap().unwrap().token, "outro");
        // outra máquina: não dá para conferir o pid, vale como viva
        let remote = LockInfo { host: "OUTRA-MAQUINA".into(), pid: 4, ..held.clone() };
        put(&dir, &remote);
        assert!(matches!(inspect(&dir).unwrap(), LockState::Held(_)));
        // assumir: grava a nossa e devolve quem estava
        put(&dir, &held);
        let got = acquire(&dir, true).unwrap().expect("assumir sempre consegue");
        assert_eq!(got.replaced, Some(held.clone()));
        got.lock.verify().unwrap();
        // o outro processo assume de volta: ao conferir antes de gravar, esta sessão descobre que perdeu a trava
        put(&dir, &held);
        let err = got.lock.verify().unwrap_err();
        assert!(matches!(err, CoreError::Locked(_)), "{err}");
        drop(got);
        assert_eq!(read(&dir).unwrap().unwrap().token, "outro", "soltar a nossa não apaga a trava de outro");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Pid de um processo vivo que não é este: um filho que fica esperando enquanto o teste roda.
    fn live_other_pid() -> u32 {
        use std::sync::OnceLock;
        static CHILD: OnceLock<u32> = OnceLock::new();
        *CHILD.get_or_init(|| {
            let cmd = if cfg!(windows) { ("cmd", vec!["/C", "ping -n 30 127.0.0.1 >NUL"]) } else { ("sleep", vec!["30"]) };
            let child = std::process::Command::new(cmd.0).args(cmd.1).stdout(std::process::Stdio::null()).spawn().expect("processo filho");
            let pid = child.id();
            std::mem::forget(child);
            pid
        })
    }
}
