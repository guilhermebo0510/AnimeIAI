//! Ferramenta curada da camada 3D de referência (S24 3D): `aiai_model3d` importa um modelo (.glb/.gltf/.obj) e cria a
//! camada 3D (referência por padrão: aparece no canvas, não sai no render final), lista, e muda transformação, câmera e
//! aparência. Açúcar sobre `aiai_bridge::model3d_tools`; roda no app (GPU) e no headless (rasterizador CPU).

use crate::context::ToolError;
use crate::tools::Plan;
use crate::tools_preprod::{object_schema, required};
use serde_json::{Map, Value};

const MODEL3D_DESC: &str = r##"Camada 3D de REFERÊNCIA (como a camada 3D do Clip Studio e o modelo de referência do Harmony): um modelo 3D desenhado por quadro no canvas com câmera própria, para posicionar e desenhar por cima (confira com aiai_render include_reference:true; no render final e na exportação a camada NÃO sai enquanto `reference` for true, o padrão). Unidades em metros, Y para cima. `op`:
- import {path (absoluto: .glb, .gltf ou .obj; .vrm ainda não), cut?, name?} ou {asset, cut} → cria o ativo 3D (GLB normalizado no projeto; licença do arquivo guardada em info.license) e, com `cut`, a camada no topo do corte com a câmera enquadrando o modelo. Limite: 500000 triângulos. Devolve asset, layer, info (triângulos, partes, caixa, licença) e layer_state.
- list → {models, layers} (não altera nada)
- transform {layer, pos?: [x,y,z], rot_deg?: [x,y,z] (Euler X→Y→Z), scale?: [x,y,z]}
- camera {layer, target?: [x,y,z], yaw_deg? (0 = de frente), pitch_deg? (+ = de cima), distance?, fov_deg? (1..150), frame_model?: true reenquadra o modelo}
- mode {layer, mode?: toon|flat|wire|silhouette, outline_px? (0 = sem contorno), outline_color? "#rrggbb", color? "#rrggbb" ("" = cores do modelo), reference?, opacity? 0..1}
- mannequin {cut, name?} → cria o MANEQUIM posável (corpo anime neutro com esqueleto humanoide VRM; ativo embutido, reusado no projeto) e a camada dele em pé no topo do corte.
- poses {layer?, frame?} → poses prontas (stand, walk, run, sit, point, tpose) e ossos com IK; com `layer`: a pose no quadro, as juntas posáveis com a posição no mundo (para mirar o IK) e os quadros com chave de pose (não altera nada)
- pose {layer, frame?, key?, reset?: true (volta à pose em T), preset?: stand|walk|run|sit|point|tpose, pose?: {rotations: {osso: [x,y,z,w]}, hips: [x,y,z]}, bones?: {osso: [rx,ry,rz] graus, Euler X→Y→Z nos eixos do mundo, delta sobre o repouso em T}, hips?: [x,y,z] m (y < 0 abaixa o corpo), mirror?: true (troca esquerda ↔ direita), ik?: {bone: leftHand|rightHand|leftFoot|rightFoot, target: [x,y,z] m no mundo}} → aplica nessa ordem. Só para camadas com esqueleto (Manequim ou .glb com pele; os ossos têm o nome do arquivo). Ossos do Manequim: hips, spine, chest, upperChest, neck, head, left/right + Shoulder, UpperArm, LowerArm, Hand, UpperLeg, LowerLeg, Foot, Toes e os dedos (leftIndexProximal...). No repouso o personagem olha para +Z, a esquerda dele é +X e os braços estão em T: {"leftUpperArm": [0,0,-70]} baixa o braço esquerdo, {"rightUpperArm": [0,0,70]} o direito, {"leftUpperLeg": [-30,0,0]} leva a coxa esquerda para a frente, {"leftLowerLeg": [40,0,0]} dobra o joelho.
Pose por quadro: chave `m3d.pose` (com `key: true` ou quando a pose já é animada); entre duas chaves a pose é misturada (slerp por osso) com o easing do trecho (`hold` segura).
Chaves por quadro: `frame` (padrão 0) e `key` (true = grava chave em toda propriedade mudada; sem `key`, só as propriedades que já têm chaves recebem chave, as outras mudam o valor-base). Cada mudança é um passo de undo; devolve o Outcome + model3d (parâmetros) + state (avaliado no quadro). Chave desconhecida é erro.
Exemplo: {"op":"import","path":"C:\\AnimeIAI\\calibracao\\modelos\\cubo.glb","cut":"<corte>"} e depois {"op":"camera","layer":"<camada>","yaw_deg":35,"pitch_deg":15}; {"op":"mannequin","cut":"<corte>"} e {"op":"pose","layer":"<camada>","preset":"walk","frame":0,"key":true}"##;

pub fn definitions() -> Vec<Value> {
    vec![serde_json::json!({ "name": "aiai_model3d", "description": MODEL3D_DESC, "inputSchema": object_schema::<aiai_tools::model3d::Model3dRequest>() })]
}

/// Plano da ferramenta da camada 3D; `None` se o nome não é daqui.
pub fn plan(name: &str, m: &Map<String, Value>) -> Option<Result<Plan, ToolError>> {
    match name {
        "aiai_model3d" => Some(required(m, &["op"]).map(|_| Plan { action: "MODEL3D", params: Value::Object(m.clone()), save_png: None })),
        _ => None,
    }
}
