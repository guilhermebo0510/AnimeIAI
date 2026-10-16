//! Sandbox de gravação: o MCP só escreve (PNG, projeto `.aiai`, renders) dentro de pastas permitidas.
//!
//! * `AIAI_MCP_ALLOWED_DIRS` (separador `;`) substitui a lista padrão;
//! * padrão: `C:\AnimeIAI\producoes`, `C:\AnimeIAI\calibracao`, `C:\AnimeIAI\sprints\registro\evidencias` e a pasta temporária do sistema;
//! * `AIAI_MCP_ALLOW_FS_WRITE=0|false` desliga toda gravação;
//! * leitura (`check_read`: referência, fonte, SoundFont, legenda): `C:\AnimeIAI` inteiro + as pastas de gravação;
//!   `AIAI_MCP_READ_DIRS` (separador `;`) substitui a pasta-base de leitura.
//!
//! Regras: caminho relativo resolve a partir do diretório atual e depois é conferido (nada de "redirecionar" para dentro
//! da pasta permitida em silêncio); `..` é recusado; a comparação usa a forma canônica do ancestral existente mais próximo
//! (symlink/junção que escapa é pego) e ignora maiúsculas no Windows; nome de arquivo com `:` (fluxo alternativo NTFS) é recusado.

use anyhow::{Result, bail};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Sandbox {
    dirs: Vec<PathBuf>,
    writes_enabled: bool,
    /// Pastas-base onde a LEITURA é permitida (referências, fontes, SoundFonts, legendas); as pastas de gravação também valem.
    read_dirs: Vec<PathBuf>,
}

/// Pastas permitidas quando `AIAI_MCP_ALLOWED_DIRS` não está definida.
pub fn default_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\AnimeIAI\producoes"),
        PathBuf::from(r"C:\AnimeIAI\calibracao"),
        PathBuf::from(r"C:\AnimeIAI\sprints\registro\evidencias"),
        std::env::temp_dir(),
    ]
}

/// Pastas-base de leitura quando `AIAI_MCP_READ_DIRS` não está definida.
pub fn default_read_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from(r"C:\AnimeIAI")]
}

fn writes_disabled(val: Option<&str>) -> bool {
    matches!(val, Some(v) if v == "0" || v.eq_ignore_ascii_case("false"))
}

impl Sandbox {
    pub fn new(dirs: Vec<PathBuf>, writes_enabled: bool) -> Self {
        Sandbox { dirs, writes_enabled, read_dirs: default_read_dirs() }
    }

    /// Troca as pastas-base de leitura (além das de gravação).
    pub fn with_read_dirs(mut self, read_dirs: Vec<PathBuf>) -> Self {
        self.read_dirs = read_dirs;
        self
    }

    pub fn from_env() -> Self {
        let dirs = match std::env::var("AIAI_MCP_ALLOWED_DIRS") {
            Ok(v) if !v.trim().is_empty() => v.split(';').map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from).collect(),
            _ => default_dirs(),
        };
        let sb = Sandbox::new(dirs, !writes_disabled(std::env::var("AIAI_MCP_ALLOW_FS_WRITE").ok().as_deref()));
        match std::env::var("AIAI_MCP_READ_DIRS") {
            Ok(v) if !v.trim().is_empty() => sb.with_read_dirs(v.split(';').map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from).collect()),
            _ => sb,
        }
    }

    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Valida um destino de gravação e devolve o caminho absoluto. Não cria nada.
    pub fn check_write(&self, input: &str) -> Result<PathBuf> {
        if !self.writes_enabled {
            bail!("gravação em disco desligada por AIAI_MCP_ALLOW_FS_WRITE=0 (destino: {input})");
        }
        if input.trim().is_empty() {
            bail!("caminho vazio");
        }
        if input.contains('\0') {
            bail!("caminho inválido (byte nulo)");
        }
        let p = PathBuf::from(input);
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("caminho recusado: '..' não é permitido ({input})");
        }
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if name.contains(':') {
                bail!("caminho recusado: nome com ':' ({input})");
            }
        }
        let abs = if p.is_absolute() || p.has_root() {
            if !p.is_absolute() {
                bail!("caminho recusado: informe a letra da unidade ({input})");
            }
            p
        } else {
            std::env::current_dir()?.join(p)
        };
        if !self.inside(&abs) {
            bail!("caminho {} fora das pastas permitidas {:?} (AIAI_MCP_ALLOWED_DIRS)", abs.display(), self.dirs);
        }
        Ok(abs)
    }

    /// Valida um caminho de LEITURA (absoluto, sem `..`, dentro de `C:\AnimeIAI` ou das pastas de gravação) e o devolve.
    pub fn check_read(&self, input: &str) -> Result<PathBuf> {
        if input.trim().is_empty() || input.contains('\0') {
            bail!("caminho vazio ou inválido");
        }
        let p = PathBuf::from(input);
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            bail!("caminho recusado: '..' não é permitido ({input})");
        }
        if !p.is_absolute() {
            bail!("leitura exige caminho absoluto com a letra da unidade ({input})");
        }
        let cand = norm(&canonical_lenient(&p));
        let ok = self.read_dirs.iter().chain(self.dirs.iter()).any(|d| cand.starts_with(norm(&canonical_lenient(d))));
        if !ok {
            bail!("leitura de {} fora das pastas permitidas {:?} e {:?} (AIAI_MCP_READ_DIRS / AIAI_MCP_ALLOWED_DIRS)", p.display(), self.read_dirs, self.dirs);
        }
        Ok(p)
    }

    fn inside(&self, p: &Path) -> bool {
        let cand = norm(&canonical_lenient(p));
        self.dirs.iter().any(|d| cand.starts_with(norm(&canonical_lenient(d))))
    }
}

/// Canonicaliza o ancestral existente mais próximo e reanexa o resto (o destino ainda pode não existir).
fn canonical_lenient(p: &Path) -> PathBuf {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (cur.file_name().map(|n| n.to_os_string()), cur.parent().map(|q| q.to_path_buf())) {
            (Some(name), Some(parent)) if !parent.as_os_str().is_empty() => {
                rest.push(name);
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// Forma comparável: no Windows ignora maiúsculas e a diferença `\\?\` entre canônico e não canônico.
fn norm(p: &Path) -> PathBuf {
    if cfg!(windows) {
        let s = p.to_string_lossy().to_lowercase();
        let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('/', "\\");
        PathBuf::from(s)
    } else {
        p.to_path_buf()
    }
}
