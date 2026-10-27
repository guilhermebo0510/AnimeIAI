//! Workspace Áudio nível Audacity/Audition (S24 3.8–3.12, 4.13): painéis Forma de onda (`wave`), Efeitos (`rack`),
//! Medidores (`meters`), Multipista (`multitrack`) e Voz sintética (`tts`), sobre o motor `aiai-audio` (edição, efeitos
//! com rack, pirâmide de forma de onda, espectrograma, LUFS em fluxo).
//!
//! Regras comuns:
//! - toda edição é **não destrutiva**: os passos (`EditStep`) vão para `aiai_tools::audio_edit`, que grava um asset NOVO
//!   (a fonte fica intacta, `params.edited_from`/`steps` guardam a origem); o clipe do corte passa a apontar para ele no
//!   mesmo passo de desfazer. O trabalho pesado roda num fio sobre uma cópia mínima do documento e volta como comandos;
//! - a reprodução (prévia, seleção, takes, mixagem da multipista) é do `player` deste módulo, com medidores ao vivo;
//! - nada de arquivo com `--no-session` (`user_files == false`): as predefinições de rack ficam desligadas.

pub(crate) mod hw;
pub(crate) mod meters;
pub(crate) mod multitrack;
pub(crate) mod player;
pub(crate) mod rack;
pub(crate) mod tts;
pub(crate) mod wave;

use crate::app::actions;
use crate::app::commands::NodeId;
use crate::app::editor::Editor;
use aiai_audio::AudioBuf;
use aiai_audio::analysis::{WaveformCache, WaveformPyramid};
use aiai_core::{AssetType, AudioPlacement, Command, CoreSession, Id, IdGen, NodeKind, Project};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Buffers decodificados guardados (os WAV do projeto; o mais antigo sai).
const BUF_CAP: usize = 8;

/// Estado do workspace Áudio que não mora no documento.
pub(crate) struct AudioWsState {
    pub player: player::Player,
    pub wave: wave::WaveState,
    pub rack: rack::RackState,
    pub meters: meters::MeterState,
    pub mt: multitrack::MtState,
    pub tts: tts::TtsState,
    pub hw: hw::HwInfo,
    /// Pode ler/gravar arquivos do usuário (predefinições de rack); falso com `--no-session`.
    pub user_files: bool,
    /// Trabalho em segundo plano (edição, TTS) e o que fazer com o asset novo.
    pub job: Option<Job>,
    pyramids: WaveformCache,
    bufs: HashMap<String, Arc<AudioBuf>>,
    order: VecDeque<String>,
}

impl Default for AudioWsState {
    fn default() -> Self {
        AudioWsState {
            player: Default::default(),
            wave: Default::default(),
            rack: Default::default(),
            meters: Default::default(),
            mt: Default::default(),
            tts: Default::default(),
            hw: Default::default(),
            user_files: false,
            job: None,
            pyramids: WaveformCache::new(16),
            bufs: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

/// Áudio de um asset pronto para desenhar: chave (hash do blob), amostras e pirâmide.
#[derive(Clone)]
pub(crate) struct Loaded {
    pub key: String,
    pub buf: Arc<AudioBuf>,
    pub pyr: Arc<WaveformPyramid>,
}

impl AudioWsState {
    /// Lê (uma vez) o WAV do asset e a pirâmide dele.
    pub(crate) fn load(&mut self, core: &CoreSession, asset: &Id) -> Option<Loaded> {
        let key = aiai_tools::audio_edit::asset_blob_key(&core.project, asset)?;
        let buf = match self.bufs.get(&key) {
            Some(b) => b.clone(),
            None => {
                let b = Arc::new(aiai_tools::audio::asset_audio(&core.project, &core.blobs, asset).ok()?);
                self.bufs.insert(key.clone(), b.clone());
                self.order.push_back(key.clone());
                while self.order.len() > BUF_CAP {
                    if let Some(old) = self.order.pop_front() {
                        self.bufs.remove(&old);
                    }
                }
                b
            }
        };
        let b2 = buf.clone();
        let pyr = self.pyramids.get_or_build(&key, || Ok((*b2).clone())).ok()?;
        Some(Loaded { key, buf, pyr })
    }
}

// ------------------------------------------------------------------------------------------------ documento

/// Assets de áudio do projeto (id, nome), em ordem de nome.
pub(crate) fn audio_assets(p: &Project) -> Vec<(Id, String)> {
    let mut v: Vec<(Id, String)> = p.nodes.values().filter(|n| matches!(&n.data, NodeKind::Asset(a) if a.asset == AssetType::AudioClip)).map(|n| (n.id.clone(), n.name.clone())).collect();
    v.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.0.cmp(&b.0.0)));
    v
}

pub(crate) fn asset_params(p: &Project, id: &Id) -> Value {
    match p.node(id).map(|n| &n.data) {
        Some(NodeKind::Asset(a)) => a.params.clone(),
        _ => Value::Null,
    }
}

pub(crate) fn node_name(p: &Project, id: &Id) -> String {
    p.node(id).map(|n| n.name.clone()).unwrap_or_else(|| "?".into())
}

pub(crate) fn cut_audio(p: &Project, cut: &NodeId) -> (u32, Vec<AudioPlacement>) {
    match p.node(cut).map(|n| &n.data) {
        Some(NodeKind::Cut(cd)) => (cd.duration, cd.audio.clone()),
        _ => (0, Vec::new()),
    }
}

/// Tempo em `m:ss.mmm` (ou `h:mm:ss`).
pub(crate) fn fmt_time(s: f64) -> String {
    let s = s.max(0.0);
    let m = (s / 60.0).floor();
    if m >= 60.0 {
        format!("{}:{:02}:{:04.1}", (m / 60.0) as u32, (m % 60.0) as u32, s % 60.0)
    } else {
        format!("{}:{:06.3}", m as u32, s - m * 60.0)
    }
}

// ------------------------------------------------------------------------------------------------ trabalhos em segundo plano

/// Resultado de um trabalho: os comandos que criam o asset novo e os blobs dele (entram no `BlobStore` real ao aplicar).
pub(crate) struct JobOut {
    pub commands: Vec<Command>,
    pub asset: Id,
    pub blobs: Vec<Arc<Vec<u8>>>,
    pub duration_s: f64,
}

/// O que fazer com o asset novo.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum After {
    /// O clipe `placement` do corte passa a tocar o asset novo (mesmo passo de desfazer). `trimmed`: o resultado já é só
    /// o trecho usado (aparar) e o recorte do clipe volta a zero.
    Retarget { cut: NodeId, placement: Id, trimmed: bool },
    /// O editor de forma de onda abre o asset novo (asset fora de corte).
    Open,
    /// Take novo da Voz sintética.
    Take { line: u64, label: String },
}

pub(crate) struct Job {
    rx: crossbeam_channel::Receiver<Result<JobOut, String>>,
    pub after: After,
    pub label: String,
    pub started: Instant,
}

/// Cópia mínima do documento com um asset (e o blob dele): o suficiente para `audio_edit` rodar fora do fio da UI.
fn minimal_copy(core: &CoreSession, asset: &Id) -> Option<(Project, aiai_core::BlobStore)> {
    let n = core.project.node(asset)?.clone();
    let NodeKind::Asset(a) = &n.data else { return None };
    let bytes = core.blobs.get(a.blob.as_ref()?)?;
    let mut p = Project::new("audio-edit");
    p.settings = core.project.settings.clone();
    p.nodes.insert(asset.clone(), aiai_core::Node { parent: None, children: Vec::new(), ..n });
    let mut b = aiai_core::BlobStore::default();
    b.put(bytes.to_vec());
    Some((p, b))
}

fn new_blobs_of(cmds: &[Command], blobs: &aiai_core::BlobStore) -> Vec<Arc<Vec<u8>>> {
    cmds.iter()
        .filter_map(|c| match c {
            Command::CreateNode { data: NodeKind::Asset(a), .. } => a.blob.as_ref().and_then(|h| blobs.get(h)),
            _ => None,
        })
        .collect()
}

/// Edição por passos num fio: `audio_edit` grava o resultado como asset novo na cópia; os comandos voltam para a UI.
pub(crate) fn spawn_edit(st: &mut AudioWsState, core: &CoreSession, asset: &Id, steps: Vec<aiai_audio::steps::EditStep>, label: String, after: After) -> Result<(), String> {
    if st.job.is_some() {
        return Err(crate::i18n::tr("audio.job_busy").into());
    }
    let (p, mut b) = minimal_copy(core, asset).ok_or_else(|| crate::i18n::tr("audio.no_audio").to_owned())?;
    let req = aiai_tools::audio_edit::AudioEditRequest { asset: asset.clone(), steps, name: None, track: aiai_core::AudioTrack::Sfx, bits: aiai_tools::audio::WavBits::Pcm24, place: Default::default() };
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("aiai-audio-edit".into())
        .spawn(move || {
            let mut ids = IdGen::Uuid7;
            let r = aiai_tools::audio_edit::audio_edit(&p, &mut ids, &mut b, &req).map(|out| {
                let blobs = new_blobs_of(&out.clip.commands, &b);
                JobOut { commands: out.clip.commands, asset: out.clip.asset, blobs, duration_s: out.clip.duration_s }
            });
            let _ = tx.send(r.map_err(|e| e.to_string()));
        })
        .map_err(|e| e.to_string())?;
    st.job = Some(Job { rx, after, label, started: Instant::now() });
    Ok(())
}

/// Ação da bridge que cria um asset de áudio (TTS) rodada numa cópia do documento; o asset volta como `CreateNode`.
/// `pad` = (silêncio antes, silêncio depois) em segundos e `post` = passos depois da síntese (altura, ganho), aplicados
/// por `audio_edit` na cópia quando o motor não tem o ajuste.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_action(st: &mut AudioWsState, core: &CoreSession, action: &'static str, params: Value, pad: (f64, f64), post: Vec<aiai_audio::steps::EditStep>, label: String, after: After) -> Result<(), String> {
    if st.job.is_some() {
        return Err(crate::i18n::tr("audio.job_busy").into());
    }
    let mut copy = CoreSession::new(core.project.clone());
    copy.path = core.path.clone();
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("aiai-audio-job".into())
        .spawn(move || {
            let r = (|| -> Result<JobOut, String> {
                let v = actions::run(&mut copy, action, params)?;
                let mut asset = v.get("asset").and_then(|a| serde_json::from_value::<Id>(a.clone()).ok()).ok_or("ação sem asset")?;
                // pós-processo (altura/pausas onde o motor não tem o ajuste): um audio_edit na cópia
                let mut post = post;
                if pad.0 > 0.0 || pad.1 > 0.0 {
                    let dur = aiai_tools::audio::asset_audio(&copy.project, &copy.blobs, &asset).map_err(|e| e.to_string())?.duration_s();
                    let mut pads = Vec::new();
                    if pad.1 > 0.0 {
                        pads.push(aiai_audio::steps::EditStep::InsertSilence { at_s: (dur - 1e-4).max(0.0), duration_s: pad.1 });
                    }
                    if pad.0 > 0.0 {
                        pads.push(aiai_audio::steps::EditStep::InsertSilence { at_s: 0.0, duration_s: pad.0 });
                    }
                    pads.extend(post);
                    post = pads;
                }
                if !post.is_empty() {
                    let req = aiai_tools::audio_edit::AudioEditRequest { asset: asset.clone(), steps: post, name: Some(node_name(&copy.project, &asset)), track: aiai_core::AudioTrack::Dialogue, bits: aiai_tools::audio::WavBits::Pcm24, place: Default::default() };
                    let out = aiai_tools::audio_edit::audio_edit(&copy.project, &mut copy.ids, &mut copy.blobs, &req).map_err(|e| e.to_string())?;
                    let cmd = out.clip.batch("pós");
                    let c = serde_json::to_value(&cmd).map_err(|e| e.to_string())?;
                    actions::run(&mut copy, "APPLY_COMMAND", json!({ "command": c }))?;
                    asset = out.clip.asset;
                }
                let n = copy.project.node(&asset).ok_or("asset sumiu")?.clone();
                let blob = match &n.data {
                    NodeKind::Asset(a) => a.blob.as_ref().and_then(|h| copy.blobs.get(h)),
                    _ => None,
                };
                let dur = blob.as_ref().and_then(|b| aiai_audio::wav::read_wav(b).ok()).map(|b| b.duration_s()).unwrap_or(0.0);
                let cmd = Command::CreateNode { id: Some(asset.clone()), parent: None, index: None, name: n.name.clone(), data: n.data.clone() };
                Ok(JobOut { commands: vec![cmd], asset, blobs: blob.into_iter().collect(), duration_s: dur })
            })();
            let _ = tx.send(r);
        })
        .map_err(|e| e.to_string())?;
    st.job = Some(Job { rx, after, label, started: Instant::now() });
    Ok(())
}

/// Aplica o resultado no documento real: blobs, asset novo e (se for o caso) o clipe apontando para ele — um passo só.
pub(crate) fn apply_job(core: &mut CoreSession, out: &JobOut, after: &After, label: &str) -> Result<Value, String> {
    for b in &out.blobs {
        core.blobs.put(b.to_vec());
    }
    let mut cmds = out.commands.clone();
    if let After::Retarget { cut, placement, trimmed } = after {
        let (_, mut list) = cut_audio(&core.project, cut);
        if let Some(a) = list.iter_mut().find(|a| &a.id == placement) {
            a.asset = out.asset.clone();
            let len = out.duration_s as f32;
            if *trimmed || a.trim_start >= len {
                a.trim_start = 0.0;
                a.duration = None;
            } else if let Some(d) = a.duration {
                a.duration = Some(d.min(len - a.trim_start).max(0.05));
            }
            cmds.push(Command::SetProps { id: cut.clone(), name: None, data: json!({ "audio": list }) });
        }
    }
    let cmd = Command::Batch { commands: cmds, label: Some(label.to_owned()) };
    let c = serde_json::to_value(&cmd).map_err(|e| e.to_string())?;
    actions::run(core, "APPLY_COMMAND", json!({ "command": c }))
}

/// Recolhe o trabalho que terminou e aplica (chamado a cada quadro).
fn poll_job(core: &mut CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    let Some(job) = &ed.audio_ws.job else { return };
    match job.rx.try_recv() {
        Ok(r) => {
            let job = ed.audio_ws.job.take().unwrap_or_else(|| unreachable!());
            let r = r.and_then(|out| apply_job(core, &out, &job.after, &job.label).map(|_| out));
            if let Some(out) = actions::notify_err(ed, r) {
                match &job.after {
                    After::Open => ed.audio_ws.wave.open_edited(out.asset.clone()),
                    After::Retarget { .. } => {}
                    After::Take { line, label } => ed.audio_ws.tts.add_take(*line, label.clone(), out.asset.clone(), out.duration_s),
                }
                ed.notify(format!("{}: {}", job.label, crate::i18n::tr("audio.job_done")));
            }
        }
        Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(120)),
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            ed.audio_ws.job = None;
            ed.notify(crate::i18n::tr("audio.job_failed"));
        }
    }
}

// ------------------------------------------------------------------------------------------------ quadro

/// A cada quadro (antes dos atalhos globais): recolhe trabalhos, avança o player, faz o cabeçote do corte seguir a
/// reprodução da multipista e guarda as teclas do editor de forma de onda quando o ponteiro está nele.
pub(crate) fn tick(core: &mut CoreSession, ed: &mut Editor, ctx: &egui::Context) {
    poll_job(core, ed, ctx);
    tts::pump(core, ed);
    // um transporte por vez: a linha do tempo começou a tocar, a reprodução do workspace para
    if ed.playback.playing && ed.audio_ws.player.playing().is_some() {
        ed.audio_ws.player.stop();
    }
    ed.audio_ws.player.tick();
    multitrack::follow_playback(core, ed);
    if ed.audio_ws.player.playing().is_some() {
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    wave::collect_keys(ed, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;
    use aiai_audio::steps::EditStep;

    /// Projeto com um corte e um clipe de áudio (seno de 1 s) colocado nele.
    pub(crate) fn doc_with_clip() -> (CoreSession, NodeId, Id, Id) {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = crate::app::commands::quick_start(&mut core, 48, None).unwrap();
        let n = 48_000usize;
        let buf = AudioBuf::mono(48_000, (0..n).map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin()).collect());
        let bytes = aiai_audio::wav::write_wav(&buf, aiai_audio::wav::WavFormat::Pcm16).unwrap();
        let h = core.blobs.put(bytes);
        let asset = Id("aud-1".into());
        let cmd = Command::CreateNode { id: Some(asset.clone()), parent: None, index: None, name: "Seno".into(), data: NodeKind::Asset(aiai_core::AssetData { asset: AssetType::AudioClip, blob: Some(h), params: json!({}), source: "teste".into() }) };
        actions::run(&mut core, "APPLY_COMMAND", json!({ "command": serde_json::to_value(&cmd).unwrap() })).unwrap();
        let pl = Id("pl-1".into());
        let a = AudioPlacement { id: pl.clone(), asset: asset.clone(), track: aiai_core::AudioTrack::Sfx, start_frame: 2, trim_start: 0.0, duration: None, gain_db: 0.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0, character: None };
        crate::app::panels::audio::set_audio(&mut core, &cut, &[a]).unwrap();
        (core, cut, asset, pl)
    }

    pub(crate) fn wait_job(st: &mut AudioWsState) -> JobOut {
        let job = st.job.take().expect("trabalho");
        job.rx.recv_timeout(Duration::from_secs(30)).expect("terminou").expect("sem erro")
    }

    #[test]
    fn an_edit_makes_a_new_asset_and_retargets_the_clip_in_one_undo_step() {
        let (mut core, cut, asset, pl) = doc_with_clip();
        let mut st = AudioWsState::default();
        let after = After::Retarget { cut: cut.clone(), placement: pl.clone(), trimmed: false };
        spawn_edit(&mut st, &core, &asset, vec![EditStep::Delete { start_s: 0.25, end_s: 0.5 }], "apagar".into(), after.clone()).unwrap();
        assert!(spawn_edit(&mut st, &core, &asset, vec![EditStep::Mono], "x".into(), After::Open).is_err(), "um trabalho por vez");
        let out = wait_job(&mut st);
        assert!((out.duration_s - 0.75).abs() < 0.01, "{}", out.duration_s);
        let undo_before = core.log().len();
        apply_job(&mut core, &out, &after, "apagar").unwrap();
        assert_eq!(core.log().len(), undo_before + 1, "um passo");
        let (_, list) = cut_audio(&core.project, &cut);
        assert_eq!(list[0].asset, out.asset);
        // a fonte continua lá, intacta, e o novo sabe de onde veio
        assert!(core.project.node(&asset).is_some());
        assert_eq!(asset_params(&core.project, &out.asset).get("edited_from").and_then(|v| v.as_str()), Some(asset.0.as_str()));
        let b = aiai_tools::audio::asset_audio(&core.project, &core.blobs, &out.asset).unwrap();
        assert!((b.duration_s() - 0.75).abs() < 0.01);
        core.undo().unwrap();
        let (_, list) = cut_audio(&core.project, &cut);
        assert_eq!(list[0].asset, asset, "desfazer volta o clipe para a fonte");
        assert!(core.project.node(&out.asset).is_none(), "e tira o asset novo");
    }

    #[test]
    fn loading_keeps_one_pyramid_per_blob() {
        let (core, _cut, asset, _pl) = doc_with_clip();
        let mut st = AudioWsState::default();
        let a = st.load(&core, &asset).unwrap();
        let b = st.load(&core, &asset).unwrap();
        assert!(Arc::ptr_eq(&a.pyr, &b.pyr));
        assert_eq!(a.buf.frames(), 48_000);
        assert!(st.load(&core, &Id("nada".into())).is_none());
        assert_eq!(audio_assets(&core.project).len(), 1);
        assert_eq!(fmt_time(61.5), "1:01.500");
    }
}
