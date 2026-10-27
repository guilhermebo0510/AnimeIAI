# Auditoria volume 2 — o que foi feito (26/10/2026)

Fontes: `AUDITORIA_V2.md`, `AUDITORIA_V2_MASSIVA.md`, `AUDITORIA_V2_JORNADAS.md`, `AUDITORIA_V2_FRONTEIRAS.md`, `AUDITORIA_V2_UX.md`.
Lidos depois de fechar a auditoria 1 (ver `06-status-auditoria-1.md`). Cada item abaixo diz o que mudou e onde.

## Achados

| ID | Prio | Achado | Status | O que mudou |
|---|---|---|---|---|
| V2-01 | P1 | Texto do roteiro some ao trocar de workspace | FEITO | `panels/mod.rs` `pending_text`: o rascunho grava ao perder o foco, depois de 2 s sem digitar e, se o campo sumiu com texto pendente, quando ele volta; teste `text_typed_before_the_panel_vanished_is_committed_when_it_comes_back` |
| V2-02 | P1 | Modelagem pendente fora do Salvar e do aviso de fechar | FEITO | `modeling::pending` entra no `doc_dirty` (asterisco, aviso ao fechar/abrir/novo); `doc_save_to` grava o modelo no projeto antes (`flush_into_project`) |
| V2-03 | P1 | Cadeados: pasta, timeline e criação automática | FEITO | `commands::locked_effective` (camada ou pasta acima); usado em `tools::target`, `set_exposure` e antes de criar camada sozinha |
| V2-04 | P2 | Camada criada sozinha + traço = dois desfazeres | FEITO | `CoreSession::merge_last` junta a criação e o traço; gesto vazio desfaz a camada criada |
| V2-05 | P2 | Reacoplar a timeline muda o layout | FEITO (antes desta rodada) | o teste `detach_and_redock_restores_the_layout` passa (353 testes verdes) |
| V2-06 | P2 | Revisões de fábrica dos layouts | FEITO | os dois testes passam; o ajuste de layout desta rodada subiu Layout 5, Cenário 4, Animação 5 (e Douga), Pintura 4 |
| M-01 | P2 | Desfazer não volta a seleção transformada | FEITO | `Editor::sel_undo`: o Mover grava a seleção antes/depois; desfazer/refazer devolvem a que combina |
| M-02 | P2 | Janela de detalhes do pincel cresce sozinha | FEITO | corpo com altura fixa e rolagem própria, rodapé sempre visível, no máximo 90% da tela |
| M-03 | P1 | NLE: navalha em trilha travada corta o vinculado; efeitos de clipe travado mudam | FEITO | `razor` recusa quando o alvo ou um vinculado que atravessa o quadro está travado; o inspector de efeitos não grava em trilha travada |
| M-04 | P2 | Salvar paleta falha antes de visitar a Biblioteca | FEITO | a raiz é resolvida na hora do salvar; o resultado vira aviso junto de quem pediu |
| J01 | P2 | PNG inválido deixa camada vazia | FEITO | a imagem é decodificada antes de mudar o projeto; se a importação falha, a camada preparada sai; sucesso = um passo de desfazer |
| J02 | P2 | Borracha vetorial com título de borracha raster | FEITO | `shelf::current_sub` prefere a última escolhida e a aba aberta |
| J03 | P3 | "Corrigir" só navega | FEITO | botão "Abrir <workspace> ›" e dica "nada muda sozinho" |
| F01 | P2 | Inspector do NLE: Entrada/Duração ignoram o vínculo | FEITO | Entrada (slip) e Duração (trim) usam o grupo vinculado, como o Início |
| F02 | P2 | Preflight aprova com modelo 3D faltando | FEITO | camadas 3D visíveis e fora do modo referência têm o blob do modelo conferido |

## UX

| ID | Status | O que mudou |
|---|---|---|
| UX-01 | FEITO | Figura: "Contorno" com a ponta vetorial no painel principal da ferramenta |
| UX-02 | FEITO | Figura em camada raster: "Contorno com o pincel atual" percorre a geometria (preenchimento antes, um passo de desfazer); a ponta vetorial escolhida agora vale (antes era sempre Marcador) |
| UX-03 | FEITO | diagramas (formas, modos) numa caixa fixa à esquerda com o nome logo ao lado |
| UX-04 | ABERTO | lista de pincéis em colunas quando o painel alarga |
| UX-05 | FEITO | cabeçalho da Modelagem: o que não cabe vai para "Mais ›" (medido pela largura real) |
| UX-06 | FEITO | parâmetros da operação ativa primeiro; catálogo de operações num bloco recolhível |
| UX-07 | FEITO | X/Y/Z nomeados e coloridos, com unidade (m, °) |
| UX-08 | FEITO | três linhas: transporte com o alcance (seleção ou arquivo inteiro), Editar (fades com a curva ao lado) e Ver |
| UX-09 | FEITO | Voz sintética: motor e fala primeiro; hardware e motores opcionais recolhidos no fim |
| UX-10 | FEITO | ferramentas do NLE com ícones desenhados (seleção, ripple, roll, navalha, slip, slide) e nome curto; atalho na dica |
| UX-11 | FEITO | inspector do NLE: bloco "Sequência (a edição inteira)" separado do clipe |
| UX-12 | FEITO | Composição: o Mover mostra só o que vale ali (a camada inteira) |

## Propostas de produto ainda abertas (das rodadas massiva, jornadas e fronteiras)

Não são falhas; ficam como próximos passos, nesta ordem:

1. ~~Breadcrumb e destino do próximo traço~~ FEITO: barra de status "Desenhando em: EP › cena › corte (ou painel / ficha) › camada › cel N · L×A" nos workspaces de canvas, com dica; `commands::target_breadcrumb`; título da janela já usa o nome do projeto.
2. ~~Nome-base estável nas versões de áudio~~ FEITO: `audio_edit::next_version_name` ("Fala" → "Fala v2" → "Fala v3", tira "(editado)" antigos); vale na edição e no colar entre arquivos; o histórico já mostra o resumo dos passos de cada versão.
3. ~~Selo de referência 3D~~ FEITO: selo "Referência · fora da entrega" na linha da camada; estado ("Referência · fora da entrega" / "Participa do render") com a caixa "Só referência" logo abaixo do nome nas Propriedades da camada e no alto da seção 3D (`model3d::output_state`).
4. ~~Exposto × escolhido~~ FEITO: a linha do tempo mostra "Exposto neste quadro: Cel N" e, quando difere, "Escolhido para expor: Cel M" (com dica do que cada um faz); a lista de cels rola até o cel criado ou duplicado.
5. ~~Colar áudio e precisão da seleção~~ FEITO: colar de outra versão seleciona o trecho inserido, como o da mesma versão; o resultado de uma edição abre mantendo a seleção (`open_edited`; recortar/apagar/aparar continuam limpando); campos Início/Fim/Duração editáveis (segundos ou amostras), "Desde 0"/"Ir a 0" e a duração total.
6. ~~Recursos indisponíveis~~ FEITO: a janela de abertura diz "Recursos indisponíveis" com tipo (desenho, áudio, modelo 3D...), nome, uso e estado (ausente/ilegível); hash e mensagem técnica em "Detalhes técnicos"; "Localizar arquivo…" e "Procurar em pasta…" (backup) só aceitam conteúdo idêntico ao esperado (`persist::restore_blob`), "Abrir pasta do projeto" e "Continuar com aviso". Junto: reabrir no mesmo processo não mostra mais o desenho perdido vindo dos caches (tiles decodificados, camadas do quadro e miniaturas respeitam os blobs danificados; era o F-REC-05 inconclusivo).
7. Produção: avisar amostra insuficiente nas métricas (massiva).
8. UX-04: grade de pincéis em colunas.
