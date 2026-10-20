//! Orçamento de VRAM no app (ver `crate::gpu_mem`): a medida de cada quadro, o despejo de texturas sob pressão e de
//! painéis que não aparecem, a saída limpa quando a GPU se perde e o aviso na abertura seguinte.

use super::StudioApp;
use crate::gpu_mem::{self, Pressure, MB};
use crate::i18n::tr;
use std::time::{Duration, Instant};

/// Painel que não aparece há este tempo solta as texturas que só ele mostra.
pub(crate) const HIDDEN_RELEASE: Duration = Duration::from_secs(3);

impl StudioApp {
    /// Fim de cada quadro, antes da pintura: mede, decide a pressão e solta o que não cabe. Como roda antes de o egui
    /// enviar as texturas novas à placa, uma textura grande criada neste quadro já entra na conta (guarda preventiva).
    pub(crate) fn gpu_tick(&mut self, ctx: &egui::Context) {
        gpu_mem::set_project(self.core.path.as_deref());
        self.gpu.budget_pref_mb = self.prefs.gpu_budget_mb;
        let before = self.gpu.snap.egui_bytes;
        let p = self.gpu.tick(ctx, super::model3d::gpu_cached_bytes());
        let level = self.gpu_evict(p);
        if let Some(level) = level {
            // mede de novo: o que foi solto sai do total já neste quadro
            self.gpu.tick(ctx, super::model3d::gpu_cached_bytes());
            let freed = before.max(self.gpu.snap.egui_bytes) - self.gpu.snap.egui_bytes;
            self.gpu.note_eviction(level, freed);
        }
        if p != self.gpu_pressure_seen {
            if p > self.gpu_pressure_seen {
                let msg = tr(if p == Pressure::Critical { "gpu.notice.critical" } else { "gpu.notice.high" })
                    .replace("{used}", &(self.gpu.snap.used / MB).to_string())
                    .replace("{limit}", &(self.gpu.snap.limit / MB).to_string());
                self.notice = Some((msg, Instant::now()));
            }
            self.gpu_pressure_seen = p;
        }
    }

    /// Solta texturas conforme a pressão. Devolve o nível quando algo foi pedido (para a telemetria medir o efeito).
    fn gpu_evict(&mut self, p: Pressure) -> Option<Pressure> {
        let mut acted = false;
        // painéis que sumiram (aba trocada, workspace trocado): soltam o que só eles mostram
        acted |= super::gpu_owners::release_hidden(self, p);
        if p >= Pressure::High {
            if super::model3d::gpu_cached_bytes() > 0 {
                super::model3d::gpu_release_cached();
                acted = true;
            }
        }
        acted.then_some(p)
    }

    /// O dispositivo da GPU se perdeu (falta de memória de vídeo ou queda do driver). A pintura do egui-wgpu entraria em
    /// pânico no próximo quadro e o eframe 0.36 não deixa recriar o dispositivo; então, antes de pintar: grava o que só
    /// existe em memória, solta a trava do projeto, deixa o marcador que a próxima abertura explica, reabre o app no
    /// mesmo projeto (execução interativa) e sai.
    pub(crate) fn gpu_lost_exit(&mut self, reason: &str) -> ! {
        tracing::error!("GPU perdida ({reason}): gravando o estado e reabrindo");
        self.prefs_dirty = true;
        self.persist();
        if self.layouts_dirty_at.is_some() {
            self.save_layouts();
        }
        // projeto sem pasta com alterações: cópia de recuperação agora (a tela inicial a oferece ao reabrir)
        if self.core.path.is_none() && self.doc_dirty() {
            if let Some(dir) = self.autosave.dir.clone() {
                match aiai_core::persist::snapshot(&self.core.project, &self.core.blobs, &dir) {
                    Ok(()) => tracing::info!("cópia de recuperação gravada em {}", dir.display()),
                    Err(e) => tracing::error!("cópia de recuperação falhou: {e}"),
                }
            }
        }
        // o diário (wal.ndjson) já foi gravado a cada comando; a trava sai para a reabertura entrar sem perguntar
        self.core.release_lock();
        self.bridge = None;
        gpu_mem::record_loss_and_relaunch("frame");
        std::process::exit(3)
    }

    /// Na abertura: marcadores de execuções que perderam a GPU viram um aviso na janela "Avisos ao abrir".
    pub(crate) fn gpu_startup_markers(&mut self) {
        let consume = !self.cli.no_session && !self.cli.bridge && self.cli.screenshot.is_none();
        let markers = gpu_mem::take_markers(self.cli.wait_pid, consume);
        if markers.is_empty() {
            return;
        }
        let mut lines = Vec::new();
        for m in &markers {
            lines.push(
                tr("gpu.lost.line")
                    .replace("{reason}", &m.reason)
                    .replace("{used}", &m.used_mb.to_string())
                    .replace("{limit}", &m.limit_mb.to_string())
                    .replace("{instances}", &m.instances.to_string()),
            );
        }
        lines.push(tr("gpu.lost.saved").to_owned());
        lines.push(tr("gpu.lost.reduce").to_owned());
        match &mut self.start.warn {
            Some(w) => w.lines.extend(lines),
            None => self.start.warn = Some(super::doc::OpenWarnings { lines, ..Default::default() }),
        }
    }

    /// Preferências > Desempenho: orçamento de VRAM e o uso medido agora (por subsistema, serviço 3D, alocador do wgpu e
    /// o que o sistema conta para o processo).
    pub(crate) fn prefs_vram(&mut self, ui: &mut egui::Ui, t: crate::theme::Tokens) {
        use egui::RichText;
        let dim = |s: String| RichText::new(s).size(11.5).color(t.text_dim);
        ui.add_space(12.0);
        ui.label(tr("prefs.vram"));
        let s = self.gpu.snap.clone();
        let auto = gpu_mem::auto_budget(s.os.map_or(0, |o| o.dedicated), s.instances);
        let label = |mb: u32| if mb == 0 { tr("prefs.vram.auto").replace("{mb}", &(auto / MB).to_string()) } else { format!("{mb} MB") };
        let mut b = self.prefs.gpu_budget_mb;
        let r = egui::ComboBox::from_id_salt("prefs_vram_budget").width(260.0).selected_text(label(b)).show_ui(ui, |ui| {
            for mb in gpu_mem::BUDGET_CHOICES_MB {
                ui.selectable_value(&mut b, mb, label(mb));
            }
        });
        self.rects.note("prefs:vram_budget", r.response.rect);
        if b != self.prefs.gpu_budget_mb {
            self.prefs.gpu_budget_mb = b;
            self.mark_dirty();
        }
        ui.label(dim(
            tr("prefs.vram.used")
                .replace("{used}", &(s.used / MB).to_string())
                .replace("{limit}", &(s.limit / MB).to_string())
                .replace("{pressure}", tr(&format!("gpu.pressure.{}", s.pressure.key())))
                .replace("{instances}", &s.instances.to_string()),
        ));
        if let Some(o) = s.os {
            ui.label(dim(tr("prefs.vram.os").replace("{dedicated}", &(o.dedicated / MB).to_string()).replace("{budget}", &(o.local_budget / MB).to_string()).replace("{shared}", &(o.nonlocal_usage / MB).to_string())));
        }
        let mb1 = |b: u64| format!("{:.1}", b as f64 / MB as f64);
        egui::Grid::new("prefs_vram_table").num_columns(3).spacing([16.0, 2.0]).show(ui, |ui| {
            for u in s.subs.iter().filter(|u| u.bytes > 0) {
                ui.label(dim(tr(&format!("gpu.sub.{}", u.key)).to_owned()));
                ui.label(dim(format!("{} MB", mb1(u.bytes))));
                ui.label(dim(format!("{} {}", u.count, tr("prefs.vram.textures"))));
                ui.end_row();
            }
            ui.label(dim(tr("gpu.sub.service3d").to_owned()));
            ui.label(dim(format!("{} MB", mb1(s.native_3d))));
            ui.label(dim(String::new()));
            ui.end_row();
            if let (Some(a), Some(r)) = (s.wgpu_allocated, s.wgpu_reserved) {
                ui.label(dim(tr("gpu.sub.wgpu").to_owned()));
                ui.label(dim(format!("{} MB", mb1(a))));
                ui.label(dim(format!("{} MB {}", mb1(r), tr("prefs.vram.reserved"))));
                ui.end_row();
            }
        });
        ui.label(dim(tr("prefs.vram.help").to_owned()));
    }

    /// `UI_ACTION gpu`: `state` (o mesmo de `ui_state.gpu`), `simulate_usage {mb}` (a pressão passa a ver este uso;
    /// 0 desliga), `simulate_oom` (o caminho do erro de falta de memória do wgpu), `simulate_lost` (o caminho do
    /// dispositivo perdido: o app grava, reabre se for interativo e sai no quadro seguinte), `budget {mb}` (orçamento
    /// desta execução; 0 = automático).
    pub(crate) fn gpu_bridge(&mut self, op: &str, mb: Option<u64>) -> Result<serde_json::Value, aiai_bridge::protocol::BridgeError> {
        use aiai_bridge::protocol::BridgeError;
        match op {
            "state" => {}
            "simulate_usage" => gpu_mem::simulate_usage(mb.unwrap_or(0) * MB),
            "simulate_oom" => gpu_mem::simulate_oom(),
            "simulate_lost" => gpu_mem::simulate_lost("UI_ACTION gpu"),
            "budget" => self.prefs.gpu_budget_mb = mb.unwrap_or(0).min(u32::MAX as u64) as u32,
            other => return Err(BridgeError::invalid_params(format!("gpu: op desconhecida {other:?} (state|simulate_usage|simulate_oom|simulate_lost|budget)"))),
        }
        Ok(serde_json::json!({ "op": op, "gpu": self.gpu.to_json() }))
    }
}
