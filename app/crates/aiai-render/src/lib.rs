//! Compositor do AnimeIAI. O caminho de referência é CPU (determinístico, base dos golden tests e das métricas do
//! agente); efeitos pesados ganham caminho GPU nas sprints de satsuei, sempre com o equivalente CPU para comparação.

pub mod compose;
pub use aiai_paint::blend;

pub use compose::{FrameCache, RenderError, RenderOptions, RenderOutput, Renderer, cut_canvas, render_frame, render_frame_cached};
