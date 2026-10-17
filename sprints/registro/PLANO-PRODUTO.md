# Plano de produto (a partir de 2026-10-16)

Orientação do usuário: parar a calibração pixel a pixel, régua perceptual (decisão 25), fechar o corte D e voltar o foco
para o produto: **funções no MCP, interface e o micro anime original**.

## Diagnóstico (levantamento de cobertura do MCP, 2026-10-16)

- 47 ferramentas MCP cobriam todas as ações da bridge (menos PING/GET_METRICS), mas módulos inteiros do motor nunca
  foram ligados à bridge: **todo `preprod.rs` (S16)** e **todo `production.rs` (S19)** — só os testes os usavam.
- Câmera, chaves de camada, multiplano, os 25 efeitos de satsuei, ajustes e grade só são alcançáveis por `aiai_apply`
  com JSON escrito à mão; `params` é JSON sem tipo, então o agente nem descobre o formato dos efeitos.
- Não há como trazer arquivo externo (PNG/WAV/.cube) para o blob store: LUT inutilizável.
- `aiai_render` não faz faixa de quadros nem folha de contato; não há prévia de um quadro da edição (legenda/transição)
  sem exportar tudo; o `.srt` só existe como temporário de queima.

## Fatias, em ordem

| # | Fatia | Conteúdo | Pronto quando |
|---|---|---|---|
| P1 | MCP S16 | `aiai_script`, `aiai_beats`, `aiai_storyboard` (painéis, desenho do painel, folha de e-conte), `aiai_animatic`, `aiai_cuts_from_storyboard` | teste da bridge da cadeia inteira + fumaça headless olhando a folha e o corte gerado — **feito 2026-10-16** |
| P2 | MCP S19 | tabela de cortes (カット表) com miniaturas, etapas + sugestão automática + lint de todos os quadros expostos, correções, retakes (comparar/folha de diferença), relatório de produção | teste da bridge (lint do corte, etapas, correção, retake com folha) — **feito 2026-10-16** |
| P3 | MCP satsuei/câmera/cor | ferramentas curadas com schema tipado: efeitos por camada/camada de efeito (`aiai_effects`), câmera do corte com chaves e tremor (`aiai_camera`), chaves/profundidade de camada (multiplano), ajustes e grade (`aiai_grade`) | o agente monta um corte com pan multiplano + glow/difusão + grade só pelas ferramentas, conferido no render — **feito 2026-10-16** (`aiai_effects`, `aiai_grade`, `aiai_camera`, `aiai_layer_motion`; evidencias/S13/mcp_multiplano_q00/q71) |
| P4 | Prévia e entrega | `aiai_render` com faixa/folha de contato, prévia de quadro da edição, `.srt` ao lado do MP4, `tts_regenerate`, mixagem da edição em WAV | uma conferência de legenda sem exportar o vídeo |
| P5 | Micro anime (S22) | produzir pelo MCP, do roteiro ao MP4, o curta original de 30–60 s com os critérios da seção 7 do protocolo; cada falta de ferramenta vira lacuna e é corrigida na hora | MP4 final + checagem dos critérios |
| P6 | Interface (S20) | painéis de pré-produção (roteiro/storyboard/animatic), tabela de produção, timeline de edição e satsuei na janela do app, PT/EN, claro/escuro; validação visual com o mouse real | cada fluxo do micro anime possível também pela UI |

Importação de arquivo (PNG/WAV/.cube) fica para depois de P5: o curta não usa imagem externa (regra), e um LUT pode ser
gerado dentro do app pela grade.

## Regras que continuam

- Nenhuma IA generativa de imagem/vídeo; áudio só local e gratuito (VOICEVOX/Kokoro), vozes originais.
- Nada do episódio de referência entra no git nem no produto.
- Commits só por pathspec; build sem `--workspace`, `-j 2`.
