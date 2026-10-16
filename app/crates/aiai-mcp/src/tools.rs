//! Catálogo das ferramentas MCP genéricas e estáveis (a lista não muda entre sprints, então a sessão do agente não precisa
//! reiniciar) e o despacho delas para as ações da bridge. As descrições são em português: é o que o agente lê.
//!
//! Ferramentas curadas de alto nível (desenhar curva, preencher…) entram nas sprints seguintes como açúcar sobre
//! comandos do Command Bus, nunca como caminho paralelo.

use crate::context::{Ctx, ToolError, mcp_image, take_png};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

pub struct ToolResult {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn error(e: &ToolError) -> Self {
        ToolResult { content: vec![json!({"type": "text", "text": format!("Falha (código {}): {}", e.code, e.message)})], is_error: true }
    }
}

const APPLY_DESC: &str = "Aplica UM comando ao documento (entra no undo/redo e aparece ao vivo no app). Devolve o Outcome: revision, description, created (ids criados, na ordem), affected, can_undo, can_redo. \
Use `if_revision` (a revision que você viu por último) para não pisar em mudança feita por outra pessoa/agente: se não bater, falha com código -32010 e nada muda. \
Ids novos podem ser omitidos (a sessão gera) — leia-os em `created`. \
Hierarquia: episode > scene > cut > layer > cel (palette/character/asset/document são raízes). \
Cola dos comandos principais (campo `kind`):\n\
- create_node {name, parent?, index?, id?, data}: data é {\"type\":\"episode\",\"number\":1} | {\"type\":\"scene\"} | {\"type\":\"cut\",\"duration\":48} | {\"type\":\"layer\",\"kind\":\"vector\"} (kinds: group, vector, raster, fill, text, guide, reference, adjustment, effect, particles) | {\"type\":\"cel\",\"number\":1}. Sem parent = raiz (só episode/character/palette/asset/document).\n\
- set_props {id, name?, data?}: renomeia e/ou aplica um JSON Merge Patch em `data` do nó (ex.: {\"opacity\":0.5,\"blend\":\"multiply\"}).\n\
- delete_node {id}: apaga o nó e a subárvore. move_node {id, parent?, index?}.\n\
- insert_strokes {cel, items:[{index, stroke}]}: stroke = {\"id\":\"\",\"points\":[{\"x\":10,\"y\":20,\"pressure\":0.8}],\"geometry\":\"spline\",\"width\":4,\"tip\":\"g_pen\",\"color\":{\"type\":\"direct\",\"rgba\":[0,0,0,1]}}; coordenadas em pixels do quadro do corte.\n\
- set_tiles {cel, tiles:[{x,y,blob}]} (raster; blob = hash já gravado).\n\
- add_track {cut, track:{\"id\":\"\",\"name\":\"A\",\"kind\":\"drawing\",\"layer\":\"<id da camada>\"}}; set_cells {cut, track, cells:[{frame, cell:{\"type\":\"cel\",\"number\":1}|{\"type\":\"empty\"}|null}]} (quadro 0-based).\n\
- batch {commands:[...], label?}: vários comandos num único passo de undo (ids dados explicitamente em create_node permitem referenciar dentro do lote).\n\
`aiai_schema {\"which\":\"commands\"}` devolve o JSON Schema completo de todos os comandos.";

pub fn definitions() -> Value {
    json!([
        {
            "name": "aiai_status",
            "description": "Estado do AnimeIAI: com o app aberto (--bridge) devolve projeto, revisão, histórico, FPS/GPU; sem o app devolve o estado do modo headless (projeto em disco, se houver). Chame primeiro para saber se há app ou só headless.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "aiai_apply",
            "description": APPLY_DESC,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": { "type": "object", "description": "Comando do Command Bus: objeto com `kind` (create_node, set_props, delete_node, move_node, insert_strokes, set_tiles, set_cells, add_track, batch...).", "properties": { "kind": { "type": "string" } }, "required": ["kind"] },
                    "if_revision": { "type": "integer", "minimum": 0, "description": "Só aplica se a revisão atual do documento for esta." },
                    "author": { "type": "string", "description": "Autor no log (padrão agent:claude)." }
                },
                "required": ["command"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_query",
            "description": "Consulta o documento sem alterá-lo. `query` é um objeto com `kind`: project {include_nodes?}, node {id}, tree {id?, depth?} (árvore resumida: id, nome, tipo, filhos), roots {category}, xsheet {cut}, exposure {cut, frame}, change_frames {cut}, log {from?, limit?, include_commands?}, history, schema {which}, stats, validate. \
Confirme sempre o efeito de um aiai_apply lendo de volta (tree/node), nunca pelo 'ok' do retorno. `aiai_schema {\"which\":\"queries\"}` lista todas.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "object", "description": "Consulta: objeto com `kind`.", "properties": { "kind": { "type": "string" } }, "required": ["kind"] } },
                "required": ["query"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_undo",
            "description": "Desfaz o último passo do histórico (um batch conta como um passo). Devolve o Outcome com can_undo/can_redo. No modo headless via CLI não há histórico entre processos ('nada para desfazer'); dentro do servidor MCP o histórico vale enquanto o processo viver.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "aiai_redo",
            "description": "Refaz o último passo desfeito. Vale só até o próximo aiai_apply.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "aiai_log",
            "description": "Lê o log da sessão (quem fez o quê: seq, revision, op apply/undo/redo, description, author, time). `include_commands:true` inclui o comando de cada entrada. Pagina com from/limit; devolve `total`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "integer", "minimum": 0, "description": "Primeira entrada (padrão 0)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Quantidade (padrão 100)." },
                    "include_commands": { "type": "boolean", "description": "Inclui o JSON do comando (padrão false)." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_project",
            "description": "Arquivo do projeto, sempre com caminho explícito (sem diálogo do SO). op=new {name, path?, settings?, overwrite?}: cria projeto (com `path` já salva a pasta .aiai; recusa sobrescrever um projeto existente sem overwrite:true). op=open {path}: abre a pasta .aiai. op=save {path?}: salva (sem path, na pasta atual). \
Só grava em pastas permitidas (AIAI_MCP_ALLOWED_DIRS; padrão producoes/, calibracao/, sprints/registro/evidencias/, temp). Sem o app aberto roda em modo headless: depois de new/open as demais ferramentas de documento passam a atuar nesse projeto, e cada mudança é salva sozinha.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "op": { "type": "string", "enum": ["new", "open", "save"] },
                    "path": { "type": "string", "description": "Pasta do projeto (.aiai), caminho absoluto." },
                    "name": { "type": "string", "description": "Nome do projeto (op=new)." },
                    "settings": { "type": "object", "description": "op=new: width, height, fps_num, fps_den, dialogue_language, subtitle_language, audio_sample_rate (o que faltar fica no padrão 1920x1080, 24000/1001)." },
                    "overwrite": { "type": "boolean", "description": "op=new: permite recomeçar sobre um projeto existente." }
                },
                "required": ["op"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_render",
            "description": "Renderiza um quadro (ou intervalo) de um corte com o motor real e devolve a imagem (conteúdo de imagem MCP, reduzida) e/ou grava em `out`. Parâmetros: cut (id do corte), frame (0-based) ou range [início, fim], size [largura, altura], backend cpu|gpu, layers (ids), out (pasta/arquivo permitido). Exige o app aberto; enquanto a sprint de render não termina o app responde com erro claro.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cut": { "type": "string", "description": "Id do corte." },
                    "frame": { "type": "integer", "minimum": 0 },
                    "range": { "type": "array", "items": { "type": "integer", "minimum": 0 }, "minItems": 2, "maxItems": 2 },
                    "size": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "minItems": 2, "maxItems": 2 },
                    "backend": { "type": "string", "enum": ["cpu", "gpu"] },
                    "layers": { "type": "array", "items": { "type": "string" } },
                    "out": { "type": "string", "description": "Destino (dentro das pastas permitidas)." }
                },
                "required": ["cut"],
                "additionalProperties": true
            }
        },
        {
            "name": "aiai_compare",
            "description": "Compara duas imagens (arquivos ou quadros renderizados) e devolve métricas (a, b, metrics[], mask, out_diff). Serve à validação visual contra a referência. Exige o app aberto; enquanto a sprint de métricas não termina o app responde com erro claro.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "a": { "description": "Imagem A (caminho ou {cut, frame})." },
                    "b": { "description": "Imagem B (caminho ou {cut, frame})." },
                    "metrics": { "type": "array", "items": { "type": "string" } },
                    "mask": { "type": "string" },
                    "out_diff": { "type": "string", "description": "Onde gravar a imagem de diferença (pasta permitida)." }
                },
                "additionalProperties": true
            }
        },
        {
            "name": "aiai_screenshot",
            "description": "Captura a janela do app (framebuffer do egui) ou só um painel/retângulo. Devolve a imagem (reduzida a no máximo 1280 px no lado maior) e, com `out`, grava o PNG em tamanho real numa pasta permitida. `region` = {x,y,w,h} em pontos lógicos; `panel` = nome do painel (veja aiai_ui get_state). Exige o app aberto com --bridge.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "region": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" }, "w": { "type": "number" }, "h": { "type": "number" } }, "required": ["x", "y", "w", "h"] },
                    "panel": { "type": "string" },
                    "out": { "type": "string", "description": "Caminho do PNG a gravar (pasta permitida)." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_input",
            "description": "Injeta eventos de entrada DENTRO do egui (sem tocar no cursor do SO), em pontos lógicos. `events` é uma lista de objetos com `kind`: mouse_move {x,y}, mouse_down/mouse_up {x,y,button: primary|secondary|middle}, wheel {dx,dy}, key {key, pressed, modifiers?}, text {text}, pen {x,y,pressure,phase: down|move|up} (caneta simulada com pressão). Os eventos entram no próximo quadro, em ordem. Exige o app aberto; depois confirme o efeito com aiai_ui get_state / aiai_screenshot.",
            "inputSchema": {
                "type": "object",
                "properties": { "events": { "type": "array", "minItems": 1, "maxItems": 2000, "items": { "type": "object", "properties": { "kind": { "type": "string" } }, "required": ["kind"], "additionalProperties": true } } },
                "required": ["events"],
                "additionalProperties": false
            }
        },
        {
            "name": "aiai_ui",
            "description": "Estado e ações da interface. action=get_state devolve workspace, painéis com retângulos (lógicos e físicos), ferramenta ativa, foco, diálogos, escala de DPI e tamanho da janela. Outras ações (UI_ACTION): set_workspace {workspace}, open_panel {panel}, set_tool {tool}, open_dialog {dialog}... — os campos extras vão junto. Exige o app aberto.",
            "inputSchema": {
                "type": "object",
                "properties": { "action": { "type": "string", "description": "get_state | set_workspace | open_panel | set_tool | open_dialog | ..." } },
                "required": ["action"],
                "additionalProperties": true
            }
        },
        {
            "name": "aiai_schema",
            "description": "Autodescrição: devolve o JSON Schema de `commands` (todos os comandos aceitos por aiai_apply), `queries` (todas as consultas de aiai_query) ou `project` (formato do documento). Use para descobrir comandos novos sem reiniciar a sessão.",
            "inputSchema": {
                "type": "object",
                "properties": { "which": { "type": "string", "enum": ["commands", "queries", "project"] } },
                "additionalProperties": false
            }
        }
    ])
}

fn args_object(args: &Value) -> Result<Map<String, Value>, ToolError> {
    match args {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(ToolError::invalid("os argumentos devem ser um objeto JSON")),
    }
}

fn require<'a>(m: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a Value, ToolError> {
    m.get(key).filter(|v| !v.is_null()).ok_or_else(|| ToolError::invalid(format!("Missing '{key}': {what}")))
}

/// Copia só as chaves presentes.
fn pick(m: &Map<String, Value>, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = m.get(*k).filter(|v| !v.is_null()) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    Value::Object(out)
}

/// Ação da bridge, parâmetros e (para screenshot) destino do PNG, a partir do nome da ferramenta e dos argumentos.
pub struct Plan {
    pub action: &'static str,
    pub params: Value,
    pub save_png: Option<String>,
}

pub fn plan(name: &str, args: &Value) -> Result<Plan, ToolError> {
    let m = args_object(args)?;
    let simple = |action: &'static str, params: Value| Ok(Plan { action, params, save_png: None });
    match name {
        "aiai_status" => simple("GET_STATUS", json!({})),
        "aiai_apply" => {
            let cmd = require(&m, "command", "objeto {\"kind\": ...}")?;
            if !cmd.is_object() {
                return Err(ToolError::invalid("'command' deve ser um objeto {\"kind\": ...}"));
            }
            simple("APPLY_COMMAND", pick(&m, &["command", "if_revision", "author"]))
        }
        "aiai_query" => {
            let q = require(&m, "query", "objeto {\"kind\": ...}")?;
            if !q.is_object() {
                return Err(ToolError::invalid("'query' deve ser um objeto {\"kind\": ...}"));
            }
            simple("QUERY", pick(&m, &["query"]))
        }
        "aiai_undo" => simple("UNDO", json!({})),
        "aiai_redo" => simple("REDO", json!({})),
        "aiai_log" => simple("GET_LOG", pick(&m, &["from", "limit", "include_commands"])),
        "aiai_project" => {
            let op = require(&m, "op", "new | open | save")?.as_str().unwrap_or("");
            let action = match op {
                "new" => "NEW_PROJECT",
                "open" => "OPEN_PROJECT",
                "save" => "SAVE_PROJECT",
                other => return Err(ToolError::invalid(format!("Invalid action: op deve ser new|open|save, veio {other:?}"))),
            };
            if op == "open" {
                require(&m, "path", "pasta .aiai a abrir")?;
            }
            simple(action, pick(&m, &["path", "name", "settings", "overwrite"]))
        }
        "aiai_render" => {
            require(&m, "cut", "id do corte")?;
            simple("RENDER", Value::Object(m))
        }
        "aiai_compare" => simple("COMPARE", Value::Object(m)),
        "aiai_screenshot" => {
            let mut params = pick(&m, &["region", "panel"]);
            // o PNG volta em base64 e é o MCP quem grava em `out` (já validado pelo sandbox)
            params["inline"] = json!(true);
            let save_png = m.get("out").and_then(Value::as_str).map(str::to_string);
            Ok(Plan { action: "SCREENSHOT", params, save_png })
        }
        "aiai_input" => {
            let ev = require(&m, "events", "lista de eventos")?;
            if !ev.as_array().is_some_and(|a| !a.is_empty()) {
                return Err(ToolError::invalid("'events' deve ser uma lista não vazia de objetos com `kind`"));
            }
            simple("INPUT", pick(&m, &["events"]))
        }
        "aiai_ui" => {
            let action = require(&m, "action", "get_state | set_workspace | ...")?.as_str().ok_or_else(|| ToolError::invalid("'action' deve ser texto"))?;
            if action == "get_state" {
                simple("GET_UI_STATE", json!({}))
            } else {
                simple("UI_ACTION", Value::Object(m))
            }
        }
        "aiai_schema" => {
            let which = m.get("which").and_then(Value::as_str).unwrap_or("commands");
            simple("QUERY", json!({ "query": { "kind": "schema", "which": which } }))
        }
        other => Err(ToolError::invalid(format!("Unknown tool: {other}"))),
    }
}

/// Executa uma ferramenta: monta o plano, roteia (app/headless) e formata o conteúdo MCP (texto + imagem opcional).
pub fn call_tool(ctx: &mut Ctx, name: &str, args: &Value) -> ToolResult {
    match run(ctx, name, args) {
        Ok(r) => r,
        Err(e) => ToolResult::error(&e),
    }
}

fn run(ctx: &mut Ctx, name: &str, args: &Value) -> Result<ToolResult, ToolError> {
    let plan = plan(name, args)?;
    let save_to: Option<PathBuf> = match &plan.save_png {
        Some(p) => Some(ctx.sandbox.check_write(p).map_err(|e| ToolError::invalid(format!("'out': {e}")))?),
        None => None,
    };
    let mut data = ctx.invoke(plan.action, plan.params, None)?;
    finish(&mut data, save_to.as_ref())
}

/// Formata a resposta: imagem (se houver `png_base64`) reduzida + texto com o resto.
pub fn finish(data: &mut Value, save_to: Option<&PathBuf>) -> Result<ToolResult, ToolError> {
    let png = take_png(data, save_to)?;
    let mut content = vec![json!({"type": "text", "text": serde_json::to_string(data).unwrap_or_default()})];
    if let Some(p) = png {
        content.push(mcp_image(&p.bytes));
    }
    Ok(ToolResult { content, is_error: false })
}
