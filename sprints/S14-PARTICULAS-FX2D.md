# S14 — Partículas e efeitos 2D de anime

## Objetivo
Partículas 2D com sprites desenhados no app e os efeitos típicos de anime (linhas de velocidade/foco, impacto, brilhos, onomatopeia, fundos de gag), animados e com "cara de desenhado" (posterização temporal, formas chapadas).

## Pesquisa obrigatória
Trapcode Particular (emissores, física, vida), efeitos de anime (sakuga FX: pétalas, kira-kira, impacto, linhas de foco), CSP régua concêntrica e de linhas paralelas, "FX desenhado" (ruído binarizado + posterização temporal, citado na pesquisa A3).

## Tarefas
- [ ] Sistema de partículas: emissores (ponto, linha, retângulo, círculo, borda de máscara, ao longo de curva), taxa/rajada, vida, velocidade inicial com variação, forças (gravidade, vento, turbulência por ruído, atrator, arrasto), rotação e giro 3D falso (achatamento para pétalas), tamanho/cor/opacidade por idade (curvas), colisão simples com chão; profundidade (escala por Z + desfoque de profundidade); semente fixa (determinístico); pré-simulação ("começar cheio").
- [ ] Sprites: cels desenhados no app (vetor/raster), com variações escolhidas por partícula e animação de sprite.
- [ ] Presets: pétalas de rosa caindo, pétalas ao vento, brilhos (kira-kira) com cintilação, neve, chuva (riscos), poeira de luz, faíscas, bolhas, folhas.
- [ ] Linhas de velocidade e de foco animadas (geradas pela régua da S09: densidade, espessura, afinamento, cor, aleatoriedade por quadro em 2s).
- [ ] Impact frame (inversão/alto contraste/duas cores com máscara), flash branco, tremor de tela (usa shake da S13).
- [ ] Tipografia/onomatopeia: texto com fonte (ou letras desenhadas como vetor), contorno duplo, deformação, animação de escala/tremor (ex.: "ゴゴゴ").
- [ ] Fundos de gag/chibi: padrões (listras radiais, bolinhas, tons de mangá/screentone), cor chapada, faixas.
- [ ] Posterização temporal e "boil" (redesenho com jitter de linha a cada 2–3 quadros) como opção de efeito.
- [ ] MCP: `aiai_particles` (preset ou parâmetros, sprites, emissor, semente), `aiai_speed_lines`, `aiai_impact`, `aiai_onomatopoeia`, `aiai_gag_bg`.

## Testes
- Golden de simulação determinística (quadros 0, 12, 48 com semente fixa).
- Desempenho: 2000 partículas com sprite em 1080p a ≥ 24 fps de prévia.
- Mouse: posicionar emissor arrastando, ajustar vento por slider, ver prévia.

## Critério de pronto
- [ ] Prova de uso: corte de 2 s com pétalas de rosa (sprites desenhados pelo agente) caindo sobre um close do personagem com difusão, e um corte de gag com linhas de foco, impacto e onomatopeia; MP4 em evidências.
- [ ] Linha de base contra os cortes candidatos C/D do episódio (pétalas, "ゴ", chibi).
