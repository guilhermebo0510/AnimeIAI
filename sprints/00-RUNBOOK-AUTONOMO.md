# Runbook da rodada autônoma

Este documento diz **como** executar as sprints sem o usuário no meio. O usuário só valida no fim (S23). Decisões técnicas, troca de stack, de biblioteca ou de abordagem são do executor, desde que registradas em `registro/DECISOES.md`.

## 1. Autorizações dadas pelo usuário (2026-10-15)

- Rodar do começo ao fim sem pedir validação intermediária.
- Usar mouse e teclado reais da máquina para testar o app, quanto tempo for preciso.
- Abrir e fechar o app sem perguntar.
- Acessar a internet para pesquisa, documentação, engenharia reversa (estudar o funcionamento, nunca copiar código de terceiros sem licença que permita).
- Instalar repositórios, bibliotecas e ferramentas sem risco, dentro do projeto (ou ferramentas de sistema comuns: winget/choco/uv/cargo).
- Copiar código livremente de `C:\ANIGO` e `C:\smart-canvas-arquitetura` (projetos do próprio usuário).
- Git local, sem remoto.
- Mudar qualquer decisão destes documentos, inclusive o stack, se a execução mostrar que outra opção é melhor.
- Impedir o computador de dormir durante a rodada.

**Não autorizado:** IA generativa de imagem/vídeo; APIs pagas; clonar voz de pessoa real; enviar código ou dados para fora; mexer em configuração de sistema/segurança do Windows (plano de energia incluso); apagar arquivos do usuário fora de `C:\AnimeIAI`.

## 2. Laço de execução (por sprint)

```
para cada sprint em ordem (ver 00-ESTADO-ATUAL.md):
  0. ler 00-LICOES-DE-IMPLEMENTACAO.md (bloqueante) e conferir disco (df -h /c; parar builds abaixo de 8 GB)
  1. pesquisar: estado da arte da ferramenta (docs oficiais, papers, código aberto com licença compatível,
     ANIGO/Smart Canvas). Registrar achados em registro/pesquisa/<sprint>.md
  2. planejar: refinar a lista de tarefas da sprint no próprio arquivo da sprint (seção "Execução")
  3. implementar em fatias pequenas; a cada fatia:
       a. cargo check / cargo test do crate tocado (um binário de teste por vez)
       b. teste golden (render CPU de referência) quando houver pixel envolvido
       c. abrir o app com --bridge e exercitar a ferramenta PELO MCP (comando real, screenshot real)
       d. exercitar a mesma ferramenta pelo MOUSE (harness SendInput) quando ela tiver interação de UI
       e. olhar o resultado (imagem), não só o retorno da chamada
  4. prova de uso: produzir o artefato pedido no "Critério de pronto" da sprint usando só o app via MCP
  5. registrar: atualizar 00-ESTADO-ATUAL.md, commit local, novas lições em 00-LICOES, lacunas em registro/LACUNAS.md
  6. seguir para a próxima sprint
```

Uma sprint só fecha quando **todos** os itens do "Critério de pronto" têm evidência (arquivo de imagem/vídeo/relatório em `registro/evidencias/<sprint>/`). Teste unitário passando não é evidência de ferramenta funcionando.

## 2.1 Checkpoints de calibração antecipada

Para não descobrir na S21 que uma ferramenta não serve, há três ensaios curtos contra a referência no meio do caminho (régua do protocolo, nível N1, um único quadro; reprovar aqui não bloqueia a sprint, mas cada causa vira lacuna e é atacada antes de seguir):
- **Depois da S08:** um quadro do corte candidato A (personagem: linha, balde, sombra, olhos).
- **Depois da S11:** um quadro do corte candidato B (cenário: perspectiva, pintura, texturas).
- **Depois da S14:** um quadro do corte candidato C (efeitos: partículas, glow, profundidade).

Resultado de cada checkpoint em `registro/calibracao/checkpoint-<n>/`.

## 2.2 Distribuição de trabalho entre modelos (pedido do usuário, 2026-10-15)

- **Sonnet com esforço alto** (`model: "sonnet"`, `effort: "high"`) para implementações de dificuldade pequena e média: adaptar código do ANIGO/Smart Canvas, comandos e testes repetitivos, i18n, widgets de UI simples, scripts de QA, wrappers de FFmpeg/VOICEVOX, golden tests, documentação.
- **Opus** (sessão principal ou `model: "opus"`) para o que é complexo: arquitetura do núcleo, motor de pincel e rasterização vetorial, fechamento de gap, intercalação, compositor/efeitos em WGSL, timesheet, métricas, integração e depuração difícil, e todo o trabalho de desenho/calibração via MCP.
- Haiku não é usado para implementação (custo de retrabalho maior que a economia).
- A sessão principal (Opus) orquestra: escreve o briefing de cada subagente (arquivos, contrato, testes, critério de pronto), revisa o diff, roda os testes e valida visualmente. Resultado de subagente é relatório, não prova.
- **Nunca dois agentes compilando ao mesmo tempo** (8 GB de RAM, disco curto, lock do `target/`): subagentes que compilam rodam um por vez; em paralelo só tarefas sem build (pesquisa, leitura, escrita de docs/scripts Python).

## 3. Ciclo de autocorreção

Quando uma ferramenta não produz o resultado esperado:
1. Reproduzir o problema com um caso mínimo (comando MCP + imagem).
2. Pesquisar como os programas de referência resolvem (documentação, vídeos de tutorial, papers, código aberto).
3. Corrigir ou trocar a técnica. Se trocar biblioteca/abordagem, registrar em `registro/DECISOES.md` (o que, por quê, o que foi descartado e o que foi só adiado).
4. Repetir até passar. Depois de 3 tentativas falhas na mesma técnica, pesquisar de novo antes de insistir.

## 4. Estado e retomada (sobreviver a limite de sessão e a resumo de contexto)

- **Fonte da verdade do progresso:** `00-ESTADO-ATUAL.md` (sprint atual, fatia atual, próximo passo concreto, bloqueios). Atualizar antes de qualquer operação longa (build pesado, render, download) e ao fim de cada fatia.
- **Batimento:** `sprints/.heartbeat` recebe a data/hora a cada fatia concluída. Serve para a retomada automática saber se ainda há uma sessão trabalhando.
- **Commits locais frequentes** (no mínimo um por fatia concluída) com mensagem `S<nn>: <o que>`.
- **Retomada automática:** no início da rodada, criar uma tarefa agendada local (ferramenta `scheduled-tasks`) que a cada 60 min:
  1. lê `sprints/.heartbeat`; se tiver menos de 45 min, não faz nada (há sessão ativa);
  2. senão, abre uma sessão com o prompt: "Retome a rodada autônoma do AnimeIAI: leia C:\AnimeIAI\sprints\00-RUNBOOK-AUTONOMO.md e 00-ESTADO-ATUAL.md e continue do próximo passo registrado."
  3. a tarefa é apagada pela própria rodada ao concluir a S23.
- **Keep-awake:** pedir `request_keep_awake` ao app no início da rodada e renovar quando necessário.
- **Depois de um resumo de contexto:** reler este runbook, `00-ESTADO-ATUAL.md`, as lições e o arquivo da sprint corrente antes de qualquer ação.

## 5. Quando parar e chamar o usuário

Só nestes casos (fora disso, decidir e seguir):
- Disco abaixo de 5 GB sem nada seguro para limpar dentro de `C:\AnimeIAI`.
- Algo exigiria ação proibida (seção 1) ou conta/credencial.
- Hardware impede um requisito sem alternativa (registrar o que foi tentado).
- S23 concluída: apresentar.

## 6. Disciplina de evidência

- Toda afirmação de "funciona" tem um arquivo de evidência (PNG, MP4, WAV, JSON de métrica) e o comando que o gerou.
- Falha é registrada como falha, com a saída real. Nunca maquiar (regra do usuário).
- Relatórios de calibração mostram referência, resultado, mapa de diferença e números, lado a lado.

## 7. Estrutura de pastas do projeto (criada na S00)

```
C:\AnimeIAI\
  anime de referencia\     (somente leitura; fora do git)
  anime-studio-pesquisa\   (pesquisa inicial; versionada)
  sprints\                 (este planejamento + registro/ + evidências)
  app\                     (workspace Cargo: crates/, assets/, tests/)
  tools\                   (scripts de QA em Python: métricas, extração de frames; harness PowerShell)
  sidecars\                (VOICEVOX engine, modelos ONNX, FFmpeg se precisar empacotar; fora do git)
  calibracao\              (frames extraídos da referência e projetos de calibração; fora do git)
  producoes\               (projetos .aiai produzidos: micro anime etc.)
```
