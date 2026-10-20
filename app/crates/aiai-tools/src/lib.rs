//! Operações de ferramenta. Cada operação lê o documento, calcula o resultado com o motor (aiai-paint/aiai-render) e
//! devolve **comandos** do núcleo — quem chama aplica pela sessão, então tudo tem undo, entra no log e funciona igual
//! na UI, na bridge/MCP e no modo headless.

pub mod anim;
pub mod audio;
pub mod audio_edit;
pub mod background;
pub mod calib;
pub mod char_material;
pub mod character;
pub mod compare;
pub mod context;
pub mod delivery;
pub mod draw;
pub mod edit;
pub mod erase;
pub mod export;
pub mod fill;
pub mod fx2d;
pub mod guide;
pub mod import;
pub mod library;
pub mod model3d;
pub mod modeling;
pub mod paint;
pub mod points;
pub mod preprod;
pub mod preview;
pub mod text_layer;
pub mod production;
pub mod production_history;
pub mod production_stats;
pub mod ruler;
pub mod transform;

pub use context::{ToolError, ToolResult};
