# S09 — Réguas, grades espaciais e angulares, perspectiva, guias 3D

## Objetivo
Dar referência espacial para cenários complexos e ângulos de personagem: réguas com snap (perspectiva, simetria, concêntrica, paralela, curva, elipse), grades (quadrada, isométrica, de proporção, de perspectiva sobre planos), transferidor/snap de ângulo, quadro de câmera e campo, e **guias 3D** (manequim posável, blocos de cenário, câmera com lente) renderizadas como linhas para desenhar por cima.

## Pesquisa obrigatória
CSP réguas (perspectiva 1/2/3 pontos, olho de peixe, simetria, concêntrica, linha paralela, curva, elipse em perspectiva) e modelos 3D; Photoshop Vanishing Point; uso de 3D→layout em estúdios (OLM, ufotable); código do ANIGO (`anigo-renderer`, `anigo-charbase` manequim/base de personagem, `anigo-ik` para pose, `anigo-modeling` primitivas).

## Tarefas
- [ ] Régua de perspectiva: 1, 2 e 3 pontos; horizonte; pontos de fuga arrastáveis (inclusive fora do canvas); snap do traço vetorial e do pincel raster à direção mais próxima; desligar por traço.
- [ ] Grade de perspectiva em planos (piso, paredes, teto) com espaçamento e origem; grade sobre plano definido por 4 pontos.
- [ ] Grades 2D: quadrada, isométrica, terços, áurea, grade de cabeças para model sheet.
- [ ] Réguas: simetria (linear, radial com N eixos), concêntrica (linhas de foco), paralela (linhas de velocidade), curva, elipse em perspectiva, régua de linha.
- [ ] Transferidor: snap de ângulo configurável (15/30/45/90°), medidor de ângulo, régua de distância.
- [ ] Quadro de câmera, área segura de título/ação, campo (field chart 12F/16F) e marcação de pan (quadro inicial/final).
- [ ] `aiai-guide3d`: cena 3D por corte com primitivas (caixa, cilindro, esfera, plano, escada, arco), manequim humano posável (copiar/adaptar do ANIGO: esqueleto, IK, proporções ajustáveis anime — altura em cabeças, cabeça grande para chibi), câmera (posição, alvo, FOV/lente em mm, altura do olho, rolagem).
- [ ] Render da guia 3D como linhas de contorno + vincos (shader de linha do ANIGO) e como sombreado plano, numa camada de guia; a régua de perspectiva 2D pode ser **gerada a partir da câmera 3D** (pontos de fuga casados).
- [ ] MCP: `aiai_ruler` (criar/editar réguas e grades), `aiai_guide3d_scene` (primitivas, transformações), `aiai_mannequin_pose` (ângulos por osso ou pose nomeada: em pé, sentado, andando, apontando, cabeça 3/4, perfil…), `aiai_camera3d`, `aiai_guide3d_render`.

## Testes
- Golden: traços com snap nas três perspectivas; grade de piso; elipse em perspectiva; manequim em 5 vistas de turnaround.
- Consistência: pontos de fuga gerados pela câmera 3D batem com a projeção dos blocos (erro < 0,5 px).
- Mouse: arrastar ponto de fuga, desenhar com snap ligado e desligado, girar osso do manequim.

## Critério de pronto
- [ ] Prova de uso: o agente monta em blocos o interior de uma sala (piso, paredes, janela, mesa, cadeira), escolhe câmera em dois pontos, gera réguas e desenha por MCP o layout em linha por cima; render das etapas em evidências.
- [ ] Manequim posado em 5 vistas para servir de guia ao turnaround da S10.
