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

fn write_atomic(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let tmp = path.with_extension("tmp-write");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    // Confere que o temporário ficou inteiro antes de substituir o original.
    let written = std::fs::metadata(&tmp)?.len();
    if written != bytes.len() as u64 {
        let _ = std::fs::remove_file(&tmp);
        return Err(CoreError::Io(format!("escrita incompleta em {} ({written} de {} bytes)", tmp.display(), bytes.len())));
    }
    std::fs::rename(&tmp, path)?;
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

fn write_blob(dir: &Path, h: &BlobHash, bytes: &[u8]) -> CoreResult<()> {
    let path = blob_path(dir, h);
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().expect("tem pai"))?;
    write_atomic(&path, bytes)
}

/// Salva o projeto da sessão em `dir` (cria se preciso) e zera o WAL.
pub fn save(session: &mut CoreSession, dir: &Path) -> CoreResult<()> {
    std::fs::create_dir_all(dir)?;
    let json = to_canonical_json(&session.project)?;
    let referenced = session.project.referenced_blobs();
    let blob_bytes: u64 = referenced.iter().filter_map(|h| session.blobs.get(h)).map(|b| b.len() as u64).sum();
    ensure_space(dir, json.len() as u64 + blob_bytes)?;
    for h in &referenced {
        let b = session.blobs.get(h).ok_or_else(|| CoreError::MissingBlob(h.0.clone()))?;
        write_blob(dir, h, &b)?;
    }
    write_atomic(&dir.join(PROJECT_FILE), json.as_bytes())?;
    // WAL recomeça a partir desta revisão.
    let wal = Wal::create(dir, session.project.revision)?;
    session.attach_wal(wal);
    session.blobs.take_dirty();
    session.path = Some(dir.to_path_buf());
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
    for h in project.referenced_blobs() {
        let bytes = std::fs::read(blob_path(dir, &h)).map_err(|_| CoreError::MissingBlob(h.0.clone()))?;
        blobs.insert_clean(h, bytes)?;
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
        let mut file = std::fs::File::create(&path)?;
        writeln!(file, "{}", serde_json::to_string(&WalLine::Header { wal_base_revision: base_revision })?)?;
        file.sync_data()?;
        Ok(Wal { dir: dir.to_path_buf(), file })
    }

    fn open_append(dir: &Path, keep_existing: bool, base_revision: u64) -> CoreResult<Self> {
        if !keep_existing || !dir.join(WAL_FILE).exists() {
            return Wal::create(dir, base_revision);
        }
        let file = std::fs::OpenOptions::new().append(true).open(dir.join(WAL_FILE))?;
        Ok(Wal { dir: dir.to_path_buf(), file })
    }

    /// Grava os blobs novos (para a recuperação ter o conteúdo) e depois a entrada.
    pub fn append(&mut self, entry: &LogEntry, blobs: &mut BlobStore) -> CoreResult<()> {
        for h in blobs.take_dirty() {
            if let Some(b) = blobs.get(&h) {
                write_blob(&self.dir, &h, &b)?;
                writeln!(self.file, "{}", serde_json::to_string(&WalLine::Blob { hash: h.0.clone() })?)?;
            }
        }
        writeln!(self.file, "{}", serde_json::to_string(&WalLine::Entry(entry.clone()))?)?;
        self.file.sync_data()?;
        Ok(())
    }
}
