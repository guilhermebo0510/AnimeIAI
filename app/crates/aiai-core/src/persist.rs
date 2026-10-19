//! Persistência em pasta `.aiai`:
//!
//! ```text
//! meu-projeto.aiai/
//!   project.json      documento canônico (chaves ordenadas, indentado: diffs legíveis em git)
//!   blobs/ab/<hash>   conteúdo por BLAKE3 (tiles, máscaras, imagens, áudio)
//!   wal.ndjson        comandos aplicados depois do último salvamento (recuperação após queda)
//! ```
//!
//! Salvar é atômico (arquivo temporário + renomear) e confere espaço livre antes (o disco desta máquina vive cheio e
//! escrita interrompida já truncou arquivo — ver lições).

use crate::blob::{BlobHash, BlobStore};
use crate::command::Command;
use crate::error::{CoreError, CoreResult};
use crate::model::Project;
use crate::session::{CoreSession, LogEntry};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const WAL_FILE: &str = "wal.ndjson";
pub const BLOB_DIR: &str = "blobs";

/// Espaço mínimo livre para aceitar salvar (bytes).
pub const MIN_FREE_BYTES: u64 = 512 * 1024 * 1024;

/// JSON canônico do projeto (o `serde_json` sem `preserve_order` ordena chaves de mapas; structs têm ordem fixa).
pub fn to_canonical_json(project: &Project) -> CoreResult<String> {
    let v = serde_json::to_value(project)?;
    let mut s = serde_json::to_string_pretty(&v)?;
    s.push('\n');
    Ok(s)
}

pub fn blob_path(dir: &Path, h: &BlobHash) -> PathBuf {
    dir.join(BLOB_DIR).join(&h.0[..2]).join(&h.0)
}

/// Repete uma operação de arquivo que falhou por bloqueio passageiro de outro processo (antivírus, indexador, visualizador
/// que mapeou o arquivo): no Windows, compartilhamento (32), trava (33), seção mapeada pelo usuário (1224) e acesso negado
/// durante a troca (5). Até ~1,5 s no total; outros erros voltam na hora.
pub(crate) fn retry_io<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut attempt = 0u64;
    loop {
        match op() {
            Err(e) if attempt < 10 && matches!(e.raw_os_error(), Some(5 | 32 | 33 | 1224)) => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(25 * attempt));
            }
            other => return other,
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    write_atomic_with(path, bytes, true)
}

/// `durable` = `sync_all` antes da troca (sobrevive a queda de energia). Sem ele o arquivo continua atômico e
/// sobrevive ao app travar (o sistema já tem os dados), só não a desligar o PC no meio.
fn write_atomic_with(path: &Path, bytes: &[u8], durable: bool) -> CoreResult<()> {
    let tmp = path.with_extension("tmp-write");
    {
        let mut f = retry_io(|| std::fs::File::create(&tmp))?;
        f.write_all(bytes)?;
        if durable {
            f.sync_all()?;
        }
    }
    // Confere que o temporário ficou inteiro antes de substituir o original.
    let written = std::fs::metadata(&tmp)?.len();
    if written != bytes.len() as u64 {
        let _ = std::fs::remove_file(&tmp);
        return Err(CoreError::Io(format!("escrita incompleta em {} ({written} de {} bytes)", tmp.display(), bytes.len())));
    }
    retry_io(|| std::fs::rename(&tmp, path))?;
    Ok(())
}

#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: ponteiros válidos para u64 locais e string terminada em zero.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some(avail)
}

#[cfg(not(windows))]
pub fn free_space(_path: &Path) -> Option<u64> {
    None
}

fn ensure_space(dir: &Path, extra: u64) -> CoreResult<()> {
    if let Some(free) = free_space(dir) {
        if free < MIN_FREE_BYTES + extra {
            return Err(CoreError::Io(format!(
                "espaço livre insuficiente para salvar ({} MB livres)",
                free / (1024 * 1024)
            )));
        }
    }
    Ok(())
}

fn write_blob(dir: &Path, h: &BlobHash, bytes: &[u8], durable: bool) -> CoreResult<()> {
    let path = blob_path(dir, h);
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("tem pai"))?;
    write_atomic_with(&path, bytes, durable)
}

/// Grava vários blobs duráveis em paralelo: o custo do salvamento é o `sync_all` de cada arquivo (dezenas de ms no
/// Windows com antivírus), e em série ~200 tiles deixavam a interface parada por segundos.
fn write_blobs_parallel(dir: &Path, items: &[(BlobHash, std::sync::Arc<Vec<u8>>)]) -> CoreResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 16);
    let chunk = items.len().div_ceil(threads);
    std::thread::scope(|sc| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                sc.spawn(move || -> CoreResult<()> {
                    for (h, b) in part {
                        write_blob(dir, h, b, true)?;
                    }
                    Ok(())
                })
            })
            .collect();
        for hd in handles {
            hd.join().map_err(|_| CoreError::Io("gravação de blob entrou em pânico".into()))??;
        }
        Ok(())
    })
}

/// Salva o projeto da sessão em `dir` (cria se preciso) e zera o WAL.
pub fn save(session: &mut CoreSession, dir: &Path) -> CoreResult<()> {
    std::fs::create_dir_all(dir)?;
    let json = to_canonical_json(&session.project)?;
    let referenced = session.project.referenced_blobs();
    let blob_bytes: u64 = referenced.iter().filter_map(|h| session.blobs.get(h)).map(|b| b.len() as u64).sum();
    ensure_space(dir, json.len() as u64 + blob_bytes)?;
    let required = session.project.required_blobs();
    let mut pending = Vec::new();
    for h in &referenced {
        match session.blobs.get(h) {
            Some(b) => pending.push((h.clone(), b)),
            None if required.contains(h) => return Err(CoreError::MissingBlob(h.0.clone())),
            None => {} // string com cara de hash em JSON livre que não é blob
        }
    }
    write_blobs_parallel(dir, &pending)?;
    write_atomic(&dir.join(PROJECT_FILE), json.as_bytes())?;
    // WAL recomeça a partir desta revisão.
    let wal = Wal::create(dir, session.project.revision)?;
    session.attach_wal(wal);
    session.blobs.take_dirty();
    session.path = Some(dir.to_path_buf());
    // o que nem o documento nem o histórico usam sai da memória (traços desfeitos e descartados, tiles antigos)
    session.collect_garbage();
    Ok(())
}

/// Cópia de segurança do documento numa pasta `.aiai` (autosave de projeto ainda sem pasta). Não mexe na sessão:
/// sem WAL, sem trocar `path`; roda em outra thread com um clone (os blobs são `Arc`, o clone é barato).
pub fn snapshot(project: &Project, blobs: &BlobStore, dir: &Path) -> CoreResult<()> {
    std::fs::create_dir_all(dir)?;
    let json = to_canonical_json(project)?;
    let referenced = project.referenced_blobs();
    let blob_bytes: u64 = referenced.iter().filter_map(|h| blobs.get(h)).map(|b| b.len() as u64).sum();
    ensure_space(dir, json.len() as u64 + blob_bytes)?;
    let pending: Vec<_> = referenced.iter().filter_map(|h| blobs.get(h).map(|b| (h.clone(), b))).collect();
    write_blobs_parallel(dir, &pending)?;
    write_atomic(&dir.join(PROJECT_FILE), json.as_bytes())?;
    let _ = std::fs::remove_file(dir.join(WAL_FILE));
    Ok(())
}

/// Abre um projeto: lê o JSON, os blobs referenciados e reaplica o WAL (se houver comandos depois da revisão salva).
pub fn open(dir: &Path) -> CoreResult<(CoreSession, OpenReport)> {
    let text = std::fs::read_to_string(dir.join(PROJECT_FILE))?;
    let project: Project = serde_json::from_str(&text)?;
    if project.format > crate::FORMAT_VERSION {
        return Err(CoreError::invalid("format", format!("formato {} mais novo que o suportado {}", project.format, crate::FORMAT_VERSION)));
    }
    project.validate().map_err(|e| CoreError::invalid("project", e))?;
    let mut blobs = BlobStore::default();
    let required = project.required_blobs();
    for h in project.referenced_blobs() {
        match std::fs::read(blob_path(dir, &h)) {
            Ok(bytes) => blobs.insert_clean(h, bytes)?,
            Err(_) if required.contains(&h) => return Err(CoreError::MissingBlob(h.0.clone())),
            Err(_) => {}
        }
    }
    let mut session = CoreSession::new(project);
    session.blobs = blobs;
    let mut report = OpenReport::default();
    // Recuperação: reaplica entradas do WAL posteriores à revisão salva.
    let wal_path = dir.join(WAL_FILE);
    if wal_path.exists() {
        let saved_rev = session.project.revision;
        let content = std::fs::read_to_string(&wal_path)?;
        for (line_no, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: WalLine = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(_) => {
                    report.wal_truncated_at = Some(line_no);
                    break; // linha incompleta (queda no meio da escrita)
                }
            };
            match entry {
                WalLine::Header { .. } => {}
                WalLine::Blob { hash } => {
                    let h = BlobHash(hash);
                    if let Ok(bytes) = std::fs::read(blob_path(dir, &h)) {
                        let _ = session.blobs.insert_clean(h, bytes);
                    }
                }
                WalLine::Entry(e) => {
                    if e.revision <= saved_rev {
                        continue;
                    }
                    let result = match e.op.as_str() {
                        "apply" | "undo" | "redo" => match e.command {
                            Some(c) => replay(&mut session, c, &e.author),
                            None => Ok(()),
                        },
                        _ => Ok(()),
                    };
                    match result {
                        Ok(()) => report.replayed += 1,
                        Err(err) => {
                            report.replay_error = Some(format!("revisão {}: {err}", e.revision));
                            break;
                        }
                    }
                }
            }
        }
    }
    session.attach_wal(Wal::open_append(dir, report.replayed > 0, session.project.revision)?);
    session.path = Some(dir.to_path_buf());
    Ok((session, report))
}

/// Reaplica um comando do WAL como comando novo (o histórico de undo não é recuperado, só o documento).
fn replay(session: &mut CoreSession, c: Command, author: &str) -> CoreResult<()> {
    let wal = session.detach_wal();
    let r = session.apply_as(c, author, None).map(|_| ());
    if let Some(w) = wal {
        session.attach_wal(w);
    }
    r
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OpenReport {
    pub replayed: usize,
    pub wal_truncated_at: Option<usize>,
    pub replay_error: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum WalLine {
    Header { wal_base_revision: u64 },
    Blob { hash: String },
    Entry(LogEntry),
}

/// Diário de comandos depois do último salvamento.
pub struct Wal {
    dir: PathBuf,
    file: std::fs::File,
}

impl Wal {
    pub fn create(dir: &Path, base_revision: u64) -> CoreResult<Self> {
        let path = dir.join(WAL_FILE);
        let mut file = retry_io(|| std::fs::File::create(&path))?;
        writeln!(file, "{}", serde_json::to_string(&WalLine::Header { wal_base_revision: base_revision })?)?;
        file.sync_data()?;
        Ok(Wal { dir: dir.to_path_buf(), file })
    }

    fn open_append(dir: &Path, keep_existing: bool, base_revision: u64) -> CoreResult<Self> {
        if !keep_existing || !dir.join(WAL_FILE).exists() {
            return Wal::create(dir, base_revision);
        }
        let file = retry_io(|| std::fs::OpenOptions::new().append(true).open(dir.join(WAL_FILE)))?;
        Ok(Wal { dir: dir.to_path_buf(), file })
    }

    /// Grava os blobs novos (para a recuperação ter o conteúdo) e depois a entrada.
    pub fn append(&mut self, entry: &LogEntry, blobs: &mut BlobStore) -> CoreResult<()> {
        for h in blobs.take_dirty() {
            if let Some(b) = blobs.get(&h) {
                // sem sync por tile (custava ~1/3 s por pincelada): a linha do WAL abaixo é que vai sincronizada
                write_blob(&self.dir, &h, &b, false)?;
                writeln!(self.file, "{}", serde_json::to_string(&WalLine::Blob { hash: h.0.clone() })?)?;
            }
        }
        writeln!(self.file, "{}", serde_json::to_string(&WalLine::Entry(entry.clone()))?)?;
        self.file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod retry_tests {
    use super::retry_io;

    #[test]
    fn retry_io_rides_out_transient_locks_but_not_real_errors() {
        let mut calls = 0;
        let r = retry_io(|| {
            calls += 1;
            if calls < 3 { Err(std::io::Error::from_raw_os_error(1224)) } else { Ok(7) }
        });
        assert_eq!(r.unwrap(), 7);
        assert_eq!(calls, 3, "duas falhas passageiras e o sucesso");
        let mut calls = 0;
        let r: std::io::Result<()> = retry_io(|| {
            calls += 1;
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "não existe"))
        });
        assert!(r.is_err());
        assert_eq!(calls, 1, "erro de verdade volta na primeira");
    }
}
