# Lições de implementação (leitura bloqueante antes de cada sprint)

Erros já pagos neste PC, nos projetos Smart Canvas e ANIGO do mesmo usuário. Acrescentar caso novo sempre que um erro custar mais de 15 minutos.

## Máquina (medido em 2026-10-15)

- i7-10750H (6 núcleos/12 threads), **7,9 GB de RAM**, GTX 1650 **4 GB** + Intel UHD, Windows 11, tela com escala 125%.
- Disco C: 476 GB, ~42 GB livres depois da limpeza do usuário. **Vive quase cheio.**
- Instalados: Rust 1.96, Node 24, Python 3.13 + uv, FFmpeg 9.0 (build "full" do gyan, GPL — só como processo externo), Git 2.43, gh, VS 2022 + Build Tools 2019, WebView2 155, winget e choco. Blender não instalado.

## Disco e compilação Rust

1. `target/` de um app egui+wgpu chega a 10–25 GB. `target/debug/incremental` sozinho já chegou a 16 GB. Usar `CARGO_INCREMENTAL=0` em builds de teste e de app (incremental + não incremental no mesmo `target/` corrompe libs: LNK2019 `anon.*.llvm.*`; conserto `cargo clean -p <crate>`).
2. **Ordem de limpeza** quando o disco apertar: (1) `target/debug/deps/*.pdb`; (2) duplicatas de `deps` (manter o mais novo por nome sem hash); (3) `target/debug/incremental`; (4) `cargo clean` só no fim.
3. **Cada arquivo em `tests/` é um binário que religa tudo.** Preferir um binário de integração por crate (`tests/suite.rs` com `mod`s). Rodar um `--test` por vez.
4. `LNK1318 ... LIMIT (12)` não é disco: é o `mspdbsrv`. Matar o processo e usar `-j 2`; perfil de teste com `debug = false`.
5. ICE do rustc ("internal compiler error", "can't find crate") neste PC costuma ser disco cheio: rodar `df -h /c` antes de suspeitar do código, e conferir tamanho dos fontes editados (disco cheio **trunca arquivo para 0 byte** antes do erro aparecer; já aconteceu).
6. Editar `[profile.*]` do workspace = recompilar tudo (equivale a `cargo clean`). Definir os perfis na S00 e não mexer depois sem 30 GB livres.
7. Perfil herdado do ANIGO: `[profile.dev] opt-level=1, debug="line-tables-only"` e `[profile.dev.package."*"] opt-level=3, debug=false` (sem isso a UI roda a 6 FPS).
8. `cargo test --workspace` sem `-j 2` estoura os 8 GB de RAM.
9. O `.exe` em uso trava o link (`Acesso negado, os error 5`): fechar o app antes de compilar. Matar a árvore inteira se houver vigia relançando.

## Bridge, MCP e testes do app vivo

10. **MCP dentro do processo do app cai a cada recompilação** (Smart Canvas). Solução adotada (ANIGO): servidor MCP **stdio separado** (`aiai-mcp`) que fala com a bridge do app por HTTP/NDJSON e relê o arquivo de descoberta a cada chamada. Além disso, um modo CLI (`aiai-mcp call <ação> '<json>'`) para usar pela Bash quando o MCP da sessão não estiver carregado ou depois de mudar a lista de ferramentas (lista nova só aparece com sessão nova).
11. Ferramenta MCP não adivinha efeito: o veredito vem do render/estado real lido de volta (round-trip), nunca do retorno "ok".
12. Screenshot de WebView não enxerga superfície nativa (Smart Canvas). No app egui, capturar pela própria bridge (`SCREENSHOT` lê o framebuffer) e pelo render offscreen.
13. Escrever num campo legado/genérico pode "dar certo" sem efeito nenhum (Smart Canvas: `position` legado). Sempre ler de volta o dado canônico.
14. Andaime de debug não escreve no projeto (cubos de debug viraram 322 nós persistidos).

## Mouse e teclado reais (SendInput)

15. PowerShell é DPI-unaware: chamar `SetProcessDPIAware()` antes; em 125% sem isso os cliques caem 25% fora.
16. `Marshal.SizeOf(INPUT)` tem de ser 40 em x64; struct errada faz `SendInput` devolver 0 sem erro. Checar retorno.
17. Montar a união da struct numa variável própria (`$u.mi = $mi; $ev.u = $u`); atribuição encadeada escreve numa cópia e é descartada em silêncio.
18. Movimento absoluto não vira delta: `SetCursorPos` para posicionar, `MOUSEEVENTF_MOVE` relativo para arrastar.
19. Salvar `.ps1` com BOM UTF-8 (PowerShell 5.1 lê como ANSI).
20. **Conferir a janela da frente antes de clicar.** O app reabre menor depois de recompilar e o Claude Desktop fica atrás; um duplo clique já abriu outra conversa. Focar (`FindWindow` pelo título), maximizar, capturar e conferir a barra de título.
21. Rodar hook `WH_MOUSE_LL` em paralelo e descartar a corrida se aparecer evento com `injected=0` (mouse físico do usuário contaminou).
22. Captura de tela reduzida ≠ escala de DPI: converter coordenadas pela razão real entre captura e tela, não por 1,25.
23. Preferir a ação `INPUT` da bridge (eventos injetados dentro do egui, sem tocar no cursor do SO) para a maioria dos testes; usar SendInput real para validar o caminho do usuário (caneta/mouse do Windows, foco, arrasto entre painéis).

## GPU

24. Testes que pedem GPU padrão pegam a Intel e podem fechar o processo: usar `PowerPreference::HighPerformance` (GTX 1650).
25. GPU do notebook fica a 300 MHz no plano "Economia de energia": FPS baixo no dev não é bug do motor. Não mexer no plano de energia.
26. Todo WGSL novo passa por validação com naga em teste (sem GPU) antes de rodar no app; erro de shader em runtime derruba o loop de render.
27. O verificador WGSL não aceita struct aninhada em uniform (ANIGO).
28. O app só redesenha quando algo muda: para medir custo, manter algo animando.

## Processo

29. Módulo com shader+struct+teste mas sem chamada no loop de render **não existe** para o usuário (auditoria do Smart Canvas achou dezenas). Toda feature visual precisa de evidência na tela.
30. Quebras de linha mistas por arquivo: script em modo texto reescreve o arquivo inteiro e some com o diff real. Editar preservando CRLF/LF.
31. Pesquisar antes de implementar, e de novo depois da primeira tentativa falha.

## Trabalho com vários agentes

32. **Nunca `git add -A` (nem `git add app/crates`) com outro agente trabalhando**: o commit leva o trabalho em andamento dele. Sempre `git add` com os caminhos exatos do próprio escopo. (2026-10-15: aconteceu e foi desfeito com `reset --soft`.)
32b. **O índice do git é compartilhado entre agentes**: `git add` de um entra no `git commit` do outro (2026-10-16: o trabalho de áudio saiu dentro do commit dos efeitos). Commitar SEMPRE com pathspec: `git commit -m "..." -- <caminhos>` (commita só esses caminhos, ignorando o resto do índice), sem `git add` prévio.
33. Dois agentes compilando no mesmo `target/` não quebram nada (o cargo serializa pelo lock), mas um espera o outro; tarefas sem build podem rodar em paralelo à vontade.

## Calibração N1 (corte D, quadro 0, ~95 tentativas)

34. **Paquímetro de faixa vence a meia altura.** Medir linha pela meia altura do contraste erra quando os dois lados são diferentes (pele de um lado, cabelo do outro): o centro escorrega para o lado fraco. `MEASURE_LINE` com `line_color` + `tolerance` (trecho contínuo de pixels da cor da tinta) deu FLIP 0,163→0,148 e F1 0,81→0,85 de uma vez. Para realces (faixas claras de cor variável), medir primeiro com a cor média e remedir com a cor da própria faixa; a busca precisa ser ≥ metade da largura máxima (busca 6 trava a largura em 12).
35. **A cor da tinta muda ao longo da mesma linha** (mais clara no alto, quase da luma do cabelo sobre a sombra de trás). Cor por trecho de ~180 px, medida no centro medido e filtrada (só amostras com cara de tinta), valeu SSIM +0,005 e F1 +0,015. Em luma, linha sobre cabelo carmim quase some; o que pesa no SSIM/F1 são as bordas contra áreas claras.
36. **Densidade de medição importa**: pontos a cada 8 px → 3 px e sem pular pontos subiu F1 0,856→0,872. Suavização forte (±3 pontos a 8 px = ±24 px) corta curvas; usar janela pequena.
37. **Linha que encosta numa área clara e nítida (pele, fundo) se ancora pela borda dela** (`edge_offset_line`), com a largura medida nos perfis; a busca tem que cobrir o erro da estimativa (o queixo errou 12 px numa busca de 10 e ficou ondulado).
38. **Balde com classificação automática por ponto mais interno erra** (ponto cai numa linha ou sombra): regiões grandes ganham semente manual; cores de pele/boca só automáticas se muito próximas e região pequena; cor de realce nunca vira região do balde.
39. **Ler a régua antes de perseguir**: medir quanto uma perturbação imperceptível custa em cada métrica (decisão 23) evitou trabalhar contra um limite de FLIP que nem +2 níveis de cor cumpria. O F1 de bordas a 2 px é a métrica mais exigente (equivale a ~1 px de registro).
40. **Ordem canônica de camadas num lugar só** (`d218.ORDER` + `reorder()`): script que reordena com lista própria esqueceu camadas novas e pôs a base branca do fundo por cima do cabelo.

## Calibração N1, redesenhos de tremor (corte D, desenhos 7 e 8)

41. **Redesenho de tremor = mesmo desenho, deformado**: em vez de reler tudo, um campo de deslocamento esparso (correlação de janela entre a referência do desenho 1 e a do redesenho, em pontos ao longo das estimativas, com filtro de vizinhos) leva as estimativas para perto; as medições de sempre fazem o resto. Partes que mudam de verdade (alça, laço, gola, olhos) precisam de releitura própria por varredura.
42. **Duas estimativas, a medição escolhe**: medir a estimativa do desenho 1 e a do campo e ficar com a que cai mais na tinta/realce/aresta valeu F1 +0,01 no 7. O desempate importa (linhas do cabelo: desenho 1; contornos diretos: só o campo — escolher pela tinta no centro piorou).
43. **Replicar o detector da métrica** (`canny_pair.py`: luma 709, σ 1, Sobel/8, NMS, histerese com limiares da referência) mostrou o que o mapa de bordas esconde: arestas da referência entre 7 e 10 de gradiente só valem porque se ligam a uma aresta forte (o brilho da gota, a sombra encostada); as nossas, mais nítidas, viram "sobrando" onde a da referência é fraca. Cor exata (brilho a luma 254, nó do laço mais claro) e sombra encostando na gota decidem se a borda existe.
44. **Polígono digitado de varredura fica meio pixel para dentro** (o vértice é o último pixel do trecho, a borda real está meio pixel além): crescer 1 px para fora valeu F1 +0,008 no 8.
45. **Tolerância frouxa na varredura engana**: carmim escuro do cabelo fica a ~55 da tinta e entrou como tinta (forma errada na gola do 7); usar tolerância justa (≤ 30) e conferir a cor nos pixels.
46. **Borda medida pode pular para uma estrutura interna** (o anel do olho do 8 tinha a borda de fora puxada para a fresta rosada, cortando 15–20 px do anel); trocar o círculo inteiro piorou (o anel não é círculo) — o que funcionou foi o arco lido por varredura por coluna. E toda mudança em linha que serve de borda do balde (anel, contorno) pode abrir vazamento: conferir FLIP/ΔE junto com o F1.

## Calibração do corte C (rosas, quadro 15)

47. **Ler as bordas da referência, não só a tinta**: o Canny da régua não vê as linhas vermelho-escuras suaves dentro das rosas (gradiente 4–9, abaixo do limiar ~11); o que conta são as fronteiras salmão × vermelho das pétalas e as linhas contra áreas claras. `edge_grid.py` (bordas da referência + as nossas por cima, com grade) é o melhor auxílio de leitura; o recorte só de tinta (`ink_grid.py`) engana nessas áreas.
48. **O balde precisa fechar as pétalas**: linhas de pétala soltas deixam salmão e vermelho na mesma região e a mediana pinta tudo de uma cor só — perde-se a borda mais forte da rosa. Folga 8 no balde (em vez de 4) fechou as pontas e deu F1 +0,026 de uma vez; `c131_mixed.py` lista as regiões de cor misturada (vazamentos) e `barrier_view.py` mostra onde fechar.
49. **Cor da região = mediana da referência sobre a região inteira** (não no ponto mais interno) e fundo elíptico medido por anéis de raio: FLIP 0,32 → 0,25 sem tocar nas linhas.
50. **Satsuei medido pela borda**: ajuste de erf numa borda limpa da referência deu σ ≈ 0,9–1,1 contra 0,6 nosso; desfoque 3,0 no quadro + foco suave na periferia subiu SSIM e F1 juntos.
51. **Cache de medidas por linha**: remedir 160 linhas leva 2,5 min; com cache (chave = estimativa + parâmetros) o ciclo editar → medir → balde → COMPARE cai para ~8 s, o que permite escolher por linha a VARIANTE de medição (meia altura × faixa, âncora, busca curta) pela régua (`c131_opt.py`) sem tocar na geometria digitada.
52. **Experimentos globais pequenos enganam**: largura ×1,25, cor mais clara das linhas internas, tolerância de cor maior e busca curta pioraram o F1 no conjunto mesmo quando um caso local parecia pedir isso. Testar sempre no quadro inteiro e, para decisões locais, com `boxf1.py` na caixa.


## Calibração do corte A (#138)

53. **Antes de digitar fundo, ver se ele tem borda na régua**: no A o fundo desfocado (σ≈3) não gera NENHUMA aresta no Canny da referência (as 44 049 px de borda estão todas no personagem), então fundo e ornamentos só pesam em SSIM/FLIP — precisão de forma média basta, e o que importa é não criar aresta falsa (faixa nítida sem desfoque, borda de faixa passando por trás do personagem). Faixas verticais com cor por banda ajustada ao perfil (mínimos quadrados com o desfoque simulado) deram resíduo ~2 níveis.
54. **PROBE_COLOR devolve `rgb` em 0–1**: dividir por 255 de novo pinta preto. E `MEASURE_EDGE` com busca 16 em peças separadas por vãos de ~8 px (pétalas) pula para a borda vizinha; busca 7→3 com estimativa digitada a ±5 px.
55. **Para escolher as sementes, imitar o balde do app inteiro**: núcleos = componentes de {D > gap/2} e região de cada núcleo por propagação que NÃO atravessa linha (watershed com máscara fora da linha). Atribuir pelo núcleo mais próximo em linha reta misturou o lilás do outro lado do fecho na mediana do branco do olho (corte reto de Voronoi). Gap grande (36) fecha os vãos do contorno digitado, mas deixa sem núcleo as regiões fechadas estreitas (anel da íris, branco do olho): segunda passada com gap 4 só onde ficou vazio.
56. **"É fundo" não se decide por cor parecida**: o branco do olho e o cabelo claro ficam a < 30 da parede creme e foram deixados vazios. Fundo = regiões que contêm pontos digitados de fundo.
57. **Íris, broche e pálpebras grossas não se medem com o paquímetro de linha**: o miolo escuro (íris marinho, pedra) puxa a linha para dentro e a engrossa. Geometria digitada a partir de perfis de linha/coluna (raio do contorno nas duas direções) e cor do núcleo medida no perfil.

## Micro anime (S22)

58. **Ferramenta que lê pixels do cel (balde, lint) não pode passar pela posição animada da camada.** A camada com chaves de deslize pintou o quadro inteiro de uma cor só; a primeira suspeita (polilinha fora do canvas) estava errada — a pista era que a primeira execução, ANTES de criar as chaves, tinha saído certa. Quando um script reexecutável começa a falhar, perguntar o que a execução anterior deixou no documento.
59. **Spline fechado de 4 pontos vira oval**: retângulo de manga desenhado como spline fechado incha e a semente cai fora; usar pontos intermediários (ou polilinha) em formas retas.
60. **Recorte pelo quadro abre regiões**: personagem cortado pela borda precisa de uma linha de fechamento na borda (o lint ignora regiões que tocam a borda, então elas ficam sem cor sem aviso).

## Interface (S20)

61. **Abrir projeto versionado no app para testar a UI grava nele**: cada comando vai para o `wal.ndjson` na hora (sem salvar). Para testes que editam, copiar o projeto para o scratchpad e abrir a cópia; conferir `git status producoes/` ao fechar.
62. **Render de painel que não é o quadro do corte usa `camera: false`**: o padrão do `RenderOptions` enquadra pela câmera do projeto (16:9) e recorta uma folha de modelo larga, deslocando tudo o que se desenha por cima em coordenadas do canvas.
63. **Coordenadas do `aiai_input` são pontos lógicos** (1536 de largura com escala 125%), não pixels da captura reduzida (1280): converter ou ler os retângulos do `GET_UI_STATE`.
64. **Atalho global consome a tecla antes do widget que viria depois no quadro.** Os atalhos do editor rodavam antes da barra de menus e comiam Esc/setas: o menu aberto nunca fechava com Esc. Atalho de editor só com `!egui_wants_keyboard_input() && !Popup::is_any_open()`.
65. **O PC é compartilhado com outros projetos do usuário** (builds `sc-core` do Smart Canvas rodando junto): RAM paginando deixa quadros de vários segundos e builds 10× mais lentos. Antes de medir desempenho ou rodar o roteiro de mouse, conferir `cargo.exe` de outros projetos; não mexer neles.
66. **Veredito de UI pelo estado, não por diferença de pixels da janela inteira**: a captura muda sozinha (miniaturas e cortes carregando em segundo plano). Pixels só na região do efeito (canvas sem o rodapé de latência, cursor fora) ou um campo do `GET_UI_STATE` (`popup_open`, `dialogs`).
