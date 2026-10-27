//! Persistência em pasta `.aiai`:
//!
//! ```text
//! meu-projeto.aiai/
//!   meu-projeto.aiaiproj  arquivo para abrir o projeto (Arquivo > Abrir, Explorer): aponta para a própria pasta
//!   project.json      documento canônico (chaves ordenadas, indentado: diffs legíveis em git)
//!   blobs/ab/<hash>   conteúdo por BLAKE3 (tiles, máscaras, imagens, áudio)
//!   wal.ndjson        comandos aplicados depois do último salvamento (recuperação após queda)
//!   project.lock      trava "aberto em outro lugar" (pid, máquina, início; ver `lock`)
//!   backup/           cópias do documento: `project.json.open-N.bak` (ao abrir) e `.auto-N.bak` (periódica), N = 1..3
//!   thumbnail.png     miniatura (256 px no lado maior) do 1º quadro do 1º corte, gravada pelo app ao salvar
//! ```
//!
//! Salvar é atômico (arquivo temporário + `sync` + renomear + `sync` da pasta) e confere espaço livre antes (o disco desta
//! máquina vive cheio e escrita interrompida já truncou arquivo — ver lições). Todo blob vai para o disco com `sync` antes
//! da linha do diário (ou do `project.json`) que o cita. Abrir é tolerante: blob ilegível vira "danificado" no relatório
//! (o projeto abre sem ele, a referência fica) e linha ruim do diário é pulada, com cópia do diário original guardada.

use crate::blob::{BlobHash, BlobStore};
use crate::command::Command;
use crate::error::{CoreError, CoreResult};
use crate::lock::LockInfo;
use crate::model::{NodeKind, Project};
use crate::session::{CoreSession, LogEntry};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE: &str = "project.json";
pub const WAL_FILE: &str = "wal.ndjson";
pub const BLOB_DIR: &str = "blobs";
pub const BACKUP_DIR: &str = "backup";
/// Cópias do documento mantidas por série (ao abrir / periódica).
pub const BACKUP_KEEP: usize = 3;
pub const THUMBNAIL_FILE: &str = "thumbnail.png";
/// Lado maior da miniatura do projeto (limite do `.ora` e tamanho típico do Explorer).
pub const THUMBNAIL_SIZE: u32 = 256;
/// Extensão do arquivo que abre o projeto (fica dentro da pasta, com o nome dela).
pub const OPENER_EXT: &str = "aiaiproj";

/// Caminho do arquivo que abre o projeto: `meu-projeto.aiai/meu-projeto.aiaiproj`.
pub fn opener_path(dir: &Path) -> PathBuf {
    let stem = dir.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| "projeto".into());
    dir.join(format!("{stem}.{OPENER_EXT}"))
}

/// Grava (ou atualiza) o arquivo que abre o projeto. Pequeno JSON: quem lê é o próprio programa (a pasta é o projeto).
pub fn write_opener(dir: &Path, project: &Project) -> CoreResult<()> {
    let body = serde_json::to_string_pretty(&serde_json::json!({ "aiai_project": PROJECT_FILE, "name": project.name, "format": project.format }))?;
    let path = opener_path(dir);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
        write_atomic(&path, body.as_bytes())?;
    }
    Ok(())
}

/// Pasta do projeto a partir do que o usuário escolheu: a própria pasta, o `.aiaiproj` ou o `project.json` dentro dela.
pub fn resolve_project_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return path.join(PROJECT_FILE).is_file().then(|| path.to_path_buf());
    }
    let is_opener = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(OPENER_EXT));
    let is_doc = path.file_name().is_some_and(|n| n.eq_ignore_ascii_case(PROJECT_FILE));
    if is_opener || is_doc {
        let dir = path.parent()?;
        return dir.join(PROJECT_FILE).is_file().then(|| dir.to_path_buf());
    }
    None
}

/// Espaço mínimo livre para aceitar salvar (bytes).
pub const MIN_FREE_BYTES: u64 = 512 * 1024 * 1024;

/// JSON canônico do projeto (o `serde_json` sem `preserve_order` ordena chaves de mapas; structs têm ordem fixa).
pub fn to_canonical_json(project: &Project) -> CoreResult<String> {
    let v = serde_json::to_value(project)?;
    let mut s = serde_json::to_string_pretty(&v)?;
    s.push('\n');
    Ok(s)
}

/// Problema de um blob que o disco não tem (os demais são cópias ilegíveis, vazias ou corrompidas).
pub const BLOB_ABSENT: &str = "arquivo ausente";

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
    write_atomic_synced(path, bytes)?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

/// Gravação atômica e durável de um arquivo pequeno (pincéis, miniatura, trava): temporário, `sync`, confere o tamanho,
/// renomeia e sincroniza a pasta.
pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    write_atomic(path, bytes)
}

/// Temporário + `sync_all` + troca (sobrevive a queda de energia). A entrada da pasta (o renomear) fica para quem chama
/// sincronizar com [`sync_dir`]: os blobs sincronizam as pastas uma vez por lote.
fn write_atomic_synced(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let tmp = path.with_extension("tmp-write");
    {
        let mut f = retry_io(|| std::fs::File::create(&tmp))?;
        f.write_all(bytes)?;
        f.sync_all()?;
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

/// Torna durável a entrada de pasta (criar/renomear) depois de uma troca atômica. No Unix é o `fsync` da pasta. No
/// Windows abre a pasta com `FILE_FLAG_BACKUP_SEMANTICS` e chama `FlushFileBuffers`; sistema de arquivos que não aceita
/// isso (FAT/exFAT, rede) responde "função inválida" (1), "acesso negado" (5) ou "não suportado" (50): aí vale a garantia
/// do próprio sistema (o NTFS registra o renomear no diário dele) e não é erro. Qualquer outra falha volta.
pub(crate) fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let unsupported = |e: &std::io::Error| matches!(e.raw_os_error(), Some(1 | 5 | 50));
        let f = match std::fs::OpenOptions::new().write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(dir) {
            Ok(f) => f,
            Err(e) if unsupported(&e) => return Ok(()),
            Err(e) => return Err(e),
        };
        match f.sync_all() {
            Err(e) if unsupported(&e) => Ok(()),
            r => r,
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::File::open(dir)?.sync_all()
    }
}

/// (ano, mês, dia, hora, minuto, segundo) UTC de um instante em ms desde a época Unix (sem dependência de calendário).
pub(crate) fn utc_parts(ms: u64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = (ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // dias desde 1970-01-01 -> data civil (algoritmo de H. Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Carimbo para nomes de arquivo: `20261019-140312` (UTC).
fn file_stamp() -> String {
    let (y, mo, d, h, mi, s) = utc_parts(now_ms());
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// Guarda uma cópia de `path` ao lado (`<nome>.<tag>-<data>`) antes que algo o sobrescreva. Se já existe uma cópia
/// com o mesmo conteúdo, devolve ela (abrir o app várias vezes não acumula cópias iguais).
fn keep_copy(path: &Path, tag: &str) -> CoreResult<PathBuf> {
    let bytes = retry_io(|| std::fs::read(path)).map_err(|e| CoreError::Io(format!("{}: {e}", path.display())))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = path.parent().unwrap_or(Path::new("."));
    let prefix = format!("{name}.{tag}-");
    if let Ok(rd) = std::fs::read_dir(parent) {
        for e in rd.flatten() {
            let same_size = e.metadata().is_ok_and(|m| m.len() == bytes.len() as u64);
            if same_size && e.file_name().to_string_lossy().starts_with(&prefix) && std::fs::read(e.path()).is_ok_and(|b| b == bytes) {
                return Ok(e.path());
            }
        }
    }
    let stamp = file_stamp();
    let mut copy = parent.join(format!("{prefix}{stamp}"));
    let mut n = 1;
    while copy.exists() {
        n += 1;
        copy = parent.join(format!("{prefix}{stamp}-{n}"));
    }
    write_atomic(&copy, &bytes)?;
    Ok(copy)
}

/// Cópia de um arquivo que não pôde ser lido/entendido (`<nome>.corrupt-<data>`, ao lado), feita antes de qualquer
/// gravação por cima dele (ex.: `brushes.json` ilegível). A mesma cópia já guardada = devolve a existente.
pub fn keep_corrupt_copy(path: &Path) -> CoreResult<PathBuf> {
    keep_copy(path, "corrupt")
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

/// Grava um blob (durável) se o disco ainda não tem uma cópia boa. Arquivo existente com tamanho diferente é cópia
/// ruim (queda no meio de uma gravação antiga: o renomear chegou ao disco e os dados não) e é regravado; com o mesmo
/// tamanho, `verify` confere o hash lendo o arquivo. Devolve se gravou.
fn write_blob(dir: &Path, h: &BlobHash, bytes: &[u8], verify: bool) -> CoreResult<bool> {
    if !h.is_well_formed() {
        return Err(CoreError::invalid("blob", format!("hash malformado: {h}")));
    }
    let path = blob_path(dir, h);
    match std::fs::metadata(&path) {
        Ok(m) if m.len() == bytes.len() as u64 => {
            if !verify || retry_io(|| std::fs::read(&path)).is_ok_and(|b| BlobHash::of(&b) == *h) {
                return Ok(false);
            }
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(CoreError::Io(format!("{}: {e}", path.display()))),
    }
    std::fs::create_dir_all(path.parent().expect("tem pai"))?;
    write_atomic_synced(&path, bytes)?;
    Ok(true)
}

/// Grava vários blobs duráveis em paralelo e depois sincroniza as pastas que receberam arquivo: o custo é o `sync_all`
/// de cada arquivo (dezenas de ms no Windows com antivírus), e em série ~200 tiles deixavam a interface parada por
/// segundos. `verify(h)` = conferir o hash de um arquivo de mesmo tamanho que já existe. Devolve os que foram gravados.
fn write_blobs_parallel(dir: &Path, items: &[(BlobHash, std::sync::Arc<Vec<u8>>)], verify: &(dyn Fn(&BlobHash) -> bool + Sync)) -> CoreResult<Vec<BlobHash>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 16);
    let chunk = items.len().div_ceil(threads);
    let written = std::thread::scope(|sc| -> CoreResult<Vec<BlobHash>> {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                sc.spawn(move || -> CoreResult<Vec<BlobHash>> {
                    let mut wrote = Vec::new();
                    for (h, b) in part {
                        if write_blob(dir, h, b, verify(h))? {
                            wrote.push(h.clone());
                        }
                    }
                    Ok(wrote)
                })
            })
            .collect();
        let mut all = Vec::new();
        for hd in handles {
            all.extend(hd.join().map_err(|_| CoreError::Io("gravação de blob entrou em pânico".into()))??);
        }
        Ok(all)
    })?;
    if !written.is_empty() {
        // as pastas `blobs/xx` (o renomear de cada arquivo) e `blobs` (pasta de prefixo nova)
        let shards: BTreeSet<PathBuf> = written.iter().filter_map(|h| blob_path(dir, h).parent().map(Path::to_path_buf)).collect();
        for d in &shards {
            sync_dir(d)?;
        }
        sync_dir(&dir.join(BLOB_DIR))?;
    }
    Ok(written)
}

/// Garante a trava da pasta antes de gravar nela: a desta sessão (conferindo que ninguém assumiu) ou uma nova, se a
/// pasta está livre ou com trava velha. Pasta travada por outro processo vivo = erro (a sessão abriu só para leitura).
fn claim_lock(session: &CoreSession, dir: &Path) -> CoreResult<Option<crate::lock::ProjectLock>> {
    if let Some(l) = session.lock().filter(|l| l.dir() == dir) {
        l.verify()?;
        return Ok(None);
    }
    match crate::lock::acquire(dir, false)? {
        Ok(a) => Ok(Some(a.lock)),
        Err(owner) => Err(CoreError::Locked(format!("{} (use Salvar como para gravar uma cópia em outra pasta)", owner.describe()))),
    }
}

/// Salva o projeto da sessão em `dir` (cria se preciso) e zera o WAL. Recusa, sem gravar nada, se outro processo vivo
/// tem a trava da pasta.
pub fn save(session: &mut CoreSession, dir: &Path) -> CoreResult<()> {
    std::fs::create_dir_all(dir)?;
    let new_lock = claim_lock(session, dir)?;
    let json = to_canonical_json(&session.project)?;
    let referenced = session.project.referenced_blobs();
    let blob_bytes: u64 = referenced.iter().filter_map(|h| session.blobs.get(h)).map(|b| b.len() as u64).sum();
    ensure_space(dir, json.len() as u64 + blob_bytes)?;
    let required = session.project.required_blobs();
    let mut pending = Vec::new();
    for h in &referenced {
        match session.blobs.get(h) {
            Some(b) => pending.push((h.clone(), b)),
            // danificado desde a abertura: a referência fica no documento (nada é apagado), o arquivo ruim também
            None if session.blobs.is_missing(h) => {}
            None if required.contains(h) => return Err(CoreError::MissingBlob(h.0.clone())),
            None => {} // string com cara de hash em JSON livre que não é blob
        }
    }
    // pasta nova (Salvar como): nada conferido nela ainda; na mesma pasta, só o que não foi lido/gravado nesta sessão
    if session.path.as_deref() != Some(dir) {
        session.blobs.clear_on_disk();
    }
    let blobs = &session.blobs;
    write_blobs_parallel(dir, &pending, &|h| !blobs.is_on_disk(h))?;
    for (h, _) in &pending {
        session.blobs.mark_on_disk(h.clone());
    }
    write_atomic(&dir.join(PROJECT_FILE), json.as_bytes())?;
    write_opener(dir, &session.project)?;
    // WAL recomeça a partir desta revisão.
    let wal = Wal::create(dir, session.project.revision)?;
    session.attach_wal(wal);
    if new_lock.is_some() {
        session.set_lock(new_lock);
    }
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
    write_blobs_parallel(dir, &pending, &|_| true)?;
    write_atomic(&dir.join(PROJECT_FILE), json.as_bytes())?;
    let _ = std::fs::remove_file(dir.join(WAL_FILE));
    Ok(())
}

fn backup_name(kind: &str, n: usize) -> String {
    format!("{PROJECT_FILE}.{kind}-{n}.bak")
}

/// Cópias da série `kind` em `backup/`, mais nova primeiro.
pub fn backups(dir: &Path, kind: &str) -> Vec<PathBuf> {
    (1..=BACKUP_KEEP).map(|n| dir.join(BACKUP_DIR).join(backup_name(kind, n))).filter(|p| p.is_file()).collect()
}

/// Guarda `bytes` como cópia mais nova da série `kind` em `backup/` (mantém as [`BACKUP_KEEP`] últimas; 1 = mais nova).
/// Igual à mais nova = nada a fazer. Só o documento: os blobs são por conteúdo e nunca são apagados do disco, então a
/// cópia continua apontando para tiles que existem (uma coleta de lixo em disco, se vier, tem de respeitar `backup/`).
fn rotate_backup(dir: &Path, kind: &str, bytes: &[u8]) -> CoreResult<Option<PathBuf>> {
    let bdir = dir.join(BACKUP_DIR);
    let newest = bdir.join(backup_name(kind, 1));
    if std::fs::read(&newest).is_ok_and(|b| b == bytes) {
        return Ok(None);
    }
    ensure_space(dir, bytes.len() as u64)?;
    std::fs::create_dir_all(&bdir)?;
    for n in (1..BACKUP_KEEP).rev() {
        let from = bdir.join(backup_name(kind, n));
        if from.is_file() {
            let to = bdir.join(backup_name(kind, n + 1));
            retry_io(|| std::fs::rename(&from, &to))?;
        }
    }
    write_atomic(&newest, bytes)?;
    Ok(Some(newest))
}

/// Cópia periódica do documento em memória (projeto com pasta e alterações não salvas): ponto de restauração além do
/// diário. Os blobs que ele cita já estão no disco (o diário grava cada um antes da linha que o usa).
pub fn backup_document(dir: &Path, project: &Project) -> CoreResult<Option<PathBuf>> {
    let json = to_canonical_json(project)?;
    rotate_backup(dir, "auto", json.as_bytes())
}

/// Abre um projeto: lê o JSON, os blobs referenciados e reaplica o WAL (se houver comandos depois da revisão salva).
/// Outro processo vivo com a trava = abre só para leitura (`report.locked_by`; sem diário, e o Salvar na pasta recusa).
pub fn open(dir: &Path) -> CoreResult<(CoreSession, OpenReport)> {
    open_with(dir, false)
}

/// Como [`open`]; `take_over` assume a trava mesmo de outro processo vivo (o usuário escolheu "Assumir").
pub fn open_with(dir: &Path, take_over: bool) -> CoreResult<(CoreSession, OpenReport)> {
    let doc_bytes = std::fs::read(dir.join(PROJECT_FILE))?;
    let project: Project = serde_json::from_slice(&doc_bytes)?;
    if project.format > crate::FORMAT_VERSION {
        return Err(CoreError::invalid("format", format!("formato {} mais novo que o suportado {}", project.format, crate::FORMAT_VERSION)));
    }
    project.validate().map_err(|e| CoreError::invalid("project", e))?;
    let mut report = OpenReport::default();
    let lock = match crate::lock::acquire(dir, take_over)? {
        Ok(a) => {
            report.replaced_lock = a.replaced;
            Some(a.lock)
        }
        Err(owner) => {
            report.locked_by = Some(owner);
            None
        }
    };
    let read_only = lock.is_none();

    // Blobs do documento: ilegível não impede abrir; fica marcado como danificado e vai para o relatório.
    let mut blobs = BlobStore::default();
    let required = project.required_blobs();
    let mut damaged: Vec<(BlobHash, String, &'static str)> = Vec::new();
    for h in project.referenced_blobs() {
        if !h.is_well_formed() {
            blobs.mark_missing(h.clone());
            damaged.push((h, "hash malformado no documento".into(), PROJECT_FILE));
            continue;
        }
        match read_blob(dir, &h) {
            BlobRead::Ok(bytes) => {
                blobs.insert_clean(h.clone(), bytes).expect("hash conferido");
                blobs.mark_on_disk(h);
            }
            BlobRead::Absent if !required.contains(&h) => {} // string com cara de hash em JSON livre que não é blob
            BlobRead::Absent => {
                blobs.mark_missing(h.clone());
                damaged.push((h, BLOB_ABSENT.into(), PROJECT_FILE));
            }
            BlobRead::Bad(why) => {
                blobs.mark_missing(h.clone());
                damaged.push((h, why, PROJECT_FILE));
            }
        }
    }
    let mut session = CoreSession::new(project);
    session.blobs = blobs;

    // Recuperação: reaplica entradas do WAL posteriores à revisão salva.
    let wal_path = dir.join(WAL_FILE);
    let mut wal_keep: Option<usize> = None; // bytes do diário que continuam valendo (se algo foi pulado)
    let mut wal_len = 0usize;
    let mut wal_ends_clean = true;
    if wal_path.exists() {
        let saved_rev = session.project.revision;
        let content = retry_io(|| std::fs::read(&wal_path))?;
        wal_len = content.len();
        wal_ends_clean = content.last().is_none_or(|&b| b == b'\n');
        let lines = split_lines(&content);
        let non_empty = |from: usize| lines[from..].iter().filter(|(s, e)| !content[*s..*e].trim_ascii().is_empty()).count();
        for (i, &(start, end)) in lines.iter().enumerate() {
            let line = &content[start..end];
            if line.trim_ascii().is_empty() {
                continue;
            }
            let entry: WalLine = match serde_json::from_slice(line) {
                Ok(e) => e,
                Err(_) => {
                    // linha incompleta (queda no meio da escrita) ou estragada: para aqui, sem perder o resto (cópia)
                    report.wal_truncated_at = Some(i);
                    report.wal_lines_skipped = non_empty(i);
                    report.wal_tail_only = report.wal_lines_skipped == 1;
                    wal_keep = Some(start);
                    break;
                }
            };
            match entry {
                WalLine::Header { .. } => {}
                WalLine::Blob { hash } => {
                    let h = BlobHash(hash);
                    if !h.is_well_formed() {
                        damaged.push((h, "hash malformado no diário".into(), WAL_FILE));
                        continue;
                    }
                    match read_blob(dir, &h) {
                        BlobRead::Ok(bytes) => {
                            session.blobs.insert_clean(h.clone(), bytes).expect("hash conferido");
                            session.blobs.mark_on_disk(h);
                        }
                        // já carregado pelo documento? então o que falta é só esta cópia velha: nada a fazer
                        _ if session.blobs.contains(&h) => {}
                        BlobRead::Absent => {
                            session.blobs.mark_missing(h.clone());
                            damaged.push((h, BLOB_ABSENT.into(), WAL_FILE));
                        }
                        BlobRead::Bad(why) => {
                            session.blobs.mark_missing(h.clone());
                            damaged.push((h, why, WAL_FILE));
                        }
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
                            report.wal_lines_skipped = non_empty(i);
                            wal_keep = Some(start);
                            break;
                        }
                    }
                }
            }
        }
    }
    report.damaged_blobs = describe_damage(&session.project, damaged);

    if !read_only {
        if let Some(keep) = wal_keep {
            // o diário original (com as linhas puladas) fica guardado; o novo termina na última linha boa, para os
            // próximos comandos não grudarem na linha ruim (o replay seguinte pararia nela e perderia tudo depois)
            let copy = keep_copy(&wal_path, "damaged")?;
            report.wal_backup = Some(copy);
            let content = retry_io(|| std::fs::read(&wal_path))?;
            write_atomic(&wal_path, &content[..keep.min(content.len())])?;
        } else if !wal_ends_clean && wal_len > 0 && report.replayed > 0 {
            // última linha legível mas sem a quebra de linha: completa antes de anexar
            let mut f = retry_io(|| std::fs::OpenOptions::new().append(true).open(&wal_path))?;
            f.write_all(b"\n")?;
            f.sync_data()?;
        }
        session.attach_wal(Wal::open_append(dir, report.replayed > 0, session.project.revision)?);
        match rotate_backup(dir, "open", &doc_bytes) {
            Ok(p) => report.backup = p,
            Err(e) => report.warnings.push(format!("cópia de segurança do documento ao abrir falhou: {e}")),
        }
    }
    session.set_lock(lock);
    session.path = Some(dir.to_path_buf());
    Ok((session, report))
}

enum BlobRead {
    Ok(Vec<u8>),
    Absent,
    /// Truncado, com hash diferente ou ilegível (o motivo vai para o relatório).
    Bad(String),
}

fn read_blob(dir: &Path, h: &BlobHash) -> BlobRead {
    match retry_io(|| std::fs::read(blob_path(dir, h))) {
        Ok(bytes) if BlobHash::of(&bytes) == *h => BlobRead::Ok(bytes),
        Ok(bytes) if bytes.is_empty() => BlobRead::Bad("arquivo vazio (gravação interrompida)".into()),
        Ok(bytes) => BlobRead::Bad(format!("conteúdo não confere com o hash ({} bytes no disco: truncado ou corrompido)", bytes.len())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BlobRead::Absent,
        Err(e) => BlobRead::Bad(format!("ilegível: {e}")),
    }
}

/// Linhas de `content` como intervalos de bytes (sem o `\n`; a última pode não ter).
fn split_lines(content: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &b) in content.iter().enumerate() {
        if b == b'\n' {
            out.push((start, i));
            start = i + 1;
        }
    }
    if start < content.len() {
        out.push((start, content.len()));
    }
    out
}

/// Para cada blob danificado, quem o usa no documento (camada/cel/tile, recurso...), para o usuário saber o que falta.
fn describe_damage(project: &Project, damaged: Vec<(BlobHash, String, &'static str)>) -> Vec<DamagedBlob> {
    let mut seen = BTreeSet::new();
    let damaged: Vec<_> = damaged.into_iter().filter(|(h, _, _)| seen.insert(h.clone())).collect();
    if damaged.is_empty() {
        return Vec::new();
    }
    let mut users: BTreeMap<BlobHash, Vec<String>> = BTreeMap::new();
    let mut kinds: BTreeMap<BlobHash, &'static str> = BTreeMap::new();
    for (id, n) in &project.nodes {
        let mut add = |h: &BlobHash, kind: &'static str, what: String| {
            if seen.contains(h) {
                users.entry(h.clone()).or_default().push(what);
                kinds.entry(h.clone()).or_insert(kind);
            }
        };
        match &n.data {
            NodeKind::Cel(c) => {
                let layer = n.parent.as_ref().and_then(|p| project.node(p)).map(|l| l.name.as_str()).unwrap_or("?");
                let at = format!("camada \"{layer}\", cel \"{}\" ({id})", n.name);
                for t in &c.tiles {
                    add(&t.blob, "tile", format!("{at}: tile ({}, {})", t.x, t.y));
                }
                for f in &c.fills {
                    if let Some(m) = &f.mask {
                        add(m, "mask", format!("{at}: máscara de preenchimento"));
                    }
                }
                if let Some(i) = &c.image {
                    add(i, "image", format!("{at}: imagem"));
                }
            }
            NodeKind::Asset(a) => {
                if let Some(b) = &a.blob {
                    let kind = match a.asset {
                        crate::model::AssetType::AudioClip => "audio",
                        crate::model::AssetType::Model3d => "model3d",
                        crate::model::AssetType::Texture | crate::model::AssetType::Background => "image",
                        crate::model::AssetType::Font => "font",
                        _ => "asset",
                    };
                    add(b, kind, format!("recurso \"{}\" ({id})", n.name));
                }
            }
            _ => {}
        }
    }
    damaged
        .into_iter()
        .map(|(hash, problem, source)| {
            let used_by = users.remove(&hash).unwrap_or_else(|| {
                vec![if source == WAL_FILE { "só no diário (o documento recuperado não o usa mais)".into() } else { "citado em parâmetros do documento".into() }]
            });
            let kind = kinds.remove(&hash).unwrap_or(if source == WAL_FILE { "journal" } else { "other" });
            let missing = problem == BLOB_ABSENT;
            DamagedBlob { hash, problem, used_by, kind, missing }
        })
        .collect()
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

/// Blob citado que não pôde ser lido ao abrir.
#[derive(Debug, Clone, PartialEq)]
pub struct DamagedBlob {
    pub hash: BlobHash,
    pub problem: String,
    /// Onde o documento usa este blob (para dizer ao usuário que tiles estão danificados).
    pub used_by: Vec<String>,
    /// O que é: tile, mask, image, audio, model3d, font, asset, journal (só no diário) ou other.
    pub kind: &'static str,
    /// `true` = o arquivo não existe; `false` = existe mas está ilegível, vazio ou corrompido.
    pub missing: bool,
}

/// Devolve ao projeto um blob que abriu danificado ou ausente, a partir de uma cópia que o usuário achou (backup, outra
/// máquina). O conteúdo precisa conferir com o hash que o documento espera; outro arquivo dá `Ok(None)` e nada muda.
/// Grava na pasta do projeto (se houver uma) e põe na memória; o documento e o histórico não mudam.
pub fn restore_blob(session: &mut CoreSession, bytes: Vec<u8>) -> CoreResult<Option<BlobHash>> {
    let h = BlobHash::of(&bytes);
    if !session.blobs.is_missing(&h) {
        return Ok(None);
    }
    if let Some(dir) = session.path.clone() {
        write_blob(&dir, &h, &bytes, true)?;
        session.blobs.mark_on_disk(h.clone());
    }
    session.blobs.insert_clean(h.clone(), bytes)?;
    // caches de imagem que se guiam pelo contador refazem o que mostrava o buraco
    session.change_counter += 1;
    Ok(Some(h))
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct OpenReport {
    pub replayed: usize,
    /// Linha (0-based) do diário que não pôde ser lida; a leitura parou nela.
    pub wal_truncated_at: Option<usize>,
    pub replay_error: Option<String>,
    /// Linhas não vazias do diário que não foram reaplicadas (a ruim e as seguintes).
    pub wal_lines_skipped: usize,
    /// A linha ilegível era a última (queda no meio da gravação): perde-se só ela.
    pub wal_tail_only: bool,
    /// Cópia do diário original, guardada antes de reescrevê-lo sem as linhas puladas.
    pub wal_backup: Option<PathBuf>,
    pub damaged_blobs: Vec<DamagedBlob>,
    /// Outro processo vivo tem a trava: aberto só para leitura (sem diário; Salvar na pasta recusa).
    pub locked_by: Option<LockInfo>,
    /// Trava anterior substituída: velha (o processo caiu) ou assumida (`open_with(.., true)`).
    pub replaced_lock: Option<LockInfo>,
    /// Cópia do `project.json` guardada em `backup/` nesta abertura (`None` = igual à última, ou só leitura).
    pub backup: Option<PathBuf>,
    /// Outros problemas que não impediram abrir.
    pub warnings: Vec<String>,
}

impl OpenReport {
    pub fn read_only(&self) -> bool {
        self.locked_by.is_some()
    }
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

    /// Grava os blobs novos (para a recuperação ter o conteúdo) e depois a entrada. Os blobs vão com `sync` (arquivo e
    /// pasta) ANTES da linha do diário que os cita ser sincronizada: queda de energia depois do diário nunca deixa uma
    /// linha apontando para um blob vazio. Em paralelo, para a pincelada não esperar um `sync` de cada vez.
    pub fn append(&mut self, entry: &LogEntry, blobs: &mut BlobStore) -> CoreResult<()> {
        let dirty = blobs.take_dirty();
        let items: Vec<_> = dirty.iter().filter_map(|h| blobs.get(h).map(|b| (h.clone(), b))).collect();
        let written = {
            let store = &*blobs;
            write_blobs_parallel(&self.dir, &items, &|h| !store.is_on_disk(h))
        };
        if let Err(e) = written {
            // continuam pendentes: o próximo comando (ou o Salvar) tenta de novo
            for h in dirty {
                blobs.mark_dirty(h);
            }
            return Err(e);
        }
        for (h, _) in &items {
            blobs.mark_on_disk(h.clone());
            writeln!(self.file, "{}", serde_json::to_string(&WalLine::Blob { hash: h.0.clone() })?)?;
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
