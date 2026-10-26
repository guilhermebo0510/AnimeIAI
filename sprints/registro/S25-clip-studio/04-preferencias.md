# S25 — Preferências: Clip Studio × AnimeIAI

Fonte: as 17 categorias de Preferências do Clip Studio Paint (capturas `calibracao/csp/pref_00.png` … `pref_16.png`, fora do git).
Regra do lote: o que serve a um estúdio de anime e tem comportamento de verdade por trás entra; nenhum botão que não faz nada.
Quadrinhos (bordas de quadro, texto de balão, importação de ComicStudio), Tabmate, smartphone e toque com dedos não se aplicam.

Antes deste lote as nossas Preferências eram 4 abas (Geral, Atalhos, Pastas, Desempenho). Agora são 9 categorias numa lista à
esquerda: **Geral, Ferramentas, Tablet, Cursor, Camada e seleção, Tela de pintura, Atalhos, Arquivo e pastas, Desempenho**.

Legenda: **tem** = já existia · **novo** = entrou neste lote, com consumidor real · **falta** = serve, ficou para depois ·
**n/a** = não se aplica a anime ou ao nosso motor.

## 1. Ferramentas (`pref_00`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Alternar ferramenta temporariamente ao segurar a tecla + tempo (ms) | Ferramentas > "Alternar temporariamente…" + ms (100–2000, padrão 500). `app/tool_hold.rs` arma a troca depois dos atalhos de ferramenta; solta depois do tempo volta à anterior, toque curto troca de vez | **novo** |
| Distância mínima de arrasto (3) | Ferramentas > "Distância mínima de arrasto" (px da tela, 0–20, padrão 3). `canvas.rs::on_move` segura mover/escalar/girar a seleção, área de seleção, forma, pontos, alças e réguas até o ponteiro andar a distância | **novo** |
| Tamanho da alça | alças do Mover têm 9 px fixos | n/a (sem demanda) |
| Rolagem automática na borda da tela de pintura | — | falta |
| Visualização rápida enquanto navega | — (o render é na GPU, sem modo reduzido para navegar) | n/a |
| Começar do tamanho atual ao mudar o tamanho arrastando | Ctrl+Alt+arrastar sempre parte do tamanho atual | **novo** (comportamento fixo) |
| Exibir ajuste do tamanho do pincel na tela | Ferramentas > "Mostrar o círculo e o tamanho…" (círculo do tamanho novo + valor) | **novo** |
| (Clip Studio: Ctrl+Alt+arrastar muda o tamanho) | Ferramentas > "Mudar o tamanho da ponta com Ctrl+Alt…": pincel, caneta e borracha | **novo** |
| Reduzir pressão da caneta com dedo/mouse no início e fim | a ponta fina de início/fim é propriedade do pincel (afilar), não preferência | n/a |
| Pré-visualizar linhas com Shift | a caneta em formas já encaixa em 15° com Shift; a prévia de linha livre não existe | falta |
| Visualização de pincelada (reduzir atraso) | o traço ao vivo é da GPU | n/a |

## 2. Tablet (`pref_01`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Serviço de tablet: Wintab ou Tablet PC | Tablet: "Windows Ink: a pressão da caneta é lida pelo app" + estado da caneta detectada (`pen_seen`). Sem Wintab nem driver próprio (a pressão chega pelo sistema, via winit) | **novo** (texto honesto) |
| Modo mouse do driver / área de operação / frequência de entrada | configurações do driver da mesa, não do app | n/a |
| (curva de pressão) | Tablet: mínimo + curva (gama) com o desenho da curva — já existia, saiu da aba Geral para cá | **tem** |
| — | Tablet > "Usar a pressão da caneta": desligada, a caneta desenha como o mouse (pressão cheia) | **novo** |
| — | Tablet > "Teste de pressão": área de rabisco que mostra a força lida e o resultado da curva em tempo real (para a fase em que a mesa for ligada) | **novo** |

## 3. Gestos de toque (`pref_02`)

Toque de um/dois/três dedos (mover, pinça, girar, desfazer, refazer): o app é para caneta e mouse; o toque com dedos fica para
quando houver tela de toque. **falta / n/a agora.**

## 4. Comando (`pref_03`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Transformar: alvo e confirmar com duplo clique | o Mover confirma com Enter; alvo é a seleção/camada | n/a |
| Colar SVG como vetor | a colagem de SVG não existe | falta |

## 5. Interface (`pref_04`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Cor de interface (escuro/claro) | Geral > Tema | **tem** |
| Ajustar densidade | Geral > "Densidade da interface": Compacta / Normal / Espaçosa (espaço entre controles e enchimento dos botões, `theme::apply_density`; sobrevive à troca de tema) | **novo** |
| Zoom da interface | Geral > Zoom da interface | **tem** |
| IME | o egui cuida do IME | n/a |
| Interface para toque / layout tablet | — | n/a |

## 6. Desempenho (`pref_05`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Memória alocada (%) | Desempenho > Memória de vídeo (orçamento em MB, S24) | **tem** |
| Unidade de processamento (GPU) | Desempenho > Placa de vídeo | **tem** |
| Desfazer: níveis (200) | Desempenho > "Níveis de desfazer" (10–2000, padrão 500) → `CoreSession::undo_limit`, aplicado a cada quadro; baixar o número descarta as ações antigas na hora (`session.rs`, agora `while`) | **novo** |
| Desfazer: intervalo entre ações (ms) | — | n/a |

## 7. Cursor (`pref_06`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Cursor tipo pincel / caneta / borracha: forma do pincel, mira, ponto | Cursor > Pincel, Caneta, Borracha: Forma da ponta / Mira / Ponto (`canvas.rs::cursor_look_with`) | **novo** |
| Cursor tipo figura, preenchimento, visualização, correção de linha: específico da ferramenta | Cursor > "Balde, conta-gotas e seleção": Específico da ferramenta / Mira | **novo** (um só controle para as ferramentas sem ponta) |
| Exibição adicional quando o cursor é pequeno: nenhuma, ponto, linha | Cursor > "Quando a ponta é pequena demais": Nenhum / Ponto / Mira (limite: 7 px na tela) | **novo** |
| Posição do cursor invertido: com ou sem atraso para estabilização | — | falta |
| (Mira precisa, Caps Lock) | Cursor > "Mira precisa sempre" | **tem** |

## 8. Camada e quadro (`pref_07`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Nome das cópias (`* Cópia`) | Camada e seleção > "Nome das cópias": `Nome (cópia)` / `Nome cópia` / `Cópia de Nome` / `Nome 2`; vale para Copiar para camada (seleção) e para duplicar material de personagem (`behavior_prefs::copy_name_tr`) | **novo** |
| Opacidade/cor de camadas de seleção e máscara | Camada e seleção > cor das formigas da seleção (com prévia); não há máscara de cor sobreposta | **novo** (só a seleção) |
| Modo atravessar para pastas novas | — | n/a (sem esse modo de mistura de pasta) |
| Espaços da borda de quadro | — | n/a (quadrinhos) |

## 9. Animação (`pref_08`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Mesa de luz: opacidade, cor da camada (anterior/seguinte), subcor | painel Mesa de luz: opacidade, antes/depois, cores anterior e seguinte, voltar às cores padrão | **tem** |
| Como colar nas células; adicionar/remover célula com a camada; estender clipe; vincular operações | a linha do tempo de cels é nossa (exposições do X-sheet); sem equivalente em preferências | n/a |

## 10. Régua e unidade (`pref_09`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Cores das réguas, da linha de grade, margem de segurança | Tela de pintura > cor e opacidade da grade de pixels; as réguas usam a cor de destaque do tema | **novo** (grade) · falta (réguas) |
| Opacidade da régua/grade | Tela de pintura > "Opacidade" da grade | **novo** |
| Unidade de comprimento (px/mm/cm) e de texto (pt) | — o projeto não tem resolução em dpi; a animação trabalha em pixels do quadro | n/a por ora |

## 11. Tela de pintura (`pref_10`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Transparente: cor 1 e cor 2 | Tela de pintura > "Cores próprias do xadrez" (duas cores; sem marcar, as do tema) | **novo** |
| Passo do ângulo (5°) | Tela de pintura > "Passo dos botões de girar" (1–90°, padrão 15°) → botões da barra do canvas e do Navegador | **novo** |
| Lista de zoom / limitar escala de exibição | — | falta |
| Qualidade de exibição, resolução de exibição | — | n/a |

## 12. Arquivo (`pref_11`)

| Clip Studio | AnimeIAI | Situação |
|---|---|---|
| Recuperação automática a cada N min | Arquivo e pastas > recuperação do projeto sem pasta (padrão 2 min) e cópia do documento de projeto com pasta (padrão 5 min), `autosave.rs::set_intervals` | **novo** |
| Salvar em segundo plano / usar cache de camadas ao exportar | a cópia automática já roda em outra thread | **tem** |
| Pasta de projetos | Arquivo e pastas > Pasta de projetos | **tem** |
| Mostrar status de salvamento, importações ComicStudio/IllustStudio, gerenciamento de páginas | — | n/a |

## 13–16. Demais categorias

| Clip Studio | Situação |
|---|---|
| Conversão de cores (perfis RGB/CMYK, intenção) | n/a — o pipeline é sRGB; o perfil de saída é escolha da Entrega |
| Editor de história (texto de balão, quebras de linha) | n/a — a ferramenta Texto tem os próprios ajustes |
| 3D (figura padrão, renderização, camadas 3D, OBJ) | n/a nas Preferências — a camada 3D tem painel próprio |
| Conexão com smartphone | n/a |
| Tabmate 2 | n/a |

## Como cada ajuste novo está ligado

Tudo vive em `Prefs.behavior` (`behavior_prefs.rs`, `serde(default)`: um `prefs.json` antigo abre com os padrões). O app copia a
seção para o editor a cada quadro (`StudioApp::sync_behavior`: `Editor.beh`, memória do egui para quem só tem o `Context`,
`undo_limit`, intervalos de cópia e densidade).

| Ajuste | Onde age |
|---|---|
| `tool_hold`, `tool_hold_ms` | `app/tool_hold.rs`, chamado de `StudioApp::editor_shortcuts` depois dos atalhos de ferramenta; decisão pura em `behavior_prefs::hold_step` |
| `min_drag_px` | `canvas.rs::on_press` guarda a origem; `on_move` segura os arrastos de objeto até `behavior_prefs::drag_armed` |
| `size_drag`, `size_drag_preview` | `canvas.rs`: Ctrl+Alt+botão primário abre o arrasto (`on_press`), `on_move` muda `pen_width`/`brush_size`/`eraser_size` por `size_from_drag`, o círculo é desenhado em `paint_canvas` |
| `pen_pressure` | `canvas.rs::curve`: desligada, toda pressão vira 1,0 |
| `cursor_brush/pen/eraser`, `cursor_other`, `cursor_small` | `canvas.rs::cursor_look_with` e o desenho do cursor |
| `copy_name` | `sel_ops.rs` (Copiar para camada) e `panels/char_folder.rs` (duplicar material) |
| `sel_color` | `selection.rs::ants` |
| `checker` | `canvas.rs::update_render` refaz a textura quando muda |
| `grid_color`, `grid_alpha` | `canvas.rs::paint_grid` |
| `rotate_step_deg` | botões de girar da barra do canvas e do Navegador |
| `undo_levels` | `CoreSession::undo_limit` (e o corte do excesso virou `while` em `aiai-core/src/session.rs`) |
| `ui_density` | `theme::apply_density` |
| `recovery_minutes`, `backup_minutes` | `autosave.rs::set_intervals` |

## Ficou de fora e por quê

- Rolagem automática na borda, lista de zoom, prévia de linha com Shift, cor das réguas, atraso de estabilização do cursor
  invertido: úteis, mas pedem mudanças no motor de gestos/réguas que outros agentes estão tocando; ficam para o próximo lote.
- Unidades mm/cm: o projeto de anime não tem dpi; sem isso a conversão seria um número sem sentido.
- Gestos de toque com dedos, Wintab, Tabmate, smartphone, IME, conversão de cores, histórias em quadrinhos: não se aplicam.
- Nome das cópias do pincel duplicado (`panels/brushes.rs`) segue "Nome 2": o arquivo está em edição por outro agente.
