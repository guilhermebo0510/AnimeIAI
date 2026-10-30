# Fase 3, C2: máscaras desenhadas, matte por luma/alfa invertido e pré-composição (desenho, antes de codar)

> Escrito em 30/10 (~19:05) enquanto o Codex audita a rodada 4 do Copiloto (sem build). Base: `fase3-estado-e-lacunas.md`,
> Composição 2. Código lido: `LayerData` (`aiai-core/src/model.rs`: `clip_to_below`, `mask: Option<Id>`, `keys`),
> `compose.rs` (`mask_values` usa **luma × alfa** da camada-máscara, sempre), `comp_timeline.rs` (`matte_combo`).

## 1. Modo do matte (pequeno, primeiro)

Hoje a camada-máscara entra como luma × alfa, sem escolha. No After Effects são quatro: Alfa, Alfa invertido, Luma,
Luma invertido.

- `LayerData.mask_mode: MaskMode { Alpha, AlphaInverted, Luma, LumaInverted }`, padrão **Luma** (o comportamento de
  hoje, para os projetos que já existem não mudarem), `skip_serializing_if` padrão.
- `mask_values`: Alpha = `a`; Luma = o de hoje; invertidos = `1 − valor`.
- Linha do tempo: o combo de Matte ganha os quatro modos quando há camada-máscara (como as colunas TrkMat do AE).
- Teste: camada branca com alfa 0,5 como máscara → Alfa 0,5; Luma 0,5; Alfa invertido 0,5; Luma invertido 0,5; uma
  máscara preta opaca → Alfa 1, Luma 0.

## 2. Máscaras desenhadas na própria camada

- `LayerData.masks: Vec<ShapeMask>`; cada uma: `path` (pontos com alças, fechado, no espaço da camada), `mode`
  (Somar, Subtrair, Interseção, Diferença), `feather` (px), `expansion` (px, negativo encolhe), `opacity`,
  `inverted`, e chaves próprias (`path`, `feather`, `expansion`, `opacity`) no mesmo `KeyTracks` da camada
  (`mask.<i>.feather` etc.), para o cronômetro e o editor de gráfico que já existem.
- Render: cobertura do caminho rasterizada (o preenchimento de polígono antisserrilhado do `aiai-paint`), expansão por
  dilatar/erodir a distância (campo de distância, que o vetorizador já tem), feather por desfoque gaussiano separável
  (o do satsuei), combinadas na ordem da lista, multiplicando o alfa da camada antes do blend. A cobertura é cacheada
  por (camada, quadro, versão das máscaras).
- Chave de caminho: interpolação ponto a ponto (mesmo número de pontos; se mudar, a chave nova reamostra).
- Ferramenta: caneta de máscara no canvas da Composição (clique = canto, arrastar = alça, fechar no primeiro ponto),
  retângulo e elipse prontos; editar pontos com a ferramenta de pontos que já existe para vetores.
- MCP: `aiai_effects` (ou uma ferramenta `aiai_mask`) com `add/set/remove/key`.
- Testes: retângulo Somar → alfa 1 dentro, 0 fora; feather 10 → rampa monotônica de ~20 px na borda; expansão −5 →
  borda 5 px para dentro; Subtrair depois de Somar → buraco; chave de feather 0→20 em 10 quadros → valor no quadro 5
  pelo easing.

## 3. Pré-composição

- `LayerType::Precomp` com `params: { cut: Id, start: i32 (quadro do corte de origem no quadro 0 desta camada),
  speed: f32, loop: bool }`. Render: o quadro do corte de origem (`frame × speed + start`) renderizado pelo mesmo
  `Renderer` no tamanho do canvas do corte de origem, como uma imagem de camada, com transformação, matte, máscaras e
  efeitos normais por cima.
- Guarda contra ciclo (A dentro de B dentro de A): recusada no comando e no render (profundidade máxima 8).
- "Pré-compor": selecionar camadas → criar um corte novo só com elas (mesmo tamanho e duração), e uma camada Precomp no
  lugar. Desfazer volta tudo num passo.
- Duplo clique na camada Precomp abre o corte de origem.
- Testes: corte A com um quadrado animado; corte B com Precomp de A deslocado em +5 quadros → o quadro 10 de B mostra o
  quadro 15 de A; ciclo recusado; pré-compor duas camadas e desfazer.

## Ordem

1 (meia hora, sem risco), 2 (o grosso), 3. Cada parte com teste de unidade, bateria no app e a rodada WS-R08 do Codex
para a Composição.
