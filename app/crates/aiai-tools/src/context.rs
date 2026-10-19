//! Ajudantes para localizar cortes, camadas e cels a partir do documento.

use aiai_core::*;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{0}")]
    Core(#[from] CoreError),
    #[error("não encontrado: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("render: {0}")]
    Render(String),
    /// O que o pedido diz ter lido (revisão, contagem) não bate com o documento: leia de novo antes de editar.
    #[error("{0}")]
    Stale(String),
}

pub type ToolResult<T> = Result<T, ToolError>;

pub fn node<'a>(p: &'a Project, id: &Id) -> ToolResult<&'a Node> {
    p.node(id).ok_or_else(|| ToolError::NotFound(id.to_string()))
}

pub fn cut_data<'a>(p: &'a Project, id: &Id) -> ToolResult<&'a CutData> {
    match &node(p, id)?.data {
        NodeKind::Cut(c) => Ok(c),
        other => Err(ToolError::Invalid(format!("{id} é {}, não corte", other.type_name()))),
    }
}

pub fn layer_data<'a>(p: &'a Project, id: &Id) -> ToolResult<&'a LayerData> {
    match &node(p, id)?.data {
        NodeKind::Layer(l) => Ok(l),
        other => Err(ToolError::Invalid(format!("{id} é {}, não camada", other.type_name()))),
    }
}

/// Cel exposto pela camada no quadro (mesma regra do compositor).
pub fn exposed_cel<'a>(p: &'a Project, cut: &CutData, layer: &Id, frame: u32) -> Option<&'a Node> {
    match cut.xsheet.track_for_layer(layer) {
        Some(track) => match track.exposed_at(frame)? {
            CellRef::Cel { number } => p.cel_by_number(layer, number),
            CellRef::Empty => None,
        },
        None => {
            let l = p.node(layer)?;
            l.children
                .iter()
                .filter_map(|c| p.node(c))
                .filter(|n| matches!(n.data, NodeKind::Cel(_)))
                .min_by_key(|n| match &n.data {
                    NodeKind::Cel(c) => c.number,
                    _ => u32::MAX,
                })
        }
    }
}

/// Tamanho do canvas do corte.
pub fn canvas_size(p: &Project, cut: &CutData) -> (u32, u32) {
    aiai_render::cut_canvas(p, cut)
}
