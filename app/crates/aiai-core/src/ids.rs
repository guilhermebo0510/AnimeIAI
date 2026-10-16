use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Identificador estável de nó/objeto (UUID v7 em texto; ordenável pelo tempo de criação).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Id(pub String);

impl Id {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id(s.to_string())
    }
}

/// Gerador de ids injetável. `Uuid7` em produção; `Sequential` em testes (ids previsíveis, projeto byte a byte reprodutível).
#[derive(Debug, Clone)]
pub enum IdGen {
    Uuid7,
    Sequential { prefix: String, next: u64 },
}

impl IdGen {
    pub fn sequential(prefix: &str) -> Self {
        IdGen::Sequential { prefix: prefix.to_string(), next: 1 }
    }

    pub fn next(&mut self) -> Id {
        match self {
            IdGen::Uuid7 => Id(uuid::Uuid::now_v7().to_string()),
            IdGen::Sequential { prefix, next } => {
                let id = Id(format!("{prefix}{:06}", *next));
                *next += 1;
                id
            }
        }
    }
}
