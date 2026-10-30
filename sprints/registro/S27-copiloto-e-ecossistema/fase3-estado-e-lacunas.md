# Fase 3: o que já existe e o que falta (Modelagem e Composição)

> Levantamento pelo código e pelas descrições das ferramentas do MCP (30/10 de manhã, `ec0a63a`), antes das rodadas
> WS-R10 (Modelagem) e WS-R08 (Composição) com o Codex. Vale como hipótese a confirmar no app.

## Modelagem (pedido do dono: rig, animação 3D como referência, câmera)

**Já existe**

- Modelagem simples no estilo do modo Edição do Blender: primitivas (cubo, esfera, cilindro, cone, plano, toro),
  objetos num modelo, faces com normal e centro, extrusão e modificadores (`aiai_modeling`, `app/modeling/*`: gizmo,
  multivista, seleção, escultura, UV, normais).
- **Camada 3D de referência** (`aiai_model3d`): importar GLB/GLTF/OBJ (até 500 mil triângulos, licença guardada),
  transformar, câmera própria (yaw, pitch, distância, FOV, reenquadrar), modos de desenho (toon, chapado, arame,
  silhueta, contorno) e "não sai no render".
- **Manequim posável**: corpo anime neutro com esqueleto humanoide VRM (55 ossos, torção e busto) e pesos de skin, vindo
  do ANIGO (`aiai-3d-charbase`); poses prontas (em pé, andar, correr, sentar, apontar, T), ossos por rotação e IK, **pose
  por quadro com chave** e mistura entre chaves (slerp por osso, com easing); **posar na tela** (arrastar juntas e IK no
  canvas, `model3d.rs` press/drag/release e `paint_pose`), espelhar e uma **biblioteca de poses** do usuário (salvar,
  listar, apagar).
- Guia 3D (`aiai_guide3d`): cena de blocos e manequins vista por uma câmera vira linhas de guia no cel.
- Rig 2D (`aiai_rig`): pai e filho entre camadas e IK de dois ossos.

**Falta (a confirmar na rodada)**

1. **Riggar o modelo do próprio usuário**: hoje o esqueleto só vem no manequim. Falta pôr um esqueleto num modelo
   importado ou modelado (encaixar o esqueleto humanoide nas proporções do modelo) e pesar automaticamente (pesos por
   difusão de calor, como o "Com pesos automáticos" do Blender), com pintura de pesos para corrigir.
2. **VRM** (o formato de personagem anime com esqueleto pronto): o import diz "ainda não".
3. Posar: já há arrasto de juntas, IK, espelhar e biblioteca de poses. Falta conferir o gizmo de rotação por osso,
   copiar e colar pose entre quadros e a biblioteca **do projeto** (hoje é do usuário, numa pasta dele).
4. **Animar como referência**: linha do tempo da camada 3D com as chaves de pose e de câmera visíveis e editáveis
   (mover, apagar, easing), ciclos (andar em loop), e a passagem para o 2D (cada quadro vira guia de linha para o genga).
5. **Câmera 3D animada**: caminho da câmera com chaves e easing, travelling e dolly, e a câmera 3D ligada à câmera do
   corte (o multiplano 2D já existe).
6. **Mão e rosto**: dedos (o VRM tem) e expressões básicas para referência.

## Composição (pedido do dono: nível After Effects, partículas 2D e 3D)

**Já existe**

- Pilha de efeitos de satsuei por camada (desfoques, glow, difusão, halation, luz transmitida, light wrap, rim light,
  PARA, aberração cromática, vinheta, grão, lens flare, god rays, sombra, contorno, overlay, duas tonalidades,
  ajuste, grade…), camada de efeito que vale para o que está embaixo, efeito em 2 ou 3 quadros (posterize), chaves de
  intensidade (`aiai_effects`), grade de cor por corte (`aiai_grade`).
- Câmera 2D do corte com pan, zoom, rotação, tremor e easing, e **multiplano** pela profundidade da camada
  (`aiai_camera`, `aiai_layer_motion`).
- **Linha do tempo da composição** com chaves e easing (clique, Shift, caixa de seleção, arrasto, interpolação pelo
  botão direito), **modos de mistura**, **matte** (recortar pelo alfa da camada de baixo, ou outra camada como máscara),
  **cronômetro por propriedade** (também nas propriedades dos efeitos) e um **editor de gráfico** de valores com alças
  (`panels/comp_timeline.rs`, `effect_controls.rs`, com o editor de curvas do ajuste de cor).
- **Partículas 2D** determinísticas por camada (pétalas, brilhos, neve, chuva, poeira, folhas, com ajustes)
  (`aiai_particles`, `aiai-anim/particles.rs`).

**Falta (a confirmar na rodada)**

1. No editor de gráfico, o **gráfico de velocidade** (o do After Effects para acertar easing) e várias propriedades ao
   mesmo tempo; a conferir na rodada.
2. **Pré-composição** (um corte ou um grupo de camadas usado como camada, com o próprio tempo), **máscaras desenhadas**
   com bezier, feather e expansão animáveis, e matte por **luma** e invertido (o matte de hoje usa outra camada inteira).
3. **Camadas 3D e câmera 3D** na composição (profundidade de verdade, não só o fator do multiplano), luz simples.
4. **Partículas 3D** (emissor no espaço com a câmera: profundidade, desfoque pela distância, sombra) e emissor
   seguindo uma camada ou um caminho; forças (vento, turbulência), colisão com o chão.
5. **Rastreamento de movimento** (ponto e plano) para grudar efeito em algo que se move.
6. Expressões simples (ligar uma propriedade a outra: wiggle, loop).

**Ordem proposta para a fase 3:** primeiro o que destrava a produção dos cortes: (Modelagem) 1, 3 e 4; (Composição) 2
e 4. Depois câmera 3D e partículas 3D, VRM, rastreamento e expressões. Cada item com o Codex na rodada do workspace.
