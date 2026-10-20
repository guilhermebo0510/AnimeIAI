//! Ferramentas curadas de pré-produção (S16: roteiro, beats, storyboard/e-conte, animatic e cortes a partir do
//! storyboard) e de produção/revisão (S19: tabela de cortes, etapas, lint de todos os quadros, correções, retakes,
//! relatório). Cada uma é açúcar sobre uma ação da bridge (`aiai_bridge::preprod_tools` / `review_tools`); as de
//! documento escolhem a operação por `op`. Rodam no app e no headless.

use crate::context::ToolError;
use crate::tools::{Plan, schema_of};
use serde_json::{Map, Value, json};

const SCRIPT_DESC: &str = r##"Roteiro do projeto (documento Script): cenas com elementos {kind: action|dialogue|parenthetical|transition|note, character (id), speaker, text (idioma original, ex. japonês), translation (legenda, ex. português), emotion, direction, est_seconds}. `op`:
- create {name, title?, language?, translation_language?, scenes:[{heading, elements:[...]}]} → {doc, script}
- read {doc} → roteiro com ids + estimated_seconds
- edit {doc, ops:[{op:"insert_scene",heading,index?} | {op:"update_scene",scene,heading} | {op:"delete_scene",scene} | {op:"move_scene",scene,index} | {op:"insert_element",scene,element,index?} | {op:"update_element",element,patch} | {op:"delete_element",element} | {op:"move_element",element,scene,index?} | {op:"set_meta",title?,language?,translation_language?}]} → ids criados (um passo de undo)
- timing {doc} → segundos estimados por elemento/cena (moras/s por idioma), sem gravar
- apply_timing {doc} → grava est_seconds nos elementos que ainda não têm
- to_fountain {doc, out?} / from_fountain {name, fountain}
O roteiro é a fonte das falas (aiai_tts_line), das legendas (aiai_edit) e da duração dos painéis (aiai_storyboard).
Exemplo: {"op":"create","name":"Roteiro","title":"Chuva de verão","scenes":[{"heading":"EXT. PONTE - TARDE","elements":[{"kind":"action","text":"雨が降り始める。","translation":"Começa a chover."},{"kind":"dialogue","speaker":"ハル","text":"傘、持ってる？","translation":"Você trouxe guarda-chuva?"}]}]}"##;

const BEATS_DESC: &str = r##"Quadro de beats (estrutura da história em atos com cartões e segundos-alvo; documento Notes schema "beats"). `op`:
- create {name, board?:{acts:[{name, beats:[{id,title,summary,target_seconds}]}]}} → {doc}
- read {doc} → board + total_target_seconds
- edit {doc, ops:[{op:"add_act",name} | {op:"add_beat",act (índice),title,summary?,target_seconds?} | {op:"update_beat",beat,title?,summary?,target_seconds?} | {op:"delete_beat",beat} | {op:"move_beat",beat,act,index?}]}
Exemplo: {"op":"create","name":"Beats"} e depois {"op":"edit","doc":"<id>","ops":[{"op":"add_act","name":"Início"},{"op":"add_beat","act":0,"title":"Encontro na ponte","target_seconds":12}]}"##;

const STORYBOARD_DESC: &str = r##"Storyboard / e-conte (documento Storyboard). Cada painel tem um corte pequeno próprio (canvas do storyboard, padrão 480×270) com a camada vetorial "rascunho": desenhe nele com aiai_draw_curves/aiai_paint usando o `drawing.cut`/`drawing.layer` devolvidos (quadro 0). `op`:
- create {name, title?, canvas?:[w,h]} → {storyboard}
- read {storyboard} → painéis + total_seconds
- panel_add {storyboard, index?, spec:{cut_number?, shot: establishing|wide|full|medium|medium_close|close|extreme_close|detail|over_shoulder, angle: eye|high|low|dutch|bird|worm, action, dialogue_refs:[ids de elementos do roteiro], dialogue_text, sfx, seconds (padrão 3), camera_move?:{kind: pan|tilt|zoom_in|zoom_out|track|shake|none, from:{center:[0..1,0..1],zoom}, to:{...}}, scene? (id da cena do roteiro)}} → {panel, drawing:{cut,layer,cel}}
- panel_update {storyboard, panel, patch:{... mesmos campos, clear_camera}} / panel_delete / panel_move {index} / renumber {step?}
- panel_drawing {storyboard, panel} → {cut, layer, cel}
- sheet {storyboard, out: .png, options?:{width}} → folha de e-conte (Nº | painel com quadro/seta de câmera | ação | fala/SE | segundos). Olhe a imagem.
Depois: aiai_animatic (timing) e aiai_cuts_from_storyboard (gera os cortes de produção).
Exemplo: {"op":"panel_add","storyboard":"<id>","spec":{"shot":"wide","action":"Haru corre na ponte","seconds":2.5,"camera_move":{"kind":"pan","from":{"center":[0.3,0.5],"zoom":1.2},"to":{"center":[0.7,0.5],"zoom":1.2}}}}"##;

const ANIMATIC_DESC: &str = r##"Renderiza o animatic do storyboard em MP4 (H.264, sem áudio): cada painel pela duração dele (segundos × fps). `captions` põe a fala do painel como legenda, `cut_numbers` o Nº do corte no canto; `width`/`height` (pares; padrão o tamanho do projeto), `fps` (padrão o do projeto). Devolve frames, seconds e o início/duração de cada painel. Use para conferir o ritmo antes de animar.
Exemplo: {"storyboard":"<id>","out":"C:\\AnimeIAI\\saida\\animatic.mp4","width":960,"height":540,"captions":true,"cut_numbers":true}"##;

const CUTS_FROM_STORYBOARD_DESC: &str = r##"Gera as cenas e os cortes de produção a partir do storyboard, num passo de undo: um corte por painel com número, duração (segundos × fps), nota = ação, falas vinculadas, câmera inicial (e chaves de pan/zoom se o painel tiver `to`) e uma camada de REFERÊNCIA com o desenho do painel escalado ao tamanho do projeto (para o layout decalcar). `scene_per_panel_group`: uma cena por grupo de painéis consecutivos com a mesma `scene` (senão uma cena só); `script`: nomeia as cenas pelo cabeçalho do roteiro; `fps` opcional. Devolve {scenes, cuts:[{panel, cut}]}.
Exemplo: {"storyboard":"<id>","episode":"<episódio>","scene_per_panel_group":true,"script":"<roteiro>"}"##;

const PRODUCTION_DESC: &str = r##"Estado da produção (S19). `op`:
- cut_table → カット表: cortes na ordem (número, cena, duração, as 8 etapas layout/genga/douga/shiage/background/satsuei/color/sound com estado not_started|in_progress|review|approved|retake, correções e retakes abertos) + total em segundos
- set_stage {cut, stage, status}
- suggest_stages {cut, apply?} → avanços de etapa deduzidos do conteúdo do corte (nunca rebaixa aprovadas/retake); apply grava num passo de undo
- lint_cut {cut} → lint de TODAS as camadas de preenchimento em todos os quadros expostos (regiões sem cor; clean = true se nenhuma). Use antes de exportar.
- provenance {id, history?} → quem criou/mudou o nó (+ últimas entradas do log que o tocam)
- report {out?: .md} → relatório de produção em Markdown (progresso, bloqueios, correções, retakes, lint)
Gestão (S24; prazo/responsável/peso opcionais por corte):
- set_stages {cuts:[...], stage, status} → a mesma etapa em vários cortes (um passo de undo)
- set_meta {cuts:[...], due?:"AAAA-MM-DD", assignee?, weight?} → vazio/<=0 limpa o campo (um passo de undo)
- history {cut?, limit?} → eventos de status {corte, etapa, de, para, t_ms, autor} (índice do log, gravado em producao-historico.ndjson)
- stats {stuck_days?} / bottlenecks {stuck_days?} → contagens por etapa, matriz cena×etapa, fluxo cumulativo, burndown, vazão semanal, tempo por etapa, taxa de retake; fila/vazão/espera por etapa e achados (gargalo, ociosa, travados, atrasados, retakes)
- analysis_package {out?, stuck_days?} → pacote de dados para a análise por IA (com o roteiro do que devolver). NUNCA mude o projeto por causa da análise: devolva o texto com note_add {text, cut?, ai:true}
- stats_csv {out?} → os números dos gráficos em CSV; note_add {text, cut?, ai?} / note_list / note_delete {note} → notas de gestão com carimbo de hora
Exemplo: {"op":"lint_cut","cut":"<corte>"}"##;

const CORRECTION_DESC: &str = r##"Correção de revisão: traços em VERMELHO numa camada de guia "correção" do corte (não aparece no render final) + nota no documento "Produção". `op`:
- add {cut, frame (0-based), note, strokes?:[curvas como em aiai_draw_curves: {points,width,...}; a cor é sempre vermelha], author?} → {correction}
- resolve {correction, resolved? (padrão true; false reabre), by?, retake?}
- list {cut}
Exemplo: {"op":"add","cut":"<corte>","frame":12,"note":"olho esquerdo maior que o model sheet","strokes":[{"points":[[900,400],[960,380],[1000,420]],"width":4}]}"##;

const RETAKE_DESC: &str = r##"Retake: guarda o render atual de um quadro como snapshot (antes da refeitura) e compara depois. `op`:
- create {cut, frame, reason, author?} → {retake}
- compare {retake, frame?, out?: .png, panel_w?} → fração de pixels mudados, média/máx. da diferença; com `out`, folha ANTES | DEPOIS | DIFERENÇA (olhe a imagem)
- resolve {retake, resolved?} / list
Exemplo: {"op":"compare","retake":"<id>","out":"C:\\AnimeIAI\\producoes\\curta\\retake1.png"}"##;

/// Schema de tipo-união (`oneOf` por `op`) com `type: object` no topo (os clientes MCP exigem).
pub(crate) fn object_schema<T: schemars::JsonSchema>() -> Value {
    let mut s = schema_of::<T>();
    if let Some(o) = s.as_object_mut() {
        o.entry("type").or_insert(json!("object"));
    }
    s
}

pub fn definitions() -> Vec<Value> {
    use aiai_bridge::preprod_tools as pp;
    vec![
        json!({ "name": "aiai_script", "description": SCRIPT_DESC, "inputSchema": object_schema::<pp::ScriptRequest>() }),
        json!({ "name": "aiai_beats", "description": BEATS_DESC, "inputSchema": object_schema::<pp::BeatsRequest>() }),
        json!({ "name": "aiai_storyboard", "description": STORYBOARD_DESC, "inputSchema": object_schema::<pp::StoryboardRequest>() }),
        json!({ "name": "aiai_animatic", "description": ANIMATIC_DESC, "inputSchema": object_schema::<pp::AnimaticRequest>() }),
        json!({ "name": "aiai_cuts_from_storyboard", "description": CUTS_FROM_STORYBOARD_DESC, "inputSchema": object_schema::<aiai_tools::preprod::CutsFromStoryboardRequest>() }),
        json!({ "name": "aiai_production", "description": PRODUCTION_DESC, "inputSchema": object_schema::<aiai_bridge::review_tools::ProductionRequest>() }),
        json!({ "name": "aiai_correction", "description": CORRECTION_DESC, "inputSchema": object_schema::<aiai_bridge::review_tools::CorrectionOp>() }),
        json!({ "name": "aiai_retake", "description": RETAKE_DESC, "inputSchema": object_schema::<aiai_bridge::review_tools::RetakeOp>() }),
    ]
}

pub(crate) fn required(m: &Map<String, Value>, keys: &[&str]) -> Result<(), ToolError> {
    for k in keys {
        if m.get(*k).is_none_or(Value::is_null) {
            return Err(ToolError::invalid(format!("Missing '{k}': veja a descrição da ferramenta")));
        }
    }
    Ok(())
}

/// Plano de uma ferramenta de pré-produção; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    let with_req = |keys: &[&str], action: &'static str| -> Result<Plan, ToolError> {
        required(m, keys)?;
        Ok(Plan { action, params: Value::Object(m.clone()), save_png: None })
    };
    Some(match name {
        "aiai_script" => with_req(&["op"], "SCRIPT"),
        "aiai_beats" => with_req(&["op"], "BEATS"),
        "aiai_storyboard" => with_req(&["op"], "STORYBOARD"),
        "aiai_animatic" => with_req(&["storyboard", "out"], "ANIMATIC"),
        "aiai_cuts_from_storyboard" => with_req(&["storyboard", "episode"], "CUTS_FROM_STORYBOARD"),
        "aiai_production" => with_req(&["op"], "PRODUCTION"),
        "aiai_correction" => with_req(&["op"], "CORRECTION"),
        "aiai_retake" => with_req(&["op"], "RETAKE"),
        _ => return None,
    })
}
