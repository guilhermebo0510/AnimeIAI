# S25 — Lado direito e topo das áreas de desenho (organização inspirada no Clip Studio)

Pedido do usuário: "mais ícones e menos texto, uma coisa mais limpa e organizada", com a nossa identidade (ícones
originais, desenhados na grade de 24 px do app). Este registro cobre a coluna da direita, a barra de comandos, a barra do
canvas, o lançador da seleção, a barra de materiais e o acesso aos filtros. O lado esquerdo (prateleira, sub-ferramentas,
propriedades da ferramenta, tamanhos, cores) é de outro agente (`02-…`), e o motor de pincel do agente de calibração.

## O que mudou

### 1. Coluna da direita
- **Navegador no topo** (Layout, Cenário, Animação Genga/Douga, Pintura): miniatura com o retângulo da visão (arrastar
  move a visão, a roda aproxima), e embaixo duas linhas de ícones:
  - zoom: afastar, controle deslizante (logarítmico), aproximar, 100 %, enquadrar e o valor em %;
  - rotação: girar 15° para a esquerda, controle deslizante (−180°…180°), girar para a direita, sem rotação, espelho
    horizontal, espelho vertical e o valor em graus.
  O espelho vertical é só de visão: a `View` guarda um espelho só, então vertical = horizontal + meia volta; a rotação
  mostrada é a do usuário (`navigator::user_rotation_deg`), e o rodapé do canvas mostra a mesma.
- **Propriedades da camada** (painel novo `layer_props`) logo abaixo: tipo e nome, a lista de **filtros** da camada
  (olho para ligar/desligar, lixeira para tirar, clique para ajustar, "+ Filtro" com o mesmo cardápio do menu) e papel,
  profundidade e deslocamento.
- **Camadas | Histórico | Propriedades** em abas embaixo (Ação automática fica para depois: não existe gravação de ações
  no app ainda; quando existir, entra como mais uma aba).

### 2. Painel Camadas
- Barra de ícones: nova raster, nova vetorial, nova pasta (ícone do tipo com "+"), mais tipos (o menu antigo, com 3D e
  manequim) | recortar na de baixo, travar, **referência do balde** (liga a camada de linha à camada de preenchimento
  mais próxima abaixo dela; aceso quando alguma camada de preenchimento usa a linha) | mesclar, apagar.
- Mistura e opacidade numa linha só, logo abaixo da barra.
- Linhas com olho e cadeado à esquerda, **miniatura do cel exposto** (xadrez por baixo, selo do tipo no canto), "100%
  Normal" em cima e o nome embaixo. O cadeado destravado só aparece com o mouse em cima.
- As miniaturas são rasterizadas numa thread própria (`layer_thumbs.rs`), com assinatura barata do cel; só refaz o que
  mudou e nunca durante um gesto. Entram no orçamento de VRAM (`gpu_owners::Slot::LayerThumbs`).
- Mantido: escolha múltipla (Ctrl/Shift), arrastar para reordenar/entrar em pasta, renomear com duplo clique, Delete,
  Ctrl+G, cels, ligação do balde, avisos do recorte. Subir/descer, agrupar, mover para pasta, renomear, mesclar e apagar
  estão no menu de contexto da linha.

### 3. Histórico (painel novo)
Lista os passos de desfazer do documento com nomes legíveis (`history::step_label`: "Pintura · Raster 3",
"Traço ×3 · Vetor 4", "Opacidade · Linha", "Nova camada · Linha", "Mostrar/ocultar · …"; os lotes com rótulo mostram o
rótulo), mais antigo em cima, desfeitos atenuados embaixo, a primeira linha é o estado inicial. Clicar num passo desfaz ou
refaz até ele. O core ganhou `CoreSession::history_commands` (os comandos das pilhas) para os rótulos.

### 4. Barra de materiais
Coluna de ícones entre o canvas e a coluna da direita (dentro do painel do canvas: vai junto ao maximizar ou soltar):
Texturas, Padrões, Imagens | Materiais 3D, Modelos 3D, Poses | Pincéis, Paletas e degradês, Personagens do projeto.
Clicar abre o painel da categoria (busca, etiquetas, miniaturas, largura arrastável) tirando espaço do canvas; clicar de
novo recolhe. Duplo clique ou arrastar para o canvas usa o recurso pelo caminho da Biblioteca (`library::use_entry`:
textura/imagem/modelo vira camada, paleta entra no projeto e fica ativa, pincel é escolhido). Padrões e Imagens são as
texturas com etiqueta, categoria ou subpasta "padrão/pattern/tile" ou "imagem/foto/referência". Personagens: duplo clique
deixa a paleta de settei do personagem ativa nas Cores.

### 5. Barra de comandos (abaixo das abas de workspace, só nos workspaces de canvas)
Novo, Abrir, Salvar | Desfazer, Refazer | Limpar (dentro da seleção, ou a camada inteira), Limpar fora da seleção,
Preencher a seleção, Transformar | Cancelar seleção, Inverter seleção, Mostrar borda da seleção | Encaixar nas réguas |
Filtro. Dicas com o atalho em uso (do mapa de atalhos). Botões apagados quando o comando não pode rodar
(`workbar::Cmd::available`). Fica embaixo das abas de workspace para as abas não pularem ao trocar entre um workspace com
canvas e um sem.

### 6. Barra do canvas
Sem a linha de título "Canvas": a própria barra de ícones é o cabeçalho (arrastar, duplo clique para maximizar, botões de
soltar/maximizar no fim dela). Saíram as opções da ferramenta ("Redondo duro ›", "Tamanho 24 px", "Opacidade",
"Média móvel 4": estão no painel de propriedades da ferramenta à esquerda) e o campo de graus. Ficaram só ícones de visão:
enquadrar, 100 %, zoom, girar, sem rotação, espelho H/V, grade, quadro de câmera, guias, referências, papel cebola
(Animação e Pintura), réguas à mostra e a fase Genga/Douga na Animação. O encaixe nas réguas foi para a barra de comandos.

### 7. Lançador da seleção
Barrinha flutuante embaixo da seleção ativa (em cima, se não couber): cancelar, inverter, expandir 4 px, contrair 4 px,
preencher, limpar, limpar fora, transformar, copiar para camada nova. Some durante o desenho, com transformação pendente e
no Editar pontos. Delete com o cursor no canvas limpa a seleção.

### 8. Filtros
Menu **Filtro** na barra de menus (workspaces de canvas) e botão Filtro na barra de comandos, com o mesmo cardápio
(`workbar::filter_groups`). Um filtro é um efeito não destrutivo da camada ativa (ação `EFFECTS`, um passo de desfazer);
ao aplicar abre a janela **Filtro** com os parâmetros da pilha (o painel Controles de efeito da Composição numa janela),
com o canvas atualizando ao vivo. "Renderizar" desenha linhas de foco/velocidade numa camada vetorial nova (ação `LINES`).

| Clip Studio (Filtro) | O que temos agora no menu Filtro | Falta |
|---|---|---|
| Filtros rápidos / Efeitos rápidos | Efeitos rápidos: Foco suave, Brilho mágico, Impacto, Filme, Entardecer (pilhas prontas de anime) | filtros "artísticos" de um clique (pintura a óleo, aquarela) |
| Correção | Níveis, Curvas, Matiz/saturação/luminosidade, Equilíbrio de cor, Brilho e contraste, Mapa de degradê, Limiar, Posterizar, Inverter, Correção primária (lift/gamma/gain) | — (LUT existe, mas precisa de arquivo .cube: fica no navegador de efeitos da Composição) |
| Desfoque | Gaussiano, Direcional (movimento), Radial, Sensível à cor (não mistura regiões de cel) | Suavizar simples (o gaussiano cobre) |
| Distorcer | — | ondas, ondulação, redemoinho, lente, coordenadas polares (não temos deformação de pixels como filtro) |
| Efeito | Luz e brilho (Brilho, Difusão, Halação, Luz transmitida, Light wrap, Luz de recorte, Raios de luz, Reflexo de lente); Estilo (Sombra, Contorno, Sobrepor cor, Duas cores, Para); Lente e filme (Aberração cromática, Vinheta, Grão, Anti-faixas) | mosaico, extrair linhas, retícula como filtro |
| Renderizar | Linhas de foco, Linhas de velocidade (traços vetoriais editáveis) | ruído Perlin, nuvens |
| Tornar nítido | — | nitidez, máscara de nitidez |

## Layouts de fábrica
Os de desenho trazem Navegador, Propriedades da camada e Camadas | Histórico | Propriedades à direita. A revisão de
fábrica subiu uma vez junto com a do lado esquerdo (Layout 4, Cenário 3, Pintura 3, Animação 4 com o Douga) e a
Composição foi a 4 (Histórico em aba com Projeto e Camadas). Layouts com nome continuam valendo; quem tem layout próprio vê
o aviso "o padrão foi atualizado".

## Testes
Testes de unidade novos (`cargo test -p aiai-studio --lib`, 324+ verdes no lado direito):
- `history`: plano do pulo (desfazer/refazer N), nomes legíveis com a camada (sem id cru), pular para um passo e
  voltar, estado inicial.
- `workbar`: disponibilidade de cada comando (sem corte, sem seleção, camada travada, vetor x raster), textos e
  atalhos das dicas, todo efeito do menu Filtro valida e nenhum fica de fora, filtro na camada ativa, efeito rápido
  num passo só, linhas de foco numa camada vetorial nova.
- `sel_ops`: limpar/limpar fora/preencher pela cobertura e pela colocação da camada, só os tiles que mudam, posição do
  lançador, e num documento de verdade: preencher, expandir, copiar para camada nova, limpar a camada, recusa sem
  seleção.
- `materials`: texturas separadas em padrões/imagens (etiqueta e subpasta), abrir/trocar/recolher e a largura que o
  canvas devolve.
- `navigator`: espelho vertical (cima vira baixo, rotação mostrada continua 0), os dois espelhos = meia volta,
  rotação do usuário com e sem espelho.
- `layer_thumbs`: assinatura do cel muda só com o conteúdo; redução mantém a proporção.
- `layer_props`, `i18n_right` (toda chave usada nos fontes do lado direito existe nos dois idiomas).
- `tests/suite.rs`: subir/descer pelo menu de contexto da linha.

## Capturas (mouse e teclado reais, `input.ps1 -ProcessId`, cópia `calibracao/tmp/s25r_copy.aiai`)
Em `evidencias/direita/`, para pôr ao lado de `calibracao/csp/32_materiais_overlay.png`, `31_barra_materiais.png`,
`35_historico_acao.png`, `36_barra_comandos.png`, `30_menu_filtro.png` e `00_inicio.png`:
- `r00_inicio.png`: Pintura com a direita nova, a barra de comandos e a barra do canvas só com ícones.
- `r01_materiais_texturas.png`, `r02_materiais_padroes.png`: painel de materiais aberto (biblioteca de teste em
  `calibracao/tmp/s25r_lib`: papéis, retículas e hachura feitos por código); `r03_padrao_usado.png`: duplo clique
  numa retícula = camada de referência nova com miniatura.
- `r04_historico.png`, `r05_historico_pulo.png`: o Histórico e um pulo de 3 passos (desfeitos atenuados).
- `r06_lancador_selecao.png`, `r07_lancador_preencher.png`: lançador embaixo da seleção; expandir e preencher pelo
  lançador (miniatura da camada atualiza).
- `r08_menu_filtro.png`, `r09_janela_filtro.png`: menu Filtro > Desfoque e a janela Filtro com o desfoque gaussiano.
- `r10_navegador_espelho_rotacao.png`: espelho vertical e 45° pelo controle deslizante do Navegador.
- `r11_ws_animacao.png`, `r11_ws_cenario.png`, `r11_ws_layout.png`: as outras áreas de desenho.

## Observações
- Na Animação a coluna da direita tem quatro blocos (Navegador, Propriedades da camada, Mesa de luz, Camadas): a
  miniatura do Navegador fica baixa numa tela de 1080 p; os blocos são redimensionáveis.
- "Ação automática" não existe ainda (não há gravação de ações); quando houver, entra como aba ao lado do Histórico.
- O encaixe na grade do Clip Studio não existe aqui (a grade é só de visão); o encaixe é o das réguas, que inclui a
  régua de grade e a de perspectiva.
