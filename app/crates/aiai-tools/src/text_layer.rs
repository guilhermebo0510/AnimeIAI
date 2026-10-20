//! Camada de texto editável pelo MCP/bridge (`aiai_text_layer`): cria uma camada `text` ou muda uma existente com os
//! mesmos campos do painel Texto. O texto e o estilo ficam em `params.text` (ver `aiai_paint::text_layer`); o render
//! refaz o layout a cada quadro, então a camada continua editável. Um passo de undo por chamada.

use crate::context::*;
use aiai_core::*;
use aiai_paint::text_layer::{TextAlign, TextEffect, TextParams, TextShadow, TextStroke, TextWarp, WarpStyle, canvas_bounds, layout};
use aiai_paint::text_style::{TextStyle, builtin_styles};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

/// Distingue "campo ausente" (não mexe) de `null` (apaga): `Some(None)` = `null` explícito.
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TextLayerRequest {
    /// Camada de texto a editar. Omitida = cria uma camada nova (exige `cut`).
    #[serde(default)]
    pub layer: Option<Id>,
    /// Corte que recebe a camada nova.
    #[serde(default)]
    pub cut: Option<Id>,
    /// Camada nova entra logo acima desta (na pasta, se for pasta); sem ela, no topo do corte.
    #[serde(default)]
    pub above: Option<Id>,
    /// Nome da camada no painel (padrão: a primeira linha do texto).
    #[serde(default)]
    pub name: Option<String>,
    /// O texto (`\n` quebra linha).
    #[serde(default)]
    pub text: Option<String>,
    /// Família da fonte (instalada, da pasta Documentos/AnimeIAI/Fontes, ou "Noto Sans JP", embutida e padrão). Veja `list_fonts`.
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    /// Corpo em px do canvas (altura do em).
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<ColorRef>,
    /// Rastreamento em milésimos de em (100 = 0,1 em entre as letras).
    #[serde(default)]
    pub tracking: Option<f32>,
    /// Entrelinha como múltiplo do tamanho (1.25 = 125%).
    #[serde(default)]
    pub leading: Option<f32>,
    #[serde(default)]
    pub align: Option<TextAlign>,
    /// Tategaki: colunas verticais, da direita para a esquerda.
    #[serde(default)]
    pub vertical: Option<bool>,
    /// Âncora no canvas. Texto de ponto: borda esquerda/centro/direita da linha conforme `align` (no vertical, a borda
    /// direita da primeira coluna), topo da primeira linha em y. Texto de caixa: canto superior esquerdo da caixa.
    #[serde(default)]
    pub pos: Option<[f32; 2]>,
    /// Texto de caixa [largura, altura] (quebra automática); `null` volta a texto de ponto.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<[f32; 2]>")]
    pub box_size: Option<Option<[f32; 2]>>,
    /// Graus (horário) em torno da âncora.
    #[serde(default)]
    pub rotation: Option<f32>,
    /// Contorno por fora das letras {width px, color}; `null` tira.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<TextStroke>")]
    pub stroke: Option<Option<TextStroke>>,
    /// Sombra {offset [x,y], blur, color com alfa}; `null` tira.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<TextShadow>")]
    pub shadow: Option<Option<TextShadow>>,
    /// Tinta do preenchimento no lugar de `color`: {"type":"gradient","gradient":{...}} (degradê na caixa das letras),
    /// {"type":"pattern","image":"<blob>","scale":1} ou {"type":"solid","color":...}; `null` volta a `color`.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<Paint>")]
    pub fill: Option<Option<Paint>>,
    /// Pilha de estilos (substitui a lista inteira; `[]` tira todos). Cada item é plano: {"type": "stroke"|"drop_shadow"|
    /// "inner_shadow"|"outer_glow"|"inner_glow"|"bevel_emboss"|"satin"|"gradient_overlay"|"color_overlay"|
    /// "pattern_overlay"|"long_shadow", "enabled", "blend", "opacity", ...campos do efeito}.
    #[serde(default)]
    pub effects: Option<Vec<TextEffect>>,
    /// Distorção {style: arc|arch|wave|flag|bulge|rise|path, bend -1..1, distort -1..1, path [[x,y]...], path_offset};
    /// `null` tira.
    #[serde(default, deserialize_with = "double_option")]
    #[schemars(with = "Option<TextWarp>")]
    pub warp: Option<Option<TextWarp>>,
    /// Texto no caminho de um traço vetorial (id do traço em qualquer cel): vira `warp` {style: path} com os pontos dele.
    #[serde(default)]
    pub warp_path_stroke: Option<Id>,
    /// Aplica um estilo pronto pelo nome (ver `list_styles`) antes dos outros campos (as medidas escalam com `size`).
    #[serde(default)]
    pub style: Option<String>,
    /// Aplica um estilo de um arquivo `.aiaistyle` (caminho absoluto).
    #[serde(default)]
    pub style_file: Option<String>,
    /// Opacidade da camada (0..1).
    #[serde(default)]
    pub opacity: Option<f32>,
    /// Só lista as fontes disponíveis (não altera nada).
    #[serde(default)]
    pub list_fonts: bool,
    /// Só lista os estilos prontos (não altera nada).
    #[serde(default)]
    pub list_styles: bool,
}

pub struct TextLayerPlan {
    pub command: Command,
    pub layer: Id,
    pub created: bool,
    pub params: TextParams,
}

/// Famílias disponíveis: nome, se tem negrito/itálico de verdade e se veio da pasta do usuário.
pub fn list_fonts() -> Value {
    let cat = aiai_paint::fonts::catalog();
    let fams: Vec<Value> = cat
        .families()
        .into_iter()
        .map(|f| {
            let faces = cat.family_faces(&f);
            json!({ "family": f, "bold": faces.iter().any(|x| x.bold), "italic": faces.iter().any(|x| x.italic), "user": faces.iter().any(|x| x.user) })
        })
        .collect();
    json!({ "fonts": fams, "user_dir": aiai_paint::fonts::user_fonts_dir().map(|p| p.display().to_string()), "default": aiai_paint::fonts::DEFAULT_FAMILY })
}

fn check_font(f: &str) -> ToolResult<()> {
    let f = f.trim();
    if f.is_empty() || f.eq_ignore_ascii_case(aiai_paint::fonts::DEFAULT_FAMILY) {
        return Ok(());
    }
    let cat = aiai_paint::fonts::catalog();
    if cat.pick(f, false, false).is_some() {
        return Ok(());
    }
    let names: Vec<String> = cat.families().into_iter().filter(|n| n.to_lowercase().contains(&f.to_lowercase())).take(12).collect();
    let hint = if names.is_empty() { "use list_fonts para ver as famílias".to_owned() } else { format!("parecidas: {}", names.join(", ")) };
    Err(ToolError::Invalid(format!("fonte '{f}' não encontrada ({hint})")))
}

/// Estilos prontos: nome e o que cada um traz.
pub fn list_styles() -> Value {
    let all: Vec<Value> = builtin_styles()
        .iter()
        .map(|s| json!({ "name": s.name, "effects": s.effects.iter().map(|e| e.kind.key()).collect::<Vec<_>>(), "fill": s.fill.as_ref().map(|f| match f { Paint::Solid { .. } => "solid", Paint::Gradient { .. } => "gradient", Paint::Pattern { .. } => "pattern" }), "warp": s.warp.as_ref().map(|w| w.style.key()) }))
        .collect();
    json!({ "styles": all })
}

/// Pontos (amostrados como o render desenha) do traço vetorial `id`, procurado em todos os cels.
fn stroke_points(p: &Project, id: &Id) -> ToolResult<Vec<[f32; 2]>> {
    for n in p.nodes.values() {
        if let NodeKind::Cel(c) = &n.data {
            if let Some(s) = c.strokes.iter().find(|s| &s.id == id) {
                let pts: Vec<[f32; 2]> = aiai_paint::sample_stroke(s).iter().map(|q| [q.x, q.y]).collect();
                if pts.len() < 2 {
                    return Err(ToolError::Invalid(format!("o traço {id} é curto demais para caminho")));
                }
                return Ok(pts);
            }
        }
    }
    Err(ToolError::NotFound(format!("traço {id}")))
}

fn validate_paint(pt: &Paint) -> ToolResult<()> {
    if let Paint::Gradient { gradient } = pt {
        gradient.validate().map_err(ToolError::Invalid)?;
    }
    Ok(())
}

/// Posição e tamanho do texto depois da mudança (para o agente conferir sem renderizar).
pub fn info(p: &TextParams) -> Value {
    let lay = layout(p);
    let b = canvas_bounds(p, &lay);
    json!({
        "bounds": b,
        "lines": lay.line_count(),
        "font_used": lay.family_used(),
        "params": p.to_value(),
    })
}

pub fn text_layer_command(p: &Project, ids: &mut IdGen, req: &TextLayerRequest) -> ToolResult<TextLayerPlan> {
    let mut cmds = Vec::new();
    let (layer, mut params, created, parent_cut) = match &req.layer {
        Some(id) => {
            let l = layer_data(p, id)?;
            if l.kind != LayerType::Text {
                return Err(ToolError::Invalid(format!("{id} não é uma camada de texto; omita `layer` para criar uma")));
            }
            let params = TextParams::from_layer_params(&l.params).transpose().map_err(ToolError::Invalid)?.unwrap_or_default();
            (id.clone(), params, false, None)
        }
        None => {
            let cut = req.cut.clone().ok_or_else(|| ToolError::Invalid("Missing 'cut': informe o corte da camada nova (ou 'layer' para editar uma existente)".into()))?;
            let cd = cut_data(p, &cut)?;
            let (w, h) = canvas_size(p, cd);
            let params = TextParams { size: (h as f32 * 0.08).clamp(24.0, 160.0), pos: [w as f32 / 2.0, h as f32 / 2.0], align: TextAlign::Center, ..Default::default() };
            (ids.next(), params, true, Some(cut))
        }
    };
    if created && req.text.is_none() {
        return Err(ToolError::Invalid("Missing 'text': o texto da camada nova".into()));
    }
    if let Some(v) = &req.text {
        params.content = v.clone();
    }
    if let Some(sz) = req.size {
        // o estilo escala com o tamanho final
        params.size = sz;
    }
    if let Some(name) = &req.style {
        let all = builtin_styles();
        let st = all.iter().find(|s| s.name.eq_ignore_ascii_case(name.trim()) || s.name.to_lowercase().starts_with(&name.trim().to_lowercase()));
        match st {
            Some(st) => st.apply(&mut params),
            None => return Err(ToolError::Invalid(format!("estilo '{name}' não existe; prontos: {}", all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))),
        }
    }
    if let Some(path) = &req.style_file {
        let text = std::fs::read_to_string(path).map_err(|e| ToolError::Invalid(format!("{path}: {e}")))?;
        TextStyle::from_json(&text).map_err(ToolError::Invalid)?.apply(&mut params);
    }
    if let Some(Some(f)) = &req.fill {
        validate_paint(f)?;
    }
    for e in req.effects.iter().flatten() {
        if let aiai_paint::text_layer::TextEffectKind::Stroke(s) = &e.kind {
            validate_paint(&s.paint)?;
        }
    }
    if let Some(f) = &req.font {
        check_font(f)?;
        params.font = f.trim().to_owned();
    }
    macro_rules! set {
        ($field:ident) => {
            if let Some(v) = &req.$field {
                params.$field = v.clone();
            }
        };
    }
    set!(bold);
    set!(italic);
    set!(size);
    set!(color);
    set!(tracking);
    set!(leading);
    set!(align);
    set!(vertical);
    set!(pos);
    set!(box_size);
    set!(rotation);
    set!(stroke);
    set!(shadow);
    set!(fill);
    set!(effects);
    set!(warp);
    if let Some(sid) = &req.warp_path_stroke {
        let pts = stroke_points(p, sid)?;
        let w = params.warp.get_or_insert_with(TextWarp::default);
        w.style = WarpStyle::Path;
        w.path = Some(pts);
    }
    let params = params.sanitized();
    let opacity = match req.opacity {
        Some(o) if !(0.0..=1.0).contains(&o) => return Err(ToolError::Invalid("opacity fora de 0..1".into())),
        o => o,
    };
    let patch_text = json!({ "params": { TextParams::KEY: params.to_value() } });
    if created {
        let cut = parent_cut.expect("corte");
        let (parent, index) = match req.above.as_ref() {
            Some(a) => {
                let n = node(p, a)?;
                match (&n.data, &n.parent) {
                    (NodeKind::Layer(l), _) if l.kind == LayerType::Group => (a.clone(), None),
                    (NodeKind::Layer(_), Some(par)) => {
                        let idx = p.node(par).and_then(|pn| pn.children.iter().position(|c| c == a)).map(|i| i + 1);
                        (par.clone(), idx)
                    }
                    _ => return Err(ToolError::Invalid(format!("{a} não é uma camada"))),
                }
            }
            None => (cut, None),
        };
        let first: String = params.content.lines().next().unwrap_or("").chars().take(28).collect();
        let name = req.name.clone().unwrap_or_else(|| if first.trim().is_empty() { "Texto".to_owned() } else { first.trim().to_owned() });
        let mut data = LayerData::new(LayerType::Text);
        data.params = json!({ TextParams::KEY: params.to_value() });
        if let Some(o) = opacity {
            data.opacity = o;
        }
        cmds.push(Command::CreateNode { id: Some(layer.clone()), parent: Some(parent), index, name, data: NodeKind::Layer(data) });
    } else {
        let mut patch = patch_text;
        if let (Some(o), Some(m)) = (opacity, patch.as_object_mut()) {
            m.insert("opacity".into(), json!(o));
        }
        cmds.push(Command::SetProps { id: layer.clone(), name: req.name.clone(), data: patch });
    }
    let label = format!("Texto \"{}\"", params.content.lines().next().unwrap_or("").chars().take(24).collect::<String>());
    Ok(TextLayerPlan { command: Command::Batch { commands: cmds, label: Some(label) }, layer, created, params })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(json: Value) -> TextLayerRequest {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn styles_effects_and_warp_reach_the_params() {
        let r = req(json!({
            "text": "Título",
            "style": "shōnen",
            "effects": [{"type": "outer_glow", "size": 10}, {"type": "stroke", "width": 3, "position": "inside"}],
            "warp": {"style": "wave", "bend": 0.3},
            "fill": {"type": "gradient", "gradient": {"stops": [{"pos": 0, "color": {"type": "direct", "rgba": [1, 0, 0, 1]}}, {"pos": 1, "color": {"type": "direct", "rgba": [0, 0, 1, 1]}}]}}
        }));
        assert_eq!(r.effects.as_ref().unwrap().len(), 2);
        assert!(matches!(r.warp, Some(Some(_))) && matches!(r.fill, Some(Some(_))));
        assert!(list_styles()["styles"].as_array().unwrap().len() >= 6);
    }

    #[test]
    fn double_option_tells_absent_from_null() {
        let r = req(json!({"stroke": null, "shadow": {"blur": 3.0}}));
        assert!(matches!(r.stroke, Some(None)));
        assert!(matches!(r.shadow, Some(Some(_))));
        assert!(r.box_size.is_none());
        let r = req(json!({"box_size": null}));
        assert!(matches!(r.box_size, Some(None)));
    }
}
