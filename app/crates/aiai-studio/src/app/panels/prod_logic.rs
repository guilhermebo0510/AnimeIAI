//! Lógica pura (sem egui) das visões da Produção: etapa atual e estado geral do corte, colunas da Lista, ordenação,
//! agrupamento, filtros, visões salvas e as preferências guardadas ao lado do projeto. Fica separada para ser testada
//! sem janela; `prod_views` desenha em cima disto.

use aiai_core::{Id, Stage, StageStatus};
use aiai_tools::production::{ALL_STAGES, CutRow};
use aiai_tools::production_history::parse_date;
use aiai_tools::production_stats::status_idx;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;

/// Arquivo das preferências da Produção (visão, colunas, ordem, visões salvas), na pasta `.aiai` do projeto.
pub(crate) const PREFS_FILE: &str = "producao-ui.json";

/// Etapa atual do corte: a primeira, na ordem do pipeline, que ainda não está aprovada (`None` = todas aprovadas).
pub(crate) fn current_stage(r: &CutRow) -> Option<Stage> {
    ALL_STAGES.into_iter().find(|s| r.stages.get(s).copied().unwrap_or_default() != StageStatus::Approved)
}

fn stage_of(r: &CutRow, s: Stage) -> StageStatus {
    r.stages.get(&s).copied().unwrap_or_default()
}

/// Estado geral do corte: retake se alguma etapa está em retake; aprovado se todas; senão o mais adiantado em curso
/// (revisão > andamento); senão não iniciado.
pub(crate) fn cut_status(r: &CutRow) -> StageStatus {
    let all = |st: StageStatus| ALL_STAGES.iter().all(|s| stage_of(r, *s) == st);
    let any = |st: StageStatus| ALL_STAGES.iter().any(|s| stage_of(r, *s) == st);
    if any(StageStatus::Retake) {
        StageStatus::Retake
    } else if all(StageStatus::Approved) {
        StageStatus::Approved
    } else if any(StageStatus::Review) {
        StageStatus::Review
    } else if any(StageStatus::InProgress) || any(StageStatus::Approved) {
        StageStatus::InProgress
    } else {
        StageStatus::NotStarted
    }
}

/// Fração de etapas aprovadas (0..1).
pub(crate) fn progress(r: &CutRow) -> f32 {
    ALL_STAGES.iter().filter(|s| stage_of(r, **s) == StageStatus::Approved).count() as f32 / ALL_STAGES.len() as f32
}

pub(crate) fn is_done(r: &CutRow) -> bool {
    current_stage(r).is_none()
}

// ------------------------------------------------------------------------------------------------ visões e colunas

/// Visões do painel principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProdView {
    #[default]
    List,
    Thumbs,
    Big,
    Board,
}

impl ProdView {
    pub(crate) const ALL: [ProdView; 4] = [ProdView::List, ProdView::Thumbs, ProdView::Big, ProdView::Board];

    pub(crate) fn key(self) -> &'static str {
        match self {
            ProdView::List => "prod.view.list",
            ProdView::Thumbs => "prod.view.thumbs",
            ProdView::Big => "prod.view.big",
            ProdView::Board => "prod.view.board",
        }
    }

    pub(crate) fn tag(self) -> &'static str {
        match self {
            ProdView::List => "list",
            ProdView::Thumbs => "thumbs",
            ProdView::Big => "big",
            ProdView::Board => "board",
        }
    }

}

/// Coluna da Lista.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Col {
    Thumb,
    Cut,
    Scene,
    Frames,
    Current,
    Status,
    Stage(Stage),
    Progress,
    Assignee,
    Due,
    Weight,
    Retakes,
    Corrections,
}

impl Col {
    /// Todas as colunas, na ordem do menu "Colunas".
    pub(crate) fn all() -> Vec<Col> {
        let mut v = vec![Col::Thumb, Col::Cut, Col::Scene, Col::Frames, Col::Current, Col::Status, Col::Progress];
        v.extend(ALL_STAGES.iter().map(|s| Col::Stage(*s)));
        v.extend([Col::Assignee, Col::Due, Col::Weight, Col::Retakes, Col::Corrections]);
        v
    }

    pub(crate) fn defaults() -> Vec<Col> {
        let mut v = vec![Col::Thumb, Col::Cut, Col::Scene, Col::Current];
        v.extend(ALL_STAGES.iter().map(|s| Col::Stage(*s)));
        v.push(Col::Due);
        v
    }

    /// Chave i18n do título; as colunas de etapa usam o nome da etapa.
    pub(crate) fn title_key(self) -> &'static str {
        match self {
            Col::Thumb => "prod.col.thumb",
            Col::Cut => "prod.col.cut",
            Col::Scene => "prod.col.scene",
            Col::Frames => "prod.col.frames",
            Col::Current => "prod.col.current",
            Col::Status => "prod.col.status",
            Col::Stage(s) => super::production::stage_key(s),
            Col::Progress => "prod.col.progress",
            Col::Assignee => "prod.col.assignee",
            Col::Due => "prod.col.due",
            Col::Weight => "prod.col.weight",
            Col::Retakes => "prod.col.retakes",
            Col::Corrections => "prod.col.corrections",
        }
    }

    /// Largura da coluna em pontos.
    pub(crate) fn width(self) -> f32 {
        match self {
            Col::Thumb => 58.0,
            Col::Cut => 84.0,
            Col::Scene => 96.0,
            Col::Frames => 38.0,
            Col::Current => 62.0,
            Col::Status => 92.0,
            Col::Stage(_) => 42.0,
            Col::Progress => 80.0,
            Col::Assignee => 76.0,
            Col::Due => 80.0,
            Col::Weight => 48.0,
            Col::Retakes => 40.0,
            Col::Corrections => 40.0,
        }
    }

    pub(crate) fn sortable(self) -> bool {
        !matches!(self, Col::Thumb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Sort {
    pub col: Col,
    pub asc: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort { col: Col::Cut, asc: true }
    }
}

/// `None` por último, nos dois sentidos.
fn cmp_opt<T: Ord>(a: Option<T>, b: Option<T>, asc: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => {
            if asc { x.cmp(&y) } else { y.cmp(&x) }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Ordem de dois cortes por coluna; empate sempre pelo número do corte (ordem estável e previsível).
pub(crate) fn compare(a: &CutRow, b: &CutRow, sort: Sort) -> Ordering {
    let by_key = |o: Ordering| if sort.asc { o } else { o.reverse() };
    let primary = match sort.col {
        Col::Thumb | Col::Cut => by_key(a.number.cmp(&b.number)),
        Col::Scene => by_key(a.scene_name.to_lowercase().cmp(&b.scene_name.to_lowercase())),
        Col::Frames => by_key(a.duration.cmp(&b.duration)),
        Col::Current => by_key(current_stage(a).map_or(99, |s| s as i32).cmp(&current_stage(b).map_or(99, |s| s as i32))),
        Col::Status => by_key(status_idx(cut_status(a)).cmp(&status_idx(cut_status(b)))),
        Col::Stage(s) => by_key(status_idx(stage_of(a, s)).cmp(&status_idx(stage_of(b, s)))),
        Col::Progress => by_key(progress(a).partial_cmp(&progress(b)).unwrap_or(Ordering::Equal)),
        Col::Assignee => cmp_opt(a.assignee.as_deref().map(str::to_lowercase), b.assignee.as_deref().map(str::to_lowercase), sort.asc),
        Col::Due => cmp_opt(a.due.as_deref().and_then(parse_date), b.due.as_deref().and_then(parse_date), sort.asc),
        Col::Weight => cmp_opt(a.weight.map(|w| (w * 100.0) as i64), b.weight.map(|w| (w * 100.0) as i64), sort.asc),
        Col::Retakes => by_key(a.open_retakes.cmp(&b.open_retakes)),
        Col::Corrections => by_key(a.open_corrections.cmp(&b.open_corrections)),
    };
    primary.then_with(|| a.number.cmp(&b.number))
}

// ------------------------------------------------------------------------------------------------ agrupar

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Group {
    #[default]
    None,
    Scene,
    Stage,
    Assignee,
    Status,
}

impl Group {
    pub(crate) const ALL: [Group; 5] = [Group::None, Group::Scene, Group::Stage, Group::Assignee, Group::Status];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Group::None => "prod.group.none",
            Group::Scene => "prod.group.scene",
            Group::Stage => "prod.group.stage",
            Group::Assignee => "prod.group.assignee",
            Group::Status => "prod.group.status",
        }
    }
}

/// Chave de um grupo (a interface a transforma em título traduzido).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GroupKey {
    All,
    Scene(String),
    Stage(Option<Stage>),
    Assignee(Option<String>),
    Status(StageStatus),
}

/// Divide os cortes (já ordenados) em grupos; a ordem dos grupos: cenas na ordem de aparição, etapas na ordem do
/// pipeline (concluídos por último), responsáveis em ordem alfabética (sem responsável por último), estados na ordem fixa.
pub(crate) fn group_rows<'a>(rows: Vec<&'a CutRow>, group: Group) -> Vec<(GroupKey, Vec<&'a CutRow>)> {
    let mut out: Vec<(GroupKey, Vec<&CutRow>)> = Vec::new();
    for r in rows {
        let key = match group {
            Group::None => GroupKey::All,
            Group::Scene => GroupKey::Scene(r.scene_name.clone()),
            Group::Stage => GroupKey::Stage(current_stage(r)),
            Group::Assignee => GroupKey::Assignee(r.assignee.clone()),
            Group::Status => GroupKey::Status(cut_status(r)),
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(r),
            None => out.push((key, vec![r])),
        }
    }
    match group {
        Group::Stage => out.sort_by_key(|(k, _)| if let GroupKey::Stage(s) = k { s.map_or(99, |s| s as i32) } else { 0 }),
        Group::Assignee => out.sort_by(|(a, _), (b, _)| match (a, b) {
            (GroupKey::Assignee(x), GroupKey::Assignee(y)) => cmp_opt(x.as_deref().map(str::to_lowercase), y.as_deref().map(str::to_lowercase), true),
            _ => Ordering::Equal,
        }),
        Group::Status => out.sort_by_key(|(k, _)| if let GroupKey::Status(s) = k { status_idx(*s) } else { 0 }),
        Group::None | Group::Scene => {}
    }
    out
}

// ------------------------------------------------------------------------------------------------ filtros

/// Filtros em chips da Lista (e das Miniaturas, do Grande e do Quadro, que mostram os mesmos cortes).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Filters {
    /// Etapa (com `status`: "esta etapa neste estado"; sozinha: cortes cuja etapa atual é ela).
    pub stage: Option<Stage>,
    /// Estado (com `stage`: dessa etapa; sozinho: de qualquer etapa).
    pub status: Option<StageStatus>,
    /// Só cortes com retake aberto ou etapa em retake.
    pub retake: bool,
    /// Só cortes com prazo vencido e incompletos.
    pub overdue: bool,
    /// Só cortes travados (etapa em curso sem mudança há mais que N dias; vem da análise).
    pub stuck: bool,
    pub assignee: Option<String>,
    pub search: String,
}

/// O que os filtros precisam saber além da linha: o dia de hoje e os cortes travados.
pub(crate) struct FilterCtx<'a> {
    pub today: i64,
    pub stuck: &'a HashSet<String>,
    /// Filtro por achado (Gargalos): só estes cortes; `None` = todos.
    pub only: Option<&'a HashSet<String>>,
}

impl Filters {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Filters::default()
    }

    pub(crate) fn matches(&self, r: &CutRow, cx: &FilterCtx) -> bool {
        if let Some(only) = cx.only {
            if !only.contains(&r.cut.0) {
                return false;
            }
        }
        match (self.stage, self.status) {
            (Some(s), Some(st)) => {
                if stage_of(r, s) != st {
                    return false;
                }
            }
            (Some(s), None) => {
                if current_stage(r) != Some(s) {
                    return false;
                }
            }
            (None, Some(st)) => {
                if !ALL_STAGES.iter().any(|s| stage_of(r, *s) == st) {
                    return false;
                }
            }
            (None, None) => {}
        }
        if self.retake && !(r.open_retakes > 0 || ALL_STAGES.iter().any(|s| stage_of(r, *s) == StageStatus::Retake)) {
            return false;
        }
        if self.overdue && !r.due.as_deref().and_then(parse_date).is_some_and(|d| d < cx.today && !is_done(r)) {
            return false;
        }
        if self.stuck && !cx.stuck.contains(&r.cut.0) {
            return false;
        }
        if let Some(a) = &self.assignee {
            if r.assignee.as_deref() != Some(a.as_str()) {
                return false;
            }
        }
        let q = self.search.trim().to_lowercase();
        if !q.is_empty() {
            let hay = format!("c{:03} {} {} {}", r.number, r.name, r.scene_name, r.assignee.as_deref().unwrap_or("")).to_lowercase();
            if !q.split_whitespace().all(|w| hay.contains(w)) {
                return false;
            }
        }
        true
    }
}

// ------------------------------------------------------------------------------------------------ visões salvas e preferências

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedView {
    pub name: String,
    pub view: ProdView,
    pub cols: Vec<Col>,
    pub sort: Sort,
    pub group: Group,
    pub filters: Filters,
}

/// Preferências da Produção guardadas ao lado do projeto (`producao-ui.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProdPrefs {
    pub view: ProdView,
    pub cols: Vec<Col>,
    pub sort: Sort,
    pub group: Group,
    pub saved: Vec<SavedView>,
    /// Análise por IA ligada (desligada por padrão).
    pub ai_enabled: bool,
    /// Dias sem mudança para um corte contar como travado.
    pub stuck_days: f32,
    /// Tamanho das miniaturas: 0 pequeno, 1 médio, 2 grande.
    pub thumb_size: u8,
}

impl Default for ProdPrefs {
    fn default() -> Self {
        ProdPrefs { view: ProdView::List, cols: Col::defaults(), sort: Sort::default(), group: Group::None, saved: Vec::new(), ai_enabled: false, stuck_days: 3.0, thumb_size: 1 }
    }
}

impl ProdPrefs {
    /// Lê `producao-ui.json` (ausente ou ilegível = padrão). O segundo valor é o aviso de arquivo ilegível.
    pub(crate) fn load(dir: &std::path::Path) -> (ProdPrefs, Option<String>) {
        match std::fs::read_to_string(dir.join(PREFS_FILE)) {
            Ok(text) => match serde_json::from_str::<ProdPrefs>(&text) {
                Ok(p) => (p, None),
                Err(e) => (ProdPrefs::default(), Some(format!("{PREFS_FILE}: {e}"))),
            },
            Err(_) => (ProdPrefs::default(), None),
        }
    }

    pub(crate) fn save(&self, dir: &std::path::Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(PREFS_FILE), text).map_err(|e| format!("{PREFS_FILE}: {e}"))
    }
}

/// Ids dos cortes de uma lista de linhas (para filtros por achado e seleção).
pub(crate) fn id_set<'a>(ids: impl IntoIterator<Item = &'a Id>) -> HashSet<String> {
    ids.into_iter().map(|i| i.0.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn row(num: u32, scene: &str, stages: &[(Stage, StageStatus)], due: Option<&str>, who: Option<&str>) -> CutRow {
        let mut map: BTreeMap<Stage, StageStatus> = ALL_STAGES.iter().map(|s| (*s, StageStatus::NotStarted)).collect();
        for (s, st) in stages {
            map.insert(*s, *st);
        }
        CutRow {
            cut: Id(format!("c{num}")),
            number: num,
            name: format!("C{num:03}"),
            episode: None,
            scene: Some(Id(scene.into())),
            scene_name: scene.into(),
            duration: 10 * num,
            stages: map,
            open_corrections: 0,
            retakes: 0,
            open_retakes: 0,
            due: due.map(str::to_owned),
            assignee: who.map(str::to_owned),
            weight: None,
            notes: String::new(),
            thumbnail: None,
        }
    }

    use StageStatus::*;

    fn sample() -> Vec<CutRow> {
        vec![
            row(1, "Ponte", &ALL_STAGES.map(|s| (s, Approved)), Some("2026-10-01"), Some("Ana")),
            row(2, "Ponte", &[(Stage::Layout, Approved), (Stage::Genga, InProgress)], Some("2026-10-10"), Some("Bia")),
            row(3, "Praça", &[(Stage::Layout, Review)], None, None),
            row(4, "Praça", &[(Stage::Layout, Approved), (Stage::Genga, Retake)], Some("2030-01-01"), Some("ana")),
        ]
    }

    #[test]
    fn current_stage_and_overall_status() {
        let rows = sample();
        assert_eq!(current_stage(&rows[0]), None);
        assert_eq!(current_stage(&rows[1]), Some(Stage::Genga));
        assert_eq!(current_stage(&rows[2]), Some(Stage::Layout));
        assert_eq!(current_stage(&rows[3]), Some(Stage::Genga));
        let st: Vec<StageStatus> = rows.iter().map(cut_status).collect();
        assert_eq!(st, vec![Approved, InProgress, Review, Retake]);
        assert!(is_done(&rows[0]) && !is_done(&rows[1]));
        assert!((progress(&rows[1]) - 0.125).abs() < 1e-6);
        assert!((progress(&rows[0]) - 1.0).abs() < 1e-6);
        assert_eq!(cut_status(&row(9, "X", &[], None, None)), NotStarted);
    }

    #[test]
    fn sorting_by_column_keeps_missing_values_last_in_both_directions() {
        let rows = sample();
        let order = |sort: Sort| {
            let mut v: Vec<&CutRow> = rows.iter().collect();
            v.sort_by(|a, b| compare(a, b, sort));
            v.iter().map(|r| r.number).collect::<Vec<_>>()
        };
        assert_eq!(order(Sort { col: Col::Cut, asc: false }), vec![4, 3, 2, 1]);
        assert_eq!(order(Sort { col: Col::Due, asc: true }), vec![1, 2, 4, 3]);
        assert_eq!(order(Sort { col: Col::Due, asc: false }), vec![4, 2, 1, 3]);
        // responsável sem diferenciar maiúsculas: Ana, ana (empate -> nº), Bia, ninguém
        assert_eq!(order(Sort { col: Col::Assignee, asc: true }), vec![1, 4, 2, 3]);
        assert_eq!(order(Sort { col: Col::Status, asc: false }), vec![4, 1, 3, 2]);
        assert_eq!(order(Sort { col: Col::Stage(Stage::Genga), asc: true }), vec![3, 2, 1, 4]);
        assert_eq!(order(Sort { col: Col::Frames, asc: false }), vec![4, 3, 2, 1]);
    }

    #[test]
    fn grouping_orders_groups_and_reports_unassigned_last() {
        let rows = sample();
        let refs: Vec<&CutRow> = rows.iter().collect();
        let by_scene = group_rows(refs.clone(), Group::Scene);
        assert_eq!(by_scene.iter().map(|(k, v)| (k.clone(), v.len())).collect::<Vec<_>>(), vec![(GroupKey::Scene("Ponte".into()), 2), (GroupKey::Scene("Praça".into()), 2)]);
        let by_stage = group_rows(refs.clone(), Group::Stage);
        let keys: Vec<GroupKey> = by_stage.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec![GroupKey::Stage(Some(Stage::Layout)), GroupKey::Stage(Some(Stage::Genga)), GroupKey::Stage(None)]);
        let by_who = group_rows(refs.clone(), Group::Assignee);
        let keys: Vec<GroupKey> = by_who.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, vec![GroupKey::Assignee(Some("Ana".into())), GroupKey::Assignee(Some("ana".into())), GroupKey::Assignee(Some("Bia".into())), GroupKey::Assignee(None)]);
        assert_eq!(by_who.last().unwrap().0, GroupKey::Assignee(None));
        let by_status = group_rows(refs.clone(), Group::Status);
        assert_eq!(by_status.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(), vec![GroupKey::Status(InProgress), GroupKey::Status(Review), GroupKey::Status(Approved), GroupKey::Status(Retake)]);
        assert_eq!(group_rows(refs, Group::None).len(), 1);
    }

    #[test]
    fn filters_combine_and_overdue_ignores_finished_cuts() {
        let rows = sample();
        let stuck: HashSet<String> = ["c3".to_owned()].into();
        let today = parse_date("2026-10-20").unwrap();
        let cx = FilterCtx { today, stuck: &stuck, only: None };
        let ids = |f: &Filters| rows.iter().filter(|r| f.matches(r, &cx)).map(|r| r.number).collect::<Vec<_>>();
        assert_eq!(ids(&Filters::default()), vec![1, 2, 3, 4]);
        // atrasado: o 1 venceu mas está completo; o 2 venceu e está incompleto; o 4 vence só em 2030
        assert_eq!(ids(&Filters { overdue: true, ..Default::default() }), vec![2]);
        assert_eq!(ids(&Filters { retake: true, ..Default::default() }), vec![4]);
        assert_eq!(ids(&Filters { stuck: true, ..Default::default() }), vec![3]);
        assert_eq!(ids(&Filters { stage: Some(Stage::Genga), status: Some(InProgress), ..Default::default() }), vec![2]);
        assert_eq!(ids(&Filters { status: Some(Review), ..Default::default() }), vec![3]);
        assert_eq!(ids(&Filters { stage: Some(Stage::Genga), ..Default::default() }), vec![2, 4]);
        assert_eq!(ids(&Filters { assignee: Some("Bia".into()), ..Default::default() }), vec![2]);
        assert_eq!(ids(&Filters { search: "praça c003".into(), ..Default::default() }), vec![3]);
        assert_eq!(ids(&Filters { search: "ana".into(), ..Default::default() }), vec![1, 4]);
        // filtro por achado
        let only: HashSet<String> = ["c2".to_owned(), "c4".to_owned()].into();
        let cx2 = FilterCtx { today, stuck: &stuck, only: Some(&only) };
        assert_eq!(rows.iter().filter(|r| Filters::default().matches(r, &cx2)).count(), 2);
        assert!(Filters::default().is_empty() && !Filters { retake: true, ..Default::default() }.is_empty());
    }

    #[test]
    fn prefs_round_trip_and_tolerate_old_files() {
        let dir = std::env::temp_dir().join(format!("aiai-prodprefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut p = ProdPrefs { view: ProdView::Thumbs, ..Default::default() };
        p.saved.push(SavedView { name: "Retakes".into(), view: ProdView::List, cols: vec![Col::Cut, Col::Stage(Stage::Douga)], sort: Sort { col: Col::Due, asc: false }, group: Group::Scene, filters: Filters { retake: true, ..Default::default() } });
        p.save(&dir).unwrap();
        let (back, warn) = ProdPrefs::load(&dir);
        assert!(warn.is_none());
        assert_eq!(back, p);
        // arquivo antigo com só um campo: o resto vem do padrão
        std::fs::write(dir.join(PREFS_FILE), r#"{"view":"board"}"#).unwrap();
        let (old, warn) = ProdPrefs::load(&dir);
        assert!(warn.is_none());
        assert_eq!((old.view, old.cols.len(), old.ai_enabled), (ProdView::Board, Col::defaults().len(), false));
        // arquivo ilegível: padrão + aviso (nada é engolido em silêncio)
        std::fs::write(dir.join(PREFS_FILE), "{ruim").unwrap();
        let (bad, warn) = ProdPrefs::load(&dir);
        assert_eq!(bad, ProdPrefs::default());
        assert!(warn.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
