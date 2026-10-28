//! Operações de ferramenta. Cada operação lê o documento, calcula o resultado com o motor (aiai-paint/aiai-render) e
//! devolve **comandos** do núcleo — quem chama aplica pela sessão, então tudo tem undo, entra no log e funciona igual
//! na UI, na bridge/MCP e no modo headless.

pub mod assist;
pub mod rig;
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
pub mod figure;
pub mod fill;
pub mod fill_area;
pub mod freeform;
pub mod fx2d;
pub mod anime_fx;
pub mod hints;
pub mod grad_tool;
pub mod health;
pub mod gradient_paint;
pub mod guide;
pub mod import;
pub mod import_layers;
pub mod layer_merge;
pub mod library;
pub mod line_fix;
pub mod model3d;
pub mod modeling;
pub mod paint;
pub mod points;
pub mod quick_shape;
pub mod preprod;
pub mod preview;
pub mod text_layer;
pub mod production;
pub mod production_history;
pub mod production_stats;
pub mod ruler;
pub mod transform;

pub use context::{ToolError, ToolResult};
