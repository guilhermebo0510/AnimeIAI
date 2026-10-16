# Relatório inicial — programa único de produção de anime

Data: 2026-10-15. Fase 1 (pesquisa de stack e ferramentas). Base: 5 relatórios brutos em `pesquisa-bruta/` (A1 pipeline, A2 ferramentas 2D, A3 composição/3D/áudio/entrega, A4 pré-produção/gestão/IA, A5 stack técnico). Este documento consolida e dá a recomendação; os brutos têm as URLs e o nível de confiança de cada afirmação.

**Como confiar neste relatório.** Nada foi testado em máquina. Dados de repositório (licença, estrelas, releases, issues) foram lidos na API do GitHub em 15/10/2026 e conferi por amostragem os mais decisivos (Graphite, Vello, Kitsu, OpenToonz v1.8.0, issue do winit, issue do cosmic-text, versões do Tauri). O resto vem de leitura de páginas, em parte por resumos de modelo auxiliar. Tudo o que é julgamento (notas, prioridades, "dificuldade") está marcado como inferência nos brutos. As lacunas estão na seção 9.

---

## 1. Veredito em 10 linhas

1. **Dá para construir, mas não como "cinco programas colados".** O que prende estúdios japoneses não é falta de ferramenta de desenho (Clip Studio Paint já é bom o bastante); é o **cut** (カット) e o **timesheet** (タイムシート) circulando entre programas que não conversam. Um programa único ganha onde os programas atuais falham: ligar roteiro → storyboard → timesheet → camadas → revisão → composição → entrega num grafo só.
2. **O timesheet é o centro do produto.** Clip Studio, TVPaint e Harmony só têm linha do tempo; só o RETAS Stylos tinha folha de timing (A1, relatório METI/AJA 2023, lido no PDF). A ponte atual é o XDTS (Celsys) + app gratuito da Toei, com adoção parcial. Modelar timesheet como estrutura de dados de primeira classe é a decisão de arquitetura mais valiosa.
3. **O mercado está aberto, mas não vazio.** RETAS está sem manutenção, CACANi parado desde 2019, Adobe Animate em manutenção. Mas a Sony já prototipa o **AnimeCanvas** com A-1/CloverWorks (timesheet embutido, sensação de papel, camadas vermelhas de correção, travas por etapa). Os requisitos públicos dele são uma boa especificação de referência.
4. **O maior risco técnico é o escopo, não a tecnologia.** São 7+ produtos (pintura, xsheet, compositor, storyboard, roteiro, áudio, edição). Precisa ser feito em fatias verticais, com o núcleo de desenho + timesheet primeiro.
5. **Stack recomendado (confiança média-alta):** núcleo Rust headless + canvas/xsheet/preview nativos em wgpu + casca de UI web (Tauri v2 + TypeScript) + FFmpeg + servidor MCP interno. Detalhes e alternativas na seção 6.
6. **Dois fatos mudam a escolha de UI**: (a) winit 0.31-beta tem bug aberto de pressão da caneta "travada por quadro" no Windows (issue #4741, aberta 06/10/2026), então a caneta tem de ser tratada por código próprio; (b) as bibliotecas de texto em Rust não têm texto vertical japonês nem ruby (cosmic-text, issue aberta desde 2022; Parley idem), e CSS num webview tem. Roteiro e storyboard em japonês ficam melhores em UI web.
7. **Não forkar OpenToonz.** Licença boa (BSD-3 pelo LICENSE.txt) e projeto vivo (v1.8.0, 19/06/2026), mas é C++/Qt 5.15, render CPU-first e difícil de testar por agente. Vale como **oráculo de domínio** (xsheet, paleta indireta, XDTS). Krita, Blender, Natron, Synfig, Pencil2D são GPL: só para ler.
8. **Testabilidade por agente é requisito de arquitetura**, não detalhe: núcleo headless, projeto textual determinístico, Command Bus único (UI, CLI, MCP e testes usam o mesmo caminho), renderer de referência em CPU para golden images, replay de strokes. Isto casa com o fluxo implementar→testar→validar→corrigir que você quer.
9. **IA entra como assistente opcional e rastreável**, nunca no caminho crítico. O setor japonês reage forte (CODA, AJA, caso WIT) e há risco de direito autoral por saída gerada. Isso vira diferencial de produto (política de IA por projeto, proveniência por cel, tabela de licenças de modelo).
10. **O que mais falta saber** (seção 9): spec oficial do XDTS, sensação real de pincel vs. latência (spike), entrevistas com animadores, e som/edição/entrega do lado japonês.

---

## 2. O pipeline real (A1) — o que o software precisa refletir

**Unidade de trabalho = cut.** Um episódio de TV de 30 min tem ~300 cuts. No papel, o cut viaja num saco (カット袋) com layout, genga, douga e timesheet, controlado por tabelas (カット表, 進行表, 集計表). No digital mais avançado documentado (studio MOTHER), o saco virou **uma pasta com dois arquivos: `.clip` + `.tdts`**, e a ordem das camadas no `.clip` já carrega o fluxo de aprovação (BG, LO, LOEN = checagem da direção, LOSA = checagem do diretor de animação, G = genga).

**Ordem das etapas** (mantida no digital): planejamento → composição da série/roteiro → settei (design, cor) → e-conte/vídeo-conte → layout → genga (+ 2ª genga, 作監) → douga (+ verificação) → shiage/cor → fundos (美術) → efeitos/3DCG → satsuei (composição) → edição → som (afureko → MA) → entrega.

**Fatos que condicionam o produto** (A1, confiança alta salvo nota):
- **Digitalização é parcial.** De 15 empresas de desenho 2D consultadas pelo METI/AJA, só 4 contratantes, 1 estúdio e 2 subcontratadas tinham o desenho 100% digital por dentro. Checagem digital pela direção é "extremamente rara". Muito freelancer ainda trabalha em papel. A mistura papel+digital foi estimada em ~2,2 milhões de ienes/mês de custo de conversão por episódio contínuo.
- **Sem estúdio que use uma ferramenta só**: desenho em CSP/RETAS/TVPaint/Harmony, shiage em RETAS PaintMan/CSP/OpenToonz, satsuei em After Effects, 3D em 3ds Max/Maya/Blender, storyboard em Storyboard Pro/CSP, gestão em ShotGrid ou sistema próprio.
- **Escala**: ~4.000 desenhos coloridos por episódio; 300–400 GB por episódio na CloverWorks; fundos de 200–300 MB cada.
- **Resistência real ao digital**: veteranos querem "sensação de papel". Ferramenta nova tem de preservar essa sensação (mesma conclusão do AnimeCanvas).
- **Revisão** (grafo de checagem): layout → LOEN → LOSA; genga → correção do 作監 → direção → 2ª genga → 総作監; douga → 動画検査; shiage → 色指定検査; satsuei → direção checa em vídeo → retake. O objeto de cada aprovação é o cut.

**Gargalos ordenados por força de evidência** (A1 §7), que viram o argumento de valor do programa:
1. Mistura papel+digital (scan com alinhamento por pino, correção de folha, impressão sem perder vínculo com o cut).
2. Timesheet desligado das ferramentas de desenho.
3. Checagem de diretor/作監 fora das ferramentas (não há software para o 演出).
4. Specs por empresa/obra/etapa (resolução, camadas, nomes), aplicadas à mão.
5. Gestão de produção fragmentada.
6. Shiage com ferramenta em fim de vida (PaintMan).
7. Híbrido 2D×3D (o 3D não segue o timesheet).
8. Dados pesados e render no fim do cronograma.

---

## 3. Ferramentas existentes → o que recriar

### 3.1 Desenho, animação e pintura (A2)

Estado em 15/10/2026: Clip Studio Paint 5.0 (11/03/2026), Harmony 25, TVPaint 12.x (12.1 traz Timesheet com import/export XDTS), OpenToonz 1.8.0, Tahoma2D 1.6.3, Krita 5.3/6.0 (fill com Close Gaps), Blender 5.2.2 LTS (Grease Pencil com fills com buracos), SAI2 ainda em preview técnico, RETAS 6.6.0 sem suporte, CACANi parado, Animate em manutenção.

**Conjunto mínimo (A2 §7, prioridade inferida, a validar com animadores)** — sem isto nenhum estúdio migra:
- **P0**: pincel com sensação de nível CSP/SAI/TVPaint (pressão, inclinação, baixa latência, estabilização configurável) em canvas HD/4K sem lag; linha limpa raster **e** vetor com largura variável editável; xsheet/timeline com exposição por referência, onion skin, light table, áudio com scrub e waveform; câmera 2D com keyframes; import/export sem perda de timing (**PSD, sequências PNG/TGA/TIFF/EXR, XDTS**, vídeo); tinta-e-pinta com fechamento de gap e paletas nomeadas reutilizáveis.
- **P1**: camadas de arte linha/cor separadas, "paint behind"; transformação/warp por quadro e em lote; biblioteca de assets; undo ilimitado e autosave; composição básica com EXR; papéis de quadro (key/breakdown/inbetween).
- **P2**: fluxo de equipe e versionamento; rigging cut-out com deformadores; grafo de nós/FX; scripting (Python); cleanup de scan (ainda útil para parte do mercado e para ex-usuários de RETAS).
- **P3** (diferenciais, não bloqueiam): auto-intercalação e auto-paint propagado (pesquisa aberta; nenhuma solução geral em produção), 3D de referência, câmera multiplano 3D/USD, IA opcional.

**O mais difícil de reproduzir (dificuldade 4–5, inferência de A2):** a *sensação* do motor de pincel; vetor com largura variável editável por ponto (o formato TVG da Harmony nem foi decodificado completamente pela comunidade); tinta-e-pinta com camadas de arte e paleta indireta (Toonz Raster, Harmony); fechamento de gap robusto; auto-paint e auto-inbetween entre quadros (ainda artigos de pesquisa em ago/2026).

**Interoperabilidade** (A2 §3): todos os formatos nativos são fechados (`.clip` = SQLite sem spec, `.tvpp`, `.tvg`, `.sai2`). Os elos abertos são **XDTS** (Celsys; CSP, TVPaint, OpenToonz, Toei, Callipeg), **OCA** (formato aberto da comunidade, JSON + PNG/EXR), PSD, sequências e USD. Conclusão: importar/exportar pelos formatos abertos; **não** tentar ler os nativos como requisito.

### 3.2 Composição/satsuei, fundos, 3D, cor, áudio (A3)

- **Composição** (padrão: After Effects, camadas + plugins). Os plugins japoneses (PSOFT CelMX/CelFX, OLM, Pencil+ 4 Line) existem porque cels são **cor chapada sem anti-aliasing**, e blur/glow genéricos sujam a cor. Logo **seleção por cor** e **blur consciente de cor** têm de ser nativos. A3 §2.4 traz uma tabela de 29 efeitos com algoritmo e custo de GPU (blur gaussiano separável, bokeh, glow em pirâmide, difusão, halation, 透過光, light wrap/rim light, aberração, grão, deband, flare, god rays, shake e retime em 1s/2s/3s, parallax/câmera USD, partículas, etc.). Quase todos são fragment shaders de poucas passadas; o custo real é o **número de camadas** (50–200 por cut) em float16 a 1080p–4K.
- **Cor**: AE 26.5 e Blender 5.0 já em ACES. Recomendação inferida: OpenColorIO, float16 de trabalho, EXR, ACES opcional sobre o fluxo sRGB/Rec.709 típico do anime.
- **Fundos**: Photoshop é o padrão; Ghibli ainda pinta em guache e digitaliza; tendência forte de **3D → layout → pintura** (OLM, ufotable, MAPPA, Quunplant com Blender). PSD é a troca. O maior gargalo do Sol Levante foi render de planos com pans maiores que 4K.
- **3D toon**: receita documentada do Guilty Gear Xrd (vertex color como offset de limiar, luz por personagem, normais editadas à mão, inverted hull com largura por vértice, linhas por UV, animação sem interpolação). Estúdios tratam as linhas como passe ajustável na composição. FX: ruído binarizado + posterização temporal para parecer desenhado.
- **Entrega**: ARIB TR-B32 alvo -24 LKFS, pico -1 dBTP; Netflix -27 LKFS dialog-gated, -2 dBTP, 5.1, IMF App 2E, teste PSE obrigatório para animação (tolerâncias divergem entre fontes; conferir). Animação em 24 fps, normalmente 1s/2s/3s.
- **Áudio — MVP**: timeline com waveform ligada à imagem, medidor BS.1770 com presets ARIB/Netflix/EBU, lip-sync assistido (Rhubarb A–H,X preenche a coluna de boca do timesheet), mixdown estéreo/5.1, justapor cuts com export EDL/OTIO. **Não construir DAW**: delegar por AAF/OTIO/stems.

### 3.3 Pré-produção, gestão, formatos e IA (A4)

- **Roteiro/e-conte**: o e-conte japonês é uma tabela (nº do corte | imagem | conteúdo | fala/SE | segundos), sem formato padrão; Storyboard Pro 25.1 tem o "Ember" (IA que gera cenas a partir de script); Storyboarder está parado desde 2024. A pesquisa não achou formato real de honpon de estúdio (só guias de concurso).
- **Gestão**: Kitsu (AGPL-3.0, vivo, unidade = *shot*, não *cut*, sem uso japonês achado), AYON (launcher Apache-2.0), Prism (LGPL-3.0). Perforce é grátis até 5 usuários. OpenTimelineIO 0.18.1 (Apache-2.0). Não existe adaptador XDTS⇄OTIO.
- **Modelo interno proposto** (A4 §2.5): Projeto → Temporada → Episódio → Parte → Cena → **Corte** (metadados, vínculos com roteiro/storyboard/áudio, **timesheet**, camadas/cels, estado por etapa com retakes). Assets compartilhados (settei, fundos, bancos de cor, LUTs). Histórico imutável por corte/etapa com autoria humana vs. IA.
- **IA em 2026**: abertos e úteis ToonComposer (MIT, ~57 GB de VRAM), AniDoc (Apache, mas a base SVD tem licença restritiva), AniSora/Wan (Apache-2.0); ToonCrafter sem licença clara; LVCD/AnimeInbet sem licença; interpolação genérica (RIFE) destrói o ritmo de "twos". Windows ML/ONNX Runtime é o caminho de IA no Windows (DirectML em manutenção).
- **Risco social e legal**: CODA exigiu opt-in à OpenAI (27/10/2025); AJA + editoras com declaração conjunta; WIT admitiu fundos gerados por IA (2026); nos EUA saída 100% gerada não é protegível (certiorari negado em Thaler, 02/03/2026); AI Act Art. 50 em vigor desde 02/08/2026 (datas do Omnibus conflitam). Daí a recomendação de design: IA desligada por padrão, política de IA por projeto, proveniência por cel, tabela de licenças de modelo, proveniência guardada **no projeto** (C2PA se perde em re-encode).

---

## 4. Lacunas de mercado = onde o produto pode ser melhor

(Ausência na pesquisa significa "não achei", não "não existe".)
1. Grafo único roteiro ↔ corte ↔ camadas ↔ timesheet ↔ revisão ↔ entrega.
2. Timesheet de primeira classe, com XDTS completo (cuidado com índice de quadro 0 vs 1) e adaptador XDTS⇄OTIO.
3. Settei versionado e vinculado ao corte, com alerta de mudança.
4. Gestão de retakes com comparação lado a lado.
5. Revisão do diretor/作監 dentro da ferramenta (camadas vermelhas, travas por etapa, registro de aprovação).
6. Painel de licenças e proveniência de IA.
7. Suporte nativo a texto vertical japonês e formato de e-conte estruturado.
8. Rastreador de produção local-first embutido para estúdio pequeno.
9. Entrega em pacote aberto (OTIO + XDTS + EXR + OCIO + proveniência), sem aprisionar o estúdio.

---

## 5. Arquitetura-alvo do produto (módulos)

| Módulo | Conteúdo | Dependência principal | Fase sugerida |
|---|---|---|---|
| **Núcleo** | Documento (projeto→episódio→corte→camada/cel), timesheet, Command Bus, undo, projeto em pasta + JSON canônico + blobs BLAKE3, cache de frames | — | 1 |
| **Desenho** | Pincéis, stroke vetorial de largura variável, camadas raster em tiles, onion skin, light table, fill com gap | núcleo, wgpu/Vello | 1 |
| **Xsheet/Timeline** | Exposição por referência, trilhas, câmera, áudio, scrub, playback com cache RAM/disco | núcleo, áudio | 1 |
| **Tinta-e-pinta** | Paletas nomeadas, estilos por índice, paint behind, lint de regiões não pintadas | desenho | 2 |
| **Composição** | Grafo de nós GPU com biblioteca de efeitos anime, câmera 2D/multiplano, OCIO/EXR | wgpu | 2 |
| **Exportação/mídia** | FFmpeg (LGPL), sequências, PSD, XDTS, OTIO/EDL, ProRes/H.264 | FFmpeg | 1–2 |
| **Pré-produção** | Roteiro estruturado (incluindo vertical), e-conte, animatic, settei | UI web | 3 |
| **Revisão/produção** | Etapas, aprovação, retakes, tracker local | núcleo | 3 |
| **Áudio** | Waveform, loudness, lip-sync, mixdown | cpal/symphonia | 3 |
| **Rigging/3D/IA** | Cut-out, import USD, plugins de IA opcionais | tudo acima | 4 |

---

## 6. Stack: linguagens, repositórios e caminhos (A5)

### 6.1 Recomendação

**Arquitetura A — núcleo Rust headless + canvas/xsheet/preview nativos em wgpu + casca de UI web (Tauri v2/WebView2 + TypeScript) + FFmpeg + servidor MCP/CLI.**
- **Núcleo** (`core`): documento, Command Bus, undo, I/O, grafo de composição, cache. Sem UI. Testável por `cargo test`, proptest e cargo-fuzz.
- **Render**: wgpu 30.x + **Vello** (GPU) com **Vello CPU como renderer de referência determinístico**; kurbo para geometria; modelo híbrido (traço vetorial com perfil de largura + camada raster em tiles + preenchimento por índice de paleta).
- **Entrada de caneta**: código próprio com `WM_POINTER` e `GetPointerFramePenInfoHistory` no thread de entrada (por causa do bug do winit), Wintab opcional; traço renderizado em swapchain nativo (flip, latência 1).
- **UI web**: dockview (docking), xyflow (nós), editor de roteiro com CSS `writing-mode`/ruby; xsheet densa em canvas nativo (não DOM).
- **Mídia**: FFmpeg como biblioteca LGPL dinâmica ou sidecar; cpal + symphonia para áudio; OpenColorIO/OpenImageIO/OpenEXR por FFI.
- **Controle por agente**: servidor MCP interno + CLI (`render`, `verify-project`, `golden-update`, `replay-input`), `tauri-driver`/mcp-server-tauri para a UI.

**Notas ponderadas do agente A5** (julgamento dele, tabela recalculável em A5 §7.5): A 85, Rust+CEF "estilo Graphite" 77, tudo-Rust/egui 74, fork OpenToonz/Qt 60. Trate como ordem de grandeza, não medição: os fatores decisivos são os do item 6 do veredito e a testabilidade.

### 6.2 Alternativas descartadas ou adiadas

| Alternativa | Por que não agora |
|---|---|
| Fork do OpenToonz (C++/Qt) | Qt 5.15 (EOL), render CPU-first, estado global, difícil de testar por agente; o escopo novo teria de ser feito do zero. Usar como oráculo |
| Tudo Rust (egui/Xilem/Slint) | API 0.x com quebras, sem texto vertical/ruby, IME e acessibilidade menos maduros; Slint com licença GPLv3/comercial. É o plano de contingência da casca |
| Rust + CEF renderizando em textura wgpu (estilo Graphite) | Resolve o "airspace" (canvas nativo × webview), mas custa CEF e forks de `cef-rs`/winit. É a **evolução** se o spike S2 falhar |
| Blender como host | Ótimo para protótipo/validação (scriptável, `blender -b`), mas GPL, 3D-cêntrico, não é um programa novo |
| Godot / Flutter / Avalonia / C# | Não atendem docking/texto/caneta profissional como o resto, ou não foram avaliados a fundo |

### 6.3 Repositórios-chave (dados de 15/10/2026)

| Repo | Licença | Uso |
|---|---|---|
| GraphiteEditor/Graphite | Apache-2.0 (27,6k★) | **Melhor referência viva**: Rust + wgpu + Vello + nós + UI web. Código reutilizável |
| linebender/vello (+ kurbo, parley) | Apache-2.0/MIT, v0.11.0 (02/10/2026) | Render vetorial; `vello_tests` com snapshots CPU×GPU e fuzz |
| gfx-rs/wgpu | Apache-2.0, v30.0.1 | GPU; CI com WARP/lavapipe e nv-flip (padrão a copiar) |
| tauri-apps/tauri | Apache-2.0, v2.12.4 estável (v3 em alfa: não usar) | Casca |
| opentoonz/opentoonz | BSD-3 (LICENSE.txt), v1.8.0 | Oráculo de domínio: xsheet, paletas, XDTS, `tcomposer` CLI |
| mypaint/libmypaint | ISC (última release 2020) | Base/inspiração de pincel |
| AcademySoftwareFoundation: OpenTimelineIO, OpenColorIO, OpenImageIO, openexr, openfx | Apache-2.0/BSD-3 | Intercâmbio, cor, imagem (OpenFX 1.5 só tem GPU Metal/CUDA/OpenCL: efeitos próprios em WGSL, host OFX só CPU e depois) |
| zmwangx/rust-ffmpeg, nathanbabcock/ffmpeg-sidecar | WTFPL / MIT | Vídeo |
| Krita, Blender, Natron, Synfig, Pencil2D, Olive, Kdenlive | GPL | **Só leitura** (não incorporar código) |
| cgwire/kitsu | AGPL-3.0 | Integração opcional por API, nunca incorporar |

### 6.4 Riscos (A5 §7.6) e como tratar

R1 latência/airspace → spikes S1/S2 antes de qualquer feature; R2 churn de dependências (wgpu, Vello 0.x, winit beta) → fixar versões e camada fina de abstração; R3 pressão da caneta → entrada própria + matriz de tablets; **R4 compilação/disco do Rust neste ambiente** (histórico seu: disco C: quase cheio, `rustc` com OOM no incremental) → `CARGO_INCREMENTAL=0`, `target` em disco com folga, crates de laço pesado com `opt-level 3`; R5 FFmpeg/GPL/codec → build LGPL ou sidecar, sem x264 embutido, revisão jurídica antes de distribuir; R6 escopo → fatias verticais; R8 WebView2 → runtime de versão fixa empacotado.

### 6.5 Spikes de decisão (primeiros ciclos)

S1 latência de caneta ponta a ponta (WM_POINTER nativo vs. canvas WebView2) • S2 airspace (child window wgpu sob WebView2 em resize/minimizar/DPI) • S3 traço vetorial pressão→largura→Vello, CPU×GPU com nv-flip • S4 xsheet 5.000 cels × 30 colunas a 60 fps • S5 cache/playback 1080p 24 fps com 3 camadas + glow, áudio mestre • S6 exportação FFmpeg com NVENC/AMF/QSV e verificação de licença • S7 harness de agente (CLI + MCP + replay de input + golden em WARP).

---

## 7. Como o ciclo "implementar → testar → validar → corrigir" funciona neste projeto

1. **Tudo é comando.** A UI, o CLI, o MCP e os testes passam pelo mesmo Command Bus, com esquema JSON e relógio/RNG injetáveis.
2. **Projeto textual canônico** (chaves ordenadas) + blobs por conteúdo: diffs legíveis em git; o agente vê o que mudou.
3. **Golden images**: cenas de referência renderizadas por Vello CPU (hash determinístico); caminho GPU comparado por métrica perceptual (nv-flip) com tolerância por backend; relatório lado a lado com mapa de diferença.
4. **Replay de caneta**: amostras brutas `{t,x,y,pressão,tilt,...}` em JSONL reproduzidas pelo mesmo caminho do thread de entrada (a sua experiência com SendInput/Raw Input aplica-se aqui).
5. **Veredito vem do render real**, não da ferramenta MCP (lição que você já registrou no Smart Canvas).
6. **Você valida pontos de qualidade subjetiva** (sensação do pincel, look de composição, ritmo de animação): o agente gera A/B lado a lado e você aprova. Média por métrica não aprova imagem.

---

## 8. Estratégia de validação final (recriar um anime)

Proposta para discutirmos, não decisão tomada:
- **Escopo da validação**: 3 a 5 cuts, não um episódio. Um cut de ação (genga + douga em twos, câmera com parallax, glow/halation), um de diálogo (lip-sync, timing do timesheet, áudio), um de fundo 3D→pintado com câmera, um de efeitos (FX 2D-look), e a montagem final com áudio e entrega em ProRes/H.264 com loudness medido.
- **Régua objetiva**: o cut precisa ser **produzido de ponta a ponta dentro do programa** (roteiro → conte → timesheet → desenho → pintura → composição → áudio → vídeo), exportar XDTS e abrir no app da Toei/CSP/OpenToonz sem perda de timing, e passar nos medidores de entrega.
- **Material de origem**: para não depender de direito autoral, sugiro cenas **originais ou de domínio/licença livre** como alvo de validação; se você quiser recriar cenas de obras existentes, isso é uso privado de teste e tem de ficar fora do repositório distribuído. É uma decisão sua (seção 10).

---

## 9. Lacunas e o que a pesquisa NÃO confirmou

- **Spec oficial do XDTS** (exige formulário de cadastro na Celsys) e **spec do OCA**: não lidas. Estrutura (linha de cabeçalho + JSON com `header/version/timeTables/fields/tracks/frames`) inferida do código do OpenToonz; armadilha do índice 0/1.
- **Estrutura física do timesheet** (colunas, 144 quadros por folha de 6 s) e o termo "tsuke": não confirmados em fonte aberta.
- **Som, edição e entrega no lado japonês**: confiança baixa (A1). Spec de entrega da Crunchyroll não é pública; tolerância do TR-B32 diverge (±1 vs ±2 LU; versão 1.7 de 09/10/2026 a conferir).
- **Estúdios com pouca informação**: Madhouse, Wit, Shaft, Sunrise/BNF (software de desenho), Kyoto Animation (software), Trigger/Science SARU/Polygon (pipelines técnicos).
- **Preços**: Harmony, Storyboard Pro, TVPaint, CACANi, Moho, Live2D não confirmados em loja oficial.
- **Sem dados independentes** de market share e sem opinião de animadores profissionais além de listicles/blogs. As prioridades P0–P3 são inferência a validar em entrevistas.
- **Latência real** de WebView2 vs. nativo e `desynchronized` no Windows: sem medição (spike S1).
- **Licenças citadas de memória** (NOASSERTION na API) e implicações de NVENC/ProRes no FFmpeg a confirmar na versão empacotada.
- **Tauri 3, winit 0.31, Vello 0.x e wgpu** mudam rápido; versões acima valem para 15/10/2026.
- Várias fontes são PR de fabricante ou conteúdo patrocinado (marcado nos brutos). Alguns números de IA e de vídeo generativo vêm de blogs de marketing (confiança baixa).

---

## 10. Decisões que preciso de você

1. **Stack**: aceitar a Arquitetura A (Rust + wgpu + shell web Tauri v2) como hipótese de trabalho, **sujeita aos spikes S1/S2**, com D (CEF em textura) e B (tudo-Rust) como planos B? Ou prefere eu tentar derrubar a recomendação antes?
2. **Plataforma**: Windows primeiro e só depois multiplataforma? (A recomendação assume que sim; o canvas nativo reduz o custo de portar.)
3. **Idioma e mercado-alvo**: japonês nativo desde o início (texto vertical, e-conte, XDTS) ou internacional com japonês como módulo? Isso muda a prioridade de roteiro/conte e a validação.
4. **Fonte da especificação XDTS**: o cadastro na Celsys (aceitar termos) é uma ação sua; ou eu sigo pelo código aberto do OpenToonz/Toei e valido com arquivos reais que você tiver.
5. **Escopo da validação** (seção 8): material original/livre ou cenas de obras existentes (uso privado)?
6. **Política de IA**: aprovo "desligada por padrão, política por projeto, proveniência por cel", ou você quer IA como um dos pilares desde o início?
7. **Licença do produto**: fechado/comercial permissivo (o que exclui código GPL) ou aberto? Já determina FFmpeg, Qt e o que posso reaproveitar.

---

## 11. Próximos passos propostos (nesta ordem)

1. Você decide a seção 10 (principalmente 1, 3 e 7).
2. **Material de referência** (o que você mencionou): ADR de stack + ADR do modelo de dados (corte/timesheet/projeto) + roteiro de macro-sprints no formato que você já usa nas trilhas + lições de implementação obrigatórias (disco, compilação, harness).
3. **Spikes S1–S7** como primeira sprint, com relatório de números reais antes de qualquer feature.
4. Entrevistas curtas com 3–5 animadores (as prioridades P0–P3 são hipótese).
5. Fatia vertical 1: núcleo + canvas + xsheet com um cut desenhado e exportado em vídeo, com golden tests.

Os cinco brutos em `pesquisa-bruta/` guardam as URLs e as tabelas completas (matriz de features, 29 efeitos com algoritmo, ~25 papers).
