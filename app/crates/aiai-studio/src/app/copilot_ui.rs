//! Copiloto de traço na interface (protótipo): caixas de sugestão no canvas, fila única, fantasma magenta, Tab/Esc.
//!
//! O gerador roda num processo separado (aiai_tools::copilot); aqui a interface só prepara o contexto (rápido) e
//! manda para uma thread que espera o serviço. Nunca duas caixas gerando ao mesmo tempo: a caixa onde se acabou de
//! desenhar passa na frente; as outras esperam. Nada entra no documento até aceitar (um passo de desfazer).
//!
//! Versões (rodada CP-R02): cada pedido leva a versão da geometria da caixa, do conteúdo dela e do histórico. O
//! resultado que chega depois de desfazer, desligar, descartar, mover, redimensionar ou apagar a caixa é descartado
//! (nunca "ressuscita"); o que chega depois de um traço novo na caixa aparece esmaecido até chegar o novo.
//! O MCP, com o app aberto, usa a mesma fila e a mesma thread, e o fantasma dele aparece no canvas (`bridge`).

use super::editor::Xf;
use super::ui_kit::Env;
use aiai_bridge::host::HostRequest;
use aiai_core::{Id, Stroke};
use aiai_tools::copilot::{self as cp, AcceptWhich, Suggestion, SuggestRequest};
use aiai_tools::vectorize::WidthMode;
use egui::{Color32, Pos2, Rect, RichText, Vec2};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

/// Estado de uma caixa: ligada (entra na fila), pausada (guarda o fantasma, não gera e o foco pausado não se move),
/// desligada (sem fantasma e sem geração).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BoxState {
    On,
    Paused,
    Off,
}

#[derive(Clone, Debug)]
pub(crate) struct CBox {
    pub id: u32,
    pub cut: Id,
    /// [x, y, largura, altura] no canvas.
    pub rect: [f32; 4],
    pub prompt: String,
    pub state: BoxState,
    /// Semente da geração ("outra versão" troca).
    pub seed: u64,
    /// Caixa de foco: a única automática do corte, que vai para onde você desenha. As outras são fixas (criadas
    /// com + ou fixadas a partir do foco) e ficam onde estão.
    pub auto: bool,
    /// Camada de onde a caixa lê o desenho (a do traço que a criou ou moveu; o cartão mostra e deixa trocar).
    pub layer: Option<Id>,
    /// Somar o prompt geral ao desta caixa (desligue numa caixa sobre a mão quando o geral descreve o rosto).
    pub use_global: bool,
    /// Versão da geometria/estado (mover, redimensionar, desligar): resultado de outra versão é descartado.
    pub geo: u64,
    /// Versão do conteúdo (traço novo na caixa): resultado de versão anterior aparece esmaecido.
    pub content: u64,
    /// Caixa criada por um agente (MCP).
    pub llm: bool,
    /// Assinatura do que a caixa leu no último pedido (cel exposto, traços da janela de contexto, movimento da camada,
    /// paletas) e a janela (espaço do cel): uma edição só a refaz se isto mudar.
    pub src: u64,
    pub src_region: Option<[f32; 4]>,
}

/// O fantasma de uma caixa. `vis_*` é o que aparece de fato (sem o que uma caixa menor cobre), no espaço do cel: é o
/// que a pintura, o ímã e o aceitar usam.
pub(crate) struct Ghost {
    pub box_id: u32,
    pub s: Suggestion,
    /// Desenhou-se na caixa depois: fica mais claro até chegar o novo.
    pub stale: bool,
    pub vis_improve: Vec<Stroke>,
    pub vis_complete: Vec<Stroke>,
}

/// Arrasto de caixa no canvas.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// Criando: canto de partida.
    New(Vec2),
    /// Movendo: caixa e deslocamento do ponteiro até o canto.
    Move(u32, Vec2),
    /// Redimensionando pelo canto oposto fixo.
    Resize(u32, Vec2),
}

/// Caixa esperando a vez, com o quadro em que o pedido nasceu (o pedido é desse quadro, não do que estiver na tela).
#[derive(Clone, Copy, Debug)]
struct Pending {
    id: u32,
    at: Instant,
    frame: u32,
}

struct Job {
    box_id: u32,
    geo: u64,
    content: u64,
    hist: u64,
    prep: cp::Prepared,
}

/// Resultado da thread: caixa, versões do pedido e a sugestão (ou o erro).
type Done = (u32, u64, u64, u64, Result<Suggestion, String>);

pub(crate) struct CopilotUi {
    pub on: bool,
    pub improve: bool,
    pub complete: bool,
    pub boxes: Vec<CBox>,
    next_box: u32,
    pub global_prompt: String,
    pub global_on: bool,
    /// Passos do modelo (0 = os da mudança escolhida).
    pub steps: u32,
    /// Quanto a sugestão pode mudar o desenho (0 = só limpar … 1 = criar a partir do esboço).
    pub change: f32,
    /// Resolução de geração em px (0 = automática: o tamanho da janela de contexto, de 384 a 512).
    pub res: u32,
    pub opacity: f32,
    pub pause_s: f32,
    /// Força do ímã (0 = desligado).
    pub magnet: f32,
    /// Cor da caixa e do fantasma (opacas; a do fantasma recebe a opacidade `opacity`).
    pub box_color: Color32,
    pub ghost_color: Color32,
    /// Aceitar Melhorar substitui os traços do rascunho que a versão limpa cobre, dentro da caixa (desligado: mantém o
    /// rascunho).
    pub replace_rough: bool,
    /// Largura do traço aceito: do desenho gerado, da caneta (com a variação do desenho) ou fixa.
    pub width_mode: WidthMode,
    pub ghosts: Vec<Ghost>,
    /// Caixas esperando a vez.
    dirty: Vec<Pending>,
    /// Pedidos na thread (caixa, desde quando), na ordem.
    inflight: Vec<(u32, Instant)>,
    /// Versão do histórico: sobe ao desfazer/refazer, desligar e descartar (resultados em voo caem).
    hist: u64,
    /// Os `vis_*` dos fantasmas precisam ser refeitos (caixas ou fantasmas mudaram).
    vis_dirty: bool,
    /// Pedidos do MCP esperando o resultado de uma caixa (caixa, versão do conteúdo, pedido).
    bridge_wait: Vec<(u32, u64, HostRequest)>,
    tx: Option<Sender<Job>>,
    rx: Option<Receiver<Done>>,
    pub new_box_mode: bool,
    drag: Option<Drag>,
    pub last: String,
    /// Avisos da última sugestão: resolução baixada pela memória, GPU limitada, prompt cortado.
    pub last_reduced: bool,
    pub last_throttled: bool,
    pub last_truncated: bool,
    /// Desfazer/refazer já vistos (`Session::history_moves`): mudou, os fantasmas caem.
    seen_moves: u64,
    /// Mudanças no documento já vistas (`Session::change_counter`): outra edição (transformar, preencher, colar…)
    /// põe as caixas ligadas do corte na fila.
    seen_change: u64,
    /// Onde aceitar: na própria camada ou numa camada limpa acima.
    pub target: cp::AcceptTarget,
    /// Tirar o foco do teclado no próximo quadro (depois do Tab que aceitou).
    drop_focus: bool,
    /// Pedido ao dock: mostrar (true) ou esconder (false) o painel Copiloto.
    pub panel_req: Option<bool>,
    /// Um campo de texto tinha o teclado no quadro anterior (lido em `keys`).
    typing: bool,
    /// `project_sig` no último Salvar/Abrir: diferente = caixas ou prompts não salvos.
    saved_sig: u64,
}

impl Default for CopilotUi {
    fn default() -> Self {
        Self {
            on: false,
            improve: true,
            complete: true,
            boxes: Vec::new(),
            next_box: 1,
            global_prompt: String::new(),
            global_on: true,
            steps: 0,
            change: cp::CHANGE_DEFAULT,
            res: 0,
            opacity: 0.25,
            pause_s: 0.4,
            magnet: 0.7,
            box_color: BOX_COLOR,
            ghost_color: GHOST_COLOR,
            replace_rough: true,
            width_mode: WidthMode::Drawing,
            ghosts: Vec::new(),
            dirty: Vec::new(),
            inflight: Vec::new(),
            hist: 0,
            vis_dirty: false,
            bridge_wait: Vec::new(),
            tx: None,
            rx: None,
            new_box_mode: false,
            drag: None,
            last: String::new(),
            last_reduced: false,
            last_throttled: false,
            last_truncated: false,
            seen_moves: 0,
            seen_change: 0,
            target: cp::AcceptTarget::Same,
            drop_focus: false,
            panel_req: None,
            typing: false,
            saved_sig: 0,
        }
    }
}

/// Cores padrão da caixa e do fantasma: fora das cores de trabalho do genga e do douga (preto = linha, vermelho e azul =
/// sombra e luz), para nunca se confundir com o desenho. Trocáveis em Ajustes.
pub(crate) const BOX_COLOR: Color32 = Color32::from_rgb(22, 170, 80);
pub(crate) const GHOST_COLOR: Color32 = Color32::from_rgb(214, 36, 160);

fn worker(c: &mut CopilotUi) -> &Sender<Job> {
    if c.tx.is_none() {
        let (tx, jobs) = channel::<Job>();
        let (done, rx) = channel::<Done>();
        std::thread::Builder::new()
            .name("copiloto".into())
            .spawn(move || {
                for j in jobs {
                    let r = cp::finish(j.prep).map_err(|e| e.to_string());
                    if done.send((j.box_id, j.geo, j.content, j.hist, r)).is_err() {
                        break;
                    }
                }
            })
            .expect("thread do copiloto");
        c.tx = Some(tx);
        c.rx = Some(rx);
    }
    c.tx.as_ref().unwrap()
}

/// A caixa volta para a fila, para o quadro `frame` (o fantasma dela fica velho até chegar o novo). Só caixas ligadas,
/// e só com alguma sugestão pedida (Melhorar ou Completar).
fn requeue(c: &mut CopilotUi, id: u32, frame: u32) {
    if !(c.improve || c.complete) {
        return;
    }
    let Some(b) = c.boxes.iter_mut().find(|b| b.id == id && b.state == BoxState::On) else { return };
    b.content += 1;
    for g in c.ghosts.iter_mut().filter(|g| g.box_id == id) {
        g.stale = true;
    }
    // já esperando: só atualiza a hora (não entra duas vezes)
    c.dirty.retain(|d| d.id != id);
    c.dirty.push(Pending { id, at: Instant::now(), frame });
}

/// Todas as caixas ligadas do corte voltam para a fila (mudou um ajuste ou o prompt geral).
fn requeue_cut(c: &mut CopilotUi, cut: Option<&Id>, frame: u32) {
    let ids: Vec<u32> = c.boxes.iter().filter(|b| Some(&b.cut) == cut).map(|b| b.id).collect();
    for id in ids {
        requeue(c, id, frame);
    }
}

/// A caixa mudou de lugar, de tamanho ou de estado: o pedido em voo e o fantasma dela não valem mais.
fn invalidate_box(c: &mut CopilotUi, id: u32) {
    if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
        b.geo += 1;
    }
    c.ghosts.retain(|g| g.box_id != id);
    c.vis_dirty = true;
}

/// Todos os pedidos em voo deixam de valer (desfazer/refazer, desligar, descartar).
fn invalidate_all(c: &mut CopilotUi) {
    c.hist += 1;
    c.vis_dirty = true;
}

/// Caixas e prompts salvos com o projeto: `copilot.json` ao lado do `project.json`, gravado ao Salvar (fora do desfazer,
/// como as réguas de um programa de desenho). As caixas de agente (MCP) são da sessão e não entram.
const SIDECAR: &str = "copilot.json";

fn state_name(s: BoxState) -> &'static str {
    match s {
        BoxState::On => "on",
        BoxState::Paused => "paused",
        BoxState::Off => "off",
    }
}

/// Assinatura do que vai para o `copilot.json` (as caixas que não são de agente, os prompts e os ajustes).
fn project_sig(c: &CopilotUi) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for b in c.boxes.iter().filter(|b| !b.llm) {
        (b.id, &b.cut, &b.prompt, state_name(b.state), b.seed, b.auto, &b.layer, b.use_global).hash(&mut h);
        b.rect.map(f32::to_bits).hash(&mut h);
    }
    (&c.global_prompt, c.global_on, c.improve, c.complete, c.change.to_bits(), c.res, c.steps).hash(&mut h);
    h.finish()
}

/// As caixas, os prompts ou os ajustes mudaram desde o último Salvar ou Abrir (o projeto tem trabalho não salvo)?
pub(crate) fn project_state_dirty(c: &CopilotUi) -> bool {
    // 0 = ainda sem projeto aberto ou novo (a referência nasce ao abrir ou criar)
    c.saved_sig != 0 && project_sig(c) != c.saved_sig
}

/// Grava as caixas e os ajustes do projeto. Sem caixa nem prompt e sem arquivo anterior, não cria nada.
pub(crate) fn save_project_state(c: &mut CopilotUi, dir: &std::path::Path) -> std::io::Result<()> {
    save_project_file(c, dir)?;
    c.saved_sig = project_sig(c);
    Ok(())
}

fn save_project_file(c: &CopilotUi, dir: &std::path::Path) -> std::io::Result<()> {
    let path = dir.join(SIDECAR);
    let boxes: Vec<serde_json::Value> = c
        .boxes
        .iter()
        .filter(|b| !b.llm)
        .map(|b| {
            serde_json::json!({
                "id": b.id, "cut": b.cut, "rect": b.rect, "prompt": b.prompt, "state": state_name(b.state), "seed": b.seed,
                "auto": b.auto, "layer": b.layer, "use_global": b.use_global,
            })
        })
        .collect();
    if boxes.is_empty() && c.global_prompt.trim().is_empty() && !path.exists() {
        return Ok(());
    }
    let v = serde_json::json!({
        "version": 1, "boxes": boxes, "global_prompt": c.global_prompt, "global_on": c.global_on, "improve": c.improve,
        "complete": c.complete, "change": c.change, "res": c.res, "steps": c.steps,
    });
    let tmp = dir.join("copilot.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&v).map_err(std::io::Error::other)?)?;
    std::fs::rename(&tmp, &path)
}

/// Outro projeto aberto (ou novo): fantasmas, fila e pedidos do projeto anterior caem, e as caixas e os ajustes vêm do
/// `copilot.json` dele (sem o arquivo, nenhuma caixa). Devolve um aviso se o arquivo existe mas não se lê.
pub(crate) fn load_project_state(c: &mut CopilotUi, dir: Option<&std::path::Path>) -> Option<String> {
    c.ghosts.clear();
    c.dirty.clear();
    c.drag = None;
    c.new_box_mode = false;
    invalidate_all(c);
    fail_waiters(c, None, "outro projeto foi aberto");
    c.boxes.clear();
    c.global_prompt.clear();
    c.next_box = 1;
    // os ajustes que o copilot.json guarda são do projeto: voltam ao padrão antes de ler (projeto sem arquivo não herda
    // os do anterior, CP3-07)
    let d = CopilotUi::default();
    (c.global_on, c.improve, c.complete, c.change, c.res, c.steps) = (d.global_on, d.improve, d.complete, d.change, d.res, d.steps);
    // o estado recém-aberto é o "salvo" (abrir não deixa o projeto sujo)
    let warn = load_project_file(c, dir);
    c.saved_sig = project_sig(c);
    warn
}

fn load_project_file(c: &mut CopilotUi, dir: Option<&std::path::Path>) -> Option<String> {
    let path = dir?.join(SIDECAR);
    // sem arquivo é o normal (nenhuma caixa); arquivo que existe e não se lê (acesso negado, disco) avisa (CP3-07)
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return Some(format!("{}: {e}", path.display())),
    };
    let v: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return Some(format!("{}: {e}", path.display())),
    };
    let f = |k: &str| v.get(k);
    if let Some(x) = f("global_prompt").and_then(|x| x.as_str()) {
        c.global_prompt = x.to_owned();
    }
    for (k, slot) in [("global_on", &mut c.global_on), ("improve", &mut c.improve), ("complete", &mut c.complete)] {
        if let Some(x) = f(k).and_then(|x| x.as_bool()) {
            *slot = x;
        }
    }
    if let Some(x) = f("change").and_then(|x| x.as_f64()) {
        c.change = (x as f32).clamp(0.0, 1.0);
    }
    if let Some(x) = f("res").and_then(|x| x.as_u64()) {
        c.res = x as u32;
    }
    if let Some(x) = f("steps").and_then(|x| x.as_u64()) {
        c.steps = (x as u32).min(12);
    }
    for b in f("boxes").and_then(|x| x.as_array()).into_iter().flatten() {
        let rect: Option<[f32; 4]> = serde_json::from_value(b.get("rect").cloned().unwrap_or_default()).ok();
        // id no domínio u32 (menos o último, para o próximo id não estourar): sem truncar nem colidir (CP3-06)
        let id = b.get("id").and_then(|x| x.as_u64()).and_then(|x| u32::try_from(x).ok()).filter(|x| *x >= 1 && *x < u32::MAX);
        let (Some(id), Some(cut), Some(rect)) = (id, b.get("cut").and_then(|x| x.as_str()), rect) else { continue };
        if !rect.iter().all(|x| x.is_finite()) || rect[2] < 4.0 || rect[3] < 4.0 || c.boxes.iter().any(|x| x.id == id) {
            continue;
        }
        let state = match b.get("state").and_then(|x| x.as_str()) {
            Some("paused") => BoxState::Paused,
            Some("off") => BoxState::Off,
            _ => BoxState::On,
        };
        c.boxes.push(CBox {
            id,
            cut: Id::from(cut),
            rect,
            prompt: b.get("prompt").and_then(|x| x.as_str()).unwrap_or_default().to_owned(),
            state,
            seed: b.get("seed").and_then(|x| x.as_u64()).unwrap_or(1),
            auto: b.get("auto").and_then(|x| x.as_bool()).unwrap_or(false),
            layer: b.get("layer").and_then(|x| x.as_str()).map(Id::from),
            use_global: b.get("use_global").and_then(|x| x.as_bool()).unwrap_or(true),
            geo: 0,
            content: 0,
            llm: false,
            src: 0,
            src_region: None,
        });
    }
    // uma caixa por vez: arquivo antigo (várias caixas, foco + fixas) fica só com a primeira; as outras não ficam
    // escondidas na fila nem no fantasma
    let dropped = c.boxes.len().saturating_sub(1);
    c.boxes.truncate(1);
    c.next_box = c.boxes.iter().map(|b| b.id.saturating_add(1)).max().unwrap_or(1);
    (dropped > 0).then(|| format!("{}: {dropped} caixa(s) a mais do formato antigo (uma caixa por vez) não foram abertas", path.display()))
}

/// Próximo id de caixa: sem estourar no fim do u32 (volta a 1) e sem repetir um id em uso (CP3-06).
fn new_box_id(c: &mut CopilotUi) -> u32 {
    loop {
        let id = c.next_box.max(1);
        c.next_box = if id >= u32::MAX - 1 { 1 } else { id + 1 };
        if !c.boxes.iter().any(|b| b.id == id) {
            return id;
        }
    }
}

/// Responde (com erro) os pedidos do MCP que esperavam uma caixa.
fn fail_waiters(c: &mut CopilotUi, id: Option<u32>, msg: &str) {
    let mut keep = Vec::new();
    for (bid, ver, r) in std::mem::take(&mut c.bridge_wait) {
        if id.is_none_or(|x| x == bid) {
            r.respond(Err(aiai_bridge::BridgeError::invalid_params(msg)));
        } else {
            keep.push((bid, ver, r));
        }
    }
    c.bridge_wait = keep;
}

/// Liga/desliga. Ao ligar, o serviço começa a carregar os modelos (sem esperar e sem travar a interface). Ao desligar,
/// os fantasmas e a fila somem e o resultado em andamento é descartado.
pub(crate) fn set_on(env: &mut Env, on: bool) {
    env.ed.copilot.on = on;
    env.ed.copilot.panel_req = Some(on);
    env.ed.copilot.seen_change = env.core.change_counter;
    env.ed.copilot.seen_moves = env.core.history_moves;
    if !on {
        let c = &mut env.ed.copilot;
        c.ghosts.clear();
        c.dirty.clear();
        c.new_box_mode = false;
        c.drag = None;
        invalidate_all(c);
        fail_waiters(c, None, "o Copiloto foi desligado");
    }
    if on {
        if let Err(e) = cp::start_service() {
            env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
            env.ed.copilot.on = false;
        }
    }
}

/// Reinicia o serviço depois de uma falha (botão do painel).
pub(crate) fn restart(env: &mut Env) {
    cp::stop_service();
    if let Err(e) = cp::start_service() {
        env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
    }
}

/// Um traço terminou no corte. **Uma caixa por vez** (decisão de 30/10 com o dono: várias caixas enchiam o painel de
/// prompts, brigavam na sobreposição e, numa GPU de 4 a 6 GB, só esperavam na fila):
/// - sem caixa, o traço forma a caixa em volta dele e gera;
/// - traço que toca a caixa a atualiza (ela cresce se o traço sai dela) e gera de novo;
/// - traço fora dela, com sugestão à vista: é só desenho; outra caixa só depois de aceitar (Tab) ou descartar (Esc);
/// - traço fora dela sem sugestão à vista (gerando, nada sugerido, recém-aberta): a caixa vai até o traço.
pub(crate) fn on_stroke(env: &mut Env, cut: &Id, bbox: [f32; 4], traced: bool, make_box: bool) {
    // esta mudança já foi tratada aqui (o `tick` não precisa refazer o corte todo)
    env.ed.copilot.seen_change = env.core.change_counter;
    let layer = env.ed.layer.clone();
    let frame = env.ed.frame;
    let c = &mut env.ed.copilot;
    if !c.on || !(c.improve || c.complete) || traced {
        return;
    }
    let (cx, cy) = (bbox[0] + bbox[2] / 2.0, bbox[1] + bbox[3] / 2.0);
    let Some(b) = c.boxes.first().cloned() else {
        if make_box {
            let min = bbox[2].max(bbox[3]).max(256.0);
            let id = new_box_id(c);
            c.boxes.push(CBox { id, cut: cut.clone(), rect: [cx - min / 2.0, cy - min / 2.0, min, min], prompt: String::new(), state: BoxState::On, seed: 1, auto: true, layer, use_global: true, geo: 0, content: 0, llm: false, src: 0, src_region: None });
            requeue(c, id, frame);
        }
        return;
    };
    let id = b.id;
    let r = b.rect;
    let touches = &b.cut == cut && bbox[0] <= r[0] + r[2] && bbox[0] + bbox[2] >= r[0] && bbox[1] <= r[1] + r[3] && bbox[1] + bbox[3] >= r[1];
    let shown = c.ghosts.iter().any(|g| g.box_id == id && !(g.vis_improve.is_empty() && g.vis_complete.is_empty()));
    if b.state != BoxState::On {
        // pausada ou desligada: guarda o lugar, a fonte e o fantasma (CP1-24, CP2-08)
        return;
    }
    if touches {
        let bx = c.boxes.iter_mut().find(|x| x.id == id).unwrap();
        let mut moved = false;
        // a caixa lê a camada em que você está desenhando
        if bx.layer != layer {
            bx.layer = layer.clone();
            moved = true;
        }
        // o traço saiu da caixa (mais de 4 px): ela cresce para cobri-lo, com folga (até 1024 px de lado); traço que fica
        // dentro, mesmo perto da borda, não mexe nela
        let out = bbox[0] < r[0] - 4.0 || bbox[1] < r[1] - 4.0 || bbox[0] + bbox[2] > r[0] + r[2] + 4.0 || bbox[1] + bbox[3] > r[1] + r[3] + 4.0;
        let (x0, y0) = (r[0].min(bbox[0] - 16.0), r[1].min(bbox[1] - 16.0));
        let (x1, y1) = ((r[0] + r[2]).max(bbox[0] + bbox[2] + 16.0), (r[1] + r[3]).max(bbox[1] + bbox[3] + 16.0));
        if out {
            let (w, h) = ((x1 - x0).min(1024.0), (y1 - y0).min(1024.0));
            bx.rect = [x0.max(cx - w / 2.0).min(x1 - w), y0.max(cy - h / 2.0).min(y1 - h), w, h];
            moved = true;
        }
        if moved {
            invalidate_box(c, id);
        }
        requeue(c, id, frame);
    } else if !shown && make_box {
        // sem sugestão à vista: a caixa vai até o traço (no tamanho que já tem, pelo menos 256 e cobrindo o traço)
        let bx = c.boxes.iter_mut().find(|x| x.id == id).unwrap();
        let (w, h) = (r[2].max(bbox[2] + 32.0).max(256.0), r[3].max(bbox[3] + 32.0).max(256.0));
        bx.cut = cut.clone();
        bx.layer = layer;
        bx.rect = [cx - w / 2.0, cy - h / 2.0, w, h];
        bx.auto = true;
        bx.llm = false;
        invalidate_box(c, id);
        requeue(c, id, frame);
    }
}

/// Tira a caixa (aceita por inteiro, descartada ou consumida pelo ímã): some do canvas e do painel.
fn remove_box(c: &mut CopilotUi, id: u32) {
    invalidate_box(c, id);
    c.boxes.retain(|b| b.id != id);
    c.dirty.retain(|d| d.id != id);
    fail_waiters(c, Some(id), "a caixa foi aceita ou descartada");
}

/// Uma vez por quadro: recebe o que ficou pronto e manda a próxima caixa quando a vez chegar.
pub(crate) fn tick(env: &mut Env, ctx: &egui::Context) {
    if std::mem::take(&mut env.ed.copilot.drop_focus) {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    let moves = env.core.history_moves;
    let change = env.core.change_counter;
    let frame = env.ed.frame;
    if moves != env.ed.copilot.seen_moves {
        // desfazer/refazer: os fantasmas e os pedidos em voo foram calculados sobre um desenho que não existe mais (não
        // gera de novo sozinho)
        let c = &mut env.ed.copilot;
        c.seen_moves = moves;
        c.seen_change = change;
        c.ghosts.clear();
        // o que esperava a vez também cai (desfazer não gera de novo sozinho, CP2-06)
        c.dirty.clear();
        invalidate_all(c);
    } else if change != env.ed.copilot.seen_change {
        // outra edição (transformar, preencher, colar, apagar camada…): geram de novo só as caixas ligadas do corte
        // cujo desenho lido mudou (pintar outra camada, renomear ou mexer em outro corte não gasta GPU)
        env.ed.copilot.seen_change = change;
        if env.ed.copilot.on {
            if let Some(cut) = env.ed.cut.clone() {
                let hit: Vec<u32> = env
                    .ed
                    .copilot
                    .boxes
                    .iter()
                    .filter(|b| b.cut == cut && b.state == BoxState::On)
                    .filter(|b| {
                        let layer = b.layer.clone().filter(|l| env.core.project.node(l).is_some()).or_else(|| env.ed.layer.clone());
                        layer.is_none_or(|l| b.src_region.is_none() || cp::source_signature(&env.core.project, &cut, &l, frame, b.src_region) != b.src)
                    })
                    .map(|b| b.id)
                    .collect();
                for id in hit {
                    requeue(&mut env.ed.copilot, id, frame);
                }
            }
        }
    }
    // o que ficou pronto (recebe tudo primeiro, depois mexe no estado)
    let mut done = Vec::new();
    if let Some(rx) = env.ed.copilot.rx.as_ref() {
        while let Ok(x) = rx.try_recv() {
            done.push(x);
        }
    }
    for d in done {
        receive(env, d);
    }
    if std::mem::take(&mut env.ed.copilot.vis_dirty) {
        refresh_visible(&mut env.ed.copilot);
    }
    let c = &env.ed.copilot;
    if !c.on {
        return;
    }
    if !c.inflight.is_empty() || !c.dirty.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    if !c.inflight.is_empty() {
        return;
    }
    // nada pedido: a fila não gera à toa
    if !(c.improve || c.complete) {
        env.ed.copilot.dirty.clear();
        return;
    }
    // a vez: a caixa que mudou por último, depois da pausa
    let pause = std::time::Duration::from_secs_f32(c.pause_s);
    let Some(&p) = c.dirty.iter().max_by_key(|d| d.at) else { return };
    if p.at.elapsed() < pause {
        return;
    }
    env.ed.copilot.dirty.retain(|d| d.id != p.id);
    let Some(b) = env.ed.copilot.boxes.iter().find(|b| b.id == p.id && b.state == BoxState::On).cloned() else { return };
    let Some(layer) = b.layer.clone().filter(|l| env.core.project.node(l).is_some()).or_else(|| env.ed.layer.clone()) else { return };
    let c = &env.ed.copilot;
    let mut prompt = b.prompt.clone();
    // o da caixa primeiro (pesa mais: diz o que há NESTA caixa); o geral depois, como contexto do todo
    if c.global_on && b.use_global && !c.global_prompt.trim().is_empty() {
        prompt = if prompt.is_empty() { c.global_prompt.clone() } else { format!("{prompt}, {}", c.global_prompt) };
    }
    let req = SuggestRequest {
        cut: b.cut.clone(),
        frame: p.frame,
        layer,
        rect: b.rect,
        prompt,
        improve: c.improve,
        complete: c.complete,
        steps: (c.steps > 0).then_some(c.steps),
        res: (c.res > 0).then_some(c.res),
        change: Some(c.change),
        strength: None,
        control_end: None,
        cfg: None,
        init: None,
        seed: Some(b.seed),
        context: None,
        tip: env.ed.opts.pen_tip,
        color: env.ed.color.clone(),
    };
    match cp::prepare(&env.core.project, &env.core.blobs, &req) {
        Ok(prep) => send_job(&mut env.ed.copilot, &b, prep),
        Err(e) => {
            env.ed.copilot.last = e.to_string();
            env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
        }
    }
}

fn send_job(c: &mut CopilotUi, b: &CBox, prep: cp::Prepared) {
    if let Some(x) = c.boxes.iter_mut().find(|x| x.id == b.id) {
        x.src = prep.src;
        x.src_region = Some(prep.region);
    }
    let job = Job { box_id: b.id, geo: b.geo, content: b.content, hist: c.hist, prep };
    let tx = worker(c).clone();
    if tx.send(job).is_ok() {
        c.inflight.push((b.id, Instant::now()));
    }
}

/// Um resultado chegou: só vira fantasma se ainda valer (Copiloto ligado, mesmo histórico, a caixa existe, não está
/// desligada e não mudou de lugar). Responde o MCP que esperava esta caixa.
fn receive(env: &mut Env, (id, geo, content, hist, r): Done) {
    let c = &mut env.ed.copilot;
    if let Some(k) = c.inflight.iter().position(|x| x.0 == id) {
        c.inflight.remove(k);
    }
    let bx = c.boxes.iter().find(|b| b.id == id).cloned();
    let stale_reason = if !c.on {
        Some("o Copiloto foi desligado")
    } else if hist != c.hist {
        Some("o desenho voltou atrás (desfazer/refazer) ou as sugestões foram descartadas")
    } else if bx.is_none() {
        Some("a caixa foi apagada")
    } else if bx.as_ref().is_some_and(|b| b.state == BoxState::Off) {
        Some("a caixa foi desligada")
    } else if bx.as_ref().is_some_and(|b| b.geo != geo) {
        Some("a caixa mudou (lugar, tamanho, camada lida) ou o desenho foi aceito durante a geração")
    } else {
        None
    };
    // o MCP que esperava esta caixa (na versão deste pedido) recebe a resposta
    let waiters: Vec<HostRequest> = {
        let mut out = Vec::new();
        let mut keep = Vec::new();
        for (bid, ver, rq) in std::mem::take(&mut c.bridge_wait) {
            if bid == id && ver == content {
                out.push(rq);
            } else {
                keep.push((bid, ver, rq));
            }
        }
        c.bridge_wait = keep;
        out
    };
    match (&r, stale_reason) {
        (Ok(s), None) => {
            c.last = format!("{:.1} s · {} + {}", s.ms as f32 / 1000.0, s.improve.len(), s.complete.len());
            c.last_reduced = s.reduced;
            c.last_throttled = s.throttled;
            c.last_truncated = s.prompt_truncated;
            for rq in waiters {
                let mut v = cp::summary(s);
                v["box"] = serde_json::json!(id);
                rq.respond(Ok(v));
            }
            let b = bx.unwrap();
            c.ghosts.retain(|g| g.box_id != id);
            c.vis_dirty = true;
            if s.improve.is_empty() && s.complete.is_empty() {
                // nada a mostrar: não fica fantasma vazio (o Esc/Tab dele não fariam nada e enganariam); e a caixa só
                // existe enquanto há sugestão: sem nada sugerido ela some, a não ser que tenha prompt próprio (não se
                // perde o que foi digitado). O próximo traço forma outra.
                c.last = format!("{:.1} s · {}", s.ms as f32 / 1000.0, crate::i18n::tr("copilot.nothing"));
                if b.prompt.trim().is_empty() && !b.llm {
                    remove_box(c, id);
                }
                return;
            }
            c.ghosts.push(Ghost { box_id: id, s: s.clone(), stale: b.content != content, vis_improve: s.improve.clone(), vis_complete: s.complete.clone() });
            c.vis_dirty = true;
        }
        (Ok(_), Some(why)) => {
            for rq in waiters {
                rq.respond(Err(aiai_bridge::BridgeError::invalid_params(format!("sugestão descartada: {why}"))));
            }
        }
        (Err(e), _) => {
            c.last = e.clone();
            for rq in waiters {
                rq.respond(Err(aiai_bridge::BridgeError::invalid_params(e.clone())));
            }
            if stale_reason.is_none() {
                env.ed.notify(format!("{}: {e}", crate::i18n::tr("copilot.title")));
            }
        }
    }
}

/// Retângulos (canvas) que têm prioridade sobre a caixa `id` no quadro: caixas menores do mesmo corte (empate: a de id
/// maior), não desligadas e com fantasma neste quadro. Na sobreposição vale o fantasma delas.
fn priority_rects(c: &CopilotUi, id: u32, frame: u32) -> Vec<[f32; 4]> {
    let Some(me) = c.boxes.iter().find(|b| b.id == id) else { return Vec::new() };
    let area = me.rect[2] * me.rect[3];
    c.boxes
        .iter()
        .filter(|b| b.id != id && b.cut == me.cut && b.state != BoxState::Off)
        .filter(|b| {
            let a = b.rect[2] * b.rect[3];
            a < area || (a == area && b.id > id)
        })
        .filter(|b| c.ghosts.iter().any(|g| g.box_id == b.id && g.s.frame == frame))
        .map(|b| b.rect)
        .collect()
}

/// Refaz o que cada fantasma mostra: os traços dele SEM os trechos que caem em caixas de prioridade (o corte é na
/// curva desenhada, não por um ponto), no espaço do cel.
fn refresh_visible(c: &mut CopilotUi) {
    let prios: Vec<Vec<[f32; 4]>> = c.ghosts.iter().map(|g| priority_rects(c, g.box_id, g.s.frame)).collect();
    for (g, pr) in c.ghosts.iter_mut().zip(prios) {
        if pr.is_empty() {
            g.vis_improve = g.s.improve.clone();
            g.vis_complete = g.s.complete.clone();
            continue;
        }
        let s = &g.s;
        let inside = |x: f32, y: f32| {
            let (cx, cy) = s.to_canvas(x, y);
            pr.iter().any(|r| cx >= r[0] && cx <= r[0] + r[2] && cy >= r[1] && cy <= r[1] + r[3])
        };
        let cut = |v: &Vec<Stroke>| v.iter().flat_map(|st| cp::split_stroke(st, &inside).1).collect::<Vec<_>>();
        g.vis_improve = cut(&s.improve);
        g.vis_complete = cut(&s.complete);
    }
}

/// Os quatro cantos da caixa na tela (sup. esq., sup. dir., inf. dir., inf. esq. do canvas): com a vista girada a
/// caixa vira um losango, e é assim que ela é desenhada e pega.
fn corners(b: &CBox, xf: &Xf) -> [Pos2; 4] {
    let r = b.rect;
    [(r[0], r[1]), (r[0] + r[2], r[1]), (r[0] + r[2], r[1] + r[3]), (r[0], r[1] + r[3])].map(|(x, y)| xf.to_screen(Vec2::new(x, y)))
}

fn tag_rect(cs: &[Pos2; 4]) -> Rect {
    Rect::from_min_size(cs[0] - Vec2::new(0.0, 16.0), Vec2::new(54.0, 16.0))
}

/// Caixas e fantasmas por cima do canvas (só visão).
pub(crate) fn paint(env: &mut Env, painter: &egui::Painter, xf: &Xf, cut: &Id) {
    let c = &env.ed.copilot;
    if !c.on && c.ghosts.is_empty() {
        return;
    }
    let frame = env.ed.frame;
    for g in c.ghosts.iter().filter(|g| &g.s.cut == cut && g.s.frame == frame) {
        let a = (c.opacity * if g.stale { 0.45 } else { 1.0 } * 255.0) as u8;
        let gc = c.ghost_color;
        let col = Color32::from_rgba_unmultiplied(gc.r(), gc.g(), gc.b(), a);
        for st in g.vis_improve.iter().chain(&g.vis_complete) {
            ghost_line(painter, xf, &g.s, st, col, c.width_mode, env.ed.opts.pen_width, env.ed.opts.pen_tip);
        }
    }
    if !c.on {
        return;
    }
    for b in c.boxes.iter().filter(|b| &b.cut == cut) {
        let cs = corners(b, xf);
        let col = match b.state {
            BoxState::On => c.box_color,
            BoxState::Paused => c.box_color.gamma_multiply(0.55),
            BoxState::Off => c.box_color.gamma_multiply(0.25),
        };
        let pts = [cs[0], cs[1], cs[2], cs[3], cs[0]];
        if b.auto {
            // foco: tracejada (segue o desenho); fixas: contínuas
            painter.extend(egui::Shape::dashed_line(&pts, egui::Stroke::new(1.0, col), 6.0, 4.0));
        } else {
            painter.add(egui::Shape::line(pts.to_vec(), egui::Stroke::new(1.0, col)));
        }
        // alça de mover (com o número e o estado) e as de redimensionar nos cantos
        let tag = tag_rect(&cs);
        painter.rect_filled(tag, 2.0, col);
        let busy = c.inflight.iter().any(|x| x.0 == b.id);
        let waiting = c.dirty.iter().any(|d| d.id == b.id);
        let mark = if busy { "…" } else if waiting { "·" } else { "" };
        let st = match b.state {
            BoxState::On => "",
            BoxState::Paused => " ⏸",
            BoxState::Off => " ✕",
        };
        let name = if b.auto { crate::i18n::tr("copilot.focus").to_string() } else if b.llm { format!("IA {}", b.id) } else { b.id.to_string() };
        painter.text(tag.left_center() + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{name}{st}{mark}"), egui::FontId::proportional(11.0), Color32::WHITE);
        for k in cs {
            painter.rect_filled(Rect::from_center_size(k, Vec2::splat(6.0)), 0.0, col);
        }
    }
}

/// O fantasma com a largura que o traço terá ao aceitar (o modo de Largura do painel e a caneta ativa) e na escala do
/// movimento da camada.
fn ghost_line(painter: &egui::Painter, xf: &Xf, s: &Suggestion, st: &Stroke, col: Color32, mode: WidthMode, pen_width: f32, tip: aiai_core::VectorTip) {
    // a prévia é o traço que o aceite grava (mesma caneta, largura, perfil e ponta: CP2-10); só a cor e a opacidade
    // são do fantasma
    let mut st = st.clone();
    cp::apply_pen(&mut st, mode, pen_width, tip, &aiai_core::ColorRef::default());
    let smp = aiai_paint::sample_stroke(&st);
    if smp.len() < 2 {
        return;
    }
    let [a, b, _, d, e, _] = s.placement;
    let layer_scale = (a * e - b * d).abs().sqrt();
    let k = if layer_scale.is_finite() && layer_scale > 0.0 { layer_scale } else { 1.0 } * xf.scale;
    let pts: Vec<(Pos2, f32)> = smp
        .iter()
        .map(|q| {
            let (x, y) = s.to_canvas(q.x, q.y);
            (xf.to_screen(Vec2::new(x, y)), q.r * k)
        })
        .collect();
    // faixa de largura variável com 1 px de borda suave (o mesh do egui não tem antisserrilhado próprio)
    let clear = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 0);
    let mut mesh = egui::Mesh::default();
    let n = pts.len();
    for i in 0..n {
        let prev = pts[i.saturating_sub(1)].0;
        let next = pts[(i + 1).min(n - 1)].0;
        let dir = (next - prev).normalized();
        let nrm = if dir.x.is_finite() && dir.length_sq() > 0.0 { egui::vec2(-dir.y, dir.x) } else { egui::vec2(0.0, 1.0) };
        let (p, r) = pts[i];
        // linha mais fina que 1 px na tela: fica 1 px, mais clara na proporção (como o antisserrilhado faria)
        let (r, c) = if r < 0.5 { (0.5, col.gamma_multiply((r * 2.0).clamp(0.15, 1.0))) } else { (r, col) };
        let inner = (r - 0.5).max(0.0);
        let outer = r + 0.5;
        mesh.colored_vertex(p + nrm * outer, clear);
        mesh.colored_vertex(p + nrm * inner, c);
        mesh.colored_vertex(p - nrm * inner, c);
        mesh.colored_vertex(p - nrm * outer, clear);
    }
    for i in 0..(n as u32 - 1) {
        let (a0, b0) = (i * 4, (i + 1) * 4);
        for s in 0..3 {
            mesh.add_triangle(a0 + s, a0 + s + 1, b0 + s);
            mesh.add_triangle(a0 + s + 1, b0 + s + 1, b0 + s);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Clique no canvas: alças das caixas (mover pela etiqueta, redimensionar pelos cantos) e o modo "nova caixa".
/// `true` = consumido.
pub(crate) fn press(env: &mut Env, xf: &Xf, pos: Pos2) -> bool {
    let c = &mut env.ed.copilot;
    if !c.on {
        return false;
    }
    let cv = xf.to_canvas(pos);
    if c.new_box_mode {
        c.drag = Some(Drag::New(cv));
        return true;
    }
    let Some(cut) = env.ed.cut.clone() else { return false };
    let c = &mut env.ed.copilot;
    for b in c.boxes.iter().filter(|b| b.cut == cut) {
        let cs = corners(b, xf);
        if tag_rect(&cs).contains(pos) {
            c.drag = Some(Drag::Move(b.id, cv - Vec2::new(b.rect[0], b.rect[1])));
            return true;
        }
        // cada canto e o oposto dele, no espaço do canvas (vale com a vista girada)
        let r = b.rect;
        let canvas_c = [(r[0], r[1]), (r[0] + r[2], r[1]), (r[0] + r[2], r[1] + r[3]), (r[0], r[1] + r[3])];
        for k in 0..4 {
            if (cs[k] - pos).length() <= 7.0 {
                let o = canvas_c[(k + 2) % 4];
                c.drag = Some(Drag::Resize(b.id, Vec2::new(o.0, o.1)));
                return true;
            }
        }
    }
    false
}

pub(crate) fn drag(env: &mut Env, xf: &Xf, pos: Pos2) -> bool {
    let c = &mut env.ed.copilot;
    let Some(d) = c.drag else { return false };
    let cv = xf.to_canvas(pos);
    match d {
        Drag::New(_) => {}
        Drag::Move(id, off) => {
            if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                b.rect[0] = cv.x - off.x;
                b.rect[1] = cv.y - off.y;
            }
        }
        Drag::Resize(id, fixed) => {
            if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                b.rect = [fixed.x.min(cv.x), fixed.y.min(cv.y), (cv.x - fixed.x).abs().max(16.0), (cv.y - fixed.y).abs().max(16.0)];
            }
        }
    }
    true
}

pub(crate) fn release(env: &mut Env, xf: &Xf, pos: Pos2) -> bool {
    let Some(d) = env.ed.copilot.drag.take() else { return false };
    let cv = xf.to_canvas(pos);
    let frame = env.ed.frame;
    if let Drag::New(a) = d {
        let r = [a.x.min(cv.x), a.y.min(cv.y), (cv.x - a.x).abs(), (cv.y - a.y).abs()];
        if r[2] >= 16.0 && r[3] >= 16.0 {
            if let Some(cut) = env.ed.cut.clone() {
                let layer = env.ed.layer.clone();
                let c = &mut env.ed.copilot;
                // uma caixa por vez: a caixa desenhada à mão troca a atual (o prompt dela vem junto)
                let (prompt, use_global) = c.boxes.first().map(|b| (b.prompt.clone(), b.use_global)).unwrap_or((String::new(), true));
                let old: Vec<u32> = c.boxes.iter().map(|b| b.id).collect();
                for o in old {
                    remove_box(c, o);
                }
                let id = new_box_id(c);
                c.boxes.push(CBox { id, cut, rect: r, prompt, state: BoxState::On, seed: 1, auto: false, layer, use_global, geo: 0, content: 0, llm: false, src: 0, src_region: None });
                requeue(c, id, frame);
                c.vis_dirty = true;
            }
        }
        env.ed.copilot.new_box_mode = false;
    } else if let Drag::Move(id, _) | Drag::Resize(id, _) = d {
        // caixa mudou de lugar: o fantasma e o pedido em voo dela não valem mais, e ela volta para a fila
        let c = &mut env.ed.copilot;
        invalidate_box(c, id);
        requeue(c, id, frame);
    }
    true
}

/// A sugestão como o artista a vê (sem o que caixas menores cobrem), com a caneta ativa aplicada (ponta, cor e a
/// largura escolhida no painel).
fn as_seen(env: &Env, g: &Ghost) -> Suggestion {
    let mut s = g.s.clone();
    s.improve = g.vis_improve.clone();
    s.complete = g.vis_complete.clone();
    let (mode, w, tip, color) = (env.ed.copilot.width_mode, env.ed.opts.pen_width, env.ed.opts.pen_tip, env.ed.color.clone());
    for st in s.improve.iter_mut().chain(s.complete.iter_mut()) {
        cp::apply_pen(st, mode, w, tip, &color);
    }
    s
}

/// O comando de aceitar um fantasma. Rascunho que não é vetorial não recebe traços: vai para a camada limpa.
fn accept_cmd(env: &mut Env, g: &Ghost, which: AcceptWhich, only: Option<&[usize]>, target: cp::AcceptTarget, replace: bool) -> Result<aiai_core::Command, String> {
    let s = as_seen(env, g);
    let vector = aiai_tools::context::layer_data(&env.core.project, &s.layer).is_ok_and(|l| l.kind == aiai_core::LayerType::Vector);
    let target = if target == cp::AcceptTarget::Same && !vector { cp::AcceptTarget::Clean } else { target };
    cp::accept_command_to(&env.core.project, &s, which, only, target, replace, &mut env.core.ids).map_err(|e| e.to_string())
}

/// Tira de um fantasma o que foi aceito (categoria ou índices, na ordem do que se vê): o resto continua à vista e
/// aceitável, já com as assinaturas do documento novo (o próprio aceite não o deixa velho). O fantasma passa a ser só
/// o que se via (o trecho escondido por uma caixa menor não volta). `true` = não sobrou nada.
fn strip_ghost(p: &aiai_core::Project, g: &mut Ghost, which: AcceptWhich, only: Option<&[usize]>) -> bool {
    let mut v = g.s.clone();
    v.improve = std::mem::take(&mut g.vis_improve);
    v.complete = std::mem::take(&mut g.vis_complete);
    let empty = cp::strip_accepted(&mut v, which, only);
    cp::refresh_after_own_accept(p, &mut v);
    g.vis_improve = v.improve.clone();
    g.vis_complete = v.complete.clone();
    g.s = v;
    empty
}

/// O aceite mudou o documento: os pedidos em voo deste corte foram calculados antes dele e não valem mais (chegariam
/// como atuais sobre um desenho que já não existe, CP2-01). Caem, e as caixas deles voltam para a fila.
fn drop_inflight_after_accept(c: &mut CopilotUi, cut: &Id, frame: u32) {
    let ids: Vec<u32> = c.inflight.iter().map(|x| x.0).collect();
    for id in ids {
        if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id && &b.cut == cut) {
            b.geo += 1;
            requeue(c, id, frame);
        }
    }
}

/// Aceita os fantasmas do quadro (todos, ou só Melhorar/Completar) na camada de cada sugestão. Várias caixas de uma
/// vez = um passo só de desfazer. O que não foi aceito continua à vista. Sugestão velha (o desenho em volta da caixa,
/// o movimento da camada ou a paleta mudou desde o pedido) não entra, a não ser com `force` (Shift+Tab): todas são
/// conferidas antes de mexer no documento.
pub(crate) fn accept(env: &mut Env, which: AcceptWhich) -> usize {
    accept_with(env, which, false)
}

pub(crate) fn accept_with(env: &mut Env, which: AcceptWhich, force: bool) -> usize {
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    let (target, replace) = (env.ed.copilot.target, env.ed.copilot.replace_rough);
    let ghosts: Vec<Ghost> = std::mem::take(&mut env.ed.copilot.ghosts);
    let mut keep = Vec::new();
    let mut n = 0;
    // a guarda, antes de qualquer mudança: o que está velho fica de fora (e à vista, esmaecido, até chegar o novo)
    let mut stale = 0;
    let mut todo = Vec::new();
    for g in ghosts {
        let here = Some(&g.s.cut) == cut.as_ref() && g.s.frame == frame;
        let empty = match which {
            AcceptWhich::All => g.vis_improve.is_empty() && g.vis_complete.is_empty(),
            AcceptWhich::Improve => g.vis_improve.is_empty(),
            AcceptWhich::Complete => g.vis_complete.is_empty(),
        };
        if !here || empty {
            keep.push(g);
        } else if !force && cp::check_fresh(&env.core.project, &g.s).is_err() {
            stale += 1;
            keep.push(g);
        } else {
            todo.push(g);
        }
    }
    if stale > 0 {
        env.ed.notify(format!("{} ({stale})", crate::i18n::tr("copilot.stale_skipped")));
    }
    let mut done_boxes = Vec::new();
    for mut g in todo {
        match accept_cmd(env, &g, which, None, target, replace) {
            Ok(cmd) => match super::commands::run(env.core, cmd) {
                Ok(_) => {
                    n += 1;
                    if !strip_ghost(&env.core.project, &mut g, which, None) {
                        keep.push(g);
                    } else {
                        done_boxes.push(g.box_id);
                    }
                }
                Err(e) => {
                    env.ed.notify(e);
                    keep.push(g);
                }
            },
            Err(e) => {
                env.ed.notify(e);
                keep.push(g);
            }
        }
    }
    env.ed.copilot.ghosts = keep;
    env.ed.copilot.vis_dirty = true;
    // várias caixas aceitas de uma vez (Tab) = um passo só de desfazer: o Ctrl+Z volta tudo, inclusive a camada limpa
    // criada pela primeira delas
    if n >= 2 {
        env.core.merge_last(n, &format!("Copiloto: aceitar {n} caixas"));
    }
    // o próprio aceitar mudou o documento: não é motivo para gerar de novo; mas o que estava em voo foi calculado antes
    env.ed.copilot.seen_change = env.core.change_counter;
    env.ed.copilot.seen_moves = env.core.history_moves;
    if n > 0 {
        if let Some(cut) = cut.as_ref() {
            drop_inflight_after_accept(&mut env.ed.copilot, cut, frame);
        }
    }
    // tudo aceito: a caixa some (os traços já são traços comuns da camada)
    for id in done_boxes {
        remove_box(&mut env.ed.copilot, id);
    }
    n
}

/// Descarta os fantasmas do quadro (e o que estiver em voo deixa de valer).
pub(crate) fn discard(env: &mut Env) -> usize {
    let frame = env.ed.frame;
    let cut = env.ed.cut.clone();
    let c = &mut env.ed.copilot;
    let before = c.ghosts.len();
    c.ghosts.retain(|g| Some(&g.s.cut) != cut.as_ref() || g.s.frame != frame);
    let ids: Vec<u32> = c.boxes.iter().filter(|b| Some(&b.cut) == cut.as_ref()).map(|b| b.id).collect();
    c.dirty.retain(|d| !ids.contains(&d.id));
    invalidate_all(c);
    // descartada: a caixa some; o próximo traço forma outra
    for id in ids {
        remove_box(c, id);
    }
    before - c.ghosts.len()
}

/// Há fantasma à vista neste quadro?
fn ghosts_here(env: &Env) -> bool {
    let (frame, cut) = (env.ed.frame, env.ed.cut.clone());
    env.ed.copilot.ghosts.iter().any(|g| Some(&g.s.cut) == cut.as_ref() && g.s.frame == frame && !(g.vis_improve.is_empty() && g.vis_complete.is_empty()))
}

/// Esc: com fantasma à vista e nenhum traço em andamento, descarta. Chamado pelos atalhos do editor ANTES do Esc geral
/// (que cancela ferramentas); campo de texto em uso fica com a tecla (o chamador já confere). `true` = consumiu.
pub(crate) fn escape(env: &mut Env, ctx: &egui::Context) -> bool {
    if !ghosts_here(env) || matches!(env.ed.gesture, super::editor::Gesture::Stroke(_)) {
        return false;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        discard(env);
        return true;
    }
    false
}

/// Tab aceita tudo (Esc também descarta aqui, se ainda chegar). `true` = consumiu a tecla.
pub(crate) fn keys(env: &mut Env, ui: &egui::Ui) -> bool {
    // havia um campo de texto com o teclado no quadro ANTERIOR? (no quadro do Tab o egui já passou o foco para o
    // próximo campo, inclusive nos painéis desenhados antes do canvas, então o estado de agora não serve)
    let typing = std::mem::replace(&mut env.ed.copilot.typing, ui.ctx().egui_wants_keyboard_input());
    if !ghosts_here(env) || typing {
        return false;
    }
    // Shift+Tab primeiro: o `consume_key` sem modificador também casa com Shift (e engoliria o Shift+Tab como Tab)
    let (force, tab, esc) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab), i.consume_key(egui::Modifiers::NONE, egui::Key::Tab), i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)));
    let tab = tab || force;
    if tab {
        // o egui também anda o foco com Tab: cancela, e no próximo quadro ninguém fica com o foco (o Ctrl+Z seguinte
        // tem de desfazer o aceitar, não o texto de um campo do painel)
        ui.ctx().memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        env.ed.copilot.drop_focus = true;
        let n = accept_with(env, AcceptWhich::All, force);
        if n > 0 {
            env.ed.notify(format!("{} ({n})", crate::i18n::tr("copilot.accepted")));
        }
    } else if esc {
        discard(env);
    }
    tab || esc
}

/// `COPILOT` pela ponte com o app aberto: o pedido do agente entra na MESMA fila e thread do painel (a interface não
/// congela), aparece no canvas numa caixa "IA" e a resposta sai quando o resultado chega. Aceitar e descartar usam o
/// mesmo fantasma que o artista vê (o mesmo id); aceitar recusa sugestão velha (o desenho mudou), salvo `force`.
pub(crate) fn bridge(env: &mut Env, r: HostRequest) {
    use aiai_bridge::BridgeError;
    let req: cp::CopilotRequest = match serde_json::from_value(r.params.clone()) {
        Ok(v) => v,
        Err(e) => return r.respond(Err(BridgeError::invalid_params(format!("COPILOT: {e}")))),
    };
    match req.op.as_str() {
        "suggest" => {
            let Some(s) = req.suggest else { return r.respond(Err(BridgeError::invalid_params("suggest pede `suggest`"))) };
            if !env.ed.copilot.on {
                set_on(env, true);
                if !env.ed.copilot.on {
                    return r.respond(Err(BridgeError::invalid_params(format!("o Copiloto não ligou: {}", cp::service_error()))));
                }
            }
            let prep = match cp::prepare(&env.core.project, &env.core.blobs, &s) {
                Ok(p) => p,
                Err(e) => return r.respond(Err(BridgeError::invalid_params(e.to_string()))),
            };
            let c = &mut env.ed.copilot;
            // uma caixa por vez: o pedido do agente usa a caixa que existe (vira caixa "IA", no lugar e no tamanho
            // pedidos) ou cria a única
            let id = match (!c.boxes.is_empty()).then_some(0) {
                Some(k) => {
                    // pedido novo na mesma caixa: o fantasma e o pedido em voo anteriores (de outra camada, quadro ou
                    // prompt) não valem mais (CP2-07)
                    let id = c.boxes[k].id;
                    invalidate_box(c, id);
                    let b = &mut c.boxes[k];
                    b.prompt = s.prompt.clone();
                    b.layer = Some(s.layer.clone());
                    b.cut = s.cut.clone();
                    b.rect = s.rect;
                    b.llm = true;
                    b.auto = false;
                    b.use_global = false;
                    b.state = BoxState::On;
                    b.content += 1;
                    b.id
                }
                None => {
                    let id = new_box_id(c);
                    c.boxes.push(CBox { id, cut: s.cut.clone(), rect: s.rect, prompt: s.prompt.clone(), state: BoxState::On, seed: s.seed.unwrap_or(1), auto: false, layer: Some(s.layer.clone()), use_global: false, geo: 0, content: 1, llm: true, src: 0, src_region: None });
                    id
                }
            };
            c.dirty.retain(|d| d.id != id);
            let b = c.boxes.iter().find(|b| b.id == id).cloned().unwrap();
            c.bridge_wait.push((id, b.content, r));
            send_job(c, &b, prep);
            c.vis_dirty = true;
        }
        "accept" => {
            let Some(sid) = req.id else { return r.respond(Err(BridgeError::invalid_params("accept pede `id`"))) };
            let Some(k) = env.ed.copilot.ghosts.iter().position(|g| g.s.id == sid) else {
                return r.respond(Err(BridgeError::invalid_params(format!("sugestão {sid} não está à vista (já aceita, descartada ou velha)"))));
            };
            if !req.force {
                if let Err(e) = cp::check_fresh(&env.core.project, &env.ed.copilot.ghosts[k].s) {
                    return r.respond(Err(BridgeError::invalid_params(e.to_string())));
                }
            }
            let mut g = env.ed.copilot.ghosts.remove(k);
            let res = accept_cmd(env, &g, req.which, req.only.as_deref(), req.target, req.replace).and_then(|cmd| super::commands::run(env.core, cmd).map_err(|e| e.to_string()));
            match res {
                Ok(_) => {
                    // só sai o que foi aceito (categoria ou índices); o resto fica, com as assinaturas novas (CP2-05)
                    let all_gone = strip_ghost(&env.core.project, &mut g, req.which, req.only.as_deref());
                    // o corte/quadro da sugestão aceita, não o que o editor mostra agora (CP3-08)
                    let (cut, frame) = (g.s.cut.clone(), g.s.frame);
                    let left = serde_json::json!({"improve": g.vis_improve.len(), "complete": g.vis_complete.len()});
                    if !all_gone {
                        env.ed.copilot.ghosts.push(g);
                    } else {
                        // tudo aceito: a caixa some, como no Tab
                        remove_box(&mut env.ed.copilot, g.box_id);
                    }
                    env.ed.copilot.vis_dirty = true;
                    env.ed.copilot.seen_change = env.core.change_counter;
                    drop_inflight_after_accept(&mut env.ed.copilot, &cut, frame);
                    r.respond(Ok(serde_json::json!({"accepted": sid, "revision": env.core.project.revision, "remaining": left})));
                }
                Err(e) => {
                    env.ed.copilot.ghosts.push(g);
                    r.respond(Err(BridgeError::invalid_params(e)));
                }
            }
        }
        "discard" => {
            let Some(sid) = req.id else { return r.respond(Err(BridgeError::invalid_params("discard pede `id`"))) };
            let before = env.ed.copilot.ghosts.len();
            let boxes: Vec<u32> = env.ed.copilot.ghosts.iter().filter(|g| g.s.id == sid).map(|g| g.box_id).collect();
            env.ed.copilot.ghosts.retain(|g| g.s.id != sid);
            // descartada: a caixa some, como no Esc
            for b in boxes {
                remove_box(&mut env.ed.copilot, b);
            }
            env.ed.copilot.vis_dirty = true;
            r.respond(Ok(serde_json::json!({"discarded": before != env.ed.copilot.ghosts.len()})));
        }
        "status" => r.respond(Ok(state_json(&env.ed.copilot))),
        other => r.respond(Err(BridgeError::invalid_params(format!("COPILOT: op desconhecida '{other}' (suggest, accept, discard, status)")))),
    }
}

/// Corpo do painel Copiloto (no dock, coluna própria à direita; nunca por cima do desenho): estado, o que sugerir e quanto
/// mudar, prompt geral, caixas (cada uma com estado, tamanho de geração e prompt), aceitar e, fechados, os ajustes finos.
pub(crate) fn panel(env: &mut Env, ui: &mut egui::Ui) {
    use crate::app::ui_kit::section;
    use crate::icons::IconId;
    use crate::i18n::tr;
    let state = cp::service_state();
    let cur_cut = env.ed.cut.clone();
    let frame = env.ed.frame;
    let t = env.t;
    // onde cada caixa vai gerar (janela de contexto e tamanho de geração), para mostrar antes de gerar
    let res = env.ed.copilot.res;
    // o foco primeiro, depois as fixas
    let mut plans: Vec<(u32, bool, [f32; 4], Option<([u32; 4], [u32; 2])>)> = env
        .ed
        .copilot
        .boxes
        .iter()
        .filter(|b| Some(&b.cut) == cur_cut.as_ref())
        .map(|b| (b.id, b.auto, b.rect, cp::plan(&env.core.project, &b.cut, b.rect, None, (res > 0).then_some(res)).ok()))
        .collect();
    plans.sort_by_key(|p| !p.1);
    if !env.ed.copilot.on {
        // desligado: o painel explica e liga
        ui.add_space(6.0);
        ui.label(RichText::new(tr("copilot.off")).size(12.0).color(t.text_dim));
        ui.add_space(4.0);
        if ui.button(RichText::new(tr("copilot.turn_on")).strong()).clicked() {
            set_on(env, true);
        }
        return;
    }

    ui.spacing_mut().item_spacing.y = 5.0;
    let mut act: Option<Option<AcceptWhich>> = None;
    let mut again = false;

    // --- estado: serviço, fila e a última geração numa linha só
    {
        let c = &env.ed.copilot;
        let (dot, svc) = match state {
            "pronto" if !c.inflight.is_empty() => (t.st_progress, tr("copilot.svc.busy").to_string()),
            "pronto" => (t.st_ok, tr("copilot.svc.ready").to_string()),
            "carregando" => (t.st_progress, tr("copilot.svc.loading").to_string()),
            "gerando" => (t.st_progress, tr("copilot.svc.busy").to_string()),
            "falhou" => (t.danger, tr("copilot.svc.failed").to_string()),
            _ => (t.text_dim, tr("copilot.svc.stopped").to_string()),
        };
        let busy = c.inflight.first().map(|(id, at)| format!(" · {} {id} · {:.0} s", tr("copilot.generating"), at.elapsed().as_secs_f32())).unwrap_or_default();
        // um texto só, que quebra a linha no painel estreito (antes a fila ficava à direita, por cima do estado)
        let queue = format!(" · {} {}", tr("copilot.queue"), c.dirty.len());
        ui.horizontal_wrapped(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            ui.label(RichText::new(format!("{svc}{busy}")).size(12.0).color(t.text));
            ui.label(RichText::new(queue).size(11.0).color(t.text_dim));
        });
        if !c.last.is_empty() {
            ui.label(RichText::new(format!("{}: {}", tr("copilot.last"), c.last)).size(11.0).color(t.text_dim));
        }
        // avisos da última sugestão (texto em cor de alerta legível nos dois temas)
        // a potência lida ao vivo (a cada 30 s) vale mais que a da última sugestão: avisa já durante a espera (CP2-12)
        let throttled = cp::gpu_power_limited().unwrap_or(c.last_throttled);
        for (on, key) in [(c.last_reduced, "copilot.warn.reduced"), (throttled, "copilot.warn.throttled"), (c.last_truncated, "copilot.warn.truncated")] {
            if on {
                ui.label(RichText::new(tr(key)).size(11.0).color(t.danger));
            }
        }
        if !(c.improve || c.complete) {
            ui.label(RichText::new(tr("copilot.warn.nothing")).size(11.0).color(t.danger));
        }
    }
    if state == "falhou" {
        ui.label(RichText::new(cp::service_error()).size(11.0).color(t.danger));
        ui.horizontal(|ui| {
            if ui.button(tr("copilot.restart")).on_hover_text(tr("copilot.restart.tip")).clicked() {
                restart(env);
            }
            if ui.small_button(tr("copilot.open_log")).on_hover_text(cp::service_log_path().display().to_string()).clicked() {
                let _ = std::process::Command::new("notepad").arg(cp::service_log_path()).spawn();
            }
        });
    }

    // --- o que sugerir e quanto mudar
    section(ui, &t, "copilot.sec.suggest");
    {
        let c = &mut env.ed.copilot;
        ui.horizontal(|ui| {
            again |= ui.toggle_value(&mut c.improve, tr("copilot.improve")).on_hover_text(tr("copilot.improve.tip")).changed();
            again |= ui.toggle_value(&mut c.complete, tr("copilot.complete")).on_hover_text(tr("copilot.complete.tip")).changed();
        });
    }
    let mut notes: Vec<(String, Rect)> = Vec::new();
    {
        let c = &mut env.ed.copilot;
        ui.label(RichText::new(tr("copilot.change")).size(12.0).color(t.text_dim)).on_hover_text(tr("copilot.change.tip"));
        let mut v = c.change * 100.0;
        let w = (ui.available_width() - 64.0).max(80.0);
        ui.spacing_mut().slider_width = w;
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=100.0).max_decimals(0)).on_hover_text(tr("copilot.change.tip"));
        // o trilho (sem a caixa do número), para quem clica numa posição
        notes.push(("copilot:change".into(), Rect::from_min_size(r.rect.min, Vec2::new(w, r.rect.height()))));
        if r.changed() {
            c.change = v / 100.0;
            again = true;
        }
    }

    // --- perfil de velocidade (atalho para resolução e passos; mexer neles à mão vira "personalizado")
    {
        let c = &mut env.ed.copilot;
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("copilot.profile")).size(12.0).color(t.text_dim)).on_hover_text(tr("copilot.profile.tip"));
            // na GPU de 4 GB, 640 px com cfg > 1 não cabe e cai para 512: o Qualidade ganha com passos, não com tamanho
            const PROFILES: [(&str, u32, u32); 3] = [("copilot.profile.fast", 384, 0), ("copilot.profile.balanced", 0, 0), ("copilot.profile.quality", 512, 8)];
            for (key, res, steps) in PROFILES {
                let on = c.res == res && c.steps == steps;
                let r = ui.selectable_label(on, RichText::new(tr(key)).size(12.0)).on_hover_text(tr(&format!("{key}.tip")));
                notes.push((format!("copilot:profile:{}", key.rsplit('.').next().unwrap_or(key)), r.rect));
                if r.clicked() && !on {
                    c.res = res;
                    c.steps = steps;
                    again = true;
                }
            }
            if !PROFILES.iter().any(|p| p.1 == c.res && p.2 == c.steps) {
                ui.label(RichText::new(tr("copilot.profile.custom")).size(11.0).color(t.text_dim));
            }
        });
    }

    // --- prompt geral: largo, duas linhas; vale ao sair do campo
    section(ui, &t, "copilot.sec.prompt");
    {
        let c = &mut env.ed.copilot;
        let r = ui.add(egui::TextEdit::multiline(&mut c.global_prompt).desired_rows(2).desired_width(f32::INFINITY).hint_text(tr("copilot.global.hint")));
        notes.push(("copilot:global".into(), r.rect));
        again |= prompt_done(ui, &r, &mut c.global_prompt) && c.global_on;
        again |= ui.checkbox(&mut c.global_on, RichText::new(tr("copilot.global.all")).size(12.0)).changed();
    }

    // --- caixas
    ui.horizontal(|ui| {
        section_inline(ui, &t, tr("copilot.sec.boxes"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let nb = env.ed.copilot.new_box_mode;
            if env.icon_button_sized(ui, IconId::Plus, "copilot.new_box.tip", nb, true, "copilot:new_box", 22.0, false).clicked() {
                env.ed.copilot.new_box_mode = !nb;
            }
        });
    });
    if plans.is_empty() {
        ui.label(RichText::new(tr("copilot.boxes.empty")).size(11.5).color(t.text_dim));
    }
    // as caixas e os prompts vivem só nesta sessão (protótipo): dito no painel, não escondido (CP1-17)
    ui.label(RichText::new(tr("copilot.boxes.temp")).size(10.5).color(t.text_dim));
    let mut remove = None;
    let mut redo_box = Vec::new();
    for (id, auto, rect_b, plan) in &plans {
        let (id, auto) = (*id, *auto);
        egui::Frame::new().stroke(egui::Stroke::new(1.0, t.border)).corner_radius(4.0).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let badge = if auto { tr("copilot.focus").to_string() } else { id.to_string() };
                let (r, resp) = ui.allocate_exact_size(Vec2::new(if auto { 40.0 } else { 22.0 }, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 3.0, env.ed.copilot.box_color);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, badge, egui::FontId::proportional(11.5), Color32::WHITE);
                if auto {
                    resp.on_hover_text(tr("copilot.focus.tip"));
                }
                let st = env.ed.copilot.boxes.iter().find(|b| b.id == id).map(|b| b.state).unwrap_or(BoxState::Off);
                for (s, icon, key, name) in [
                    (BoxState::On, IconId::Play, "copilot.box.state.on", "on"),
                    (BoxState::Paused, IconId::Pause, "copilot.box.state.paused", "paused"),
                    (BoxState::Off, IconId::EyeOff, "copilot.box.state.off", "off"),
                ] {
                    if env.icon_button_sized(ui, icon, key, st == s, true, &format!("copilot:box:{id}:{name}"), 22.0, false).clicked() && st != s {
                        let c = &mut env.ed.copilot;
                        if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                            b.state = s;
                        }
                        match s {
                            BoxState::On => redo_box.push(id),
                            // desligada: sem fantasma e fora da fila
                            BoxState::Off => {
                                invalidate_box(c, id);
                                c.dirty.retain(|d| d.id != id);
                                fail_waiters(c, Some(id), "a caixa foi desligada");
                            }
                            BoxState::Paused => c.dirty.retain(|d| d.id != id),
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if env.icon_button_sized(ui, IconId::Trash, "copilot.box.delete", false, true, &format!("copilot:box:{id}:delete"), 22.0, false).clicked() {
                        remove = Some(id);
                    }
                    if env.icon_button_sized(ui, IconId::Refresh, "copilot.box.again", false, true, &format!("copilot:box:{id}:again"), 22.0, false).clicked() {
                        let c = &mut env.ed.copilot;
                        if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                            b.seed = b.seed.wrapping_add(1);
                            b.state = BoxState::On;
                        }
                        redo_box.push(id);
                    }
                });
            });
            // tamanho da caixa → tamanho da geração, e o aviso quando a escala atrapalha
            if let Some((win, gsz)) = plan {
                let z = cp::gen_zoom(*win, *gsz);
                let txt = format!("{:.0} × {:.0} px  →  {} {} × {}  ({:.1}×)", rect_b[2], rect_b[3], tr("copilot.box.gen"), gsz[0], gsz[1], z);
                // fora da faixa boa, o texto fica na cor de alerta (legível nos dois temas) e diz o que acontece
                let (col, tip) = if z < 0.6 {
                    (t.danger, Some("copilot.zoom.down"))
                } else if z > 1.6 {
                    (t.text, Some("copilot.zoom.up"))
                } else {
                    (t.text_dim, None)
                };
                let r = ui.label(RichText::new(txt).size(11.0).color(col));
                r.on_hover_text(format!("{}{}", tr("copilot.box.gen.tip"), tip.map(|k| format!("\n\n{}", tr(k))).unwrap_or_default()));
            }
            // de onde a caixa lê o desenho, e trocar para a camada ativa
            let src = env.ed.copilot.boxes.iter().find(|b| b.id == id).and_then(|b| b.layer.clone());
            let active = env.ed.layer.clone();
            let src_name = src.as_ref().and_then(|l| env.core.project.node(l)).map(|n| n.name.clone()).unwrap_or_else(|| tr("copilot.src.none").to_string());
            let src_info = src.as_ref().and_then(|l| aiai_tools::context::layer_data(&env.core.project, l).ok()).map(|l| (l.kind, l.visible, l.locked));
            // nome longo (CP3-05): a linha quebra dentro da largura do painel e o nome é cortado com "…" (o inteiro na dica)
            ui.horizontal_wrapped(|ui| {
                let full = format!("{} {src_name}", tr("copilot.src"));
                let short: String = if full.chars().count() > 34 { format!("{}…", full.chars().take(33).collect::<String>()) } else { full.clone() };
                ui.add(egui::Label::new(RichText::new(short).size(11.0).color(t.text_dim))).on_hover_text(format!("{full}

{}", tr("copilot.src.tip")));
                if let Some((kind, visible, locked)) = src_info {
                    if kind != aiai_core::LayerType::Vector {
                        ui.label(RichText::new(tr("copilot.src.raster")).size(11.0).color(t.text_dim)).on_hover_text(tr("copilot.src.raster.tip"));
                    }
                    if !visible || locked {
                        ui.label(RichText::new(tr(if locked { "copilot.src.locked" } else { "copilot.src.hidden" })).size(11.0).color(t.danger));
                    }
                }
                if active.is_some() && active != src && ui.small_button(tr("copilot.src.use_active")).on_hover_text(tr("copilot.src.use_active.tip")).clicked() {
                    // outra fonte: o fantasma e o pedido em voo da camada anterior não valem mais (CP2-07)
                    if let Some(b) = env.ed.copilot.boxes.iter_mut().find(|b| b.id == id) {
                        b.layer = active.clone();
                    }
                    invalidate_box(&mut env.ed.copilot, id);
                    redo_box.push(id);
                }
            });
            let c = &mut env.ed.copilot;
            if let Some(b) = c.boxes.iter_mut().find(|b| b.id == id) {
                let r = ui.add(egui::TextEdit::multiline(&mut b.prompt).desired_rows(2).desired_width(f32::INFINITY).hint_text(tr("copilot.box.hint")));
                notes.push((format!("copilot:box:{id}:prompt"), r.rect));
                if prompt_done(ui, &r, &mut b.prompt) {
                    redo_box.push(id);
                }
                if ui.checkbox(&mut b.use_global, RichText::new(tr("copilot.box.use_global")).size(11.5)).on_hover_text(tr("copilot.box.use_global.tip")).changed() {
                    redo_box.push(id);
                }
            }
        });
    }
    {
        let c = &mut env.ed.copilot;
        if let Some(id) = remove {
            // apagar = descartar: o fantasma e a caixa somem
            c.ghosts.retain(|g| g.box_id != id);
            remove_box(c, id);
        }
        for id in redo_box {
            requeue(c, id, frame);
        }
    }

    // --- aceitar
    section(ui, &t, "copilot.sec.accept");
    let has = ghosts_here(env);
    ui.horizontal(|ui| {
        for (icon, key, name, w) in [
            (IconId::Check, "copilot.accept_all", "copilot:accept:all", Some(AcceptWhich::All)),
            (IconId::ToolWand, "copilot.accept_improve", "copilot:accept:improve", Some(AcceptWhich::Improve)),
            (IconId::Plus, "copilot.accept_complete", "copilot:accept:complete", Some(AcceptWhich::Complete)),
            (IconId::Close, "copilot.discard", "copilot:discard", None),
        ] {
            if env.icon_button_sized(ui, icon, key, false, has, name, 26.0, false).clicked() {
                act = Some(w);
            }
        }
        ui.add_space(6.0);
        let c = &mut env.ed.copilot;
        let label = |x: cp::AcceptTarget| tr(if x == cp::AcceptTarget::Clean { "copilot.target.clean" } else { "copilot.target.same" }).to_string();
        let tr_rect = egui::ComboBox::from_id_salt("copilot_target").width(ui.available_width().max(90.0)).selected_text(label(c.target)).show_ui(ui, |ui| {
            for x in [cp::AcceptTarget::Same, cp::AcceptTarget::Clean] {
                let r = ui.selectable_value(&mut c.target, x, label(x));
                notes.push((format!("copilot:target:{}", if x == cp::AcceptTarget::Clean { "clean" } else { "same" }), r.rect));
            }
        })
        .response
        .on_hover_text(tr("copilot.target.tip"))
        .rect;
        notes.push(("copilot:target".into(), tr_rect));
    });
    {
        let c = &mut env.ed.copilot;
        ui.add_enabled_ui(c.target == cp::AcceptTarget::Same, |ui| {
            ui.checkbox(&mut c.replace_rough, RichText::new(tr("copilot.replace")).size(12.0)).on_hover_text(tr("copilot.replace.tip"));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("copilot.width")).size(12.0).color(t.text_dim)).on_hover_text(tr("copilot.width.tip"));
            let label = |m: WidthMode| tr(match m {
                WidthMode::Drawing => "copilot.width.drawing",
                WidthMode::Pen => "copilot.width.pen",
                WidthMode::Fixed => "copilot.width.fixed",
            })
            .to_string();
            egui::ComboBox::from_id_salt("copilot_width").width(140.0).selected_text(label(c.width_mode)).show_ui(ui, |ui| {
                for m in [WidthMode::Drawing, WidthMode::Pen, WidthMode::Fixed] {
                    ui.selectable_value(&mut c.width_mode, m, label(m));
                }
            });
        });
    }

    // --- ajustes finos (fechados por padrão)
    egui::CollapsingHeader::new(RichText::new(tr("copilot.sec.adjust")).size(12.0).color(t.text_dim)).id_salt("copilot_adjust").default_open(false).show(ui, |ui| {
        let c = &mut env.ed.copilot;
        egui::Grid::new("copilot_adjust_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new(tr("copilot.steps")).size(12.0)).on_hover_text(tr("copilot.steps.tip"));
            let auto = tr("copilot.steps.auto").to_string();
            again |= ui.add(egui::Slider::new(&mut c.steps, 0..=12).custom_formatter(move |n, _| if n < 0.5 { auto.clone() } else { format!("{n:.0}") })).changed();
            ui.end_row();
            ui.label(RichText::new(tr("copilot.res")).size(12.0)).on_hover_text(tr("copilot.res.tip"));
            let label = |r: u32| if r == 0 { tr("copilot.res.auto").to_string() } else { format!("{r} px") };
            egui::ComboBox::from_id_salt("copilot_res").width(120.0).selected_text(label(c.res)).show_ui(ui, |ui| {
                for r in [0, 384, 512, 640, 768] {
                    again |= ui.selectable_value(&mut c.res, r, label(r)).changed();
                }
            });
            ui.end_row();
            ui.label(RichText::new(tr("copilot.opacity")).size(12.0));
            let mut v = c.opacity * 100.0;
            if ui.add(egui::Slider::new(&mut v, 10.0..=60.0).suffix(" %").max_decimals(0)).changed() {
                c.opacity = v / 100.0;
            }
            ui.end_row();
            ui.label(RichText::new(tr("copilot.magnet")).size(12.0)).on_hover_text(tr("copilot.magnet.tip"));
            let mut v = c.magnet * 100.0;
            if ui.add(egui::Slider::new(&mut v, 0.0..=100.0).suffix(" %").max_decimals(0)).changed() {
                c.magnet = v / 100.0;
            }
            ui.end_row();
            ui.label(RichText::new(tr("copilot.pause")).size(12.0)).on_hover_text(tr("copilot.pause.tip"));
            ui.add(egui::Slider::new(&mut c.pause_s, 0.2..=2.0).suffix(" s"));
            ui.end_row();
            ui.label(RichText::new(tr("copilot.colors")).size(12.0)).on_hover_text(tr("copilot.colors.tip"));
            ui.horizontal(|ui| {
                ui.color_edit_button_srgba(&mut c.ghost_color).on_hover_text(tr("copilot.color.ghost"));
                ui.color_edit_button_srgba(&mut c.box_color).on_hover_text(tr("copilot.color.box"));
                if ui.small_button(tr("copilot.colors.reset")).clicked() {
                    c.ghost_color = GHOST_COLOR;
                    c.box_color = BOX_COLOR;
                }
            });
            ui.end_row();
        });
    });
    if again {
        requeue_cut(&mut env.ed.copilot, cur_cut.as_ref(), frame);
    }
    for (k, r) in notes {
        env.note(&k, r);
    }
    match act {
        Some(Some(w)) => {
            accept(env, w);
        }
        Some(None) => {
            discard(env);
        }
        None => {}
    }
}

/// Campo de prompt: quebra de linha não tem sentido num prompt (o texto quebra sozinho na tela), então Enter aplica
/// (sai do campo) e qualquer quebra vira espaço. `true` = terminou de editar (Enter ou clicou fora).
fn prompt_done(ui: &egui::Ui, r: &egui::Response, text: &mut String) -> bool {
    // o campo de várias linhas consome o Enter (vira uma quebra): a quebra é o sinal de Enter
    let mut enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if text.contains(['\r', '\n']) {
        *text = text.replace(['\r', '\n'], " ").trim().to_string();
        enter = true;
    }
    if enter {
        r.surrender_focus();
    }
    enter || r.lost_focus()
}

/// Título de seção sem a linha (para dividir a linha com botões).
fn section_inline(ui: &mut egui::Ui, t: &crate::theme::Tokens, text: &str) {
    ui.label(RichText::new(text).size(11.5).color(t.text_dim).strong());
}

// ------------------------------------------------------------------------------------------------ ímã

/// Uma linha fantasma visível que o ímã conhece, com os pontos densos no canvas.
struct MLine {
    pts: Vec<Vec2>,
}

/// Ímã do traço em andamento: puxa a caneta para o fantasma VISÍVEL mais perto e guarda o caminho puxado.
pub(crate) struct Magnet {
    lines: Vec<MLine>,
    radius: f32,
    strength: f32,
    pulled: usize,
    total: usize,
    /// Pontos (canvas) que foram puxados: o trecho do fantasma perto deles sai no fim do traço.
    path: Vec<Vec2>,
}

/// Ímã para um traço de caneta que começa agora (só com fantasma à vista e força > 0). `scale` = px de tela por px do
/// canvas: o raio de 12 px de tela vira canvas. Usa só o que aparece (o mesmo que a pintura e o aceitar).
pub(crate) fn magnet_for(env: &Env, scale: f32) -> Option<Magnet> {
    let c = &env.ed.copilot;
    if !c.on || c.magnet <= 0.0 {
        return None;
    }
    let (cut, frame) = (env.ed.cut.clone()?, env.ed.frame);
    let mut lines = Vec::new();
    for g in c.ghosts.iter().filter(|g| g.s.cut == cut && g.s.frame == frame) {
        for st in g.vis_improve.iter().chain(&g.vis_complete) {
            let smp = aiai_paint::sample_stroke(st);
            if smp.len() >= 2 {
                lines.push(MLine {
                    pts: smp
                        .iter()
                        .map(|q| {
                            let (x, y) = g.s.to_canvas(q.x, q.y);
                            Vec2::new(x, y)
                        })
                        .collect(),
                });
            }
        }
    }
    (!lines.is_empty()).then(|| Magnet { lines, radius: 12.0 / scale.max(1e-3), strength: c.magnet, pulled: 0, total: 0, path: Vec::new() })
}

impl Magnet {
    /// O ponto (canvas) puxado para a linha fantasma mais perto, com força que cai com a distância.
    pub(crate) fn pull(&mut self, p: Vec2) -> Vec2 {
        self.total += 1;
        let mut best: Option<(f32, Vec2)> = None;
        for l in &self.lines {
            for k in 0..l.pts.len() - 1 {
                let (a, b) = (l.pts[k], l.pts[k + 1]);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let q = a + ab * t;
                let d = (p - q).length();
                if d <= self.radius && best.is_none_or(|x| d < x.0) {
                    best = Some((d, q));
                }
            }
        }
        let Some((d, q)) = best else { return p };
        self.pulled += 1;
        let u = d / self.radius;
        let w = self.strength * (1.0 - u * u).powi(2);
        let out = p + (q - p) * w;
        self.path.push(out);
        out
    }
}

/// Fim do traço com ímã: o trecho de fantasma coberto pelo caminho puxado sai (o traço desenhado entrou no lugar dele);
/// o resto fica. O corte é geométrico (perto do caminho), nos dados do fantasma, não só no que aparece.
/// `true` = o traço seguiu o fantasma quase todo (então não pede sugestão nova).
/// Caixas cujo fantasma está fresco agora (antes de gravar o traço com ímã): só essas têm o restante renovado depois
/// (CP3-03b); uma edição alheia anterior continua deixando o fantasma velho.
pub(crate) fn fresh_ghosts(env: &Env) -> Vec<u32> {
    env.ed.copilot.ghosts.iter().filter(|g| cp::check_fresh(&env.core.project, &g.s).is_ok()).map(|g| g.box_id).collect()
}

pub(crate) fn magnet_done(env: &mut Env, m: Magnet, fresh: &[u32]) -> bool {
    let mostly = m.total > 0 && m.pulled * 10 >= m.total * 7;
    if m.path.is_empty() {
        return mostly;
    }
    let (cut, frame) = (env.ed.cut.clone(), env.ed.frame);
    let rad = m.radius;
    let path = &m.path;
    let near_path = |x: f32, y: f32| {
        let p = Vec2::new(x, y);
        path.windows(2).any(|w| {
            let ab = w[1] - w[0];
            let t = ((p - w[0]).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            (p - (w[0] + ab * t)).length() <= rad
        }) || (path.len() == 1 && (p - path[0]).length() <= rad)
    };
    for g in env.ed.copilot.ghosts.iter_mut().filter(|g| Some(&g.s.cut) == cut.as_ref() && g.s.frame == frame) {
        let s = g.s.clone();
        let inside = |x: f32, y: f32| {
            let (cx, cy) = s.to_canvas(x, y);
            near_path(cx, cy)
        };
        let cut_v = |v: &Vec<Stroke>| v.iter().flat_map(|st| cp::split_stroke(st, &inside).1).filter(|st| aiai_paint::sample_stroke(st).len() >= 3).collect::<Vec<_>>();
        g.s.improve = cut_v(&s.improve);
        g.s.complete = cut_v(&s.complete);
        // o traço que seguiu o fantasma é mudança da própria sugestão: o restante continua aceitável (Tab sem forçar)
        if mostly && fresh.contains(&g.box_id) {
            cp::refresh_after_own_accept(&env.core.project, &mut g.s);
        }
    }
    let gone: Vec<u32> = env.ed.copilot.ghosts.iter().filter(|g| g.s.improve.is_empty() && g.s.complete.is_empty()).map(|g| g.box_id).collect();
    env.ed.copilot.ghosts.retain(|g| !g.s.improve.is_empty() || !g.s.complete.is_empty());
    // o fantasma inteiro virou traço do artista: a caixa some, como num aceite
    for id in gone {
        remove_box(&mut env.ed.copilot, id);
    }
    env.ed.copilot.vis_dirty = true;
    refresh_visible(&mut env.ed.copilot);
    mostly
}

/// Estado para a ponte (GET_UI_STATE e `COPILOT status`): serviço, caixas, fila e fantasmas (o que aparece, no canvas).
pub(crate) fn state_json(c: &CopilotUi) -> serde_json::Value {
    let pts = |s: &Suggestion, v: &Vec<Stroke>| {
        v.iter()
            .map(|st| {
                st.points
                    .iter()
                    .map(|q| {
                        let (x, y) = s.to_canvas(q.x, q.y);
                        [x.round(), y.round()]
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "on": c.on, "improve": c.improve, "complete": c.complete, "magnet": c.magnet, "replace_rough": c.replace_rough,
        "steps": c.steps, "res": c.res, "change": c.change, "width_mode": c.width_mode,
        "service": cp::service_state(), "service_error": cp::service_error(), "service_log": cp::service_log_path(),
        "gpu_power_limited": cp::gpu_power_limited(),
        "busy": c.inflight.first().map(|b| b.0), "inflight": c.inflight.iter().map(|b| b.0).collect::<Vec<_>>(),
        "queue": c.dirty.iter().map(|d| d.id).collect::<Vec<_>>(),
        "boxes": c.boxes.iter().map(|b| serde_json::json!({"id": b.id, "cut": b.cut, "rect": b.rect, "state": format!("{:?}", b.state), "prompt": b.prompt, "auto": b.auto,
            "llm": b.llm, "seed": b.seed, "layer": b.layer, "use_global": b.use_global, "geo": b.geo, "content": b.content})).collect::<Vec<_>>(),
        "ghosts": c.ghosts.iter().map(|g| serde_json::json!({"box": g.box_id, "id": g.s.id, "cut": g.s.cut, "frame": g.s.frame, "stale": g.stale,
            "improve": pts(&g.s, &g.vis_improve), "complete": pts(&g.s, &g.vis_complete),
            "window": g.s.window, "gen_size": g.s.gen_size, "ms": g.s.ms, "reduced": g.s.reduced, "throttled": g.s.throttled,
            "prompt_tokens": g.s.prompt_tokens, "prompt_truncated": g.s.prompt_truncated})).collect::<Vec<_>>(),
        "target": c.target, "new_box_mode": c.new_box_mode,
        "last": c.last, "last_reduced": c.last_reduced, "last_throttled": c.last_throttled, "last_truncated": c.last_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cbox(id: u32, auto: bool, llm: bool) -> CBox {
        CBox { id, cut: Id::from("c1"), rect: [10.0, 20.0, 300.0, 200.0], prompt: format!("p{id}"), state: BoxState::Paused, seed: 7, auto, layer: Some(Id::from("L")), use_global: false, geo: 3, content: 4, llm, src: 9, src_region: None }
    }

    #[test]
    fn boxes_and_prompts_round_trip_through_the_project_sidecar_and_a_bad_file_only_warns() {
        // CP1-17: as caixas e os prompts voltam ao abrir o projeto; as do agente (MCP) não
        let dir = std::env::temp_dir().join(format!("aiai-copilot-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = CopilotUi::default();
        // sem caixa nem prompt, não cria arquivo
        save_project_state(&mut a, &dir).unwrap();
        assert!(!dir.join(SIDECAR).exists());
        a.boxes = vec![cbox(2, true, false), cbox(5, false, false), cbox(6, false, true)];
        a.global_prompt = "boy, black hair".into();
        a.change = 0.62;
        a.res = 512;
        a.steps = 8;
        a.improve = false;
        save_project_state(&mut a, &dir).unwrap();
        let mut b = CopilotUi::default();
        b.boxes = vec![cbox(40, false, false)];
        assert!(load_project_state(&mut b, Some(&dir)).is_none());
        assert_eq!(b.boxes.iter().map(|x| x.id).collect::<Vec<_>>(), vec![2, 5], "a caixa do agente não volta; a antiga sai");
        let x = &b.boxes[0];
        assert_eq!((x.rect, x.prompt.as_str(), x.state, x.seed, x.auto, x.layer.clone(), x.use_global), ([10.0, 20.0, 300.0, 200.0], "p2", BoxState::Paused, 7, true, Some(Id::from("L")), false));
        assert_eq!((b.global_prompt.as_str(), b.res, b.steps, b.improve), ("boy, black hair", 512, 8, false));
        assert!((b.change - 0.62).abs() < 1e-6);
        assert_eq!(b.next_box, 6);
        // projeto sem arquivo: nenhuma caixa
        assert!(load_project_state(&mut b, Some(&dir.join("nada"))).is_none());
        assert!(b.boxes.is_empty() && b.global_prompt.is_empty());
        // arquivo editado à mão: dois focos no mesmo corte, id repetido, caixa sem tamanho → só o que vale
        std::fs::write(dir.join(SIDECAR), r#"{"version":1,"boxes":[{"id":1,"cut":"c","rect":[0,0,100,100],"auto":true},{"id":1,"cut":"c","rect":[0,0,50,50]},{"id":2,"cut":"c","rect":[0,0,300,300],"auto":true},{"id":3,"cut":"c","rect":[0,0,0,10]}]}"#).unwrap();
        assert!(load_project_state(&mut b, Some(&dir)).is_none());
        assert_eq!(b.boxes.iter().map(|x| (x.id, x.auto)).collect::<Vec<_>>(), vec![(1, true), (2, false)]);
        // arquivo estragado: aviso, nenhuma caixa
        std::fs::write(dir.join(SIDECAR), b"{nao e json").unwrap();
        assert!(load_project_state(&mut b, Some(&dir)).is_some());
        assert!(b.boxes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copilot_edits_count_as_unsaved_and_a_failed_save_keeps_them_unsaved() {
        // CP-CX03 (parcial): mudar só o prompt deixa o projeto sujo; o Salvar que não grava o copilot.json não limpa
        let dir = std::env::temp_dir().join(format!("aiai-copilot-dirty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut c = CopilotUi::default();
        assert!(!project_state_dirty(&c), "sem projeto aberto, nada conta");
        load_project_state(&mut c, Some(&dir));
        assert!(!project_state_dirty(&c), "recém-aberto");
        c.global_prompt = "boy".into();
        assert!(project_state_dirty(&c), "prompt novo");
        save_project_state(&mut c, &dir).unwrap();
        assert!(!project_state_dirty(&c), "salvo");
        c.boxes.push(cbox(3, false, false));
        // o arquivo vira pasta: gravar falha e o estado continua sujo
        let _ = std::fs::remove_file(dir.join(SIDECAR));
        std::fs::create_dir_all(dir.join(SIDECAR)).unwrap();
        assert!(save_project_state(&mut c, &dir).is_err());
        assert!(project_state_dirty(&c), "a falha não pode limpar");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
