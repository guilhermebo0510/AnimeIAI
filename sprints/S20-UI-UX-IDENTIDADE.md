# S20 — UI/UX profissional e identidade visual

## Objetivo
Deixar o programa com cara de software profissional: logomarca e ícone originais, conjunto completo de ícones vetoriais próprios, temas claro/escuro refinados, workspaces por fase bem organizados, atalhos, PT/EN revisados, sem estouro de layout. Esta sprint é transversal: a S00 criou a base, e cada sprint de ferramenta já entrega UI utilizável. Aqui se faz a auditoria e o acabamento.

## Pesquisa obrigatória
Blender 4/5, DaVinci Resolve, Clip Studio, Toon Boom Harmony (organização de workspaces, densidade, iconografia), preferências do usuário registradas no ANIGO (abas verticais de propriedades, painéis recolhíveis com aba de reabrir, sem termos como "Inspetor", sem info de GPU na barra de status, sem ferramenta duplicada entre barra e painel).

## Tarefas
- [ ] Logomarca e ícone do app (SVG original, versões clara/escura, 16–512 px, splash), desenhados no próprio AnimeIAI ou em SVG à mão; sem semelhança com marcas existentes.
- [ ] Iconografia completa e coerente (grade de 24 px, traço uniforme) para todas as ferramentas e painéis; substitui qualquer ícone provisório.
- [ ] Auditoria de cada workspace: painéis certos, ordem lógica, nada duplicado, propriedades em abas verticais, painéis recolhíveis.
- [ ] Barra de ferramentas contextual por ferramenta ativa; painel de propriedades da ferramenta; dicas (tooltip) com atalho.
- [ ] Atalhos padrão (inspirados em CSP/Photoshop: B pincel, E borracha, G balde, V mover, espaço pan, Ctrl+Z…), editor de atalhos, detecção de conflito.
- [ ] Varredura automática de estouro de layout (PT e EN, larguras mínimas) como no ANIGO; zero estouros.
- [ ] Tema: contraste AA nos textos, estados de foco/hover/ativo, sem cor solta fora dos tokens.
- [ ] Tela inicial: novo projeto (presets de resolução/fps), recentes, abrir.
- [ ] Preferências: idioma, tema, desempenho (cache, GPU), pastas, motor de voz.

## Testes
- Varredura de estouro; teste de chaves i18n; captura de todos os workspaces em PT/EN × claro/escuro.
- Roteiro de mouse percorrendo cada workspace e as ferramentas principais.

## Critério de pronto
- [ ] Prancha de capturas (todos os workspaces, 2 idiomas, 2 temas) e a logomarca em evidências; zero estouros; zero emoji; zero ícone provisório.

## Execução (P6, a partir de 2026-10-17)

Decisão: os painéis novos chamam as MESMAS ações da bridge dentro do processo (`aiai_bridge::core_ops::handle_core_action`
com autor `user`), via um helper `app/actions.rs`. Uma regra de negócio só, o mesmo undo/log do MCP, e cada fluxo
feito pelo agente fica possível pela UI sem reimplementar. Renders longos (animatic, export) bloqueiam o quadro por
enquanto, com aviso na barra de status (lacuna: fila em segundo plano).

| Fatia | Workspaces | Conteúdo | Estado |
|---|---|---|---|
| P6a | Roteiro, Storyboard | helper de ações; Estrutura (cenas do roteiro), Roteiro ligado ao documento (elementos editáveis, fala + tradução, tempo estimado), Cenas/Prancha do storyboard (miniaturas, propriedades do painel, adicionar/mover/apagar, animatic, gerar cortes) | feito 2026-10-17 (evidencias/S20/p6a_*.png; animatic bloqueia a UI ~15 s) |
| P6b | Produção | Episódios (filtro por episódio/cena, resumo das etapas, relatório), Tabela de cortes (miniatura do quadro 0 em cache, nº, cena, duração, 8 etapas clicáveis com menu, correções/retakes abertos, sugerir etapas, lint, abrir na Animação), Propriedades do corte (etapas, lint por camada, correções com ir ao quadro e resolver, retakes com comparar ANTES/DEPOIS/DIFERENÇA) | feito 2026-10-17 (evidencias/S20/p6b_*.png; tokens st_* de estado no tema) |
| P6c | Edição, Áudio, Entrega | **Edição feita** (Mídia: documento de edição, Nova edição, cortes com miniatura e duplo clique para inserir; Monitor: quadro da edição em segundo plano com transição/legenda/créditos, quadro a quadro e arrasto; Linha do tempo: clipes proporcionais, sobreposição das fusões, legendas, créditos, cabeçote, zoom; Propriedades: entrada/saída, transição e duração, mover/tirar, legenda início/fim/texto, créditos e fade, tudo por EDIT_SET; evidencias/S20/p6c_*.png). **Entrega feita** (Predefinições TV/ARIB+SRT, Web −14, NVENC, ProRes, PNG; Fila com exportação em segundo plano numa cópia da sessão, uma por vez, estado/tempo/resultado; Propriedades: edição/episódio/corte, nome, pasta saida/, CRF; edição do micro anime exportada pela UI: 784 qd, 1920x1080, −24,0 LUFS, SRT; evidencias/S20/p6c_entrega_*.png, p6c_mp4_q246.png). **Áudio feito** (Mídia: cortes com nº de clipes, clique = corte ativo; Monitor do corte em segundo plano; Mixer: faixas Diálogo/SE/BGM/Ambiente com os clipes sobre a duração do corte e cabeçote, ganho/pan/início/trilha/fades/remover por APPLY_COMMAND SetProps, medição BS.1770 do corte, WAV do corte e WAV da edição em segundo plano; WAV da edição conferido no FFmpeg: −24,0 LUFS; evidencias/S20/p6c_audio*.png). Sem reprodução ao vivo (lacuna 19) | feito 2026-10-17 |
| P6d | Composição, Cor | Composição: pilha de efeitos da camada ativa (19 efeitos no menu, campos numéricos/cor/ponto editáveis, liga/desliga, segurar por N qd, ordem, remover) por EFFECTS e câmera do corte (zoom, rotação, centro, aproximar, panorâmica, tremer, limpar chaves) por CAMERA. Cor: Navegador = quadro do corte em segundo plano; Propriedades = escolha do corte, correção primária (lift/gamma/gain, saturação, temperatura, matiz, contraste, pivô), vinheta/difusão/grão por GRADE | feito 2026-10-17 (evidencias/S20/p6d_*.png) |
| P6e | todos | prancha PT/EN × claro/escuro, varredura de estouro, roteiro de mouse real | em andamento: **prancha feita** (`tools/ui/prancha.py`, 52 capturas pela bridge, evidencias/S20/prancha/prancha_{pt,en}_{dark,light}.png). Achados: workspace Personagens só com painéis vazios (Personagens, Folha de modelo); Navegador dos workspaces de canvas vazio; monitor da Edição abre no quadro 0 (preto pelo fade). Falta varredura automática de estouro e roteiro de mouse real |
| P6f | Personagens, Navegador | lista de personagens (settei, paleta, voz), folha de modelo (MODEL_SHEET/CONSISTENCY), navegador com miniatura do quadro e retângulo da vista | pendente |
