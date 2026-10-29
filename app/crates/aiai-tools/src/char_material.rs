//! Pasta do personagem (S24 2.5): os materiais de um personagem (folha de modelo, expressões, tabela de boca, olhos,
//! figurinos e folhas extras) como cortes-canvas filhos dele, listados em `CharacterData.materials`. Criar a partir de
//! um modelo (grade de células com rótulo), importar uma imagem, renomear, duplicar, apagar, reordenar e anotar: cada
//! operação devolve UM comando (`Batch`) para a sessão aplicar, então entra no undo e roda igual no app, na
//! bridge/MCP e no headless.
//!
//! Projetos antigos não têm a lista: [`materials`] devolve a lista guardada mais os cortes soltos do personagem (o da
//! folha de modelo como `model_sheet`, os outros como `other`), sem mexer no documento; a primeira mudança grava a
//! lista inteira.

use crate::character::{ModelSheetRequest, build_sheet};
use crate::context::*;
use aiai_audio::lipsync::MouthShape;
use aiai_core::*;
use aiai_paint::text_layer::{TextAlign, TextParams};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Maior lado (px) do corte de uma imagem importada (acima disso a imagem é reduzida).
pub const IMPORT_MAX_SIDE: u32 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaterialAction {
    /// Lista os materiais do personagem (não altera nada).
    #[default]
    List,
    /// Cria um material novo do `kind` (a partir de um modelo com células).
    Create,
    /// Cria um material com a imagem de `path` (um arquivo do disco).
    Import,
    Rename,
    /// Copia o material (e a paleta, se for figurino).
    Duplicate,
    Delete,
    /// Muda a posição na lista (`index`, 0-based).
    Move,
    /// Muda notas, nome e paleta (`notes`, `name`, `palette`).
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CharMaterialRequest {
    /// Personagem (nó `character`).
    pub character: Id,
    /// Padrão: `list`.
    #[serde(default)]
    pub action: MaterialAction,
    /// Material alvo (rename, duplicate, delete, move, update).
    #[serde(default)]
    pub material: Option<Id>,
    /// Tipo do material novo: model_sheet, expressions, mouth_chart, eyes, outfit, other (create; padrão other). Em import vale o mesmo.
    #[serde(default)]
    pub kind: Option<MaterialKind>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Arquivo de imagem (import): PNG, JPEG, WebP, BMP, TIFF, GIF.
    #[serde(default)]
    pub path: Option<String>,
    /// Posição (move) ou onde o material novo entra (create/import/duplicate; padrão: no fim, o duplicado logo depois do original).
    #[serde(default)]
    pub index: Option<usize>,
    /// Rótulos das células do modelo (da esquerda para a direita, de cima para baixo); o que faltar usa o padrão em português.
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    /// Paleta do figurino (update). Em create de figurino, vale no lugar da cópia automática da paleta do personagem.
    #[serde(default)]
    pub palette: Option<Id>,
    /// Folha de modelo/figurino: vistas (graus de rotação) e altura da figura em px.
    #[serde(default)]
    pub views: Option<Vec<f32>>,
    #[serde(default)]
    pub figure_px: Option<f32>,
    /// Folha extra (`other`): tamanho do canvas em px.
    #[serde(default)]
    pub size: Option<[u32; 2]>,
    /// Figurino: criar uma variante de paleta copiando a do personagem (padrão true).
    #[serde(default)]
    pub new_palette: Option<bool>,
}

/// Resultado de uma operação: o comando (nada em `list`), o material mexido e o que mais ajuda a desenhar nele.
pub struct CharMaterialPlan {
    pub command: Option<Command>,
    pub material: Option<Id>,
    pub cut: Option<Id>,
    pub line_layer: Option<Id>,
    pub fill_layer: Option<Id>,
    /// Células do modelo: rótulo e retângulo [x, y, w, h] no canvas.
    pub cells: Vec<([f32; 4], String)>,
    pub canvas: Option<[u32; 2]>,
    /// A lista depois da operação (em `list`, a lista atual).
    pub list: Vec<CharMaterial>,
}

fn character<'a>(p: &'a Project, id: &Id) -> ToolResult<&'a CharacterData> {
    match &node(p, id)?.data {
        NodeKind::Character(c) => Ok(c),
        other => Err(ToolError::Invalid(format!("{id} é {}, não personagem", other.type_name()))),
    }
}

/// Os materiais do personagem: a lista guardada (sem os de corte que sumiu) e depois os cortes soltos dele (projetos
/// antigos: a folha de modelo é `model_sheet`, os outros `other`; o id é `legacy-<corte>`).
pub fn materials(p: &Project, ch: &Id) -> Vec<CharMaterial> {
    let Some(NodeKind::Character(c)) = p.node(ch).map(|n| &n.data) else { return Vec::new() };
    let is_cut = |id: &Id| matches!(p.node(id).map(|n| &n.data), Some(NodeKind::Cut(_)));
    let mut out: Vec<CharMaterial> = c.materials.iter().filter(|m| is_cut(m.content.cut())).cloned().collect();
    let sheet = c.measurements.get("sheet_cut").and_then(Value::as_str).map(|s| Id(s.to_owned()));
    let children = p.node(ch).map(|n| n.children.clone()).unwrap_or_default();
    for k in children.iter().filter(|k| is_cut(k)) {
        if out.iter().any(|m| m.content.cut() == k) {
            continue;
        }
        let kind = if sheet.as_ref() == Some(k) { MaterialKind::ModelSheet } else { MaterialKind::Other };
        let name = p.node(k).map(|n| n.name.clone()).unwrap_or_default();
        out.push(CharMaterial { id: Id(format!("legacy-{}", k.0)), kind, name, content: MaterialContent::Sheet { cut: k.clone() }, notes: String::new(), palette: None });
    }
    out
}

/// Material por id (inclusive os `legacy-*`).
pub fn material(p: &Project, ch: &Id, id: &Id) -> Option<CharMaterial> {
    materials(p, ch).into_iter().find(|m| &m.id == id)
}

/// Material que mostra o corte (para a interface: "este corte é qual material?").
pub fn material_of_cut(p: &Project, ch: &Id, cut: &Id) -> Option<CharMaterial> {
    materials(p, ch).into_iter().find(|m| m.content.cut() == cut)
}

/// Nome padrão de um material novo (em português; a interface manda o nome traduzido).
pub fn default_name(kind: MaterialKind, existing: &[CharMaterial]) -> String {
    let base = match kind {
        MaterialKind::ModelSheet => "Folha de modelo",
        MaterialKind::Expressions => "Expressões",
        MaterialKind::MouthChart => "Tabela de boca",
        MaterialKind::Eyes => "Olhos",
        MaterialKind::Outfit => "Figurino",
        MaterialKind::Other => "Folha extra",
    };
    name_with(base, kind, existing)
}

/// `base` com número quando já existe material do tipo ("Figurino 2"); folha de modelo, expressões, boca e olhos só
/// ganham número a partir do segundo, figurino e folha extra já saem numerados.
pub fn name_with(base: &str, kind: MaterialKind, existing: &[CharMaterial]) -> String {
    let n = existing.iter().filter(|m| m.kind == kind).count();
    if n == 0 && !matches!(kind, MaterialKind::Outfit | MaterialKind::Other) { base.to_owned() } else { format!("{base} {}", n + 1) }
}

/// Rótulos padrão de uma folha de expressões (4 × 3).
pub const EXPRESSION_LABELS: [&str; 12] = ["neutro", "alegre", "triste", "bravo", "surpreso", "vergonha", "medo", "cansado", "maroto", "chorando", "sono", "nojo"];
/// Rótulos padrão do detalhe dos olhos (3 × 3).
pub const EYE_LABELS: [&str; 9] = ["aberto", "meio", "fechado", "piscada", "esquerda", "direita", "cima", "baixo", "brilho"];

/// As formas de boca do lip-sync (a ordem das células da tabela): as mesmas de `aiai_audio::lipsync::MouthShape`.
pub const MOUTH_SHAPES: [MouthShape; 9] = [MouthShape::Rest, MouthShape::A, MouthShape::I, MouthShape::U, MouthShape::E, MouthShape::O, MouthShape::N, MouthShape::Half, MouthShape::Open];

/// Rótulo padrão de uma forma de boca ("A あ", "N ん (fechada)"...).
pub fn mouth_label(s: MouthShape) -> &'static str {
    match s {
        MouthShape::Rest => "repouso",
        MouthShape::A => "A あ",
        MouthShape::I => "I い",
        MouthShape::U => "U う",
        MouthShape::E => "E え",
        MouthShape::O => "O お",
        MouthShape::N => "N ん (fechada)",
        MouthShape::Half => "meia",
        MouthShape::Open => "aberta",
    }
}

/// Modelo de uma folha com células: colunas × linhas, tamanho da célula, rótulos padrão e guias de rosto.
struct Template {
    cols: u32,
    rows: u32,
    cell: [u32; 2],
    labels: Vec<String>,
    /// Linha dos olhos (fração da altura da célula) e eixo central, só nas folhas de rosto.
    face_guides: bool,
}

fn template(kind: MaterialKind, size: Option<[u32; 2]>) -> Option<Template> {
    Some(match kind {
        MaterialKind::Expressions => Template { cols: 4, rows: 3, cell: [512, 512], labels: EXPRESSION_LABELS.iter().map(|s| (*s).to_owned()).collect(), face_guides: true },
        MaterialKind::MouthChart => Template { cols: 3, rows: 3, cell: [448, 448], labels: MOUTH_SHAPES.iter().map(|s| mouth_label(*s).to_owned()).collect(), face_guides: false },
        MaterialKind::Eyes => Template { cols: 3, rows: 3, cell: [640, 320], labels: EYE_LABELS.iter().map(|s| (*s).to_owned()).collect(), face_guides: true },
        MaterialKind::Other => {
            let [w, h] = size.unwrap_or([1920, 1080]);
            Template { cols: 1, rows: 1, cell: [w.clamp(64, 8192), h.clamp(64, 8192)], labels: Vec::new(), face_guides: false }
        }
        _ => return None,
    })
}

fn guide_line(pts: Vec<[f32; 2]>, width: f32, hex: &str) -> Stroke {
    Stroke { taper: None,
        id: Id(String::new()),
        points: pts.into_iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
        geometry: StrokeGeometry::Polyline,
        tension: 0.0,
        tip: VectorTip::Marker,
        width,
        profile: WidthProfile::default(),
        color: ColorRef::Direct { rgba: Rgba::from_hex(hex).expect("cor fixa") },
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
        group: String::new(),
    }
}

/// Camada nova com um cel de número 1 (traços e imagem opcionais).
fn push_layer(cmds: &mut Vec<Command>, idgen: &mut IdGen, parent: &Id, name: &str, data: LayerData, strokes: Vec<Stroke>, image: Option<BlobHash>) -> Id {
    let l = idgen.next();
    cmds.push(Command::CreateNode { id: Some(l.clone()), parent: Some(parent.clone()), index: None, name: name.into(), data: NodeKind::Layer(data) });
    let mut cel = CelData::new(1);
    cel.strokes = strokes;
    cel.image = image;
    cmds.push(Command::CreateNode { id: Some(idgen.next()), parent: Some(l.clone()), index: None, name: format!("{name}1"), data: NodeKind::Cel(cel) });
    l
}

struct Built {
    cmds: Vec<Command>,
    cut: Id,
    line: Id,
    fill: Id,
    cells: Vec<([f32; 4], String)>,
    canvas: [u32; 2],
}

/// Corte de um modelo com células: grade e guias, rótulos (camadas de texto numa pasta), cor e linha.
fn build_template(idgen: &mut IdGen, ch: &Id, name: &str, t: &Template, labels: Option<&[String]>) -> Built {
    let (cw, chh) = (t.cell[0] as f32, t.cell[1] as f32);
    let (w, h) = (t.cell[0] * t.cols, t.cell[1] * t.rows);
    let cut = idgen.next();
    let mut cd = CutData::new(1);
    cd.canvas = Some([w, h]);
    let mut cmds = vec![Command::CreateNode { id: Some(cut.clone()), parent: Some(ch.clone()), index: None, name: name.to_owned(), data: NodeKind::Cut(cd) }];
    let mut grid = Vec::new();
    for c in 0..=t.cols {
        let x = c as f32 * cw;
        grid.push(guide_line(vec![[x, 0.0], [x, h as f32]], 3.0, "#5b8fd0c8"));
    }
    for r in 0..=t.rows {
        let y = r as f32 * chh;
        grid.push(guide_line(vec![[0.0, y], [w as f32, y]], 3.0, "#5b8fd0c8"));
    }
    let mut cells = Vec::new();
    for r in 0..t.rows {
        for c in 0..t.cols {
            let (x, y) = (c as f32 * cw, r as f32 * chh);
            let i = (r * t.cols + c) as usize;
            let label = labels.and_then(|l| l.get(i)).cloned().or_else(|| t.labels.get(i).cloned()).unwrap_or_default();
            if t.face_guides {
                grid.push(guide_line(vec![[x + cw / 2.0, y + chh * 0.08], [x + cw / 2.0, y + chh * 0.92]], 1.0, "#5b8fd090"));
                grid.push(guide_line(vec![[x + cw * 0.08, y + chh * 0.45], [x + cw * 0.92, y + chh * 0.45]], 1.5, "#e08a4ab0"));
            } else if t.cols * t.rows > 1 {
                grid.push(guide_line(vec![[x + cw / 2.0, y + chh * 0.1], [x + cw / 2.0, y + chh * 0.9]], 1.5, "#5b8fd080"));
            }
            cells.push(([x, y, cw, chh], label));
        }
    }
    push_layer(&mut cmds, idgen, &cut, "guia-grade", LayerData::new(LayerType::Guide), grid, None);
    if cells.iter().any(|(_, l)| !l.is_empty()) {
        let group = idgen.next();
        cmds.push(Command::CreateNode { id: Some(group.clone()), parent: Some(cut.clone()), index: None, name: "rótulos".into(), data: NodeKind::Layer(LayerData::new(LayerType::Group)) });
        let size = (chh * 0.075).clamp(18.0, 40.0);
        for ([x, y, _, _], label) in cells.iter().filter(|(_, l)| !l.is_empty()) {
            let params = TextParams { content: label.clone(), size, pos: [x + 16.0, y + 10.0], align: TextAlign::Left, color: ColorRef::Direct { rgba: Rgba::from_hex("#3b4a63").expect("cor fixa") }, ..Default::default() }.sanitized();
            let mut data = LayerData::new(LayerType::Text);
            data.params = json!({ TextParams::KEY: params.to_value() });
            cmds.push(Command::CreateNode { id: Some(idgen.next()), parent: Some(group.clone()), index: None, name: label.clone(), data: NodeKind::Layer(data) });
        }
    }
    let fill = push_layer(&mut cmds, idgen, &cut, "cor", LayerData::new(LayerType::Fill), Vec::new(), None);
    let line = push_layer(&mut cmds, idgen, &cut, "linha", LayerData::new(LayerType::Vector), Vec::new(), None);
    cmds.push(Command::SetProps { id: fill.clone(), name: None, data: json!({"line_refs": [line.0]}) });
    Built { cmds, cut, line, fill, cells, canvas: [w, h] }
}

/// Corte de uma imagem importada: a imagem numa camada de referência travada e a linha por cima para anotar/decalcar.
fn build_import(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, ch: &Id, name: &str, path: &str) -> ToolResult<(Built, BlobHash)> {
    let _ = p;
    let dim = ::image::image_dimensions(path).map_err(|e| ToolError::Invalid(format!("não consegui abrir {path}: {e}")))?;
    let (iw, ih) = (dim.0.max(1), dim.1.max(1));
    let s = (IMPORT_MAX_SIDE as f32 / iw.max(ih) as f32).min(1.0);
    let (w, h) = (((iw as f32 * s).round() as u32).max(1), ((ih as f32 * s).round() as u32).max(1));
    let img = crate::import::fitted_image(path, w, h, crate::import::ImageFit::Contain)?;
    let png = img.encode_png().map_err(ToolError::Render)?;
    let hash = blobs.put(png);
    let cut = idgen.next();
    let mut cd = CutData::new(1);
    cd.canvas = Some([w, h]);
    let mut cmds = vec![Command::CreateNode { id: Some(cut.clone()), parent: Some(ch.clone()), index: None, name: name.to_owned(), data: NodeKind::Cut(cd) }];
    let mut refl = LayerData::new(LayerType::Reference);
    refl.locked = true;
    push_layer(&mut cmds, idgen, &cut, "imagem", refl, Vec::new(), Some(hash.clone()));
    let line = push_layer(&mut cmds, idgen, &cut, "linha", LayerData::new(LayerType::Vector), Vec::new(), None);
    Ok((Built { cmds, cut: cut.clone(), line: line.clone(), fill: line, cells: Vec::new(), canvas: [w, h] }, hash))
}

/// Palette nova para um figurino: cópia da paleta do personagem (mesmos ids de cor) ou vazia.
fn outfit_palette(p: &Project, idgen: &mut IdGen, ch: &Id, name: &str, cmds: &mut Vec<Command>) -> Id {
    let data = character(p, ch).ok().and_then(|c| c.palette.as_ref()).and_then(|pal| p.node(pal)).and_then(|n| match &n.data {
        NodeKind::Palette(d) => Some(d.clone()),
        _ => None,
    });
    let id = idgen.next();
    let who = p.node(ch).map(|n| n.name.clone()).unwrap_or_default();
    cmds.push(Command::CreateNode { id: Some(id.clone()), parent: None, index: None, name: format!("{who} · {name}"), data: NodeKind::Palette(data.unwrap_or_default()) });
    id
}

fn insert_at(list: &mut Vec<CharMaterial>, m: CharMaterial, index: Option<usize>) {
    let at = index.unwrap_or(list.len()).min(list.len());
    list.insert(at, m);
}

fn set_list(ch: &Id, list: &[CharMaterial]) -> Command {
    Command::SetProps { id: ch.clone(), name: None, data: json!({ "materials": list }) }
}

fn batch(cmds: Vec<Command>, label: String) -> Command {
    Command::Batch { commands: cmds, label: Some(label) }
}

fn clean_name(req: &Option<String>) -> ToolResult<Option<String>> {
    match req.as_deref().map(str::trim) {
        Some("") => Err(ToolError::Invalid("o nome não pode ser vazio".into())),
        Some(n) => Ok(Some(n.chars().take(80).collect())),
        None => Ok(None),
    }
}

/// Copia a subárvore de um corte para outro corte novo (ids novos; referências entre os nós copiados, como as linhas
/// de uma camada de cor ou as trilhas do timesheet, seguem para os ids novos).
pub fn clone_cut(p: &Project, idgen: &mut IdGen, cut: &Id, parent: &Id, name: &str) -> ToolResult<(Vec<Command>, Id)> {
    cut_data(p, cut)?;
    let ids = p.subtree(cut);
    let map: std::collections::BTreeMap<Id, Id> = ids.iter().map(|i| (i.clone(), idgen.next())).collect();
    fn remap(v: &mut Value, map: &std::collections::BTreeMap<Id, Id>) {
        match v {
            Value::String(s) => {
                if let Some(n) = map.get(&Id(s.clone())) {
                    *s = n.0.clone();
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| remap(x, map)),
            Value::Object(o) => o.values_mut().for_each(|x| remap(x, map)),
            _ => {}
        }
    }
    let mut cmds = Vec::new();
    for old in &ids {
        let n = node(p, old)?;
        let mut data = serde_json::to_value(&n.data).map_err(|e| ToolError::Invalid(e.to_string()))?;
        // o `id` do traço e os demais campos não são ids de nó: só as strings iguais a um id copiado mudam
        remap(&mut data, &map);
        let data: NodeKind = serde_json::from_value(data).map_err(|e| ToolError::Invalid(e.to_string()))?;
        let (par, nm) = if old == cut { (Some(parent.clone()), name.to_owned()) } else { (n.parent.as_ref().and_then(|x| map.get(x)).cloned(), n.name.clone()) };
        cmds.push(Command::CreateNode { id: Some(map[old].clone()), parent: par, index: None, name: nm, data });
    }
    // o corte novo não herda a ligação com roteiro/storyboard de um corte de cena (as folhas não têm, mas por garantia)
    Ok((cmds, map[cut].clone()))
}

pub fn plan(p: &Project, idgen: &mut IdGen, blobs: &mut BlobStore, req: &CharMaterialRequest) -> ToolResult<CharMaterialPlan> {
    let ch = &req.character;
    let cdata = character(p, ch)?;
    let list = materials(p, ch);
    let empty = |list: Vec<CharMaterial>| CharMaterialPlan { command: None, material: None, cut: None, line_layer: None, fill_layer: None, cells: Vec::new(), canvas: None, list };
    let target = |req: &CharMaterialRequest| -> ToolResult<usize> {
        let id = req.material.as_ref().ok_or_else(|| ToolError::Invalid("Missing 'material': informe o id do material (veja action list)".into()))?;
        list.iter().position(|m| &m.id == id).ok_or_else(|| ToolError::NotFound(format!("material {id}")))
    };
    let name = clean_name(&req.name)?;
    match req.action {
        MaterialAction::List => Ok(empty(list)),
        MaterialAction::Create => {
            let kind = req.kind.unwrap_or_default();
            let nm = name.unwrap_or_else(|| default_name(kind, &list));
            let mut out = list.clone();
            let mid = idgen.next();
            let (mut cmds, built_cut, line, fill, cells, canvas, palette): (Vec<Command>, Id, Option<Id>, Option<Id>, Vec<([f32; 4], String)>, Option<[u32; 2]>, Option<Id>) = match kind {
                MaterialKind::ModelSheet | MaterialKind::Outfit => {
                    let mut pre = Vec::new();
                    let palette = if kind == MaterialKind::Outfit {
                        match (&req.palette, req.new_palette.unwrap_or(true)) {
                            (Some(pal), _) => Some(pal.clone()),
                            (None, true) => Some(outfit_palette(p, idgen, ch, &nm, &mut pre)),
                            (None, false) => None,
                        }
                    } else {
                        None
                    };
                    let views = req.views.clone().unwrap_or_else(|| if kind == MaterialKind::Outfit { vec![0.0, 90.0, 180.0] } else { vec![0.0, 45.0, 90.0, 135.0, 180.0] });
                    let sheet = build_sheet(p, idgen, &ModelSheetRequest { character: ch.clone(), name: nm.clone(), heads_tall: cdata.heads_tall, figure_px: req.figure_px.unwrap_or(800.0), views, pose: None })?;
                    pre.extend(sheet.commands);
                    // só a folha de modelo grava as medidas (e vira a folha principal); o figurino não mexe nelas
                    if kind == MaterialKind::ModelSheet {
                        pre.push(Command::SetProps { id: ch.clone(), name: None, data: json!({"measurements": sheet.measurements}) });
                    }
                    (pre, sheet.cut, Some(sheet.line_layer), Some(sheet.fill_layer), Vec::new(), None, palette)
                }
                _ => {
                    let t = template(kind, req.size).ok_or_else(|| ToolError::Invalid("tipo sem modelo".into()))?;
                    let b = build_template(idgen, ch, &nm, &t, req.labels.as_deref());
                    (b.cmds, b.cut, Some(b.line), Some(b.fill), b.cells, Some(b.canvas), None)
                }
            };
            insert_at(&mut out, CharMaterial { id: mid.clone(), kind, name: nm.clone(), content: MaterialContent::Sheet { cut: built_cut.clone() }, notes: req.notes.clone().unwrap_or_default(), palette }, req.index);
            cmds.push(set_list(ch, &out));
            Ok(CharMaterialPlan { command: Some(batch(cmds, format!("Material \"{nm}\""))), material: Some(mid), cut: Some(built_cut), line_layer: line, fill_layer: fill, cells, canvas, list: out })
        }
        MaterialAction::Import => {
            let path = req.path.as_deref().ok_or_else(|| ToolError::Invalid("Missing 'path': o arquivo de imagem".into()))?;
            let kind = req.kind.unwrap_or_default();
            let file_stem = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty());
            let nm = name.or(file_stem).unwrap_or_else(|| default_name(kind, &list));
            let (b, hash) = build_import(p, idgen, blobs, ch, &nm, path)?;
            let mid = idgen.next();
            let mut out = list.clone();
            insert_at(&mut out, CharMaterial { id: mid.clone(), kind, name: nm.clone(), content: MaterialContent::Image { blob: hash, cut: b.cut.clone() }, notes: req.notes.clone().unwrap_or_default(), palette: None }, req.index);
            let mut cmds = b.cmds;
            cmds.push(set_list(ch, &out));
            Ok(CharMaterialPlan { command: Some(batch(cmds, format!("Importar \"{nm}\""))), material: Some(mid), cut: Some(b.cut), line_layer: Some(b.line), fill_layer: None, cells: Vec::new(), canvas: Some(b.canvas), list: out })
        }
        MaterialAction::Rename => {
            let i = target(req)?;
            let nm = name.ok_or_else(|| ToolError::Invalid("Missing 'name': o nome novo".into()))?;
            let mut out = list.clone();
            out[i].name = nm.clone();
            let cut = out[i].content.cut().clone();
            let cmds = vec![Command::SetProps { id: cut.clone(), name: Some(nm.clone()), data: Value::Null }, set_list(ch, &out)];
            Ok(CharMaterialPlan { command: Some(batch(cmds, format!("Renomear material para \"{nm}\""))), material: Some(out[i].id.clone()), cut: Some(cut), line_layer: None, fill_layer: None, cells: Vec::new(), canvas: None, list: out })
        }
        MaterialAction::Update => {
            let i = target(req)?;
            let mut out = list.clone();
            let mut cmds = Vec::new();
            if let Some(nm) = name {
                out[i].name = nm.clone();
                cmds.push(Command::SetProps { id: out[i].content.cut().clone(), name: Some(nm), data: Value::Null });
            }
            if let Some(n) = &req.notes {
                out[i].notes = n.clone();
            }
            if let Some(pal) = &req.palette {
                if !matches!(node(p, pal)?.data, NodeKind::Palette(_)) {
                    return Err(ToolError::Invalid(format!("{pal} não é paleta")));
                }
                out[i].palette = Some(pal.clone());
            }
            if let Some(k) = req.kind {
                out[i].kind = k;
            }
            cmds.push(set_list(ch, &out));
            let cut = out[i].content.cut().clone();
            Ok(CharMaterialPlan { command: Some(batch(cmds, "Editar material".into())), material: Some(out[i].id.clone()), cut: Some(cut), line_layer: None, fill_layer: None, cells: Vec::new(), canvas: None, list: out })
        }
        MaterialAction::Move => {
            let i = target(req)?;
            let to = req.index.ok_or_else(|| ToolError::Invalid("Missing 'index': a posição nova (0-based)".into()))?;
            let mut out = list.clone();
            let m = out.remove(i);
            let at = to.min(out.len());
            let id = m.id.clone();
            out.insert(at, m);
            Ok(CharMaterialPlan { command: Some(batch(vec![set_list(ch, &out)], "Mover material".into())), material: Some(id), cut: None, line_layer: None, fill_layer: None, cells: Vec::new(), canvas: None, list: out })
        }
        MaterialAction::Duplicate => {
            let i = target(req)?;
            let src = list[i].clone();
            let nm = name.unwrap_or_else(|| format!("{} (cópia)", src.name));
            let (mut cmds, new_cut) = clone_cut(p, idgen, src.content.cut(), ch, &nm)?;
            let palette = match (&src.palette, src.kind == MaterialKind::Outfit) {
                (Some(pal), true) => {
                    // a variante de paleta também é copiada: o figurino novo se pinta sem mexer no original
                    match p.node(pal).map(|n| (&n.data, n.name.clone())) {
                        Some((NodeKind::Palette(d), pname)) => {
                            let id = idgen.next();
                            cmds.insert(0, Command::CreateNode { id: Some(id.clone()), parent: None, index: None, name: format!("{pname} (cópia)"), data: NodeKind::Palette(d.clone()) });
                            Some(id)
                        }
                        _ => None,
                    }
                }
                (pal, _) => pal.clone(),
            };
            let content = match &src.content {
                MaterialContent::Sheet { .. } => MaterialContent::Sheet { cut: new_cut.clone() },
                MaterialContent::Image { blob, .. } => MaterialContent::Image { blob: blob.clone(), cut: new_cut.clone() },
            };
            let mid = idgen.next();
            let mut out = list.clone();
            insert_at(&mut out, CharMaterial { id: mid.clone(), kind: src.kind, name: nm.clone(), content, notes: src.notes.clone(), palette }, Some(req.index.unwrap_or(i + 1)));
            cmds.push(set_list(ch, &out));
            Ok(CharMaterialPlan { command: Some(batch(cmds, format!("Duplicar material \"{}\"", src.name))), material: Some(mid), cut: Some(new_cut), line_layer: None, fill_layer: None, cells: Vec::new(), canvas: None, list: out })
        }
        MaterialAction::Delete => {
            let i = target(req)?;
            let mut out = list.clone();
            let m = out.remove(i);
            let cut = m.content.cut().clone();
            let cmds = vec![set_list(ch, &out), Command::DeleteNode { id: cut }];
            Ok(CharMaterialPlan { command: Some(batch(cmds, format!("Apagar material \"{}\"", m.name))), material: Some(m.id), cut: None, line_layer: None, fill_layer: None, cells: Vec::new(), canvas: None, list: out })
        }
    }
}

/// O material como JSON para o MCP/bridge.
pub fn material_json(m: &CharMaterial) -> Value {
    let (imported, blob) = match &m.content {
        MaterialContent::Image { blob, .. } => (true, Some(blob.clone())),
        MaterialContent::Sheet { .. } => (false, None),
    };
    json!({ "id": m.id, "kind": m.kind.key(), "name": m.name, "cut": m.content.cut(), "imported": imported, "blob": blob, "notes": m.notes, "palette": m.palette })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (CoreSession, Id) {
        let mut s = CoreSession::for_tests("t");
        let ch = s.ids.next();
        s.apply(Command::CreateNode { id: Some(ch.clone()), parent: None, index: None, name: "Hana".into(), data: NodeKind::Character(CharacterData { heads_tall: 6.0, ..Default::default() }) }).unwrap();
        (s, ch)
    }

    fn run(s: &mut CoreSession, req: CharMaterialRequest) -> CharMaterialPlan {
        let pl = plan(&s.project, &mut s.ids, &mut s.blobs, &req).unwrap();
        if let Some(c) = pl.command.clone() {
            s.apply(c).unwrap();
        }
        pl
    }

    fn req(ch: &Id, action: MaterialAction, kind: Option<MaterialKind>) -> CharMaterialRequest {
        CharMaterialRequest { character: ch.clone(), action, kind, ..Default::default() }
    }

    #[test]
    fn templates_make_labeled_cells_with_drawable_layers() {
        let (mut s, ch) = session();
        for (kind, cells) in [(MaterialKind::Expressions, 12), (MaterialKind::MouthChart, 9), (MaterialKind::Eyes, 9), (MaterialKind::Other, 1)] {
            let pl = run(&mut s, req(&ch, MaterialAction::Create, Some(kind)));
            assert_eq!(pl.cells.len(), cells, "{kind:?}");
            let cut = pl.cut.unwrap();
            s.project.validate().unwrap();
            let names: Vec<String> = s.project.subtree(&cut).iter().filter_map(|i| s.project.node(i)).map(|n| n.name.clone()).collect();
            assert!(names.contains(&"linha".to_owned()) && names.contains(&"cor".to_owned()) && names.contains(&"guia-grade".to_owned()), "{names:?}");
            if kind != MaterialKind::Other {
                // um rótulo de texto por célula, numa pasta
                assert_eq!(s.project.subtree(&cut).iter().filter_map(|i| s.project.node(i)).filter(|n| matches!(&n.data, NodeKind::Layer(l) if l.kind == LayerType::Text)).count(), cells);
            }
        }
        let list = materials(&s.project, &ch);
        assert_eq!(list.iter().map(|m| m.kind).collect::<Vec<_>>(), vec![MaterialKind::Expressions, MaterialKind::MouthChart, MaterialKind::Eyes, MaterialKind::Other]);
        // a tabela de boca tem uma célula por forma do lip-sync, na ordem das formas
        let mouth = run(&mut s, req(&ch, MaterialAction::Create, Some(MaterialKind::MouthChart)));
        assert_eq!(mouth.cells.len(), MOUTH_SHAPES.len());
        assert!(mouth.cells.iter().any(|(_, l)| l.starts_with('A')) && mouth.cells[0].1 == "repouso");
    }

    #[test]
    fn custom_labels_replace_the_defaults() {
        let (mut s, ch) = session();
        let mut r = req(&ch, MaterialAction::Create, Some(MaterialKind::Expressions));
        r.labels = Some(vec!["neutral".into(), "happy".into()]);
        let pl = run(&mut s, r);
        assert_eq!(pl.cells[0].1, "neutral");
        assert_eq!(pl.cells[1].1, "happy");
        assert_eq!(pl.cells[2].1, "triste");
    }

    #[test]
    fn model_sheet_and_outfit_have_their_own_rules() {
        let (mut s, ch) = session();
        let sheet = run(&mut s, req(&ch, MaterialAction::Create, Some(MaterialKind::ModelSheet)));
        let sheet_cut = sheet.cut.clone().unwrap();
        let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
        assert_eq!(c.measurements["sheet_cut"], json!(sheet_cut.0));
        // figurino: folha própria e variante de paleta, sem tocar nas medidas
        let mut r = req(&ch, MaterialAction::Create, Some(MaterialKind::Outfit));
        r.name = Some("Uniforme".into());
        let out = run(&mut s, r);
        let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
        assert_eq!(c.measurements["sheet_cut"], json!(sheet_cut.0));
        let m = c.materials.iter().find(|m| m.kind == MaterialKind::Outfit).unwrap();
        let pal = m.palette.clone().expect("variante de paleta");
        assert!(matches!(s.project.node(&pal).unwrap().data, NodeKind::Palette(_)));
        assert_eq!(s.project.node(&pal).unwrap().name, "Hana · Uniforme");
        s.project.validate().unwrap();
        // duplicar o figurino copia a folha e a paleta
        let mut d = req(&ch, MaterialAction::Duplicate, None);
        d.material = out.material.clone();
        let dup = run(&mut s, d);
        assert_ne!(dup.cut, out.cut);
        let copy = materials(&s.project, &ch).into_iter().find(|m| Some(&m.id) == dup.material.as_ref()).unwrap();
        assert_eq!(copy.name, "Uniforme (cópia)");
        assert_ne!(copy.palette, Some(pal));
        s.project.validate().unwrap();
        // a camada de cor da cópia aponta para a linha DA CÓPIA
        let fill = s.project.subtree(copy.content.cut()).into_iter().filter_map(|i| s.project.node(&i).cloned()).find(|n| n.name == "cor").unwrap();
        let NodeKind::Layer(l) = &fill.data else { panic!() };
        assert!(l.line_refs.iter().all(|r| s.project.subtree(copy.content.cut()).contains(r)));
        assert_eq!(l.line_refs.len(), 1);
    }

    #[test]
    fn rename_move_update_delete_and_undo() {
        let (mut s, ch) = session();
        let a = run(&mut s, req(&ch, MaterialAction::Create, Some(MaterialKind::Expressions)));
        let b = run(&mut s, req(&ch, MaterialAction::Create, Some(MaterialKind::Eyes)));
        let mut r = req(&ch, MaterialAction::Rename, None);
        r.material = a.material.clone();
        r.name = Some("Rosto".into());
        run(&mut s, r);
        assert_eq!(s.project.node(a.cut.as_ref().unwrap()).unwrap().name, "Rosto");
        let mut r = req(&ch, MaterialAction::Move, None);
        r.material = b.material.clone();
        r.index = Some(0);
        run(&mut s, r);
        assert_eq!(materials(&s.project, &ch)[0].id, b.material.clone().unwrap());
        let mut r = req(&ch, MaterialAction::Update, None);
        r.material = a.material.clone();
        r.notes = Some("vista 3/4 depois".into());
        run(&mut s, r);
        assert_eq!(material(&s.project, &ch, a.material.as_ref().unwrap()).unwrap().notes, "vista 3/4 depois");
        let mut r = req(&ch, MaterialAction::Delete, None);
        r.material = a.material.clone();
        run(&mut s, r);
        assert!(s.project.node(a.cut.as_ref().unwrap()).is_none());
        assert_eq!(materials(&s.project, &ch).len(), 1);
        s.undo().unwrap();
        assert!(s.project.node(a.cut.as_ref().unwrap()).is_some());
        assert_eq!(materials(&s.project, &ch).len(), 2);
        s.project.validate().unwrap();
        // apagar o corte por fora (painel Projeto) tira o material da lista
        s.apply(Command::DeleteNode { id: b.cut.clone().unwrap() }).unwrap();
        assert_eq!(materials(&s.project, &ch).len(), 1);
        s.project.validate().unwrap();
    }

    #[test]
    fn legacy_cuts_show_up_and_are_adopted_on_first_change() {
        let (mut s, ch) = session();
        // projeto antigo: folha de modelo e um corte solto, sem lista
        let v = crate::character::model_sheet(&s.project, &mut s.ids, &ModelSheetRequest { character: ch.clone(), name: "Folha".into(), heads_tall: 6.0, figure_px: 200.0, views: vec![0.0, 90.0], pose: None }).unwrap();
        let mut cmds = v.commands;
        // tira a lista que o model_sheet novo grava: o projeto antigo não a tem
        cmds.pop();
        cmds.push(Command::SetProps { id: ch.clone(), name: None, data: json!({"measurements": {"heads_tall": 6.0, "sheet_cut": v.cut.0}}) });
        s.apply(Command::Batch { commands: cmds, label: None }).unwrap();
        let loose = s.ids.next();
        s.apply(Command::CreateNode { id: Some(loose.clone()), parent: Some(ch.clone()), index: None, name: "Rascunho".into(), data: NodeKind::Cut(CutData::new(1)) }).unwrap();
        let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
        assert!(c.materials.is_empty());
        let list = materials(&s.project, &ch);
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].kind, list[1].kind), (MaterialKind::ModelSheet, MaterialKind::Other));
        // renomear um material "solto" grava a lista inteira
        let mut r = req(&ch, MaterialAction::Rename, None);
        r.material = Some(list[1].id.clone());
        r.name = Some("Poses".into());
        run(&mut s, r);
        let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
        assert_eq!(c.materials.len(), 2);
        assert_eq!(c.materials[1].name, "Poses");
        assert_eq!(c.materials[0].kind, MaterialKind::ModelSheet);
    }

    #[test]
    fn import_makes_a_cut_with_the_image_and_a_line_layer() {
        let (mut s, ch) = session();
        let dir = std::env::temp_dir().join(format!("aiai-mat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ref.png");
        ::image::RgbaImage::from_pixel(40, 20, ::image::Rgba([200, 30, 30, 255])).save(&path).unwrap();
        let mut r = req(&ch, MaterialAction::Import, Some(MaterialKind::Outfit));
        r.path = Some(path.display().to_string());
        let pl = run(&mut s, r);
        let m = material(&s.project, &ch, pl.material.as_ref().unwrap()).unwrap();
        assert_eq!(m.name, "ref");
        let MaterialContent::Image { blob, cut } = &m.content else { panic!("{m:?}") };
        assert!(s.project.referenced_blobs().contains(blob));
        assert_eq!(cmds_canvas(&s.project, cut), [40, 20]);
        s.project.validate().unwrap();
        // o corte renderiza a imagem
        let img = aiai_render::render_frame(&s.project, &s.blobs, cut, 0, &aiai_render::RenderOptions { include_reference: true, camera: false, background: Some([1.0, 1.0, 1.0]), ..Default::default() }).unwrap().image;
        assert_eq!((img.width, img.height), (40, 20));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn cmds_canvas(p: &Project, cut: &Id) -> [u32; 2] {
        let (w, h) = canvas_size(p, cut_data(p, cut).unwrap());
        [w, h]
    }

    #[test]
    fn bad_requests_are_invalid_params_not_panics() {
        let (mut s, ch) = session();
        let bad = |s: &mut CoreSession, r: CharMaterialRequest| plan(&s.project, &mut s.ids, &mut s.blobs, &r).is_err();
        assert!(bad(&mut s, req(&ch, MaterialAction::Rename, None)));
        assert!(bad(&mut s, req(&Id("nada".into()), MaterialAction::List, None)));
        let mut r = req(&ch, MaterialAction::Delete, None);
        r.material = Some(Id("x".into()));
        assert!(bad(&mut s, r));
        let mut r = req(&ch, MaterialAction::Import, None);
        r.path = Some("Z:/nao/existe.png".into());
        assert!(bad(&mut s, r));
        let mut r = req(&ch, MaterialAction::Create, Some(MaterialKind::Other));
        r.name = Some("  ".into());
        assert!(bad(&mut s, r));
    }
}
