//! Números da gestão da Produção, todos determinísticos e sem IA: contagens por etapa e estado (em cortes e em quadros),
//! matriz cena × etapa, fluxo cumulativo, burndown, vazão por semana, tempo médio por etapa, taxa de retake, a análise de
//! gargalos (fila, vazão, ociosidade, travados, atrasados) e o pacote de dados que um agente lê pelo MCP.
//!
//! Tudo vem da tabela de cortes (`CutRow`) e do histórico de status (`StatusEvent`, `production_history`). O relógio é um
//! parâmetro (`StatsConfig::now_ms`): nada aqui lê a hora do sistema, então os testes são exatos. Os dias são dias UTC
//! (instante em ms / 86 400 000).

use crate::production::{ALL_STAGES, CutRow, ProdNote, stage_label, status_label};
use crate::production_history::{DAY_MS, StatusEvent, day_of, format_date, parse_date};
use aiai_core::{Id, Stage, StageStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// Estados na ordem de exibição (e dos vetores de contagem).
pub const STATUS_ORDER: [StageStatus; 5] = [StageStatus::NotStarted, StageStatus::InProgress, StageStatus::Review, StageStatus::Approved, StageStatus::Retake];

pub fn status_idx(s: StageStatus) -> usize {
    STATUS_ORDER.iter().position(|x| *x == s).unwrap_or(0)
}

pub fn stage_idx(s: Stage) -> usize {
    ALL_STAGES.iter().position(|x| *x == s).unwrap_or(0)
}

/// Etapas que precisam estar aprovadas para esta poder começar (Fundo e Som correm em paralelo; Satsuei junta Shiage e Fundo).
pub fn predecessors(s: Stage) -> &'static [Stage] {
    match s {
        Stage::Layout | Stage::Background | Stage::Sound => &[],
        Stage::Genga => &[Stage::Layout],
        Stage::Douga => &[Stage::Genga],
        Stage::Shiage => &[Stage::Douga],
        Stage::Satsuei => &[Stage::Shiage, Stage::Background],
        Stage::Color => &[Stage::Satsuei],
    }
}

/// Parâmetros da análise.
#[derive(Debug, Clone, Copy)]
pub struct StatsConfig {
    /// "Agora" (ms desde a época Unix).
    pub now_ms: u64,
    /// Etapa em andamento/revisão sem mudança há mais que isto (dias) está travada.
    pub stuck_days: f64,
    /// Janela da vazão (dias).
    pub window_days: u32,
    /// Semanas do gráfico de vazão.
    pub weeks: usize,
    /// Máximo de dias do fluxo cumulativo/burndown (a série termina hoje).
    pub max_days: i64,
}

impl StatsConfig {
    pub fn new(now_ms: u64) -> Self {
        StatsConfig { now_ms, stuck_days: 3.0, window_days: 7, weeks: 8, max_days: 120 }
    }
}

// ================================================================ saída

/// Contagem por estado (ordem de [`STATUS_ORDER`]).
pub type Counts = [usize; 5];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageStat {
    pub stage: Stage,
    /// Cortes por estado.
    pub counts: Counts,
    /// Quadros por estado (a duração dos cortes soma no estado).
    pub frames: [u32; 5],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneRow {
    pub scene: Option<Id>,
    pub name: String,
    pub cuts: usize,
    /// Por etapa (ordem de `ALL_STAGES`): cortes por estado.
    pub cells: Vec<Counts>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DayPoint {
    /// Dia (UTC, dias desde a época).
    pub day: i64,
    /// Células corte × etapa por estado no fim do dia (fluxo cumulativo).
    pub flow: Counts,
    /// Cortes ainda não concluídos (alguma etapa sem aprovação) no fim do dia.
    pub remaining_cuts: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WeekPoint {
    /// Primeiro dia da semana (blocos de 7 dias terminando hoje).
    pub start_day: i64,
    /// Etapas aprovadas na semana.
    pub approvals: usize,
    /// Cortes que ficaram completos na semana.
    pub cuts_done: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageTiming {
    pub stage: Stage,
    /// Média de dias entre "em andamento" e "aprovado".
    pub avg_days: Option<f64>,
    pub samples: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RetakeRate {
    pub stage: Stage,
    /// Cortes que passaram pela etapa (não "não iniciado" ou com evento).
    pub touched: usize,
    /// Destes, os que tiveram retake (evento ou estado atual).
    pub retaken: usize,
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Forecast {
    pub remaining_cuts: usize,
    /// Cortes concluídos por dia na janela.
    pub per_day: f64,
    /// Dias até terminar nesse ritmo; `None` sem vazão.
    pub days_left: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stats {
    pub total_cuts: usize,
    pub done_cuts: usize,
    pub total_frames: u32,
    /// Células corte × etapa por estado (a rosca geral).
    pub overall: Counts,
    pub stages: Vec<StageStat>,
    pub scenes: Vec<SceneRow>,
    pub days: Vec<DayPoint>,
    /// Linha ideal do burndown: do restante no primeiro dia até 0 no prazo (o maior prazo dos cortes).
    pub ideal: Vec<(i64, f64)>,
    pub deadline: Option<i64>,
    pub weeks: Vec<WeekPoint>,
    pub timing: Vec<StageTiming>,
    pub retake_rates: Vec<RetakeRate>,
    pub forecast: Forecast,
    /// Há eventos no histórico (sem eles não há fluxo, vazão nem tempos).
    pub has_history: bool,
    /// Aprovações de etapa dentro da janela (`window_days`): a amostra de onde saem vazão, espera e gargalo.
    #[serde(default)]
    pub sample_approvals: usize,
    #[serde(default)]
    pub window_days: u32,
    /// Amostra abaixo de [`MIN_SAMPLE`]: os números são indicativos, não medidas (auditoria V2).
    #[serde(default)]
    pub low_sample: bool,
}

/// Menos aprovações que isto na janela = amostra pequena para falar em vazão e gargalo com precisão.
pub const MIN_SAMPLE: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StageFlow {
    pub stage: Stage,
    /// Cortes prontos para começar esta etapa (anteriores aprovadas, esta não iniciada).
    pub queue: usize,
    pub in_progress: usize,
    pub review: usize,
    pub retake: usize,
    pub approved: usize,
    /// Etapas aprovadas por dia na janela.
    pub throughput_per_day: f64,
    /// Idade média (dias) da fila: desde que a última etapa anterior foi aprovada.
    pub avg_queue_age_days: Option<f64>,
    /// Fila / vazão; `None` quando há fila mas nenhuma vazão (espera sem previsão).
    pub wait_days: Option<f64>,
    /// Etapas anteriores ainda não aprovadas em algum corte (esta etapa aguarda).
    pub waiting_cuts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    /// Etapa com o maior tempo de espera da fila.
    Bottleneck,
    /// Etapa sem fila nem trabalho, esperando a anterior.
    Idle,
    /// Etapa em andamento/revisão sem mudança há mais que N dias.
    Stuck,
    /// Prazo vencido e corte incompleto.
    Overdue,
    /// Etapa em retake ou retake aberto.
    OpenRetake,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Finding {
    pub kind: FindingKind,
    pub stage: Option<Stage>,
    /// Cortes envolvidos (o clique no achado filtra a lista por eles).
    pub cuts: Vec<Id>,
    /// Gargalo: dias de espera (`None` = sem vazão); travado/atrasado: o maior atraso em dias.
    pub days: Option<f64>,
    /// Gargalo: tamanho da fila.
    pub queue: usize,
    /// Gargalo: vazão por dia.
    pub per_day: f64,
}

impl Finding {
    /// Frase em português (o pacote para o agente; a interface monta a dela com tradução).
    pub fn text_pt(&self) -> String {
        let st = self.stage.map(stage_label).unwrap_or("");
        match self.kind {
            FindingKind::Bottleneck => match self.days {
                Some(d) => format!("Gargalo: {st} ({} cortes na fila, vazão {:.1}/dia, espera estimada {:.1} dias)", self.queue, self.per_day, d),
                None => format!("Gargalo: {st} ({} cortes na fila e nenhuma vazão na janela)", self.queue),
            },
            FindingKind::Idle => format!("Etapa ociosa: {st} (sem fila nem trabalho; aguarda a etapa anterior em {} cortes)", self.cuts.len()),
            FindingKind::Stuck => format!("{} corte(s) travados em {st} (sem mudança há até {:.0} dias)", self.cuts.len(), self.days.unwrap_or(0.0)),
            FindingKind::Overdue => format!("{} corte(s) com prazo vencido (até {:.0} dias de atraso)", self.cuts.len(), self.days.unwrap_or(0.0)),
            FindingKind::OpenRetake => format!("{} corte(s) com retake aberto{}", self.cuts.len(), if st.is_empty() { String::new() } else { format!(" em {st}") }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Analysis {
    pub stats: Stats,
    pub flows: Vec<StageFlow>,
    pub findings: Vec<Finding>,
    /// Outras etapas empatadas com o gargalo apontado (mesma espera, fila e trabalho em curso): o gargalo é só a primeira.
    #[serde(default)]
    pub bottleneck_ties: Vec<Stage>,
}

// ================================================================ linha do tempo por corte

/// Estados de um corte ao longo do tempo, reconstruídos dos eventos.
struct CutTl {
    created_ms: Option<u64>,
    init: [StageStatus; 8],
    cur: [StageStatus; 8],
    /// Por etapa: (instante, novo estado), em ordem.
    ev: [Vec<(u64, StageStatus)>; 8],
    seen: [bool; 8],
}

impl CutTl {
    fn state_at(&self, si: usize, t_end: u64) -> StageStatus {
        let ev = &self.ev[si];
        let n = ev.partition_point(|(t, _)| *t <= t_end);
        if n == 0 { self.init[si] } else { ev[n - 1].1 }
    }

    fn last_event(&self, si: usize) -> Option<(u64, StageStatus)> {
        self.ev[si].last().copied()
    }
}

fn is_done(states: impl Iterator<Item = StageStatus>) -> bool {
    states.into_iter().all(|s| s == StageStatus::Approved)
}

fn timelines(rows: &[CutRow], events: &[StatusEvent]) -> HashMap<String, CutTl> {
    let mut map: HashMap<String, CutTl> = rows
        .iter()
        .map(|r| {
            let cur: [StageStatus; 8] = std::array::from_fn(|i| r.stages.get(&ALL_STAGES[i]).copied().unwrap_or_default());
            (r.cut.0.clone(), CutTl { created_ms: None, init: cur, cur, ev: Default::default(), seen: [false; 8] })
        })
        .collect();
    for e in events {
        let Some(tl) = map.get_mut(&e.cut.0) else { continue };
        if e.created {
            tl.created_ms = Some(tl.created_ms.map_or(e.t_ms, |c| c.min(e.t_ms)));
            continue;
        }
        let Some(stage) = e.stage else { continue };
        let si = stage_idx(stage);
        if !tl.seen[si] {
            tl.seen[si] = true;
            tl.init[si] = e.from;
        }
        tl.ev[si].push((e.t_ms, e.to));
    }
    map
}

// ================================================================ análise

/// Todas as estatísticas, os fluxos por etapa e os achados.
pub fn analyze(rows: &[CutRow], events: &[StatusEvent], cfg: &StatsConfig) -> Analysis {
    let tls = timelines(rows, events);
    let today = day_of(cfg.now_ms);
    let window_ms = u64::from(cfg.window_days.max(1)) * DAY_MS;
    let since = cfg.now_ms.saturating_sub(window_ms);

    // ---- contagens atuais
    let mut stages: Vec<StageStat> = ALL_STAGES.iter().map(|s| StageStat { stage: *s, counts: [0; 5], frames: [0; 5] }).collect();
    let mut overall: Counts = [0; 5];
    for r in rows {
        for (i, st) in stages.iter_mut().enumerate() {
            let s = r.stages.get(&ALL_STAGES[i]).copied().unwrap_or_default();
            st.counts[status_idx(s)] += 1;
            st.frames[status_idx(s)] += r.duration;
            overall[status_idx(s)] += 1;
        }
    }
    let done_now = |r: &CutRow| is_done(ALL_STAGES.iter().map(|s| r.stages.get(s).copied().unwrap_or_default()));
    let done_cuts = rows.iter().filter(|r| done_now(r)).count();

    // ---- matriz cena × etapa (na ordem em que as cenas aparecem)
    let mut scenes: Vec<SceneRow> = Vec::new();
    for r in rows {
        let at = match scenes.iter().position(|s| s.scene == r.scene) {
            Some(i) => i,
            None => {
                scenes.push(SceneRow { scene: r.scene.clone(), name: r.scene_name.clone(), cuts: 0, cells: vec![[0; 5]; 8] });
                scenes.len() - 1
            }
        };
        scenes[at].cuts += 1;
        for (i, s) in ALL_STAGES.iter().enumerate() {
            scenes[at].cells[i][status_idx(r.stages.get(s).copied().unwrap_or_default())] += 1;
        }
    }

    // ---- fluxo cumulativo e burndown por dia
    let first_event_day = events.iter().map(|e| day_of(e.t_ms)).min();
    let start = first_event_day.map_or(today - 13, |d| d.min(today - 13)).max(today - cfg.max_days + 1).max(0);
    let mut days = Vec::new();
    for day in start..=today {
        let t_end = (day as u64 + 1) * DAY_MS - 1;
        let mut flow: Counts = [0; 5];
        let mut remaining = 0;
        for tl in tls.values() {
            // corte criado depois deste dia ainda não existia
            if tl.created_ms.is_some_and(|c| c > t_end) {
                continue;
            }
            let st: [StageStatus; 8] = std::array::from_fn(|i| tl.state_at(i, t_end));
            for s in st {
                flow[status_idx(s)] += 1;
            }
            if !is_done(st.into_iter()) {
                remaining += 1;
            }
        }
        days.push(DayPoint { day, flow, remaining_cuts: remaining });
    }
    let deadline = rows.iter().filter_map(|r| r.due.as_deref().and_then(parse_date)).max();
    let mut ideal = Vec::new();
    if let (Some(dl), Some(first)) = (deadline, days.first()) {
        if dl > first.day {
            // até 60 dias à frente de hoje para o gráfico não esticar demais
            let end = dl.min(today + 60);
            let slope = first.remaining_cuts as f64 / (dl - first.day) as f64;
            for d in first.day..=end {
                ideal.push((d, (first.remaining_cuts as f64 - slope * (d - first.day) as f64).max(0.0)));
            }
        }
    }

    // ---- vazão: aprovações por semana e cortes que ficaram completos
    let mut cut_done_times: Vec<u64> = Vec::new();
    {
        // reproduz os eventos em ordem, cortes na ordem do documento
        let mut state: HashMap<&str, [StageStatus; 8]> = tls.iter().map(|(k, tl)| (k.as_str(), tl.init)).collect();
        for e in events {
            let Some(stage) = e.stage else { continue };
            let Some(st) = state.get_mut(e.cut.0.as_str()) else { continue };
            let was = is_done(st.iter().copied());
            st[stage_idx(stage)] = e.to;
            if !was && is_done(st.iter().copied()) {
                cut_done_times.push(e.t_ms);
            }
        }
    }
    let approvals: Vec<u64> = events.iter().filter(|e| e.stage.is_some() && e.to == StageStatus::Approved && tls.contains_key(&e.cut.0)).map(|e| e.t_ms).collect();
    let weeks: Vec<WeekPoint> = (0..cfg.weeks.max(1))
        .rev()
        .map(|k| {
            let end_day = today - 7 * k as i64; // inclusive
            let start_day = end_day - 6;
            let in_week = |t: u64| {
                let d = day_of(t);
                d >= start_day && d <= end_day
            };
            WeekPoint { start_day, approvals: approvals.iter().filter(|t| in_week(**t)).count(), cuts_done: cut_done_times.iter().filter(|t| in_week(**t)).count() }
        })
        .collect();

    // ---- tempo por etapa e taxa de retake
    let mut timing = Vec::new();
    let mut retake_rates = Vec::new();
    for (si, stage) in ALL_STAGES.iter().enumerate() {
        let mut total = 0.0;
        let mut samples = 0usize;
        let (mut touched, mut retaken) = (0usize, 0usize);
        for tl in tls.values() {
            let ev = &tl.ev[si];
            let start = ev.iter().find(|(_, s)| matches!(s, StageStatus::InProgress)).or_else(|| ev.iter().find(|(_, s)| matches!(s, StageStatus::Review)));
            if let Some((t0, _)) = start {
                if let Some((t1, _)) = ev.iter().rev().find(|(t, s)| *s == StageStatus::Approved && t >= t0) {
                    total += (*t1 - *t0) as f64 / DAY_MS as f64;
                    samples += 1;
                }
            }
            if tl.cur[si] != StageStatus::NotStarted || !ev.is_empty() {
                touched += 1;
                if tl.cur[si] == StageStatus::Retake || ev.iter().any(|(_, s)| *s == StageStatus::Retake) {
                    retaken += 1;
                }
            }
        }
        timing.push(StageTiming { stage: *stage, avg_days: (samples > 0).then(|| total / samples as f64), samples });
        retake_rates.push(RetakeRate { stage: *stage, touched, retaken, rate: if touched > 0 { retaken as f64 / touched as f64 } else { 0.0 } });
    }

    // ---- previsão
    let done_in_window = cut_done_times.iter().filter(|t| **t > since).count();
    let per_day = done_in_window as f64 / f64::from(cfg.window_days.max(1));
    let remaining = rows.len() - done_cuts;
    let forecast = Forecast { remaining_cuts: remaining, per_day, days_left: (remaining == 0).then_some(0.0).or_else(|| (per_day > 0.0).then(|| remaining as f64 / per_day)) };

    let sample_approvals: usize = ALL_STAGES.iter().map(|s| approvals_of(events, *s, since, &tls)).sum();
    let stats = Stats {
        total_cuts: rows.len(),
        done_cuts,
        total_frames: rows.iter().map(|r| r.duration).sum(),
        overall,
        stages,
        scenes,
        days,
        ideal,
        deadline,
        weeks,
        timing,
        retake_rates,
        forecast,
        has_history: events.iter().any(|e| e.stage.is_some()),
        sample_approvals,
        window_days: cfg.window_days,
        low_sample: sample_approvals < MIN_SAMPLE,
    };

    // ---- fluxo por etapa e gargalos
    let mut flows = Vec::new();
    let mut queue_cuts: Vec<Vec<Id>> = Vec::new();
    for (si, stage) in ALL_STAGES.iter().enumerate() {
        let preds = predecessors(*stage);
        let mut q = Vec::new();
        let mut ages = Vec::new();
        let mut waiting = 0;
        for r in rows {
            let me = r.stages.get(stage).copied().unwrap_or_default();
            if me != StageStatus::NotStarted {
                continue;
            }
            let ready = preds.iter().all(|p| r.stages.get(p).copied().unwrap_or_default() == StageStatus::Approved);
            if ready {
                q.push(r.cut.clone());
                // idade: desde a última aprovação de uma etapa anterior (quando o histórico sabe)
                let tl = &tls[&r.cut.0];
                let since_ready = preds.iter().filter_map(|p| tl.ev[stage_idx(*p)].iter().rev().find(|(_, s)| *s == StageStatus::Approved).map(|(t, _)| *t)).max();
                if let Some(t) = since_ready {
                    ages.push(cfg.now_ms.saturating_sub(t) as f64 / DAY_MS as f64);
                }
            } else {
                waiting += 1;
            }
        }
        let st = &stats.stages[si];
        let thr = approvals_of(events, *stage, since, &tls) as f64 / f64::from(cfg.window_days.max(1));
        let queue = q.len();
        let wait_days = if queue == 0 { Some(0.0) } else if thr > 0.0 { Some(queue as f64 / thr) } else { None };
        flows.push(StageFlow {
            stage: *stage,
            queue,
            in_progress: st.counts[1],
            review: st.counts[2],
            retake: st.counts[4],
            approved: st.counts[3],
            throughput_per_day: thr,
            avg_queue_age_days: (!ages.is_empty()).then(|| ages.iter().sum::<f64>() / ages.len() as f64),
            wait_days,
            waiting_cuts: waiting,
        });
        queue_cuts.push(q);
    }

    let mut findings = Vec::new();
    // gargalo: maior espera (sem vazão = espera sem fim); empate: maior fila, depois mais trabalho em curso
    let key = |f: &StageFlow| (f.wait_days.unwrap_or(f64::INFINITY), f.queue, f.in_progress + f.review);
    let mut bottleneck_ties = Vec::new();
    if let Some((i, f)) = flows.iter().enumerate().filter(|(_, f)| f.queue > 0).max_by(|(_, a), (_, b)| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)) {
        findings.push(Finding { kind: FindingKind::Bottleneck, stage: Some(f.stage), cuts: queue_cuts[i].clone(), days: f.wait_days, queue: f.queue, per_day: f.throughput_per_day });
        bottleneck_ties = flows.iter().filter(|g| g.stage != f.stage && g.queue > 0 && key(g) == key(f)).map(|g| g.stage).collect();
    }
    for (i, f) in flows.iter().enumerate() {
        let not_started = stats.stages[i].counts[0];
        // ociosa: tem capacidade comprovada (já aprovou algo ou tem vazão) mas nada a fazer agora, esperando a etapa anterior
        let proven = f.approved > 0 || f.throughput_per_day > 0.0;
        if proven && f.queue == 0 && f.in_progress + f.review + f.retake == 0 && f.waiting_cuts > 0 && not_started > 0 {
            let cuts = rows.iter().filter(|r| r.stages.get(&f.stage).copied().unwrap_or_default() == StageStatus::NotStarted).map(|r| r.cut.clone()).collect();
            findings.push(Finding { kind: FindingKind::Idle, stage: Some(f.stage), cuts, days: None, queue: 0, per_day: f.throughput_per_day });
        }
    }
    // travados: em andamento/revisão sem evento há mais que stuck_days (sem histórico para a célula, não dá para saber)
    for (si, stage) in ALL_STAGES.iter().enumerate() {
        let mut cuts = Vec::new();
        let mut worst = 0.0f64;
        for r in rows {
            let s = r.stages.get(stage).copied().unwrap_or_default();
            if !matches!(s, StageStatus::InProgress | StageStatus::Review) {
                continue;
            }
            if let Some((t, _)) = tls[&r.cut.0].last_event(si) {
                let age = cfg.now_ms.saturating_sub(t) as f64 / DAY_MS as f64;
                if age > cfg.stuck_days {
                    cuts.push(r.cut.clone());
                    worst = worst.max(age);
                }
            }
        }
        if !cuts.is_empty() {
            findings.push(Finding { kind: FindingKind::Stuck, stage: Some(*stage), cuts, days: Some(worst), queue: 0, per_day: 0.0 });
        }
    }
    // atrasados
    let mut late = Vec::new();
    let mut worst_late = 0.0f64;
    for r in rows {
        if let Some(due) = r.due.as_deref().and_then(parse_date) {
            if due < today && !done_now(r) {
                late.push(r.cut.clone());
                worst_late = worst_late.max((today - due) as f64);
            }
        }
    }
    if !late.is_empty() {
        findings.push(Finding { kind: FindingKind::Overdue, stage: None, cuts: late, days: Some(worst_late), queue: 0, per_day: 0.0 });
    }
    // retakes abertos
    let retake_cuts: Vec<Id> = rows.iter().filter(|r| r.open_retakes > 0 || ALL_STAGES.iter().any(|s| r.stages.get(s) == Some(&StageStatus::Retake))).map(|r| r.cut.clone()).collect();
    if !retake_cuts.is_empty() {
        findings.push(Finding { kind: FindingKind::OpenRetake, stage: None, cuts: retake_cuts, days: None, queue: 0, per_day: 0.0 });
    }

    Analysis { stats, flows, findings, bottleneck_ties }
}

fn approvals_of(events: &[StatusEvent], stage: Stage, since_ms: u64, tls: &HashMap<String, CutTl>) -> usize {
    events.iter().filter(|e| e.stage == Some(stage) && e.to == StageStatus::Approved && e.t_ms > since_ms && tls.contains_key(&e.cut.0)).count()
}

// ================================================================ CSV

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

/// Os números dos gráficos em CSV (seções separadas por linha em branco), para planilha.
pub fn stats_csv(a: &Analysis) -> String {
    let mut o = String::new();
    let head: Vec<&str> = STATUS_ORDER.iter().map(|s| status_label(*s)).collect();
    let _ = writeln!(o, "etapa,{},quadros_{}", head.join(","), head.join(",quadros_"));
    for s in &a.stats.stages {
        let c: Vec<String> = s.counts.iter().map(|n| n.to_string()).collect();
        let f: Vec<String> = s.frames.iter().map(|n| n.to_string()).collect();
        let _ = writeln!(o, "{},{},{}", stage_label(s.stage), c.join(","), f.join(","));
    }
    let _ = writeln!(o, "\ncena,cortes,{}", ALL_STAGES.iter().map(|s| format!("{}_aprovados", stage_label(*s))).collect::<Vec<_>>().join(","));
    for r in &a.stats.scenes {
        let c: Vec<String> = r.cells.iter().map(|c| c[3].to_string()).collect();
        let _ = writeln!(o, "{},{},{}", csv_field(&r.name), r.cuts, c.join(","));
    }
    let _ = writeln!(o, "\ndia,{},cortes_restantes", head.join(","));
    for d in &a.stats.days {
        let f: Vec<String> = d.flow.iter().map(|n| n.to_string()).collect();
        let _ = writeln!(o, "{},{},{}", format_date(d.day), f.join(","), d.remaining_cuts);
    }
    let _ = writeln!(o, "\nsemana_inicio,etapas_aprovadas,cortes_concluidos");
    for w in &a.stats.weeks {
        let _ = writeln!(o, "{},{},{}", format_date(w.start_day), w.approvals, w.cuts_done);
    }
    let _ = writeln!(o, "\netapa,dias_medios_andamento_ate_aprovado,amostras,taxa_retake,cortes_com_retake,cortes_que_passaram");
    for (t, r) in a.stats.timing.iter().zip(&a.stats.retake_rates) {
        let _ = writeln!(o, "{},{},{},{:.3},{},{}", stage_label(t.stage), t.avg_days.map_or(String::new(), |d| format!("{d:.2}")), t.samples, r.rate, r.retaken, r.touched);
    }
    let _ = writeln!(o, "\netapa,fila,em_andamento,revisao,retake,aprovados,vazao_por_dia,espera_dias");
    for f in &a.flows {
        let _ = writeln!(o, "{},{},{},{},{},{},{:.3},{}", stage_label(f.stage), f.queue, f.in_progress, f.review, f.retake, f.approved, f.throughput_per_day, f.wait_days.map_or("sem_vazao".to_owned(), |d| format!("{d:.2}")));
    }
    o
}

// ================================================================ pacote para o agente

/// Versão do modelo de pedido da análise (muda quando o roteiro abaixo muda).
pub const PACKAGE_VERSION: u32 = 1;

/// Roteiro fixo que acompanha o pacote: o que o agente deve devolver (e o que nunca deve fazer).
pub const ANALYSIS_PROMPT: &str = "Você analisa a produção de um episódio de anime a partir dos dados abaixo (contagens por etapa, fila, vazão, \
tempos, atrasos, retakes e os últimos eventos). Responda em português com: 1) resumo em 5 linhas; 2) riscos (por corte ou etapa); \
3) três ações recomendadas, com o impacto estimado em dias; 4) perguntas para quem gerencia. Não aplique nenhuma mudança no projeto: \
só devolva o texto, que entra como nota 'gerada por IA' no painel (ação PRODUCTION op=note_add com ai=true).";

/// Pacote de dados compacto (JSON) para um agente analisar pelo MCP: estatísticas, gargalos e os últimos eventos.
pub fn analysis_package(project: &str, rows: &[CutRow], events: &[StatusEvent], notes: &[ProdNote], cfg: &StatsConfig) -> Value {
    let a = analyze(rows, events, cfg);
    let name_of: BTreeMap<&str, String> = rows.iter().map(|r| (r.cut.0.as_str(), format!("C{:03}", r.number))).collect();
    let stages: Vec<Value> = a
        .flows
        .iter()
        .zip(&a.stats.stages)
        .zip(a.stats.timing.iter().zip(&a.stats.retake_rates))
        .map(|((f, s), (t, rr))| {
            json!({
                "etapa": stage_label(f.stage),
                "estados": STATUS_ORDER.iter().enumerate().map(|(i, st)| (status_label(*st).to_owned(), json!(s.counts[i]))).collect::<serde_json::Map<_, _>>(),
                "fila": f.queue, "vazao_por_dia": round2(f.throughput_per_day),
                "espera_dias": f.wait_days.map(round2), "idade_media_fila_dias": f.avg_queue_age_days.map(round2),
                "dias_medios_ate_aprovar": t.avg_days.map(round2), "amostras": t.samples,
                "taxa_retake": round2(rr.rate),
            })
        })
        .collect();
    let findings: Vec<Value> = a
        .findings
        .iter()
        .map(|f| json!({ "tipo": f.kind, "etapa": f.stage.map(stage_label), "texto": f.text_pt(), "cortes": f.cuts.iter().map(|c| name_of.get(c.0.as_str()).cloned().unwrap_or_else(|| c.0.clone())).collect::<Vec<_>>() }))
        .collect();
    let overdue: Vec<Value> = rows.iter().filter(|r| r.due.as_deref().and_then(parse_date).is_some_and(|d| d < day_of(cfg.now_ms))).map(|r| json!({ "corte": format!("C{:03}", r.number), "prazo": r.due })).collect();
    let recent: Vec<Value> = events
        .iter()
        .rev()
        .filter(|e| e.stage.is_some())
        .take(60)
        .map(|e| json!({ "t": format_date(day_of(e.t_ms)), "corte": name_of.get(e.cut.0.as_str()).cloned().unwrap_or_else(|| e.cut.0.clone()), "etapa": e.stage.map(stage_label), "de": status_label(e.from), "para": status_label(e.to), "autor": e.author }))
        .collect();
    json!({
        "versao_pacote": PACKAGE_VERSION,
        "projeto": project,
        "gerado_em": format_date(day_of(cfg.now_ms)),
        "gerado_em_ms": cfg.now_ms,
        "roteiro": ANALYSIS_PROMPT,
        "cortes": a.stats.total_cuts, "cortes_completos": a.stats.done_cuts, "quadros": a.stats.total_frames,
        "tem_historico": a.stats.has_history,
        "amostra": { "aprovacoes_na_janela": a.stats.sample_approvals, "janela_dias": a.stats.window_days, "pequena": a.stats.low_sample, "minimo": MIN_SAMPLE },
        "gargalo_empatado_com": a.bottleneck_ties.iter().map(|s| stage_label(*s)).collect::<Vec<_>>(),
        "previsao": { "cortes_restantes": a.stats.forecast.remaining_cuts, "cortes_por_dia": round2(a.stats.forecast.per_day), "dias_restantes": a.stats.forecast.days_left.map(round2) },
        "prazo_final": a.stats.deadline.map(format_date),
        "etapas": stages,
        "achados": findings,
        "atrasados": overdue,
        "vazao_semanal": a.stats.weeks.iter().map(|w| json!({ "inicio": format_date(w.start_day), "etapas_aprovadas": w.approvals, "cortes_concluidos": w.cuts_done })).collect::<Vec<_>>(),
        "ultimos_eventos": recent,
        "notas": notes.iter().rev().take(20).map(|n| json!({ "t": format_date(day_of(n.t_ms)), "autor": n.author, "ia": n.ai, "texto": n.text })).collect::<Vec<_>>(),
    })
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production_history::HistoryIndex;
    use aiai_core::{Command, LogEntry};

    const DAY: u64 = DAY_MS;

    fn row(id: &str, num: u32, scene: &str, stages: &[(Stage, StageStatus)], due: Option<&str>) -> CutRow {
        let mut map: BTreeMap<Stage, StageStatus> = ALL_STAGES.iter().map(|s| (*s, StageStatus::NotStarted)).collect();
        for (s, st) in stages {
            map.insert(*s, *st);
        }
        CutRow {
            cut: Id(id.into()),
            number: num,
            name: format!("C{num:03}"),
            episode: None,
            scene: Some(Id(scene.into())),
            scene_name: scene.into(),
            duration: 24,
            stages: map,
            open_corrections: 0,
            retakes: 0,
            open_retakes: 0,
            due: due.map(str::to_owned),
            assignee: None,
            weight: None,
            notes: String::new(),
            thumbnail: None,
        }
    }

    fn ev(rev: u64, t: u64, cut: &str, stage: Stage, from: StageStatus, to: StageStatus) -> StatusEvent {
        StatusEvent { revision: rev, t_ms: t, author: "human".into(), cut: Id(cut.into()), stage: Some(stage), from, to, created: false }
    }

    use StageStatus::*;

    /// Hoje = dia 100; três cortes numa cena e um noutra.
    fn fixture() -> (Vec<CutRow>, Vec<StatusEvent>, StatsConfig) {
        let now = 100 * DAY + DAY / 2;
        let rows = vec![
            // completo
            row("a", 1, "S1", &ALL_STAGES.map(|s| (s, Approved)), Some("1970-04-20")),
            // Layout e Genga aprovados: Douga na fila
            row("b", 2, "S1", &[(Stage::Layout, Approved), (Stage::Genga, Approved)], Some("1970-04-30")),
            // Layout aprovado, Genga em andamento há 6 dias (travado) e prazo vencido (dia 95)
            row("c", 3, "S1", &[(Stage::Layout, Approved), (Stage::Genga, InProgress)], Some("1970-04-06")),
            // Layout em retake
            row("d", 4, "S2", &[(Stage::Layout, Retake)], None),
        ];
        let mut events = Vec::new();
        let mut rev = 0;
        let mut push = |t: u64, cut: &str, st: Stage, from, to| {
            rev += 1;
            events.push(ev(rev, t, cut, st, from, to));
        };
        // a: tudo aprovado no dia 92 (7 etapas antes, a última, Sound, no dia 98 = dentro da janela)
        for s in ALL_STAGES {
            let t = if s == Stage::Sound { 98 * DAY + 100 } else { 92 * DAY + 100 };
            push(t, "a", s, NotStarted, InProgress);
            push(t + 1000, "a", s, InProgress, Approved);
        }
        push(94 * DAY, "b", Stage::Layout, NotStarted, InProgress);
        push(95 * DAY, "b", Stage::Layout, InProgress, Approved);
        push(96 * DAY, "b", Stage::Genga, NotStarted, InProgress);
        push(99 * DAY, "b", Stage::Genga, InProgress, Approved);
        push(94 * DAY, "c", Stage::Layout, NotStarted, Approved);
        push(94 * DAY + 5, "c", Stage::Genga, NotStarted, InProgress);
        push(93 * DAY, "d", Stage::Layout, NotStarted, InProgress);
        push(97 * DAY, "d", Stage::Layout, InProgress, Retake);
        (rows, events, StatsConfig::new(now))
    }

    #[test]
    fn small_samples_and_bottleneck_ties_are_flagged() {
        let (rows, mut events, cfg) = fixture();
        let a = analyze(&rows, &events, &cfg);
        // na janela de 7 dias: Sound de a, Layout e Genga de b, Layout de c
        assert_eq!((a.stats.sample_approvals, a.stats.window_days), (4, 7));
        assert!(a.stats.low_sample);
        events.push(ev(99, 99 * DAY, "d", Stage::Layout, Retake, Approved));
        let a = analyze(&rows, &events, &cfg);
        assert_eq!(a.stats.sample_approvals, 5);
        assert!(!a.stats.low_sample);
        // sem histórico nenhuma etapa tem vazão: as filas do mesmo tamanho empatam com o gargalo
        let a = analyze(&rows, &[], &cfg);
        let bn = a.findings.iter().find(|f| f.kind == FindingKind::Bottleneck).expect("gargalo");
        assert!(!a.bottleneck_ties.contains(&bn.stage.unwrap()));
        for s in &a.bottleneck_ties {
            let f = a.flows.iter().find(|f| f.stage == *s).unwrap();
            assert_eq!((f.queue, f.wait_days), (bn.queue, None), "{s:?}");
        }
    }

    #[test]
    fn counts_by_stage_scene_and_frames() {
        let (rows, events, cfg) = fixture();
        let a = analyze(&rows, &events, &cfg);
        assert_eq!((a.stats.total_cuts, a.stats.done_cuts, a.stats.total_frames), (4, 1, 96));
        let layout = &a.stats.stages[0];
        // não iniciado, em andamento, revisão, aprovado, retake
        assert_eq!(layout.counts, [0, 0, 0, 3, 1]);
        assert_eq!(layout.frames, [0, 0, 0, 72, 24]);
        let douga = &a.stats.stages[2];
        assert_eq!(douga.counts, [3, 0, 0, 1, 0]);
        assert_eq!(a.stats.overall.iter().sum::<usize>(), 32);
        assert_eq!(a.stats.scenes.len(), 2);
        assert_eq!((a.stats.scenes[0].name.as_str(), a.stats.scenes[0].cuts), ("S1", 3));
        assert_eq!(a.stats.scenes[0].cells[0], [0, 0, 0, 3, 0]);
        assert_eq!(a.stats.scenes[1].cells[0], [0, 0, 0, 0, 1]);
        assert!(a.stats.has_history);
    }

    #[test]
    fn flow_and_burndown_are_rebuilt_from_history() {
        let (rows, events, cfg) = fixture();
        let a = analyze(&rows, &events, &cfg);
        let at = |d: i64| a.stats.days.iter().find(|p| p.day == d).unwrap();
        // dia 91: ninguém tinha começado (a série começa antes do primeiro evento, no dia 87 = hoje - 13)
        assert_eq!(a.stats.days.first().unwrap().day, 87);
        assert_eq!(at(91).remaining_cuts, 4);
        assert_eq!(at(91).flow, [32, 0, 0, 0, 0]);
        // dia 92 à noite: a está com 7 etapas aprovadas (Sound só no 98): nada completo ainda
        assert_eq!(at(92).flow[3], 7);
        assert_eq!(at(92).remaining_cuts, 4);
        // dia 98: a completo
        assert_eq!(at(98).remaining_cuts, 3);
        // hoje: o estado atual (retake de d, em andamento de c)
        let today = at(100);
        assert_eq!(today.remaining_cuts, 3);
        assert_eq!(today.flow[4], 1);
        // cada dia tem as 32 células
        assert!(a.stats.days.iter().all(|p| p.flow.iter().sum::<usize>() == 32));
        // prazo final = 30/04/1970 = dia 119; a linha ideal desce do restante inicial a 0 nesse dia
        assert_eq!(a.stats.deadline, Some(119));
        assert_eq!(a.stats.ideal.first(), Some(&(87, 4.0)));
        assert_eq!(a.stats.ideal.last(), Some(&(119, 0.0)));
        assert!(a.stats.ideal.windows(2).all(|w| w[1].1 <= w[0].1));
    }

    #[test]
    fn weekly_throughput_timing_and_retake_rate() {
        let (rows, events, cfg) = fixture();
        let a = analyze(&rows, &events, &cfg);
        let last = a.stats.weeks.last().unwrap();
        // semana = dias 94..=100: aprovações de Sound (a, dia 98), b Layout (95), b Genga (99), c Layout (94)
        assert_eq!((last.start_day, last.approvals, last.cuts_done), (94, 4, 1));
        // semana anterior (87..=93): 7 etapas de a no dia 92
        let prev = &a.stats.weeks[a.stats.weeks.len() - 2];
        assert_eq!((prev.approvals, prev.cuts_done), (7, 0));
        assert_eq!(a.stats.weeks.len(), 8);
        // tempo por etapa: Layout de a (1 s ~ 0 dia) e b (1 dia): média ~ 0,5 dia
        let t = &a.stats.timing[0];
        assert_eq!(t.samples, 2);
        assert!((t.avg_days.unwrap() - 0.5).abs() < 0.01, "{t:?}");
        // Layout: 4 cortes tocaram, 1 em retake
        let r = &a.stats.retake_rates[0];
        assert_eq!((r.touched, r.retaken), (4, 1));
        assert!((r.rate - 0.25).abs() < 1e-9);
        // previsão: 1 corte concluído na janela de 7 dias -> 1/7 por dia; 3 restantes -> 21 dias
        assert_eq!(a.stats.forecast.remaining_cuts, 3);
        assert!((a.stats.forecast.days_left.unwrap() - 21.0).abs() < 1e-6);
    }

    #[test]
    fn bottleneck_rules_queue_throughput_stuck_overdue_and_retake() {
        let (rows, events, cfg) = fixture();
        let a = analyze(&rows, &events, &cfg);
        let f = |s: Stage| a.flows.iter().find(|f| f.stage == s).unwrap();
        // Douga: fila só de b (Genga aprovada); c ainda em Genga
        assert_eq!(f(Stage::Douga).queue, 1);
        assert_eq!(f(Stage::Douga).waiting_cuts, 2);
        // Douga teve 1 aprovação (a, dia 92) fora da janela de 7 dias: sem vazão
        assert_eq!(f(Stage::Douga).throughput_per_day, 0.0);
        assert_eq!(f(Stage::Douga).wait_days, None);
        // Genga: aprovada por b (dia 99) dentro da janela -> 1/7 por dia; fila vazia (Layout de d em retake)
        assert!((f(Stage::Genga).throughput_per_day - 1.0 / 7.0).abs() < 1e-9);
        assert_eq!(f(Stage::Genga).queue, 0);
        // a fila de Background e Sound é de todos os não iniciados (sem anteriores)
        assert_eq!(f(Stage::Background).queue, 3);
        assert_eq!(f(Stage::Sound).queue, 3);
        // idade da fila de Douga: b teve Genga aprovada no dia 99; agora 100,5 -> 1,5 dia
        assert!((f(Stage::Douga).avg_queue_age_days.unwrap() - 1.5).abs() < 1e-6);
        // Sound tem vazão (a aprovou no dia 98): espera 3 / (1/7) = 21 dias, finita
        assert!((f(Stage::Sound).wait_days.unwrap() - 21.0).abs() < 1e-6);
        // gargalo: Background (3 na fila, sem vazão: espera sem fim) ganha de Douga (1 na fila, também sem vazão, mas
        // fila menor) e de Sound (espera finita)
        let bn = a.findings.iter().find(|x| x.kind == FindingKind::Bottleneck).unwrap();
        assert_eq!((bn.stage, bn.queue, bn.days), (Some(Stage::Background), 3, None));
        // travado: c com Genga em andamento desde o dia 94 (6,5 dias > 3)
        let stuck = a.findings.iter().find(|x| x.kind == FindingKind::Stuck && x.stage == Some(Stage::Genga)).unwrap();
        assert_eq!(stuck.cuts, vec![Id("c".into())]);
        assert!(stuck.days.unwrap() > 6.0);
        // atrasado: c (prazo dia 95 = 06/04) incompleto; a (dia 109) está no prazo e completa
        let late = a.findings.iter().find(|x| x.kind == FindingKind::Overdue).unwrap();
        assert_eq!(late.cuts, vec![Id("c".into())]);
        assert_eq!(late.days, Some(5.0));
        // retake: d
        let rt = a.findings.iter().find(|x| x.kind == FindingKind::OpenRetake).unwrap();
        assert_eq!(rt.cuts, vec![Id("d".into())]);
        // Satsuei está ociosa (aguarda Shiage em todos os cortes incompletos; só a tem tudo aprovado)
        assert!(a.findings.iter().any(|x| x.kind == FindingKind::Idle && x.stage == Some(Stage::Satsuei)));
        for x in &a.findings {
            assert!(!x.text_pt().is_empty());
        }
    }

    #[test]
    fn a_known_throughput_picks_the_longest_wait() {
        // 10 cortes com Layout aprovado (fila de Genga = 10), 2 aprovações de Genga na janela: espera 35 dias
        let now = 50 * DAY;
        let mut rows = Vec::new();
        let mut events = Vec::new();
        for i in 0..10 {
            let id = format!("c{i}");
            rows.push(row(&id, i + 1, "S", &[(Stage::Layout, Approved)], None));
            events.push(ev(u64::from(i) + 1, 40 * DAY, &id, Stage::Layout, NotStarted, Approved));
        }
        // dois cortes que já passaram da Genga (em Douga na fila)
        for i in 10..12 {
            let id = format!("c{i}");
            rows.push(row(&id, i + 1, "S", &[(Stage::Layout, Approved), (Stage::Genga, Approved)], None));
            events.push(ev(u64::from(i) + 100, 48 * DAY, &id, Stage::Genga, InProgress, Approved));
        }
        let a = analyze(&rows, &events, &StatsConfig::new(now));
        let genga = a.flows.iter().find(|f| f.stage == Stage::Genga).unwrap();
        assert_eq!(genga.queue, 10);
        assert!((genga.throughput_per_day - 2.0 / 7.0).abs() < 1e-9);
        assert!((genga.wait_days.unwrap() - 35.0).abs() < 1e-6);
        let bn = a.findings.iter().find(|x| x.kind == FindingKind::Bottleneck).unwrap();
        // Douga: fila 2 e vazão 0 (sem aprovações) = espera sem fim, que vence qualquer espera finita; Background e Sound
        // têm fila 12 e vazão 0 também: o desempate é a fila maior
        assert_eq!(bn.queue, 12);
        assert_eq!(bn.days, None);
    }

    #[test]
    fn empty_history_still_gives_the_current_picture() {
        let rows = vec![row("a", 1, "S", &[(Stage::Layout, Approved)], None)];
        let a = analyze(&rows, &[], &StatsConfig::new(30 * DAY));
        assert!(!a.stats.has_history);
        assert_eq!(a.stats.days.len(), 14);
        assert!(a.stats.days.iter().all(|d| d.flow[3] == 1 && d.remaining_cuts == 1));
        assert_eq!(a.stats.forecast.days_left, None);
        // sem eventos, ninguém é "travado": não há como saber a idade
        assert!(a.findings.iter().all(|f| f.kind != FindingKind::Stuck));
    }

    #[test]
    fn csv_and_package_carry_the_numbers() {
        let (rows, events, cfg) = fixture();
        let a = analyze(&rows, &events, &cfg);
        let csv = stats_csv(&a);
        assert!(csv.starts_with("etapa,não iniciado,em andamento,"));
        assert!(csv.contains("\nLO,0,0,0,3,1,"));
        assert!(csv.contains("S1,3,"));
        assert!(csv.contains("semana_inicio,etapas_aprovadas,cortes_concluidos"));
        let pkg = analysis_package("t", &rows, &events, &[], &cfg);
        assert_eq!(pkg["versao_pacote"], 1);
        assert_eq!(pkg["cortes"], 4);
        assert_eq!(pkg["etapas"].as_array().unwrap().len(), 8);
        assert_eq!(pkg["etapas"][0]["etapa"], "LO");
        assert_eq!(pkg["etapas"][0]["estados"]["aprovado"], 3);
        assert!(pkg["achados"].as_array().unwrap().iter().any(|f| f["tipo"] == "overdue" && f["cortes"][0] == "C003"));
        assert!(pkg["ultimos_eventos"].as_array().unwrap().len() <= 60);
        assert!(pkg["roteiro"].as_str().unwrap().contains("Não aplique nenhuma mudança"));
        assert_eq!(pkg["prazo_final"], "1970-04-30");
    }

    #[test]
    fn index_sync_feeds_the_analysis_end_to_end() {
        const STATUS_NAMES: [&str; 3] = ["in_progress", "review", "approved"];
        // log sintético → índice → análise, sem passar pelas linhas montadas à mão
        let mut idx = HistoryIndex::default();
        let log: Vec<LogEntry> = (0..3)
            .map(|i| LogEntry {
                seq: i,
                revision: i + 1,
                op: "apply".into(),
                description: String::new(),
                author: "human".into(),
                time_ms: (90 + i) * DAY,
                command: Some(Command::SetProps { id: Id("a".into()), name: None, data: json!({ "stages": { "layout": STATUS_NAMES[i as usize] } }) }),
            })
            .collect();
        idx.sync(&log);
        let rows = vec![row("a", 1, "S", &[(Stage::Layout, Approved)], None)];
        let a = analyze(&rows, &idx.events, &StatsConfig::new(100 * DAY));
        assert_eq!(a.stats.weeks.iter().map(|w| w.approvals).sum::<usize>(), 1);
        let day = |d: i64| a.stats.days.iter().find(|p| p.day == d).unwrap().flow;
        assert_eq!((day(89)[0], day(90)[1], day(91)[2], day(92)[3]), (8, 1, 1, 1));
    }
}
