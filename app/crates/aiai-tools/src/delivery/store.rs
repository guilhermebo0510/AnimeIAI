//! Estado da Entrega guardado no projeto (`entrega.json` na pasta `.aiai`, fora do `project.json`: não gera revisão nem
//! desfazer): os destinos, a fila de exportação (reabre e oferece retomar) e a biblioteca de entregáveis (o que já foi
//! exportado, com miniatura e o pedido original para exportar de novo). Miniaturas e pôsteres ficam em
//! `entrega-miniaturas/`. Apagar um entregável move os arquivos para `_Lixeira` ao lado deles.
//!
//! Tudo aqui é puro (sem render nem ffmpeg): o app e a bridge aplicam as mesmas operações e gravam o mesmo arquivo.

use super::formats::FormatPreset;
use super::names::{self, DEFAULT_TEMPLATE};
use super::{SourceRef, name_ctx};
use crate::context::{ToolError, ToolResult};
use aiai_core::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "entrega.json";
pub const THUMBS_DIR: &str = "entrega-miniaturas";
pub const TRASH_DIR: &str = "_Lixeira";
pub const VERSION: u32 = 1;

/// Onde as entregas são gravadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dest {
    /// `saida` ao lado da pasta do projeto.
    #[default]
    Project,
    /// `Documentos\AnimeIAI\Exportações\<projeto>`.
    Documents,
    /// Uma pasta escolhida pelo usuário.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub dest: Dest,
    /// Pasta quando `dest` é `custom`.
    pub custom_dir: String,
    /// Modelo de nome com `{projeto}`, `{episodio}`, `{corte}`, `{formato}`, `{data}`, `{hora}`, `{fonte}`, `{versao}`.
    pub name_template: String,
    /// Acrescenta `_v001`, `_v002`... quando o modelo não tem `{versao}`: nunca sobrescreve uma entrega anterior.
    pub auto_version: bool,
    /// Pastas extras que recebem uma cópia de cada entrega (outra unidade, pasta sincronizada).
    pub extra_dirs: Vec<String>,
    /// Gera o pacote de entrega (zip) ao concluir cada exportação.
    pub package: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { dest: Dest::Project, custom_dir: String::new(), name_template: DEFAULT_TEMPLATE.into(), auto_version: true, extra_dirs: Vec::new(), package: false }
    }
}

impl Settings {
    /// Pasta de saída. O projeto precisa ter pasta (`project_dir`); sem ela não há onde gravar.
    pub fn out_dir(&self, project_dir: &Path, project_name: &str) -> PathBuf {
        let default = || project_dir.parent().map(Path::to_path_buf).unwrap_or_else(|| project_dir.to_path_buf()).join("saida");
        match self.dest {
            Dest::Project => default(),
            Dest::Documents => aiai_paint::userdirs::animeiai_dir().map(|d| d.join("Exportações").join(names::safe_name(project_name))).unwrap_or_else(default),
            Dest::Custom if self.custom_dir.trim().is_empty() => default(),
            Dest::Custom => PathBuf::from(self.custom_dir.trim()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        names::validate_template(&self.name_template)?;
        if self.dest == Dest::Custom && !Path::new(self.custom_dir.trim()).is_absolute() {
            return Err("a pasta de entrega precisa ser um caminho absoluto (com a letra da unidade)".into());
        }
        for d in &self.extra_dirs {
            if !Path::new(d.trim()).is_absolute() {
                return Err(format!("a pasta extra \"{d}\" precisa ser um caminho absoluto"));
            }
        }
        Ok(())
    }
}

/// O pedido de uma exportação (o que a fila guarda e o que "exportar de novo" repete).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobSpec {
    pub id: u64,
    pub source: SourceRef,
    /// Nome da fonte quando foi pedido (para mostrar sem abrir o documento).
    pub source_name: String,
    /// Id da predefinição (`tv`, `youtube`...).
    pub preset: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crf: Option<u32>,
    /// Arquivo de saída (em sequência de imagens, o padrão com `%05d`).
    pub out: String,
    /// Pastas que recebem cópia ao concluir.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_dirs: Vec<String>,
    /// Gera o pacote de entrega (zip) ao concluir.
    #[serde(default)]
    pub package: bool,
    /// Quadro do pôster/miniatura (padrão: o do meio da fonte).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_frame: Option<u32>,
    /// Revisão do projeto quando foi pedido.
    #[serde(default)]
    pub revision: u64,
    pub created_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Done,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JobRecord {
    pub spec: JobSpec,
    pub status: Status,
    /// Motivo da falha (texto do motor, sem resumir) ou aviso de retomada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Quantas vezes já foi tentada.
    #[serde(default)]
    pub attempts: u32,
    /// Duração da última execução (s).
    #[serde(default)]
    pub secs: f32,
    /// Foi interrompida (o app fechou no meio) e voltou para a fila ao reabrir.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resumed: bool,
}

/// Resultado de uma exportação concluída.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct JobResult {
    /// Arquivo principal primeiro; depois as legendas ao lado. Em sequência de imagens, o primeiro quadro.
    pub files: Vec<String>,
    #[serde(default)]
    pub is_sequence: bool,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_s: f64,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lufs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_peak: Option<f64>,
    /// Miniatura (arquivo dentro de `entrega-miniaturas/`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    /// Cópias nas pastas extras.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub copies: Vec<String>,
    /// Pacote de entrega (zip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Pôster gerado junto (para o pacote); passa para o entregável ao concluir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<PosterInfo>,
}

/// Pôster feito no editor (quadro escolhido e título sobre a imagem).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PosterInfo {
    pub file: String,
    pub frame: u32,
    pub title: String,
}

/// Uma entrega pronta na biblioteca de entregáveis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Deliverable {
    pub spec: JobSpec,
    pub result: JobResult,
    pub finished_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<PosterInfo>,
    /// O arquivo principal não está mais no disco (calculado ao carregar; não é guardado).
    #[serde(skip)]
    pub missing: bool,
}

impl Deliverable {
    pub fn id(&self) -> u64 {
        self.spec.id
    }
    pub fn main_file(&self) -> Option<&str> {
        self.result.files.first().map(String::as_str)
    }
    /// Pasta que contém a entrega (a da sequência de imagens, ou a do arquivo).
    pub fn folder(&self) -> Option<PathBuf> {
        self.main_file().and_then(|f| Path::new(f).parent().map(Path::to_path_buf))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub settings: Settings,
    pub next_id: u64,
    /// Fila parada: nada novo começa até retomar (reabrir com itens pendentes deixa parada de propósito).
    pub paused: bool,
    pub jobs: Vec<JobRecord>,
    pub deliverables: Vec<Deliverable>,
}

impl Default for Store {
    fn default() -> Self {
        Store { version: VERSION, settings: Settings::default(), next_id: 1, paused: false, jobs: Vec::new(), deliverables: Vec::new() }
    }
}

/// Resultado de abrir o arquivo: o estado e, se o arquivo estava ilegível, o aviso (o ruim é guardado ao lado, nunca apagado).
#[derive(Debug)]
pub struct Loaded {
    pub store: Store,
    pub warning: Option<String>,
}

fn io_err(what: &str, p: &Path, e: std::io::Error) -> ToolError {
    ToolError::Invalid(format!("{what} {}: {e}", p.display()))
}

impl Store {
    pub fn file(project_dir: &Path) -> PathBuf {
        project_dir.join(FILE)
    }

    pub fn thumbs_dir(project_dir: &Path) -> PathBuf {
        project_dir.join(THUMBS_DIR)
    }

    /// Lê `entrega.json` (ausente = estado novo). Ilegível ou de versão mais nova: erro com o motivo.
    pub fn read(project_dir: &Path) -> ToolResult<Store> {
        let path = Self::file(project_dir);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
            Err(e) => return Err(io_err("ler", &path, e)),
        };
        let mut s: Store = serde_json::from_str(&text).map_err(|e| ToolError::Invalid(format!("{} ilegível: {e}", path.display())))?;
        if s.version > VERSION {
            return Err(ToolError::Invalid(format!("{} é da versão {} (este app entende até a {VERSION})", path.display(), s.version)));
        }
        s.version = VERSION;
        s.refresh_missing();
        Ok(s)
    }

    /// Como [`Store::read`], mas um arquivo ilegível vira estado novo: o ruim é renomeado `entrega.json.ruim-<ms>` (nada é
    /// perdido em silêncio) e o aviso diz o que houve.
    pub fn load(project_dir: &Path, now_ms: u64) -> Loaded {
        match Self::read(project_dir) {
            Ok(store) => Loaded { store, warning: None },
            Err(e) => {
                let path = Self::file(project_dir);
                let kept = path.with_extension(format!("json.ruim-{now_ms}"));
                let note = match std::fs::rename(&path, &kept) {
                    Ok(()) => format!("{e}; o arquivo foi guardado em {}", kept.display()),
                    Err(re) => format!("{e}; não consegui guardar uma cópia ({re})"),
                };
                Loaded { store: Store::default(), warning: Some(note) }
            }
        }
    }

    /// Grava por arquivo temporário + renomear (queda no meio não deixa o arquivo pela metade).
    pub fn save(&self, project_dir: &Path) -> ToolResult<()> {
        let path = Self::file(project_dir);
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(|e| ToolError::Invalid(e.to_string()))?;
        std::fs::write(&tmp, text).map_err(|e| io_err("gravar", &tmp, e))?;
        std::fs::rename(&tmp, &path).map_err(|e| io_err("gravar", &path, e))
    }

    pub fn refresh_missing(&mut self) {
        for d in &mut self.deliverables {
            d.missing = d.main_file().is_none_or(|f| !Path::new(f).exists());
        }
    }

    /// Reabriu o projeto: o que estava rodando voltou para a fila (marcado como retomado) e, havendo pendentes, a fila
    /// fica parada até o usuário retomar.
    pub fn on_reopen(&mut self) {
        for j in &mut self.jobs {
            if j.status == Status::Running {
                j.status = Status::Queued;
                j.resumed = true;
            }
        }
        if self.jobs.iter().any(|j| j.status == Status::Queued) {
            self.paused = true;
        }
    }

    pub fn pending(&self) -> usize {
        self.jobs.iter().filter(|j| j.status == Status::Queued).count()
    }

    // ------------------------------------------------------------------------------------------ nomes

    /// Pedido pronto para entrar na fila: caminho de saída pelo modelo de nome (versão livre na pasta e na fila).
    pub fn plan(&self, p: &Project, project_dir: &Path, source: &SourceRef, preset: &FormatPreset, crf: Option<u32>, now_ms: u64) -> ToolResult<JobSpec> {
        self.settings.validate().map_err(ToolError::Invalid)?;
        let info = source.info(p)?;
        let dir = self.settings.out_dir(project_dir, &p.name);
        let ctx = name_ctx(p, &info, preset.id, now_ms);
        // sequência de imagens e stems saem numa pasta própria (a versão está no nome da pasta)
        let sequence = matches!(preset.kind, super::formats::Kind::PngSequence | super::formats::Kind::Stems);
        let reserved: Vec<PathBuf> = self.jobs.iter().filter(|j| matches!(j.status, Status::Queued | Status::Running)).map(|j| PathBuf::from(&j.spec.out)).chain(self.deliverables.iter().filter_map(|d| d.main_file().map(PathBuf::from))).collect();
        let out = names::resolve_output(&dir, &self.settings.name_template, &ctx, preset.ext, self.settings.auto_version, sequence, &reserved);
        Ok(JobSpec {
            id: 0,
            source: source.clone(),
            source_name: info.name,
            preset: preset.id.to_owned(),
            crf,
            out: out.display().to_string(),
            extra_dirs: self.settings.extra_dirs.clone(),
            package: self.settings.package,
            poster_frame: None,
            revision: p.revision,
            created_ms: now_ms,
        })
    }

    // ------------------------------------------------------------------------------------------ fila

    /// Põe no fim da fila e devolve o id.
    pub fn add_job(&mut self, mut spec: JobSpec) -> u64 {
        spec.id = self.next_id;
        self.next_id += 1;
        let id = spec.id;
        self.jobs.push(JobRecord { spec, status: Status::Queued, error: None, attempts: 0, secs: 0.0, resumed: false });
        id
    }

    pub fn job(&self, id: u64) -> Option<&JobRecord> {
        self.jobs.iter().find(|j| j.spec.id == id)
    }

    fn job_mut(&mut self, id: u64) -> Option<&mut JobRecord> {
        self.jobs.iter_mut().find(|j| j.spec.id == id)
    }

    /// Tira da fila (o que está rodando não sai: cancele primeiro).
    pub fn remove_job(&mut self, id: u64) -> Result<(), String> {
        match self.job(id).map(|j| j.status) {
            None => Err(format!("item {id} não está na fila")),
            Some(Status::Running) => Err("o item está exportando: cancele antes de tirar".into()),
            Some(_) => {
                self.jobs.retain(|j| j.spec.id != id);
                Ok(())
            }
        }
    }

    /// Muda a posição de um item que ainda não começou (`to` = índice na lista; o que roda e o que terminou não mudam de lugar).
    pub fn move_job(&mut self, id: u64, to: usize) -> Result<(), String> {
        let from = self.jobs.iter().position(|j| j.spec.id == id).ok_or_else(|| format!("item {id} não está na fila"))?;
        if self.jobs[from].status != Status::Queued {
            return Err("só dá para reordenar itens que ainda estão na fila".into());
        }
        // o menor índice permitido é o logo depois do último item que já rodou/roda
        let first = self.jobs.iter().position(|j| j.status == Status::Queued).unwrap_or(0);
        let to = to.clamp(first, self.jobs.len() - 1);
        let j = self.jobs.remove(from);
        self.jobs.insert(to.min(self.jobs.len()), j);
        Ok(())
    }

    /// Item na fila que sobe/desce `delta` posições entre os itens ainda na fila.
    pub fn nudge_job(&mut self, id: u64, delta: i32) -> Result<(), String> {
        let queued: Vec<usize> = self.jobs.iter().enumerate().filter(|(_, j)| j.status == Status::Queued).map(|(i, _)| i).collect();
        let pos = queued.iter().position(|&i| self.jobs[i].spec.id == id).ok_or("só dá para reordenar itens que ainda estão na fila")?;
        let to = (pos as i32 + delta).clamp(0, queued.len() as i32 - 1) as usize;
        self.move_job(id, queued[to])
    }

    /// Próximo item a rodar (o primeiro na fila), se a fila não estiver parada.
    pub fn next_runnable(&self) -> Option<u64> {
        if self.paused || self.jobs.iter().any(|j| j.status == Status::Running) {
            return None;
        }
        self.jobs.iter().find(|j| j.status == Status::Queued).map(|j| j.spec.id)
    }

    pub fn mark_running(&mut self, id: u64) {
        if let Some(j) = self.job_mut(id) {
            j.status = Status::Running;
            j.attempts += 1;
            j.error = None;
        }
    }

    /// Cancela um item que ainda está na fila (o que roda é cancelado pelo executor, que depois chama [`Store::finish_canceled`]).
    pub fn cancel_queued(&mut self, id: u64) -> bool {
        match self.job_mut(id) {
            Some(j) if j.status == Status::Queued => {
                j.status = Status::Canceled;
                true
            }
            _ => false,
        }
    }

    pub fn finish_canceled(&mut self, id: u64, secs: f32) {
        if let Some(j) = self.job_mut(id) {
            j.status = Status::Canceled;
            j.secs = secs;
        }
    }

    pub fn finish_err(&mut self, id: u64, msg: String, secs: f32) {
        if let Some(j) = self.job_mut(id) {
            j.status = Status::Failed;
            j.error = Some(msg);
            j.secs = secs;
        }
    }

    /// Concluída: vira entregável (o pedido original fica junto, para exportar de novo).
    pub fn finish_ok(&mut self, id: u64, mut result: JobResult, secs: f32, now_ms: u64) {
        let Some(j) = self.job_mut(id) else { return };
        j.status = Status::Done;
        j.secs = secs;
        j.error = None;
        let spec = j.spec.clone();
        self.deliverables.retain(|d| d.spec.id != id);
        let poster = result.poster.take();
        self.deliverables.push(Deliverable { spec, result, finished_ms: now_ms, poster, missing: false });
    }

    /// Tenta de novo um item que falhou ou foi cancelado (vai para o fim da fila).
    pub fn retry(&mut self, id: u64) -> Result<(), String> {
        let from = self.jobs.iter().position(|j| j.spec.id == id).ok_or_else(|| format!("item {id} não está na fila"))?;
        if !matches!(self.jobs[from].status, Status::Failed | Status::Canceled) {
            return Err("só dá para repetir um item que falhou ou foi cancelado".into());
        }
        let mut j = self.jobs.remove(from);
        j.status = Status::Queued;
        j.error = None;
        j.resumed = false;
        self.jobs.push(j);
        Ok(())
    }

    /// Limpa os itens terminados (concluídos, falhos e cancelados); os entregáveis ficam na biblioteca.
    pub fn clear_finished(&mut self) -> usize {
        let n = self.jobs.len();
        self.jobs.retain(|j| matches!(j.status, Status::Queued | Status::Running));
        n - self.jobs.len()
    }

    // ------------------------------------------------------------------------------------------ entregáveis

    pub fn deliverable(&self, id: u64) -> Option<&Deliverable> {
        self.deliverables.iter().find(|d| d.spec.id == id)
    }

    pub fn deliverable_mut(&mut self, id: u64) -> Option<&mut Deliverable> {
        self.deliverables.iter_mut().find(|d| d.spec.id == id)
    }

    /// Tira da biblioteca e move os arquivos (vídeo, legendas, cópias do pacote e miniatura) para `_Lixeira` na pasta de
    /// cada um. Nunca apaga de vez. Devolve onde ficaram.
    pub fn trash_deliverable(&mut self, id: u64, project_dir: &Path, now_ms: u64) -> Result<Vec<PathBuf>, String> {
        let d = self.deliverable(id).ok_or_else(|| format!("entregável {id} não existe"))?.clone();
        let stamp = {
            let (date, time) = names::date_time_of(now_ms);
            format!("{date}_{time}")
        };
        let mut moved = Vec::new();
        let mut todo: Vec<PathBuf> = Vec::new();
        if d.result.is_sequence {
            // sequência de imagens: a pasta inteira
            if let Some(dir) = d.folder() {
                todo.push(dir);
            }
        } else {
            todo.extend(d.result.files.iter().map(PathBuf::from));
        }
        todo.extend(d.result.package.iter().map(PathBuf::from));
        if let Some(p) = &d.poster {
            todo.push(PathBuf::from(&p.file));
        }
        for src in todo {
            if !src.exists() {
                continue;
            }
            let parent = src.parent().ok_or_else(|| format!("{} sem pasta", src.display()))?;
            let trash = parent.join(TRASH_DIR);
            std::fs::create_dir_all(&trash).map_err(|e| format!("criar {}: {e}", trash.display()))?;
            let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mut dst = trash.join(format!("{stamp}_{name}"));
            let mut k = 1;
            while dst.exists() {
                dst = trash.join(format!("{stamp}_{k}_{name}"));
                k += 1;
            }
            std::fs::rename(&src, &dst).map_err(|e| format!("mover {} para {}: {e}", src.display(), dst.display()))?;
            moved.push(dst);
        }
        if let Some(t) = &d.result.thumbnail {
            let _ = std::fs::remove_file(Self::thumbs_dir(project_dir).join(t));
        }
        self.deliverables.retain(|x| x.spec.id != id);
        Ok(moved)
    }
}

/// Matriz de exportação: cada fonte em cada formato (fonte por fonte, formatos na ordem pedida).
pub fn matrix(sources: &[SourceRef], presets: &[String]) -> Vec<(SourceRef, String)> {
    sources.iter().flat_map(|s| presets.iter().map(move |p| (s.clone(), p.clone()))).collect()
}

/// Tempo em segundos "1 h 02 min", "3 min 05 s", "42 s" para a fila.
pub fn fmt_duration(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{} h {:02} min", s / 3600, (s / 60) % 60)
    } else if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

/// Tamanho de arquivo legível (KB/MB/GB).
pub fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::super::formats::preset;
    use super::super::testkit::session_with_cut;
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai_store_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn spec(out: &str) -> JobSpec {
        JobSpec { id: 0, source: SourceRef::cut(aiai_core::Id("c1".into())), source_name: "C001".into(), preset: "tv".into(), crf: None, out: out.into(), extra_dirs: vec![], package: false, poster_frame: None, revision: 3, created_ms: 1 }
    }

    fn done_result(file: &str) -> JobResult {
        JobResult { files: vec![file.into()], frames: 24, width: 64, height: 36, fps: 24.0, duration_s: 1.0, size_bytes: 10, ..Default::default() }
    }

    #[test]
    fn the_queue_survives_closing_and_reopening() {
        let dir = tmpdir("persist");
        let mut s = Store::default();
        let a = s.add_job(spec("C:\\x\\a.mp4"));
        let b = s.add_job(spec("C:\\x\\b.mp4"));
        let c = s.add_job(spec("C:\\x\\c.mp4"));
        s.mark_running(a);
        s.finish_ok(b, done_result("C:\\x\\b.mp4"), 2.0, 5);
        s.save(&dir).unwrap();
        assert!(!Store::file(&dir).with_extension("json.tmp").exists());
        // reabre: o que rodava volta para a fila, marcado, e a fila fica parada esperando o usuário
        let mut r = Store::read(&dir).unwrap();
        assert_eq!(r.jobs.len(), 3);
        r.on_reopen();
        assert!(r.paused);
        let ja = r.job(a).unwrap();
        assert_eq!((ja.status, ja.resumed, ja.attempts), (Status::Queued, true, 1));
        assert_eq!(r.job(b).unwrap().status, Status::Done);
        assert_eq!(r.deliverables.len(), 1);
        assert_eq!(r.next_runnable(), None, "parada: nada começa sozinho");
        r.paused = false;
        assert_eq!(r.next_runnable(), Some(a), "retomar recomeça pelo primeiro");
        r.mark_running(a);
        assert_eq!(r.next_runnable(), None, "um por vez");
        // sem itens pendentes a fila não fica parada
        let mut empty = Store::default();
        empty.on_reopen();
        assert!(!empty.paused);
        let _ = c;
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn queue_operations_reorder_cancel_and_retry() {
        let mut s = Store::default();
        let ids: Vec<u64> = (0..4).map(|i| s.add_job(spec(&format!("C:\\x\\{i}.mp4")))).collect();
        s.mark_running(ids[0]);
        // reordenar só entre os que ainda estão na fila
        s.nudge_job(ids[3], -1).unwrap();
        assert_eq!(s.jobs.iter().map(|j| j.spec.id).collect::<Vec<_>>(), vec![ids[0], ids[1], ids[3], ids[2]]);
        s.nudge_job(ids[3], -5).unwrap();
        assert_eq!(s.jobs[1].spec.id, ids[3], "não passa na frente do que está rodando");
        assert!(s.move_job(ids[0], 3).is_err(), "o que roda não muda de lugar");
        assert!(s.remove_job(ids[0]).is_err());
        // cancelar na fila e repetir
        assert!(s.cancel_queued(ids[1]));
        assert!(!s.cancel_queued(ids[0]));
        s.finish_err(ids[2], "ffmpeg falhou: disco cheio".into(), 3.0);
        assert_eq!(s.job(ids[2]).unwrap().error.as_deref(), Some("ffmpeg falhou: disco cheio"));
        s.retry(ids[2]).unwrap();
        let j = s.job(ids[2]).unwrap();
        assert_eq!((j.status, j.error.clone()), (Status::Queued, None));
        assert_eq!(s.jobs.last().unwrap().spec.id, ids[2], "volta para o fim da fila");
        assert!(s.retry(ids[3]).is_err(), "só falho ou cancelado");
        s.retry(ids[1]).unwrap();
        // limpar tira o que terminou e deixa fila e execução
        s.finish_canceled(ids[0], 1.0);
        s.finish_ok(ids[3], done_result("C:\\x\\3.mp4"), 1.0, 9);
        assert_eq!(s.clear_finished(), 2);
        assert_eq!(s.jobs.len(), 2);
        assert_eq!(s.deliverables.len(), 1, "o entregável fica na biblioteca");
    }

    #[test]
    fn matrix_is_every_source_in_every_format() {
        let a = SourceRef::cut(aiai_core::Id("a".into()));
        let b = SourceRef::edit(aiai_core::Id("b".into()));
        let m = matrix(&[a.clone(), b.clone()], &["tv".into(), "preview".into(), "wav".into()]);
        assert_eq!(m.len(), 6);
        assert_eq!((m[0].0.clone(), m[0].1.as_str()), (a, "tv"));
        assert_eq!((m[5].0.clone(), m[5].1.as_str()), (b, "wav"));
    }

    #[test]
    fn plan_names_files_by_the_template_with_versions_and_the_queue() {
        let (s, ep, cut) = session_with_cut(24);
        let dir = tmpdir("plan");
        let project_dir = dir.join("rosa.aiai");
        let mut st = Store::default();
        st.settings.name_template = "{projeto}_{episodio}_{corte}_{formato}_{data}".into();
        let src = SourceRef::cut(cut.clone());
        let tv = preset("tv").unwrap();
        let now = super::super::testkit::civil_ms(2026, 10, 20, 14, 5);
        let a = st.plan(&s.project, &project_dir, &src, tv, None, now).unwrap();
        let out = PathBuf::from(&a.out);
        assert_eq!(out.parent().unwrap(), dir.join("saida"), "por padrão `saida` ao lado do projeto");
        assert_eq!(out.file_name().unwrap().to_string_lossy(), "Entrega_Ep 1_C000_tv_2026-10-20_v001.mp4");
        st.add_job(a);
        // o item da fila já reserva a v001
        let b = st.plan(&s.project, &project_dir, &src, tv, None, now).unwrap();
        assert!(b.out.ends_with("_v002.mp4"), "{}", b.out);
        // formato diferente tem a sua própria contagem
        let c = st.plan(&s.project, &project_dir, &src, preset("png").unwrap(), None, now).unwrap();
        assert!(c.out.ends_with("_v001_%05d.png") && c.out.contains("png_2026-10-20_v001"), "{}", c.out);
        // episódio e edição
        let e = st.plan(&s.project, &project_dir, &SourceRef::episode(ep), preset("web").unwrap(), None, now).unwrap();
        assert!(e.out.contains("Entrega_Ep 1_web_2026-10-20_v001"), "{}", e.out);
        // modelo inválido é recusado antes de entrar na fila
        st.settings.name_template = "{nada}".into();
        assert!(st.plan(&s.project, &project_dir, &src, tv, None, now).is_err());
        // pasta personalizada
        st.settings.name_template = "{corte}".into();
        st.settings.dest = Dest::Custom;
        st.settings.custom_dir = dir.join("outra").display().to_string();
        assert!(st.plan(&s.project, &project_dir, &src, tv, None, now).unwrap().out.starts_with(&st.settings.custom_dir));
        st.settings.custom_dir = "relativo".into();
        assert!(st.plan(&s.project, &project_dir, &src, tv, None, now).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_files_are_kept_aside_and_never_hidden() {
        let dir = tmpdir("bad");
        std::fs::write(Store::file(&dir), "{ não é json").unwrap();
        assert!(Store::read(&dir).is_err());
        let l = Store::load(&dir, 123);
        assert!(l.store.jobs.is_empty());
        let w = l.warning.unwrap();
        assert!(w.contains("ilegível") && w.contains("ruim-123"), "{w}");
        assert!(dir.join("entrega.json.ruim-123").exists() && !Store::file(&dir).exists());
        // versão mais nova que a do app: erro, nada é sobrescrito sem aviso
        std::fs::write(Store::file(&dir), r#"{"version": 99}"#).unwrap();
        assert!(Store::read(&dir).unwrap_err().to_string().contains("versão 99"));
        // arquivo ausente = estado novo, sem aviso
        let none = tmpdir("none");
        let l = Store::load(&none, 1);
        assert!(l.warning.is_none() && l.store.next_id == 1);
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(none);
    }

    #[test]
    fn deleting_a_deliverable_moves_it_to_the_trash_folder() {
        let dir = tmpdir("trash");
        let out = dir.join("saida");
        std::fs::create_dir_all(&out).unwrap();
        let mp4 = out.join("a_v001.mp4");
        let srt = out.join("a_v001.srt");
        std::fs::write(&mp4, b"video").unwrap();
        std::fs::write(&srt, b"srt").unwrap();
        let thumbs = Store::thumbs_dir(&dir);
        std::fs::create_dir_all(&thumbs).unwrap();
        std::fs::write(thumbs.join("1.png"), b"png").unwrap();
        let mut s = Store::default();
        let id = s.add_job(spec(&mp4.display().to_string()));
        let mut r = done_result(&mp4.display().to_string());
        r.files.push(srt.display().to_string());
        r.thumbnail = Some("1.png".into());
        s.finish_ok(id, r, 1.0, 1_760_000_000_000);
        s.refresh_missing();
        assert!(!s.deliverable(id).unwrap().missing);
        let moved = s.trash_deliverable(id, &dir, 1_760_000_000_000).unwrap();
        assert_eq!(moved.len(), 2);
        assert!(!mp4.exists() && !srt.exists() && !thumbs.join("1.png").exists());
        assert!(moved.iter().all(|m| m.parent().unwrap() == out.join(TRASH_DIR)));
        assert_eq!(std::fs::read(&moved[0]).unwrap(), b"video", "o conteúdo é o mesmo: nada foi apagado");
        assert!(s.deliverables.is_empty());
        assert!(s.trash_deliverable(id, &dir, 1).is_err());
        // arquivo que sumiu por fora aparece como ausente
        let id2 = s.add_job(spec("C:\\nada\\x.mp4"));
        s.finish_ok(id2, done_result("C:\\nada\\x.mp4"), 1.0, 1);
        s.refresh_missing();
        assert!(s.deliverable(id2).unwrap().missing);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn durations_and_sizes_read_well() {
        assert_eq!(fmt_duration(42.0), "42 s");
        assert_eq!(fmt_duration(185.0), "3 min 05 s");
        assert_eq!(fmt_duration(3720.0), "1 h 02 min");
        assert_eq!(fmt_size(500), "500 B");
        assert_eq!(fmt_size(1_500_000), "1.5 MB");
        assert_eq!(fmt_size(2_340_000_000), "2.34 GB");
    }
}
