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
