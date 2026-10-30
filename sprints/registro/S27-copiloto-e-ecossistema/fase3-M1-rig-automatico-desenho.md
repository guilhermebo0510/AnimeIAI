# Fase 3, M1: riggar o modelo do próprio usuário (desenho, antes de codar)

> Escrito em 30/10 (~19:00) enquanto o Codex audita a rodada 4 do Copiloto (sem build nem GPU nesse período).
> Base: `fase3-estado-e-lacunas.md`, item Modelagem 1. Leitura do código: `aiai-3d-charbase` (esqueleto VRM de 55 ossos
> e pesos presos à topologia do manequim), `aiai-3d-core/src/mesh.rs` (o `Vertex` já tem `joints: [u16; 4]` e pesos, e
> o GLB grava e lê skin).

## O problema

Hoje só o manequim tem esqueleto. Os pesos dele (`weights.rs`) dependem das regiões da topologia do manequim
(landmarks, `Region`), então não servem para um modelo importado ou modelado pelo usuário.

## O que o usuário faz (no estilo do Blender, "Com pesos automáticos")

1. Na Modelagem, com o modelo selecionado: **Esqueleto → Humanoide**. O esqueleto VRM do manequim aparece escalado à
   caixa do modelo: altura pela caixa; quadril, ombros e cabeça pelas fatias horizontais da malha (largura máxima
   em cada altura).
2. **Encaixar**: arrastar as juntas na vista (frente e lado), com espelho esquerda/direita ligado por padrão. Uma
   junta arrastada move os filhos junto (Shift move só ela).
3. **Pesar automaticamente**: um botão. Barra de progresso; dá para cancelar.
4. **Conferir e corrigir**: posar (já existe: arrastar juntas, IK, poses prontas) e ver a deformação; **pintar pesos**
   (pincel somar/subtrair/suavizar por osso, com a cor do peso na malha).
5. O modelo riggado vai para o Layout como camada 3D de referência, com as mesmas poses por quadro do manequim.

## Pesos automáticos: calor dos ossos (Baran e Popović, 2007; é o do Blender)

Para cada osso *i*, resolver `(−L + H) wᵢ = H pᵢ`, onde:

- `L` é o Laplaciano cotangente da malha (vértices × vértices, esparso, simétrico);
- `H` é diagonal: `H_jj = c / d_j²`, com `d_j` a distância do vértice *j* ao osso mais perto **que ele enxerga** (o
  segmento até o osso não atravessa a malha), e 0 se nenhum osso é visível; `c ≈ 1`;
- `pᵢ_j = 1` se o osso mais perto de *j* é *i*, senão 0.

Depois: normalizar a soma para 1, ficar com as 4 maiores influências, renormalizar.

**Resolver sem dependência nova:** gradiente conjugado com pré-condicionador de Jacobi (a matriz é simétrica e
positiva definida quando há algum `H > 0`). Uma fatoração serve para todos os ossos no Blender; aqui, CG por osso com o
resultado do osso anterior como palpite dá conta de malhas de referência (até ~50 mil vértices, alvo < 10 s na CPU do
dono). Acima disso, decimar uma cópia (o decimate já existe nos modificadores), pesar a cópia e levar os pesos ao
original pelo vértice mais perto.

**Visibilidade**: raio do vértice ao ponto mais perto do osso contra a malha (BVH simples por triângulos; a malha de
referência cabe). Sem nenhum osso visível (vértice dentro de uma dobra), vale o mais perto, com `H` reduzido.

**Casos que o manequim já ensinou** (`weights.rs`): isolar esquerda e direita no fim (nada de peso do braço esquerdo no
direito), ossos de torção recebem parte do peso do osso pai por interpolação ao longo do comprimento, dedos com
alcance curto.

## Malha que não é "uma peça só"

Modelos importados têm partes soltas (olho, cabelo, roupa). O Laplaciano de uma peça solta sem osso visível fica
singular. Regra: cada componente conexa é resolvida à parte; componente pequena (< 2% dos vértices) recebe o peso
inteiro do osso mais perto do centro dela (olho → cabeça, presilha → cabeça), como o "parent to bone" do Blender.

## Testes antes de ligar no app

- Cilindro com dois ossos em linha: o peso do osso de cima cai de 1 a 0 suavemente perto da junta, monotônico ao longo
  do eixo, soma 1 em todo vértice.
- Duas peças soltas (cilindro + esfera pequena): a esfera inteira no osso mais perto.
- O manequim pesado por calor × os pesos dele de hoje: diferença média pequena nos braços e pernas (régua de que o
  método genérico não piora o caso conhecido).
- Posar o modelo pesado (cotovelo a 90°) e conferir que nenhum vértice anda mais que o comprimento do osso.

## MCP

`aiai_modeling` ganha `rig_humanoid {object}` (cria e encaixa pela caixa), `rig_joint {object, joint, pos}`,
`rig_weights {object}` (calor) e `rig_paint {object, bone, op, verts|brush}`. Tudo no undo.

## Fora deste item

VRM (M2), linha do tempo das chaves de pose (M4) e câmera 3D animada (M5) usam o mesmo esqueleto, mas ficam nos seus
itens.
