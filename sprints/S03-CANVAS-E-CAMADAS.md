# S03 — Motor de canvas e camadas, compositor de referência, golden tests

## Objetivo
O canvas onde tudo é desenhado: viewport wgpu dentro do egui, navegação, árvore de camadas com mistura/máscara/clipping, compositor GPU e compositor CPU de referência idênticos em semântica, export PNG e infraestrutura de golden tests. Decidir aqui o rasterizador vetorial (S05 usa).

## Tarefas
- [ ] Viewport do canvas: textura wgpu desenhada como imagem egui (sem janela filha); pan (espaço/botão do meio), zoom (roda, ancorado no cursor), rotação da visão (R), espelhar visão, ajustar à janela, 100%, régua de pixels; xadrez de transparência; quadro de câmera (1920×1080) com área segura.
- [ ] `aiai-paint`: estrutura de tiles 256×256 esparsos (CPU f32 e GPU RGBA16F), upload incremental de tiles sujos.
- [ ] `aiai-render`: grafo de composição a partir da árvore de camadas: visibilidade, opacidade, modos de mistura (normal, multiplicar, tela, sobrepor, luz suave, luz forte, adicionar, subtrair, escurecer, clarear, diferença, cor, matiz, saturação, luminosidade), máscara de camada, clipping, pastas com mistura "passar"/isolada. Fórmulas iguais em WGSL e em Rust (uma tabela de referência; testes cruzados).
- [ ] Compositor CPU determinístico (multithread por tile com ordem fixa) e compositor GPU; comparação FLIP GPU×CPU nos testes.
- [ ] Espaço de cor: trabalho linear, entrada/saída sRGB; conversões exatas e testadas.
- [ ] Spike do rasterizador vetorial: comparar (a) tesselação própria + MSAA/cobertura analítica, (b) `vello` GPU + `vello_cpu`, em qualidade de borda (com e sem AA), velocidade (1000 traços), determinismo e dependências. Registrar decisão em `registro/DECISOES.md`.
- [ ] Orçamento de memória (8 GB de RAM, 4 GB de VRAM): cada camada 1080p em RGBA16F custa ~16 MB; um corte com 100+ camadas não cabe inteiro na VRAM. Cache de tiles com limite configurável e despejo LRU (GPU → RAM → disco), camadas ocultas não residentes, pastas achatadas em cache quando não estão sendo editadas. Medir e registrar o pico.
- [ ] Export PNG (8/16 bits) de quadro/camada; import PNG/JPEG como camada raster ou de referência.
- [ ] Golden tests: `app/tests/golden/` com cenas JSON → render CPU → hash + PNG; atualização só com `--update-golden` e revisão visual.
- [ ] Bridge: ação RENDER completa (CPU ou GPU, quadro, camadas, tamanho). MCP `aiai_render`.
- [ ] UI: painel de Camadas (árvore, arrastar para reordenar, olho, cadeado, opacidade, modo, miniatura), painel de Navegador (miniatura do canvas).
- [ ] Comandos: todas as propriedades de camada, reordenar, agrupar, mesclar para baixo, duplicar.

## Testes
- Golden: cada modo de mistura com 3 casos; máscara; clipping; pasta isolada; 4K vazia (desempenho).
- GPU×CPU: FLIP médio ≤ 0,01 nas cenas golden.
- Mouse: pan, zoom e rotação no canvas; arrastar camada no painel; mudar opacidade por slider; conferir por render.
- Desempenho: 30 camadas 1080p a ≥ 30 fps na GTX 1650 (medido com algo animando).

## Critério de pronto
- [ ] Cena de teste com 10 camadas e todos os modos renderizada idêntica por MCP (CPU) e GPU dentro da tolerância; imagens em evidências.
- [ ] Decisão do rasterizador vetorial registrada com números.
- [ ] Roteiro de mouse executado com capturas.
