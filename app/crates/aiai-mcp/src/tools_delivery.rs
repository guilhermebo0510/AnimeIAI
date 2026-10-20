//! Ferramenta curada da Entrega (S24 4.8–4.12): `aiai_delivery` cuida da fila de exportação, dos destinos, dos
//! entregáveis, da verificação final e do pacote de entrega. Açúcar sobre `aiai_bridge::delivery_tools`; roda no app
//! (a fila que o painel mostra) e no headless (a chamada roda os itens).

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const DELIVERY_DESC: &str = r##"ENTREGA: fila de exportação, destinos, entregáveis, verificação final e pacote de entrega (o mesmo da workspace Entrega do app). O estado fica em `entrega.json` ao lado do projeto (o projeto precisa ter pasta). Formatos (`preset`): tv (ARIB −24 LKFS), web (−14 LUFS), youtube (1080p H.264), master (CRF 10, EBU), nvenc, prores, h265, webm (VP9), preview (360p leve e rápido), png (sequência), wav (mixagem), stems (um WAV por trilha), contact (folha de contato); `formats` diz quais o ffmpeg deste PC consegue e o motivo dos que não. `op`:
- formats → lista de formatos com `available`/`reason`, os tokens de nome e o modelo padrão
- settings {settings?: {dest: project|documents|custom, custom_dir, name_template, auto_version, extra_dirs, package}} → lê ou muda os destinos. Modelo de nome com {projeto} {episodio} {corte} {formato} {data} {hora} {fonte} {versao}; sem {versao} e com auto_version o nome ganha _v001, _v002... (nunca sobrescreve); extra_dirs recebem cópia de cada entrega; package gera o zip (vídeo, legendas, creditos.txt com as linhas VOICEVOX, pôster, checksums SHA-256)
- preflight {cut|episode|edit (+start/end) ou source, preset?, loudness?: true} → semáforo `checks` [{code, level: pass|warn|fail, message, args, fix?: {workspace, panel}}] + `blocking` (há falha) + `loudness` medido: codificador, duração, buracos e som além da imagem na edição, mídia danificada/ausente, loudness contra o alvo do formato, legendas fora de cps/linha, quadro ímpar ou de tamanhos diferentes, fontes ausentes, disco e pasta
- queue_add {cut|episode|edit ou source, preset | sources[] × presets[] (matriz), crf?, poster_frame?, run?: false} → põe na fila (nome e versão resolvidos); `run: true` já exporta (no headless a chamada espera acabar)
- queue_list · queue_remove {id} · queue_move {id, to} · queue_cancel {id} (na fila) · queue_retry {id} · queue_clear (tira concluídos/falhos/cancelados) · queue_pause · queue_resume · queue_run {limit?} → no app a fila anda sozinha com progresso por quadro; reabrir o projeto devolve a fila parada com o que faltava
- list {count?} → entregáveis (mais recentes primeiro): arquivos, formato, resolução, duração, tamanho, loudness, miniatura, pôster, cópias, pacote, `missing` (arquivo sumiu)
- delete {id} (vai para `_Lixeira` ao lado do arquivo) · package {id} (gera o zip) · poster {id, frame?, title?, subtitle?} (pôster 1280 px com título sobre a imagem) · reexport {id, run?} (mesmo pedido, versão seguinte)
Chave desconhecida é erro. Exemplo: {"op":"preflight","edit":"<edição>","preset":"youtube"} e depois {"op":"queue_add","edit":"<edição>","presets":["youtube","preview"],"run":true}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_delivery", "description": DELIVERY_DESC, "inputSchema": object_schema::<aiai_bridge::delivery_tools::DeliveryRequest>() })]
}

/// Plano da ferramenta de entrega; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_delivery" => Some(required(m, &["op"]).map(|_| Plan { action: "DELIVERY", params: Value::Object(m.clone()), save_png: None })),
        _ => None,
    }
}
