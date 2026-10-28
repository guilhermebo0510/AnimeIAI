//! Ferramenta Degradê (S25, à moda do Clip Studio): o pedido de `aiai_gradient` completo —
//! - degradês prontos pelo nome (`preset`): os do programa (`builtin_gradients`), os da ferramenta que usam as cores em
//!   uso (cor principal → transparente, principal → secundária, listra, listra com a secundária) e céus;
//! - onde criar: pintar na camada raster (só dentro da seleção quando há uma), ou uma CAMADA DE DEGRADÊ editável — uma
//!   camada de preenchimento marcada (`params.gradient_layer`) com uma região que cobre o canvas (ou a seleção) e guarda
//!   o degradê com os dois pontos (`gradient.absolute`); arrastar de novo nela (ou pedir com `layer` = ela) refaz os
//!   pontos, e o editor de degradê troca as paradas;
//! - degradê livre (`freeform`): pontos de cor soltos (ver [`crate::freeform`]); sempre pixels (numa camada raster, ou
//!   numa raster nova com `editable`).

use crate::context::*;
use crate::figure::clip_to_mask;
use crate::freeform::{FreeField, FreeformPoint};
use crate::paint::{GradientRequest, cel_raster, resolve_color, selection_mask, tiles_for_region};
use aiai_core::paint::builtin_gradients;
use aiai_core::*;
use aiai_paint::{Image, Mask};
use serde_json::{Value, json};

/// Marca da camada de degradê editável em `params`.
pub const GRAD_LAYER: &str = "gradient_layer";

/// Prontos da ferramenta que dependem das cores em uso (chave estável, nome em pt).
pub const COLOR_PRESETS: [(&str, &str); 5] = [
    ("main_to_transparent", "Cor principal → transparente"),
    ("main_to_sub", "Cor principal → secundária"),
    ("stripe", "Listra"),
    ("sub_stripe", "Listra com a cor secundária"),
    ("photosphere", "Fotosfera"),
];

/// Céus da ferramenta (além dos do programa).
pub fn sky_presets() -> Vec<(&'static str, Gradient)> {
    let h = |s: &str| Rgba::from_hex(s).map(|c| c.0).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let lin = |cols: &[&str]| Gradient { space: GradientSpace::Oklab, ..Gradient::from_colors(&cols.iter().map(|c| h(c)).collect::<Vec<_>>(), 90.0) };
    vec![("Céu noturno", lin(&["#05060f", "#0d1838", "#23407a", "#3f6fb0"])), ("Fim de tarde", lin(&["#4a5fa8", "#c886a8", "#ffb27a", "#ffe2a8"])), ("Céu azul", lin(&["#2f78d4", "#6fb0ee", "#cfe8fb"]))]
}

/// Todos os nomes que `preset` aceita (para mensagens e para o app).
pub fn preset_names() -> Vec<String> {
    COLOR_PRESETS.iter().map(|(k, _)| k.to_string()).chain(builtin_gradients().into_iter().map(|(n, _)| n.to_string())).chain(sky_presets().into_iter().map(|(n, _)| n.to_string())).collect()
}

/// O degradê pronto `name` (sem diferenciar maiúsculas) com as cores principal e secundária dadas (cores de paleta
/// continuam ligadas à paleta).
pub fn preset_gradient(name: &str, main: &ColorRef, sub: &ColorRef) -> Option<Gradient> {
    let name = name.trim();
    let stop = |pos: f32, c: &ColorRef, opacity: f32| GradientStop { pos, color: c.clone(), opacity };
    let g = match name {
        "main_to_transparent" => Gradient { stops: vec![stop(0.0, main, 1.0), stop(1.0, main, 0.0)], space: GradientSpace::Linear, ..Default::default() },
        "main_to_sub" => Gradient { stops: vec![stop(0.0, main, 1.0), stop(1.0, sub, 1.0)], ..Default::default() },
        // fotosfera (Clip Studio): luz radial da cor principal, miolo cheio que se espalha até sumir
        "photosphere" => Gradient {
            shape: GradientShape::Radial,
            stops: vec![stop(0.0, main, 1.0), stop(0.12, main, 0.9), stop(0.4, main, 0.3), stop(1.0, main, 0.0)],
            space: GradientSpace::Linear,
            ..Default::default()
        },
        // listras de borda dura: metade da faixa na cor, metade vazia (ou na secundária), repetindo
        "stripe" => Gradient { stops: vec![stop(0.0, main, 1.0), stop(0.5, main, 1.0), stop(0.5, main, 0.0), stop(1.0, main, 0.0)], extend: GradientExtend::Repeat, ..Default::default() },
        "sub_stripe" => Gradient { stops: vec![stop(0.0, main, 1.0), stop(0.5, main, 1.0), stop(0.5, sub, 1.0), stop(1.0, sub, 1.0)], extend: GradientExtend::Repeat, ..Default::default() },
        _ => return builtin_gradients().into_iter().chain(sky_presets()).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, g)| g),
    };
    Some(g)
}

/// A camada é uma camada de degradê editável.
pub fn is_gradient_layer(l: &LayerData) -> bool {
    l.kind == LayerType::Fill && l.params.get(GRAD_LAYER).and_then(Value::as_bool) == Some(true)
}

/// Região do degradê da camada de degradê no quadro: (cel, índice da região, região).
pub fn gradient_region<'a>(p: &'a Project, cut: &Id, layer: &Id, frame: u32) -> Option<(Id, usize, &'a FillRegion)> {
    let cd = cut_data(p, cut).ok()?;
    let node = exposed_cel(p, cd, layer, frame)?;
    let NodeKind::Cel(cel) = &node.data else { return None };
    let i = cel.fills.iter().position(|f| f.gradient.as_ref().is_some_and(|g| g.absolute.is_some()))?;
    Some((node.id.clone(), i, &cel.fills[i]))
}

/// Comando que troca o degradê da camada de degradê (paradas, pontos, forma) mantendo a área.
pub fn set_layer_gradient(p: &Project, cut: &Id, layer: &Id, frame: u32, g: Gradient) -> ToolResult<Command> {
    g.validate().map_err(ToolError::Invalid)?;
    let (cel, i, _) = gradient_region(p, cut, layer, frame).ok_or_else(|| ToolError::Invalid("a camada não é uma camada de degradê".into()))?;
    let NodeKind::Cel(cd) = &node(p, &cel)?.data else { unreachable!() };
    let mut fills = cd.fills.clone();
    fills[i].gradient = Some(g);
    Ok(Command::ReplaceFills { cel, fills })
}

/// Resultado: o comando e a camada criada (camada de degradê nova).
pub struct GradPlan {
    pub command: Command,
    pub layer: Option<Id>,
}

/// O degradê do pedido (sem a geometria): `gradient` completo, `preset` ou `stops`; `None` = nenhum dado.
fn requested(p: &Project, req: &GradientRequest) -> ToolResult<Option<Gradient>> {
    let main = req.main_color.clone().unwrap_or(ColorRef::Direct { rgba: Rgba([0.0, 0.0, 0.0, 1.0]) });
    let sub = req.sub_color.clone().unwrap_or(ColorRef::Direct { rgba: Rgba::WHITE });
    let g = if let Some(g) = &req.gradient {
        Some(g.clone())
    } else if let Some(name) = &req.preset {
        if name.starts_with("main") || name.contains("stripe") {
            if req.main_color.is_none() {
                return Err(ToolError::Invalid(format!("o pronto '{name}' usa a cor principal: informe `main_color`")));
            }
            if name.starts_with("sub") || name == "main_to_sub" {
                if req.sub_color.is_none() {
                    return Err(ToolError::Invalid(format!("o pronto '{name}' usa a cor secundária: informe `sub_color`")));
                }
            }
        }
        Some(preset_gradient(name, &main, &sub).ok_or_else(|| ToolError::Invalid(format!("degradê pronto '{name}' não existe; prontos: {}", preset_names().join(", "))))?)
    } else if !req.stops.is_empty() {
        Some(Gradient::from_hex_stops(&req.stops).map_err(ToolError::Invalid)?)
    } else {
        None
    };
    if let Some(g) = &g {
        g.validate().map_err(ToolError::Invalid)?;
        for s in &g.stops {
            resolve_color(p, &s.color)?;
        }
    }
    Ok(g)
}

/// Põe a geometria do pedido (forma de `kind`, dois pontos `from`/`to`, repetição de `extend`) no degradê.
fn with_geometry(req: &GradientRequest, g: Gradient) -> ToolResult<Gradient> {
    if !req.from.iter().chain(&req.to).all(|v| v.is_finite()) {
        return Err(ToolError::Invalid("`from`/`to` inválidos".into()));
    }
    if (req.from[0] - req.to[0]).hypot(req.from[1] - req.to[1]) < 0.5 {
        return Err(ToolError::Invalid("`from` e `to` coincidem: o degradê precisa de uma direção".into()));
    }
    Ok(Gradient { shape: req.kind.shape(), absolute: Some([req.from, req.to]), extend: req.extend.unwrap_or(g.extend), ..g })
}

fn check_opacity(req: &GradientRequest) -> ToolResult<()> {
    if !(0.0..=1.0).contains(&req.opacity) {
        return Err(ToolError::Invalid("opacidade fora de 0..1".into()));
    }
    Ok(())
}

fn raster_target<'a>(p: &'a Project, req: &GradientRequest) -> ToolResult<(&'a Node, &'a CelData, u32, u32)> {
    let cut = cut_data(p, &req.cut)?;
    let l = layer_data(p, &req.layer)?;
    if l.kind != LayerType::Raster {
        return Err(ToolError::Invalid("o degradê pinta numa camada raster (ou use `editable` para criar uma camada de degradê)".into()));
    }
    if l.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, cut);
    Ok((node, cel, w, h))
}

/// Campo do degradê livre do pedido.
fn free_field(p: &Project, pts: &[FreeformPoint], w: u32, h: u32) -> ToolResult<FreeField> {
    if pts.len() > 256 {
        return Err(ToolError::Invalid("degradê livre com mais de 256 pontos".into()));
    }
    let mut v = Vec::with_capacity(pts.len());
    for q in pts {
        if !q.at.iter().all(|x| x.is_finite()) {
            return Err(ToolError::Invalid("ponto do degradê livre inválido".into()));
        }
        v.push((q.at, resolve_color(p, &q.color)?));
    }
    Ok(FreeField::new(&v, (w as f32).hypot(h as f32)))
}

/// Pinta o degradê (ou o degradê livre) na imagem, limitado à seleção.
fn paint_image(p: &Project, req: &GradientRequest, g: Option<&Gradient>, img: &mut Image, mask: Option<&(Vec<f32>, u32, u32)>) -> ToolResult<()> {
    let (w, h) = (img.width, img.height);
    let before = mask.map(|_| img.clone());
    if !req.freeform.is_empty() {
        let f = free_field(p, &req.freeform, w, h)?;
        crate::freeform::paint(img, &f, req.blend, req.opacity);
    } else {
        let g = g.ok_or_else(|| ToolError::Invalid("degradê sem `stops`, `gradient` nem `preset`".into()))?;
        let mut err = None;
        let r = g.resolve(|c| {
            resolve_color(p, c).unwrap_or_else(|e| {
                err = Some(e);
                [1.0, 0.0, 1.0, 1.0]
            })
        });
        if let Some(e) = err {
            return Err(e);
        }
        aiai_paint::brush::gradient_fill_resolved(img, &r, [0.0, 0.0, w as f32, h as f32], req.blend, req.opacity);
    }
    if let (Some(m), Some(b)) = (mask, &before) {
        clip_to_mask(img, b, m, [0, 0, w as i32, h as i32]);
    }
    Ok(())
}

/// Degradê em pixels na camada raster do pedido (só dentro da seleção `mask`, cobertura do canvas).
pub fn raster_command(p: &Project, blobs: &mut BlobStore, req: &GradientRequest, mask: Option<&(Vec<f32>, u32, u32)>) -> ToolResult<Command> {
    check_opacity(req)?;
    let g = if req.freeform.is_empty() { Some(with_geometry(req, requested(p, req)?.ok_or_else(|| ToolError::Invalid("degradê sem `stops`, `gradient` nem `preset`".into()))?)?) } else { None };
    let (node, cel, w, h) = raster_target(p, req)?;
    let mut img = cel_raster(blobs, cel, w, h)?;
    paint_image(p, req, g.as_ref(), &mut img, mask)?;
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, [0, 0, w as i32, h as i32]) })
}

/// Máscara da área da camada de degradê: a seleção (recortada na caixa dela) ou o canvas inteiro.
fn area_mask(blobs: &mut BlobStore, mask: Option<&(Vec<f32>, u32, u32)>, w: u32, h: u32) -> ToolResult<(BlobHash, [i32; 4])> {
    match mask {
        None => {
            let m = Mask::from_coverage(w, h, &vec![1.0; (w * h) as usize]);
            Ok((blobs.put(m.encode()), [0, 0, w as i32, h as i32]))
        }
        Some((cov, mw, mh)) => {
            let (mw, mh) = (*mw as usize, *mh as usize);
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
            for (i, v) in cov.iter().enumerate() {
                if *v > 0.0 {
                    let (x, y) = (i % mw, i / mw);
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
            if x0 >= x1 || y0 >= y1 {
                return Err(ToolError::Invalid("a seleção está vazia".into()));
            }
            let (bw, bh) = (x1 - x0, y1 - y0);
            let mut sub = Vec::with_capacity(bw * bh);
            for y in y0..y1 {
                sub.extend_from_slice(&cov[y * mw + x0..y * mw + x1]);
            }
            let _ = mh;
            let m = Mask::from_coverage(bw as u32, bh as u32, &sub);
            Ok((blobs.put(m.encode()), [x0 as i32, y0 as i32, bw as i32, bh as i32]))
        }
    }
}

/// Nome livre entre os irmãos: "Degradê 1", "Degradê 2"...
fn unique_name(p: &Project, parent: &Id, base: &str) -> String {
    let taken: Vec<&str> = p.node(parent).map(|n| n.children.iter().filter_map(|c| p.node(c)).map(|n| n.name.as_str()).collect()).unwrap_or_default();
    (1..).map(|i| format!("{base} {i}")).find(|c| !taken.contains(&c.as_str())).unwrap_or_else(|| base.to_owned())
}

/// Onde entra a camada nova: logo acima de `req.layer` (ou no topo do corte, se `layer` é o corte).
fn new_layer_place(p: &Project, req: &GradientRequest) -> ToolResult<(Id, Option<usize>)> {
    let n = node(p, &req.layer)?;
    match &n.data {
        NodeKind::Cut(_) => Ok((req.layer.clone(), None)),
        NodeKind::Layer(l) if l.kind == LayerType::Group => Ok((req.layer.clone(), None)),
        NodeKind::Layer(_) => {
            let parent = n.parent.clone().ok_or_else(|| ToolError::Invalid("camada sem pai".into()))?;
            let idx = p.node(&parent).and_then(|pn| pn.children.iter().position(|c| *c == req.layer)).map(|i| i + 1);
            Ok((parent, idx))
        }
        other => Err(ToolError::Invalid(format!("`layer` deve ser uma camada ou o corte (é {})", other.type_name()))),
    }
}

/// O pedido inteiro de `aiai_gradient`: pixels na camada raster, camada de degradê nova (ou a existente refeita) ou
/// degradê livre. `mask` = seleção já como cobertura (o app); sem ela vale `req.selection`.
pub fn gradient_action(p: &Project, blobs: &mut BlobStore, ids: &mut IdGen, req: &GradientRequest, mask: Option<&(Vec<f32>, u32, u32)>) -> ToolResult<GradPlan> {
    check_opacity(req)?;
    let sel = match (&req.selection, mask) {
        (Some(shape), None) => Some(selection_mask(p, blobs, &req.cut, &req.layer, req.frame, shape)?),
        _ => None,
    };
    let mask = mask.or(sel.as_ref());
    if !req.editable {
        return Ok(GradPlan { command: raster_command(p, blobs, req, mask)?, layer: None });
    }
    let cut = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, cut);
    let target = layer_data(p, &req.layer).ok();
    // a camada de degradê já existe: refaz os pontos (e as paradas, se o pedido traz um degradê)
    if let Some(l) = target.filter(|l| is_gradient_layer(l)) {
        if !req.freeform.is_empty() {
            return Err(ToolError::Invalid("o degradê livre é pixel: não entra numa camada de degradê (use uma camada raster)".into()));
        }
        if l.locked {
            return Err(ToolError::Invalid("camada travada".into()));
        }
        let (cel, i, old) = gradient_region(p, &req.cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid("a camada de degradê não tem a região do degradê neste quadro".into()))?;
        let base = match requested(p, req)? {
            Some(g) => g,
            None => old.gradient.clone().expect("região com degradê"),
        };
        let g = with_geometry(req, base)?;
        let NodeKind::Cel(cd) = &node(p, &cel)?.data else { unreachable!() };
        let mut fills = cd.fills.clone();
        fills[i].gradient = Some(g);
        if mask.is_some() {
            let (hash, bounds) = area_mask(blobs, mask, w, h)?;
            (fills[i].mask, fills[i].bounds) = (Some(hash), bounds);
        }
        let props = Command::SetProps { id: req.layer.clone(), name: None, data: json!({ "opacity": req.opacity, "blend": req.blend }) };
        return Ok(GradPlan { command: Command::Batch { commands: vec![Command::ReplaceFills { cel, fills }, props], label: Some("Camada de degradê".into()) }, layer: None });
    }
    let (parent, index) = new_layer_place(p, req)?;
    let (layer, cel) = (ids.next(), ids.next());
    let mut data = LayerData::new(if req.freeform.is_empty() { LayerType::Fill } else { LayerType::Raster });
    data.opacity = req.opacity;
    data.blend = req.blend;
    let mut cmds = Vec::new();
    if req.freeform.is_empty() {
        let g = with_geometry(req, requested(p, req)?.ok_or_else(|| ToolError::Invalid("degradê sem `stops`, `gradient` nem `preset`".into()))?)?;
        data.params = json!({ GRAD_LAYER: true });
        let (hash, bounds) = area_mask(blobs, mask, w, h)?;
        let region = FillRegion { id: Id(String::new()), seed: [bounds[0] as f32 + 0.5, bounds[1] as f32 + 0.5], color: ColorRef::default(), mask: Some(hash), bounds, gradient: Some(g) };
        cmds.push(Command::CreateNode { id: Some(layer.clone()), parent: Some(parent.clone()), index, name: unique_name(p, &parent, "Degradê"), data: NodeKind::Layer(data) });
        cmds.push(Command::CreateNode { id: Some(cel.clone()), parent: Some(layer.clone()), index: None, name: "1".into(), data: NodeKind::Cel(CelData::new(1)) });
        cmds.push(Command::ReplaceFills { cel, fills: vec![region] });
    } else {
        // degradê livre numa camada raster nova (a opacidade é a da camada; o campo entra cheio)
        let mut img = Image::new(w, h);
        let free = GradientRequest { opacity: 1.0, blend: BlendMode::Normal, ..req.clone() };
        paint_image(p, &free, None, &mut img, mask)?;
        let tiles = tiles_for_region(blobs, &img, [0, 0, w as i32, h as i32]);
        cmds.push(Command::CreateNode { id: Some(layer.clone()), parent: Some(parent.clone()), index, name: unique_name(p, &parent, "Degradê livre"), data: NodeKind::Layer(data) });
        cmds.push(Command::CreateNode { id: Some(cel.clone()), parent: Some(layer.clone()), index: None, name: "1".into(), data: NodeKind::Cel(CelData::new(1)) });
        cmds.push(Command::SetTiles { cel, tiles });
    }
    Ok(GradPlan { command: Command::Batch { commands: cmds, label: Some("Camada de degradê".into()) }, layer: Some(layer) })
}

/// O que a bridge devolve junto do resultado.
pub fn plan_value(plan: &GradPlan) -> Value {
    match &plan.layer {
        Some(l) => json!({ "layer": l }),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_paint::brush::GradientKind;

    fn session() -> (CoreSession, Id, Id, Id) {
        let mut s = CoreSession::for_tests("grad");
        s.project.settings.width = 200;
        s.project.settings.height = 100;
        let mk = |s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind| {
            let id = s.ids.next();
            s.apply(Command::CreateNode { id: Some(id.clone()), parent: parent.cloned(), index: None, name: name.into(), data }).unwrap();
            id
        };
        let ep = mk(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = mk(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = mk(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
        let ras = mk(&mut s, Some(&cut), "R", NodeKind::Layer(LayerData::new(LayerType::Raster)));
        mk(&mut s, Some(&ras), "1", NodeKind::Cel(CelData::new(1)));
        let vec_l = mk(&mut s, Some(&cut), "V", NodeKind::Layer(LayerData::new(LayerType::Vector)));
        mk(&mut s, Some(&vec_l), "1", NodeKind::Cel(CelData::new(1)));
        (s, cut, ras, vec_l)
    }

    fn red() -> ColorRef {
        ColorRef::Direct { rgba: Rgba([1.0, 0.0, 0.0, 1.0]) }
    }

    fn req(cut: &Id, layer: &Id) -> GradientRequest {
        GradientRequest {
            cut: cut.clone(),
            frame: 0,
            layer: layer.clone(),
            kind: GradientKind::Linear,
            from: [0.0, 50.0],
            to: [200.0, 50.0],
            stops: vec![],
            blend: BlendMode::Normal,
            opacity: 1.0,
            gradient: None,
            preset: Some("main_to_transparent".into()),
            main_color: Some(red()),
            sub_color: None,
            extend: None,
            editable: false,
            selection: None,
            freeform: vec![],
        }
    }

    fn render(s: &CoreSession, cut: &Id) -> Image {
        aiai_render::render_frame(&s.project, &s.blobs, cut, 0, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image
    }

    fn run(s: &mut CoreSession, r: &GradientRequest) -> GradPlan {
        let mut ids = std::mem::replace(&mut s.ids, IdGen::sequential("x"));
        let plan = gradient_action(&s.project, &mut s.blobs, &mut ids, r, None).unwrap();
        s.ids = ids;
        s.apply(plan.command.clone()).unwrap();
        plan
    }

    #[test]
    fn color_presets_follow_the_current_colors() {
        let blue = ColorRef::Direct { rgba: Rgba([0.0, 0.0, 1.0, 1.0]) };
        let g = preset_gradient("main_to_sub", &red(), &blue).unwrap();
        assert_eq!((g.stops[0].color.clone(), g.stops[1].color.clone()), (red(), blue.clone()));
        let t = preset_gradient("main_to_transparent", &red(), &blue).unwrap();
        assert_eq!(t.stops[1].opacity, 0.0);
        let st = preset_gradient("stripe", &red(), &blue).unwrap();
        assert_eq!(st.extend, GradientExtend::Repeat);
        assert!(preset_gradient("céu NOTURNO", &red(), &blue).is_some() && preset_gradient("Arco-íris", &red(), &blue).is_some());
        assert!(preset_gradient("nada", &red(), &blue).is_none());
        assert!(preset_names().len() >= 4 + builtin_gradients().len() + 3);
    }

    #[test]
    fn raster_gradient_with_preset_and_selection() {
        let (mut s, cut, ras, _) = session();
        let mut r = req(&cut, &ras);
        r.selection = Some(aiai_paint::select::SelectShape::Rect { rect: [0.0, 0.0, 200.0, 50.0] });
        run(&mut s, &r);
        let img = render(&s, &cut);
        assert!(img.get(2, 10)[3] > 0.95 && img.get(2, 10)[0] > 0.95, "vermelho cheio no começo");
        assert!(img.get(100, 10)[3] > 0.3 && img.get(100, 10)[3] < 0.7, "meio: meio transparente");
        assert!(img.get(2, 80)[3] < 0.01, "fora da seleção não pinta");
        // camada vetorial sem `editable` é erro
        let (mut s2, cut2, _, vec_l) = session();
        let mut ids = IdGen::sequential("y");
        assert!(gradient_action(&s2.project, &mut s2.blobs, &mut ids, &req(&cut2, &vec_l), None).is_err());
        let _ = &mut s2;
    }

    #[test]
    fn editable_layer_is_created_and_redone_in_place() {
        let (mut s, cut, _, vec_l) = session();
        let mut r = req(&cut, &vec_l);
        r.editable = true;
        r.preset = Some("main_to_sub".into());
        r.sub_color = Some(ColorRef::Direct { rgba: Rgba([0.0, 0.0, 1.0, 1.0]) });
        r.opacity = 0.5;
        let plan = run(&mut s, &r);
        let layer = plan.layer.expect("camada nova");
        let l = layer_data(&s.project, &layer).unwrap();
        assert!(is_gradient_layer(l) && (l.opacity - 0.5).abs() < 1e-6);
        // entrou logo acima da camada ativa, com nome numerado
        let cut_children = &s.project.node(&cut).unwrap().children;
        assert_eq!(cut_children.iter().position(|c| *c == layer), cut_children.iter().position(|c| *c == vec_l).map(|i| i + 1));
        assert_eq!(s.project.node(&layer).unwrap().name, "Degradê 1");
        let img = render(&s, &cut);
        assert!(img.get(5, 50)[0] > img.get(5, 50)[2] && img.get(195, 50)[2] > img.get(195, 50)[0], "vermelho → azul");
        // refazer: só a geometria, mantendo as paradas
        let mut again = req(&cut, &layer);
        again.editable = true;
        (again.preset, again.main_color) = (None, None);
        (again.from, again.to) = ([200.0, 50.0], [0.0, 50.0]);
        again.opacity = 1.0;
        let p2 = run(&mut s, &again);
        assert!(p2.layer.is_none());
        let (_, _, reg) = gradient_region(&s.project, &cut, &layer, 0).unwrap();
        let g = reg.gradient.as_ref().unwrap();
        assert_eq!(g.absolute, Some([[200.0, 50.0], [0.0, 50.0]]));
        assert_eq!(g.stops[0].color, red(), "paradas mantidas");
        let img = render(&s, &cut);
        assert!(img.get(5, 50)[2] > img.get(5, 50)[0], "invertido");
        // as paradas trocam pelo editor
        let mut g2 = g.clone();
        g2.stops[0].color = ColorRef::Direct { rgba: Rgba([0.0, 1.0, 0.0, 1.0]) };
        let c = set_layer_gradient(&s.project, &cut, &layer, 0, g2).unwrap();
        s.apply(c).unwrap();
        assert!(render(&s, &cut).get(195, 50)[1] > 0.9);
        // degradê livre não entra na camada de degradê
        let mut free = again.clone();
        free.freeform = vec![FreeformPoint { at: [10.0, 10.0], color: red() }];
        let mut ids = IdGen::sequential("z");
        assert!(gradient_action(&s.project, &mut s.blobs, &mut ids, &free, None).is_err());
    }

    #[test]
    fn freeform_paints_raster_or_a_new_layer() {
        let (mut s, cut, ras, vec_l) = session();
        let mut r = req(&cut, &ras);
        r.preset = None;
        r.freeform = vec![FreeformPoint { at: [10.0, 50.0], color: red() }, FreeformPoint { at: [190.0, 50.0], color: ColorRef::Direct { rgba: Rgba([0.0, 0.0, 1.0, 1.0]) } }];
        run(&mut s, &r);
        let img = render(&s, &cut);
        assert!(img.get(10, 50)[0] > 0.9 && img.get(190, 50)[2] > 0.9 && img.get(100, 0)[3] > 0.99);
        let mut e = r.clone();
        (e.layer, e.editable) = (vec_l, true);
        let plan = run(&mut s, &e);
        let l = layer_data(&s.project, plan.layer.as_ref().unwrap()).unwrap();
        assert_eq!(l.kind, LayerType::Raster);
        assert!(s.project.node(plan.layer.as_ref().unwrap()).unwrap().name.starts_with("Degradê livre"));
    }

    #[test]
    fn bad_requests_are_errors() {
        let (mut s, cut, ras, _) = session();
        let mut ids = IdGen::sequential("e");
        let mut r = req(&cut, &ras);
        r.preset = Some("nada".into());
        assert!(gradient_action(&s.project, &mut s.blobs, &mut ids, &r, None).is_err());
        let mut r = req(&cut, &ras);
        r.to = r.from;
        assert!(gradient_action(&s.project, &mut s.blobs, &mut ids, &r, None).is_err());
        let mut r = req(&cut, &ras);
        r.main_color = None;
        assert!(gradient_action(&s.project, &mut s.blobs, &mut ids, &r, None).is_err(), "pronto de cor sem a cor");
        let mut r = req(&cut, &ras);
        r.preset = None;
        assert!(gradient_action(&s.project, &mut s.blobs, &mut ids, &r, None).is_err(), "sem degradê nenhum");
    }
}
