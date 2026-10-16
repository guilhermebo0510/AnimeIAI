# Visão do produto

## 1. O que é

Um programa desktop (Windows primeiro) que reúne, num só projeto e num só grafo de dados, o trabalho de um estúdio de anime inteiro:

roteiro → personagens e settei → storyboard/e-conte → animatic → layout (ângulos, perspectiva, câmera) → genga/douga (desenho e animação) → shiage (pintura) → bijutsu (cenário, texturas) → satsuei (composição, efeitos, partículas, câmera, cor) → som (vozes, SFX, música, mix) → edição → entrega (vídeo).

Não é um clone do Clip Studio, do Photoshop, do After Effects ou do Premiere. É a junção das ferramentas mais úteis de cada um, redesenhadas para conversar entre si e para serem **operáveis por IA**: cada ação da interface é um comando do núcleo, e o mesmo comando é exposto pelo MCP.

## 2. Quem opera

- **Claude, via MCP**, faz a produção: escreve o roteiro, desenha os personagens e o model sheet, define ângulos, desenha cada quadro com traços, pinta com balde usando as cores do settei, pinta cenários com pincéis e texturas, compõe, aplica efeitos, gera vozes, monta e exporta. O modelo mental é o do Blender com MCP: olhar o alvo, agir, renderizar, comparar, corrigir.
- **Humano**, opcionalmente, faz retoques com mouse ou caneta pela mesma interface.

## 3. Princípios

1. **Tudo é desenhado.** Sem geração de imagem (regra 1). A qualidade vem de ferramentas boas e de um laço de verificação rigoroso.
2. **Vetor e raster convivem**, como no Clip Studio: partes animáveis em vetor (linha limpa editável, troca de espessura, intercalação), cenário e pintura em raster com pincéis e texturas (referência Photoshop).
3. **Corte (カット) é a unidade de trabalho**; o timesheet é estrutura de dados de primeira classe.
4. **O agente precisa de olhos**: render determinístico, captura do app, comparação com referência, métricas objetivas, overlay de diferença.
5. **Tudo é comando**: UI, MCP, CLI e testes passam pelo mesmo Command Bus; tudo tem undo e fica no log.
6. **Projeto em pasta legível** (JSON canônico + blobs por hash), versionável em git.
7. **Integração futura com o ANIGO** (3D de personagem e cenário com cel shading). O AnimeIAI nasce separado, mas o 3D de guia e o renderer reaproveitam código do ANIGO e o formato não fecha a porta para unir os dois.

## 4. Fora do escopo agora

- Multiplataforma (só Windows).
- Colaboração em rede, servidor, contas.
- APIs pagas, IA generativa de imagem.
- Leitura de formatos proprietários fechados (`.clip`, `.tvpp`, `.psd` com efeitos avançados). PSD básico (camadas raster) entra na exportação.
- DAW completa (o áudio é suficiente para diálogo, SFX e BGM simples).

## 5. Resultado esperado no fim da rodada

1. O programa compila, abre, tem interface profissional em PT/EN, claro/escuro, e todas as ferramentas do `06-MAPA-DE-FERRAMENTAS.md` marcadas como MVP funcionando.
2. Pelo menos **3 cortes do episódio de referência reproduzidos** dentro da régua do `07-PROTOCOLO-DE-VALIDACAO.md` (meta: 5).
3. Um **micro anime original de 30–60 s**, com história, personagens, cenários, animação, efeitos, cor, vozes, SFX/BGM e edição, produzido inteiramente dentro do app via MCP, exportado em MP4.
4. Relatório final com o que funciona, o que não atingiu a meta e por quê, e o que ficou para depois.

## 6. Idioma e formato do micro anime

Vozes em japonês (motor VOICEVOX, timbre mais próximo de anime entre as opções gratuitas locais) com legenda em português embutida; versão com voz em português (Kokoro/Piper) é opcional se o tempo permitir. 1920×1080, 23,976 fps, animação majoritariamente em 2s (desenho a cada 2 quadros), estéreo 48 kHz, loudness alvo -24 LKFS.
