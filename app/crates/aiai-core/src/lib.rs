//! Núcleo do AnimeIAI: documento (projeto → episódio → cena → corte → camadas/cels/timesheet), Command Bus com
//! undo/redo por inverso, consultas, armazenamento de blobs por conteúdo e persistência em pasta `.aiai`.
//!
//! Regras (ADR-002): todo efeito no documento é um [`Command`] serializável aplicado por [`CoreSession::apply`];
//! nada aqui depende de GPU ou de UI; relógio e gerador de ids são injetáveis para testes determinísticos.

pub mod blob;
pub mod color;
pub mod command;
pub mod error;
pub mod ids;
pub mod keys;
pub mod model;
pub mod patch;
pub mod persist;
pub mod query;
pub mod session;
pub mod stroke;
pub mod xsheet;

pub use blob::{BlobHash, BlobStore};
pub use color::{ColorRef, Rgba};
pub use command::{Command, Outcome};
pub use error::CoreError;
pub use ids::{Id, IdGen};
pub use keys::{Ease, Key, KeyTracks};
pub use model::*;
pub use query::{Query, QueryResult};
pub use session::{Clock, CoreSession, LogEntry};
pub use stroke::*;
pub use xsheet::*;

/// Versão do formato do `project.json`. Subir quando houver mudança incompatível (com migração em `persist`).
pub const FORMAT_VERSION: u32 = 1;
