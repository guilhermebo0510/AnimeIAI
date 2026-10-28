# Plano do trabalho contínuo (a partir de 27/10/2026, noite)

Pedido do usuário: trabalhar sem parar e sem esperar validação até cumprir tudo abaixo, na ordem. Um item por vez,
teste + conferência visual + commit a cada item. O hook de Stop (`tools/overnight_stop_hook.py`, em
`~/.claude/settings.json`) mantém o trabalho andando enquanto `sprints/.overnight-done` não existir.

Regras que continuam valendo: sem IA generativa de imagem; não copiar código de terceiros (ANIGO é nosso e pode ser
aproveitado); não pôr imagens/pontas/texturas do Clip Studio no app; material de referência e `calibracao/` fora do git;
instalar só o que não oferece risco; não mexer em configurações de sistema/segurança; erro não mascarado; validar a olho;
build com `CARGO_INCREMENTAL=0 -j 2`, nunca `--workspace`; fechar o app do usuário só pelo PID para pôr build novo.
O mouse está liberado (pode usar o Clip Studio e o app de verdade, segurando `calibracao/tmp/ui.lock`).

## 1. Auditorias 1 e 2 — tudo que estiver aberto

Status em `06-status-auditoria-1.md` e `07-status-auditoria-2.md`. Em aberto na auditoria 1 (27/10, 22h):

- [x] 4.7h colorização por dicas
- [x] 4.8f casamento de inclusão (BasicPBC) no `propagate_fills`
- [x] 4.4c alocações (imagem f32 de quadro inteiro no composto) — cel de tiles/preenchimento/traço montado na caixa
- [x] 4.4a compositor na GPU (e as metas de 4K de 4.4g) — medido: retângulo sujo + quadro reciclado (78→21–28 ms); GPU nos plugins
- [x] 4.6g Kokoro sem `uv` (ONNX embutido) — decisão registrada: ort baixa binários de fora + G2P em Python; fica o uv com verificação
- [x] 4.6h cor linear / HDR (mistura em luz linear no projeto, tiles AIT2 sem corte)
- [x] 4.6k projeto em arquivos por corte com trava por corte
- [x] 4.6d rigging 2D nativo (ossos, IK, pai-filho)
- [x] 4.9h bancada de calibração (Steger, w(s), junções; tolerância de borda) — w(s) aponta 6 canetas para recalibrar (seção 2)
- [x] 4.9g plugins `.aiaiplugin` (filtros WGSL na GPU)
- [x] 4.8e IA pontual de traço (só assistência determinística; sem IA generativa de imagem) — linha fantasma + embelezar
- [x] 4.1c–f mesa digitalizadora (WM_POINTER nativo, inclinação, borracha traseira, coalescência; WinTab fica de fora)
- [x] 1.7B / 1.7C2 decisões do usuário (1.7B virou opção; 1.7C2 registrada)
- [x] 3.1–3.11 roteiro do curta (rig e cenas) — feito no rig e ensaiado numa cópia do projeto (renders antes/depois em `evidencias/S25/curta/`, a aprovar): 3.2/3.3 cabelo e busto em 3/4, 3.9 satsuei, 3.10 blush/cintilância/respiração, 3.8 braços em arco e flare, 3.11 passagem C005→C006 (+ transição dupla exposição). 3.1 (perfil) e 3.7 (3 planos de atmosfera) ficam para a próxima rodada de produção com o usuário

Auditoria 2: as 8 propostas estão feitas; relidos os `AUDITORIA_V2*.md` (28/10): os 35 IDs estão no `07-status` (F-NLE-01..03
= F01, F-REC-04 = F02, F-REC-05 na proposta 6).

## 2. Calibração com o Clip Studio Paint

- [x] Rodar de novo as pranchas de calibração (canetas, lápis, pincéis, aerógrafos, borrachas, decoração, misturadores)
  contra o CSP e corrigir o que destoar — lápis melhorou (2,641→2,532), o resto igual; feltro testado em 3 variações, ficou
  como estava (`09-equivalencia-csp.md`)
- [x] Conferir que todas as subferramentas do CSP que usamos têm equivalente e funcionam (lista por grupo) — as 203 em
  `09-equivalencia-csp.md`; faltavam e foram feitas: Livro, Prédio, Mancha de sangue, as 7 linhas com régua, os 8 de
  roupas (multi-ponta em sequência) e o degradê Fotosfera

## 3. Criação de pincéis e canetas pelo usuário

- [x] Estudar (internet + o próprio CSP) como o CSP deixa criar qualquer caneta: categorias da Configuração de
  sub-ferramenta, pontas, dinâmicas, textura, mistura, entrada/saída, correção, etc. — `10-criador-de-pinceis.md`
- [x] Comparar com o nosso criador (Configurações avançadas, Criar pincel) e implementar o que falta — novos no motor:
  tamanho na tela, pelo menos 1 px, variar para a secundária e por traço, densidade pelo intervalo, rotação da textura,
  partículas alinhadas, borda de aquarela (escurecer, largura), 10 modos de mesclagem, forma do pincel (copiar/colar);
  no editor: mistura de tinta, spray, grão, misturar pontas com escurecer, borracha vetorial/todas as camadas, encaixe;
  ponta minúscula que sumia na divisa de pixels corrigida; lista de categorias parada e só os parâmetros rolam;
  `set_brush` com `custom` (qualquer campo do pincel pela ponte/MCP). Fica de fora: saída em % do traço (motivo no 10)

## 4. Workspace de Modelagem

- [x] Ver o que o ANIGO tem (código nosso) e o Blender oferece; trazer ferramentas e melhorias de interface — `11-modelagem.md` (cabelo fica de fora: personagem 3D)
  - [x] multivista (1/2/4 vistas, Ctrl+Alt+Q, Ctrl+Espaço, nome da vista no canto, grade no plano da vista alinhada) — `11-modelagem.md`
  - [x] escultura (20 pincéis do ANIGO, simetria, dyntopo, máscara, alisar tudo, remalhar, simetrizar)
  - [x] editor de UV (costuras, desdobrar, empacotar, projeções, métricas, editor 2D com G/R/S)
  - [x] normais personalizadas para a sombra cel (esfera, vista, de outro objeto, média, suavizar)

## 5. Revisão das workspaces de Edição, Áudio e Roteiro

- [x] Pesquisar o que melhorar e implementar — `12-revisao-edicao-audio-roteiro.md` (ondas e marcadores na Edição, multipista que cabe no painel, Ctrl+Enter, Fountain e elenco no Roteiro)

## 6. Por último: pasta `ferramenta de Ia`

- [x] Só depois de 1 a 5: ler toda a discussão (Claude, ChatGPT, Gemini) e implementar a ferramenta (experimental) —
  a discussão fechou no R16 (MVP-0: prévia de intercalação de um quadro, aceito pelos três revisores); implementado na
  ordem do §13: manifesto da base, origem da entrada no log, prévia (tabela de suporte, limites, pares por id), aceite
  com registro no cel, bridge/MCP com dono da proposta, interface (sobreposição, barra, corrigir pares) —
  `sprints/registro/S26-intercalacao/README.md`. Fica com o dono: P-3 (limites e orçamento), decisão síncrono × worker
  com os números do E1 e o teste com pessoa (§11)

Ao terminar tudo: criar `sprints/.overnight-done`.
