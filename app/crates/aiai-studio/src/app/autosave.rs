//! Autosave de projeto ainda sem pasta: a cada [`INTERVAL`] com alterações, uma cópia vai para
//! `%LOCALAPPDATA%\AnimeIAI\recovery\<início>-<pid>.aiai` numa thread à parte. Projeto com pasta já tem o WAL.
//! Saída normal apaga a cópia desta execução; depois de uma queda, a tela inicial oferece as cópias que sobraram.
//! Projeto com pasta: cópia do documento em `backup/` a cada [`DOC_INTERVAL`] com alterações, e a miniatura
//! `thumbnail.png` depois de salvar.

use aiai_core::{persist, BlobStore, Project};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) const INTERVAL: Duration = Duration::from_secs(120);

pub(crate) fn recovery_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|d| d.join("AnimeIAI").join("recovery"))
}

/// Cópias deixadas por execuções anteriores (mais nova primeiro), sem a desta execução.
pub(crate) fn leftovers(root: &Path, mine: Option<&Path>) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "aiai") && p.join(persist::PROJECT_FILE).is_file() && Some(p.as_path()) != mine)
        .map(|p| (std::fs::metadata(p.join(persist::PROJECT_FILE)).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

pub(crate) fn is_recovery(path: &Path) -> bool {
    recovery_root().is_some_and(|r| path.starts_with(r))
}

pub(crate) struct Autosave {
    /// Pasta desta execução (`None` = desligado, ex.: `--no-session`).
    pub(crate) dir: Option<PathBuf>,
    last: Instant,
    job: Option<JoinHandle<Result<(), String>>>,
    pub(crate) wrote: bool,
    /// Cópia periódica do documento do projeto COM pasta (`backup/project.json.auto-N.bak`), ver [`DOC_INTERVAL`].
    doc_last: Instant,
    doc_job: Option<JoinHandle<Result<(), String>>>,
    /// Miniatura `thumbnail.png` gravada depois de salvar.
    thumb_job: Option<JoinHandle<Result<(), String>>>,
    /// Intervalos das cópias (Preferências > Arquivo); começam nos padrões [`INTERVAL`] e [`DOC_INTERVAL`].
    interval: Duration,
    doc_interval: Duration,
}

/// Cópia do documento em memória enquanto há alterações não salvas num projeto com pasta (os blobs já estão no disco
/// pelo diário; a cópia é só o JSON).
pub(crate) const DOC_INTERVAL: Duration = Duration::from_secs(5 * 60);

impl Autosave {
    pub(crate) fn new(enabled: bool) -> Self {
        let dir = enabled.then(recovery_root).flatten().map(|r| {
            let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            r.join(format!("{unix}-{}.aiai", std::process::id()))
        });
        Autosave { dir, last: Instant::now(), job: None, wrote: false, doc_last: Instant::now(), doc_job: None, thumb_job: None, interval: INTERVAL, doc_interval: DOC_INTERVAL }
    }

    /// Intervalos das Preferências (minutos): da cópia de recuperação (sem pasta) e da do documento (com pasta).
    pub(crate) fn set_intervals(&mut self, recovery_minutes: u32, backup_minutes: u32) {
        self.interval = Duration::from_secs(recovery_minutes.max(1) as u64 * 60);
        self.doc_interval = Duration::from_secs(backup_minutes.max(1) as u64 * 60);
    }

    /// Chamado a cada quadro. `project_dir` = pasta do projeto quando ele tem pasta, trava e alterações não salvas.
    /// Devolve o erro da última cópia ou miniatura, se houve.
    pub(crate) fn tick_folder(&mut self, project_dir: Option<&Path>, project: &Project) -> Option<String> {
        let mut err = None;
        if self.doc_job.as_ref().is_some_and(|j| j.is_finished()) {
            match self.doc_job.take().map(|j| j.join()) {
                Some(Ok(Err(e))) => err = Some(format!("{}: {e}", crate::i18n::tr("msg.doc_backup_failed"))),
                Some(Err(_)) => err = Some(format!("{}: pânico", crate::i18n::tr("msg.doc_backup_failed"))),
                _ => {}
            }
        }
        if self.thumb_job.as_ref().is_some_and(|j| j.is_finished()) {
            match self.thumb_job.take().map(|j| j.join()) {
                Some(Ok(Err(e))) => err = Some(format!("{}: {e}", crate::i18n::tr("msg.thumb_failed"))),
                Some(Err(_)) => err = Some(format!("{}: pânico", crate::i18n::tr("msg.thumb_failed"))),
                _ => {}
            }
        }
        match project_dir {
            // desligado (execuções de teste/captura) ou nada a guardar: o relógio recomeça (a 1ª cópia vem 5 min depois
            // das primeiras alterações, não logo na primeira)
            None => self.doc_last = Instant::now(),
            Some(_) if self.dir.is_none() => self.doc_last = Instant::now(),
            Some(dir) => {
                if self.doc_job.is_none() && self.doc_last.elapsed() >= self.doc_interval {
                    self.doc_last = Instant::now();
                    let (p, dir) = (project.clone(), dir.to_path_buf());
                    self.doc_job = Some(std::thread::spawn(move || persist::backup_document(&dir, &p).map(|_| ()).map_err(|e| e.to_string())));
                }
            }
        }
        err
    }

    /// Depois de salvar: miniatura do 1º quadro do 1º corte em `thumbnail.png`, noutra thread (render inteiro + redução).
    pub(crate) fn spawn_thumbnail(&mut self, project: &Project, blobs: &BlobStore, dir: &Path) {
        if self.thumb_job.is_some() {
            return; // a anterior ainda não foi recolhida (o resultado dela é avisado); a próxima gravação refaz
        }
        let (p, b, dir) = (project.clone(), blobs.clone(), dir.to_path_buf());
        self.thumb_job = Some(std::thread::spawn(move || write_thumbnail(&p, &b, &dir)));
    }

    /// Chamado a cada quadro. `needed` = projeto sem pasta e com alterações. Devolve o erro da última cópia, se houve.
    pub(crate) fn tick(&mut self, needed: bool, project: &Project, blobs: &BlobStore) -> Option<String> {
        let mut err = None;
        if self.job.as_ref().is_some_and(|j| j.is_finished()) {
            match self.job.take().map(|j| j.join()) {
                Some(Ok(Ok(()))) => self.wrote = true,
                Some(Ok(Err(e))) => err = Some(e),
                Some(Err(_)) => err = Some("autosave entrou em pânico".into()),
                None => {}
            }
        }
        let Some(dir) = self.dir.clone() else { return err };
        if needed && self.job.is_none() && self.last.elapsed() >= self.interval {
            self.last = Instant::now();
            let (p, b) = (project.clone(), blobs.clone());
            self.job = Some(std::thread::spawn(move || persist::snapshot(&p, &b, &dir).map_err(|e| e.to_string())));
        }
        err
    }

    /// O projeto ganhou pasta ou a sessão terminou bem: a cópia desta execução não serve mais.
    pub(crate) fn discard(&mut self) {
        if let Some(j) = self.job.take() {
            let _ = j.join();
        }
        if let Some(d) = &self.dir {
            if self.wrote {
                let _ = std::fs::remove_dir_all(d);
            }
        }
        self.wrote = false;
        self.last = Instant::now();
    }
}

/// Render do 1º quadro do 1º corte (câmera, fundo branco) reduzido para [`persist::THUMBNAIL_SIZE`] no lado maior.
/// Projeto sem corte não tem o que mostrar: nada é gravado.
fn write_thumbnail(project: &Project, blobs: &BlobStore, dir: &Path) -> Result<(), String> {
    let Some(cut) = super::commands::first_cut(project) else { return Ok(()) };
    let (w, h) = (project.settings.width.max(1), project.settings.height.max(1));
    let max_w = if w >= h { persist::THUMBNAIL_SIZE } else { (persist::THUMBNAIL_SIZE * w / h).max(1) };
    let img = aiai_tools::production::cut_thumbnail(project, blobs, &cut, 0, max_w).map_err(|e| e.to_string())?;
    let png = img.encode_png()?;
    persist::write_file_atomic(&dir.join(persist::THUMBNAIL_FILE), &png).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrips_and_leftovers_lists_newest_first() {
        let root = std::env::temp_dir().join(format!("aiai-autosave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = aiai_core::CoreSession::for_tests("rec");
        let b = s.blobs.put(vec![3; 500]);
        let _ = b;
        persist::snapshot(&s.project, &s.blobs, &root.join("1-1.aiai")).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        persist::snapshot(&s.project, &s.blobs, &root.join("2-2.aiai")).unwrap();
        let l = leftovers(&root, Some(&root.join("2-2.aiai")));
        assert_eq!(l, vec![root.join("1-1.aiai")], "a desta execução fica de fora");
        let (o, rep) = persist::open(&root.join("1-1.aiai")).unwrap();
        assert_eq!((o.project.name.as_str(), rep.replayed), ("rec", 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn thumbnail_is_first_frame_of_first_cut_256_on_long_side() {
        use aiai_core::{Command, CutData, EpisodeData, NodeKind, SceneData};
        let dir = std::env::temp_dir().join(format!("aiai-thumb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = aiai_core::CoreSession::for_tests("thumb");
        write_thumbnail(&s.project, &s.blobs, &dir).unwrap();
        assert!(!dir.join(persist::THUMBNAIL_FILE).exists(), "sem corte, sem miniatura");
        let ep = s.apply(Command::CreateNode { id: None, parent: None, index: None, name: "EP".into(), data: NodeKind::Episode(EpisodeData { number: 1 }) }).unwrap().created[0].clone();
        let sc = s.apply(Command::CreateNode { id: None, parent: Some(ep), index: None, name: "S".into(), data: NodeKind::Scene(SceneData::default()) }).unwrap().created[0].clone();
        s.apply(Command::CreateNode { id: None, parent: Some(sc), index: None, name: "C".into(), data: NodeKind::Cut(CutData::new(12)) }).unwrap();
        write_thumbnail(&s.project, &s.blobs, &dir).unwrap();
        let img = aiai_paint::Image::load_png(&dir.join(persist::THUMBNAIL_FILE)).unwrap();
        let (w, h) = (s.project.settings.width, s.project.settings.height);
        assert_eq!(img.width.max(img.height), persist::THUMBNAIL_SIZE, "{}x{} de {w}x{h}", img.width, img.height);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
