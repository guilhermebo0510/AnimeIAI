//! Prévia de intercalação de um quadro (R16, MVP-0): antes de gravar, mostra o desenho que o motor atual
//! (`aiai_anim::inbetween`) faria num quadro entre duas chaves, com os pares de traços, o que ele preserva, aproxima ou
//! recusa, e só grava quando alguém aceita (um passo de desfazer, com o alcance escrito).
//!
//! * [`inbetween_preview`] é a função pura: lê o projeto, não grava nada.
//! * [`ProposalStore`] guarda a proposta na PRÓPRIA sessão (`CoreSession::ext_mut`): quem pediu a prévia (interface ou
//!   MCP) e quem aceita usam a mesma instância; trocar de documento ou perder a sessão perde a proposta, com erro
//!   explícito (sem cair em outra sessão).
//! * [`inbetween_frame_command`] monta o comando do aceite a partir do candidato guardado (não recalcula).

use crate::context::*;
use aiai_anim::{InbetweenOptions, MotionMode, align_work, correspond, correspond_work, inbetween_pairs, morph_path, pair_path, resampled_len};
use aiai_core::command::CellEdit;
use aiai_core::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Commit-base do motor (R16 §0 P-1; manifesto em `sprints/registro/S26-intercalacao/`).
pub const ENGINE_BASE: &str = "6655c0688eb2ebbdcae454df21e090a7f4e0bbfc";

/// Limites da prévia síncrona (R16 REQ-C02; valores **meta**, a fixar em P-3 pelo dono do produto).
pub const MAX_STROKES_PER_KEY: usize = 200;
pub const MAX_POINTS_TOTAL: usize = 20_000;
/// Pontos por traço de um par (o motor reamostra para `2 × max(N_A, N_B, 8)`; num fechado o alinhamento testa `n`
/// deslocamentos × 2 sentidos, trabalho ~2n²).
pub const MAX_PAIR_SAMPLES: usize = 1_000;
/// Pontos somados da saída (candidato e sobreposição).
pub const MAX_OUTPUT_POINTS: usize = 60_000;
/// Amostras somadas das duas chaves e do candidato (a conta exata de `aiai_paint::sample_count`: alças, fechamento,
/// controles da spline). A sobreposição desenha tudo isso; acima, a prévia recusa antes de expandir (CX10 C10-02,
/// CX11 C11-02, A7-04).
pub const MAX_PREVIEW_SAMPLES: usize = 400_000;
/// Trabalho de alinhamento somado na proposta (casar os traços + cada par fechado ~2n², a prévia e o motor), em
/// comparações ponto a ponto (A6-02: o limite por par sozinho deixava passar muitos pares fechados grandes).
pub const MAX_ALIGN_WORK: u64 = 20_000_000;

/// Caminho direto com sentido ou começo diferentes: recusa quando alinhar reduziria a distância média a menos da
/// metade e em mais de 1 px (abaixo disso é ruído de um desenho quase simétrico).
const SENSE_RATIO: f32 = 0.5;
const SENSE_MIN_PX: f32 = 1.0;

pub type ProposalId = u64;

/// Envelope atribuído pelo dono da sessão (R16 REQ-D02); o cliente nunca manda candidato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProposalEnvelope {
    pub id: ProposalId,
    pub document_epoch: u64,
    pub revision: u64,
    pub generation: u64,
}

fn default_max_cost() -> f32 {
    InbetweenOptions::default().max_cost
}

/// Pedido de prévia. `manual_pairs` é por `Stroke.id` (pares por índice não são aceitos).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InbetweenPreviewRequest {
    pub cut: Id,
    /// Camada vetorial com faixa no timesheet.
    pub layer: Id,
    /// Quadro (0-based) a intercalar, estritamente entre as duas chaves.
    pub frame: u32,
    /// Quadros de início dos blocos das chaves; sem eles, os blocos-chave mais próximos antes e depois de `frame`.
    #[serde(default)]
    pub frame_a: Option<u32>,
    #[serde(default)]
    pub frame_b: Option<u32>,
    /// Números dos cels-chave (conferidos contra os blocos; sem quadros, o bloco mais próximo que expõe cada um).
    #[serde(default)]
    pub key_a: Option<u32>,
    #[serde(default)]
    pub key_b: Option<u32>,
    #[serde(default)]
    pub motion: MotionMode,
    #[serde(default)]
    pub bend: f32,
    #[serde(default = "default_max_cost")]
    pub max_cost: f32,
    /// Pares manuais (id do traço em A, id do traço em B).
    #[serde(default)]
    pub manual_pairs: Vec<(Id, Id)>,
}

impl InbetweenPreviewRequest {
    pub fn new(cut: Id, layer: Id, frame: u32) -> Self {
        InbetweenPreviewRequest { cut, layer, frame, frame_a: None, frame_b: None, key_a: None, key_b: None, motion: MotionMode::Global, bend: 0.0, max_cost: default_max_cost(), manual_pairs: Vec::new() }
    }
}

/// Classe de suporte (R16 REQ-G01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Preserve,
    Approximate,
    Refuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoticeCode {
    // aproxima (aviso; aceite permitido)
    Resampled,
    BezierDirect,
    TensionDiffers,
    TipDiffers,
    ColorSwitch,
    Unmatched,
    GroupMorph,
    KeepsColor,
    // preserva, com observação sobre a prévia
    TipPreviewSimplified,
    // recusa (bloqueia o aceite)
    BezierResampled,
    BezierUnmatched,
    SenseMismatch,
    ProfileDiffers,
    ProfileReparametrized,
    ClosedMismatch,
    AliasedMismatch,
    ColorTraceMismatch,
    PointWidth,
    PointCorner,
    PointHandle,
    CelTransform,
    TooLarge,
    InvalidOutput,
}

impl NoticeCode {
    pub fn support(self) -> Support {
        use NoticeCode::*;
        match self {
            Resampled | BezierDirect | TensionDiffers | TipDiffers | ColorSwitch | Unmatched | GroupMorph | KeepsColor => Support::Approximate,
            TipPreviewSimplified => Support::Preserve,
            _ => Support::Refuse,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Notice {
    pub stroke_a: Option<Id>,
    pub stroke_b: Option<Id>,
    pub code: NoticeCode,
    pub text: String,
}

/// Par para desenhar as ligações (centros no espaço do cel).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PairView {
    pub a: Option<Id>,
    pub b: Option<Id>,
    pub a_center: Option<[f32; 2]>,
    pub b_center: Option<[f32; 2]>,
    /// Veio dos pares manuais.
    pub manual: bool,
    /// Caminho direto do motor (pontos copiados um a um).
    pub direct: bool,
    pub support: Support,
    /// Par, 1 para N do grupo (`a`/`b` = os dois traços que o motor interpola) ou traço sem par.
    pub kind: PairKind,
    /// Índice do traço que esta correspondência gera no candidato (`None` = não desenha neste quadro ou há recusa).
    pub candidate: Option<usize>,
    /// De qual traço a saída herda os atributos (ponta, perfil, cor no 1 para N...): o de A num par, o sem par nos demais.
    pub keeps: Option<Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PairKind {
    Pair,
    GroupMorph,
    Unmatched,
}

/// Proposta (R16 REQ-D01): só em memória, nunca gravada no projeto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InbetweenProposal {
    pub id: ProposalId,
    pub document_epoch: u64,
    pub revision: u64,
    pub generation: u64,
    /// Pares manuais mudaram e "Atualizar" ainda não rodou: aceite bloqueado.
    pub stale: bool,
    pub cut: Id,
    pub layer: Id,
    pub frame: u32,
    pub frame_a: u32,
    pub frame_b: u32,
    pub key_a: u32,
    pub key_b: u32,
    pub cel_a: Id,
    pub cel_b: Id,
    pub strokes_a: Vec<Id>,
    pub strokes_b: Vec<Id>,
    /// (frame − frame_a) / (frame_b − frame_a), em (0, 1).
    pub t: f32,
    /// Opções do motor, com os pares manuais já convertidos para índice.
    pub options: InbetweenOptions,
    pub manual_pairs: Vec<(Id, Id)>,
    pub pairs: Vec<PairView>,
    /// Exatamente o que será gravado (vazio quando há recusa).
    pub candidate: Vec<Stroke>,
    pub approximations: Vec<Notice>,
    pub refusals: Vec<Notice>,
    /// Preserva, com observação (ex.: acabamento da ponta não representado na prévia).
    pub notes: Vec<Notice>,
    /// Pedido que gerou esta proposta (Atualizar refaz com ele).
    pub request: InbetweenPreviewRequest,
}

impl InbetweenProposal {
    /// Cópia leve para a interface (sem o candidato, os pares e as listas de traços, que podem ter milhares de pontos).
    /// `acceptable` deve ser lido na proposta inteira.
    pub fn light(&self) -> InbetweenProposal {
        InbetweenProposal {
            id: self.id,
            document_epoch: self.document_epoch,
            revision: self.revision,
            generation: self.generation,
            stale: self.stale,
            cut: self.cut.clone(),
            layer: self.layer.clone(),
            frame: self.frame,
            frame_a: self.frame_a,
            frame_b: self.frame_b,
            key_a: self.key_a,
            key_b: self.key_b,
            cel_a: self.cel_a.clone(),
            cel_b: self.cel_b.clone(),
            strokes_a: Vec::new(),
            strokes_b: Vec::new(),
            t: self.t,
            options: self.options.clone(),
            manual_pairs: self.manual_pairs.clone(),
            pairs: Vec::new(),
            candidate: Vec::new(),
            approximations: self.approximations.clone(),
            refusals: self.refusals.clone(),
            notes: self.notes.clone(),
            request: self.request.clone(),
        }
    }

    /// Dá para aceitar? (`Err` = motivo, para a interface mostrar com o botão desligado.)
    pub fn acceptable(&self) -> Result<(), String> {
        if let Some(r) = self.refusals.first() {
            return Err(format!("{} recusa(s): {}", self.refusals.len(), r.text));
        }
        if self.stale {
            return Err("os pares mudaram: Atualizar antes de aceitar".into());
        }
        if self.candidate.is_empty() {
            return Err("a prévia não tem desenho para gravar".into());
        }
        Ok(())
    }
}

/// Começos de bloco da trilha (quadro, cel) antes de `duration`: entradas iguais seguidas são um bloco só.
pub fn block_starts(track: &Track, duration: u32) -> Vec<(u32, CellRef)> {
    let mut out: Vec<(u32, CellRef)> = Vec::new();
    for (&f, &c) in track.cells.range(..duration) {
        if out.last().map(|l| l.1) != Some(c) {
            out.push((f, c));
        }
    }
    out
}

fn notice(code: NoticeCode, a: Option<&Stroke>, b: Option<&Stroke>, text: impl Into<String>) -> Notice {
    Notice { stroke_a: a.map(|s| s.id.clone()), stroke_b: b.map(|s| s.id.clone()), code, text: text.into() }
}

fn center(s: &Stroke) -> Option<[f32; 2]> {
    if s.points.is_empty() {
        return None;
    }
    let n = s.points.len() as f32;
    Some([s.points.iter().map(|q| q.x).sum::<f32>() / n, s.points.iter().map(|q| q.y).sum::<f32>() / n])
}

fn label(s: &Stroke, side: &str, i: usize) -> String {
    if s.id.0.is_empty() { format!("{side}#{i}") } else { format!("{side}:{}", s.id.0) }
}

/// Resolve as duas chaves (R16 REQ-C01 item 2) pelos começos de bloco.
fn resolve_keys(p: &Project, cut: &CutData, req: &InbetweenPreviewRequest) -> ToolResult<((u32, u32, Id), (u32, u32, Id))> {
    let track = cut.xsheet.track_for_layer(&req.layer).ok_or_else(|| ToolError::Invalid("configure o timesheet desta camada (a prévia precisa da faixa da camada)".into()))?;
    let starts: Vec<(u32, u32, Id, CelRole)> = block_starts(track, cut.duration)
        .into_iter()
        .filter_map(|(f, c)| match c {
            CellRef::Cel { number } => {
                let n = p.cel_by_number(&req.layer, number)?;
                match &n.data {
                    NodeKind::Cel(cd) => Some((f, number, n.id.clone(), cd.role)),
                    _ => None,
                }
            }
            CellRef::Empty => None,
        })
        .collect();
    // os blocos que abrem um vão: chave ou breakdown, menos o que só volta a expor o desenho do limite anterior (a
    // entrada que um aceite grava para o quadro seguinte continuar como estava). Sem isso, depois do primeiro aceite
    // a prévia do quadro ao lado saía "entre Rosto1 → Rosto1" e com t errado (o vão passava a ser o da volta).
    let mut bounds: Vec<u32> = Vec::new();
    let mut last_bound: Option<u32> = None;
    for s in starts.iter().filter(|s| is_boundary(s.3)) {
        if last_bound != Some(s.1) {
            bounds.push(s.0);
        }
        last_bound = Some(s.1);
    }
    let opens = |s: &(u32, u32, Id, CelRole)| bounds.contains(&s.0);
    let pick = |explicit: Option<u32>, key: Option<u32>, before: bool| -> ToolResult<(u32, u32, Id)> {
        let side = if before { "A" } else { "B" };
        match explicit {
            Some(f) => {
                let s = starts.iter().find(|s| s.0 == f).ok_or_else(|| ToolError::Invalid(format!("o quadro {f} (chave {side}) não começa um bloco de desenho na faixa da camada")))?;
                if let Some(k) = key {
                    if s.1 != k {
                        return Err(ToolError::Invalid(format!("o bloco do quadro {f} expõe o desenho {}, não {k} (chave {side})", s.1)));
                    }
                }
                Ok((s.0, s.1, s.2.clone()))
            }
            None => {
                let side_ok = |s: &&(u32, u32, Id, CelRole)| if before { s.0 < req.frame } else { s.0 > req.frame };
                let found = match key {
                    // o bloco mais próximo que expõe esse desenho
                    Some(k) => {
                        let mut c: Vec<_> = starts.iter().filter(side_ok).filter(|s| s.1 == k).collect();
                        c.sort_by_key(|s| if before { u32::MAX - s.0 } else { s.0 });
                        c.first().copied()
                    }
                    // o bloco-chave (chave ou breakdown, A6-06) mais próximo
                    None => {
                        let mut c: Vec<_> = starts.iter().filter(side_ok).filter(|s| opens(s)).collect();
                        c.sort_by_key(|s| if before { u32::MAX - s.0 } else { s.0 });
                        c.first().copied()
                    }
                };
                let s = found.ok_or_else(|| {
                    ToolError::Invalid(format!("sem chave ou breakdown {} do quadro {} nesta camada (chave {side})", if before { "antes" } else { "depois" }, req.frame + 1))
                })?;
                Ok((s.0, s.1, s.2.clone()))
            }
        }
    };
    let a = pick(req.frame_a, req.key_a, true)?;
    let b = pick(req.frame_b, req.key_b, false)?;
    if !(a.0 < req.frame && req.frame < b.0) {
        return Err(ToolError::Invalid(format!("o quadro {} precisa ficar entre as chaves (quadros {} e {})", req.frame + 1, a.0 + 1, b.0 + 1)));
    }
    if b.0 >= cut.duration {
        return Err(ToolError::Invalid(format!("a chave B ({}) passa da duração do corte ({})", b.0, cut.duration)));
    }
    // o quadro alvo não pode ser o começo de uma chave ou breakdown (a prévia substituiria um desenho-chave na folha)
    if let Some(s) = starts.iter().find(|s| s.0 == req.frame && opens(s)) {
        return Err(ToolError::Invalid(format!("o quadro {} começa o desenho {} ({}); a prévia não substitui chave nem breakdown", req.frame + 1, s.1, role_name(s.3))));
    }
    Ok((a, b))
}

/// Chave e breakdown delimitam o vão da intercalação.
fn is_boundary(r: CelRole) -> bool {
    matches!(r, CelRole::Key | CelRole::Breakdown)
}

fn role_name(r: CelRole) -> &'static str {
    match r {
        CelRole::Key => "chave",
        CelRole::Breakdown => "breakdown",
        CelRole::Inbetween => "intercalação",
    }
}

/// As chaves que a prévia usaria (quadro A, desenho A, quadro B, desenho B), sem calcular nada: para ligar ou não o
/// comando na interface.
pub fn resolve_frames(p: &Project, req: &InbetweenPreviewRequest) -> ToolResult<(u32, u32, u32, u32)> {
    let cut = cut_data(p, &req.cut)?;
    if layer_data(p, &req.layer)?.kind != LayerType::Vector {
        return Err(ToolError::Invalid("a prévia de intercalação é só para camada vetorial".into()));
    }
    if req.frame >= cut.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora do corte", req.frame)));
    }
    let ((fa, ka, _), (fb, kb, _)) = resolve_keys(p, cut, req)?;
    Ok((fa, ka, fb, kb))
}

/// Índice do traço com este id no cel (erro se não existir ou se o id se repetir no cel).
fn index_of(strokes: &[Stroke], id: &Id, side: &str) -> ToolResult<usize> {
    if id.0.is_empty() {
        return Err(ToolError::Invalid(format!("par manual com id vazio (chave {side})")));
    }
    let hits: Vec<usize> = strokes.iter().enumerate().filter(|(_, s)| &s.id == id).map(|(i, _)| i).collect();
    match hits.as_slice() {
        [i] => Ok(*i),
        [] => Err(ToolError::NotFound(format!("traço {id} na chave {side} (par manual)"))),
        _ => Err(ToolError::Invalid(format!("o id {id} aparece {} vezes na chave {side}: par manual ambíguo", hits.len()))),
    }
}

/// Converte os pares manuais (R16 REQ-C01 item 3): nada é ignorado em silêncio.
fn manual_indices(a: &[Stroke], b: &[Stroke], pairs: &[(Id, Id)]) -> ToolResult<Vec<(usize, usize)>> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (ia, ib) in pairs {
        let i = index_of(a, ia, "A")?;
        let j = index_of(b, ib, "B")?;
        if out.iter().any(|p| p.0 == i) {
            return Err(ToolError::Invalid(format!("o traço {ia} de A está em mais de um par manual")));
        }
        if out.iter().any(|p| p.1 == j) {
            return Err(ToolError::Invalid(format!("o traço {ib} de B está em mais de um par manual")));
        }
        let (sa, sb) = (&a[i], &b[j]);
        let mut why = Vec::new();
        if sa.closed != sb.closed {
            why.push("um é fechado e o outro aberto");
        }
        if sa.color_trace != sb.color_trace {
            why.push("só um é linha de cor");
        }
        if sa.aliased != sb.aliased {
            why.push("só um é sem antisserrilhado");
        }
        if !why.is_empty() {
            return Err(ToolError::Invalid(format!("par manual {ia} × {ib} incompatível: {}", why.join(", "))));
        }
        out.push((i, j));
    }
    Ok(out)
}

/// Entrada de cada traço (R16 §5): atributos que o motor não leva adiante recusam.
fn classify_input(s: &Stroke, side: &str, i: usize, refusals: &mut Vec<Notice>) {
    let (a, b) = if side == "A" { (Some(s), None) } else { (None, Some(s)) };
    if s.points.iter().any(|q| q.width != 1.0) {
        refusals.push(notice(NoticeCode::PointWidth, a, b, format!("{}: largura por ponto ≠ 1 (o motor grava 1)", label(s, side, i))));
    }
    if s.points.iter().any(|q| q.corner) {
        refusals.push(notice(NoticeCode::PointCorner, a, b, format!("{}: ponto marcado como canto (o motor não preserva)", label(s, side, i))));
    }
    if s.points.iter().any(|q| q.handle.is_some()) {
        refusals.push(notice(NoticeCode::PointHandle, a, b, format!("{}: ponto com alça (o motor não preserva)", label(s, side, i))));
    }
}

/// Grupo com um par casado (o motor faz 1 para N: o traço sem par nasce do par do grupo).
fn group_partner(a: &[Stroke], b: &[Stroke], pairs: &[aiai_anim::Pair], g: &str) -> Option<(usize, usize)> {
    if g.is_empty() {
        return None;
    }
    pairs.iter().find_map(|&(i, j)| match (i, j) {
        (Some(i), Some(j)) if a[i].group == g && b[j].group == g => Some((i, j)),
        _ => None,
    })
}

/// Prévia de um quadro (R16 REQ-C01): calcula, classifica e devolve a proposta. Nada é gravado.
pub fn inbetween_preview(p: &Project, env: ProposalEnvelope, req: &InbetweenPreviewRequest) -> ToolResult<InbetweenProposal> {
    // 1. camada e quadro
    let cut = cut_data(p, &req.cut)?;
    let ld = layer_data(p, &req.layer)?;
    if p.cut_of(&req.layer).as_ref() != Some(&req.cut) {
        return Err(ToolError::Invalid(format!("a camada {} não é deste corte", req.layer)));
    }
    if ld.kind != LayerType::Vector {
        return Err(ToolError::Invalid("a prévia de intercalação é só para camada vetorial".into()));
    }
    if req.frame >= cut.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora do corte (duração {})", req.frame, cut.duration)));
    }
    if !(req.bend.is_finite() && (-2.0..=2.0).contains(&req.bend) && req.max_cost.is_finite() && req.max_cost >= 0.0) {
        return Err(ToolError::Invalid("bend entre -2 e 2 e max_cost ≥ 0 (números finitos)".into()));
    }
    // 2. chaves pelos começos de bloco
    let ((frame_a, key_a, cel_a), (frame_b, key_b, cel_b)) = resolve_keys(p, cut, req)?;
    let (ca, cb) = match (&node(p, &cel_a)?.data, &node(p, &cel_b)?.data) {
        (NodeKind::Cel(x), NodeKind::Cel(y)) => (x, y),
        _ => return Err(ToolError::Invalid("as chaves precisam ser cels".into())),
    };
    let (a, b) = (&ca.strokes, &cb.strokes);
    let t = (req.frame - frame_a) as f32 / (frame_b - frame_a) as f32;
    if !(t.is_finite() && t > 0.0 && t < 1.0) {
        return Err(ToolError::Invalid(format!("tempo fora de (0, 1): {t}")));
    }
    // 3. pares manuais por id
    let manual = manual_indices(a, b, &req.manual_pairs)?;
    let options = InbetweenOptions { motion: req.motion, bend: req.bend, pairs: manual.clone(), max_cost: req.max_cost };
    let mut prop = InbetweenProposal {
        id: env.id,
        document_epoch: env.document_epoch,
        revision: env.revision,
        generation: env.generation,
        stale: false,
        cut: req.cut.clone(),
        layer: req.layer.clone(),
        frame: req.frame,
        frame_a,
        frame_b,
        key_a,
        key_b,
        cel_a,
        cel_b,
        strokes_a: a.iter().map(|s| s.id.clone()).collect(),
        strokes_b: b.iter().map(|s| s.id.clone()).collect(),
        t,
        options: options.clone(),
        manual_pairs: req.manual_pairs.clone(),
        pairs: Vec::new(),
        candidate: Vec::new(),
        approximations: Vec::new(),
        refusals: Vec::new(),
        notes: Vec::new(),
        request: req.clone(),
    };
    let ident = [0.0, 0.0, 1.0, 0.0];
    if ca.transform != ident || cb.transform != ident {
        prop.refusals.push(notice(NoticeCode::CelTransform, None, None, "uma das chaves tem transformação de cel (a intercalação ignora o transform): desfaça a transformação ou desenhe no lugar"));
    }
    // limites antes do motor: contagens, comprimento desenhado (amostras da sobreposição e do render) e trabalho de
    // alinhamento somado (a correspondência e cada par fechado custam ~n²)
    let pts_total: usize = a.iter().chain(b).map(|s| s.points.len()).sum();
    let key_samples: usize = a.iter().chain(b).map(aiai_paint::sample_count).fold(0usize, usize::saturating_add);
    let mut too_large: Option<String> = None;
    if a.len() > MAX_STROKES_PER_KEY || b.len() > MAX_STROKES_PER_KEY || pts_total > MAX_POINTS_TOTAL {
        too_large = Some(format!("{} e {} traços, {} pontos; limite {} traços por chave e {} pontos", a.len(), b.len(), pts_total, MAX_STROKES_PER_KEY, MAX_POINTS_TOTAL));
    } else if key_samples > MAX_PREVIEW_SAMPLES {
        too_large = Some(format!("{key_samples} amostras de traço nas duas chaves; limite {MAX_PREVIEW_SAMPLES}"));
    } else {
        let w = correspond_work(a, b, &options);
        if w > MAX_ALIGN_WORK {
            too_large = Some(format!("casar os traços custaria {w} comparações; limite {MAX_ALIGN_WORK}"));
        }
    }
    if let Some(why) = too_large {
        prop.refusals.push(notice(NoticeCode::TooLarge, None, None, format!("desenho grande demais para a prévia ({why})")));
        return Ok(prop);
    }
    // 4. entrada
    for (i, s) in a.iter().enumerate() {
        classify_input(s, "A", i, &mut prop.refusals);
    }
    for (i, s) in b.iter().enumerate() {
        classify_input(s, "B", i, &mut prop.refusals);
    }
    // 5. pares (os mesmos do motor, calculados uma vez) e classificação de cada correspondência efetiva
    let pairs = correspond(a, b, &options);
    let mut out_points = 0usize;
    let mut work = correspond_work(a, b, &options);
    for &(ia, ib) in &pairs {
        let manual_pair = matches!((ia, ib), (Some(i), Some(j)) if manual.contains(&(i, j)));
        let mut notes: Vec<Notice> = Vec::new();
        let mut direct = false;
        let mut kind = PairKind::Pair;
        let mut keeps: Option<&Stroke> = ia.map(|i| &a[i]).or(ib.map(|j| &b[j]));
        // os dois traços que o motor interpola nesta correspondência (no 1 para N, o par do grupo)
        let mut link: (Option<&Stroke>, Option<&Stroke>) = (ia.map(|i| &a[i]), ib.map(|j| &b[j]));
        match (ia, ib) {
            (Some(i), Some(j)) => {
                let (sa, sb) = (&a[i], &b[j]);
                let (la, lb) = (label(sa, "A", i), label(sb, "B", j));
                let is_direct = sa.points.len() == sb.points.len() && sa.geometry == sb.geometry && sa.closed == sb.closed;
                let n = if is_direct { sa.points.len() } else { resampled_len(sa.points.len(), sb.points.len()) };
                // a prévia alinha uma vez para classificar e o motor de novo para gerar
                work += 2 * align_work(n, sa.closed);
                if n > MAX_PAIR_SAMPLES || work > MAX_ALIGN_WORK {
                    notes.push(notice(NoticeCode::TooLarge, Some(sa), Some(sb), format!("{la} × {lb}: {n} pontos por traço; o alinhamento somado passa do limite da prévia ({MAX_PAIR_SAMPLES} por par, {MAX_ALIGN_WORK} no total)")));
                } else {
                    let path = pair_path(sa, sb);
                    direct = path.direct;
                    out_points += path.samples;
                    let realigned = path.reversed || path.shift != 0;
                    let bezier = sa.geometry == StrokeGeometry::Bezier || sb.geometry == StrokeGeometry::Bezier;
                    if path.direct {
                        if realigned && path.aligned_cost < path.identity_cost * SENSE_RATIO && path.identity_cost - path.aligned_cost > SENSE_MIN_PX {
                            notes.push(notice(NoticeCode::SenseMismatch, Some(sa), Some(sb), format!("{la} × {lb}: sentido diferente entre A e B")));
                        }
                        if bezier {
                            notes.push(notice(NoticeCode::BezierDirect, Some(sa), Some(sb), format!("{la} × {lb}: interpolação aproximada dos controles (Bézier)")));
                        }
                    } else if bezier {
                        notes.push(notice(NoticeCode::BezierResampled, Some(sa), Some(sb), format!("{la} × {lb}: Bézier com contagens diferentes (a reamostragem trata os controles como curva)")));
                    } else {
                        notes.push(notice(NoticeCode::Resampled, Some(sa), Some(sb), format!("{la} × {lb}: pontos reamostrados ({n} por traço); o traço vira spline")));
                    }
                    profile_checks(sa, sb, realigned, &la, &lb, &mut notes);
                }
                attribute_checks(sa, sb, &la, &lb, false, &mut notes);
            }
            (Some(i), None) | (None, Some(i)) => {
                let side_a = ia.is_some();
                let s = if side_a { &a[i] } else { &b[i] };
                let (na, nb) = if side_a { (Some(s), None) } else { (None, Some(s)) };
                let ls = label(s, if side_a { "A" } else { "B" }, i);
                match group_partner(a, b, &pairs, &s.group) {
                    Some((pi, pj)) => {
                        // 1 para N: o motor interpola `from` → `to` e guarda os atributos de `s` (o traço sem par)
                        kind = PairKind::GroupMorph;
                        keeps = Some(s);
                        let other = if side_a { &b[pj] } else { &a[pi] };
                        let lo = if side_a { label(other, "B", pj) } else { label(other, "A", pi) };
                        let (from, to) = if side_a { (s, other) } else { (other, s) };
                        link = if side_a { (Some(s), Some(other)) } else { (Some(other), Some(s)) };
                        let n = resampled_len(from.points.len(), to.points.len());
                        // a prévia alinha uma vez para classificar (morph_path) e o motor de novo (CX11 C11-03)
                        work += 2 * align_work(n, from.closed && to.closed);
                        if n > MAX_PAIR_SAMPLES || work > MAX_ALIGN_WORK {
                            notes.push(notice(NoticeCode::TooLarge, na, nb, format!("{ls}: {n} pontos por traço no grupo; o alinhamento somado passa do limite da prévia")));
                        } else if s.geometry == StrokeGeometry::Bezier || other.geometry == StrokeGeometry::Bezier {
                            notes.push(notice(NoticeCode::BezierResampled, na, nb, format!("{ls}: Bézier no grupo 1 para N (vira reamostragem)")));
                        } else {
                            out_points += n;
                            let path = morph_path(from, to);
                            notes.push(notice(NoticeCode::GroupMorph, na, nb, format!("{ls}: sem par; nasce de {lo}, do grupo \"{}\" (reamostrado, vira spline)", s.group)));
                            let (la, lb) = if side_a { (ls.clone(), lo.clone()) } else { (lo.clone(), ls.clone()) };
                            profile_checks(from, to, path.reversed || path.shift != 0, &la, &lb, &mut notes);
                            attribute_checks(from, to, &la, &lb, true, &mut notes);
                        }
                    }
                    None if s.geometry == StrokeGeometry::Bezier => {
                        kind = PairKind::Unmatched;
                        notes.push(notice(NoticeCode::BezierUnmatched, na, nb, format!("{ls}: Bézier sem par (viraria polilinha)")));
                    }
                    None => {
                        kind = PairKind::Unmatched;
                        out_points += s.points.len();
                        notes.push(notice(NoticeCode::Unmatched, na, nb, format!("{ls}: sem par: aparece/some aos poucos; durante o desenho fica aberto e um fragmento curto pode sumir")));
                    }
                }
            }
            (None, None) => {}
        }
        // recusa de entrada de um dos traços (largura por ponto, canto, alça) também recusa a correspondência
        let ids = [link.0.map(|s| &s.id), link.1.map(|s| &s.id)];
        let input_refused = prop.refusals.iter().any(|n| (n.stroke_a.is_some() && n.stroke_a.as_ref() == ids[0]) || (n.stroke_b.is_some() && n.stroke_b.as_ref() == ids[1]));
        let support = if input_refused || notes.iter().any(|n| n.code.support() == Support::Refuse) {
            Support::Refuse
        } else if notes.iter().any(|n| n.code.support() == Support::Approximate) {
            Support::Approximate
        } else {
            Support::Preserve
        };
        prop.pairs.push(PairView {
            a: link.0.map(|s| s.id.clone()),
            b: link.1.map(|s| s.id.clone()),
            a_center: link.0.and_then(center),
            b_center: link.1.and_then(center),
            manual: manual_pair,
            direct,
            support,
            kind,
            candidate: None,
            keeps: keeps.map(|s| s.id.clone()),
        });
        for n in notes {
            match n.code.support() {
                Support::Refuse => prop.refusals.push(n),
                Support::Approximate => prop.approximations.push(n),
                Support::Preserve => prop.notes.push(n),
            }
        }
    }
    if out_points > MAX_OUTPUT_POINTS {
        prop.refusals.push(notice(NoticeCode::TooLarge, None, None, format!("desenho grande demais para a prévia ({out_points} pontos na saída; limite {MAX_OUTPUT_POINTS})")));
    }
    // 6. candidato, só sem recusa (com os pares já calculados: o motor não casa de novo)
    if !prop.refusals.is_empty() {
        return Ok(prop);
    }
    let per_pair = inbetween_pairs(a, b, t, &options, &pairs);
    let mut candidate = Vec::new();
    for (k, s) in per_pair.into_iter().enumerate() {
        if let Some(s) = s {
            prop.pairs[k].candidate = Some(candidate.len());
            candidate.push(s);
        }
    }
    // 7. saída válida, finita e dentro do comprimento desenhável
    for (k, s) in candidate.iter().enumerate() {
        let finite = s.width.is_finite() && s.opacity.is_finite() && s.points.iter().all(|q| q.x.is_finite() && q.y.is_finite() && q.pressure.is_finite());
        if let Err(e) = s.validate().and_then(|_| if finite { Ok(()) } else { Err("valor não finito".into()) }) {
            prop.refusals.push(notice(NoticeCode::InvalidOutput, None, None, format!("traço {k} da saída inválido: {e}")));
        }
    }
    let out_samples = candidate.iter().map(aiai_paint::sample_count).fold(key_samples, usize::saturating_add);
    if out_samples > MAX_PREVIEW_SAMPLES {
        prop.refusals.push(notice(NoticeCode::TooLarge, None, None, format!("desenho grande demais para a prévia ({out_samples} amostras com a saída; limite {MAX_PREVIEW_SAMPLES})")));
    }
    if candidate.is_empty() {
        prop.refusals.push(notice(NoticeCode::InvalidOutput, None, None, "o motor não gerou nenhum traço"));
    }
    if prop.refusals.is_empty() {
        prop.candidate = candidate;
    } else {
        for pv in &mut prop.pairs {
            pv.candidate = None;
        }
    }
    Ok(prop)
}

/// Perfil de largura: diferente recusa; igual e com o começo invertido/deslocado também (o perfil segue o parâmetro).
fn profile_checks(sa: &Stroke, sb: &Stroke, realigned: bool, la: &str, lb: &str, notes: &mut Vec<Notice>) {
    if sa.profile != sb.profile {
        notes.push(notice(NoticeCode::ProfileDiffers, Some(sa), Some(sb), format!("{la} × {lb}: perfis de largura diferentes")));
    } else if !sa.profile.0.is_empty() && realigned {
        notes.push(notice(NoticeCode::ProfileReparametrized, Some(sa), Some(sb), format!("{la} × {lb}: o perfil de largura não acompanha a inversão/deslocamento do começo")));
    }
}

/// Atributos do traço que o motor não interpola (R16 §5). No 1 para N (`morph`), cor e opacidade ficam as do traço sem
/// par, sem interpolar: aviso.
fn attribute_checks(sa: &Stroke, sb: &Stroke, la: &str, lb: &str, morph: bool, notes: &mut Vec<Notice>) {
    if sa.closed != sb.closed {
        notes.push(notice(NoticeCode::ClosedMismatch, Some(sa), Some(sb), format!("{la} × {lb}: um fechado e um aberto")));
    }
    if sa.aliased != sb.aliased {
        notes.push(notice(NoticeCode::AliasedMismatch, Some(sa), Some(sb), format!("{la} × {lb}: antisserrilhado diferente")));
    }
    if sa.color_trace != sb.color_trace {
        notes.push(notice(NoticeCode::ColorTraceMismatch, Some(sa), Some(sb), format!("{la} × {lb}: só um é linha de cor")));
    }
    let who = if morph { "a do traço sem par" } else { "a de A" };
    if (sa.tension - sb.tension).abs() > 1e-4 {
        notes.push(notice(NoticeCode::TensionDiffers, Some(sa), Some(sb), format!("{la} × {lb}: tensões diferentes (usa {who})")));
    }
    if sa.tip != sb.tip {
        notes.push(notice(NoticeCode::TipDiffers, Some(sa), Some(sb), format!("{la} × {lb}: pontas diferentes (usa {who})")));
    } else if matches!(sa.tip, VectorTip::Square | VectorTip::Genga) {
        notes.push(notice(NoticeCode::TipPreviewSimplified, Some(sa), Some(sb), format!("{la} × {lb}: prévia simplificada: acabamento da ponta/textura não representado")));
    }
    if morph {
        if sa.color != sb.color || (sa.opacity - sb.opacity).abs() > 1e-4 {
            notes.push(notice(NoticeCode::KeepsColor, Some(sa), Some(sb), format!("{la} × {lb}: cor/opacidade não interpoladas no 1 para N (usa {who})")));
        }
    } else if sa.color != sb.color && !(matches!(sa.color, ColorRef::Direct { .. }) && matches!(sb.color, ColorRef::Direct { .. })) {
        notes.push(notice(NoticeCode::ColorSwitch, Some(sa), Some(sb), format!("{la} × {lb}: cores de paleta diferentes: troca na metade (t = 0,5)")));
    }
}

// ---------------------------------------------------------------------------------------------- aceite (REQ-A01…A05)

/// O que o aceite faz (texto do botão, R16 REQ-A05).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AcceptScope {
    pub number: u32,
    pub frame: u32,
    /// `frame + 1` ganha uma entrada que mantém o que já estava lá (`None` = nada a gravar).
    pub restore_next: Option<(u32, CellRef)>,
    /// O quadro alvo já começava outro desenho (entrada explícita na folha): ele sai dali.
    pub replaces: Option<CellRef>,
}

impl AcceptScope {
    pub fn button_text(&self, layer_name: &str) -> String {
        let mut s = format!("Criar desenho {layer_name}{} no quadro {}", self.number, self.frame + 1);
        if let Some(c) = self.replaces {
            let what = match c {
                CellRef::Cel { number } => format!("{layer_name}{number}"),
                CellRef::Empty => "o vazio".into(),
            };
            s.push_str(&format!(" no lugar de {what}"));
        }
        if let Some((f, c)) = self.restore_next {
            let what = match c {
                CellRef::Cel { number } => format!("{layer_name}{number}"),
                CellRef::Empty => "vazio".into(),
            };
            s.push_str(&format!(" (o quadro {} continua mostrando {what})", f + 1));
        }
        s
    }
}

/// Alcance do aceite sem montar o comando.
pub fn accept_scope(p: &Project, prop: &InbetweenProposal) -> ToolResult<AcceptScope> {
    let cut = cut_data(p, &prop.cut)?;
    let track = cut.xsheet.track_for_layer(&prop.layer).ok_or_else(|| ToolError::Invalid("configure o timesheet desta camada".into()))?;
    // o próximo número livre; a camada que já usa o maior número possível não tem "próximo" (CX16 C16-08)
    let number = match p.node(&prop.layer).and_then(|l| l.children.iter().filter_map(|c| p.node(c)).filter_map(|n| match &n.data {
        NodeKind::Cel(c) => Some(c.number),
        _ => None,
    }).max()) {
        None => 1,
        Some(max) => max.checked_add(1).ok_or_else(|| ToolError::Invalid(format!("a camada já tem um cel de número {max}, o maior possível: renumere os cels para criar um novo")))?,
    };
    let next = prop.frame + 1;
    let restore_next = (next < cut.duration && !track.cells.contains_key(&next)).then(|| (next, track.exposed_at(next).unwrap_or(CellRef::Empty)));
    let replaces = track.cells.get(&prop.frame).copied();
    Ok(AcceptScope { number, frame: prop.frame, restore_next, replaces })
}

/// Precondições do aceite que dependem do projeto (R16 REQ-A02; época, revisão, geração e consumo são do [`ProposalStore`]).
/// A camada ou uma pasta acima dela está travada? (a mesma trava efetiva do resto do editor, A6-03)
pub fn layer_locked_effective(p: &Project, layer: &Id) -> Option<String> {
    let mut cur = Some(layer.clone());
    while let Some(id) = cur {
        let n = p.node(&id)?;
        match &n.data {
            NodeKind::Layer(l) if l.locked => {
                return Some(if &id == layer { "a camada está travada: destrave para aceitar".into() } else { format!("a pasta \"{}\" está travada: destrave para aceitar", n.name) });
            }
            NodeKind::Layer(_) => cur = n.parent.clone(),
            _ => return None,
        }
    }
    None
}

/// Por que esta proposta não pode ser aceita agora, ou `None`: a MESMA regra para a interface, o estado da interface,
/// o MCP e o aceite (CX10 C10-07, A6-03).
pub fn blocked_reason(p: &Project, prop: &InbetweenProposal) -> Option<String> {
    if let Err(e) = prop.acceptable() {
        return Some(e);
    }
    check_project(p, prop).err().map(|e| e.to_string())
}

fn check_project(p: &Project, prop: &InbetweenProposal) -> ToolResult<()> {
    prop.acceptable().map_err(ToolError::Invalid)?;
    let cut = cut_data(p, &prop.cut)?;
    let ld = layer_data(p, &prop.layer)?;
    if ld.kind != LayerType::Vector {
        return Err(ToolError::Invalid("a camada não é mais vetorial".into()));
    }
    if let Some(why) = layer_locked_effective(p, &prop.layer) {
        return Err(ToolError::Invalid(why));
    }
    if cut.xsheet.track_for_layer(&prop.layer).is_none() {
        return Err(ToolError::Invalid("configure o timesheet desta camada".into()));
    }
    if !(prop.frame_a < prop.frame && prop.frame < prop.frame_b) || prop.frame_b >= cut.duration {
        return Err(ToolError::Invalid("o quadro não está mais entre as chaves dentro do corte".into()));
    }
    Ok(())
}

/// Comando do aceite (R16 REQ-A01/A03): cel novo com o candidato guardado, `frame` → cel novo e, se precisar, a entrada
/// que mantém `frame + 1`. Um `Batch`, um passo de desfazer. Não recalcula.
pub fn inbetween_frame_command(p: &Project, idgen: &mut IdGen, prop: &InbetweenProposal, source: InputSource) -> ToolResult<(Command, AcceptScope)> {
    check_project(p, prop)?;
    let cut = cut_data(p, &prop.cut)?;
    let track = cut.xsheet.track_for_layer(&prop.layer).expect("conferido");
    let scope = accept_scope(p, prop)?;
    let mut cel = CelData::new(scope.number);
    cel.role = CelRole::Inbetween;
    cel.strokes = prop.candidate.clone();
    cel.inbetween_origin = Some(InbetweenOrigin {
        source,
        mechanism: "inbetween".into(),
        engine: ENGINE_BASE.into(),
        options: serde_json::to_value(&prop.options).map_err(|e| ToolError::Invalid(e.to_string()))?,
        key_a: prop.key_a,
        key_b: prop.key_b,
        cel_a: prop.cel_a.clone(),
        cel_b: prop.cel_b.clone(),
        strokes_a: prop.strokes_a.clone(),
        strokes_b: prop.strokes_b.clone(),
        frame_a: prop.frame_a,
        frame_b: prop.frame_b,
        frame: prop.frame,
        t: prop.t,
        revision: prop.revision,
    });
    let layer_name = p.node(&prop.layer).map(|n| n.name.clone()).unwrap_or_default();
    let mut cells = vec![CellEdit { frame: prop.frame, cell: Some(CellRef::Cel { number: scope.number }) }];
    if let Some((f, c)) = scope.restore_next {
        cells.push(CellEdit { frame: f, cell: Some(c) });
    }
    let cmd = Command::Batch {
        commands: vec![
            Command::CreateNode { id: Some(idgen.next()), parent: Some(prop.layer.clone()), index: None, name: format!("{layer_name}{}", scope.number), data: NodeKind::Cel(cel) },
            Command::SetCells { cut: prop.cut.clone(), track: track.id.clone(), cells },
        ],
        label: Some(format!("Intercalar no quadro {} entre {} e {}", prop.frame + 1, prop.key_a, prop.key_b)),
    };
    Ok((cmd, scope))
}

// ---------------------------------------------------------------------------------------------- dono da proposta

/// Propostas presas a uma sessão (R16 REQ-D02/M01): uma proposta viva por vez (a nova substitui a anterior).
#[derive(Debug, Default)]
pub struct ProposalStore {
    next_id: ProposalId,
    generation: u64,
    pub current: Option<InbetweenProposal>,
    /// Ids já aceitos (para responder "proposta já usada" em vez de "não encontrada").
    consumed: Vec<ProposalId>,
    /// Motivo da última invalidação (a interface mostra na barra de status com "gerar de novo").
    pub invalidated: Option<(String, InbetweenPreviewRequest)>,
}

/// Maior inteiro exato em JSON (2^53): os ids cabem abaixo dele.
const MAX_EXACT_ID: u64 = 1 << 53;

/// Começo aleatório dos ids de uma loja de propostas (CX16 C16-07): cada sessão (o app, o MCP headless, uma sessão
/// reaberta) sorteia o seu bloco de 2^16 ids entre 2^37 blocos, sem depender do relógio — duas lojas criadas no mesmo
/// milissegundo não começam iguais. Sorteio pelo `RandomState` (chaves aleatórias do sistema por processo, diferentes
/// a cada instância), misturado com o processo, a hora e um contador: um id de outra sessão não acha proposta aqui.
fn random_id_base() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static STORES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u32(std::process::id());
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u64(STORES.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let base = (h.finish() % MAX_EXACT_ID) & !0xFFFF;
    base.clamp(1 << 16, MAX_EXACT_ID - (1 << 17))
}

impl ProposalStore {
    fn envelope(&mut self, s_epoch: u64, revision: u64) -> ProposalEnvelope {
        if self.next_id == 0 || self.next_id + 1 >= MAX_EXACT_ID {
            self.next_id = random_id_base();
        }
        self.next_id += 1;
        self.generation += 1;
        ProposalEnvelope { id: self.next_id, document_epoch: s_epoch, revision, generation: self.generation }
    }
}

/// Pede uma prévia nova (substitui a anterior) e devolve a proposta guardada.
/// Pedido que falha não mexe na proposta viva nem no aviso da que caiu (A7-01: um Atualizar com par inválido não
/// apaga a prévia nem os pares já feitos).
pub fn preview(s: &mut CoreSession, req: &InbetweenPreviewRequest) -> ToolResult<InbetweenProposal> {
    let (epoch, revision) = (s.document_epoch(), s.project.revision);
    let saved = (0, s.ext_mut::<ProposalStore>().generation);
    let env = s.ext_mut::<ProposalStore>().envelope(epoch, revision);
    let prop = inbetween_preview(&s.project, env, req);
    let store = s.ext_mut::<ProposalStore>();
    match prop {
        Ok(prop) => {
            store.invalidated = None;
            store.current = Some(prop.clone());
            Ok(prop)
        }
        Err(e) => {
            // o envelope não foi usado: a geração volta (a proposta viva continua aceitável); o id gasto não volta
            store.generation = saved.1;
            Err(e)
        }
    }
}

/// Proposta atual com este id (erros explícitos: outra época, substituída, já usada, descartada).
fn current_checked(s: &mut CoreSession, id: ProposalId) -> ToolResult<InbetweenProposal> {
    invalidate_if_changed(s);
    let (epoch, revision) = (s.document_epoch(), s.project.revision);
    let store = s.ext_mut::<ProposalStore>();
    if store.consumed.contains(&id) {
        return Err(ToolError::Invalid(format!("proposta {id} já usada")));
    }
    let Some(prop) = store.current.clone().filter(|p| p.id == id) else {
        let why = match (&store.current, &store.invalidated) {
            (Some(c), _) => format!("a proposta {id} foi substituída pela {}", c.id),
            (None, Some((r, _))) => format!("a proposta {id} não vale mais: {r}"),
            (None, None) => format!("proposta {id} não encontrada nesta sessão (descartada, ou a sessão foi reaberta)"),
        };
        return Err(ToolError::Stale(why));
    };
    if prop.document_epoch != epoch {
        return Err(ToolError::Stale("o documento foi trocado ou reaberto depois da prévia".into()));
    }
    if prop.revision != revision {
        return Err(ToolError::Stale(format!("o projeto mudou depois da prévia (revisão {} → {revision})", prop.revision)));
    }
    if prop.generation != store.generation {
        return Err(ToolError::Stale("resultado de uma geração antiga da prévia".into()));
    }
    Ok(prop)
}

/// Invalida a proposta se o documento ou a revisão mudaram (sem recalcular, R10 H-3). Devolve o motivo quando invalidou.
pub fn invalidate_if_changed(s: &mut CoreSession) -> Option<String> {
    let (epoch, revision) = (s.document_epoch(), s.project.revision);
    let store = s.ext_mut::<ProposalStore>();
    let prop = store.current.as_ref()?;
    let why = if prop.document_epoch != epoch {
        "o documento foi trocado ou reaberto".to_owned()
    } else if prop.revision != revision {
        format!("o projeto mudou (revisão {} → {revision})", prop.revision)
    } else {
        return None;
    };
    let req = prop.request.clone();
    store.current = None;
    store.generation += 1;
    store.invalidated = Some((why.clone(), req));
    Some(why)
}

/// Muda os pares manuais da proposta atual: fica desatualizada (aceite bloqueado) até [`refresh`].
pub fn set_manual_pairs(s: &mut CoreSession, id: ProposalId, pairs: Vec<(Id, Id)>) -> ToolResult<()> {
    let prop = current_checked(s, id)?;
    // o par é conferido na hora (ids, repetição, fechado/aberto, linha de cor, antisserrilhado): par inválido é recusado
    // e os pares de antes ficam (A7-01)
    let strokes = |cel: &Id| match s.project.node(cel).map(|n| &n.data) {
        Some(NodeKind::Cel(c)) => c.strokes.clone(),
        _ => Vec::new(),
    };
    manual_indices(&strokes(&prop.cel_a), &strokes(&prop.cel_b), &pairs)?;
    let store = s.ext_mut::<ProposalStore>();
    let prop = store.current.as_mut().expect("conferida");
    prop.request.manual_pairs = pairs;
    prop.stale = true;
    Ok(())
}

/// Muda as opções do motor (movimento, arco, custo máximo) da proposta atual: desatualizada até [`refresh`] (A6-08).
pub fn set_options(s: &mut CoreSession, id: ProposalId, motion: MotionMode, bend: f32, max_cost: f32) -> ToolResult<()> {
    if !(bend.is_finite() && (-2.0..=2.0).contains(&bend) && max_cost.is_finite() && max_cost >= 0.0) {
        return Err(ToolError::Invalid("bend entre -2 e 2 e max_cost ≥ 0".into()));
    }
    current_checked(s, id)?;
    let store = s.ext_mut::<ProposalStore>();
    let prop = store.current.as_mut().expect("conferida");
    if (prop.request.motion, prop.request.bend, prop.request.max_cost) != (motion, bend, max_cost) {
        prop.request.motion = motion;
        prop.request.bend = bend;
        prop.request.max_cost = max_cost;
        prop.stale = true;
    }
    Ok(())
}

/// Atualizar: refaz a prévia com o pedido (e os pares) atuais; a proposta nova tem outro id e substitui a anterior.
pub fn refresh(s: &mut CoreSession, id: ProposalId) -> ToolResult<InbetweenProposal> {
    let req = {
        let store = s.ext_mut::<ProposalStore>();
        match store.current.as_ref().filter(|p| p.id == id) {
            Some(p) => p.request.clone(),
            None => return Err(ToolError::Stale(format!("proposta {id} não é a atual"))),
        }
    };
    preview(s, &req)
}

/// Descartar: a proposta some (o projeto não muda).
pub fn discard(s: &mut CoreSession, id: ProposalId) -> ToolResult<()> {
    let store = s.ext_mut::<ProposalStore>();
    match store.current.as_ref() {
        Some(p) if p.id == id => {
            store.current = None;
            store.generation += 1;
            Ok(())
        }
        _ if store.consumed.contains(&id) => Err(ToolError::Invalid(format!("proposta {id} já usada"))),
        _ => Err(ToolError::Stale(format!("proposta {id} não é a atual"))),
    }
}

/// Resultado do aceite.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Accepted {
    pub outcome: Outcome,
    pub cel: Id,
    pub number: u32,
    /// Quadros cujas entradas mudaram.
    pub frames: Vec<u32>,
}

/// Aceitar (R16 REQ-A04): precondições, `apply_as` com a barreira de revisão e consumo único. A fonte gravada é a da
/// sessão no momento (interface: a do quadro; MCP: `DirectToolCall`).
pub fn accept(s: &mut CoreSession, id: ProposalId, author: &str) -> ToolResult<Accepted> {
    let prop = current_checked(s, id)?;
    let source = s.source;
    let (cmd, scope) = inbetween_frame_command(&s.project, &mut s.ids, &prop, source)?;
    let outcome = s.apply_as(cmd, author, Some(prop.revision))?;
    let store = s.ext_mut::<ProposalStore>();
    store.consumed.push(id);
    if store.consumed.len() > 256 {
        store.consumed.remove(0);
    }
    store.current = None;
    store.generation += 1;
    let cel = outcome.created.first().cloned().ok_or_else(|| ToolError::Invalid("o aceite não criou o cel".into()))?;
    let mut frames = vec![prop.frame];
    frames.extend(scope.restore_next.map(|r| r.0));
    Ok(Accepted { outcome, cel, number: scope.number, frames })
}

/// A última prévia que caiu (motivo e pedido, para "gerar de novo"), enquanto ninguém pediu outra.
pub fn invalidated(s: &CoreSession) -> Option<&(String, InbetweenPreviewRequest)> {
    s.ext::<ProposalStore>().and_then(|st| st.invalidated.as_ref())
}

/// Esquece o aviso da prévia que caiu.
pub fn clear_invalidated(s: &mut CoreSession) {
    s.ext_mut::<ProposalStore>().invalidated = None;
}

/// Proposta viva da sessão (para a interface e o render com prévia).
pub fn current(s: &CoreSession) -> Option<&InbetweenProposal> {
    s.ext::<ProposalStore>().and_then(|st| st.current.as_ref())
}
