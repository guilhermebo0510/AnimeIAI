//! # aiai-studio
//!
//! Shell nativo do AnimeIAI (S00): janela eframe/wgpu, tema claro/escuro, i18n PT/EN, icones SVG,
//! workspaces com docking (`egui_tiles`), menus, barra de status e preferencias.
//! O canvas real, o projeto e a bridge entram nas sprints seguintes.

pub mod app;
pub mod brand;
pub mod cli;
pub mod i18n;
pub mod icons;
pub mod keymap;
pub mod layout;
pub mod prefs;
pub mod sysmem;
pub mod theme;
pub mod workspace;
