//! Workspace Personagens: painel Personagens (lista com a paleta de settei, a altura em cabeças e a voz de cada um),
//! Folha de modelo (o material aberto da pasta do personagem, renderizado em segundo plano; na folha de modelo, com as
//! linhas-guia de proporção e o nome de cada vista) e, no Propriedades, a ficha do personagem em abas: Ficha (nome,
//! perfil, idade, altura em cabeças e a geração da folha por `MODEL_SHEET`), Paleta de settei, Consistência
//! (`CONSISTENCY`: proporções de um desenho contra a folha), Voz (`VOICE_PROFILE`) e Material (o material aberto). A
//! pasta com todos os materiais fica em `char_folder`.
//!
//! Tudo passa pelas ações da bridge ou por `SetProps` (o mesmo caminho do MCP): um ajuste = um passo de desfazer.

use super::commit_on_release;
use super::edit::EditPreview;
use crate::app::actions;
use crate::app::commands::{self as cmds, NodeId};
use crate::app::ui_kit::{Env, body_ui, dim_label, rgba_to_color32, section};
use crate::i18n::tr;
use aiai_core::{CharacterData, Command, CoreSession, Id, NodeKind, Project};
use egui::{Color32, FontId, Id as EId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};

/// Vistas da folha de modelo (graus de rotação do corpo) com a chave do nome.
pub(crate) const VIEWS: [(f32, &str); 5] = [(0.0, "char.view.front"), (45.0, "char.view.three_quarter"), (90.0, "char.view.side"), (135.0, "char.view.three_quarter_back"), (180.0, "char.view.back")];

/// Medidas de um desenho para o medidor de consistência (px do desenho; as opcionais só contam se ligadas).
#[derive(Clone)]
pub(crate) struct ConsForm {
    pub head_top: f32,
    pub chin: f32,
    pub eyes: Option<f32>,
    pub shoulders: Option<f32>,
    pub feet: Option<f32>,
    /// Medida esperando um clique no canvas (0 topo, 1 queixo, 2 olhos, 3 ombros, 4 pés).
    pub pick: Option<usize>,
}

impl Default for ConsForm {
    fn default() -> Self {
        ConsForm { head_top: 100.0, chin: 220.0, eyes: Some(170.0), shoulders: None, feet: None, pick: None }
    }
}

/// Nome e cor de cada linha da conferência no canvas.
const CONS_LINES: [(&str, Color32); 5] = [
    ("char.m.head_top", Color32::from_rgb(90, 200, 255)),
    ("char.m.chin", Color32::from_rgb(255, 170, 60)),
    ("char.m.eyes", Color32::from_rgb(120, 230, 120)),
    ("char.m.shoulders", Color32::from_rgb(230, 120, 230)),
    ("char.m.feet", Color32::from_rgb(255, 230, 90)),
];

impl ConsForm {
    fn values(&self) -> [Option<f32>; 5] {
        [Some(self.head_top), Some(self.chin), self.eyes, self.shoulders, self.feet]
    }

    fn set(&mut self, i: usize, y: f32) {
        match i {
            0 => self.head_top = y,
            1 => self.chin = y,
            2 => self.eyes = Some(y),
            3 => self.shoulders = Some(y),
            _ => self.feet = Some(y),
        }
    }
}

/// A conferência de consistência está na tela (Personagens, aba Consistência): as linhas aparecem no canvas.
fn cons_on(env: &Env) -> bool {
    env.ws == crate::workspace::WorkspaceId::Personagens && env.ed.chars.tab == CharTab::Consistency
}

/// Clique no canvas com uma medida armada ("Medir" ao lado do campo): o y do clique vira o valor (auditoria 2.22, em vez
/// de digitar pixels). Devolve se consumiu o clique.
pub(crate) fn cons_pick(env: &mut Env, y: f32) -> bool {
    if !cons_on(env) {
        env.ed.chars.cons.pick = None;
        return false;
    }
    let Some(i) = env.ed.chars.cons.pick.take() else { return false };
    env.ed.chars.cons.set(i, y.round());
    true
}

/// Linhas horizontais das medidas sobre o canvas, com o nome, enquanto a aba Consistência está aberta.
pub(crate) fn paint_cons_guides(env: &Env, painter: &egui::Painter, xf: &crate::app::editor::Xf, canvas: (u32, u32)) {
    if !cons_on(env) {
        return;
    }
    let f = &env.ed.chars.cons;
    for (i, v) in f.values().into_iter().enumerate() {
        let Some(y) = v else { continue };
        let (key, c) = CONS_LINES[i];
        let a = xf.to_screen(egui::Vec2::new(0.0, y));
        let b = xf.to_screen(egui::Vec2::new(canvas.0 as f32, y));
        let w = if f.pick == Some(i) { 2.0 } else { 1.0 };
        painter.line_segment([a, b], Stroke::new(w + 2.0, Color32::from_black_alpha(140)));
        painter.line_segment([a, b], Stroke::new(w, c));
        let g = painter.layout_no_wrap(format!("{} {y:.0}", tr(key)), FontId::proportional(11.0), c);
        let at = a + egui::vec2(4.0, -g.size().y - 2.0);
        painter.rect_filled(Rect::from_min_size(at, g.size()).expand(2.0), 2.0, Color32::from_black_alpha(160));
        painter.galley(at, g, c);
    }
}

/// Topo e base do que o corte mostra no quadro atual (alfa > 10%): o desenho inteiro, para o topo da cabeça e os pés.
fn drawing_box(env: &mut Env) -> Option<(f32, f32)> {
    let cut = env.ed.active_cut(env.core)?;
    let opts = aiai_render::RenderOptions { camera: false, include_guides: false, include_reference: false, ..Default::default() };
    let img = aiai_render::render_frame(&env.core.project, &env.core.blobs, &cut, env.ed.frame, &opts).ok()?.image;
    let (w, h) = (img.width as usize, img.height as usize);
    let row_has = |y: usize| img.data[y * w..(y + 1) * w].iter().any(|p| p[3] > 0.1);
    let top = (0..h).find(|&y| row_has(y))?;
    let bottom = (0..h).rev().find(|&y| row_has(y))?;
    Some((top as f32, (bottom + 1) as f32))
}

/// Estado do workspace Personagens que não mora no documento.
pub(crate) struct CharState {
    pub sel: Option<NodeId>,
    pub sheet: EditPreview,
    pub views: [bool; 5],
    pub figure_px: f32,
    /// Mostrar as camadas de guia da folha (grade e manequim).
    pub guides: bool,
    pub cons: ConsForm,
    /// Última medição: (personagem, change_counter, resultado).
    pub report: Option<(NodeId, u64, Result<Value, String>)>,
    /// Diálogo "Novo personagem" aberto.
    pub new_form: Option<NewCharForm>,
    /// Aba da ficha (Propriedades).
    pub tab: CharTab,
    /// Material aberto no canvas (id na pasta do personagem); vazio = a folha de modelo.
    pub open: Option<Id>,
    /// Renomeando na árvore: (material, texto em edição).
    pub rename: Option<(Id, String)>,
    /// Diálogo de arquivo da importação de imagem em andamento.
    pub import_job: Option<crossbeam_channel::Receiver<Option<std::path::PathBuf>>>,
    /// Estamos no workspace (para `tick` saber quando saiu).
    pub entered: bool,
    /// Corte ativo de antes de entrar (volta a ele ao sair) e o corte que o workspace colocou no lugar.
    pub return_cut: Option<NodeId>,
    pub pushed: Option<NodeId>,
    /// Ao sair, manter o corte do material (aberto na Animação).
    pub keep_cut: bool,
    /// Câmera e guias do canvas antes de entrar (voltam ao sair).
    pub saved_view: Option<(bool, bool)>,
}

/// Abas da ficha do personagem (Propriedades do workspace Personagens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CharTab {
    #[default]
    Settei,
    Palette,
    Consistency,
    Voice,
    Material,
}

/// Diálogo de personagem novo: nada é criado até "Criar"; a folha de modelo já sai junto se marcada.
#[derive(Clone)]
pub(crate) struct NewCharForm {
    pub name: String,
    pub heads: f32,
    pub views: [bool; 5],
    pub sheet: bool,
}

impl Default for CharState {
    fn default() -> Self {
        CharState { sel: None, sheet: EditPreview::default(), views: [true; 5], figure_px: 800.0, guides: true, cons: ConsForm::default(), report: None, new_form: None, tab: CharTab::default(), open: None, rename: None, import_job: None, entered: false, return_cut: None, pushed: None, keep_cut: false, saved_view: None }
    }
}

pub(crate) fn char_data<'a>(p: &'a Project, id: &NodeId) -> Option<&'a CharacterData> {
    match &p.node(id)?.data {
        NodeKind::Character(c) => Some(c),
        _ => None,
    }
}

/// Corte da folha de modelo do personagem: o registrado nas medidas (`sheet_cut`), senão o primeiro corte filho.
pub(crate) fn sheet_cut(p: &Project, id: &NodeId) -> Option<NodeId> {
    let c = char_data(p, id)?;
    if let Some(s) = c.measurements.get("sheet_cut").and_then(Value::as_str) {
        let sid = Id(s.to_owned());
        if cmds::is_cut(p, &sid) {
            return Some(sid);
        }
    }
    p.node(id)?.children.iter().find(|k| cmds::is_cut(p, k)).cloned()
}

/// Escolhe/corrige o personagem selecionado (o primeiro, se o atual sumiu).
pub(crate) fn sync_sel(env: &mut Env) -> Option<NodeId> {
    let all = &env.core.project.roots.characters;
    let st = &mut env.ed.chars;
    if st.sel.as_ref().is_none_or(|s| !all.contains(s)) {
        st.sel = all.first().cloned();
    }
    st.sel.clone()
}

fn select(env: &mut Env, id: NodeId) {
    // a paleta de settei do personagem vira a paleta ativa (painel Cores ao lado)
    if let Some(pal) = char_data(&env.core.project, &id).and_then(|c| c.palette.clone()) {
        env.ed.palette = Some(pal);
    }
    env.ed.chars.sel = Some(id);
}

fn set_char(core: &mut CoreSession, id: &NodeId, patch: Value) -> Result<(), String> {
    cmds::run(core, Command::SetProps { id: id.clone(), name: None, data: patch }).map(|_| ())
}

/// Personagem novo ("Personagem N") com a altura padrão de 6,5 cabeças (testes; a interface passa pelo diálogo).
#[cfg(test)]
pub(crate) fn add_character(core: &mut CoreSession) -> Result<NodeId, String> {
    let n = core.project.roots.characters.len() + 1;
    add_character_named(core, &format!("{} {n}", tr("char.name")), 6.5)
}

pub(crate) fn add_character_named(core: &mut CoreSession, name: &str, heads: f32) -> Result<NodeId, String> {
    let id = core.ids.next();
    let data = CharacterData { heads_tall: heads, ..Default::default() };
    cmds::run(core, Command::CreateNode { id: Some(id.clone()), parent: None, index: None, name: name.to_owned(), data: NodeKind::Character(data) })?;
    Ok(id)
}

/// Gera a folha de modelo do personagem (vistas marcadas, altura em cabeças).
fn make_sheet(env: &mut Env, ch: &NodeId, heads: f32, views_on: [bool; 5]) {
    let views: Vec<f32> = VIEWS.iter().zip(views_on).filter(|(_, on)| *on).map(|((d, _), _)| *d).collect();
    if views.is_empty() {
        return;
    }
    let name = format!("{} {}", tr("panel.sheet.title"), env.core.project.node(ch).map(|n| n.name.clone()).unwrap_or_default());
    let r = actions::run(env.core, "MODEL_SHEET", json!({ "character": ch, "name": name, "heads_tall": heads, "figure_px": env.ed.chars.figure_px, "views": views }));
    open_created(env, r);
}

/// Resposta de `MODEL_SHEET`: erro vira aviso; a folha nova abre no canvas.
fn open_created(env: &mut Env, r: Result<Value, String>) {
    if let Some(v) = actions::notify_err(env.ed, r) {
        if let Some(m) = v.get("material").and_then(Value::as_str) {
            env.ed.chars.open = Some(Id(m.to_owned()));
        }
    }
}

/// Janela "Novo personagem" (nome, altura em cabeças, vistas da folha de modelo).
fn new_character_window(env: &mut Env, ctx: &egui::Context) {
    let Some(mut f) = env.ed.chars.new_form.clone() else { return };
    let mut open = true;
    let (mut create, mut cancel) = (false, false);
    let r = egui::Window::new(tr("char.new_title"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::Grid::new("new_char").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(tr("char.f.name"));
                let r = ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(220.0));
                env.note("char_new:name", r.rect);
                ui.end_row();
                ui.label(tr("char.f.heads"));
                let r = ui.add(egui::DragValue::new(&mut f.heads).range(2.0..=10.0).speed(0.1).max_decimals(1));
                env.note("char_new:heads", r.rect);
                ui.end_row();
            });
            ui.add_space(6.0);
            let r = ui.checkbox(&mut f.sheet, tr("char.new_with_sheet"));
            env.note("char_new:sheet", r.rect);
            ui.add_enabled_ui(f.sheet, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, (_, key)) in VIEWS.iter().enumerate() {
                        ui.checkbox(&mut f.views[i], tr(key));
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ok = !f.name.trim().is_empty() && (!f.sheet || f.views.iter().any(|v| *v));
                let c = ui.add_enabled(ok, egui::Button::new(tr("char.create")));
                env.note("char_new:create", c.rect);
                create = c.clicked();
                let x = ui.button(tr("confirm.cancel"));
                env.note("char_new:cancel", x.rect);
                cancel = x.clicked();
            });
        });
    if let Some(r) = r {
        env.note("dialog:char_new", r.response.rect);
    }
    if create {
        match add_character_named(env.core, f.name.trim(), f.heads) {
            Ok(id) => {
                select(env, id.clone());
                if f.sheet {
                    make_sheet(env, &id, f.heads, f.views);
                }
            }
            Err(e) => env.ed.notify(e),
        }
        env.ed.chars.new_form = None;
    } else if cancel || !open {
        env.ed.chars.new_form = None;
    } else {
        env.ed.chars.new_form = Some(f);
    }
}

/// Texto curto da voz: "VOICEVOX: nome (#8)" ou "sem voz".
fn voice_summary(c: &CharacterData) -> String {
    if c.voice.is_null() {
        return tr("char.no_voice").to_owned();
    }
    let engine = c.voice.get("engine").and_then(Value::as_str).unwrap_or("voicevox").to_uppercase();
    let speaker = c.voice.get("speaker").and_then(Value::as_u64).unwrap_or(0);
    match c.voice.get("speaker_name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        Some(n) => format!("{engine}: {n} (#{speaker})"),
        None => format!("{engine} #{speaker}"),
    }
}

fn palette_colors(p: &Project, c: &CharacterData) -> Vec<Color32> {
    c.palette.as_ref().and_then(|pal| cmds::palette_data(p, pal)).map(|pd| pd.colors.iter().map(|nc| rgba_to_color32(nc.rgba)).collect()).unwrap_or_default()
}

// ------------------------------------------------------------------------------------------------ lista

pub(crate) fn list(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let sel = sync_sel(env);
    let b = ui.button(tr("char.new"));
    env.note("char:new", b.rect);
    if b.clicked() && env.ed.chars.new_form.is_none() {
        let n = env.core.project.roots.characters.len() + 1;
        env.ed.chars.new_form = Some(NewCharForm { name: format!("{} {n}", tr("char.name")), heads: 6.5, views: env.ed.chars.views, sheet: true });
    }
    let ctx = ui.ctx().clone();
    new_character_window(env, &ctx);
    ui.add_space(4.0);
    let ids = env.core.project.roots.characters.clone();
    if ids.is_empty() {
        ui.label(egui::RichText::new(tr("panel.characters.empty")).color(t.text_dim));
        return;
    }
    let mut delete = None;
    egui::ScrollArea::vertical().id_salt("char_list").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, id) in ids.iter().enumerate() {
            let p = &env.core.project;
            let (Some(n), Some(c)) = (p.node(id), char_data(p, id)) else { continue };
            let (name, heads, voice, colors, n_mat) = (n.name.clone(), c.heads_tall, voice_summary(c), palette_colors(p, c), aiai_tools::char_material::materials(p, id).len());
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 56.0), Sense::click());
            env.note(format!("char:row:{i}"), rect);
            let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            if sel.as_ref() == Some(id) {
                painter.rect_filled(rect, 4.0, t.accent_dim);
            } else if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            // paleta de settei: até 8 amostras em duas linhas (moldura vazia sem paleta)
            let sw = Rect::from_min_size(rect.left_top() + egui::vec2(5.0, 6.0), egui::vec2(44.0, 44.0));
            painter.rect_stroke(sw, 3.0, Stroke::new(1.0, t.border), StrokeKind::Inside);
            for (k, col) in colors.iter().take(8).enumerate() {
                let (cx, cy) = ((k % 4) as f32, (k / 4) as f32);
                let cell = Rect::from_min_size(sw.left_top() + egui::vec2(2.0 + cx * 10.0, 2.0 + cy * 20.0), egui::vec2(10.0, 20.0));
                painter.rect_filled(cell, 0.0, *col);
            }
            let x = sw.right() + 8.0;
            painter.text(egui::pos2(x, rect.top() + 5.0), egui::Align2::LEFT_TOP, &name, FontId::proportional(13.0), t.text);
            let line2 = format!("{} {}  ·  {n_mat} {}", fmt_num(heads), tr("char.heads"), tr("char.n_materials"));
            painter.text(egui::pos2(x, rect.top() + 23.0), egui::Align2::LEFT_TOP, line2, FontId::proportional(11.0), t.text_dim);
            painter.text(egui::pos2(x, rect.top() + 38.0), egui::Align2::LEFT_TOP, voice, FontId::proportional(11.0), t.text_dim);
            if resp.clicked() {
                select(env, id.clone());
            }
            resp.context_menu(|ui| {
                if ui.button(tr("char.delete")).clicked() {
                    delete = Some(id.clone());
                    ui.close();
                }
            });
        }
    });
    if let Some(id) = delete {
        let r = cmds::delete_node(env.core, &id);
        actions::notify_err(env.ed, r);
    }
}

/// Número com uma casa decimal, na vírgula do português (2,8).
fn fmt_dec(v: f32) -> String {
    let s = format!("{v:.1}");
    if crate::i18n::lang() == crate::i18n::Lang::Pt { s.replace('.', ",") } else { s }
}

/// Número com uma casa, sem ",0" (6,5 → "6,5"; 7 → "7"), na vírgula do português.
fn fmt_num(v: f32) -> String {
    let s = if (v - v.round()).abs() < 0.05 { format!("{}", v.round() as i64) } else { format!("{v:.1}") };
    if crate::i18n::lang() == crate::i18n::Lang::Pt { s.replace('.', ",") } else { s }
}

// ------------------------------------------------------------------------------------------------ folha de modelo

pub(crate) fn sheet(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(ch) = sync_sel(env) else {
        ui.label(egui::RichText::new(tr("panel.sheet.empty")).color(t.text_dim));
        return;
    };
    let Some(cut) = super::char_folder::open_cut(env, &ch) else {
        // sem folha: gerar aqui mesmo, com as vistas marcadas e a altura do personagem
        ui.label(egui::RichText::new(tr("char.sheet_none")).color(t.text_dim));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (i, (_, key)) in VIEWS.iter().enumerate() {
                ui.checkbox(&mut env.ed.chars.views[i], tr(key));
            }
        });
        let heads = char_data(&env.core.project, &ch).map(|c| c.heads_tall).unwrap_or(6.5);
        let views = env.ed.chars.views;
        let b = ui.add_enabled(views.iter().any(|v| *v), egui::Button::new(tr("char.sheet_new")));
        env.note("char:sheet_new_here", b.rect);
        if b.clicked() {
            make_sheet(env, &ch, heads, views);
        }
        return;
    };
    ui.horizontal(|ui| {
        let b = ui.button(tr("char.open_sheet"));
        env.note("char:open_sheet", b.rect);
        if b.on_hover_text(tr("char.open_sheet.tip")).clicked() {
            let list = super::char_folder::list_of(&env.core.project, &ch);
            if let Some(m) = list.iter().find(|m| m.content.cut() == &cut) {
                super::char_folder::open_in_animation(env, &list, &m.id);
            }
        }
        let g = ui.checkbox(&mut env.ed.chars.guides, tr("char.guides"));
        env.note("char:guides", g.rect);
        let name = env.core.project.node(&cut).map(|n| n.name.clone()).unwrap_or_default();
        dim_label(ui, &t, &name);
    });
    let guides = env.ed.chars.guides;
    let ctx = ui.ctx().clone();
    let core = &*env.core;
    // o canvas inteiro da folha (todas as vistas), não o quadro da câmera do projeto
    env.ed.chars.sheet.request(&ctx, (core.change_counter, format!("{}:{guides}", cut.0), 0), || {
        let (project, blobs, c2) = (core.project.clone(), core.blobs.clone(), cut.clone());
        move || aiai_render::render_frame(&project, &blobs, &c2, 0, &aiai_render::RenderOptions { background: Some([1.0, 1.0, 1.0]), include_guides: guides, include_reference: true, camera: false, ..Default::default() }).map(|o| o.image).map_err(|e| e.to_string())
    });
    let (w, h) = cmds::canvas_size(&env.core.project, &cut);
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let area = ui.available_rect_before_wrap();
    let label_h = 18.0;
    let s = (area.width() / w).min(((area.height() - label_h).max(10.0)) / h);
    let img = Rect::from_min_size(egui::pos2(area.center().x - w * s / 2.0, area.top() + 2.0), egui::vec2(w * s, h * s));
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(img, 0.0, Color32::WHITE);
    if let Some(tex) = &env.ed.chars.sheet.texture {
        painter.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    env.note("char:sheet", img);
    // as linhas de proporção e os nomes das vistas só valem na folha de modelo principal
    let is_model = sheet_cut(&env.core.project, &ch).as_ref() == Some(&cut);
    let m = if is_model { char_data(&env.core.project, &ch).map(|c| c.measurements.clone()).unwrap_or(Value::Null) } else { Value::Null };
    // linhas-guia de proporção (cabeça, olhos, ombros...) com o nome e a altura em cabeças na margem esquerda
    let (top, head) = (m.get("top_px").and_then(Value::as_f64).unwrap_or(0.0) as f32, m.get("head_px").and_then(Value::as_f64).unwrap_or(0.0) as f32);
    if head > 0.0 {
        for (name, heads) in aiai_tools::character::sheet_landmarks(&m) {
            let sy = img.top() + (top + heads * head) * s;
            painter.hline(img.x_range(), sy, Stroke::new(1.0, t.accent.gamma_multiply(0.45)));
            painter.text(egui::pos2(img.left() + 3.0, sy - 1.0), egui::Align2::LEFT_BOTTOM, format!("{} {}", landmark_label(&name), fmt_num(heads)), FontId::proportional(10.0), t.accent);
        }
    }
    // nome de cada vista embaixo da sua faixa
    if let (Some(slot), Some(views)) = (m.get("slot_w").and_then(Value::as_f64), m.get("views").and_then(Value::as_array)) {
        for (i, v) in views.iter().enumerate() {
            let deg = v.as_f64().unwrap_or(0.0) as f32;
            let label = VIEWS.iter().find(|(d, _)| (d - deg).abs() < 0.5).map(|(_, k)| tr(k).to_owned()).unwrap_or_else(|| format!("{deg:.0}°"));
            let cx = img.left() + (i as f32 + 0.5) * slot as f32 * s;
            painter.text(egui::pos2(cx, img.bottom() + 3.0), egui::Align2::CENTER_TOP, label, FontId::proportional(11.0), t.text_dim);
        }
    }
    if let Some(e) = &env.ed.chars.sheet.error {
        painter.text(img.center(), egui::Align2::CENTER_CENTER, e, FontId::proportional(12.0), t.text_dim);
    }
    ui.allocate_rect(area, Sense::hover());
}

// ------------------------------------------------------------------------------------------------ propriedades

pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(ch) = sync_sel(env) else {
        dim_label(ui, &t, tr("panel.characters.empty"));
        return;
    };
    let Some(c) = char_data(&env.core.project, &ch).cloned() else { return };
    let name = env.core.project.node(&ch).map(|n| n.name.clone()).unwrap_or_default();
    let w = (ui.available_width() - 4.0).max(60.0);

    // abas: Ficha, Paleta, Consistência, Voz e o material aberto
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (tab, key, name) in [
            (CharTab::Settei, "char.tab.settei", "char:tab:settei"),
            (CharTab::Palette, "char.tab.palette", "char:tab:palette"),
            (CharTab::Consistency, "char.tab.consistency", "char:tab:consistency"),
            (CharTab::Voice, "char.tab.voice", "char:tab:voice"),
            (CharTab::Material, "char.tab.material", "char:tab:material"),
        ] {
            let r = ui.selectable_label(env.ed.chars.tab == tab, tr(key));
            env.note(name, r.rect);
            if r.clicked() {
                env.ed.chars.tab = tab;
            }
        }
    });
    match env.ed.chars.tab {
        CharTab::Settei => {
            settei_tab(env, ui, &ch, &c, &name, w);
            sheet_form(env, ui, &ch, &c);
        }
        CharTab::Palette => palette_tab(env, ui, &ch, &c, &name, w),
        CharTab::Consistency => consistency(env, ui, &ch, &c),
        CharTab::Voice => voice(env, ui, &ch, &c, w),
        CharTab::Material => super::char_folder::material_tab(env, ui, &ch, w),
    }
}

fn settei_tab(env: &mut Env, ui: &mut Ui, ch: &NodeId, c: &CharacterData, name: &str, w: f32) {
    let t = env.t;
    let ch = ch.clone();
    section(ui, &t, "char.sec.settei");
    egui::Grid::new("char_props").num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("char.f.name"));
        let (r, out) = super::text_commit(ui, EId::new(("char_name", &ch.0)), &name, (w - 90.0).max(60.0));
        env.note("char:name", r.rect);
        if let Some(n) = out {
            let r = cmds::rename_node(env.core, &ch, &n);
            actions::notify_err(env.ed, r);
        }
        ui.end_row();
        dim_label(ui, &t, tr("char.f.age"));
        if let (_, Some(a)) = super::text_commit_opt(ui, EId::new(("char_age", &ch.0)), &c.age, (w - 90.0).max(60.0), "") {
            let r = set_char(env.core, &ch, json!({ "age": a }));
            actions::notify_err(env.ed, r);
        }
        ui.end_row();
        dim_label(ui, &t, tr("char.f.heads"));
        let r = commit_on_release(ui, EId::new(("char_heads", &ch.0)), c.heads_tall, |ui, v| ui.add(egui::DragValue::new(v).range(1.5..=10.0).speed(0.05).max_decimals(2)));
        if let Some(v) = r {
            let r = set_char(env.core, &ch, json!({ "heads_tall": v }));
            actions::notify_err(env.ed, r);
        }
        ui.end_row();
    });
    dim_label(ui, &t, tr("char.f.profile"));
    if let (_, Some(p)) = super::text_commit_multi(ui, EId::new(("char_profile", &ch.0)), &c.profile, w, tr("char.f.profile_hint")) {
        let r = set_char(env.core, &ch, json!({ "profile": p }));
        actions::notify_err(env.ed, r);
    }

}

fn palette_tab(env: &mut Env, ui: &mut Ui, ch: &NodeId, c: &CharacterData, name: &str, w: f32) {
    let t = env.t;
    let ch = ch.clone();
    // paleta de settei
    section(ui, &t, "char.sec.palette");
    let pals: Vec<(NodeId, String)> = env.core.project.roots.palettes.iter().filter_map(|id| env.core.project.node(id).map(|n| (id.clone(), n.name.clone()))).collect();
    let cur = c.palette.as_ref().and_then(|p| pals.iter().find(|(id, _)| id == p)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("palette.none").to_owned());
    let mut pick = None;
    ui.horizontal(|ui| {
        let cb = egui::ComboBox::from_id_salt(("char_pal", &ch.0)).width((w - 110.0).max(80.0)).selected_text(cur).show_ui(ui, |ui| {
            for (id, n) in &pals {
                if ui.selectable_label(c.palette.as_ref() == Some(id), n).clicked() {
                    pick = Some(id.clone());
                }
            }
        });
        env.note("char:palette", cb.response.rect);
        let b = ui.button(tr("char.new_palette"));
        env.note("char:new_palette", b.rect);
        if b.clicked() {
            let r = cmds::add_palette(env.core);
            if let Some(pal) = actions::notify_err(env.ed, r) {
                let _ = cmds::rename_node(env.core, &pal, &format!("{} {name}", tr("char.palette_of")));
                pick = Some(pal);
            }
        }
    });
    if let Some(pal) = pick {
        let r = set_char(env.core, &ch, json!({ "palette": pal }));
        if actions::notify_err(env.ed, r).is_some() {
            env.ed.palette = Some(pal);
        }
    }
    let n_colors = c.palette.as_ref().and_then(|p| cmds::palette_data(&env.core.project, p)).map(|pd| pd.colors.len()).unwrap_or(0);
    if c.palette.is_some() {
        dim_label(ui, &t, &format!("{n_colors} {}", tr("char.colors_hint")));
    }

}

fn voice(env: &mut Env, ui: &mut Ui, ch: &NodeId, c: &CharacterData, w: f32) {
    let t = env.t;
    section(ui, &t, "char.sec.voice");
    if c.voice.is_null() {
        let b = ui.button(tr("char.voice_set"));
        env.note("char:voice_set", b.rect);
        if b.clicked() {
            let prof = json!({ "engine": "voicevox", "speaker": 8, "speaker_name": "春日部つむぎ", "params": {} });
            let r = set_voice(env.core, ch, prof);
            actions::notify_err(env.ed, r);
        }
        return;
    }
    let mut prof = c.voice.clone();
    let mut changed = false;
    egui::Grid::new(("char_voice", &ch.0)).num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("char.v.engine"));
        dim_label(ui, &t, &prof.get("engine").and_then(Value::as_str).unwrap_or("voicevox").to_uppercase());
        ui.end_row();
        dim_label(ui, &t, tr("char.v.speaker"));
        let cur = prof.get("speaker").and_then(Value::as_u64).unwrap_or(0) as u32;
        if let Some(v) = commit_on_release(ui, EId::new(("char_spk", &ch.0)), cur, |ui, v| ui.add(egui::DragValue::new(v).range(0..=999))) {
            prof["speaker"] = json!(v);
            changed = true;
        }
        ui.end_row();
        dim_label(ui, &t, tr("char.v.speaker_name"));
        let cur = prof.get("speaker_name").and_then(Value::as_str).unwrap_or("").to_owned();
        if let (_, Some(n)) = super::text_commit_opt(ui, EId::new(("char_spkname", &ch.0)), &cur, (w - 90.0).max(60.0), "") {
            prof["speaker_name"] = json!(n);
            changed = true;
        }
        ui.end_row();
        for (key, label, lo, hi, def) in [("speed_scale", "char.v.speed", 0.5, 2.0, 1.0), ("pitch_scale", "char.v.pitch", -0.15, 0.15, 0.0), ("intonation_scale", "char.v.intonation", 0.0, 2.0, 1.0), ("volume_scale", "char.v.volume", 0.0, 2.0, 1.0)] {
            dim_label(ui, &t, tr(label));
            let cur = prof.get("params").and_then(|p| p.get(key)).and_then(Value::as_f64).unwrap_or(def) as f32;
            if let Some(v) = commit_on_release(ui, EId::new(("char_vp", &ch.0, key)), cur, |ui, v| ui.add(egui::Slider::new(v, lo..=hi).max_decimals(2))) {
                if !prof.get("params").is_some_and(Value::is_object) {
                    prof["params"] = json!({});
                }
                prof["params"][key] = json!(v);
                changed = true;
            }
            ui.end_row();
        }
    });
    dim_label(ui, &t, tr("char.v.credit_hint"));
    if changed {
        let r = set_voice(env.core, ch, prof);
        actions::notify_err(env.ed, r);
    }
}

/// `VOICE_PROFILE` com o perfil inteiro (os campos do perfil vão ao lado de `character`).
fn set_voice(core: &mut CoreSession, ch: &NodeId, mut prof: Value) -> Result<Value, String> {
    prof["character"] = json!(ch);
    actions::run(core, "VOICE_PROFILE", prof)
}

fn sheet_form(env: &mut Env, ui: &mut Ui, ch: &NodeId, c: &CharacterData) {
    let t = env.t;
    section(ui, &t, "char.sec.sheet");
    let st = &mut env.ed.chars;
    ui.horizontal_wrapped(|ui| {
        for (i, (_, key)) in VIEWS.iter().enumerate() {
            ui.checkbox(&mut st.views[i], tr(key));
        }
    });
    ui.horizontal(|ui| {
        dim_label(ui, &t, tr("char.f.figure_px"));
        ui.add(egui::DragValue::new(&mut st.figure_px).range(200.0..=2000.0).speed(4.0).max_decimals(0));
    });
    let views: Vec<f32> = VIEWS.iter().zip(st.views).filter(|(_, on)| *on).map(|((d, _), _)| *d).collect();
    let figure_px = st.figure_px;
    let has = sheet_cut(&env.core.project, ch).is_some();
    let b = ui.add_enabled(!views.is_empty(), egui::Button::new(tr(if has { "char.sheet_new_again" } else { "char.sheet_new" })));
    env.note("char:sheet_new", b.rect);
    if b.clicked() {
        let name = format!("{} {}", tr("panel.sheet.title"), env.core.project.node(ch).map(|n| n.name.clone()).unwrap_or_default());
        let r = actions::run(env.core, "MODEL_SHEET", json!({ "character": ch, "name": name, "heads_tall": c.heads_tall, "figure_px": figure_px, "views": views }));
        open_created(env, r);
    }
}

fn consistency(env: &mut Env, ui: &mut Ui, ch: &NodeId, c: &CharacterData) {
    let t = env.t;
    section(ui, &t, "char.sec.consistency");
    if c.measurements.get("heads_tall").is_none() {
        dim_label(ui, &t, tr("char.cons_need_sheet"));
        return;
    }
    dim_label(ui, &t, tr("char.cons_hint"));
    // medir no canvas: "Medir" arma a linha e o próximo clique no desenho dá o y; "Do desenho" pega topo e pés
    let mut pick_toggle: Option<usize> = None;
    let mut notes: Vec<(String, Rect)> = Vec::new();
    let f = &mut env.ed.chars.cons;
    let armed = f.pick;
    let mut pick_btn = |ui: &mut Ui, i: usize| {
        let r = ui.selectable_label(armed == Some(i), egui::RichText::new(tr(if armed == Some(i) { "char.cons_pick_armed" } else { "char.cons_pick" })).size(11.5)).on_hover_text(tr("char.cons_pick.tip"));
        notes.push((format!("char:cons_pick:{i}"), r.rect));
        if r.clicked() {
            pick_toggle = Some(i);
        }
    };
    egui::Grid::new(("char_cons", &ch.0)).num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("char.m.head_top"));
        ui.add(egui::DragValue::new(&mut f.head_top).speed(1.0).max_decimals(1));
        pick_btn(ui, 0);
        ui.end_row();
        dim_label(ui, &t, tr("char.m.chin"));
        ui.add(egui::DragValue::new(&mut f.chin).speed(1.0).max_decimals(1));
        pick_btn(ui, 1);
        ui.end_row();
        for (i, (slot, key, def)) in [(&mut f.eyes, "char.m.eyes", 170.0), (&mut f.shoulders, "char.m.shoulders", 260.0), (&mut f.feet, "char.m.feet", 900.0)].into_iter().enumerate() {
            let mut on = slot.is_some();
            if ui.checkbox(&mut on, tr(key)).changed() {
                *slot = on.then_some(slot.unwrap_or(def));
            }
            match slot {
                Some(v) => {
                    ui.add(egui::DragValue::new(v).speed(1.0).max_decimals(1));
                }
                None => {
                    ui.label("");
                }
            }
            pick_btn(ui, i + 2);
            ui.end_row();
        }
    });
    if let Some(i) = pick_toggle {
        f.pick = if f.pick == Some(i) { None } else { Some(i) };
    }
    for (n, r) in notes {
        env.note(n, r);
    }
    let b = ui.button(tr("char.cons_from_drawing")).on_hover_text(tr("char.cons_from_drawing.tip"));
    env.note("char:cons_from_drawing", b.rect);
    if b.clicked() {
        match drawing_box(env) {
            Some((top, bottom)) => {
                let f = &mut env.ed.chars.cons;
                f.head_top = top;
                f.feet = Some(bottom);
            }
            None => env.ed.notify(tr("char.cons_no_drawing")),
        }
    }
    let b = ui.button(tr("char.cons_measure"));
    env.note("char:cons_measure", b.rect);
    if b.clicked() {
        let f = env.ed.chars.cons.clone();
        let r = actions::run(env.core, "CONSISTENCY", json!({ "character": ch, "head_top": f.head_top, "chin": f.chin, "eyes": f.eyes, "shoulders": f.shoulders, "feet": f.feet }));
        env.ed.chars.report = Some((ch.clone(), env.core.change_counter, r));
    }
    let Some((rch, _, res)) = &env.ed.chars.report else { return };
    if rch != ch {
        return;
    }
    match res {
        Err(e) => dim_label(ui, &t, e),
        Ok(rep) => {
            let ok = rep.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let max = rep.get("max_error_pct").and_then(Value::as_f64).unwrap_or(0.0);
            // etiqueta de estado como na Produção: fundo do estado, texto normal por cima
            let fill = if ok { t.st_ok } else { t.st_retake };
            egui::Frame::new().fill(fill).corner_radius(3.0).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                ui.label(egui::RichText::new(format!("{}  ·  {} {}%", tr(if ok { "char.cons_ok" } else { "char.cons_bad" }), tr("char.cons_max"), fmt_dec(max as f32))).color(t.text).strong());
            });
            if let Some(errs) = rep.get("errors_pct").and_then(Value::as_object) {
                for (k, v) in errs {
                    dim_label(ui, &t, &format!("{}: {}%", measure_label(k), fmt_dec(v.as_f64().unwrap_or(0.0) as f32)));
                }
            }
        }
    }
}

/// Nome traduzido de uma linha-guia da folha (a ferramenta grava em português: `topo`, `olhos`...); outras ficam como estão.
fn landmark_label(name: &str) -> String {
    let key = match name {
        "topo" => "char.lm.top",
        "olhos" => "char.lm.eyes",
        "queixo" => "char.lm.chin",
        "ombros" => "char.lm.shoulders",
        "cintura" => "char.lm.waist",
        "quadril" => "char.lm.hips",
        "joelhos" => "char.lm.knees",
        "pés" => "char.lm.feet",
        other => return other.to_owned(),
    };
    tr(key).to_owned()
}

/// Nome legível de uma medida do relatório (`olhos`, `ombros`, `altura_total`...).
fn measure_label(k: &str) -> String {
    match k {
        "olhos" => tr("char.m.eyes").to_owned(),
        "ombros" => tr("char.m.shoulders").to_owned(),
        "altura_total" => tr("char.m.feet").to_owned(),
        other => other.replace('_', " "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;

    fn session() -> (CoreSession, NodeId) {
        let mut core = CoreSession::new(Project::new("t"));
        core.author = AUTHOR.into();
        let id = add_character(&mut core).unwrap();
        (core, id)
    }

    #[test]
    fn new_character_has_default_height_and_undo_removes_it() {
        let (mut core, id) = session();
        assert_eq!(core.project.roots.characters, vec![id.clone()]);
        assert_eq!(char_data(&core.project, &id).unwrap().heads_tall, 6.5);
        core.undo().unwrap();
        assert!(core.project.roots.characters.is_empty());
    }

    #[test]
    fn model_sheet_is_found_and_consistency_measures_against_it() {
        let (mut core, id) = session();
        assert!(sheet_cut(&core.project, &id).is_none());
        let v = actions::run(&mut core, "MODEL_SHEET", json!({ "character": id, "name": "Folha", "heads_tall": 6.0, "figure_px": 600.0, "views": [0.0, 90.0] })).unwrap();
        let cut = Id(v["cut"].as_str().unwrap().to_owned());
        assert_eq!(sheet_cut(&core.project, &id), Some(cut));
        // desenho com as mesmas proporções da folha: cabeça de 100 px, 6 cabeças → pés a 600 px do topo
        let rep = actions::run(&mut core, "CONSISTENCY", json!({ "character": id, "head_top": 0.0, "chin": 100.0, "feet": 600.0 })).unwrap();
        assert_eq!(rep["ok"], json!(true), "{rep}");
        let rep = actions::run(&mut core, "CONSISTENCY", json!({ "character": id, "head_top": 0.0, "chin": 100.0, "feet": 800.0 })).unwrap();
        assert_eq!(rep["ok"], json!(false), "{rep}");
    }

    #[test]
    fn landmarks_accept_list_and_object_formats() {
        let list = json!({ "heads_tall": 6.0, "landmarks": [{ "name": "olhos", "heads": 0.6, "y_px": 10.0 }] });
        let obj = json!({ "heads_tall": 6.0, "landmarks": { "olhos": 0.6, "franja": 0.3 } });
        for m in [&list, &obj] {
            let lm = aiai_tools::character::sheet_landmarks(m);
            assert_eq!(lm.iter().find(|(n, _)| n == "olhos").map(|x| x.1), Some(0.6));
            assert_eq!(lm.iter().find(|(n, _)| n == "pés").map(|x| x.1), Some(6.0));
        }
        assert!(aiai_tools::character::sheet_landmarks(&obj).iter().any(|(n, _)| n == "franja"));
        assert!(aiai_tools::character::sheet_landmarks(&json!({})).is_empty());
    }

    #[test]
    fn voice_and_palette_round_trip() {
        let (mut core, id) = session();
        let prof = json!({ "engine": "voicevox", "speaker": 3, "speaker_name": "x", "params": { "speed_scale": 1.1 } });
        set_voice(&mut core, &id, prof).unwrap();
        let c = char_data(&core.project, &id).unwrap();
        assert_eq!(c.voice["speaker"], json!(3));
        assert!(voice_summary(c).contains("#3"));
        let pal = cmds::add_palette(&mut core).unwrap();
        set_char(&mut core, &id, json!({ "palette": pal })).unwrap();
        assert_eq!(char_data(&core.project, &id).unwrap().palette.as_ref(), Some(&pal));
    }
}
