//! Construtores de comandos do editor: tudo o que os painéis e as ferramentas fazem no documento vira UM `Command`
//! (lotes quando precisam de vários passos, para o Ctrl+Z desfazer o gesto inteiro). Nada aqui toca a UI: recebe a
//! `CoreSession` e devolve o resultado, então dá para testar sem janela.

use crate::i18n::tr;
use aiai_core::command::{ApplyCtx, CellEdit};
use aiai_core::*;
use serde_json::{Value, json};

pub(crate) type NodeId = aiai_core::Id;

// ------------------------------------------------------------------------------------------------ consultas

/// Episódio → cenas → cortes, na ordem do documento.
pub(crate) struct EpisodeTree {
    pub id: NodeId,
    pub scenes: Vec<(NodeId, Vec<NodeId>)>,
}

pub(crate) fn episode_tree(p: &Project) -> Vec<EpisodeTree> {
    p.roots
        .episodes
        .iter()
        .filter_map(|ep| {
            let node = p.node(ep)?;
            let scenes = node
                .children
                .iter()
                .filter_map(|sc| {
                    let s = p.node(sc)?;
                    let cuts = s.children.iter().filter(|c| matches!(p.node(c).map(|n| &n.data), Some(NodeKind::Cut(_)))).cloned().collect();
                    Some((sc.clone(), cuts))
                })
                .collect();
            Some(EpisodeTree { id: ep.clone(), scenes })
        })
        .collect()
}

/// Primeiro corte do documento (ordem de episódio/cena).
pub(crate) fn first_cut(p: &Project) -> Option<NodeId> {
    episode_tree(p).into_iter().flat_map(|e| e.scenes).flat_map(|(_, cuts)| cuts).next()
}

pub(crate) fn is_cut(p: &Project, id: &NodeId) -> bool {
    matches!(p.node(id).map(|n| &n.data), Some(NodeKind::Cut(_)))
}

pub(crate) fn cut_data<'a>(p: &'a Project, id: &NodeId) -> Option<&'a CutData> {
    match &p.node(id)?.data {
        NodeKind::Cut(c) => Some(c),
        _ => None,
    }
}

pub(crate) fn layer_data<'a>(p: &'a Project, id: &NodeId) -> Option<&'a LayerData> {
    match &p.node(id)?.data {
        NodeKind::Layer(l) => Some(l),
        _ => None,
    }
}

pub(crate) fn cel_data<'a>(p: &'a Project, id: &NodeId) -> Option<&'a CelData> {
    match &p.node(id)?.data {
        NodeKind::Cel(c) => Some(c),
        _ => None,
    }
}

/// Tamanho do canvas do corte (o do projeto, se o corte não define outro).
pub(crate) fn canvas_size(p: &Project, cut: &NodeId) -> (u32, u32) {
    match cut_data(p, cut) {
        Some(c) => aiai_render::cut_canvas(p, c),
        None => (p.settings.width, p.settings.height),
    }
}

/// Uma linha da árvore de camadas, de cima para baixo (ordem visual; `children` do documento é de baixo para cima).
#[derive(Debug, Clone)]
pub(crate) struct LayerRow {
    pub id: NodeId,
    pub depth: usize,
}

pub(crate) fn layer_rows(p: &Project, parent: &NodeId) -> Vec<LayerRow> {
    fn walk(p: &Project, parent: &NodeId, depth: usize, out: &mut Vec<LayerRow>) {
        let Some(n) = p.node(parent) else { return };
        for child in n.children.iter().rev() {
            let Some(c) = p.node(child) else { continue };
            if let NodeKind::Layer(l) = &c.data {
                out.push(LayerRow { id: child.clone(), depth });
                if l.kind == LayerType::Group {
                    walk(p, child, depth + 1, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(p, parent, 0, &mut out);
    out
}

/// Cels de uma camada (número → id), ordenados por número.
pub(crate) fn layer_cels(p: &Project, layer: &NodeId) -> Vec<(u32, NodeId)> {
    let mut v: Vec<(u32, NodeId)> = p
        .node(layer)
        .map(|l| l.children.iter().filter_map(|c| cel_data(p, c).map(|cd| (cd.number, c.clone()))).collect())
        .unwrap_or_default();
    v.sort_by_key(|(n, _)| *n);
    v
}

/// Cel exposto pela camada no quadro (mesma regra do compositor).
/// Transformação cel → canvas da camada no quadro (cel exposto + movimento da camada); `None` se for a identidade.
pub(crate) fn layer_placement(p: &Project, cut: &NodeId, layer: &NodeId, frame: u32) -> Option<aiai_paint::Affine> {
    let l = layer_data(p, layer)?;
    let ct = exposed_cel(p, cut, layer, frame).and_then(|c| cel_data(p, &c).map(|d| d.transform)).unwrap_or([0.0, 0.0, 1.0, 0.0]);
    let (w, h) = canvas_size(p, cut);
    let a = aiai_render::compose::layer_placement(l, ct, frame, w, h);
    (!a.is_identity()).then_some(a)
}

pub(crate) fn exposed_cel(p: &Project, cut: &NodeId, layer: &NodeId, frame: u32) -> Option<NodeId> {
    let c = cut_data(p, cut)?;
    aiai_tools::context::exposed_cel(p, c, layer, frame).map(|n| n.id.clone())
}

/// Número do cel exposto (para a interface).
pub(crate) fn exposed_number(p: &Project, cut: &NodeId, layer: &NodeId, frame: u32) -> Option<u32> {
    exposed_cel(p, cut, layer, frame).and_then(|c| cel_data(p, &c).map(|d| d.number))
}

/// A cor atual resolvida para RGBA sRGB (paleta ou direta).
pub(crate) fn resolve_color(p: &Project, c: &ColorRef) -> Rgba {
    match c {
        ColorRef::Direct { rgba } => *rgba,
        ColorRef::Palette { palette, color } => match p.node(palette).map(|n| &n.data) {
            Some(NodeKind::Palette(pd)) => pd.resolve(color).unwrap_or(Rgba::BLACK),
            _ => Rgba::BLACK,
        },
    }
}

pub(crate) fn layer_type_key(k: LayerType) -> &'static str {
    match k {
        LayerType::Group => "layer.kind.group",
        LayerType::Vector => "layer.kind.vector",
        LayerType::Raster => "layer.kind.raster",
        LayerType::Fill => "layer.kind.fill",
        LayerType::Text => "layer.kind.text",
        LayerType::Guide => "layer.kind.guide",
        LayerType::Reference => "layer.kind.reference",
        LayerType::Adjustment => "layer.kind.adjustment",
        LayerType::Effect => "layer.kind.effect",
        LayerType::Particles => "layer.kind.particles",
    }
}

pub(crate) fn blend_key(b: BlendMode) -> &'static str {
    match b {
        BlendMode::Normal => "blend.normal",
        BlendMode::PassThrough => "blend.pass_through",
        BlendMode::Multiply => "blend.multiply",
        BlendMode::Screen => "blend.screen",
        BlendMode::Overlay => "blend.overlay",
        BlendMode::SoftLight => "blend.soft_light",
        BlendMode::HardLight => "blend.hard_light",
        BlendMode::Add => "blend.add",
        BlendMode::Subtract => "blend.subtract",
        BlendMode::Darken => "blend.darken",
        BlendMode::Lighten => "blend.lighten",
        BlendMode::Difference => "blend.difference",
        BlendMode::Color => "blend.color",
        BlendMode::Hue => "blend.hue",
        BlendMode::Saturation => "blend.saturation",
        BlendMode::Luminosity => "blend.luminosity",
        BlendMode::Behind => "blend.behind",
    }
}

pub(crate) const BLEND_MODES: [BlendMode; 17] = [
    BlendMode::Normal,
    BlendMode::PassThrough,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::Add,
    BlendMode::Subtract,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::Difference,
    BlendMode::Color,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Luminosity,
    BlendMode::Behind,
];

pub(crate) const ROLES: [LayerRole; 12] = [
    LayerRole::None,
    LayerRole::Layout,
    LayerRole::Genga,
    LayerRole::Douga,
    LayerRole::Shiage,
    LayerRole::Shadow,
    LayerRole::Highlight,
    LayerRole::Background,
    LayerRole::Book,
    LayerRole::Effect,
    LayerRole::Correction,
    LayerRole::Construction,
];

pub(crate) fn role_key(r: LayerRole) -> &'static str {
    match r {
        LayerRole::None => "role.none",
        LayerRole::Layout => "role.layout",
        LayerRole::Genga => "role.genga",
        LayerRole::Douga => "role.douga",
        LayerRole::Shiage => "role.shiage",
        LayerRole::Shadow => "role.shadow",
        LayerRole::Highlight => "role.highlight",
        LayerRole::Background => "role.background",
        LayerRole::Book => "role.book",
        LayerRole::Effect => "role.effect",
        LayerRole::Correction => "role.correction",
        LayerRole::Construction => "role.construction",
    }
}

pub(crate) fn cel_role_key(r: CelRole) -> &'static str {
    match r {
        CelRole::Key => "cel.role.key",
        CelRole::Breakdown => "cel.role.breakdown",
        CelRole::Inbetween => "cel.role.inbetween",
    }
}

// ------------------------------------------------------------------------------------------------ execução

pub(crate) fn run(core: &mut CoreSession, cmd: Command) -> Result<Outcome, String> {
    core.apply(cmd).map_err(|e| e.to_string())
}

pub(crate) fn create(id: NodeId, parent: Option<NodeId>, index: Option<usize>, name: impl Into<String>, data: NodeKind) -> Command {
    Command::CreateNode { id: Some(id), parent, index, name: name.into(), data }
}

// ------------------------------------------------------------------------------------------------ projeto

/// Próximo número livre entre irmãos de um tipo (episódio, cena, corte).
fn next_number(p: &Project, siblings: &[NodeId]) -> u32 {
    siblings
        .iter()
        .filter_map(|s| match &p.node(s)?.data {
            NodeKind::Episode(e) => Some(e.number),
            NodeKind::Scene(s) => Some(s.number),
            NodeKind::Cut(c) => Some(c.number),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        + 1
}

pub(crate) fn add_episode(core: &mut CoreSession) -> Result<NodeId, String> {
    let n = next_number(&core.project, &core.project.roots.episodes);
    let id = core.ids.next();
    let name = format!("{} {n}", tr("tree.episode"));
    run(core, create(id.clone(), None, None, name, NodeKind::Episode(EpisodeData { number: n })))?;
    Ok(id)
}

pub(crate) fn add_scene(core: &mut CoreSession, episode: &NodeId) -> Result<NodeId, String> {
    let kids = core.project.node(episode).map(|n| n.children.clone()).unwrap_or_default();
    let n = next_number(&core.project, &kids);
    let id = core.ids.next();
    let name = format!("{} {n}", tr("tree.scene"));
    run(core, create(id.clone(), Some(episode.clone()), None, name, NodeKind::Scene(SceneData { number: n, description: String::new() })))?;
    Ok(id)
}

pub(crate) fn add_cut(core: &mut CoreSession, scene: &NodeId, duration: u32, canvas: Option<[u32; 2]>) -> Result<NodeId, String> {
    let kids = core.project.node(scene).map(|n| n.children.clone()).unwrap_or_default();
    let n = next_number(&core.project, &kids);
    let id = core.ids.next();
    let mut cd = CutData::new(duration.max(1));
    cd.number = n;
    cd.canvas = canvas;
    let name = format!("{} {n}", tr("tree.cut"));
    run(core, create(id.clone(), Some(scene.clone()), None, name, NodeKind::Cut(cd)))?;
    Ok(id)
}

/// Episódio + cena + corte de uma vez (projeto vazio): um passo de undo.
pub(crate) fn quick_start(core: &mut CoreSession, duration: u32, canvas: Option<[u32; 2]>) -> Result<NodeId, String> {
    let (cut, cmds) = quick_start_commands(core, duration, canvas);
    run(core, Command::Batch { commands: cmds, label: Some(tr("cmd.quick_start").to_owned()) })?;
    Ok(cut)
}

fn quick_start_commands(core: &mut CoreSession, duration: u32, canvas: Option<[u32; 2]>) -> (NodeId, Vec<Command>) {
    let (ep, sc, cut) = (core.ids.next(), core.ids.next(), core.ids.next());
    let mut cd = CutData::new(duration.max(1));
    cd.number = 1;
    cd.canvas = canvas;
    let cmds = vec![
        create(ep.clone(), None, None, format!("{} 1", tr("tree.episode")), NodeKind::Episode(EpisodeData { number: 1 })),
        create(sc.clone(), Some(ep), None, format!("{} 1", tr("tree.scene")), NodeKind::Scene(SceneData { number: 1, description: String::new() })),
        create(cut.clone(), Some(sc), None, format!("{} 1", tr("tree.cut")), NodeKind::Cut(cd)),
    ];
    (cut, cmds)
}

/// Começo rápido da interface: o corte já vem com as três camadas de trabalho (Fundo raster, Cor ligada à Linha,
/// Linha vetorial), cada uma com o cel 1. Devolve (corte, camada de linha).
pub(crate) fn quick_start_with_layers(core: &mut CoreSession, duration: u32, canvas: Option<[u32; 2]>) -> Result<(NodeId, NodeId), String> {
    // um passo só de desfazer: corte + camadas
    let (cut, mut cmds) = quick_start_commands(core, duration, canvas);
    let (bg, fill, line) = (core.ids.next(), core.ids.next(), core.ids.next());
    let mut fd = LayerData::new(LayerType::Fill);
    fd.line_refs.push(line.clone());
    for (id, name, data) in [
        (bg.clone(), tr("layer.default_bg"), LayerData::new(LayerType::Raster)),
        (fill.clone(), tr("layer.default_color"), fd),
        (line.clone(), tr("layer.default_line"), LayerData::new(LayerType::Vector)),
    ] {
        cmds.push(create(id.clone(), Some(cut.clone()), None, name, NodeKind::Layer(data)));
        let cel = core.ids.next();
        cmds.push(create(cel, Some(id), None, "1", NodeKind::Cel(CelData::new(1))));
    }
    run(core, Command::Batch { commands: cmds, label: Some(tr("cmd.quick_start").to_owned()) })?;
    Ok((cut, line))
}

pub(crate) fn rename_node(core: &mut CoreSession, id: &NodeId, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || core.project.node(id).is_some_and(|n| n.name == name) {
        return Ok(());
    }
    run(core, Command::SetProps { id: id.clone(), name: Some(name.to_owned()), data: Value::Null }).map(|_| ())
}

pub(crate) fn delete_node(core: &mut CoreSession, id: &NodeId) -> Result<(), String> {
    run(core, Command::DeleteNode { id: id.clone() }).map(|_| ())
}

pub(crate) fn set_project_props(core: &mut CoreSession, patch: Value) -> Result<(), String> {
    run(core, Command::SetProjectProps { patch }).map(|_| ())
}

pub(crate) fn set_cut_props(core: &mut CoreSession, cut: &NodeId, patch: Value) -> Result<(), String> {
    run(core, Command::SetProps { id: cut.clone(), name: None, data: patch }).map(|_| ())
}

// ------------------------------------------------------------------------------------------------ camadas

pub(crate) fn unique_layer_name(p: &Project, parent: &NodeId, base: &str) -> String {
    let existing: Vec<&str> = p.node(parent).map(|n| n.children.iter().filter_map(|c| p.node(c)).map(|n| n.name.as_str()).collect()).unwrap_or_default();
    (1..)
        .map(|i| format!("{base} {i}"))
        .find(|candidate| !existing.contains(&candidate.as_str()))
        .unwrap_or_else(|| base.to_owned())
}

/// Cria uma camada (e o cel 1 das que guardam cels) num passo só. Com `above` = camada ativa: entra logo acima dela (na
/// pasta, se for uma pasta); sem ela, no topo do corte. A camada de preenchimento nasce ligada à camada vetorial mais próxima
/// e entra logo ABAIXO dela (a cor fica sob a linha, como no tinta-e-pinta).
pub(crate) fn add_layer(core: &mut CoreSession, cut: &NodeId, kind: LayerType, above: Option<&NodeId>) -> Result<NodeId, String> {
    let p = &core.project;
    let (mut parent, mut index) = match above.and_then(|a| p.node(a).map(|n| (a, n))) {
        Some((a, n)) if matches!(&n.data, NodeKind::Layer(l) if l.kind == LayerType::Group) => (a.clone(), None),
        Some((a, n)) => match &n.parent {
            Some(par) => {
                let idx = p.node(par).and_then(|pn| pn.children.iter().position(|c| c == a)).map(|i| i + 1);
                (par.clone(), idx)
            }
            None => (cut.clone(), None),
        },
        None => (cut.clone(), None),
    };
    let mut data = LayerData::new(kind);
    if kind == LayerType::Text {
        // camada de texto do "+": texto de ponto no meio do canvas, editavel com a ferramenta Texto ou no painel Texto
        let tp = super::text_tool::default_layer_params(canvas_size(p, cut), ColorRef::default());
        data.params = json!({ aiai_paint::text_layer::TextParams::KEY: tp.to_value() });
    }
    if kind == LayerType::Fill {
        // a tinta fica ABAIXO da linha (tinta-e-pinta): a camada de preenchimento entra logo embaixo da camada vetorial de referência
        let rows = layer_rows(p, cut);
        let is_vec = |id: &NodeId| layer_data(p, id).is_some_and(|l| l.kind == LayerType::Vector);
        let near = above.filter(|a| is_vec(a)).cloned().or_else(|| rows.iter().find(|r| is_vec(&r.id)).map(|r| r.id.clone()));
        if let Some(v) = near {
            if let Some(vn) = p.node(&v) {
                if let Some(vp) = &vn.parent {
                    let idx = p.node(vp).and_then(|pn| pn.children.iter().position(|c| *c == v));
                    parent = vp.clone();
                    index = idx;
                }
            }
            data.line_refs.push(v);
        }
    }
    let name = unique_layer_name(p, &parent, tr(layer_type_key(kind)));
    let id = core.ids.next();
    let mut cmds = vec![create(id.clone(), Some(parent), index, name, NodeKind::Layer(data))];
    if matches!(kind, LayerType::Vector | LayerType::Raster | LayerType::Fill | LayerType::Guide) {
        let cel = core.ids.next();
        cmds.push(create(cel, Some(id.clone()), None, "1", NodeKind::Cel(CelData::new(1))));
    }
    let label = format!("{} {}", tr("cmd.add_layer"), tr(layer_type_key(kind)));
    run(core, Command::Batch { commands: cmds, label: Some(label) })?;
    Ok(id)
}

pub(crate) fn set_layer_props(core: &mut CoreSession, layer: &NodeId, patch: Value) -> Result<(), String> {
    run(core, Command::SetProps { id: layer.clone(), name: None, data: patch }).map(|_| ())
}

/// Sobe (`delta` > 0) ou desce a camada entre os irmãos (a lista do documento é de baixo para cima).
pub(crate) fn move_layer(core: &mut CoreSession, layer: &NodeId, delta: i32) -> Result<bool, String> {
    let Some(node) = core.project.node(layer) else { return Ok(false) };
    let Some(parent) = node.parent.clone() else { return Ok(false) };
    let sibs = core.project.node(&parent).map(|p| p.children.clone()).unwrap_or_default();
    let Some(i) = sibs.iter().position(|c| c == layer) else { return Ok(false) };
    // só conta irmãos que são camadas (cels não existem sob o corte/pasta, mas por segurança)
    let n = sibs.len() as i32;
    let target = (i as i32 + delta).clamp(0, n - 1);
    if target == i as i32 {
        return Ok(false);
    }
    run(core, Command::MoveNode { id: layer.clone(), parent: Some(parent), index: Some(target as usize) })?;
    Ok(true)
}

/// Onde soltar camadas arrastadas: em cima/embaixo de uma camada (no mesmo pai) ou dentro de uma pasta (no topo).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LayerDrop {
    Above(NodeId),
    Below(NodeId),
    Into(NodeId),
}

/// Move várias camadas (lista de cima para baixo) para o lugar `drop`, mantendo a ordem entre elas: um passo de desfazer.
pub(crate) fn move_layers(core: &mut CoreSession, layers: &[NodeId], drop: &LayerDrop) -> Result<(), String> {
    let p = &core.project;
    let (parent, anchor) = match drop {
        LayerDrop::Into(g) => (g.clone(), None),
        LayerDrop::Above(a) | LayerDrop::Below(a) => (p.node(a).and_then(|n| n.parent.clone()).ok_or("camada sem pai")?, Some(a.clone())),
    };
    // uma pasta não entra nela mesma nem numa de suas filhas
    if layers.iter().any(|l| p.subtree(l).contains(&parent)) {
        return Err(crate::i18n::tr("layers.err.into_self").to_owned());
    }
    if anchor.as_ref().is_some_and(|a| layers.contains(a)) {
        return Ok(());
    }
    // simula a lista de filhos do destino (índice 0 = embaixo) para calcular cada MoveNode na ordem em que roda
    let mut sim: Vec<NodeId> = p.node(&parent).map(|n| n.children.clone()).unwrap_or_default();
    let mut cmds = Vec::new();
    // de baixo para cima: cada uma entra logo acima da anterior
    let mut prev: Option<NodeId> = None;
    for l in layers.iter().rev() {
        sim.retain(|c| c != l);
        let idx = match (&prev, drop, &anchor) {
            (Some(pv), _, _) => sim.iter().position(|c| c == pv).map(|i| i + 1).unwrap_or(sim.len()),
            (None, LayerDrop::Into(_), _) => sim.len(),
            (None, LayerDrop::Above(_), Some(a)) => sim.iter().position(|c| c == a).map(|i| i + 1).unwrap_or(sim.len()),
            (None, LayerDrop::Below(_), Some(a)) => sim.iter().position(|c| c == a).unwrap_or(0),
            _ => sim.len(),
        };
        sim.insert(idx.min(sim.len()), l.clone());
        cmds.push(Command::MoveNode { id: l.clone(), parent: Some(parent.clone()), index: Some(idx) });
        prev = Some(l.clone());
    }
    run(core, Command::Batch { commands: cmds, label: Some(format!("Mover {} camada(s)", layers.len())) }).map(|_| ())
}

/// Apaga várias camadas num passo de desfazer.
pub(crate) fn delete_layers(core: &mut CoreSession, layers: &[NodeId]) -> Result<(), String> {
    // não apaga a filha de uma pasta que também vai sair (a pasta leva junto)
    let p = &core.project;
    let keep: Vec<NodeId> = layers.iter().filter(|l| !layers.iter().any(|o| o != *l && p.subtree(o).contains(l))).cloned().collect();
    let cmds = keep.iter().map(|l| Command::DeleteNode { id: l.clone() }).collect();
    run(core, Command::Batch { commands: cmds, label: Some(format!("Apagar {} camada(s)", keep.len())) }).map(|_| ())
}

/// Move a camada para dentro de uma pasta (no topo) ou, com `None`, de volta para o corte.
pub(crate) fn move_layer_to(core: &mut CoreSession, layer: &NodeId, new_parent: &NodeId) -> Result<(), String> {
    run(core, Command::MoveNode { id: layer.clone(), parent: Some(new_parent.clone()), index: None }).map(|_| ())
}

// ------------------------------------------------------------------------------------------------ cels

/// Novo cel (número = maior + 1) ou cópia de um cel existente.
pub(crate) fn add_cel(core: &mut CoreSession, layer: &NodeId, duplicate_of: Option<u32>) -> Result<u32, String> {
    let cels = layer_cels(&core.project, layer);
    let number = cels.last().map(|(n, _)| n + 1).unwrap_or(1);
    let mut data = CelData::new(number);
    if let Some(src) = duplicate_of.and_then(|n| cels.iter().find(|(m, _)| *m == n)).and_then(|(_, id)| cel_data(&core.project, id)) {
        data = src.clone();
        data.number = number;
    }
    let id = core.ids.next();
    run(core, create(id, Some(layer.clone()), None, number.to_string(), NodeKind::Cel(data)))?;
    Ok(number)
}

pub(crate) fn set_cel_role(core: &mut CoreSession, cel: &NodeId, role: CelRole) -> Result<(), String> {
    run(core, Command::SetProps { id: cel.clone(), name: None, data: json!({ "role": role }) }).map(|_| ())
}

/// O que o clique na célula do timesheet faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exposure {
    /// Expõe o cel de número N a partir deste quadro.
    Cel(u32),
    /// Remove a mudança neste quadro: segura o que vinha antes.
    Hold,
    /// Quadro vazio.
    Empty,
}

/// Define a exposição da camada no quadro: cria a trilha (preservando a exposição estática de antes) e o cel, se faltarem.
pub(crate) fn set_exposure(core: &mut CoreSession, cut: &NodeId, layer: &NodeId, frame: u32, what: Exposure) -> Result<(), String> {
    let p = &core.project;
    let cd = cut_data(p, cut).ok_or_else(|| tr("err.no_cut").to_owned())?;
    let cels = layer_cels(p, layer);
    let mut cmds: Vec<Command> = Vec::new();
    let track_id = match cd.xsheet.track_for_layer(layer) {
        Some(t) => t.id.clone(),
        None => {
            if what == Exposure::Hold {
                return Ok(());
            }
            let tid = core.ids.next();
            let name = p.node(layer).map(|n| n.name.clone()).unwrap_or_else(|| "A".into());
            let mut track = Track::new(tid.clone(), &name, TrackKind::Drawing, Some(layer.clone()));
            // a camada estática expunha o menor cel em todos os quadros: a trilha começa igual
            if let Some((n, _)) = cels.first() {
                track.cells.insert(0, CellRef::Cel { number: *n });
            }
            cmds.push(Command::AddTrack { cut: cut.clone(), index: None, track });
            tid
        }
    };
    let cell = match what {
        Exposure::Cel(n) => {
            if !cels.iter().any(|(m, _)| *m == n) {
                let id = core.ids.next();
                cmds.push(create(id, Some(layer.clone()), None, n.to_string(), NodeKind::Cel(CelData::new(n))));
            }
            Some(CellRef::Cel { number: n })
        }
        Exposure::Empty => Some(CellRef::Empty),
        Exposure::Hold => None,
    };
    cmds.push(Command::SetCells { cut: cut.clone(), track: track_id, cells: vec![CellEdit { frame, cell }] });
    let label = tr("cmd.exposure").to_owned();
    run(core, Command::Batch { commands: cmds, label: Some(label) }).map(|_| ())
}

// ------------------------------------------------------------------------------------------------ preparo de cel

/// Comandos que garantem um cel exposto na camada/quadro (vazio quando já há um).
pub(crate) struct CelPrep {
    pub cmds: Vec<Command>,
}

/// Se a camada já expõe um cel no quadro, nada a fazer. Senão: sem nenhum cel cria o 1; com trilha que não expõe nada
/// aqui, cria/expõe o cel preferido (`preferred`) ou o próximo número.
pub(crate) fn prepare_cel(core: &mut CoreSession, cut: &NodeId, layer: &NodeId, frame: u32, preferred: Option<u32>) -> Result<CelPrep, String> {
    if exposed_cel(&core.project, cut, layer, frame).is_some() {
        return Ok(CelPrep { cmds: Vec::new() });
    }
    let p = &core.project;
    let cels = layer_cels(p, layer);
    let cd = cut_data(p, cut).ok_or_else(|| tr("err.no_cut").to_owned())?;
    let mut cmds = Vec::new();
    let (number, existing) = match preferred.and_then(|n| cels.iter().find(|(m, _)| *m == n)) {
        Some((n, _)) => (*n, Some(())),
        None => (preferred.unwrap_or_else(|| cels.last().map(|(n, _)| n + 1).unwrap_or(1)), None),
    };
    if existing.is_none() {
        let id = core.ids.next();
        cmds.push(create(id, Some(layer.clone()), None, number.to_string(), NodeKind::Cel(CelData::new(number))));
    }
    if let Some(t) = cd.xsheet.track_for_layer(layer) {
        cmds.push(Command::SetCells { cut: cut.clone(), track: t.id.clone(), cells: vec![CellEdit { frame, cell: Some(CellRef::Cel { number }) }] });
    }
    Ok(CelPrep { cmds })
}

/// Monta o comando principal depois de aplicar o preparo num projeto temporário (assim `paint_command`, `fill_command`
/// etc. enxergam o cel que ainda não existe) e devolve tudo como um único lote.
pub(crate) fn plan_with_prep<F>(core: &mut CoreSession, cut: &NodeId, layer: &NodeId, frame: u32, preferred: Option<u32>, extra_prep: Vec<Command>, make: F) -> Result<Command, String>
where
    F: FnOnce(&Project, &mut BlobStore) -> Result<Command, String>,
{
    let mut prep = prepare_cel(core, cut, layer, frame, preferred)?;
    let mut all = extra_prep;
    all.append(&mut prep.cmds);
    if all.is_empty() {
        return make(&core.project, &mut core.blobs);
    }
    let mut tmp = core.project.clone();
    {
        let ctx = ApplyCtx { blobs: &core.blobs, author: &core.author, revision: tmp.revision + 1 };
        for c in &all {
            c.apply(&mut tmp, &ctx).map_err(|e| e.to_string())?;
        }
    }
    let main = make(&tmp, &mut core.blobs)?;
    let label = main.description();
    all.push(main);
    Ok(Command::Batch { commands: all, label: Some(label) })
}

// ------------------------------------------------------------------------------------------------ paletas

pub(crate) fn add_palette(core: &mut CoreSession) -> Result<NodeId, String> {
    let n = core.project.roots.palettes.len() + 1;
    let id = core.ids.next();
    run(core, create(id.clone(), None, None, format!("{} {n}", tr("palette.name")), NodeKind::Palette(PaletteData::default())))?;
    Ok(id)
}

pub(crate) fn palette_data<'a>(p: &'a Project, id: &NodeId) -> Option<&'a PaletteData> {
    match &p.node(id)?.data {
        NodeKind::Palette(pd) => Some(pd),
        _ => None,
    }
}

/// Substitui os dados da paleta (ReplaceData): cores novas, renomeadas, editadas ou removidas.
pub(crate) fn replace_palette(core: &mut CoreSession, id: &NodeId, data: PaletteData) -> Result<(), String> {
    run(core, Command::ReplaceData { id: id.clone(), data: NodeKind::Palette(data) }).map(|_| ())
}

pub(crate) fn add_palette_color(core: &mut CoreSession, palette: &NodeId, rgba: Rgba) -> Result<NodeId, String> {
    let mut pd = palette_data(&core.project, palette).cloned().ok_or_else(|| tr("err.no_palette").to_owned())?;
    let id = core.ids.next();
    let name = format!("{} {}", tr("palette.color"), pd.colors.len() + 1);
    pd.colors.push(NamedColor { id: id.clone(), name, group: String::new(), role: "normal".into(), rgba, variants: Default::default() });
    replace_palette(core, palette, pd)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (CoreSession, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = quick_start(&mut core, 12, None).unwrap();
        (core, cut)
    }

    #[test]
    fn quick_start_is_one_undo_step_and_builds_the_chain() {
        let (mut core, cut) = session();
        assert!(is_cut(&core.project, &cut));
        assert_eq!(first_cut(&core.project), Some(cut));
        assert_eq!(core.log().len(), 1);
        core.undo().unwrap();
        assert!(core.project.nodes.is_empty());
    }

    #[test]
    fn layers_insert_above_the_active_one_with_a_first_cel() {
        let (mut core, cut) = session();
        let a = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        let b = add_layer(&mut core, &cut, LayerType::Raster, Some(&a)).unwrap();
        let f = add_layer(&mut core, &cut, LayerType::Fill, Some(&a)).unwrap();
        let rows = layer_rows(&core.project, &cut);
        let order: Vec<&NodeId> = rows.iter().map(|r| &r.id).collect();
        assert_eq!(order, vec![&b, &a, &f], "raster entra acima da ativa; o preenchimento entra ABAIXO da linha que ele referencia");
        assert_eq!(layer_cels(&core.project, &a).len(), 1);
        assert_eq!(layer_data(&core.project, &f).unwrap().line_refs, vec![a.clone()], "o balde nasce ligado à camada vetorial");
        assert!(core.project.node(&a).unwrap().name.ends_with(" 1"));
        // um passo de undo por camada (lote)
        let before = core.log().len();
        add_layer(&mut core, &cut, LayerType::Group, None).unwrap();
        assert_eq!(core.log().len(), before + 1);
    }

    #[test]
    fn move_toggle_and_delete_layers() {
        let (mut core, cut) = session();
        let a = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        let b = add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        assert_eq!(layer_rows(&core.project, &cut)[0].id, b);
        assert!(move_layer(&mut core, &b, -1).unwrap());
        assert_eq!(layer_rows(&core.project, &cut)[0].id, a);
        assert!(!move_layer(&mut core, &a, 1).unwrap(), "já está no topo");
        set_layer_props(&mut core, &a, json!({"visible": false, "opacity": 0.5, "blend": BlendMode::Multiply})).unwrap();
        let l = layer_data(&core.project, &a).unwrap();
        assert!(!l.visible && (l.opacity - 0.5).abs() < 1e-6 && l.blend == BlendMode::Multiply);
        delete_node(&mut core, &b).unwrap();
        assert_eq!(layer_rows(&core.project, &cut).len(), 1);
    }

    #[test]
    fn exposure_creates_track_and_cels_in_one_step() {
        let (mut core, cut) = session();
        let a = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        assert_eq!(add_cel(&mut core, &a, None).unwrap(), 2);
        let before = core.log().len();
        set_exposure(&mut core, &cut, &a, 4, Exposure::Cel(2)).unwrap();
        assert_eq!(core.log().len(), before + 1);
        assert_eq!(exposed_number(&core.project, &cut, &a, 0), Some(1), "a trilha preserva a exposição estática");
        assert_eq!(exposed_number(&core.project, &cut, &a, 3), Some(1));
        assert_eq!(exposed_number(&core.project, &cut, &a, 4), Some(2));
        set_exposure(&mut core, &cut, &a, 8, Exposure::Empty).unwrap();
        assert_eq!(exposed_number(&core.project, &cut, &a, 9), None);
        set_exposure(&mut core, &cut, &a, 8, Exposure::Hold).unwrap();
        assert_eq!(exposed_number(&core.project, &cut, &a, 9), Some(2), "segurar remove a mudança");
        // cel que ainda não existe é criado junto
        set_exposure(&mut core, &cut, &a, 10, Exposure::Cel(5)).unwrap();
        assert_eq!(layer_cels(&core.project, &a).len(), 3);
        core.undo().unwrap();
        assert_eq!(layer_cels(&core.project, &a).len(), 2, "o undo desfaz exposição e cel juntos");
    }

    #[test]
    fn prepare_cel_creates_the_missing_cel_and_plan_batches_it() {
        let (mut core, cut) = session();
        let a = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        // apaga o cel 1: a camada fica sem nada exposto
        let cel1 = layer_cels(&core.project, &a)[0].1.clone();
        delete_node(&mut core, &cel1).unwrap();
        assert!(exposed_cel(&core.project, &cut, &a, 0).is_none());
        let before = core.log().len();
        let cmd = plan_with_prep(&mut core, &cut, &a, 0, None, vec![], |p, _| {
            let cel = exposed_cel(p, &cut, &a, 0).expect("o projeto temporário já tem o cel");
            Ok(Command::InsertStrokes { cel, items: vec![] })
        })
        .unwrap();
        run(&mut core, cmd).unwrap();
        assert_eq!(core.log().len(), before + 1);
        assert_eq!(layer_cels(&core.project, &a).len(), 1);
    }

    #[test]
    fn palettes_get_named_colors() {
        let (mut core, _) = session();
        let pal = add_palette(&mut core).unwrap();
        let c = add_palette_color(&mut core, &pal, Rgba::rgb8(200, 30, 30)).unwrap();
        let pd = palette_data(&core.project, &pal).unwrap();
        assert_eq!(pd.colors.len(), 1);
        let cref = ColorRef::Palette { palette: pal, color: c };
        assert_eq!(resolve_color(&core.project, &cref).to_hex(), "#c81e1eff");
    }

    #[test]
    fn rename_is_a_no_op_when_unchanged() {
        let (mut core, cut) = session();
        let n = core.log().len();
        rename_node(&mut core, &cut, "  ").unwrap();
        let name = core.project.node(&cut).unwrap().name.clone();
        rename_node(&mut core, &cut, &name).unwrap();
        assert_eq!(core.log().len(), n);
        rename_node(&mut core, &cut, "Corte novo").unwrap();
        assert_eq!(core.project.node(&cut).unwrap().name, "Corte novo");
    }
}
