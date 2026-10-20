//! Ferramenta curada das réguas editáveis (S24): `aiai_ruler` monta a perspectiva (1, 2 ou 3 pontos), paralelas, radial,
//! simetria ou grade em perspectiva numa camada de guia, as mesmas réguas que o app mostra no canvas e em que a caneta e
//! o pincel encaixam. Açúcar sobre `aiai_bridge::ruler_tools`; roda no app e no headless.

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const RULER_DESC: &str = r##"Réguas editáveis de uma camada de GUIA (`layer`, tipo guide), como as réguas de perspectiva do Clip Studio: o app desenha as linhas por cima do canvas (só visão; NUNCA entram no render nem na exportação, a não ser com op bake), mostra alças para arrastar e faz a caneta e o pincel encaixarem na régua ativa. Várias por camada, uma ativa. `op`:
- list → {rulers, active} (não altera nada)
- create {ruler:{kind, ...}, points?} → cria e ativa; `kind`: perspective (`vps` [[x,y]...] 1 a 3 pontos de fuga em px do canvas, podem ficar MUITO fora do canvas; sem vps nasce com `points` (padrão 2) em volta do meio; com 2/3 pontos o horizonte passa pelos dois primeiros, o terceiro é o vertical; 1 ponto usa `horizon_angle`; `density` raios por ponto 2..256) | parallel (`center` origem, `angle` graus, `spacing` px) | radial (`center`, `count` raios, `angle` do primeiro) | symmetry (`center`, `angle` do 1º eixo, `count` eixos 1..64, `mirror` true = espelho, false = só rotação; o traço é repetido nas cópias) | grid (`quad` 4 cantos sup-esq, sup-dir, inf-dir, inf-esq de um plano em perspectiva, `cells` [u,v], `extend` células além do quadrilátero: o plano continua até o horizonte; chão = quadrilátero mais estreito em cima)
- edit {id, ruler:{só os campos a mudar}, points?, eye_level?} → `points` troca o nº de pontos mantendo o horizonte; `eye_level` = y do horizonte no meio do canvas (os pontos do horizonte acompanham)
- activate {id} · delete {id}
- bake {id?, frame?, cut?} → acrescenta as linhas da régua (ou da ativa) como traços no cel da camada de guia (só aparecem com include_guides)
Campos comuns: `name`, `color` "#rrggbbaa", `visible`, `snap` (encaixe dos traços), `locked`. Chave desconhecida é erro. Cada mudança é um passo de undo; devolve o Outcome + `rulers` (como ficaram), `active`, `id`. Para desenhar linhas EM perspectiva com aiai_draw_curves, leia os `vps` e trace rumo a eles.
Exemplo: {"layer":"<camada de guia>","op":"create","ruler":{"kind":"perspective","vps":[[-1400,420],[3300,420]],"density":32}}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_ruler", "description": RULER_DESC, "inputSchema": object_schema::<aiai_tools::ruler::RulerRequest>() })]
}

/// Plano da ferramenta de réguas; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_ruler" => Some(required(m, &["layer", "op"]).map(|_| Plan { action: "RULER", params: Value::Object(m.clone()), save_png: None })),
        _ => None,
    }
}
