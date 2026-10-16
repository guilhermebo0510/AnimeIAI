//! Compositor do AnimeIAI. O caminho de referência é CPU (determinístico, base dos golden tests e das métricas do
//! agente); efeitos pesados ganham caminho GPU nas sprints de satsuei, sempre com o equivalente CPU para comparação.

pub mod blend;
pub mod compose;

pub use compose::{RenderError, RenderOptions, RenderOutput, Renderer, cut_canvas, render_frame};
