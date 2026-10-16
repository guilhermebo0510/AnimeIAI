#![recursion_limit = "512"]
//! # aiai-mcp
//!
//! Três jeitos de operar o AnimeIAI sem mexer na janela (ADR-004):
//!
//! * servidor MCP em stdio (`aiai-mcp` sem argumentos) — ferramentas genéricas e estáveis `aiai_*`;
//! * CLI `aiai-mcp call <AÇÃO> '<json>' [--png out.png] [--project <pasta>]` — para a Bash;
//! * headless — quando o app não está aberto (ou com `--project`), as ações de documento rodam em processo sobre
//!   uma `aiai_core::CoreSession` aberta da pasta `.aiai` e o projeto é salvo depois de cada mudança.
//!
//! O arquivo de descoberta da bridge é relido a cada chamada, então o MCP sobrevive a reinício/recompilação do app.

pub mod bridge_client;
pub mod cli;
pub mod context;
pub mod headless;
pub mod mcp;
pub mod sandbox;
pub mod tools;
pub mod tools_preprod;
pub mod tools_production;
