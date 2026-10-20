use crate::ids::Id;

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum CoreError {
    #[error("nó não encontrado: {0}")]
    NotFound(Id),
    #[error("tipo de nó errado para {id}: esperado {expected}, encontrado {found}")]
    WrongKind { id: Id, expected: &'static str, found: &'static str },
    #[error("valor inválido em {field}: {detail}")]
    Invalid { field: String, detail: String },
    #[error("hierarquia inválida: {0}")]
    Hierarchy(String),
    #[error("revisão desatualizada: esperada {expected}, atual {actual}")]
    StaleRevision { expected: u64, actual: u64 },
    #[error("nada para desfazer")]
    NothingToUndo,
    #[error("nada para refazer")]
    NothingToRedo,
    #[error("blob ausente: {0}")]
    MissingBlob(String),
    #[error("projeto aberto em outro lugar: {0}")]
    Locked(String),
    #[error("erro de E/S: {0}")]
    Io(String),
    #[error("JSON inválido: {0}")]
    Json(String),
}

impl CoreError {
    pub fn invalid(field: impl Into<String>, detail: impl Into<String>) -> Self {
        CoreError::Invalid { field: field.into(), detail: detail.into() }
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(e: serde_json::Error) -> Self {
        CoreError::Json(e.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
