//! Sessão: projeto em memória + blobs + histórico (undo/redo) + log + WAL. É o único escritor do documento;
//! UI, bridge/MCP, CLI e testes passam por aqui.

use crate::blob::BlobStore;
use crate::command::{Applied, ApplyCtx, Command, Outcome};
use crate::error::{CoreError, CoreResult};
use crate::ids::IdGen;
use crate::model::Project;
use crate::persist::Wal;
use serde::{Deserialize, Serialize};

/// Relógio injetável (tempo em ms desde a época Unix).
#[derive(Debug, Clone)]
pub enum Clock {
    System,
    Fixed(u64),
}

impl Clock {
    pub fn now_ms(&self) -> u64 {
        match self {
            Clock::System => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            Clock::Fixed(v) => *v,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    pub seq: u64,
    pub revision: u64,
    /// "apply" | "undo" | "redo"
    pub op: String,
    pub description: String,
    pub author: String,
    pub time_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Command>,
}

pub struct CoreSession {
    pub project: Project,
    pub blobs: BlobStore,
    undo: Vec<Applied>,
    redo: Vec<Applied>,
    log: Vec<LogEntry>,
    pub ids: IdGen,
    pub clock: Clock,
    /// Autor padrão dos comandos ("human", "agent:claude"...). Cada chamada pode sobrescrever.
    pub author: String,
    /// Roda `Project::validate` completo depois de cada comando (testes e depuração).
    pub full_validate: bool,
    pub undo_limit: usize,
    wal: Option<Wal>,
    /// Sobe a cada mudança no documento (apply/undo/redo) e também quando o projeto é trocado — para caches.
    pub change_counter: u64,
    /// Pasta `.aiai` do projeto (definida por abrir/salvar).
    pub path: Option<std::path::PathBuf>,
    /// Ações que saíram do histórico (limite do undo, redo descartado) desde a última coleta de blobs.
    dropped_since_gc: usize,
    /// Trava "aberto em outro lugar" da pasta (`None` = sem pasta, ou aberta só para leitura porque outro processo vivo
    /// tem a trava: o Salvar nessa pasta recusa).
    lock: Option<crate::lock::ProjectLock>,
}

/// Coleta os blobs órfãos depois de tantas ações saírem do histórico (sem isso a memória só crescia).
const GC_AFTER_DROPPED: usize = 64;
/// O log guarda o comando inteiro só das últimas entradas (as antigas ficam com a descrição, o autor e a hora).
const LOG_KEEP_COMMANDS: usize = 5000;

impl CoreSession {
    pub fn new(project: Project) -> Self {
        CoreSession {
            project,
            blobs: BlobStore::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            log: Vec::new(),
            ids: IdGen::Uuid7,
            clock: Clock::System,
            author: "human".into(),
            full_validate: cfg!(test),
            undo_limit: 500,
            wal: None,
            change_counter: 0,
            path: None,
            dropped_since_gc: 0,
            lock: None,
        }
    }

    /// Sessão determinística para testes: ids sequenciais, relógio fixo, validação completa.
    pub fn for_tests(name: &str) -> Self {
        let mut s = CoreSession::new(Project::new(name));
        s.ids = IdGen::sequential("id-");
        s.clock = Clock::Fixed(0);
        s.full_validate = true;
        s
    }

    pub fn attach_wal(&mut self, wal: Wal) {
        self.wal = Some(wal);
    }

    pub fn detach_wal(&mut self) -> Option<Wal> {
        self.wal.take()
    }

    pub fn wal_mut(&mut self) -> Option<&mut Wal> {
        self.wal.as_mut()
    }

    pub fn lock(&self) -> Option<&crate::lock::ProjectLock> {
        self.lock.as_ref()
    }

    pub fn set_lock(&mut self, lock: Option<crate::lock::ProjectLock>) {
        self.lock = lock;
    }

    pub fn take_lock(&mut self) -> Option<crate::lock::ProjectLock> {
        self.lock.take()
    }

    /// Fechamento limpo: solta a trava da pasta (o `project.lock` desta abertura sai do disco).
    pub fn release_lock(&mut self) {
        self.lock = None;
    }

    pub fn apply(&mut self, command: Command) -> CoreResult<Outcome> {
        let author = self.author.clone();
        self.apply_as(command, &author, None)
    }

    /// Aplica com autor explícito e, opcionalmente, exigindo que a revisão atual seja `if_revision`.
    pub fn apply_as(&mut self, mut command: Command, author: &str, if_revision: Option<u64>) -> CoreResult<Outcome> {
        if let Some(expected) = if_revision {
            if expected != self.project.revision {
                return Err(CoreError::StaleRevision { expected, actual: self.project.revision });
            }
        }
        command.resolve_ids(&mut self.ids);
        let revision = self.project.revision + 1;
        let applied = {
            let ctx = ApplyCtx { blobs: &self.blobs, author, revision };
            command.apply(&mut self.project, &ctx)?
        };
        if self.full_validate {
            if let Err(e) = self.project.validate() {
                // Bug de comando: desfaz e reporta.
                let ctx = ApplyCtx { blobs: &self.blobs, author, revision };
                applied.inverse.apply(&mut self.project, &ctx).expect("inverso deve restaurar");
                return Err(CoreError::Hierarchy(format!("o comando deixou o projeto inválido ({e}); revertido")));
            }
        }
        self.project.revision = revision;
        self.change_counter += 1;
        let description = applied.command.description();
        let outcome = Outcome { revision, description: description.clone(), created: applied.created.clone(), affected: applied.affected.clone() };
        self.record("apply", &description, author, Some(applied.command.clone()))?;
        self.undo.push(applied);
        self.blobs.tick();
        // `while`: baixar o limite nas Preferências esvazia o excesso na próxima ação, não uma entrada por vez
        while self.undo.len() > self.undo_limit && !self.undo.is_empty() {
            self.undo.remove(0);
            self.dropped_since_gc += 1;
        }
        self.dropped_since_gc += self.redo.len();
        self.redo.clear();
        if self.dropped_since_gc >= GC_AFTER_DROPPED {
            self.collect_garbage();
        }
        Ok(outcome)
    }

    pub fn undo(&mut self) -> CoreResult<Outcome> {
        let a = self.undo.pop().ok_or(CoreError::NothingToUndo)?;
        let author = self.author.clone();
        let revision = self.project.revision + 1;
        let r = {
            let ctx = ApplyCtx { blobs: &self.blobs, author: &author, revision };
            match a.inverse.apply(&mut self.project, &ctx) {
                Ok(r) => r,
                Err(e) => {
                    self.undo.push(a);
                    return Err(e);
                }
            }
        };
        self.project.revision = revision;
        self.change_counter += 1;
        let description = format!("Desfazer: {}", a.command.description());
        let outcome = Outcome { revision, description: description.clone(), created: r.created.clone(), affected: r.affected.clone() };
        self.record("undo", &description, &author, Some(a.inverse.clone()))?;
        self.redo.push(Applied { command: r.inverse, inverse: a.inverse, created: a.created, affected: r.affected });
        Ok(outcome)
    }

    pub fn redo(&mut self) -> CoreResult<Outcome> {
        let a = self.redo.pop().ok_or(CoreError::NothingToRedo)?;
        let author = self.author.clone();
        let revision = self.project.revision + 1;
        let r = {
            let ctx = ApplyCtx { blobs: &self.blobs, author: &author, revision };
            match a.command.apply(&mut self.project, &ctx) {
                Ok(r) => r,
                Err(e) => {
                    self.redo.push(a);
                    return Err(e);
                }
            }
        };
        self.project.revision = revision;
        self.change_counter += 1;
        let description = format!("Refazer: {}", a.command.description());
        let outcome = Outcome { revision, description: description.clone(), created: r.created.clone(), affected: r.affected.clone() };
        self.record("redo", &description, &author, Some(a.command.clone()))?;
        self.undo.push(r);
        Ok(outcome)
    }

    fn record(&mut self, op: &str, description: &str, author: &str, command: Option<Command>) -> CoreResult<()> {
        let entry = LogEntry {
            seq: self.log.len() as u64,
            revision: self.project.revision,
            op: op.into(),
            description: description.into(),
            author: author.into(),
            time_ms: self.clock.now_ms(),
            command,
        };
        if let Some(wal) = self.wal.as_mut() {
            wal.append(&entry, &mut self.blobs)?;
        }
        self.log.push(entry);
        if self.log.len() > LOG_KEEP_COMMANDS {
            let old = self.log.len() - LOG_KEEP_COMMANDS - 1;
            self.log[old].command = None;
        }
        Ok(())
    }

    /// Junta os últimos `n` comandos aplicados num passo só de desfazer/refazer (ex.: a camada criada sozinha no começo
    /// de um gesto e o traço gravado nela, auditoria V2-04). Só vale se eles foram os últimos registros e todos
    /// aplicações; o WAL já gravado continua igual (repetir as partes dá o mesmo estado). `false` = nada mudou.
    pub fn merge_last(&mut self, n: usize, label: &str) -> bool {
        if n < 2 || self.undo.len() < n || self.log.len() < n || !self.log[self.log.len() - n..].iter().all(|e| e.op == "apply") {
            return false;
        }
        let parts = self.undo.split_off(self.undo.len() - n);
        let command = Command::Batch { commands: parts.iter().map(|a| a.command.clone()).collect(), label: Some(label.to_owned()) };
        let inverse = Command::Batch { commands: parts.iter().rev().map(|a| a.inverse.clone()).collect(), label: None };
        let mut created = Vec::new();
        let mut affected = Vec::new();
        for a in &parts {
            created.extend(a.created.iter().cloned());
            affected.extend(a.affected.iter().cloned());
        }
        affected.dedup();
        self.undo.push(Applied { command: command.clone(), inverse, created, affected });
        let entries = self.log.split_off(self.log.len() - n);
        let (first, last) = (entries.first().cloned(), entries.last().cloned());
        if let (Some(first), Some(last)) = (first, last) {
            self.log.push(LogEntry { seq: first.seq, revision: last.revision, op: "apply".into(), description: label.to_owned(), author: last.author, time_ms: last.time_ms, command: Some(command) });
        }
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Descrições da pilha de undo (mais antigo primeiro) e de redo (próximo a refazer primeiro).
    pub fn history(&self) -> (Vec<String>, Vec<String>) {
        (
            self.undo.iter().map(|a| a.command.description()).collect(),
            self.redo.iter().rev().map(|a| a.command.description()).collect(),
        )
    }

    /// Os comandos da pilha de undo (mais antigo primeiro) e de redo (próximo a refazer primeiro): o painel Histórico
    /// monta rótulos legíveis com eles (nome da camada, ferramenta), coisa que a descrição em texto não dá.
    pub fn history_commands(&self) -> (Vec<&Command>, Vec<&Command>) {
        (self.undo.iter().map(|a| &a.command).collect(), self.redo.iter().rev().map(|a| &a.command).collect())
    }

    pub fn log(&self) -> &[LogEntry] {
        &self.log
    }

    /// Troca o projeto inteiro (abrir/novo). Histórico e log recomeçam.
    pub fn replace_project(&mut self, project: Project, blobs: BlobStore) {
        // o WAL, a pasta e a trava pertencem ao projeto anterior
        self.wal = None;
        self.path = None;
        self.lock = None;
        self.project = project;
        self.blobs = blobs;
        self.undo.clear();
        self.redo.clear();
        self.log.clear();
        self.change_counter += 1;
    }

    /// Assume o documento de outra sessão (abrir projeto) com o histórico dela: depois de uma recuperação pelo WAL
    /// os comandos reaplicados continuam desfazíveis. Autor, relógio, ids e limites desta sessão ficam.
    pub fn adopt_document(&mut self, mut other: CoreSession) {
        self.wal = other.wal.take();
        self.path = other.path.take();
        // a trava antiga só apaga o arquivo se ainda for dela (reabrir a mesma pasta grava token novo)
        self.lock = other.lock.take();
        self.project = other.project;
        self.blobs = other.blobs;
        self.undo = other.undo;
        self.redo = other.redo;
        self.log = other.log;
        self.dropped_since_gc = 0;
        self.change_counter += 1;
    }

    /// Remove blobs que nem o documento nem o histórico referenciam.
    pub fn collect_garbage(&mut self) -> usize {
        self.dropped_since_gc = 0;
        let mut keep = self.project.referenced_blobs();
        for a in self.undo.iter().chain(self.redo.iter()) {
            collect_command_blobs(&a.command, &mut keep);
            collect_command_blobs(&a.inverse, &mut keep);
        }
        self.blobs.retain(&keep)
    }
}

fn collect_command_blobs(c: &Command, out: &mut std::collections::BTreeSet<crate::blob::BlobHash>) {
    // Caminho simples e completo: serializa e procura hashes conhecidos seria caro; percorre os tipos que guardam blobs.
    use crate::model::NodeKind;
    let data_blobs = |d: &NodeKind, out: &mut std::collections::BTreeSet<crate::blob::BlobHash>| match d {
        NodeKind::Cel(cel) => {
            out.extend(cel.tiles.iter().map(|t| t.blob.clone()));
            out.extend(cel.fills.iter().filter_map(|f| f.mask.clone()));
            out.extend(cel.image.clone());
        }
        NodeKind::Asset(a) => out.extend(a.blob.clone()),
        _ => {}
    };
    match c {
        Command::CreateNode { data, .. } | Command::ReplaceData { data, .. } => data_blobs(data, out),
        Command::RestoreNodes { nodes, .. } => nodes.iter().for_each(|n| data_blobs(&n.data, out)),
        Command::SetTiles { tiles, .. } => out.extend(tiles.iter().filter_map(|t| t.blob.clone())),
        Command::ReplaceFills { fills, .. } => out.extend(fills.iter().filter_map(|f| f.mask.clone())),
        Command::SetProps { data, .. } => {
            // Patches podem citar blobs (ex.: `image`); procurar strings com cara de hash.
            fn walk(v: &serde_json::Value, out: &mut std::collections::BTreeSet<crate::blob::BlobHash>) {
                match v {
                    serde_json::Value::String(s) => {
                        let h = crate::blob::BlobHash(s.clone());
                        if h.is_well_formed() {
                            out.insert(h);
                        }
                    }
                    serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                    serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
                    _ => {}
                }
            }
            walk(data, out);
        }
        Command::Batch { commands, .. } => commands.iter().for_each(|x| collect_command_blobs(x, out)),
        _ => {}
    }
}
