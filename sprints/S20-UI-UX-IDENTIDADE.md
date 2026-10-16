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
