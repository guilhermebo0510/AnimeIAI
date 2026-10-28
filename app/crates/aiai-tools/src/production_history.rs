//! Histórico de estados da Produção: um índice de eventos `StatusEvent { corte, etapa, de, para, quando, autor }` derivado
//! do log de comandos da sessão (cada mudança de etapa é um `SetProps` com `stages`, com autor e hora).
//!
//! O log mora na sessão e o WAL recomeça a cada salvamento, então o índice se guarda ao lado do projeto
//! (`producao-historico.ndjson`, uma linha JSON por evento, só acrescenta): ao abrir lê-se o arquivo, depois
//! [`HistoryIndex::sync`] soma o que o log trouxe (entradas com revisão maior que a última do índice; a revisão sobe a
//! cada apply/undo/redo e sobrevive ao salvar, então reabrir e a recuperação pelo WAL não duplicam nada).
//!
//! Limite conhecido: o estado anterior ao primeiro evento de um corte/etapa que o índice vê é desconhecido (o comando não
//! o carrega), e conta como "não iniciado". Projetos criados depois deste recurso têm o histórico completo.

use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Arquivo do índice, dentro da pasta `.aiai` do projeto.
pub const HISTORY_FILE: &str = "producao-historico.ndjson";

pub const DAY_MS: u64 = 86_400_000;

// ================================================================ datas civis (sem dependência de calendário)

/// Dias desde 1970-01-01 de uma data civil (algoritmo de H. Hinnant).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Data civil de um número de dias desde 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `AAAA-MM-DD` → dias desde a época; `None` se o formato ou a data forem inválidos.
pub fn parse_date(s: &str) -> Option<i64> {
    let b = s.trim().as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| std::str::from_utf8(&b[r]).ok()?.parse::<u32>().ok();
    let (y, m, d) = (num(0..4)? as i64, num(5..7)?, num(8..10)?);
    if !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    // dia 31 de fevereiro volta como 3 de março: recusa
    (civil_from_days(days) == (y, m, d)).then_some(days)
}

pub fn format_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Dia (UTC) de um instante em ms.
pub fn day_of(ms: u64) -> i64 {
    (ms / DAY_MS) as i64
}

// ================================================================ eventos

/// Uma mudança de estado de etapa (ou a criação de um corte) no histórico da Produção.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StatusEvent {
    /// Revisão do projeto em que a mudança entrou (chave de ordem e de deduplicação).
    pub revision: u64,
    /// Instante (ms desde a época Unix).
    pub t_ms: u64,
    pub author: String,
    pub cut: Id,
    /// Etapa que mudou; `None` no evento de criação do corte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<Stage>,
    #[serde(default)]
    pub from: StageStatus,
    #[serde(default)]
    pub to: StageStatus,
    /// O corte foi criado neste evento.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub created: bool,
}

/// O que um comando diz sobre etapas, antes de casar com o estado anterior.
enum Raw {
    Created(Id),
    Stage(Id, Stage, StageStatus),
    /// `ReplaceData` de um corte: o mapa inteiro de etapas.
    Replace(Id, std::collections::BTreeMap<Stage, StageStatus>),
}

fn collect(cmd: &Command, out: &mut Vec<Raw>) {
    match cmd {
        Command::SetProps { id, data, .. } => {
            let Some(stages) = data.get("stages").and_then(Value::as_object) else { return };
            for (k, v) in stages {
                let Ok(stage) = serde_json::from_value::<Stage>(Value::String(k.clone())) else { continue };
                let status = if v.is_null() { Some(StageStatus::NotStarted) } else { serde_json::from_value::<StageStatus>(v.clone()).ok() };
                if let Some(s) = status {
                    out.push(Raw::Stage(id.clone(), stage, s));
                }
            }
        }
        Command::CreateNode { id: Some(id), data: NodeKind::Cut(cd), .. } => {
            out.push(Raw::Created(id.clone()));
            for (s, st) in &cd.stages {
                out.push(Raw::Stage(id.clone(), *s, *st));
            }
        }
        Command::ReplaceData { id, data: NodeKind::Cut(cd) } => out.push(Raw::Replace(id.clone(), cd.stages.clone())),
        Command::Batch { commands, .. } => commands.iter().for_each(|c| collect(c, out)),
        _ => {}
    }
}

/// Índice de eventos de status, em ordem de revisão, e o estado corrente de cada (corte, etapa) que ele conhece.
#[derive(Debug, Clone, Default)]
pub struct HistoryIndex {
    pub events: Vec<StatusEvent>,
    /// Maior revisão já incorporada.
    pub last_rev: u64,
    state: std::collections::BTreeMap<(String, Stage), StageStatus>,
}

impl HistoryIndex {
    pub fn from_events(events: Vec<StatusEvent>) -> HistoryIndex {
        let mut idx = HistoryIndex::default();
        for e in &events {
            idx.last_rev = idx.last_rev.max(e.revision);
            if let Some(s) = e.stage {
                idx.state.insert((e.cut.0.clone(), s), e.to);
            }
        }
        idx.events = events;
        idx
    }

    /// Soma as entradas novas do log (revisão > `last_rev`) e devolve os eventos acrescentados.
    pub fn sync(&mut self, log: &[LogEntry]) -> Vec<StatusEvent> {
        let mut added = Vec::new();
        for e in log {
            if e.revision <= self.last_rev {
                continue;
            }
            self.last_rev = e.revision;
            let Some(cmd) = &e.command else { continue };
            let mut raws = Vec::new();
            collect(cmd, &mut raws);
            for raw in raws {
                match raw {
                    Raw::Created(cut) => added.push(StatusEvent { revision: e.revision, t_ms: e.time_ms, author: e.author.clone(), cut, stage: None, from: StageStatus::NotStarted, to: StageStatus::NotStarted, created: true }),
                    Raw::Stage(cut, stage, to) => self.push_stage(&mut added, e, cut, stage, to),
                    Raw::Replace(cut, map) => {
                        for s in [Stage::Layout, Stage::Genga, Stage::Douga, Stage::Shiage, Stage::Background, Stage::Satsuei, Stage::Color, Stage::Sound] {
                            let to = map.get(&s).copied().unwrap_or_default();
                            // só etapas que o índice já conhecia ou que ficaram diferentes de "não iniciado"
                            if self.state.contains_key(&(cut.0.clone(), s)) || to != StageStatus::NotStarted {
                                self.push_stage(&mut added, e, cut.clone(), s, to);
                            }
                        }
                    }
                }
            }
        }
        self.events.extend(added.iter().cloned());
        added
    }

    fn push_stage(&mut self, added: &mut Vec<StatusEvent>, e: &LogEntry, cut: Id, stage: Stage, to: StageStatus) {
        let key = (cut.0.clone(), stage);
        let from = self.state.get(&key).copied().unwrap_or_default();
        self.state.insert(key, to);
        if from != to {
            added.push(StatusEvent { revision: e.revision, t_ms: e.time_ms, author: e.author.clone(), cut, stage: Some(stage), from, to, created: false });
        }
    }
}

// ================================================================ arquivo ao lado do projeto

/// Caminho do arquivo do índice na pasta `.aiai`.
pub fn history_path(dir: &Path) -> PathBuf {
    dir.join(HISTORY_FILE)
}

/// Lê o índice do disco (arquivo ausente = vazio). Linhas ilegíveis são puladas e contadas (segundo valor).
pub fn load(dir: &Path) -> (HistoryIndex, usize) {
    let Ok(text) = std::fs::read_to_string(history_path(dir)) else { return (HistoryIndex::default(), 0) };
    let mut events = Vec::new();
    let mut bad = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<StatusEvent>(line) {
            Ok(e) => events.push(e),
            Err(_) => bad += 1,
        }
    }
    (HistoryIndex::from_events(events), bad)
}

/// Acrescenta eventos ao arquivo (cria a pasta se preciso).
pub fn append(dir: &Path, events: &[StatusEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(history_path(dir))?;
    let mut buf = String::new();
    for e in events {
        buf.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
        buf.push('\n');
    }
    f.write_all(buf.as_bytes())
}

/// Regrava o arquivo inteiro (pasta nova: "Salvar como").
pub fn rewrite(dir: &Path, events: &[StatusEvent]) -> std::io::Result<()> {
    let mut buf = String::new();
    for e in events {
        buf.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
        buf.push('\n');
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(history_path(dir), buf)
}

/// Índice da sessão: o arquivo do projeto (se há pasta) + o log da sessão, gravando o que for novo. O texto devolvido,
/// se houver, é um aviso (arquivo com linhas ruins ou falha ao gravar) para quem chama mostrar.
pub fn index_for_session(session: &CoreSession) -> (HistoryIndex, Option<String>) {
    let mut warn = None;
    let (mut idx, bad) = match &session.path {
        Some(dir) => load(dir),
        None => (HistoryIndex::default(), 0),
    };
    if bad > 0 {
        warn = Some(format!("{bad} linha(s) ilegíveis em {HISTORY_FILE} foram puladas"));
    }
    let added = idx.sync(session.log());
    if let (Some(dir), false) = (&session.path, added.is_empty()) {
        if let Err(e) = append(dir, &added) {
            warn = Some(format!("não gravei {HISTORY_FILE}: {e}"));
        }
    }
    (idx, warn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(seq: u64, revision: u64, t: u64, author: &str, command: Command) -> LogEntry {
        LogEntry { seq, revision, op: "apply".into(), description: String::new(), author: author.into(), time_ms: t, command: Some(command), source: Default::default() }
    }

    fn set(cut: &str, stage: &str, status: Value) -> Command {
        Command::SetProps { id: Id(cut.into()), name: None, data: json!({ "stages": { stage: status } }) }
    }

    #[test]
    fn dates_round_trip_and_reject_impossible_days() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2026-10-20").map(format_date).as_deref(), Some("2026-10-20"));
        assert_eq!(parse_date("2024-02-29").map(format_date).as_deref(), Some("2024-02-29"));
        assert_eq!(parse_date("2026-02-29"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("26-10-20"), None);
        assert_eq!(day_of(DAY_MS * 3 + 5), 3);
    }

    #[test]
    fn events_come_from_a_synthetic_log_with_from_to_author_and_time() {
        let mut spec = CutData::new(24);
        spec.stages.insert(Stage::Layout, StageStatus::Approved);
        let log = vec![
            entry(0, 1, 1000, "human", Command::CreateNode { id: Some(Id("c1".into())), parent: None, index: None, name: "C1".into(), data: NodeKind::Cut(spec) }),
            entry(1, 2, 2000, "human", set("c1", "genga", json!("in_progress"))),
            entry(2, 3, 3000, "agent:claude", set("c1", "genga", json!("review"))),
            // mesmo estado: não é evento
            entry(3, 4, 3500, "human", set("c1", "genga", json!("review"))),
            // lote: duas etapas de cortes diferentes num passo de undo
            entry(
                4,
                5,
                4000,
                "human",
                Command::Batch { commands: vec![set("c1", "genga", json!("approved")), set("c2", "douga", json!("in_progress"))], label: None },
            ),
            // desfazer o lote: o inverso volta o estado (null = não iniciado)
            LogEntry { seq: 5, revision: 6, op: "undo".into(), description: String::new(), author: "human".into(), time_ms: 5000, command: Some(set("c1", "genga", json!("review"))), source: Default::default() },
            entry(6, 7, 6000, "human", set("c1", "layout", Value::Null)),
        ];
        let mut idx = HistoryIndex::default();
        let added = idx.sync(&log);
        let flat: Vec<(String, Option<Stage>, StageStatus, StageStatus, u64, &str, bool)> =
            added.iter().map(|e| (e.cut.0.clone(), e.stage, e.from, e.to, e.t_ms, e.author.as_str(), e.created)).collect();
        use StageStatus::*;
        assert_eq!(
            flat,
            vec![
                ("c1".into(), None, NotStarted, NotStarted, 1000, "human", true),
                ("c1".into(), Some(Stage::Layout), NotStarted, Approved, 1000, "human", false),
                ("c1".into(), Some(Stage::Genga), NotStarted, InProgress, 2000, "human", false),
                ("c1".into(), Some(Stage::Genga), InProgress, Review, 3000, "agent:claude", false),
                ("c1".into(), Some(Stage::Genga), Review, Approved, 4000, "human", false),
                ("c2".into(), Some(Stage::Douga), NotStarted, InProgress, 4000, "human", false),
                ("c1".into(), Some(Stage::Genga), Approved, Review, 5000, "human", false),
                ("c1".into(), Some(Stage::Layout), Approved, NotStarted, 6000, "human", false),
            ]
        );
        assert_eq!(idx.last_rev, 7);
        // sincronizar de novo o mesmo log não duplica
        assert!(idx.sync(&log).is_empty());
        assert_eq!(idx.events.len(), 8);
    }

    #[test]
    fn replace_data_diffs_against_the_known_state() {
        let mut idx = HistoryIndex::default();
        idx.sync(&[entry(0, 1, 10, "a", set("c", "genga", json!("review")))]);
        let mut cd = CutData::new(12);
        cd.stages.insert(Stage::Douga, StageStatus::InProgress);
        let added = idx.sync(&[entry(1, 2, 20, "a", Command::ReplaceData { id: Id("c".into()), data: NodeKind::Cut(cd) })]);
        let flat: Vec<(Stage, StageStatus, StageStatus)> = added.iter().map(|e| (e.stage.unwrap(), e.from, e.to)).collect();
        assert_eq!(flat, vec![(Stage::Genga, StageStatus::Review, StageStatus::NotStarted), (Stage::Douga, StageStatus::NotStarted, StageStatus::InProgress)]);
    }

    #[test]
    fn the_index_survives_a_reopen_through_the_sidecar_file() {
        let dir = std::env::temp_dir().join(format!("aiai-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log1 = vec![entry(0, 1, 10, "a", set("c", "genga", json!("in_progress"))), entry(1, 2, 20, "a", set("c", "genga", json!("review")))];
        let mut idx = HistoryIndex::default();
        let added = idx.sync(&log1);
        append(&dir, &added).unwrap();
        // reabrir: o arquivo traz tudo; o log novo (WAL reaplicado + uma mudança nova) só soma a mudança nova
        let (mut again, bad) = load(&dir);
        assert_eq!((bad, again.events.len(), again.last_rev), (0, 2, 2));
        let mut log2 = log1.clone();
        log2.push(entry(2, 3, 30, "b", set("c", "genga", json!("approved"))));
        let added = again.sync(&log2);
        assert_eq!(added.len(), 1);
        assert_eq!((added[0].from, added[0].to), (StageStatus::Review, StageStatus::Approved));
        append(&dir, &added).unwrap();
        let (third, _) = load(&dir);
        assert_eq!(third.events.len(), 3);
        // linha ruim é pulada e contada
        std::fs::write(history_path(&dir), format!("{}\nlixo\n", serde_json::to_string(&third.events[0]).unwrap())).unwrap();
        let (_, bad) = load(&dir);
        assert_eq!(bad, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
