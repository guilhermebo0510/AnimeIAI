# S13 — Câmera e composição (referência: After Effects + satsuei japonês)

## Objetivo
O satsuei: câmera 2D animada com multiplano, editor de curvas, pré-composição, máscaras/track mattes e a biblioteca de efeitos de composição de anime, todos em GPU (WGSL) com espelho CPU para referência.

## Pesquisa obrigatória
`anime-studio-pesquisa/pesquisa-bruta/A3` §2.4 (29 efeitos com algoritmo e custo), documentação do After Effects (camadas 3D, câmera, track matte, graph editor), plugins japoneses (PSOFT CelMX/CelFX: blur consciente de cor), técnicas de satsuei (difusão/"ディフュージョン", 透過光, パラ, light wrap, rim light), glow por pirâmide (dual Kawase), bokeh.

## Tarefas
- [ ] Câmera: posição, zoom (T.U./T.B.), rotação, pan, com keyframes e easing; caminho de câmera visível no canvas; shake procedural com semente (amplitude, frequência, decaimento).
- [ ] Multiplano: profundidade por camada (Z) → parallax automático em pan/zoom; prévia lateral da pilha de planos.
- [ ] Graph editor: curvas de valor e de velocidade, tangentes Bézier, presets de easing (ease in/out, overshoot), copiar curvas.
- [ ] Pré-composição (grupo renderizado como uma camada, com efeitos próprios), track matte (alfa/luma, invertido), máscaras animadas (forma vetorial com keyframes, suavização).
- [ ] Efeitos (cada um nó com parâmetros animáveis, GPU+CPU, golden):
  - blur gaussiano separável, direcional, radial/zoom, **blur consciente de cor** (não mistura regiões de cel distintas);
  - glow/bloom (limiar, pirâmide, cor), difusão (soft focus de anime: blur + tela/luz suave com limiar), halation;
  - 透過光 (luz transmitida: forma + glow forte + cor), light wrap, rim light 2D (borda pela máscara + direção);
  - sombra/gradiente de luz sobre personagem (パラ: degradê em multiplicar/tela recortado pelo personagem);
  - aberração cromática, vinheta, grão de filme (semente por quadro), deband (dither);
  - lens flare simples e god rays (P1), motion blur por velocidade (P1).
- [ ] Posterize time por efeito (o efeito "anda" em 2s/3s como o desenho).
- [ ] Ordem e cache de render por nó; invalidação por revisão.
- [ ] MCP: `aiai_camera_key`, `aiai_layer_depth`, `aiai_add_effect` (tipo, parâmetros, camada/grupo), `aiai_effect_key`, `aiai_matte`, `aiai_precomp`.

## Testes
- Golden por efeito (estático e com um keyframe), GPU×CPU FLIP ≤ 0,02.
- Desempenho: corte típico (BG, 3 camadas de personagem, BOOK, difusão, glow, grão) a ≥ 12 fps de prévia 1080p na GTX 1650; render final sem limite de tempo real.
- Mouse: animar câmera arrastando o quadro no canvas com keyframes; ajustar tangente no graph editor.

## Critério de pronto
- [ ] Prova de uso: corte de 3 s com o fundo da S11 e o personagem da S12: pan com multiplano (BG lento, personagem, BOOK rápido), difusão, luz de janela (透過光), パラ no personagem, grão; render MP4 em evidências.
- [ ] Linha de base de métricas de um corte do episódio com câmera em pan (erro de trajetória medido pela S04).
