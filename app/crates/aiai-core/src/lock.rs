//! Trava "projeto aberto em outro lugar" da pasta `.aiai`: `project.lock` com pid, máquina, início e um token desta
//! abertura. Duas instâncias (app + MCP headless) anexando ao mesmo `wal.ndjson` intercalam o diário e quebram a
//! recuperação; a trava avisa antes. É de cortesia (pasta de rede/nuvem não garante nada) e quem decide assumir é o
//! usuário. Trava de processo que morreu (queda) é "velha" e é substituída sem perguntar.
//!
//! Pegar a trava é "conferir se está livre e gravar a nossa"; entre processos desta máquina essas duas etapas correm
//! sob uma exclusão do sistema ([`AcqGuard`]), senão dois processos que conferem ao mesmo tempo achariam a trava livre e
//! os dois ganhariam (CX15 C15-01).

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
    read_file(&dir.join(LOCK_FILE))
}

/// Como [`read`], com o caminho do arquivo da trava (trava de corte: `cuts/<corte>.lock`).
pub fn read_file(path: &Path) -> CoreResult<Option<LockInfo>> {
    match crate::persist::retry_io(|| std::fs::read(path)) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).unwrap_or_default())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::Io(format!("{}: {e}", path.display()))),
    }
}

pub fn inspect(dir: &Path) -> CoreResult<LockState> {
    inspect_file(&dir.join(LOCK_FILE))
}

pub fn inspect_file(path: &Path) -> CoreResult<LockState> {
    let Some(info) = read_file(path)? else { return Ok(LockState::Free) };
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
    /// O arquivo da trava (`project.lock` da pasta, ou `cuts/<corte>.lock`).
    file: PathBuf,
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
        match read_file(&self.file)? {
            Some(i) if i.token == self.info.token || (i.pid == std::process::id() && i.host == host_name()) => Ok(()),
            None => {
                // apagada por fora: regrava a nossa só se, sob a exclusão, ninguém pegou nesse meio-tempo
                let _g = AcqGuard::take(&self.file)?;
                match read_file(&self.file)? {
                    None => write_lock(&self.file, &self.info),
                    Some(i) if i.token == self.info.token => Ok(()),
                    Some(i) => Err(CoreError::Locked(format!("outro processo assumiu o projeto ({})", i.describe()))),
                }
            }
            Some(i) => Err(CoreError::Locked(format!("outro processo assumiu o projeto ({})", i.describe()))),
        }
    }
}

/// Exclusão entre processos só enquanto se confere e grava uma trava: trava do sistema (`LockFileEx` no Windows,
/// `flock` nos outros) num arquivo irmão `<trava>.acq`. O sistema a solta sozinho se o processo morrer, então ela nunca
/// fica velha. O arquivo `.acq` fica na pasta: apagá-lo com outro processo esperando nele quebraria a exclusão.
struct AcqGuard(std::fs::File);

/// Quanto esperar por outro processo que está pegando a mesma trava (ele a segura por milissegundos).
const ACQ_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

impl AcqGuard {
    fn take(file: &Path) -> CoreResult<Self> {
        let mut name = file.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".acq");
        let path = file.with_file_name(name);
        let f = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(&path).map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
        let t0 = std::time::Instant::now();
        loop {
            match f.try_lock() {
                Ok(()) => return Ok(AcqGuard(f)),
                Err(std::fs::TryLockError::WouldBlock) if t0.elapsed() < ACQ_WAIT => std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(CoreError::Locked(format!("outro processo está pegando a trava {} há mais de {} s; tente de novo", file.display(), ACQ_WAIT.as_secs())));
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
            }
        }
    }
}

impl Drop for AcqGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        // sem como devolver erro aqui: se não der para apagar, a próxima abertura vê o pid morto e trata como velha. Sob a
        // exclusão, para não apagar a trava que outro acabou de gravar entre a leitura e a remoção.
        let _g = AcqGuard::take(&self.file).ok();
        if read_file(&self.file).ok().flatten().is_some_and(|i| i.token == self.info.token) {
            let _ = std::fs::remove_file(&self.file);
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
    acquire_file(dir, &dir.join(LOCK_FILE), take_over)
}

fn acquire_file(dir: &Path, file: &Path, take_over: bool) -> CoreResult<Result<Acquired, LockInfo>> {
    // conferir e gravar sob a exclusão: quem chega enquanto outro confere espera e depois vê a trava dele
    let _g = AcqGuard::take(file)?;
    let state = inspect_file(file)?;
    #[cfg(test)]
    tests::pause_after_inspect();
    let replaced = match state {
        LockState::Free | LockState::Mine(_) => None,
        LockState::Stale(i) => Some(i),
        LockState::Held(i) if take_over => Some(i),
        LockState::Held(i) => return Ok(Err(i)),
    };
    let info = LockInfo::mine();
    write_lock(file, &info)?;
    Ok(Ok(Acquired { lock: ProjectLock { dir: dir.to_path_buf(), file: file.to_path_buf(), info }, replaced }))
}

/// Pasta das travas e dos arquivos de corte (`cuts/`).
pub const CUTS_DIR: &str = "cuts";

/// Nome de arquivo seguro para o id do corte (o id é texto; só letras, números, `-` e `_` passam).
pub fn cut_file_stem(cut: &crate::Id) -> String {
    cut.0.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

/// Arquivo da trava de um corte (auditoria 4.6k): `cuts/<corte>.lock`.
pub fn cut_lock_path(dir: &Path, cut: &crate::Id) -> PathBuf {
    dir.join(CUTS_DIR).join(format!("{}.lock", cut_file_stem(cut)))
}

/// Trava um corte para esta sessão (quem edita aquele corte agora). Mesmas regras da trava da pasta: de outro
/// processo vivo = `Err(dono)`.
pub fn acquire_cut(dir: &Path, cut: &crate::Id) -> CoreResult<Result<Acquired, LockInfo>> {
    std::fs::create_dir_all(dir.join(CUTS_DIR))?;
    acquire_file(dir, &cut_lock_path(dir, cut), false)
}

/// Estado da trava de um corte.
pub fn inspect_cut(dir: &Path, cut: &crate::Id) -> CoreResult<LockState> {
    inspect_file(&cut_lock_path(dir, cut))
}

fn write_lock(file: &Path, info: &LockInfo) -> CoreResult<()> {
    let body = serde_json::to_string_pretty(info)?;
    crate::persist::write_file_atomic(file, body.as_bytes())
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

    fn wait_for(p: &Path) {
        let t0 = std::time::Instant::now();
        while !p.exists() {
            assert!(t0.elapsed() < std::time::Duration::from_secs(60), "esperando {}", p.display());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Gancho de teste em `acquire_file`: no processo filho da disputa (variável `AIAI_LOCK_SYNC`), a primeira aquisição
    /// para logo depois de conferir a trava (ainda sem gravar) até o teste mandar seguir.
    pub(super) fn pause_after_inspect() {
        static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let Ok(sync) = std::env::var("AIAI_LOCK_SYNC") else { return };
        if ONCE.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let sync = PathBuf::from(sync);
        std::fs::write(sync.join("inspected"), b"").unwrap();
        wait_for(&sync.join("go"));
    }

    /// O outro processo da disputa (só roda chamado por `two_processes_racing_for_a_free_lock_only_one_wins`).
    #[test]
    #[ignore]
    fn lock_contender_child() {
        let Ok(sync) = std::env::var("AIAI_LOCK_SYNC") else { return };
        let sync = PathBuf::from(sync);
        let dir = PathBuf::from(std::env::var("AIAI_LOCK_DIR").unwrap());
        let what = std::env::var("AIAI_LOCK_WHAT").unwrap();
        let got = if what == "folder" { acquire(&dir, false) } else { acquire_cut(&dir, &crate::Id(what)) };
        let got = got.unwrap().expect("o filho conferiu primeiro: a trava estava livre");
        std::fs::write(sync.join("held"), &got.lock.info().token).unwrap();
        wait_for(&sync.join("release"));
        drop(got);
        std::fs::write(sync.join("released"), b"").unwrap();
    }

    /// CX15 C15-01: dois processos disputam a mesma trava livre, sincronizados na aquisição: o filho confere (livre) e
    /// para antes de gravar; este processo tenta pegar a mesma trava nesse instante. Só um ganha: o segundo espera, vê a
    /// trava do primeiro e é recusado; pela sessão, a recusa não deixa mudança no projeto, no log, no histórico nem no
    /// diário. Solta a primeira, a posse passa ao segundo. Vale para a trava da pasta e para a de um corte.
    #[test]
    fn two_processes_racing_for_a_free_lock_only_one_wins() {
        use crate::command::Command;
        use crate::model::*;
        for what in ["folder", "cut"] {
            let dir = tmp(&format!("race-{what}"));
            let mut s = crate::session::CoreSession::for_tests("t");
            let mk = |s: &mut crate::session::CoreSession, parent: Option<crate::Id>, name: &str, data: NodeKind| {
                s.apply(Command::CreateNode { id: None, parent, index: None, name: name.into(), data }).unwrap().created[0].clone()
            };
            let ep = mk(&mut s, None, "EP", NodeKind::Episode(EpisodeData { number: 1 }));
            let sc = mk(&mut s, Some(ep), "S", NodeKind::Scene(SceneData::default()));
            let cut = mk(&mut s, Some(sc), "C001", NodeKind::Cut(CutData::new(24)));
            let layer = mk(&mut s, Some(cut.clone()), "A", NodeKind::Layer(LayerData::new(LayerType::Vector)));
            let cel = mk(&mut s, Some(layer), "A1", NodeKind::Cel(CelData::new(1)));
            s.apply(Command::SetProjectProps { patch: serde_json::json!({"settings": {"split_cuts": true}}) }).unwrap();
            if what == "cut" {
                // projeto dividido salvo: esta sessão tem a pasta; o filho disputa a trava do corte
                crate::persist::save(&mut s, &dir).unwrap();
            }
            let sync = dir.join("sync");
            std::fs::create_dir_all(&sync).unwrap();
            let target = if what == "folder" { "folder".to_string() } else { cut.0.clone() };
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["lock::tests::lock_contender_child", "--exact", "--ignored", "--nocapture", "--test-threads=1"])
                .env("AIAI_LOCK_SYNC", &sync)
                .env("AIAI_LOCK_DIR", &dir)
                .env("AIAI_LOCK_WHAT", &target)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            wait_for(&sync.join("inspected"));
            // o filho viu a trava livre e está parado antes de gravar: este processo tenta agora
            let wal = dir.join(crate::persist::WAL_FILE);
            let state = move |s: &crate::session::CoreSession| (serde_json::to_string(&s.project).unwrap(), s.log().len(), s.history(), std::fs::read(&wal).ok());
            let before = state(&s);
            let d2 = dir.clone();
            let cel2 = cel.clone();
            let contender = std::thread::spawn(move || {
                // Some(texto do dono) = recusado; None = ganhou
                let r: Option<String> = if what == "folder" {
                    acquire(&d2, false).unwrap().err().map(|i| format!("pid {}", i.pid))
                } else {
                    match s.apply(Command::SetProps { id: cel2, name: Some("disputa".into()), data: serde_json::json!({}) }) {
                        Err(CoreError::Locked(m)) => Some(m),
                        Err(e) => panic!("{e}"),
                        Ok(_) => None,
                    }
                };
                (r, s)
            });
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(!contender.is_finished(), "{what}: quem chega enquanto o outro confere espera a vez");
            std::fs::write(sync.join("go"), b"").unwrap();
            let (r, mut s) = contender.join().unwrap();
            wait_for(&sync.join("held"));
            let held = std::fs::read_to_string(sync.join("held")).unwrap();
            let refused = r.unwrap_or_else(|| panic!("{what}: os dois processos ganharam a mesma trava"));
            let file = if what == "folder" { dir.join(LOCK_FILE) } else { cut_lock_path(&dir, &cut) };
            assert_eq!(read_file(&file).unwrap().unwrap().token, held, "{what}: a trava continua do primeiro");
            if what == "folder" {
                assert_eq!(refused, format!("pid {}", child.id()));
            } else {
                assert!(refused.contains("outro lugar"), "{refused}");
                assert_eq!(state(&s), before, "a recusa não deixa mudança no projeto, no log, no histórico nem no diário");
            }
            // o primeiro solta: a posse passa ao segundo
            std::fs::write(sync.join("release"), b"").unwrap();
            wait_for(&sync.join("released"));
            assert!(child.wait().unwrap().success());
            if what == "folder" {
                let got = acquire(&dir, false).unwrap().expect("solta, a trava passa ao segundo");
                drop(got);
            } else {
                s.apply(Command::SetProps { id: cel, name: Some("depois".into()), data: serde_json::json!({}) }).unwrap();
            }
            drop(s);
            let _ = std::fs::remove_dir_all(&dir);
        }
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
