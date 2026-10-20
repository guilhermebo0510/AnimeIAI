//! Ferramenta curada da biblioteca de recursos (S24 2.7–2.8): `aiai_library` lista/busca, importa, usa no projeto, salva do
//! projeto, edita metadados, duplica e apaga recursos de `Documentos\AnimeIAI\{Modelos 3D, Materiais, Texturas, Paletas,
//! Poses, Pincéis}`. Açúcar sobre `aiai_bridge::library_tools`; roda no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const LIBRARY_DESC: &str = r##"Biblioteca de RECURSOS do usuário, compartilhada entre projetos e workspaces (a mesma do painel Biblioteca): modelos 3D, materiais, texturas, paletas, poses e pincéis em `Documentos\AnimeIAI\{Modelos 3D, Materiais, Texturas, Paletas, Poses, Pincéis}`. A pasta é a verdade; cada recurso tem manifesto com nome, etiquetas, autor, LICENÇA e fonte (importar sem informar a licença grava "desconhecida — verifique com o autor"). Sem sessão (app com --no-session) a biblioteca está desligada. `op`:
- list | search {kind?: model3d|material|texture|palette|pose|brush, query?, tags?, sort?: name|newest|size|kind, limit?} → {items:[{id, kind, name, path, tags, author, license, source, stats, ...}], total, counts, tags, problems} (não altera nada; `problems` = arquivos que não deu para ler, com o motivo)
- import {path (absoluto), kind? (texture|material para imagem), name?, license?, asset_author?, source?, tags?, folder?} → importa .glb/.gltf/.obj (.vrm ainda não), imagens (.png .jpg .webp .bmp), paletas (.gpl GIMP, .ase, .aiaipal, JSON), poses (.aiaipose), pincéis nativos (.aiaibrush/.aiaibrushes; os de outros programas pela janela do painel Pincéis)
- use {id, cut?, mode?: layer|asset, layer_id?} → modelo 3D: camada 3D de referência no corte (`cut`) com a câmera enquadrada, ou só o ativo (`mode: asset`; o mesmo modelo não é copiado duas vezes); textura/material de imagem: camada de referência no corte ou ativo de textura; paleta: entra no projeto (devolve `palette`); material toon: aplica as cores à camada 3D `layer_id`; pincel: devolve o `preset` para aiai_paint. Um passo de undo.
- save {from: palette|model, palette? | asset? | layer_id?, name?, license?, asset_author?, source?, tags?} → grava na biblioteca uma paleta do projeto (inclusive a do figurino de um personagem) ou o modelo 3D de um ativo/camada (a licença da cópia da biblioteca acompanha)
- update {id, name?, description?, tags? (substitui), asset_author?, license?, source?} · duplicate {id} (id novo) · delete {id} (vai para `_Lixeira` na pasta do tipo; dá para recuperar à mão)
Com o app aberto, a ação de UI `library_import {files}` (aiai_ui) abre a janela de importar do painel com esses arquivos (licença e fonte à vista antes de gravar). Chave desconhecida é erro. Exemplo: {"op":"import","path":"C:\\AnimeIAI\\calibracao\\modelos\\manequim.glb","license":"CC0-1.0","tags":["base"]} e depois {"op":"use","id":"<id>","cut":"<corte>"}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_library", "description": LIBRARY_DESC, "inputSchema": object_schema::<aiai_tools::library::LibraryRequest>() })]
}

/// Plano da ferramenta da biblioteca; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_library" => Some(required(m, &["op"]).map(|_| Plan { action: "LIBRARY", params: Value::Object(m.clone()), save_png: None })),
        _ => None,
    }
}
