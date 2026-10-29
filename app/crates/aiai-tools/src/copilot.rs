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
    /// Força do controle pelo desenho (0..2, padrão 0,8).
    #[serde(default)]
    pub strength: Option<f32>,
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
}

// ------------------------------------------------------------------------------------------------ serviço

/// O processo gerador (um por programa); carrega os modelos uma vez.
struct Service {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    ready: bool,
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
    let mut cmd = Proc::new(&py);
    cmd.arg(&script).env("PYTHONIOENCODING", "utf-8").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // sem janela de console
    }
    let mut child = cmd.spawn().map_err(|e| ToolError::Invalid(format!("não consegui iniciar o serviço do copiloto ({py} {script}): {e}")))?;
    let stdin = child.stdin.take().ok_or_else(|| ToolError::Invalid("serviço sem stdin".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| ToolError::Invalid("serviço sem stdout".into()))?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    Ok(Service { child, stdin, lines: rx, ready: false, next_id: 1 })
}

/// Estado do serviço: "parado", "carregando" ou "pronto" (não bloqueia).
pub fn service_state() -> &'static str {
    // sem esperar: com a trava presa, o serviço está atendendo um pedido
    match service().try_lock() {
        Ok(g) => match g.as_ref() {
            Some(s) if s.ready => "pronto",
            Some(_) => "carregando",
            None => "parado",
        },
        Err(_) => "gerando",
    }
}

/// Inicia o serviço se preciso (não espera a carga).
pub fn start_service() -> ToolResult<()> {
    let mut g = service().lock().map_err(|_| ToolError::Invalid("serviço do copiloto travado".into()))?;
    if g.is_none() {
        *g = Some(spawn()?);
    }
    Ok(())
}

/// Pede uma geração e espera a resposta (até `timeout`). Carrega os modelos na primeira vez.
fn request_generation(image: &std::path::Path, out: &std::path::Path, prompt: &str, res: u32, steps: u32, strength: f32, seed: u64, timeout: Duration) -> ToolResult<serde_json::Value> {
    start_service()?;
    let mut g = service().lock().map_err(|_| ToolError::Invalid("serviço do copiloto travado".into()))?;
    let s = g.as_mut().unwrap();
    let t0 = Instant::now();
    let wait_line = |s: &mut Service, left: Duration| -> ToolResult<serde_json::Value> {
        let line = s.lines.recv_timeout(left).map_err(|_| ToolError::Invalid("o serviço do copiloto não respondeu a tempo (ou fechou)".into()))?;
        serde_json::from_str(&line).map_err(|e| ToolError::Invalid(format!("resposta inválida do serviço: {e}: {line}")))
    };
    if !s.ready {
        let v = wait_line(s, timeout.saturating_sub(t0.elapsed()))?;
        if v["ready"] != true {
            let err = v["error"].as_str().unwrap_or("erro desconhecido").to_string();
            *g = None;
            return Err(ToolError::Invalid(format!("o serviço do copiloto não carregou: {err}")));
        }
        s.ready = true;
    }
    let id = s.next_id;
    s.next_id += 1;
    let req = serde_json::json!({"id": id, "image": image, "out": out, "prompt": prompt, "res": res, "steps": steps, "strength": strength, "seed": seed});
    writeln!(s.stdin, "{req}").and_then(|_| s.stdin.flush()).map_err(|e| ToolError::Invalid(format!("o serviço do copiloto fechou: {e}")))?;
    loop {
        let v = wait_line(s, timeout.saturating_sub(t0.elapsed()))?;
        if v["id"].as_u64() == Some(id) {
            if v["ok"] != true {
                return Err(ToolError::Invalid(format!("o gerador falhou: {}", v["error"].as_str().unwrap_or("?"))));
            }
            return Ok(v);
        }
    }
}

// ------------------------------------------------------------------------------------------------ sugestão

/// Janela de contexto: a caixa com margem, quadrada, de pelo menos 384 px, dentro do canvas.
pub fn context_window(rect: [f32; 4], margin: f32, canvas: (u32, u32)) -> [u32; 4] {
    let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
    let side = (rect[2].max(rect[3]) * margin.clamp(1.0, 4.0)).max(384.0).min(canvas.0.min(canvas.1) as f32);
    let x0 = (cx - side / 2.0).clamp(0.0, canvas.0 as f32 - side);
    let y0 = (cy - side / 2.0).clamp(0.0, canvas.1 as f32 - side);
    [x0.round() as u32, y0.round() as u32, side.round() as u32, side.round() as u32]
}

/// Linha da camada no quadro, em escala de cinza sobre branco, na janela [x, y, w, h] do canvas.
fn layer_lines(p: &Project, blobs: &aiai_core::BlobStore, cut_id: &Id, layer: &Id, frame: u32, win: [u32; 4]) -> ToolResult<image::GrayImage> {
    let cut = cut_data(p, cut_id)?;
    let (w, h) = canvas_size(p, cut);
    let mut out = image::GrayImage::from_pixel(win[2], win[3], image::Luma([255]));
    let Some(node) = exposed_cel(p, cut, layer, frame) else { return Ok(out) };
    let NodeKind::Cel(cd) = &node.data else { return Ok(out) };
    let name = p.node(layer).map(|n| n.name.clone()).unwrap_or_default();
    let (img, _) = aiai_render::compose::render_cel_image(p, blobs, cd, true, (w, h), &name);
    let rgba = img.to_rgba8();
    for y in 0..win[3] {
        for x in 0..win[2] {
            let (sx, sy) = (win[0] + x, win[1] + y);
            if sx >= w || sy >= h {
                continue;
            }
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
    win: [u32; 4],
    lines: image::GrayImage,
}

/// Parte 1 (com o documento, rápida): a janela de contexto e a linha da camada nela.
pub fn prepare(p: &Project, blobs: &aiai_core::BlobStore, req: &SuggestRequest) -> ToolResult<Prepared> {
    let cut = cut_data(p, &req.cut)?;
    layer_data(p, &req.layer)?;
    let canvas = canvas_size(p, cut);
    if req.rect[2] < 4.0 || req.rect[3] < 4.0 {
        return Err(ToolError::Invalid("caixa pequena demais".into()));
    }
    let win = context_window(req.rect, req.context.unwrap_or(2.0), canvas);
    let lines = layer_lines(p, blobs, &req.cut, &req.layer, req.frame, win)?;
    Ok(Prepared { req: req.clone(), win, lines })
}

/// Gera uma sugestão (bloqueia até o serviço responder).
pub fn suggest(p: &Project, blobs: &aiai_core::BlobStore, req: &SuggestRequest) -> ToolResult<Suggestion> {
    finish(prepare(p, blobs, req)?)
}

/// Parte 2 (sem o documento, demorada: rode numa thread): gera, vetoriza, corta na caixa e separa.
pub fn finish(prep: Prepared) -> ToolResult<Suggestion> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let t0 = Instant::now();
    let Prepared { req, win, lines } = prep;
    let req = &req;
    let dir = std::env::temp_dir().join("aiai-copiloto");
    std::fs::create_dir_all(&dir).map_err(|e| ToolError::Invalid(format!("pasta temporária: {e}")))?;
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (inp, outp) = (dir.join(format!("ctx-{}-{id}.png", std::process::id())), dir.join(format!("out-{}-{id}.png", std::process::id())));
    lines.save(&inp).map_err(|e| ToolError::Invalid(format!("não gravei o contexto: {e}")))?;
    // 1:1 quando a janela cabe na faixa do modelo; acima disso, reduz (blocos ficam para depois)
    let res = req.res.unwrap_or_else(|| (win[2].clamp(384, 512) / 64) * 64);
    let r = request_generation(&inp, &outp, &req.prompt, res, req.steps.unwrap_or(4).clamp(1, 30), req.strength.unwrap_or(0.8).clamp(0.0, 2.0), req.seed.unwrap_or(1), Duration::from_secs(300))?;
    let t_gen = t0.elapsed();
    let generated = image::open(&outp).map_err(|e| ToolError::Invalid(format!("saída do gerador ilegível: {e}")))?.to_luma8();
    let _ = std::fs::remove_file(&inp);
    let _ = std::fs::remove_file(&outp);
    // vetoriza a linha gerada (no tamanho da janela)
    let ink: Vec<f32> = generated.pixels().map(|q| 1.0 - q.0[0] as f32 / 255.0).collect();
    let paths = vectorize(generated.width(), generated.height(), &ink, &VectorizeOptions { threshold: 0.3, min_length: 6.0, ..VectorizeOptions::default() });
    let scale = win[2] as f32 / generated.width() as f32;
    // tinta que já existe (para separar Melhorar de Completar): distância até ela, na janela
    let have: Vec<bool> = lines.pixels().map(|q| q.0[0] < 160).collect();
    let near = dist_field(&have, win[2] as usize, win[3] as usize);
    let (bx0, by0, bx1, by1) = (req.rect[0], req.rect[1], req.rect[0] + req.rect[2], req.rect[1] + req.rect[3]);
    let (mut improve, mut complete) = (Vec::new(), Vec::new());
    for path in &paths {
        // só o que cai dentro da caixa: o traço é cortado na borda (o copiloto nunca mexe fora dela)
        for piece in clip_to_box(path, scale, [win[0] as f32, win[1] as f32], [bx0, by0, bx1, by1]) {
            let on_ink = piece
                .points
                .iter()
                .filter(|q| {
                    let (x, y) = ((q[0] * scale) as usize, (q[1] * scale) as usize);
                    x < win[2] as usize && y < win[3] as usize && near[y * win[2] as usize + x] <= 4.0
                })
                .count();
            let mut st = path_to_stroke(&piece, req.tip, req.color.clone(), scale, [win[0] as f32, win[1] as f32]);
            if !piece.closed {
                st.taper = Some(StrokeTaper::NONE);
            }
            if on_ink * 10 >= piece.points.len() * 6 {
                if req.improve {
                    improve.push(st);
                }
            } else if req.complete {
                complete.push(st);
            }
        }
    }
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
    })
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

/// Traços do cel que a versão limpa cobre: dentro da caixa e com pelo menos 70% das amostras a até `tol` px de algum
/// traço limpo (tol = 4 px ou a largura do traço, o maior). São o rascunho que o Melhorar substitui.
pub fn covered_strokes(cel: &[Stroke], clean: &[&Stroke], rect: [f32; 4]) -> Vec<Id> {
    const CELL: f32 = 8.0;
    let mut grid: std::collections::HashMap<(i32, i32), Vec<[f32; 2]>> = std::collections::HashMap::new();
    for st in clean {
        for q in aiai_paint::sample_stroke(st) {
            grid.entry(((q.x / CELL).floor() as i32, (q.y / CELL).floor() as i32)).or_default().push([q.x, q.y]);
        }
    }
    if grid.is_empty() {
        return Vec::new();
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
    let inside = |x: f32, y: f32| x >= rect[0] && x <= rect[0] + rect[2] && y >= rect[1] && y <= rect[1] + rect[3];
    cel.iter()
        .filter(|st| !st.id.0.is_empty())
        .filter(|st| {
            let smp = aiai_paint::sample_stroke(st);
            if smp.is_empty() {
                return false;
            }
            let ins = smp.iter().filter(|q| inside(q.x, q.y)).count();
            if ins * 2 < smp.len() {
                return false; // o traço é mais de fora da caixa: o copiloto não mexe nele
            }
            let tol = (st.width).max(4.0);
            let hit = smp.iter().filter(|q| near(q.x, q.y, tol)).count();
            hit * 10 >= smp.len() * 7
        })
        .map(|st| st.id.clone())
        .collect()
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
    let cel = exposed_cel(p, cut, &s.layer, s.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", s.frame)))?;
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
    let removed = if replace && !clean.is_empty() { covered_strokes(&cd.strokes, &clean, s.rect) } else { Vec::new() };
    let start = cd.strokes.len() - removed.len();
    let items: Vec<_> = picked.iter().enumerate().map(|(k, (st, _))| aiai_core::command::IndexedStroke { index: start + k, stroke: (*st).clone() }).collect();
    let insert = aiai_core::Command::InsertStrokes { cel: cel.id.clone(), items };
    if removed.is_empty() {
        return Ok(insert);
    }
    Ok(aiai_core::Command::Batch {
        label: Some(format!("Copiloto: melhorar {} traço(s) (substitui {} do rascunho)", clean.len(), removed.len())),
        commands: vec![aiai_core::Command::RemoveStrokes { cel: cel.id.clone(), ids: removed }, insert],
    })
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
}

/// Resumo de uma sugestão para o agente: contagens, tempos e os pontos de cada traço.
pub fn summary(s: &Suggestion) -> serde_json::Value {
    let pts = |v: &Vec<Stroke>| v.iter().map(|st| st.points.iter().map(|q| [q.x.round(), q.y.round()]).collect::<Vec<_>>()).collect::<Vec<_>>();
    serde_json::json!({"id": s.id, "improve": s.improve.len(), "complete": s.complete.len(), "ms": s.ms, "gen_ms": s.gen_ms, "vec_ms": s.vec_ms,
        "rect": s.rect, "improve_points": pts(&s.improve), "complete_points": pts(&s.complete)})
}
