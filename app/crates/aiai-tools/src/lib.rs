//! Operações de ferramenta. Cada operação lê o documento, calcula o resultado com o motor (aiai-paint/aiai-render) e
//! devolve **comandos** do núcleo — quem chama aplica pela sessão, então tudo tem undo, entra no log e funciona igual
//! na UI, na bridge/MCP e no modo headless.

pub mod context;
pub mod draw;
pub mod fill;
pub mod paint;

pub use context::{ToolError, ToolResult};
