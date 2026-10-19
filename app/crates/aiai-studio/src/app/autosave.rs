//! Autosave de projeto ainda sem pasta: a cada [`INTERVAL`] com alterações, uma cópia vai para
//! `%LOCALAPPDATA%\AnimeIAI\recovery\<início>-<pid>.aiai` numa thread à parte. Projeto com pasta já tem o WAL.
//! Saída normal apaga a cópia desta execução; depois de uma queda, a tela inicial oferece as cópias que sobraram.

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
}

impl Autosave {
    pub(crate) fn new(enabled: bool) -> Self {
        let dir = enabled.then(recovery_root).flatten().map(|r| {
            let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            r.join(format!("{unix}-{}.aiai", std::process::id()))
        });
        Autosave { dir, last: Instant::now(), job: None, wrote: false }
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
        if needed && self.job.is_none() && self.last.elapsed() >= INTERVAL {
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
}
