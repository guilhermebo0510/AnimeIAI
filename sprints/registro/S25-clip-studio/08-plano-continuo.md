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
- [ ] 4.4a compositor na GPU (e as metas de 4K de 4.4g)
- [ ] 4.6g Kokoro sem `uv` (ONNX embutido) — só se a instalação não oferecer risco
- [x] 4.6h cor linear / HDR (mistura em luz linear no projeto, tiles AIT2 sem corte)
- [ ] 4.6k projeto em arquivos por corte com trava por corte
- [ ] 4.6d rigging 2D nativo (ossos, IK, pai-filho)
- [x] 4.9h bancada de calibração (Steger, w(s), junções; tolerância de borda) — w(s) aponta 6 canetas para recalibrar (seção 2)
- [ ] 4.9g plugins `.aiaiplugin`
- [x] 4.8e IA pontual de traço (só assistência determinística; sem IA generativa de imagem) — linha fantasma + embelezar
- [x] 4.1c–f mesa digitalizadora (WM_POINTER nativo, inclinação, borracha traseira, coalescência; WinTab fica de fora)
- [ ] 1.7B / 1.7C2 decisões do usuário (deixar registrado o motivo)
- [ ] 3.1–3.11 roteiro do curta (rig e cenas) — scripts de produção

Auditoria 2: as 8 propostas estão feitas; reler os arquivos `AUDITORIA_V2*.md` e `auditoria-v2/` para confirmar que
nada ficou de fora.

## 2. Calibração com o Clip Studio Paint

- [ ] Rodar de novo as pranchas de calibração (canetas, lápis, pincéis, aerógrafos, borrachas, decoração, misturadores)
  contra o CSP e corrigir o que destoar
- [ ] Conferir que todas as subferramentas do CSP que usamos têm equivalente e funcionam (lista por grupo)

## 3. Criação de pincéis e canetas pelo usuário

- [ ] Estudar (internet + o próprio CSP) como o CSP deixa criar qualquer caneta: categorias da Configuração de
  sub-ferramenta, pontas, dinâmicas, textura, mistura, entrada/saída, correção, etc.
- [ ] Comparar com o nosso criador (Configurações avançadas, Criar pincel) e implementar o que falta

## 4. Workspace de Modelagem

- [ ] Ver o que o ANIGO tem (código nosso) e o Blender oferece; trazer ferramentas e melhorias de interface

## 5. Revisão das workspaces de Edição, Áudio e Roteiro

- [ ] Pesquisar o que melhorar e implementar

## 6. Por último: pasta `ferramenta de Ia`

- [ ] Só depois de 1 a 5: ler toda a discussão (Claude, ChatGPT, Gemini) e implementar a ferramenta (experimental)

Ao terminar tudo: criar `sprints/.overnight-done`.
