//! Operações de ferramenta. Cada operação lê o documento, calcula o resultado com o motor (aiai-paint/aiai-render) e
//! devolve **comandos** do núcleo — quem chama aplica pela sessão, então tudo tem undo, entra no log e funciona igual
//! na UI, na bridge/MCP e no modo headless.

pub mod anim;
pub mod audio;
pub mod audio_edit;
pub mod background;
pub mod calib;
pub mod character;
pub mod compare;
pub mod context;
pub mod draw;
pub mod edit;
pub mod erase;
pub mod export;
pub mod fill;
pub mod fx2d;
pub mod guide;
pub mod import;
pub mod paint;
pub mod points;
pub mod preprod;
pub mod preview;
pub mod text_layer;
pub mod production;
pub mod transform;

pub use context::{ToolError, ToolResult};
