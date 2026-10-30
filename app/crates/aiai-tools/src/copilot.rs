//! Copiloto de traço (protótipo): sugestões de traço numa CAIXA, a partir do que já está desenhado em volta.
//!
//! O gerador roda num processo separado (`tools/copiloto/servico.py`: modelo de imagem de anime + controle pelo
//! rascunho + Anime2Sketch), que carrega os modelos uma vez e atende por stdin/stdout em JSON. Aqui:
//! 1. recorta a janela de contexto em volta da caixa (o modelo vê mais que a caixa) e grava a linha da camada;
//! 2. pede a geração ao serviço;
//! 3. vetoriza a linha gerada (o vetorizador do app) e leva os traços de volta ao canvas;
//! 4. fica só com o que cai dentro da caixa e separa: **Melhorar** (traço que cai sobre a tinta que já existe) e
//!    **Completar** (traço novo).
//! Nada é gravado no documento até aceitar (`accept_command`).

use crate::context::*;
use crate::vectorize::{InkPath, VectorizeOptions, path_to_stroke, vectorize};
use crate::{ToolError, ToolResult};
use aiai_core::{ColorRef, Id, NodeKind, Project, Stroke, StrokeTaper, VectorTip};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Pedido de sugestão numa caixa do canvas.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SuggestRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada (vetorial ou raster) cujo desenho guia a sugestão; os traços aceitos vão para ela se for vetorial.
    pub layer: Id,
    /// Caixa da sugestão no canvas: [x, y, largura, altura].
    pub rect: [f32; 4],
    /// Prompt da caixa ("mechas do cabelo") e o geral, juntos.
    #[serde(default)]
    pub prompt: String,
    /// Melhorar o meu traço (traços sobre a tinta que já existe).
    #[serde(default = "yes")]
    pub improve: bool,
    /// Completar (traços novos).
    #[serde(default = "yes")]
    pub complete: bool,
    /// Passos do modelo (padrão 4).
    #[serde(default)]
    pub steps: Option<u32>,
    /// Resolução de geração (padrão: automática, 384..512).
    #[serde(default)]
    pub res: Option<u32>,
    /// Quanto a sugestão pode se afastar do desenho, de 0 a 1: 0 = só limpar o traço, 1 = criar a partir de um esboço
    /// (um círculo vira um rosto, seguindo o prompt). Escolhe força, fim do controle, peso do prompt e passos; os campos
    /// abaixo, quando vêm, mandam sobre a escolha.
    #[serde(default)]
    pub change: Option<f32>,
    /// Força do controle pelo desenho (0..2).
    #[serde(default)]
    pub strength: Option<f32>,
    /// Fração dos passos em que o desenho controla (0,1..1); menos = mais liberdade depois do começo.
    #[serde(default)]
    pub control_end: Option<f32>,
    /// Peso do prompt (1 = sem orientação; acima de 1 o prompt pesa mais, com prompt negativo; dobra o custo do passo).
    #[serde(default)]
    pub cfg: Option<f32>,
    /// Partir do próprio desenho (0..1, quanto ruído entra sobre ele): pouco = limpa o traço que existe, sem inventar em
    /// volta; perto de 1 = redesenha. 0 = gerar do zero só com o controle (o modo antigo). Padrão: o da Intensidade.
    #[serde(default)]
    pub init: Option<f32>,
    #[serde(default)]
    pub seed: Option<u64>,
    /// Margem de contexto em volta da caixa (×, padrão 2).
    #[serde(default)]
    pub context: Option<f32>,
    #[serde(default)]
    pub tip: VectorTip,
    #[serde(default)]
    pub color: ColorRef,
}

fn yes() -> bool {
    true
}

/// Ajustes de geração que saem de "quanto mudar".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenParams {
    pub strength: f32,
    pub control_end: f32,
    pub cfg: f32,
    pub steps: u32,
    /// Partir do desenho (img2img): fração de ruído sobre ele; 0 = do zero.
    pub init: f32,
}

/// Mudança padrão: Pouca (limpa o traço, colado no desenho; ~2 s).
pub const CHANGE_DEFAULT: f32 = 0.1;

/// Pontos medidos na bateria do prompt (`tools/copiloto/bateria_prompt.py`, rosto limpo do usuário com "boy, black hair"
/// e "girl, long hair, smiling"): (mudança, força do controle, fim do controle, cfg, passos). Entre eles, interpolação
/// linear (passos arredondados). Pouca (0,1) quase copia e limpa; Média (0,5) reinterpreta seguindo o prompt, mantendo
/// o desenho; Muita (0,9) desenha a partir do esboço.
const CHANGE_ANCHORS: [(f32, f32, f32, f32, u32); 5] = [
    (0.0, 1.0, 1.0, 1.0, 4),
    (0.1, 0.85, 0.9, 1.0, 4),
    (0.5, 0.6, 0.6, 2.0, 4),
    (0.9, 0.45, 0.45, 2.3, 6),
    (1.0, 0.4, 0.4, 2.5, 8),
];

/// Quanto ruído entra sobre o próprio desenho em cada Intensidade (rodada CP-R03, `calibracao/tmp/qualidade-r3`): gerar
/// do zero inventava fundo, hachura e cabelo em volta (CP1-10); partindo do desenho, 10 e 50 limpam o rascunho (linhas
/// tremidas e duplas viram uma, as falhas fecham) e 90 redesenha mantendo a pose. Também é mais rápido (1,7 s × 3,1 s
/// a 512 px, Intensidade 10).
const INIT_ANCHORS: [(f32, f32); 5] = [(0.0, 0.35), (0.1, 0.45), (0.5, 0.65), (0.9, 0.85), (1.0, 0.95)];

/// "Quanto mudar" (0..1) → ajustes do gerador.
pub fn change_params(change: f32) -> GenParams {
    let c = if change.is_finite() { change.clamp(0.0, 1.0) } else { CHANGE_DEFAULT };
    let k = CHANGE_ANCHORS.windows(2).position(|w| c <= w[1].0).unwrap_or(CHANGE_ANCHORS.len() - 2);
    let (a, b) = (CHANGE_ANCHORS[k], CHANGE_ANCHORS[k + 1]);
    let t = ((c - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    let l = |x: f32, y: f32| x + (y - x) * t;
    let (ia, ib) = (INIT_ANCHORS[k], INIT_ANCHORS[k + 1]);
    GenParams { strength: l(a.1, b.1), control_end: l(a.2, b.2), cfg: l(a.3, b.3), steps: l(a.4 as f32, b.4 as f32).round() as u32, init: l(ia.1, ib.1) }
}

impl SuggestRequest {
    /// Os ajustes desta sugestão: os de `change` (ou o padrão), com os campos explícitos por cima.
    pub fn gen_params(&self) -> GenParams {
        let mut g = change_params(self.change.unwrap_or(CHANGE_DEFAULT));
        if let Some(v) = self.strength.filter(|v| v.is_finite()) {
            g.strength = v.clamp(0.0, 2.0);
        }
        if let Some(v) = self.control_end.filter(|v| v.is_finite()) {
            g.control_end = v.clamp(0.1, 1.0);
        }
        if let Some(v) = self.cfg.filter(|v| v.is_finite()) {
            g.cfg = v.clamp(1.0, 8.0);
        }
        if let Some(v) = self.steps {
            g.steps = v.clamp(1, 30);
        }
        if let Some(v) = self.init.filter(|v| v.is_finite()) {
            g.init = v.clamp(0.0, 0.99);
        }
        g
    }
}

/// Uma sugestão pronta (fantasma): traços no canvas, separados por função.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Suggestion {
    pub id: u64,
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub rect: [f32; 4],
    pub improve: Vec<Stroke>,
    pub complete: Vec<Stroke>,
    /// Tempo total, da geração e da vetorização (ms).
    pub ms: u64,
    pub gen_ms: u64,
    pub vec_ms: u64,
    /// Janela de contexto que o modelo viu [x, y, w, h] (no espaço do cel) e o tamanho em que gerou [w, h].
    pub window: [i32; 4],
    pub gen_size: [u32; 2],
    /// A caixa no espaço do cel (o retângulo que envolve a caixa do canvas levada pelo movimento inverso da camada).
    /// Os traços de `improve`/`complete` estão neste espaço, o mesmo do cel onde serão gravados.
    pub cel_rect: [f32; 4],
    /// Do cel para o canvas (movimento da camada e transformação do cel, `layer_placement_in`): para desenhar o
    /// fantasma e o ímã no lugar certo. Identidade quando a camada não se move.
    pub placement: [f32; 6],
    /// O cel exposto quando o pedido saiu e a assinatura do conteúdo dele: aceitar pelo MCP recusa uma sugestão velha.
    pub cel: Option<Id>,
    pub cel_sig: u64,
    /// Assinatura do que a sugestão leu (`source_signature` na janela `src_region`, no espaço do cel): o cel exposto,
    /// os traços que tocam a janela, o movimento da camada e as paletas. Aceitar confere de novo (guarda de frescor).
    pub src_sig: u64,
    pub src_region: [f32; 4],
    /// Tokens que o prompt usou e se passou do limite do codificador (o fim é cortado).
    pub prompt_tokens: u32,
    pub prompt_truncated: bool,
    /// A resolução baixou para caber na memória de vídeo livre (o pedido era maior que `gen_size`).
    pub reduced: bool,
    /// A GPU estava com o limite de potência reduzido (notebook na bateria/economia): tudo fica várias vezes mais lento.
    pub throttled: bool,
}

impl Suggestion {
    /// Ponto do espaço do cel levado ao canvas.
    pub fn to_canvas(&self, x: f32, y: f32) -> (f32, f32) {
        aiai_paint::Affine(self.placement).apply(x, y)
    }

    /// O ponto do cel cai dentro da caixa pedida (no canvas)? É a forma exata da caixa, também com a camada girada (a
    /// `cel_rect` é só o retângulo que a envolve).
    pub fn inside(&self, x: f32, y: f32) -> bool {
        let (cx, cy) = self.to_canvas(x, y);
        in_rect(self.rect, cx, cy)
    }
}

fn in_rect(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3]
}

// ------------------------------------------------------------------------------------------------ serviço

/// O processo gerador (um por programa); carrega os modelos uma vez.
struct Service {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "{{\"quit\": true}}");
        let _ = self.stdin.flush();
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.child.kill();
    }
}

fn service() -> &'static Mutex<Option<Service>> {
    static S: OnceLock<Mutex<Option<Service>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

// Estado real do serviço, fora da trava (a interface lê sem esperar): 0 parado, 1 carregando, 2 pronto, 3 gerando,
// 4 falhou. A geração em si segura a trava; ninguém da interface espera por ela.
const ST_STOPPED: u8 = 0;
const ST_LOADING: u8 = 1;
const ST_READY: u8 = 2;
const ST_BUSY: u8 = 3;
const ST_FAILED: u8 = 4;
static STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(ST_STOPPED);

fn set_state(v: u8) {
    STATE.store(v, std::sync::atomic::Ordering::SeqCst);
}

fn last_error() -> &'static Mutex<String> {
    static E: OnceLock<Mutex<String>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(String::new()))
}

/// O serviço falhou: guarda a mensagem para o painel (e o MCP) mostrar.
fn fail(msg: &str) {
    if let Ok(mut e) = last_error().lock() {
        *e = msg.to_string();
    }
    set_state(ST_FAILED);
}

/// A última falha do serviço (vazio se nenhuma).
pub fn service_error() -> String {
    last_error().lock().map(|e| e.clone()).unwrap_or_default()
}

/// Onde o serviço escreve o diagnóstico (stderr do Python: avisos do modelo, CUDA): `%TEMP%\aiai-copiloto\servico.log`
/// (o da sessão anterior fica em `servico.1.log`).
pub fn service_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("aiai-copiloto").join("servico.log")
}

/// Caminho do script do serviço e do Python (variáveis de ambiente ou o padrão do repositório).
fn service_cmd() -> (String, String) {
    let py = std::env::var("AIAI_PYTHON").unwrap_or_else(|_| "python".into());
    let script = std::env::var("AIAI_COPILOT_SERVICE").unwrap_or_else(|_| r"C:\AnimeIAI\tools\copiloto\servico.py".into());
    (py, script)
}

fn spawn() -> ToolResult<Service> {
    let (py, script) = service_cmd();
    if !std::path::Path::new(&script).is_file() {
        return Err(ToolError::Invalid(format!("serviço do copiloto não encontrado: {script} (AIAI_COPILOT_SERVICE)")));
    }
    let log = service_log_path();
    let _ = std::fs::create_dir_all(log.parent().unwrap_or(std::path::Path::new(".")));
    let _ = std::fs::rename(&log, log.with_file_name("servico.1.log"));
    let err = std::fs::File::create(&log).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
    let mut cmd = Proc::new(&py);
    cmd.arg(&script).env("PYTHONIOENCODING", "utf-8").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // sem janela de console
    }
    let mut child = cmd.spawn().map_err(|e| ToolError::Invalid(format!("não consegui iniciar o serviço do copiloto ({py} {script}): {e}")))?;
    let stdin = child.stdin.take().ok_or_else(|| ToolError::Invalid("serviço sem stdin".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| ToolError::Invalid("serviço sem stdout".into()))?;
    // carregando ANTES do leitor existir (senão a linha de pronto podia chegar antes e ser sobrescrita)
    set_state(ST_LOADING);
    if let Ok(mut e) = last_error().lock() {
        e.clear();
    }
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(l) = line else { break };
            // a linha de "pronto" muda o estado já (o painel deixa de dizer "carregando" sem esperar um pedido)
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&l) {
                if v.get("ready").is_some() {
                    if v["ready"] == true {
                        set_state(ST_READY);
                    } else {
                        fail(v["error"].as_str().unwrap_or("o serviço não carregou"));
                    }
                    continue;
                }
            }
            if tx.send(l).is_err() {
                break;
            }
        }
        // o processo fechou a saída: morreu (ou saiu); quem estiver esperando recebe "desconectado"
        if STATE.load(std::sync::atomic::Ordering::SeqCst) != ST_STOPPED {
            fail("o serviço do copiloto fechou (veja o servico.log)");
        }
    });
    Ok(Service { child, stdin, lines: rx, next_id: 1 })
}

/// Estado do serviço: "parado", "carregando", "pronto", "gerando" ou "falhou" (não bloqueia).
pub fn service_state() -> &'static str {
    match STATE.load(std::sync::atomic::Ordering::SeqCst) {
        ST_LOADING => "carregando",
        ST_READY => "pronto",
        ST_BUSY => "gerando",
        ST_FAILED => "falhou",
        _ => "parado",
    }
}

/// Potência da GPU lida AO VIVO (CP2-12): 0 = sem leitura, 1 = normal, 2 = limitada (limite aplicado abaixo da metade
/// do padrão, como o notebook na bateria: 5 W em vez de 50 W), e quando foi lida.
static POWER: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static POWER_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// A GPU está com a potência limitada agora? `None` = sem leitura recente (sem nvidia-smi ou serviço parado). Lida
/// a cada 30 s numa thread própria enquanto o serviço vive, também durante uma geração longa.
pub fn gpu_power_limited() -> Option<bool> {
    let fresh = POWER_AT.lock().ok().and_then(|t| *t).is_some_and(|t| t.elapsed() < Duration::from_secs(90));
    match (fresh, POWER.load(std::sync::atomic::Ordering::Relaxed)) {
        (true, 1) => Some(false),
        (true, 2) => Some(true),
        _ => None,
    }
}

/// Uma leitura do limite de potência (`enforced.power.limit` e o padrão), pelo nvidia-smi.
fn read_power() -> Option<bool> {
    let mut cmd = Proc::new("nvidia-smi");
    cmd.args(["--query-gpu=enforced.power.limit,power.default_limit", "--format=csv,noheader,nounits"]).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // sem janela de console
    }
    let out = cmd.output().ok()?;
    let line = String::from_utf8_lossy(&out.stdout).lines().next()?.to_owned();
    let v: Vec<f32> = line.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() == 2 && v[1] > 0.0).then(|| v[0] < 0.5 * v[1])
}

/// A thread que lê a potência (uma só por programa): a cada 30 s enquanto o serviço não está parado.
fn start_power_watch() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("copiloto-potencia".into()).spawn(|| loop {
        if STATE.load(std::sync::atomic::Ordering::SeqCst) != ST_STOPPED {
            if let Some(limited) = read_power() {
                POWER.store(if limited { 2 } else { 1 }, std::sync::atomic::Ordering::Relaxed);
                if let Ok(mut t) = POWER_AT.lock() {
                    *t = Some(Instant::now());
                }
            }
        }
        std::thread::sleep(Duration::from_secs(30));
    });
}

/// Inicia o serviço se preciso, sem esperar a carga e SEM BLOQUEAR: com a trava ocupada (um pedido em andamento), o
/// serviço já existe. Um serviço morto (processo encerrado) é trocado por um novo.
pub fn start_service() -> ToolResult<()> {
    start_power_watch();
    let Ok(mut g) = service().try_lock() else { return Ok(()) };
    if let Some(s) = g.as_mut() {
        if !matches!(s.child.try_wait(), Ok(None)) {
            *g = None;
        }
    }
    if g.is_none() {
        *g = Some(spawn()?);
    }
    Ok(())
}

/// Para o serviço (o próximo pedido inicia outro): o "Reiniciar" do painel depois de uma falha.
pub fn stop_service() {
    if let Ok(mut g) = service().try_lock() {
        set_state(ST_STOPPED);
        *g = None;
    }
}

/// Pede uma geração e espera a resposta (até `timeout`). Carrega os modelos na primeira vez. Qualquer falha de
/// comunicação (processo morto, cano quebrado, prazo estourado) descarta o serviço: o próximo pedido inicia outro.
fn request_generation(image: &std::path::Path, out: &std::path::Path, prompt: &str, gsz: [u32; 2], gp: GenParams, seed: u64, timeout: Duration) -> ToolResult<serde_json::Value> {
    let mut g = service().lock().map_err(|_| ToolError::Invalid("serviço do copiloto travado".into()))?;
    if let Some(s) = g.as_mut() {
        if !matches!(s.child.try_wait(), Ok(None)) {
            *g = None;
        }
    }
    if g.is_none() {
        *g = Some(spawn()?);
    }
    let t0 = Instant::now();
    // espera a carga (o leitor do stdout marca pronto ou falhou)
    loop {
        match STATE.load(std::sync::atomic::Ordering::SeqCst) {
            ST_READY | ST_BUSY => break,
            ST_FAILED => {
                *g = None;
                return Err(ToolError::Invalid(format!("o serviço do copiloto não carregou: {}", service_error())));
            }
            _ => {}
        }
        let alive = g.as_mut().is_some_and(|s| matches!(s.child.try_wait(), Ok(None)));
        if !alive || t0.elapsed() >= timeout {
            *g = None;
            let msg = if alive { "o serviço do copiloto não carregou a tempo" } else { "o serviço do copiloto fechou durante a carga (veja o servico.log)" };
            fail(msg);
            return Err(ToolError::Invalid(msg.into()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let s = g.as_mut().unwrap();
    let id = s.next_id;
    s.next_id += 1;
    let req = serde_json::json!({"id": id, "image": image, "out": out, "prompt": prompt, "width": gsz[0], "height": gsz[1], "steps": gp.steps, "strength": gp.strength,
        "control_end": gp.control_end, "cfg": gp.cfg, "init": gp.init, "seed": seed});
    set_state(ST_BUSY);
    let res = (|| -> ToolResult<serde_json::Value> {
        writeln!(s.stdin, "{req}").and_then(|_| s.stdin.flush()).map_err(|e| ToolError::Invalid(format!("o serviço do copiloto fechou: {e}")))?;
        loop {
            let line = s.lines.recv_timeout(timeout.saturating_sub(t0.elapsed())).map_err(|e| match e {
                std::sync::mpsc::RecvTimeoutError::Timeout => ToolError::Invalid("o serviço do copiloto não respondeu a tempo".into()),
                std::sync::mpsc::RecvTimeoutError::Disconnected => ToolError::Invalid("o serviço do copiloto fechou (veja o servico.log)".into()),
            })?;
            let v: serde_json::Value = serde_json::from_str(&line).map_err(|e| ToolError::Invalid(format!("resposta inválida do serviço: {e}: {line}")))?;
            if v["id"].as_u64() == Some(id) {
                return Ok(v);
            }
        }
    })();
    match res {
        Ok(v) => {
            set_state(ST_READY);
            if v["ok"] != true {
                // erro do modelo (ex.: sem memória): o serviço continua vivo e atende o próximo
                return Err(ToolError::Invalid(format!("o gerador falhou: {}", v["error"].as_str().unwrap_or("?"))));
            }
            Ok(v)
        }
        Err(e) => {
            // comunicação perdida: fora com este serviço (o Drop mata o processo); o próximo pedido inicia outro
            *g = None;
            fail(&e.to_string());
            Err(e)
        }
    }
}

// ------------------------------------------------------------------------------------------------ sugestão

/// Janela de contexto: a caixa com margem em volta (cada lado cresce `margin` vezes, metade de cada lado), NO FORMATO da caixa,
/// com pelo menos 384 px de cada lado quando o canvas deixa, e sempre dentro do canvas. Nunca corta a caixa: a parte da
/// caixa dentro do canvas cabe inteira (uma caixa maior que o canvas vira o canvas todo).
pub fn context_window(rect: [f32; 4], margin: f32, canvas: (u32, u32)) -> [u32; 4] {
    let (cw, ch) = (canvas.0 as f32, canvas.1 as f32);
    let (bx0, by0) = (rect[0].clamp(0.0, cw), rect[1].clamp(0.0, ch));
    let (bx1, by1) = ((rect[0] + rect[2]).clamp(bx0, cw), (rect[1] + rect[3]).clamp(by0, ch));
    let k = (margin.clamp(1.0, 4.0) - 1.0) / 2.0;
    let w = ((bx1 - bx0) * (1.0 + 2.0 * k)).max(384.0).min(cw).round();
    let h = ((by1 - by0) * (1.0 + 2.0 * k)).max(384.0).min(ch).round();
    let x0 = ((bx0 + bx1) / 2.0 - w / 2.0).clamp(0.0, cw - w).round();
    let y0 = ((by0 + by1) / 2.0 - h / 2.0).clamp(0.0, ch - h).round();
    [x0 as u32, y0 as u32, w as u32, h as u32]
}

/// Tamanho em que o modelo gera a janela: o lado MAIOR vale `res` (automático: o lado maior da janela, de 384 a 512,
/// ou seja 1:1 até 512), o menor na mesma proporção; os dois em múltiplos de 64, de 256 a 768.
pub fn gen_size(win: [u32; 4], res: Option<u32>) -> [u32; 2] {
    let long = win[2].max(win[3]).max(1) as f32;
    let target = res.map(|r| r.clamp(256, 768) as f32).unwrap_or_else(|| long.clamp(384.0, 512.0));
    let k = target / long;
    let r64 = |v: f32| (((v / 64.0).round() as u32) * 64).clamp(256, 768);
    [r64(win[2] as f32 * k), r64(win[3] as f32 * k)]
}

/// Escala do desenho para o modelo: > 1 = o modelo vê o desenho ampliado (caixa pequena, resolução alta: mais detalhe,
/// traço mais fino na volta), < 1 = reduzido (caixa grande: traço fino do desenho pode sumir para o modelo).
pub fn gen_zoom(win: [u32; 4], gsz: [u32; 2]) -> f32 {
    gsz[0].max(gsz[1]) as f32 / win[2].max(win[3]).max(1) as f32
}

/// Onde e em que tamanho uma caixa vai gerar (sem gerar): janela de contexto e tamanho de geração.
pub fn plan(p: &Project, cut: &Id, rect: [f32; 4], margin: Option<f32>, res: Option<u32>) -> ToolResult<([u32; 4], [u32; 2])> {
    let canvas = canvas_size(p, cut_data(p, cut)?);
    let win = context_window(rect, margin.unwrap_or(2.0), canvas);
    Ok((win, gen_size(win, res)))
}

/// Linha da camada no quadro, em escala de cinza sobre branco, na janela [x, y, w, h] do canvas.
fn layer_lines(p: &Project, blobs: &aiai_core::BlobStore, cut_id: &Id, layer: &Id, frame: u32, win: [i32; 4]) -> ToolResult<image::GrayImage> {
    let cut = cut_data(p, cut_id)?;
    let (ww, wh) = (win[2].max(1) as u32, win[3].max(1) as u32);
    let mut out = image::GrayImage::from_pixel(ww, wh, image::Luma([255]));
    let Some(node) = exposed_cel(p, cut, layer, frame) else { return Ok(out) };
    let NodeKind::Cel(cd) = &node.data else { return Ok(out) };
    let name = p.node(layer).map(|n| n.name.clone()).unwrap_or_default();
    // A janela é do cel e pode sair de [0, w] × [0, h] (cel ou camada deslocados: o que se desenhou na beira do canvas
    // mora em coordenada negativa do cel). O render do cel só desenha a partir de (0, 0): o conteúdo é deslocado por
    // tiles inteiros (traços, tiles e preenchimentos juntos, sem reamostrar) até a janela cair dentro.
    let t = aiai_core::TILE_SIZE as i32;
    let (kx, ky) = (win[0].div_euclid(t), win[1].div_euclid(t));
    let (ox, oy) = (win[0] - kx * t, win[1] - ky * t);
    let shifted: aiai_core::CelData;
    let (cel, ox, oy) = if (kx, ky) != (0, 0) && cd.image.is_none() {
        let mut c2 = cd.clone();
        let (dx, dy) = ((kx * t) as f32, (ky * t) as f32);
        for st in &mut c2.strokes {
            for q in &mut st.points {
                q.x -= dx;
                q.y -= dy;
            }
        }
        for tile in &mut c2.tiles {
            tile.x -= kx;
            tile.y -= ky;
        }
        for f in &mut c2.fills {
            f.seed = [f.seed[0] - dx, f.seed[1] - dy];
            f.bounds[0] -= kx * t;
            f.bounds[1] -= ky * t;
        }
        shifted = c2;
        (&shifted, ox, oy)
    } else {
        // cel com imagem importada (presa na origem) ou janela já dentro do cel: sem deslocar
        (cd, win[0], win[1])
    };
    let (rw, rh) = ((ox + win[2]).max(1) as u32, (oy + win[3]).max(1) as u32);
    let (img, _) = aiai_render::compose::render_cel_image(p, blobs, cel, true, (rw, rh), &name);
    let (w, h) = (img.width, img.height);
    let rgba = img.to_rgba8();
    for y in 0..wh {
        for x in 0..ww {
            let (sx, sy) = (ox + x as i32, oy + y as i32);
            if sx < 0 || sy < 0 || sx as u32 >= w || sy as u32 >= h {
                continue;
            }
            let (sx, sy) = (sx as u32, sy as u32);
            let i = ((sy * w + sx) * 4) as usize;
            let (r, g, b, a) = (rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32, rgba[i + 3] as f32 / 255.0);
            let lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
            let ink = a * (1.0 - lum);
            out.put_pixel(x, y, image::Luma([(255.0 * (1.0 - ink)).round() as u8]));
        }
    }
    Ok(out)
}

/// O que a interface prepara (rápido, com o documento) para a thread de geração.
pub struct Prepared {
    pub req: SuggestRequest,
    win: [i32; 4],
    lines: image::GrayImage,
    cel_rect: [f32; 4],
    placement: [f32; 6],
    cel: Option<Id>,
    cel_sig: u64,
    /// `source_signature` na janela de contexto no momento do pedido (a interface guarda na caixa para saber se uma
    /// edição a afeta; a sugestão guarda para a guarda de frescor).
    pub src: u64,
    /// A janela de contexto no espaço do cel (a região que `src` assina).
    pub region: [f32; 4],
}

impl Prepared {
    /// A imagem de contexto (a linha que o modelo vai ler) e a janela dela no espaço do cel.
    pub fn context(&self) -> (&image::GrayImage, [i32; 4]) {
        (&self.lines, self.win)
    }
}

/// Assinatura do conteúdo de um cel (traços, tiles e preenchimentos): muda a cada edição dele.
pub fn cel_signature(p: &Project, cel: &Id) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let Some(n) = p.node(cel) {
        if let NodeKind::Cel(c) = &n.data {
            serde_json::to_string(c).unwrap_or_default().hash(&mut h);
        }
    }
    h.finish()
}

/// A camada pertence ao corte (é descendente dele)?
fn layer_in_cut(p: &Project, layer: &Id, cut: &Id) -> bool {
    let mut cur = p.node(layer).and_then(|n| n.parent.clone());
    while let Some(id) = cur {
        if &id == cut {
            return true;
        }
        cur = p.node(&id).and_then(|n| n.parent.clone());
    }
    false
}

/// Do cel ao canvas para a camada no quadro (movimento da camada, pais e transformação do cel exposto).
pub fn placement_of(p: &Project, cut: &aiai_core::model::CutData, layer: &Id, frame: u32) -> ToolResult<aiai_paint::Affine> {
    let l = layer_data(p, layer)?;
    let ct = exposed_cel(p, cut, layer, frame).and_then(|n| match &n.data {
        NodeKind::Cel(c) => Some(c.transform),
        _ => None,
    });
    let (w, h) = canvas_size(p, cut);
    Ok(aiai_render::compose::layer_placement_in(p, l, ct.unwrap_or([0.0, 0.0, 1.0, 0.0]), frame, w, h))
}

/// Assinatura do que uma caixa lê: o cel exposto da camada no quadro, o conteúdo dele (só os traços que tocam
/// `region`, no espaço do cel, quando dada; os tiles, preenchimentos e a imagem do cel inteiros), o movimento da camada
/// (camada, pais, transformação do cel) e a revisão das paletas (a cor de paleta muda o que o modelo lê sem mudar o
/// cel). Uma edição que não muda isto (outra camada, outro corte, renomear, um traço longe da caixa) não pede outra
/// geração nem deixa a sugestão velha.
pub fn source_signature(p: &Project, cut: &Id, layer: &Id, frame: u32, region: Option<[f32; 4]>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let Ok(cd) = cut_data(p, cut) else { return 0 };
    match exposed_cel(p, cd, layer, frame) {
        Some(n) => {
            n.id.hash(&mut h);
            match (&n.data, region) {
                (NodeKind::Cel(c), Some(r)) => {
                    for st in c.strokes.iter().filter(|st| stroke_touches(st, r)) {
                        serde_json::to_string(st).unwrap_or_default().hash(&mut h);
                    }
                    serde_json::to_string(&(&c.tiles, &c.fills, &c.image, &c.transform)).unwrap_or_default().hash(&mut h);
                }
                _ => cel_signature(p, &n.id).hash(&mut h),
            }
        }
        None => 1u8.hash(&mut h),
    }
    if let Ok(a) = placement_of(p, cd, layer, frame) {
        for v in a.0 {
            v.to_bits().hash(&mut h);
        }
    }
    p.nodes.values().filter(|n| matches!(n.data, NodeKind::Palette(_))).map(|n| n.provenance.revision).max().unwrap_or(0).hash(&mut h);
    h.finish()
}

/// O que o traço pinta toca o retângulo? Limites conservadores da curva inteira (âncoras, alças explícitas ou
/// automáticas, largura por ponto e perfil; `aiai_paint::stroke_bounds`), mais 2 px: uma curva com as âncoras fora e
/// a barriga dentro da caixa conta (CP3-01); um traço que nem chega perto, não.
fn stroke_touches(st: &Stroke, r: [f32; 4]) -> bool {
    let Some([x0, y0, x1, y1]) = aiai_paint::stroke_bounds(st) else { return false };
    let pad = 2.0;
    x1 + pad >= r[0] && x0 - pad <= r[0] + r[2] && y1 + pad >= r[1] && y0 - pad <= r[1] + r[3]
}

/// A sugestão depois de um aceite feito por ela mesma (parcial): o que sobrou continua valendo, com as assinaturas do
/// documento novo (senão a própria mudança a deixaria "velha").
pub fn refresh_after_own_accept(p: &Project, s: &mut Suggestion) {
    if let Ok(cd) = cut_data(p, &s.cut) {
        s.cel = exposed_cel(p, cd, &s.layer, s.frame).map(|n| n.id.clone());
    }
    s.cel_sig = s.cel.as_ref().map(|c| cel_signature(p, c)).unwrap_or(0);
    s.src_sig = source_signature(p, &s.cut, &s.layer, s.frame, Some(s.src_region));
}

/// Maior janela de contexto no espaço do cel (px): a imagem cinza e o render RGBA dela cabem com folga na RAM.
pub const CONTEXT_BUDGET_PX: f64 = 8.0 * 1024.0 * 1024.0;

/// Parte 1 (com o documento, rápida): a janela de contexto e a linha da camada nela.
pub fn prepare(p: &Project, blobs: &aiai_core::BlobStore, req: &SuggestRequest) -> ToolResult<Prepared> {
    let cut = cut_data(p, &req.cut)?;
    layer_data(p, &req.layer)?;
    // validação antes de gastar GPU: a camada é do corte, o quadro existe, a caixa tem tamanho e toca o canvas
    if !layer_in_cut(p, &req.layer, &req.cut) {
        return Err(ToolError::Invalid("a camada não é deste corte".into()));
    }
    if cut.duration > 0 && req.frame >= cut.duration {
        return Err(ToolError::Invalid(format!("quadro {} fora do corte (duração {})", req.frame + 1, cut.duration)));
    }
    if !req.rect.iter().all(|v| v.is_finite()) || req.rect[2] < 4.0 || req.rect[3] < 4.0 {
        return Err(ToolError::Invalid("caixa pequena demais".into()));
    }
    if !(req.improve || req.complete) {
        return Err(ToolError::Invalid("nem Melhorar nem Completar estão ligados".into()));
    }
    let canvas = canvas_size(p, cut);
    // o domínio é o canvas (onde a caixa foi desenhada): fora dele, nada a gerar
    let r = req.rect;
    if r[0] + r[2] <= 0.0 || r[1] + r[3] <= 0.0 || r[0] >= canvas.0 as f32 || r[1] >= canvas.1 as f32 {
        return Err(ToolError::Invalid("a caixa está fora do desenho".into()));
    }
    // a caixa é do canvas; o desenho mora no espaço do cel (camada com movimento): leva os quatro cantos pela inversa
    let fwd = placement_of(p, cut, &req.layer, req.frame)?;
    let inv = fwd.inverse().ok_or_else(|| ToolError::Invalid("o movimento da camada neste quadro não tem inversa (escala zero?)".into()))?;
    let cs = [(r[0], r[1]), (r[0] + r[2], r[1]), (r[0] + r[2], r[1] + r[3]), (r[0], r[1] + r[3])].map(|(x, y)| inv.apply(x, y));
    let (x0, y0) = cs.iter().fold((f32::MAX, f32::MAX), |a, c| (a.0.min(c.0), a.1.min(c.1)));
    let (x1, y1) = cs.iter().fold((f32::MIN, f32::MIN), |a, c| (a.0.max(c.0), a.1.max(c.1)));
    let cel_rect = [x0, y0, x1 - x0, y1 - y0];
    // a janela de contexto é a do CANVAS (o que o artista vê em volta da caixa, presa ao quadro); levada ao cel pela
    // inversa do movimento, vira o retângulo que a envolve, que pode sair de [0, w] × [0, h] (cel deslocado)
    let cw = context_window(req.rect, req.context.unwrap_or(2.0), canvas);
    let wc = [(cw[0] as f32, cw[1] as f32), ((cw[0] + cw[2]) as f32, cw[1] as f32), ((cw[0] + cw[2]) as f32, (cw[1] + cw[3]) as f32), (cw[0] as f32, (cw[1] + cw[3]) as f32)].map(|(x, y)| inv.apply(x, y));
    let (wx0, wy0) = wc.iter().fold((f32::MAX, f32::MAX), |a, c| (a.0.min(c.0), a.1.min(c.1)));
    let (wx1, wy1) = wc.iter().fold((f32::MIN, f32::MIN), |a, c| (a.0.max(c.0), a.1.max(c.1)));
    // orçamento antes de alocar (CP4-03): a camada muito reduzida (escala 0,01) leva a janela do canvas a dezenas de
    // milhares de px do cel; acima de 8 Mpx (uns 2900 × 2900) o contexto não é montado e o pedido volta com erro, sem
    // travar nem derrubar o app
    let area = (wx1 - wx0) as f64 * (wy1 - wy0) as f64;
    if !area.is_finite() || !(wx0.is_finite() && wy0.is_finite()) || area > CONTEXT_BUDGET_PX {
        return Err(ToolError::Invalid(format!(
            "a camada está reduzida demais para o Copiloto ler aqui (a área em volta da caixa daria {:.0} Mpx no desenho da camada; o limite é {:.0}); aumente a escala da camada ou use uma caixa menor",
            area / 1e6,
            CONTEXT_BUDGET_PX / 1e6
        )));
    }
    let win = [wx0.floor() as i32, wy0.floor() as i32, ((wx1 - wx0).ceil() as i32).max(1), ((wy1 - wy0).ceil() as i32).max(1)];
    let lines = layer_lines(p, blobs, &req.cut, &req.layer, req.frame, win)?;
    let cel = exposed_cel(p, cut, &req.layer, req.frame).map(|n| n.id.clone());
    let cel_sig = cel.as_ref().map(|c| cel_signature(p, c)).unwrap_or(0);
    let region = [win[0] as f32, win[1] as f32, win[2] as f32, win[3] as f32];
    let src = source_signature(p, &req.cut, &req.layer, req.frame, Some(region));
    Ok(Prepared { req: req.clone(), win, lines, cel_rect, placement: fwd.0, cel, cel_sig, src, region })
}

/// Gera uma sugestão (bloqueia até o serviço responder).
pub fn suggest(p: &Project, blobs: &aiai_core::BlobStore, req: &SuggestRequest) -> ToolResult<Suggestion> {
    finish(prepare(p, blobs, req)?)
}

/// Parte 2 (sem o documento, demorada: rode numa thread): gera, vetoriza, corta na caixa e separa.
pub fn finish(prep: Prepared) -> ToolResult<Suggestion> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let t0 = Instant::now();
    let Prepared { req, win, lines, cel_rect, placement, cel, cel_sig, src, region } = prep;
    let req = &req;
    let dir = std::env::temp_dir().join("aiai-copiloto");
    std::fs::create_dir_all(&dir).map_err(|e| ToolError::Invalid(format!("pasta temporária: {e}")))?;
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (inp, outp) = (dir.join(format!("ctx-{}-{id}.png", std::process::id())), dir.join(format!("out-{}-{id}.png", std::process::id())));
    // os dois PNG saem em qualquer caminho, inclusive de erro (o disco é apertado)
    struct Tmp(Vec<std::path::PathBuf>);
    impl Drop for Tmp {
        fn drop(&mut self) {
            for f in &self.0 {
                let _ = std::fs::remove_file(f);
            }
        }
    }
    let _tmp = Tmp(vec![inp.clone(), outp.clone()]);
    lines.save(&inp).map_err(|e| ToolError::Invalid(format!("não gravei o contexto: {e}")))?;
    // no formato da janela; 1:1 quando ela cabe na faixa do modelo, acima disso reduz (blocos ficam para depois)
    let gsz = gen_size([0, 0, win[2].max(1) as u32, win[3].max(1) as u32], req.res);
    let r = request_generation(&inp, &outp, &req.prompt, gsz, req.gen_params(), req.seed.unwrap_or(1), Duration::from_secs(300))?;
    let t_gen = t0.elapsed();
    let generated = image::open(&outp).map_err(|e| ToolError::Invalid(format!("saída do gerador ilegível: {e}")))?.to_luma8();
    // vetoriza a linha gerada (no tamanho da janela)
    let ink: Vec<f32> = generated.pixels().map(|q| 1.0 - q.0[0] as f32 / 255.0).collect();
    // simplificação de 1 px (o padrão do vetorizador, 0,4, guarda um ponto a cada poucos px: fiel, mas difícil de
    // editar à mão com o Editar pontos)
    let paths = vectorize(generated.width(), generated.height(), &ink, &VectorizeOptions { threshold: 0.3, min_length: 6.0, simplify: 1.0, ..VectorizeOptions::default() });
    let scale = win[2] as f32 / generated.width() as f32;
    // tinta que já existe (para separar Melhorar de Completar): distância até ela, na janela
    let have: Vec<bool> = lines.pixels().map(|q| q.0[0] < 160).collect();
    let near = dist_field(&have, win[2] as usize, win[3] as usize);
    let (bx0, by0, bx1, by1) = (cel_rect[0], cel_rect[1], cel_rect[0] + cel_rect[2], cel_rect[1] + cel_rect[3]);
    let (mut improve, mut complete) = (Vec::new(), Vec::new());
    // traço mais curto que isto não entra: 2,5% do lado maior da caixa, de 6 a 20 px (um laço fechado conta o perímetro
    // inteiro e vale a metade: olho, narina e boca pequenos ficam)
    let min_len = (cel_rect[2].max(cel_rect[3]) * 0.025).clamp(6.0, 20.0);
    let near_ink = |x: f32, y: f32, tol: f32| {
        let (ix, iy) = (x - win[0] as f32, y - win[1] as f32);
        ix >= 0.0 && iy >= 0.0 && (ix as usize) < win[2] as usize && (iy as usize) < win[3] as usize && near[iy as usize * win[2] as usize + ix as usize] <= tol
    };
    // a forma exata da caixa: o ponto do cel levado ao canvas cai na caixa pedida (a `cel_rect` só envolve; com a camada
    // girada ela é maior que a caixa)
    let fwd = aiai_paint::Affine(placement);
    let inside_box = |x: f32, y: f32| {
        let (cx, cy) = fwd.apply(x, y);
        in_rect(req.rect, cx, cy)
    };
    let change = req.change.unwrap_or(CHANGE_DEFAULT);
    for path in &paths {
        // só o que cai dentro da caixa: o traço é cortado na borda (o copiloto nunca mexe fora dela)
        for piece in clip_to_box(path, scale, [win[0] as f32, win[1] as f32], [bx0, by0, bx1, by1]) {
            // o corte reamostra a cada ~2 px: simplifica de novo (traço editável, com poucos pontos de controle)
            let piece = piece.simplified(1.0);
            let st = even_width(path_to_stroke(&piece, req.tip, req.color.clone(), scale, [win[0] as f32, win[1] as f32]));
            // a curva final (spline) pode sair da caixa mesmo com os pontos de controle dentro: corta a curva desenhada
            let (ins, _) = split_stroke(&st, |x, y| inside_box(x, y));
            for mut st in ins {
                let smp = aiai_paint::sample_stroke(&st);
                if smp.len() < 2 {
                    continue;
                }
                let mut len: f32 = smp.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum();
                if st.closed {
                    len += (smp[0].x - smp[smp.len() - 1].x).hypot(smp[0].y - smp[smp.len() - 1].y);
                }
                // risquinho (hachura de sombra, textura): não é linha que se aceite
                if len < if st.closed { min_len * 0.5 } else { min_len } {
                    continue;
                }
                if !st.closed {
                    st.taper = Some(StrokeTaper::NONE);
                }
                // Melhorar × Completar pelo COMPRIMENTO sobre a tinta que já existe (amostras da curva, ~1 px)
                let on = smp.iter().filter(|q| near_ink(q.x, q.y, 4.0)).count();
                if on * 10 >= smp.len() * 6 {
                    if req.improve {
                        improve.push(st);
                    }
                } else if req.complete {
                    // pouca mudança = limpeza: o traço novo só entra perto do desenho (fecha falhas, não inventa longe)
                    if change < 0.35 && !smp.iter().any(|q| near_ink(q.x, q.y, 40.0)) {
                        continue;
                    }
                    complete.push(st);
                }
            }
        }
    }
    dedupe(&mut improve);
    dedupe(&mut complete);
    // um Completar que repete um Melhorar (linha dupla) sai
    let keep: Vec<Stroke> = complete.into_iter().filter(|c| !duplicates(c, &improve)).collect();
    let complete = keep;
    let ms = t0.elapsed().as_millis() as u64;
    Ok(Suggestion {
        id,
        cut: req.cut.clone(),
        frame: req.frame,
        layer: req.layer.clone(),
        rect: req.rect,
        improve,
        complete,
        ms,
        gen_ms: r["gen_ms"].as_u64().unwrap_or(t_gen.as_millis() as u64),
        vec_ms: (t0.elapsed() - t_gen).as_millis() as u64,
        window: win,
        // o serviço pode ter baixado a resolução para caber na memória: vale o tamanho que ele usou
        gen_size: r["size"].as_array().and_then(|a| Some([a.first()?.as_u64()? as u32, a.get(1)?.as_u64()? as u32])).unwrap_or(gsz),
        cel_rect,
        placement,
        cel,
        cel_sig,
        src_sig: src,
        src_region: region,
        prompt_tokens: r["tokens"].as_u64().unwrap_or(0) as u32,
        prompt_truncated: r["truncated"].as_bool().unwrap_or(false),
        reduced: r["reduced"].as_bool().unwrap_or(false),
        throttled: r["gpu"]["throttled"].as_bool().unwrap_or(false),
    })
}

/// Largura de linha limpa (CP1-10): a medida no encontro de duas linhas ou numa mancha sai bem mais grossa que o resto
/// e vira um "nó" no traço aceito. O multiplicador de cada ponto fica entre 0,6 e 1,35 da largura do traço (a mediana)
/// e é suavizado com os vizinhos: a linha continua variando como no desenho, sem caroço.
pub fn even_width(mut st: Stroke) -> Stroke {
    let n = st.points.len();
    let w: Vec<f32> = st.points.iter().map(|q| q.width.clamp(0.6, 1.35)).collect();
    for i in 0..n {
        let (a, b) = (if i == 0 { if st.closed { n - 1 } else { 0 } } else { i - 1 }, if i + 1 == n { if st.closed { 0 } else { i } } else { i + 1 });
        st.points[i].width = (w[a] + 2.0 * w[i] + w[b]) / 4.0;
    }
    st
}

/// Pedaços de um traço pelas amostras da curva desenhada (a forma não muda): os de dentro de `inside` e os de fora.
/// Um traço todo de um lado volta inteiro e intacto (mesmo id). Os pedaços novos não têm id (o núcleo dá um ao
/// gravar), mantêm cor, opacidade e grupo, e a largura medida ponto a ponto; a ponta do corte é reta.
pub fn split_stroke(st: &Stroke, inside: impl Fn(f32, f32) -> bool) -> (Vec<Stroke>, Vec<Stroke>) {
    let smp = aiai_paint::sample_stroke(st);
    if smp.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let flags: Vec<bool> = smp.iter().map(|q| inside(q.x, q.y)).collect();
    if flags.iter().all(|&f| f) {
        return (vec![st.clone()], Vec::new());
    }
    if flags.iter().all(|&f| !f) {
        return (Vec::new(), vec![st.clone()]);
    }
    let (mut ins, mut outs) = (Vec::new(), Vec::new());
    let mut k = 0;
    while k < smp.len() {
        let f = flags[k];
        let mut e = k;
        while e + 1 < smp.len() && flags[e + 1] == f {
            e += 1;
        }
        // o pedaço vai até a primeira amostra do seguinte (sem buraco na emenda)
        let end = (e + 1).min(smp.len() - 1);
        let pts: Vec<[f32; 2]> = smp[k..=end].iter().map(|q| [q.x, q.y]).collect();
        let ws: Vec<f32> = smp[k..=end].iter().map(|q| (q.r * 2.0).max(0.2)).collect();
        if pts.len() >= 2 {
            let path = InkPath { points: pts, widths: ws, closed: false }.simplified(0.5);
            let mut n = path_to_stroke(&path, st.tip, st.color.clone(), 1.0, [0.0, 0.0]);
            n.opacity = st.opacity;
            n.aliased = st.aliased;
            n.group = st.group.clone();
            n.color_trace = st.color_trace;
            n.taper = Some(StrokeTaper::NONE);
            if f {
                ins.push(n);
            } else {
                outs.push(n);
            }
        }
        k = e + 1;
    }
    (ins, outs)
}

/// `a` repete `b` (80% das amostras de `a` a até max(3 px, largura) de `b`, e `b` não é menor)?
fn repeats(a: &Stroke, b: &Stroke) -> bool {
    let (sa, sb) = (aiai_paint::sample_stroke(a), aiai_paint::sample_stroke(b));
    if sa.is_empty() || sb.len() < sa.len() / 2 {
        return false;
    }
    let tol = a.width.max(3.0);
    let hit = sa.iter().filter(|q| sb.iter().any(|r| (r.x - q.x).hypot(r.y - q.y) <= tol)).count();
    hit * 10 >= sa.len() * 8
}

fn duplicates(a: &Stroke, others: &[Stroke]) -> bool {
    others.iter().any(|b| repeats(a, b))
}

/// Tira linhas duplas (o gerador às vezes traça a mesma linha duas vezes, lado a lado): fica a mais longa.
fn dedupe(v: &mut Vec<Stroke>) {
    v.sort_by(|a, b| aiai_paint::sample_stroke(b).len().cmp(&aiai_paint::sample_stroke(a).len()));
    let mut out: Vec<Stroke> = Vec::new();
    for st in v.drain(..) {
        if !duplicates(&st, &out) {
            out.push(st);
        }
    }
    *v = out;
}

/// Corta o caminho (coordenadas da imagem gerada; `scale`/`offset` levam ao canvas) na caixa [x0, y0, x1, y1] do
/// canvas: pedaços dentro dela, com pelo menos 6 px de canvas. O caminho é amostrado a cada ~2 px do canvas para a
/// borda cair no lugar certo.
pub fn clip_to_box(path: &InkPath, scale: f32, offset: [f32; 2], bx: [f32; 4]) -> Vec<InkPath> {
    let to_c = |q: [f32; 2]| [offset[0] + q[0] * scale, offset[1] + q[1] * scale];
    let inside = |q: [f32; 2]| { let c = to_c(q); c[0] >= bx[0] && c[0] <= bx[2] && c[1] >= bx[1] && c[1] <= bx[3] };
    let n = path.points.len();
    if n < 2 {
        return Vec::new();
    }
    // tudo dentro: o caminho inteiro (fechado continua fechado)
    if path.points.iter().all(|&q| inside(q)) {
        return vec![path.clone()];
    }
    let segs = if path.closed { n } else { n - 1 };
    let step = (2.0 / scale.max(1e-3)).max(0.5);
    let mut dense: Vec<([f32; 2], f32)> = Vec::new();
    for k in 0..segs {
        let (a, b) = (path.points[k], path.points[(k + 1) % n]);
        let (wa, wb) = (path.widths[k], path.widths[(k + 1) % n]);
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        let m = (l / step).ceil().max(1.0) as usize;
        for j in 0..m {
            let t = j as f32 / m as f32;
            dense.push(([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], wa + (wb - wa) * t));
        }
    }
    if !path.closed {
        dense.push((path.points[n - 1], path.widths[n - 1]));
    }
    let mut out = Vec::new();
    let mut cur: Vec<([f32; 2], f32)> = Vec::new();
    let flush = |cur: &mut Vec<([f32; 2], f32)>, out: &mut Vec<InkPath>| {
        let len: f32 = cur.windows(2).map(|w| (w[1].0[0] - w[0].0[0]).hypot(w[1].0[1] - w[0].0[1])).sum::<f32>() * scale;
        if cur.len() >= 2 && len >= 6.0 {
            out.push(InkPath { points: cur.iter().map(|c| c.0).collect(), widths: cur.iter().map(|c| c.1).collect(), closed: false });
        }
        cur.clear();
    };
    for &(q, w) in &dense {
        if inside(q) {
            cur.push((q, w));
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Distância (px, aproximada por chanfro 3-4) até o pixel de tinta mais perto.
fn dist_field(ink: &[bool], w: usize, h: usize) -> Vec<f32> {
    let big = 1e9f32;
    let mut d: Vec<f32> = ink.iter().map(|&b| if b { 0.0 } else { big }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 1.0);
            }
            if y > 0 {
                v = v.min(d[i - w] + 1.0);
                if x > 0 {
                    v = v.min(d[i - w - 1] + 1.4142);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + 1.4142);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 1.0);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 1.0);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + 1.4142);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + 1.4142);
                }
            }
            d[i] = v;
        }
    }
    d
}

/// Quais traços de uma sugestão aceitar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcceptWhich {
    /// Tudo (Tab).
    #[default]
    All,
    Improve,
    Complete,
}

/// Traços do cel que a versão limpa cobre, dentro da caixa: o trecho de dentro de cada um tem pelo menos 70% das amostras
/// a até `tol` px de algum traço limpo (tol = 4 px ou a largura do traço). Devolve os ids que saem e, de cada um, os
/// pedaços de FORA da caixa, que voltam como traços próprios: o Melhorar nunca apaga desenho fora da caixa.
pub fn covered_strokes(cel: &[Stroke], clean: &[&Stroke], rect: [f32; 4]) -> (Vec<Id>, Vec<Stroke>) {
    covered_strokes_by(cel, clean, |x, y| in_rect(rect, x, y))
}

/// `covered_strokes` com a forma da caixa dada por um predicado no espaço do cel (a caixa do canvas levada pelo
/// movimento da camada, que pode estar girada).
pub fn covered_strokes_by(cel: &[Stroke], clean: &[&Stroke], inside: impl Fn(f32, f32) -> bool) -> (Vec<Id>, Vec<Stroke>) {
    const CELL: f32 = 8.0;
    let mut grid: std::collections::HashMap<(i32, i32), Vec<[f32; 2]>> = std::collections::HashMap::new();
    for st in clean {
        for q in aiai_paint::sample_stroke(st) {
            grid.entry(((q.x / CELL).floor() as i32, (q.y / CELL).floor() as i32)).or_default().push([q.x, q.y]);
        }
    }
    if grid.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let near = |x: f32, y: f32, tol: f32| {
        let (cx, cy) = ((x / CELL).floor() as i32, (y / CELL).floor() as i32);
        let r = (tol / CELL).ceil() as i32;
        for gx in cx - r..=cx + r {
            for gy in cy - r..=cy + r {
                if let Some(v) = grid.get(&(gx, gy)) {
                    if v.iter().any(|p| (p[0] - x).hypot(p[1] - y) <= tol) {
                        return true;
                    }
                }
            }
        }
        false
    };
    let (mut ids, mut pieces) = (Vec::new(), Vec::new());
    for st in cel.iter().filter(|st| !st.id.0.is_empty()) {
        let smp = aiai_paint::sample_stroke(st);
        let ins: Vec<_> = smp.iter().filter(|q| inside(q.x, q.y)).collect();
        // traço que mal entra na caixa (menos de 8 amostras dentro): não é rascunho desta caixa
        if ins.len() < 8 {
            continue;
        }
        let tol = (st.width).max(4.0);
        let hit = ins.iter().filter(|q| near(q.x, q.y, tol)).count();
        if hit * 10 < ins.len() * 7 {
            continue;
        }
        ids.push(st.id.clone());
        let (_, outs) = split_stroke(st, &inside);
        pieces.extend(outs.into_iter().filter(|o| aiai_paint::sample_stroke(o).len() >= 2));
    }
    (ids, pieces)
}

/// O traço aceito com a largura escolhida (seção 6 da proposta): `Drawing` mantém a largura medida no desenho gerado;
/// `Pen` usa a largura da caneta ativa como base e guarda a variação relativa; `Fixed` usa a largura da caneta sem
/// variação. A ponta e a cor são as da caneta ativa na hora de aceitar.
pub fn apply_pen(st: &mut Stroke, mode: crate::vectorize::WidthMode, pen_width: f32, tip: VectorTip, color: &ColorRef) {
    use crate::vectorize::WidthMode;
    st.tip = tip;
    st.color = color.clone();
    match mode {
        WidthMode::Drawing => {}
        WidthMode::Pen => st.width = pen_width.max(0.1),
        WidthMode::Fixed => {
            st.width = pen_width.max(0.1);
            for q in st.points.iter_mut() {
                q.width = 1.0;
            }
        }
    }
}

/// A sugestão ainda corresponde ao desenho? Mesmo cel exposto, e o que ela leu (os traços da janela de contexto, o
/// movimento da camada, as paletas) não mudou desde o pedido. Um traço longe da caixa não a deixa velha.
pub fn check_fresh(p: &Project, s: &Suggestion) -> ToolResult<()> {
    let cut = cut_data(p, &s.cut)?;
    let now = exposed_cel(p, cut, &s.layer, s.frame).map(|n| n.id.clone());
    if now != s.cel {
        return Err(ToolError::Invalid("sugestão velha: o cel exposto neste quadro mudou desde o pedido; gere de novo".into()));
    }
    if s.src_sig != 0 {
        if source_signature(p, &s.cut, &s.layer, s.frame, Some(s.src_region)) != s.src_sig {
            return Err(ToolError::Invalid("sugestão velha: o desenho em volta da caixa, o movimento da camada ou a paleta mudou desde o pedido; gere de novo".into()));
        }
        return Ok(());
    }
    if let Some(c) = &s.cel {
        if cel_signature(p, c) != s.cel_sig {
            return Err(ToolError::Invalid("sugestão velha: o desenho do cel mudou desde o pedido; gere de novo".into()));
        }
    }
    Ok(())
}

/// O comando que põe os traços aceitos no cel exposto da camada vetorial (um passo de desfazer). Com `replace`, os
/// traços do rascunho que a versão limpa do Melhorar cobre saem no mesmo passo (substituição).
pub fn accept_command(p: &Project, s: &Suggestion, which: AcceptWhich, only: Option<&[usize]>, replace: bool) -> ToolResult<aiai_core::Command> {
    let cut = cut_data(p, &s.cut)?;
    let l = layer_data(p, &s.layer)?;
    if l.kind != aiai_core::LayerType::Vector {
        return Err(ToolError::Invalid("aceitar põe traços vetoriais: a camada precisa ser vetorial".into()));
    }
    if l.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let cel = exposed_cel(p, cut, &s.layer, s.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", s.frame + 1)))?;
    let NodeKind::Cel(cd) = &cel.data else { unreachable!() };
    let all: Vec<(&Stroke, bool)> = match which {
        AcceptWhich::All => s.improve.iter().map(|x| (x, true)).chain(s.complete.iter().map(|x| (x, false))).collect(),
        AcceptWhich::Improve => s.improve.iter().map(|x| (x, true)).collect(),
        AcceptWhich::Complete => s.complete.iter().map(|x| (x, false)).collect(),
    };
    let picked: Vec<(&Stroke, bool)> = match only {
        Some(idx) => idx.iter().filter_map(|&i| all.get(i).copied()).collect(),
        None => all,
    };
    if picked.is_empty() {
        return Err(ToolError::Invalid("nada para aceitar nesta sugestão".into()));
    }
    let clean: Vec<&Stroke> = picked.iter().filter(|x| x.1).map(|x| x.0).collect();
    let (removed, outside) = if replace && !clean.is_empty() { covered_strokes_by(&cd.strokes, &clean, |x, y| s.inside(x, y)) } else { (Vec::new(), Vec::new()) };
    let start = cd.strokes.len() - removed.len();
    // a ponta do traço limpo que chega na borda da caixa solda na ponta do pedaço do rascunho que ficou fora: a linha
    // continua sem degrau nem falha na borda
    let ends: Vec<(f32, f32)> = outside.iter().filter_map(|o| Some([o.points.first()?, o.points.last()?])).flatten().map(|q| (q.x, q.y)).collect();
    let welded: Vec<Stroke> = picked.iter().map(|(st, is_clean)| if *is_clean { weld_to(st, &ends, s) } else { (*st).clone() }).collect();
    // primeiro os pedaços de fora da caixa dos traços substituídos (o desenho de fora fica igual), depois os aceitos
    let all_new: Vec<Stroke> = outside.into_iter().chain(welded).collect();
    let items: Vec<_> = all_new.into_iter().enumerate().map(|(k, stroke)| aiai_core::command::IndexedStroke { index: start + k, stroke }).collect();
    let insert = aiai_core::Command::InsertStrokes { cel: cel.id.clone(), items };
    if removed.is_empty() {
        return Ok(insert);
    }
    Ok(aiai_core::Command::Batch {
        label: Some(format!("Copiloto: melhorar {} traço(s) (substitui {} do rascunho dentro da caixa)", clean.len(), removed.len())),
        commands: vec![aiai_core::Command::RemoveStrokes { cel: cel.id.clone(), ids: removed }, insert],
    })
}

/// Tira da sugestão o que foi aceito: a categoria `which` inteira ou, com `only`, só esses índices (na ordem de
/// `which`: com `All`, primeiro os do Melhorar, depois os do Completar). O resto continua aceitável. Devolve true quando
/// não sobra nada.
pub fn strip_accepted(s: &mut Suggestion, which: AcceptWhich, only: Option<&[usize]>) -> bool {
    let ni = s.improve.len();
    let (from, to) = match which {
        AcceptWhich::All => (0, ni + s.complete.len()),
        AcceptWhich::Improve => (0, ni),
        AcceptWhich::Complete => (ni, ni + s.complete.len()),
    };
    // posição global (Melhorar e depois Completar) de cada item aceito
    let gone: std::collections::HashSet<usize> = match only {
        Some(idx) => idx.iter().map(|&i| from + i).filter(|&g| g < to).collect(),
        None => (from..to).collect(),
    };
    let mut k = 0;
    s.improve.retain(|_| {
        k += 1;
        !gone.contains(&(k - 1))
    });
    let mut k = ni;
    s.complete.retain(|_| {
        k += 1;
        !gone.contains(&(k - 1))
    });
    s.improve.is_empty() && s.complete.is_empty()
}

/// Leva cada ponta de `st` que está na borda da caixa (até 4 px dela, no canvas) para a ponta de `ends` mais perto
/// (até 10 px): a solda do traço limpo no rascunho que continua fora da caixa.
fn weld_to(st: &Stroke, ends: &[(f32, f32)], s: &Suggestion) -> Stroke {
    let mut out = st.clone();
    if ends.is_empty() || st.closed || out.points.len() < 2 {
        return out;
    }
    let r = s.rect;
    let on_border = |x: f32, y: f32| {
        let (cx, cy) = s.to_canvas(x, y);
        let d = (cx - r[0]).abs().min((cx - r[0] - r[2]).abs()).min((cy - r[1]).abs()).min((cy - r[1] - r[3]).abs());
        d <= 4.0
    };
    let n = out.points.len();
    for k in [0, n - 1] {
        let (x, y) = (out.points[k].x, out.points[k].y);
        if !on_border(x, y) {
            continue;
        }
        let best = ends.iter().map(|&(ex, ey)| ((ex - x).hypot(ey - y), ex, ey)).min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((d, ex, ey)) = best.filter(|b| b.0 <= 10.0) {
            if d > 0.0 {
                out.points[k].x = ex;
                out.points[k].y = ey;
                out.points[k].handle = None;
            }
        }
    }
    out
}

/// Onde os traços aceitos entram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum AcceptTarget {
    /// Na própria camada da sugestão (vetorial); o Melhorar pode substituir o rascunho que cobre.
    #[default]
    Same,
    /// Numa camada vetorial "limpa" logo acima da camada da sugestão (criada na 1ª vez, com o mesmo timing de cels):
    /// o rascunho fica intacto embaixo, como na limpeza de genga para douga. Serve também para rascunho raster.
    Clean,
}

/// Marca, em `params` da camada limpa, de qual camada ela é a limpeza.
pub const CLEAN_PARAM: &str = "copilot_clean_of";

/// A camada limpa da camada `src` (irmã com `params.copilot_clean_of = src`), se já existe.
pub fn clean_layer_of(p: &Project, src: &Id) -> Option<Id> {
    let parent = p.node(src)?.parent.clone()?;
    p.node(&parent)?.children.iter().find(|c| match p.node(c).map(|n| &n.data) {
        Some(NodeKind::Layer(l)) => l.params.get(CLEAN_PARAM).and_then(|v| v.as_str()) == Some(src.as_str()),
        _ => false,
    }).cloned()
}

/// Aceitar em `target`. `Clean` cria (na 1ª vez) a camada limpa acima, o cel com o mesmo número do cel exposto no
/// rascunho e a mesma trilha de exposição; tudo num passo de desfazer.
pub fn accept_command_to(p: &Project, s: &Suggestion, which: AcceptWhich, only: Option<&[usize]>, target: AcceptTarget, replace: bool, ids: &mut aiai_core::IdGen) -> ToolResult<aiai_core::Command> {
    use aiai_core::Command as C;
    if target == AcceptTarget::Same {
        return accept_command(p, s, which, only, replace);
    }
    let cut = cut_data(p, &s.cut)?;
    layer_data(p, &s.layer)?;
    let src = cel_of_frame(p, cut, &s.layer, s.frame)?;
    let all: Vec<&Stroke> = match which {
        AcceptWhich::All => s.improve.iter().chain(s.complete.iter()).collect(),
        AcceptWhich::Improve => s.improve.iter().collect(),
        AcceptWhich::Complete => s.complete.iter().collect(),
    };
    let picked: Vec<&Stroke> = match only {
        Some(idx) => idx.iter().filter_map(|&i| all.get(i).copied()).collect(),
        None => all,
    };
    if picked.is_empty() {
        return Err(ToolError::Invalid("nada para aceitar nesta sugestão".into()));
    }
    let mut cmds = Vec::new();
    let (layer, created) = match clean_layer_of(p, &s.layer) {
        Some(l) => {
            if layer_data(p, &l)?.locked {
                return Err(ToolError::Invalid("a camada limpa está travada".into()));
            }
            (l, false)
        }
        None => {
            let srcn = p.node(&s.layer).ok_or_else(|| ToolError::Invalid("camada sumiu".into()))?;
            let parent = srcn.parent.clone();
            let index = parent.as_ref().and_then(|pa| p.node(pa)).and_then(|pa| pa.children.iter().position(|c| c == &s.layer)).map(|i| i + 1);
            let mut data = aiai_core::model::LayerData::new(aiai_core::LayerType::Vector);
            data.params = serde_json::json!({ CLEAN_PARAM: s.layer.as_str() });
            let id = ids.next();
            cmds.push(C::CreateNode { id: Some(id.clone()), parent, index, name: format!("{} · limpo", srcn.name), data: NodeKind::Layer(data) });
            // o mesmo timing do rascunho: copia a trilha de exposição (os cels têm os mesmos números)
            if let Some(t) = cut.xsheet.track_for_layer(&s.layer) {
                let mut track = aiai_core::xsheet::Track::new(ids.next(), &format!("{} limpo", t.name), t.kind, Some(id.clone()));
                track.cells = t.cells.clone();
                cmds.push(C::AddTrack { cut: s.cut.clone(), index: None, track });
            }
            (id, true)
        }
    };
    // o cel com o número do cel do rascunho (cria se falta)
    let existing = if created { None } else { p.cel_by_number(&layer, src.1) };
    let (cel, start) = match existing {
        Some(n) => match &n.data {
            NodeKind::Cel(c) => (n.id.clone(), c.strokes.len()),
            _ => return Err(ToolError::Invalid("cel inválido na camada limpa".into())),
        },
        None => {
            let id = ids.next();
            cmds.push(C::CreateNode { id: Some(id.clone()), parent: Some(layer.clone()), index: None, name: src.1.to_string(), data: NodeKind::Cel(aiai_core::model::CelData::new(src.1)) });
            (id, 0)
        }
    };
    // camada limpa já existente cuja trilha não expõe esse cel neste quadro: expõe (o rascunho mudou de timing)
    if !created {
        if let Some(t) = cut.xsheet.track_for_layer(&layer) {
            if t.exposed_at(s.frame) != Some(aiai_core::xsheet::CellRef::Cel { number: src.1 }) {
                cmds.push(C::SetCells { cut: s.cut.clone(), track: t.id.clone(), cells: vec![aiai_core::command::CellEdit { frame: s.frame, cell: Some(aiai_core::xsheet::CellRef::Cel { number: src.1 }) }] });
            }
        }
    }
    // os traços estão no espaço do cel do rascunho: leva ao canvas (movimento do rascunho) e daí ao espaço do cel limpo
    // (movimento da camada limpa e transformação do cel dela; camada e cel novos nascem sem movimento)
    let (w, h) = canvas_size(p, cut);
    let dest = if created {
        aiai_render::compose::layer_placement_in(p, &aiai_core::model::LayerData::new(aiai_core::LayerType::Vector), [0.0, 0.0, 1.0, 0.0], s.frame, w, h)
    } else {
        let ct = match existing.map(|n| &n.data) {
            Some(NodeKind::Cel(c)) => c.transform,
            _ => [0.0, 0.0, 1.0, 0.0],
        };
        aiai_render::compose::layer_placement_in(p, layer_data(p, &layer)?, ct, s.frame, w, h)
    };
    let inv = dest.inverse().ok_or_else(|| ToolError::Invalid("o movimento da camada limpa neste quadro não tem inversa (escala zero?)".into()))?;
    let src_to_dest = compose(inv, aiai_paint::Affine(s.placement));
    let n = picked.len();
    let items = picked.into_iter().enumerate().map(|(k, st)| aiai_core::command::IndexedStroke { index: start + k, stroke: affine_stroke(st, src_to_dest) }).collect();
    cmds.push(C::InsertStrokes { cel, items });
    Ok(C::Batch { commands: cmds, label: Some(format!("Copiloto: {n} traço(s) na camada limpa")) })
}

/// `m2 ∘ m1` (primeiro `m1`).
fn compose(m2: aiai_paint::Affine, m1: aiai_paint::Affine) -> aiai_paint::Affine {
    let [a1, b1, c1, d1, e1, f1] = m1.0;
    let [a2, b2, c2, d2, e2, f2] = m2.0;
    aiai_paint::Affine([a2 * a1 + b2 * d1, a2 * b1 + b2 * e1, a2 * c1 + b2 * f1 + c2, d2 * a1 + e2 * d1, d2 * b1 + e2 * e1, d2 * c1 + e2 * f1 + f2])
}

/// O traço levado por uma transformação afim: pontos, alças (só a parte linear) e a largura (pela escala média).
pub fn affine_stroke(st: &Stroke, m: aiai_paint::Affine) -> Stroke {
    if m.is_identity() {
        return st.clone();
    }
    let [a, b, _, d, e, _] = m.0;
    let scale = (a * e - b * d).abs().sqrt();
    let mut out = st.clone();
    for q in out.points.iter_mut() {
        let (x, y) = m.apply(q.x, q.y);
        q.x = x;
        q.y = y;
        if let Some([hx, hy]) = q.handle {
            q.handle = Some([a * hx + b * hy, d * hx + e * hy]);
        }
    }
    if scale.is_finite() && scale > 0.0 {
        out.width *= scale;
    }
    out
}

/// O cel exposto da camada no quadro: (id, número).
fn cel_of_frame(p: &Project, cut: &aiai_core::model::CutData, layer: &Id, frame: u32) -> ToolResult<(Id, u32)> {
    let n = exposed_cel(p, cut, layer, frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", frame + 1)))?;
    match &n.data {
        NodeKind::Cel(c) => Ok((n.id.clone(), c.number)),
        _ => Err(ToolError::Invalid("cel inválido".into())),
    }
}

// ------------------------------------------------------------------------------------------------ sugestões guardadas

fn store() -> &'static Mutex<std::collections::BTreeMap<u64, Suggestion>> {
    static S: OnceLock<Mutex<std::collections::BTreeMap<u64, Suggestion>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

/// Guarda a sugestão (fantasma) até aceitar ou descartar; devolve o id.
pub fn keep(s: Suggestion) -> u64 {
    let id = s.id;
    if let Ok(mut m) = store().lock() {
        m.insert(id, s);
    }
    id
}

pub fn get(id: u64) -> Option<Suggestion> {
    store().lock().ok().and_then(|m| m.get(&id).cloned())
}

pub fn discard(id: u64) -> bool {
    store().lock().ok().is_some_and(|mut m| m.remove(&id).is_some())
}

/// Pedido único do MCP/bridge: `op` = suggest | accept | discard | status.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct CopilotRequest {
    pub op: String,
    /// Para `suggest`: o pedido.
    #[serde(default)]
    pub suggest: Option<SuggestRequest>,
    /// Para `accept` e `discard`: o id da sugestão.
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub which: AcceptWhich,
    /// Para `accept`: só estes índices (na ordem de `which`).
    #[serde(default)]
    pub only: Option<Vec<usize>>,
    /// Para `accept`: o Melhorar substitui os traços do rascunho que cobre (padrão true); false mantém o rascunho.
    #[serde(default = "yes")]
    pub replace: bool,
    /// Para `accept`: `same` (padrão, na camada da sugestão) ou `clean` (camada limpa acima, criada na 1ª vez).
    #[serde(default)]
    pub target: AcceptTarget,
    /// Para `accept`: aceitar mesmo se o desenho mudou desde o pedido (padrão false: recusa sugestão velha).
    #[serde(default)]
    pub force: bool,
}

/// Resumo de uma sugestão para o agente: contagens, tempos e os pontos de cada traço.
pub fn summary(s: &Suggestion) -> serde_json::Value {
    let pts = |v: &Vec<Stroke>| v.iter().map(|st| st.points.iter().map(|q| [q.x.round(), q.y.round()]).collect::<Vec<_>>()).collect::<Vec<_>>();
    serde_json::json!({"id": s.id, "improve": s.improve.len(), "complete": s.complete.len(), "ms": s.ms, "gen_ms": s.gen_ms, "vec_ms": s.vec_ms,
        "window": s.window, "gen_size": s.gen_size, "zoom": gen_zoom([0, 0, s.window[2].max(1) as u32, s.window[3].max(1) as u32], s.gen_size),
        "rect": s.rect, "cel_rect": s.cel_rect, "cel": s.cel, "frame": s.frame, "cut": s.cut, "layer": s.layer,
        "prompt_tokens": s.prompt_tokens, "prompt_truncated": s.prompt_truncated, "reduced": s.reduced, "throttled": s.throttled,
        "improve_points": pts(&s.improve), "complete_points": pts(&s.complete), "points_space": "cel"})
}
