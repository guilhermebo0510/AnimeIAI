//! Biblioteca de recursos do AnimeIAI (S24 2.7–2.8): modelos 3D, materiais, texturas, paletas, poses e pincéis em
//! `Documentos\AnimeIAI\{Modelos 3D, Materiais, Texturas, Paletas, Poses, Pincéis}`.
//!
//! - A **pasta é a verdade**: o índice ([`index`]) é reconstruído a cada varredura, sem cache em disco que possa discordar.
//! - Todo recurso tem um **envelope** ([`envelope`]) com tipo, id, autor, licença, fonte, etiquetas, versão e datas;
//!   arquivos de terceiros (`.glb`, `.png`) levam o envelope num sidecar `<arquivo>.aiaiasset.json`, os formatos
//!   próprios em JSON (`.aiaipal`, `.aiaimat`, `.aiaipose`) são o próprio envelope e o `.aiaibrush` guarda o dele no zip
//!   (o manifesto do pincel é o mesmo de antes: [`envelope::Envelope::from_brush_manifest`]).
//! - Importação ([`import`]), gravação de metadados, duplicar e lixeira ([`store`]), paletas ([`palette`]) e miniaturas
//!   ([`thumb`]).
//! - Sem sessão (`--no-session`) e sem `AIAI_LIBRARY_DIR` nada é lido nem gravado ([`config::root`] devolve `None`).
//!
//! Contrato para quem grava na pasta (o trabalho de Modelagem salva modelos e poses): modelo = `.glb` em `Modelos 3D` (com
//! ou sem sidecar; sem sidecar o recurso entra com nome do arquivo e licença "não informada"); pose = `.aiaipose` em
//! `Poses` (envelope com `kind: "pose"`, a pose no `payload`) ou qualquer `.json` solto na pasta (a pose vai inteira no
//! `payload` inferido). Para gravar com manifesto use [`import::add_model_glb`] ou escreva o envelope.

pub mod config;
pub mod envelope;
pub mod import;
pub mod index;
pub mod kind;
pub mod palette;
pub mod store;
pub mod thumb;

pub use config::{bump, change_counter, enable_user_folders, root};
pub use envelope::{Envelope, parse_tags};
pub use index::{Entry, Index, Query, Sort, Stats, Storage};
pub use kind::Kind;
