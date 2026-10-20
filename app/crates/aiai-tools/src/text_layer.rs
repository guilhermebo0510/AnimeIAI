//! Camada de texto editável pelo MCP/bridge (`aiai_text_layer`): cria uma camada `text` ou muda uma existente com os
//! mesmos campos do painel Texto. O texto e o estilo ficam em `params.text` (ver `aiai_paint::text_layer`); o render
//! refaz o layout a cada quadro, então a camada continua editável. Um passo de undo por chamada.

use crate::context::*;
use aiai_core::*;
use aiai_paint::text_layer::{TextAlign, TextParams, TextShadow, TextStroke, canvas_bounds, layout};
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
    /// Opacidade da camada (0..1).
    #[serde(default)]
    pub opacity: Option<f32>,
    /// Só lista as fontes disponíveis (não altera nada).
    #[serde(default)]
    pub list_fonts: bool,
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
    fn double_option_tells_absent_from_null() {
        let r = req(json!({"stroke": null, "shadow": {"blur": 3.0}}));
        assert!(matches!(r.stroke, Some(None)));
        assert!(matches!(r.shadow, Some(Some(_))));
        assert!(r.box_size.is_none());
        let r = req(json!({"box_size": null}));
        assert!(matches!(r.box_size, Some(None)));
    }
}
