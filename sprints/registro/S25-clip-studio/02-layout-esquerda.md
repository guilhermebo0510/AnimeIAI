# S25 — Lado esquerdo das áreas de desenho (à moda do Clip Studio)

Pedido do usuário: "o Clip Studio tem mais ícones e menos texto; o nosso tem muito texto e poucos ícones". Não é cópia:
é a organização do Clip Studio (prateleira por grupos, grupo de ferramentas, propriedades, tamanho do pincel, cores
embaixo) com a nossa identidade (tema, ícones próprios desenhados na grade de 24 px). Commits: `a1549db` e o seguinte
(ajuste de alturas e do rodapé).

Referências do Clip Studio usadas (fora do git): `calibracao/csp/22_gpen_full.png`, `37_prateleira.png`, `_list_c.png`,
`33_detalhe_subferramenta.png`, `34_abas_cor.png`.

## Antes

`calibracao/csp/12_nosso_pen.png` (e `esquerda/a01_pintura_bridge.png`, primeira versão): prateleira com 8 ícones sem
grupos; painel "Ferramenta" com busca, "Todos (15)", "Lápis e canetas (1)", "Salvar como pincel novo…", "Criar pincel…",
"Importar…"; Cores com "Cor direta", "Adicionar à paleta", "Nova paleta", "Salvar na biblioteca" e um seletor quadrado.

## Depois (capturas com mouse e teclado reais, `input.ps1 -ProcessId`, instância de teste `ui_copy_left.aiai`)

Pasta: `sprints/registro/S25-clip-studio/esquerda/`.

| Captura | O que mostra |
|---|---|
| `f01_pintura.png`, `f01_esquerda.png` | Pintura de fábrica: prateleira só de ícones; Grupo de ferramentas (aba Pincel, lista com o traço), Propriedades da ferramenta (prévia + Tamanho, Opacidade, Suavização em 4 ícones), Tamanho do pincel (bolinhas 0,7…200), Cores embaixo com as abas em ícones |
| `g_seq.png` (`g01`…`g04`) | clique no Aerógrafo (o pincel dele com 120 px/80%); tecla B volta ao Pincel com o Redondo duro e 24 px lembrados; clique na bolinha 10 → 10 px destacado; ícone de pressão abre a curva (pressão, mínimo 20%, velocidade, aleatório) |
| `g04_pressao_zoom.png` | o popup da dinâmica de perto |
| `h01_avancadas.png`, `h_seq.png` | chave inglesa → Configurações avançadas em 13 categorias; olho do Intervalo aceso em Traçado → a linha Intervalo aparece no painel; Início e fim (entrada, saída, na ponta, tamanho/opacidade/pela velocidade) |
| `i_cores.png` | abas Conjunto (paletas e degradês), RGB/HSV, Histórico, Aproximada, Intermediária; amostras principal/secundária/transparente em todas |
| `i09_crop.png` | Balde escolhido: o Tamanho do pincel fica cinza e não responde (o layout não pula); dica do Degradê "em breve" |
| `j_seq.png` | clique na secundária (branca passa a ser a cor em uso), traço no canvas, a cor entra no Histórico |
| `k_seq.png` | Animação: P escolhe a Caneta (abas Caneta / Marcador, penas vetoriais com o selo de vetor), P de novo vai para o Lápis (pena vetorial e Lápis de rascunho raster) |

Comparando lado a lado com `22_gpen_full.png`: mesma ordem vertical (grupo, propriedades, tamanho, cor), prateleira
estreita só de ícones, propriedades em linhas curtas com o ícone de pressão à direita, chave inglesa e "redefinir" no
rodapé, tamanhos em bolinhas com o número embaixo, cores com abas em ícones. Diferenças nossas: a prévia do traço é
desenhada pelo nosso motor, os ícones são nossos, as penas vetoriais aparecem na Caneta com o selo de vetor.

## Atalhos novos (mapa padrão; Preferências > Atalhos continua mudando)

M Seleção, W Seleção automática, K Mover camada (passa pelo objeto 3D), H Mão e zoom (Z segue sendo o zoom), I Conta-gotas,
P Caneta → Lápis, B Pincel → Aerógrafo → Decoração, E Borracha, J Mistura, G Balde (→ Degradê quando existir),
U Figura → Régua, T Texto, Y Corrigir linha. Mudaram de lugar: papel do cel K → Shift+K, objeto 3D M → Shift+M. Na
Composição o Mover continua no V (o K lá anda nas chaves, como no After Effects).

## O que ficou de fora e por quê

- **Degradê por arrasto**: não existe no motor; o ícone aparece desabilitado com "em breve" (o balde já preenche com o
  degradê das Cores).
- **Decoração, Mistura e Corrigir linha**: existem como grupos com o que há (pincéis de cenário, Borrador, Editar
  pontos); nada foi fingido.
- **Área de mistura livre** (aba do Clip Studio onde se pinta para misturar cores): fica para depois.
- **Suavização da caneta vetorial**: o vetor só tem com e sem; os níveis médio e forte aparecem desabilitados com a dica.
- **Estabilização**: o pincel com estabilização própria (0–100) mostra a barra; sem ela, a linha mostra a do painel
  (média móvel/corda) que vale para todos — as duas estão no motor.
- **Coluna esquerda na altura toda com a linha do tempo só embaixo do canvas**: na Animação e na Pintura as colunas dos
  lados vão na altura toda; a primeira tentativa (linha do tempo embaixo do canvas e da direita) quebrava o
  "soltar e redocar" da linha do tempo (`dock_ui::detach_and_redock_restores_the_layout`) e foi trocada.
- Em 1080p com a barra de tarefas os painéis da esquerda ainda são baixos (a Propriedades mostra 4 linhas, o Tamanho duas
  fileiras); todos redimensionam e rolam.

## Testes

Novos: `app::shelf::tests` (Seleção primeiro, prateleiras por workspace, ciclo da letra, atalhos de fábrica, prateleira
acompanha a ferramenta do motor, grupos dos pincéis, sub-ferramentas e textos), `drawing_prefs::tests` (olhos com
padrão e ida e volta em JSON, lista de tamanhos, histórico), `tool_detail::tests`, `brush_size::tests` (bolinhas, tamanho
destacado), `colors::tests` (grades aproximada e intermediária). `cargo test -p aiai-studio`: 327 da lib e a suíte de
integração passando.
