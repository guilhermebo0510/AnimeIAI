# A5 — Stack técnico, linguagens, repositórios e arquiteturas candidatas

**Frente:** A5 (stack técnico) do estudo "programa único de produção de anime".
**Data da pesquisa:** 2026-10-15. Todos os números de estrelas, licenças e datas de commit abaixo foram lidos da API do GitHub (`gh api`) em 2026-10-15, salvo indicação contrária.
**Autor:** agente de pesquisa (Claude). Texto em português; fontes e URLs ao longo do texto.

---

## 0. Resumo executivo

1. **Recomendação (confiança média-alta):** *Arquitetura A — "núcleo Rust headless + canvas nativo wgpu + shell de UI web (Tauri v2/WebView2 + TypeScript)"*, com o canvas de desenho, a timeline/xsheet pesada e o compositor rodando em Rust/wgpu, e os painéis ricos em texto (roteiro, storyboard, propriedades, nós, docking) em web. Nota ponderada 85/100 contra 77 (Rust+CEF "estilo Graphite"), 74 (tudo-Rust com egui) e 60 (fork do OpenToonz com Qt). Os pesos e notas são **julgamento meu** (inferência); a tabela está na seção 7 e é recalculável.
2. **Por que não fork do OpenToonz como base do produto:** a licença é ótima (BSD-3, comercializável) e o programa é vivo (v1.8.0 em 2026-06-19, commits diários), mas é C++/Qt 5.15 (EOL), renderização de FX majoritariamente em CPU, e nada do que o usuário quer de *novo* (compositor de GPU estilo Nuke/AE, storyboard, roteiro, áudio/edição) existe nele. Mantê-lo como **referência de domínio** (xsheet, paleta indireta de cores, cleanup, tcomposer CLI) e fonte de formatos/ideias, não como base de código.
3. **Descoberta decisiva sobre o stack de UI:** bibliotecas de texto em Rust ainda **não** têm texto vertical japonês pronto (issue "Vertical text" aberta desde 2022 no cosmic-text; "East Asian line breaking rules" e "ruby" abertas no Parley). Roteiro e storyboard japoneses (縦書き, ルビ) são trivialmente suportados por CSS `writing-mode` em um webview. Isso pesa a favor de UI web para os módulos de texto.
4. **Descoberta decisiva sobre pen/latência:** o caminho de caneta do winit para Windows (0.31-beta) tem bug aberto de pressão "travada por quadro" (issue #4741, 2026-10-06). Um app de desenho profissional deve tratar `WM_POINTER` (com `GetPointerFramePenInfoHistory`) **ele mesmo**, no thread de entrada, e desenhar em swapchain nativo — não por eventos DOM. Em Linux/WebKitGTK o `pointerType` de caneta cai para "mouse" (issue tauri #10636 aberta), o que condena canvas DOM em Linux.
5. **Testabilidade por agente:** o padrão vencedor é *núcleo determinístico + renderer de referência em CPU (Vello CPU / tiny-skia) + GPU em WARP/lavapipe com comparação perceptual (nv-flip)* — é exatamente o que o repositório do wgpu e o do Vello fazem. Servidor MCP in-app + `tauri-driver`/`mcp-server-tauri` completam o laço.
6. **Maiores riscos:** (i) integração canvas nativo ↔ webview ("airspace"), (ii) churn de dependências (wgpu 22→30 em ~2 anos, Tauri 3 em alpha, winit 0.31 beta, Vello 0.x), (iii) tempo de compilação/uso de disco do Rust no ambiente do usuário (histórico documentado na memória do projeto Smart Canvas), (iv) FFmpeg/GPL/codec e (v) o escopo (7+ produtos em um). Mitigações na seção 7.
7. **Lacunas:** a cota de buscas web da sessão acabou antes de eu confirmar alguns itens (lista na seção 10). Itens não verificados estão marcados.

---

## 1. Metodologia e legenda

- **Fato verificado (F):** lido numa fonte primária nesta sessão (API do GitHub, README, doc oficial). Vem com URL.
- **Inferência (I):** conclusão minha a partir dos fatos; pode estar errada.
- **Memória (M):** conhecimento prévio do modelo, **não** reverificado nesta sessão — marcado como tal, trate como pista.
- **Confiança:** alto / médio / baixo.
- Referências à "memória do projeto do usuário" (Smart Canvas) são fatos sobre a experiência do próprio usuário com Rust+Tauri+wgpu — não são fonte externa.

---

## 2. (a) Linguagem/base do núcleo e UI

### 2.1 Linguagem do núcleo

| Opção | Prós | Contras | Veredito |
|---|---|---|---|
| **Rust** | Segurança de memória sem GC (menos crash em sessão longa); `cargo test` nativo, proptest/fuzz; ecossistema gráfico moderno (wgpu, Vello, kurbo, parley, cosmic-text, symphonia, cpal); o compilador funciona como "laço de feedback" para o agente; usuário já tem experiência Rust+Tauri+wgpu | Compilação lenta e uso de disco/memória (a memória do projeto do usuário registra: disco C: quase cheio já destruiu arquivo-fonte; `rustc` caiu por OOM no modo incremental; crates de laço pesado precisam de `opt-level 3` mesmo em build dev); crates 0.x com quebras frequentes; sem OFX/Qt nativo; poucas libs de vídeo "de alto nível" | **Recomendado** para o núcleo |
| **C++** | Ecossistema de DCC maduro (OpenToonz, Krita, Natron, OIIO, OCIO, OTIO, OpenFX, MLT); Qt nativo; Halide | Segurança de memória; build (CMake/vcpkg/Qt) é pesado para agente; sem gerenciador de pacotes único | Só se for fork de projeto C++ existente |
| **Híbrido: núcleo Rust + UI web** | Melhor produtividade para UI com agente (React/TS); docking/nós/editor de texto prontos; testes de UI com Playwright/WebDriver | Fronteira IPC/airspace; pen/latência no DOM é inferior ao nativo | **Recomendado** (variante A) |
| **C#/.NET** | Produtividade alta, Avalonia (31,7k★, MIT, push 2026-10-15), Silk.NET (5,2k★), Vortice.Windows (D3D12, 1,2k★); WPF InkCanvas no Windows | GC em laço de canvas/playback; ecossistema de gráficos 2D/vídeo/OFX menor; gráfico GPU "de baixo nível" menos testado em DCC; não pesquisei a fundo | Alternativa razoável, não escolhida (I, confiança baixa-média) |
| **Godot como host** | MIT, 118k★; motor 2D/UI completo; Pixelorama (editor de pixel art com timeline, 10,5k★, MIT) prova viabilidade para editores pequenos; GDExtension em Rust (gdext 5,3k★, MPL-2.0) | UI de docking/texto profissional/IME e controle de input de caneta são limitados; o modelo de cena do Godot não é um modelo de documento de animação; testes golden dependem do motor | Não recomendado para suíte "pro" (I, médio) |
| **Blender como host** | GPL, 20,9k★; Grease Pencil 3 (5.0/5.1), VSE, compositor, Python (`bpy`) e `blender -b` headless — **excelente controlabilidade por agente** | Modelo de dados 3D-cêntrico (keyframes, não xsheet de cels); UI própria; GPL (qualquer add-on que importe `bpy` é GPL); não é "um programa novo e rápido" | Ótimo para **protótipo de validação / referência**, não como produto (I, médio-alto) |

Fontes: <https://github.com/blender/blender> (COPYING: GPL, "não disponível sob outras licenças"); <https://developer.blender.org/docs/release_notes/5.1/grease_pencil/> (5.1: fills com buracos, API `LayerGroup.children`); <https://developer.blender.org/docs/release_notes/5.0/grease_pencil/>; memória do projeto do usuário (compilação/disco).

### 2.2 UI — comparação

| Stack de UI | Maturidade / dados verificados | Pen/latência | Docking e painéis complexos | Texto JP / IME / a11y | Agente de IA | Veredito |
|---|---|---|---|---|---|---|
| **Tauri v2 + WebView2 + TS** | Tauri 111,9k★, Apache-2.0; estável v2.12.4 (2026-10-14); **v3.0.0-alpha.5** (2026-10-14) — não usar em produção. wry 5,0k★ | WebView2 expõe Pointer Events com pressão; coalesced/predicted existem no spec, mas **não achei medições de latência de WebView2 nem confirmação de `desynchronized` no Windows atual** (baixa confiança). Windowed WebView2 envia pen direto ao processo do browser; `AllowHostInputProcessing` ainda não exposto no wry (issue #1836, 2026-09-09, com regressões abertas no WebView2) | Excelente: dockview (3,5k★, MIT), FlexLayout (MIT), golden-layout, xyflow (38,7k★, MIT) para grafo de nós | **Melhor**: CSS vertical/ruby, IME do browser, a11y do Chromium | Ótimo (React 251k★, Svelte 88k★, Solid 36k★; muito treinamento) | **Recomendado para o shell** |
| **Rust + CEF renderizando em textura wgpu ("estilo Graphite")** | Graphite Desktop: `winit` + `wgpu` + `cef` (CEF 151) + `ipc-channel`; "accelerated_paint" renderiza o frontend Svelte em texturas wgpu; usa forks próprios de winit e cef-rs | Entrada de caneta no controle do app (winit) | Idem web | Idem web | Médio: CEF/winit forkados = manutenção | Evolução futura se o "airspace" doer; alto custo hoje |
| **egui/eframe (+ egui_tiles/egui_dock)** | egui 31,5k★, Apache-2.0/MIT, **0.37.0 (2026-10-15)** — API ainda 0.x; `egui_kittest` (testes de UI via AccessKit com snapshots); `egui_tiles` (Rerun, 591★) | Nativo, via winit | OK (egui_tiles/egui_dock) | Fraco para JP vertical/IME avançado (issues de IME abertas/ fechadas no histórico); a11y via AccessKit parcial | Bom (tudo em Rust) mas churn de API | Alternativa (variante B) |
| **Slint** | 24,2k★, v1.18.1 (2026-09-21); **licença: GPLv3, Royalty-free (desktop proprietário) ou comercial** | Nativo | Sem docking nativo documentado | Média | Médio (DSL própria) | Não favorito |
| **iced / Dioxus Native (Blitz) / Floem / Xilem / Makepad** | iced 31,8k★ (0.14.0, dez/2025); Dioxus 39,4k★ mas desktop usa webview e o renderer nativo (Blitz) é **pré-alfa**; Floem 4,3k★; Xilem 5,6k★ (0.4.0 de out/2025, "experimental"); Makepad 7,2k★ | — | Imaturos para painéis complexos | Parley (Xilem/Blitz) sem JP vertical ainda | Baixo | Não recomendados hoje |
| **GPUI (Zed)** | `gpui` 0.2.2 (Apache-2.0) no monorepo do Zed (91,8k★); gpui-component (17,6k★) | Nativo | Bom (é o framework do Zed) | Média | Médio, mas Zed usa GPL/AGPL em partes — só `gpui` é Apache-2.0 | Observar (I, baixo) |
| **Qt 6 (QML/Widgets) em C++** | LGPL/GPL/comercial (Qt Community: LGPL ou GPL; comercial separado — **não se pode misturar Community e Commercial no mesmo produto**); qtbase tag v6.12.0 em rc/beta; Qt Advanced Docking System (2,5k★, LGPL-2.1), KDDockWidgets (939★) | `QTabletEvent`; Krita/OpenToonz mostram que **WinTab vs Windows Ink é fonte de bugs** — OpenToonz mantém um Qt 5.15.2 *customizado* com WinTab | Melhor docking de DCC | Boa a11y/IME; texto vertical via Qt Text/Quick (não verificado) | Médio: C++ + CMake | Alternativa (variante C) |
| **cxx-qt (Rust+Qt)** | KDAB, 1,6k★, v0.10.0 (2026-08-24), ainda 0.x | Idem Qt | Idem Qt | Idem Qt | Médio-baixo | Observar |
| **Dear ImGui** | 76,7k★, MIT; branch docking; imnodes/imgui-node-editor | Nativo | Bom | Fraco (texto/IME/a11y) | Médio | Para ferramentas internas/debug |
| **Flutter / Electron / Avalonia** | Flutter 180k★ BSD-3; Electron 123k★ MIT; Avalonia 31,7k★ MIT | Electron = mesmo Chromium em todo SO (pen consistente), custo de ~100+ MB | — | — | — | Electron = plano B se WebKitGTK/WKWebView incomodarem; Flutter/Avalonia não investigados |

Fontes principais: <https://github.com/tauri-apps/tauri> (releases, `gh api repos/tauri-apps/tauri/releases`); <https://github.com/tauri-apps/wry/issues/1836>; <https://github.com/tauri-apps/tauri/issues/10636> (pointerType → "mouse" e pressão fixa em 0.5 no WebKitGTK; relatório de 2024, ainda aberto — **F, confiança alta, só Linux**); <https://github.com/rust-windowing/winit/issues/4741>; <https://github.com/GraphiteEditor/Graphite> (`Cargo.toml` e `desktop/ui/Cargo.toml`); <https://github.com/slint-ui/slint>; <https://www.qt.io/qt-licensing>; <https://github.com/opentoonz/opentoonz/blob/master/doc/how_to_build_win.md> (Qt 5.15.2 com WinTab customizado); <https://github.com/emilk/egui/tree/master/crates/egui_kittest>; <https://github.com/dockview/dockview> (MIT no package.json; API do GitHub mostra "NOASSERTION").

### 2.3 Entrada de caneta: o que a pesquisa mostra

- **F (alto):** `winit 0.31.0-beta.1` adicionou suporte a caneta no Windows, Wayland e Web via eventos de ponteiro unificados; mas o issue **#4741** (2026-10-06, aberto) descreve que, quando um `WM_POINTER` carrega várias amostras históricas, cada `PointerMoved` herda a **mesma** pressão/inclinação/rotação do quadro atual (usa `GetPointerPenInfo` em vez de `GetPointerFramePenInfoHistory`). Para pintura: "posição suave, pressão em degraus". <https://github.com/rust-windowing/winit/releases/tag/v0.31.0-beta.1>, <https://github.com/rust-windowing/winit/issues/4741>
- **F (alto):** `octotablet` (Fuzzyzilla, 40★, MIT, push 2025-11-05) cobre Windows Ink (RealTimeStylus) e Wayland; **Pointer API do Windows e Wintab "not planned"**; macOS/Android "help needed". <https://github.com/Fuzzyzilla/octotablet>
- **F (médio):** Krita expõe ao usuário "WinTab" vs "Windows 8+ Pointer Input"; docs recomendam WinTab para a maioria dos Wacom. Há um MR (fev/2025) para trocar API em tempo de execução via interfaces do Qt6 — **não confirmei se foi mesclado**. <https://docs.krita.org/Tablet_Settings>, <https://invent.kde.org/graphics/krita/-/merge_requests/2341>
- **F (alto):** O spec de Pointer Events define `getCoalescedEvents()` (amostras brutas agregadas num `pointermove`) e `getPredictedEvents()`; `pointerrawupdate` entrega eventos com menos coalescência. <https://www.w3.org/TR/pointerevents3/>, <https://developer.mozilla.org/en-US/docs/Web/API/Element/pointerrawupdate_event>
- **F (alto):** A dica `desynchronized` de canvas (Chrome 75) busca pular a fila do compositor; "latências acima de 50 ms atrapalham a coordenação olho-mão". **Não consegui confirmar o estado atual no Windows/WebView2** (a página original diz suportar ChromeOS e Windows em 2019). <https://developer.chrome.com/blog/desynchronized>
- **I (médio):** Para latência mínima e fidelidade de pressão, o caminho é: thread de entrada nativo (WM_POINTER com histórico, ou Wintab opcional para quem exige) → buffer de amostras lock-free → renderização de traço *speculative* (cauda predita) em swapchain nativo (flip model, `desired_maximum_frame_latency = 1`), e só depois commit no documento. O wgpu documenta `desired_maximum_frame_latency` mapeando para `IDXGISwapChain2::SetMaximumFrameLatency` no DX12. <https://github.com/gfx-rs/wgpu/blob/trunk/wgpu-types/src/surface.rs> **Não há medição de latência ponta-a-ponta de nenhum desses caminhos nas fontes que li — é um spike obrigatório (S1, seção 7.7).**

---

## 3. (b) Fork vs. construir do zero

| Projeto | Licença (verificada) | Manutenção (2026-10-15) | Qualidade/arquitetura | O que se aproveita | Consequência comercial |
|---|---|---|---|---|---|
| **OpenToonz** (opentoonz/opentoonz) | **BSD-3-Clause** em `LICENSE.txt`; arquivos em `thirdparty/` e `stuff/library/mypaint brushes` têm licenças próprias | 7,8k★; v1.8.0 (2026-06-19); último commit 2026-10-15; ~229 issues abertas; 8 de 30 commits recentes no mesmo dia → ativo; mantenedores: RodneyBaker, shun-iwasawa, manongjohn... | C++/Qt 5.15 (Qt5 EOL), CMake+Boost+OpenCV; módulos: `tnzcore`, `toonzlib`, `toonzqt`, `stdfx`, `tcomposer` (render CLI), `tcleanupper`, `tconverter`, `toonzfarm`, `stopmotion`, `script*` (console de script). Poucos usos de OpenGL/FBO no código (11 resultados na busca de código) → FX majoritariamente CPU (I, médio). Tem política de desenvolvimento assistido por IA | Modelo de domínio de xsheet/níveis, paletas indiretas ("styles"), cleanup de scan, importação/exportação **XDTS** (Celsys/Toei; M — o changelog do SUSE cita a 1.4.0), plano de produção (farm), `tcomposer` como CLI, estilo de interface de studio | **Livre para uso comercial** (BSD-3: manter avisos). Libmypaint é ISC; pincéis mypaint têm licenças mistas — conferir `Licenses.txt` |
| **Tahoma2D** (tahoma2d/tahoma2d) | "Modified BSD" no README (`LICENSE.txt`); API mostra NOASSERTION | 673★; v1.6.3 (2026-09-01); ~78 commits entre mai–set/2026 | Fork do OpenToonz com stop-motion/UX; mesma base C++/Qt; tem `AI_POLICY.md` | Melhorias de UX, importação de recursos, alinhamento vetorial | Idem OpenToonz |
| **Krita** (KDE/krita) | **GPL-3.0** | 10,6k★; commits intensos (62 em set + 32 em out/2026 nos últimos 100); **Krita 5.3 (Qt5) e 6.0 (Qt6) lançados juntos em 2026-03-24**; 5.3 recomendado p/ produção; novo motor de texto; **gap-closing no Fill** | Referência de motor de pincel, tiles, "strokes queue" (serializa edições com jobs em threads; undo por cancelamento) | Padrões de arquitetura, não código (GPL incompatível com base permissiva/proprietária) | **Código GPL não pode entrar** num produto fechado; ok como leitura |
| **Blender** | GPL (v2/v3 conforme arquivo), "não disponível sob outras licenças" | 20,9k★; Grease Pencil 3 em 5.0/5.1 | Excelente headless/Python | Host de protótipo; referência de GP/fills com buracos | GPL; add-ons GPL se usarem `bpy` |
| **Synfig** | GPL-3.0 | 2,3k★; v1.5.5 (2026-03-16); ~100 commits nos últimos 12 meses | Animação vetorial com tweening/bones | Referência de tweening/vetores; não de xsheet | GPL |
| **Pencil2D** | GPL-2.0 | 1,8k★; v0.7.2 (2026-03-13); ativo (2026-10-12) | Simples, raster/vetor, timeline | Referência de UX simples | GPL |
| **Natron** (NatronGitHub) | GPL-2.0 | 5,6k★; **última release estável v2.5.0 (2022-11-26)**; pré-releases em 2023-2024; 18 commits em jul/2026 (retomada); mantenedores comunitários | Host OpenFX v1.4, motor *pull-based*, tiles, cache RAM/disco, headless, Python | Arquitetura de referência de compositor (ver seção 5) | GPL; hoje manutenção fraca → não é base |
| **Olive** | GPL-3.0 | 9,1k★; **último push 2024-12-05** (0.2 alfa parada) | Editor C++/Qt com OCIO/OIIO e render em nós | Referência | Praticamente estagnado |
| **Kdenlive / Shotcut / Flowblade / MLT** | GPL-3.0 (apps); **MLT: LGPL-2.1** | Kdenlive 5,9k★ (push 2026-10-15); Shotcut 15,4k★; MLT v7.42.0 (2026-10-03) | MLT é o motor de edição usado por todos | Motor de edição/exportação como alternativa a FFmpeg direto; formato de interchange | MLT é LGPL mas alguns módulos/plugins trazem GPL; duplicaria o timeline/compositor próprio |
| **Ardour** | GPL (NOASSERTION na API) | 5,4k★; push 2026-10-15 | DAW C++ | Referência de áudio/forma de onda | GPL |
| **Storyboarder** (Wonder Unit) | **sem licença detectada** pela API (README remete a post do autor) | 3,9k★; **último push 2024-03-17** → estagnado | Electron/JS | Fluxo de storyboard simples | Licença obscura → só referência |
| **Kitsu / Zou** (CGWire) | **AGPL-3.0** | Kitsu 734★ (push 2026-10-15), Zou 200★ | Web de gestão de produção | Integração opcional como serviço separado | AGPL: não incorporar código; ok integrar via API |
| **Graphite** (GraphiteEditor) | **Apache-2.0** | 27,6k★; alfa; desktop RC em jan/2026 (não confirmei versão atual) | Rust: grafo de nós como formato de documento; wgpu 29 + Vello 0.10 + Parley 0.9 + kurbo 0.13 + winit 0.31-beta; UI Svelte; `wgpu-executor` | **Melhor referência viva** de Rust+wgpu+Vello+nós+UI web para editor 2D; código reutilizável sob Apache-2.0 | Permissiva; mas pré-1.0 |

Fontes: <https://github.com/opentoonz/opentoonz> (LICENSE.txt, README, doc/*), <https://github.com/tahoma2d/tahoma2d>, <https://linuxiac.com/krita-5-3-and-6-0-released-with-new-text-engine-and-wayland-hdr-support/> (datas e recomendação 5.3), <https://github.com/NatronGitHub/Natron>, <https://github.com/olive-editor/olive>, <https://github.com/mltframework/mlt>, <https://github.com/wonderunit/storyboarder>, <https://github.com/cgwire/kitsu>, <https://github.com/GraphiteEditor/Graphite>.

**Avaliação do fork (I, médio-alto):**
- *Fork do OpenToonz*: dá "xsheet que funciona" no dia 1 e compatibilidade com um ecossistema real de estúdios (formatos, XDTS, Toonz raster/vetor), mas (1) a camada de UI é Qt 5.15 e a migração para Qt 6 é um projeto em si; (2) o pipeline de render é CPU-first, e o objetivo central do usuário (composição estilo AE/Nuke com efeitos de GPU) exigiria reescrever o núcleo de render; (3) testabilidade por agente é baixa (monolito com estado global, GUI Qt); (4) o escopo extra (storyboard, roteiro, áudio, edição) teria de ser construído do zero de qualquer forma. Conclusão: **melhor usar como "oráculo de domínio"** (rodar o OpenToonz/`tcomposer` para gerar casos de comparação, ler o modelo de dados) e construir núcleo novo.
- *Blender como host/fork*: ótimo para *prototipar a cena de validação* (Grease Pencil + compositor + VSE, tudo scriptável) mas mudaria o produto para "um Blender customizado", GPL.

---

## 4. (c) Motores e bibliotecas por subsistema

Legenda de maturidade: **P** produção, **E** estável com ressalvas, **X** experimental. Estrelas/licença/último push de 2026-10-15 (GitHub API), salvo nota.

### 4.1 Rasterização 2D / vetor / traço

| Biblioteca | Licença | ★ / último push | Maturidade e notas | Papel proposto |
|---|---|---|---|---|
| **Vello** (linebender) | Apache-2.0 OR MIT | 4,4k★; release **v0.11.0 (2026-10-02)**; push 2026-10-15 | README descreve 3 renderers: **Vello CPU** (o mais maduro, SIMD+multithread), **Vello GPU** (strips esparsos; backend wgpu sem compute shaders + WebGL2), **compute renderer** (experimental, exige compute). Possui `vello_tests` (snapshots comparando CPU, WGPU e WebGL) e `fuzz/` | **Renderer vetorial principal** (GPU) + **renderer de referência determinístico** (CPU) para golden |
| **kurbo** | Apache-2.0 | 1,0k★ | Curvas Bézier/geometria; base de Vello e Graphite | Geometria de traços |
| **tiny-skia** (linebender, antes RazrFalcon) | BSD-3 | 1,7k★, push 2026-08-20 | Port CPU de subconjunto do Skia; determinístico | Referência CPU simples, thumbnails, testes |
| **Skia / rust-skia** | BSD-3 / MIT | 11,0k★ / 1,8k★ | Muito maduro, backends D3D12/Vulkan; binários grandes e build pesado | Alternativa conservadora se Vello atrasar |
| **lyon** | MIT/Apache (M; API: NOASSERTION) | 2,6k★ | Tesselação CPU de caminhos | Fallback/preview de traços |
| **Blend2D** | Zlib | 2,0k★ | Rasterizador CPU C++ muito rápido | Referência CPU alternativa (C++ FFI) |
| **Pathfinder** (servo) | Apache-2.0 | 3,9k★; push 2026-04-23 | Efetivamente parado | Não usar |
| **Rive runtime** | MIT | 1,2k★ | Renderer vetorial com *pixel local storage*; foco em runtime interativo | Possível alvo de **exportação** (não core) |

Evidências: <https://github.com/linebender/vello> (README, `vello_tests`, `fuzz`), <https://github.com/linebender/vello/releases>; paper de stroke expansion na seção 9.

**I (médio):** para anime, o desenho final é majoritariamente *traço vetorial de largura variável* + *áreas chapadas* + *camadas raster* de pintura/efeitos. Recomendo **modelo híbrido** — (1) traço vetorial (centerline + amostras de pressão/velocidade → perfil de largura), (2) camada raster em tiles para pincéis/texturas, (3) preenchimentos como regiões planares com **cor por índice de paleta** (estilo Toonz).

### 4.2 Motor de pincéis

| Opção | Licença | Notas |
|---|---|---|
| **libmypaint** | ISC (texto do COPYING lido); API: NOASSERTION | 386★; **última release v1.6.1 (2020-05-11)**, push 2026-04-14; estável mas parado; usado pelo OpenToonz; `mypaint-brushes` CC0-1.0 (44★). Boa base para pincéis "naturais" |
| **Motores do Krita** | GPL-3.0 | Não reutilizável em produto proprietário; ler como referência |
| **Próprio (GPU)** | — | Carimbo (dab) em compute/raster pass com tiles; viável; dab-spacing e dinâmicas inspiradas no libmypaint |

### 4.3 Preenchimento, fechamento de gaps, auto-pintura

- **F:** Krita 5.3/6.0 trouxe *gap-closing* no Fill (linuxiac). O Grease Pencil 5.1 reformulou fills (com buracos). OpenToonz tem ferramentas de fill/"paint bucket" por região.
- **F (alto):** pesquisa recente específica de anime: *"No Pixel Left Behind: Filling Gaps in Anime Colorization"* (arXiv 2609.00800, 2026) — estudo formativo em pipelines de anime japoneses mostra que **pequenas regiões fechadas esquecidas** na colorização por balde são um problema recorrente; propõe sugestão de cor por IA. Isso orienta a UX: detecção de regiões não pintadas como *lint* do projeto.
- **Algoritmos clássicos:** LazyBrush (Sýkora et al., CGF 28(2), 2009) — <https://dcgi.fel.cvut.cz/home/sykorad/lazybrush.html> (F). Trapped-ball / "Vectorizing Cartoon Animations" (M, não verificado).
- **Auto-paint por IA (referência):** inclusion matching (arXiv 2403.18342, 2410.19424), DACoN (2509.14685), PeCA (2608.00903), AniDoc (2412.14173), LVCD (2409.12960), ToonComposer (2508.10881). Rodar via ONNX/Windows ML como recurso *opcional*.

### 4.4 Abstração de GPU, compositor e efeitos

| Opção | Licença | Notas |
|---|---|---|
| **wgpu** | Apache-2.0 | 18,3k★; **v30.0.1 (2026-08-22)**; DX12/Vulkan/Metal/GL; CI roda com **WARP (DX12 no Windows)**, **lavapipe/llvmpipe (Mesa)** e usa `nv-flip` para comparar imagens. Quebra de API a cada ~trimestre (inferência a partir do salto 22→30 relatado na memória do projeto do usuário) |
| **Dawn** | BSD-like (NOASSERTION) | 1,2k★; usado por Figma para build nativo | Alternativa C++ |
| **Vulkan/D3D12 direto, bgfx, Diligent** | — / BSD / Apache-2.0 (Diligent 4,5k★) | Mais controle, muito mais código; só vale se wgpu limitar (ex.: mesh shaders, interop de vídeo D3D11/12) |
| **naga** (shader translation) | — | Repositório gfx-rs/naga arquivado (2023-11) — **agora vive dentro do wgpu**; Figma usa naga para GLSL→WGSL |
| **Slang** | NOASSERTION | 5,7k★; push 2026-10-16 | Linguagem de shader com geração para SPIR-V/HLSL/WGSL; considerar para biblioteca de efeitos grande (I, baixo) |
| **Halide** | MIT (M; API: NOASSERTION) | 6,6k★ | Pipelines de imagem com *scheduling*; pouco usado em Rust |
| **OpenFX** | BSD-3 | 579★; **v1.5 (2024)**: GPU Rendering Suite = **Metal, CUDA, OpenCL** (sem D3D12/Vulkan) | Hospedar plugins OFX de terceiros implica CPU ou CUDA/OpenCL; **não é prioridade**. Natron implementa OFX 1.4 (referência). Preferir **API própria de efeitos em WGSL** e, no futuro, um host OFX de CPU |
| **OpenColorIO / OpenImageIO / OpenEXR** | BSD-3 / Apache-2.0 / BSD-3 | 2,1k / 2,4k / 1,9k★, todos ativos (push out/2026) | Gestão de cor ACES/ICC, I/O de imagens (PSD, EXR, TIFF…). Em Rust: `exr` (186★), `image` (5,9k★), `psd` (283★, push 2025-01) — PSD/EXR em Rust ainda são nichos; OIIO/OCIO via FFI é o caminho confiável |

Fontes: <https://github.com/gfx-rs/wgpu/blob/trunk/docs/testing.md> (testes de imagem com nv-flip), <https://github.com/gfx-rs/wgpu/blob/trunk/.github/workflows/ci.yml> (WARP/Mesa), <https://openfx.readthedocs.io/en/main/ReleaseNotes/relnotes-1.5.html>, <https://www.figma.com/blog/figma-rendering-powered-by-webgpu/>.

### 4.5 Vídeo I/O e áudio

| Item | Licença / estado | Notas |
|---|---|---|
| **FFmpeg** | LGPL-2.1+ por padrão; **GPL se habilitar libx264/x265**; `--enable-nonfree` para alguns componentes | 65,2k★ (push 2026-10-15). Decisão de empacotamento: linkar dinamicamente **build LGPL** (sem x264) e usar encoders de hardware (NVENC/AMF/QSV) + codecs nativos (ProRes, DNxHR, FFV1, etc.); ou usar FFmpeg como **processo separado** (`ffmpeg-sidecar`, 543★, MIT) para isolar licença. A doc de licenciamento do FFmpeg trata NVENC como caso especial — **verificar o LICENSE.md da versão que for empacotada** (a página que li é de espelho/commit antigo, baixa confiança) |
| **Bindings Rust** | `ffmpeg-next` = repositório **zmwangx/rust-ffmpeg** (2,0k★, WTFPL, v9.0.0 de 2026-08-05); **rsmpeg** (larksuite, 879★, MIT, **push 2025-08-24**); gstreamer-rs (GStreamer 3,3k★) | ffmpeg-next é o mais ativo; rsmpeg parado há ~14 meses |
| **GStreamer / MLT / GES** | LGPL | Úteis, mas trazem seu próprio grafo/timeline |
| **Decodificação ProRes/DNxHR** | via FFmpeg (M) | Encoders de ProRes existem no FFmpeg (`prores_ks`) (M); verificar implicações legais antes de comercializar |
| **Áudio** | cpal (3,97k★, Apache-2.0, push 2026-10-13), Symphonia (3,4k★, MPL-2.0, push 2026-08-13), rodio (2,5k★), miniaudio (7,4k★, domínio público/MIT-0 — M), dasp (1,2k★), **Rubber Band** (GPL-2.0 — evitar sem licença comercial), JUCE (AGPL-3/comercial — M) | Áudio como **relógio mestre** do playback; forma de onda e scrub de lip-sync em Rust |
| **Interchange de edição** | **OpenTimelineIO** (Apache-2.0, 2,0k★, push 2026-10-14) | Formato de troca para NLEs; não core |

Fontes: <https://ffmpeg.org/doxygen/3.4/md_LICENSE.html>, <https://github.com/zmwangx/rust-ffmpeg>, <https://github.com/larksuite/rsmpeg>, <https://github.com/RustAudio/cpal>, <https://github.com/pdeljanov/Symphonia>.

### 4.6 Texto, fontes e japonês

| Item | Estado | Implicação |
|---|---|---|
| **cosmic-text** (pop-os, 2,2k★, Apache-2.0) | Issue **"Vertical text" (#11) aberta desde 2022-10-24** | Sem texto vertical |
| **Parley** (linebender, 760★) | Issues abertas: "East Asian line breaking rules" (#301, 2025-03), "interlinear annotations (ruby)" (#255) | Sem quebra de linha CJK completa nem ruby |
| **HarfBuzz** (6,2k★) | Suporte a layout vertical (M); `rustybuzz` marcado "unmaintained" em RUSTSEC-2026-0206 (2026-07-11, dentro do relatório de `cargo audit` do Tauri) | Em Rust, olhar `harfrust` (M) |
| **Webview/CSS** | `writing-mode: vertical-rl`, `text-orientation`, ruby: suporte de browser (M, amplamente conhecido) | **Caminho mais curto** para roteiro/storyboard em japonês |

Fontes: <https://github.com/pop-os/cosmic-text/issues/11>, <https://github.com/linebender/parley/issues/301>, <https://github.com/linebender/parley/issues/255>, `gh api repos/tauri-apps/tauri/releases/tags/tauri-v3.0.0-alpha.5` (aviso RUSTSEC).

### 4.7 Persistência, undo/redo, colaboração, scripting, ML

| Tema | Opções verificadas | Recomendação (I) |
|---|---|---|
| **Formato de projeto** | serde (10,9k★), ron (4,0k★), postcard (1,5k★); **bincode arquivado**; rusqlite (4,4k★, MIT); BLAKE3 (6,5k★) para hash; zstd/lz4 | Pasta de projeto: `project.json` canônico (chaves ordenadas, determinístico) + blobs endereçados por conteúdo (BLAKE3, zstd) para frames/strokes/mídia + SQLite *opcional* só para índices/caches (nunca como fonte da verdade) → *git-friendly* e *diff-friendly* |
| **Undo/redo** | Command pattern + log de transações (WAL); Krita: strokes queue com undo por cancelamento | Um **Command Bus** único (a UI, o MCP e os testes usam o mesmo caminho) — o usuário já tem esse padrão no Smart Canvas |
| **Colaboração** | Automerge (6,7k★, MIT), Yjs (22,9k★), **Loro (6,2k★, MIT, Rust)** | Adiar; se vier, Loro/Automerge sobre o modelo de documento. Não projetar o núcleo em torno de CRDT agora |
| **Scripting/plugins** | PyO3 (16,2k★), mlua (2,9k★), wasmtime (18,7k★), extism (5,8k★, BSD-3) | Python embutido (PyO3) para pipeline/automação de estúdio; WASM (wasmtime/extism) para plugins de efeito *sandboxed* |
| **ML local** | `ort` (2,6k★, **2.0.0-rc.14**, Apache-2.0), ONNX Runtime (22,1k★, MIT), candle (21,2k★), burn (16,1k★, v0.22.0) | **DirectML está em "modo de manutenção"** (README oficial); Microsoft recomenda **Windows ML** (baseado em ONNX Runtime, EPs gerenciados pelo Windows; doc de 2026-09-01) — NPUs/GPUs otimizadas exigem Windows 11 24H2+. Para cross-platform: ORT com EPs CUDA/TensorRT/CoreML. Tratar IA (inbetweening, colorização) como **plugins opcionais**, nunca no caminho crítico |

Fontes: <https://github.com/microsoft/DirectML> (README), <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview>, <https://github.com/pykeio/ort>.

---

## 5. (d) Arquiteturas de referência e lições

| Sistema | Como separa documento / render / UI | Lição aproveitável |
|---|---|---|
| **Krita** | `KisImage` com nós; "strokes queue": `startStroke/addJob/endStroke/cancelStroke`, jobs de init/dab/finish/cancel, estratégias e dados de job separados; scheduler serializa edições e roda subtarefas em paralelo; undo só entra na pilha depois da execução; cancelar reverte a transação inteira | Modelar **cada gesto como uma transação cancelável**; undo por inverso/snapshot de tiles. Fonte: <https://docs.krita.org/en/untranslatable_pages/strokes_documentation.html>, <https://community.kde.org/Krita/Strokes_Framework> (F, alto) |
| **Natron** | Três camadas: host OpenFX, motor de render (sem GUI), GUI Qt; render *demand-driven*, por tiles, com ROI/tempo/visão; cache RAM+disco global; headless e Python; nó `DiskCache` armazena **quadros inteiros em float32** | Separação documento/render/UI ideal para teste por agente; cache por quadro em disco para pontos caros do grafo. Fonte: <https://natron.readthedocs.io/en/rb-2.6/maintainers/overview.html>, <https://natron.readthedocs.io/en/rb-2.6/maintainers/rendering.html> (F, médio-alto) |
| **Nuke** | Cache de visualização em disco por *scanlines* + buffer cache FIFO em pontos intermediários do grafo; LRU no disco | Cache em dois níveis, chaveado por hash do sub-grafo. Fonte: <https://learn.foundry.com/NUKE/content/getting_started/managing_scripts/image_caching.html> (F, médio) |
| **After Effects** | Global Performance Cache: RAM + disco persistente; v25.2 adiciona *Preview from Disk Cache* (HPPP): todo quadro renderizado vai para disco e é transmitido para RAM no playback | Playback robusto = **disco como camada L2 sempre ligada** + streaming para RAM. Fonte: <https://www.pugetsystems.com/blog/2025/02/24/upcoming-boost-to-after-effects-preview-playback-with-ram-and-disk-cache/> (F, médio) |
| **Figma** | Renderer em C++ compilado para Wasm **e** para x64/arm64 nativo (render server, **testes e debug**); WebGPU via Dawn no nativo; shaders GLSL únicos convertidos para WGSL com naga; fallback dinâmico WebGPU→WebGL; UI em TypeScript/React fora do canvas, ligada por "bindings layer"; "game engine scriptável em TypeScript" | **Núcleo único, headless, compilável para teste**; UI web fora do canvas — exatamente a Arquitetura A. Fonte: <https://www.figma.com/blog/figma-rendering-powered-by-webgpu/> (F, alto) |
| **Graphite** | Grafo de nós como documento; executor wgpu; frontend Svelte; desktop = winit+wgpu+CEF (UI em textura) | Evolução possível da A para eliminar "airspace" |
| **OpenToonz** | Cena + níveis + xsheet; `tcomposer` CLI para render; script console; farm | Modelo de dados e fluxo de estúdio japonês |
| **Blender** | DNA/RNA + depsgraph + modos (M) | Referência de dados em tempo-real; `blender -b` como oráculo |

**Lições de performance para "xsheet com milhares de cels" (I, médio):**
1. Modelo **esparso por coluna**: intervalos `(início, duração, referência_de_desenho)`, nunca um objeto por quadro; consultas por `frame → célula` em O(log n).
2. UI do xsheet **virtualizada** (só linhas/colunas visíveis) e renderizada em canvas/GPU — no webview, usar canvas 2D/WebGL para a grade (como "data grids" de canvas, p.ex. glide-data-grid, 5,4k★, MIT, mas push 2026-01) ou TanStack Virtual (7,1k★) para listas de DOM.
3. **Chave de cache = hash de conteúdo** (BLAKE3 do sub-grafo + parâmetros + versão de shader + resolução) → L1 RAM (LRU, orçamento em MB), L2 disco (tiles/quadros com zstd/lz4), proxies de meia resolução, miniaturas assíncronas.
4. **Render assíncrono priorizado em torno do cursor** (playhead), cancelável; áudio como relógio mestre, descartando quadros em vez de atrasar o som.
5. **Invalidação por dependências** (grafo de efeitos) em vez de "re-renderizar tudo".

---

## 6. (e) Testabilidade por agente

### 6.1 Pilha de testes recomendada

| Camada | Técnica | Evidência / ferramenta |
|---|---|---|
| Núcleo (modelo, comandos, undo, serialização) | `cargo test` + **proptest** (propriedades: *comando seguido de seu inverso = identidade*, *serializar→desserializar→serializar = bytes idênticos*) + **cargo-fuzz** no carregador de projeto e no codec de traço | Vello mantém `fuzz/` no repo (F) |
| Renderer vetorial | Corpus de cenas de referência renderizadas por **Vello CPU** (determinístico) e comparadas com Vello GPU/WGPU/WebGL | `vello_tests`: macro `vello_test` gera um teste de snapshot por alvo (CPU, WGPU, WASM32 WebGL) (F, alto) — copiar o padrão |
| GPU | Rodar a suíte em **WARP (D3D12 por software) no Windows** e **lavapipe/llvmpipe** no Linux; comparar com métrica perceptual (**nv-flip**) com tolerância por backend | wgpu: CI instala WARP e Mesa; testes de exemplos usam `nv-flip` (F, alto) <https://github.com/gfx-rs/wgpu/blob/trunk/docs/testing.md>. NVlabs/FLIP: 653★ |
| Exportação final | Hash determinístico apenas para o caminho CPU de referência; para GPU, tolerância; relatório visual (lado a lado + mapa de diferença) | I |
| UI web | Playwright/WebDriver (`tauri-driver` existe em `crates/tauri-driver` do repo do Tauri — F) | + MCP para Tauri: **hypothesi/mcp-server-tauri** (320★, MIT, push 2026-10-13): screenshot do webview, snapshot da árvore de acessibilidade, execução de JS, chamadas IPC, monitor de IPC, logs; plugin Rust + servidor Node por WebSocket; só Tauri v2; "pode conter bugs, foi feito com agentes" (F, médio-alto) |
| Controle do app inteiro | **Servidor MCP interno** com ferramentas = Command Bus (o usuário já tem um com 45 ferramentas e testes de cobertura no Smart Canvas — memória do projeto) | I: replicar o padrão |
| Entrada de caneta | **Gravar/reproduzir amostras brutas** `{t, x, y, pressão, tilt, rotação, pointer_id}` em JSONL; reproduzir pelo **mesmo** caminho do thread de entrada; para testes de integração real usar SendInput/Raw Input (a memória do usuário alerta que DPI, tamanho de struct e modo absoluto fazem o teste "mentir calado") | I; padrão de gravação do mesmo tipo do WAL |
| Regressão de desempenho | Benchmarks (Criterion) e *timestamp queries* de GPU; **medir sempre na mesma sessão A/B**; registrar clock/potência da GPU (a memória do usuário documenta GPU travada em P8 e bancada contaminada por outro processo) | I + memória |
| Determinismo de fluxo | Todo comando/IO passa por relógio e RNG injetáveis; nenhuma leitura de hora do sistema no núcleo | I |

### 6.2 Princípios para o agente

1. **Tudo que a UI faz é um comando** com esquema JSON; o agente usa MCP/CLI, a UI usa o mesmo barramento.
2. `animestudio-cli` headless: `render --scene X --frames a..b --out dir`, `verify-project`, `golden-update --suite`, `replay-input --file`.
3. Formato de projeto textual canônico → diffs legíveis em git (o agente vê o que mudou).
4. "Veredito" de efeito vindo do render real (round-trip), não adivinhado pela ferramenta MCP — lição documentada na memória do projeto do usuário.
5. Harness precisa esperar *todas* as texturas assíncronas antes do screenshot (também lição do usuário).

---

## 7. (f) Arquiteturas candidatas, decisão ponderada, riscos e recomendação

### 7.1 Candidata A — **Núcleo Rust + canvas/timeline nativos wgpu + UI web (Tauri v2/WebView2 + TS)** *(recomendada)*

- **Processos/camadas:** (1) `core` (crate sem UI: documento, Command Bus, undo, I/O do projeto, grafo de composição, cache); (2) `render` (wgpu + Vello/GPU; caminho CPU de referência); (3) `media` (FFmpeg dinâmico LGPL ou sidecar, áudio cpal/symphonia); (4) `app` (Tauri v2: janela principal em web; *child window* nativa com swapchain wgpu para o canvas, a timeline/xsheet densa e o preview); (5) `mcp` (servidor interno) e `cli`.
- **Entrada:** pen/mouse tratados em Rust no HWND do canvas (WM_POINTER com histórico; Wintab opcional); a UI web só recebe estado e comandos.
- **UI web:** React ou Solid + dockview + xyflow (nós) + editor de roteiro com CSS vertical/ruby + CodeMirror/Monaco para script.
- **Prós:** melhor equilíbrio de produtividade do agente × latência do canvas; reaproveita a experiência já acumulada pelo usuário; texto japonês e a11y grátis; máxima testabilidade (núcleo headless, Playwright, MCP).
- **Contras:** "airspace" (dois mundos de renderização/entrada), IPC; Tauri v3 em alfa (ficar no v2); WebView2 só Windows (multiplataforma exige WebKitGTK/WKWebView para a casca, mas o canvas é nativo, então o impacto é menor).
- **Evidências de custo do híbrido (memória do projeto do usuário):** bounds de viewport obsoletos após minimizar; comando síncrono lento no thread principal congelando a UI; fila de cena como único canal projeto→viewport. Sinal de que o híbrido funciona, mas exige disciplina.

### 7.2 Candidata B — **Tudo Rust nativo (winit + wgpu + egui/egui_tiles ou Xilem)**

- **Prós:** um único caminho de render e de entrada (sem airspace); UI testável com `egui_kittest`; binário único; ótimo para ferramentas densas (Rerun é prova de que egui aguenta apps complexos — M).
- **Contras:** API 0.x com quebras; texto japonês vertical/ruby ausente; IME e a11y menos maduros; aparência "moderna" custa trabalho manual; editor de roteiro rico e nós precisam ser escritos; menos material de treinamento para o agente que React.
- **Quando venceria:** se o texto vertical japonês não for requisito e se a UI for pequena.

### 7.3 Candidata C — **Fork do OpenToonz (C++/Qt), modernizando render**

- **Prós:** licença BSD-3; xsheet/níveis/paleta/cleanup/XDTS prontos; `tcomposer` CLI; projeto vivo; comunidade de estúdios.
- **Contras:** Qt 5.15 → Qt 6 migração; render CPU-first; estado global; escopo novo (compositor GPU, storyboard, roteiro, edição, áudio) ainda é do zero; difícil de testar por agente; C++ para o agente.
- **Variante C′:** "Rust core + Qt6 UI" (cxx-qt 0.10) — complexidade de duas toolchains, ainda 0.x.

### 7.4 Candidata D (evolução da A) — **Rust + winit + wgpu + CEF em textura (estilo Graphite)**

- Elimina o airspace e dá controle total de entrada; **custa** CEF (centenas de MB, build/forks de `cef-rs` e `winit` como o Graphite faz). Adotar apenas se o spike S2 mostrar que o airspace é inaceitável.

### 7.5 Tabela de decisão ponderada (julgamento meu; notas 1–5)

| Critério | Peso | A: Rust+wgpu+web | B: tudo Rust/egui | C: fork OpenToonz/Qt | D: Rust+CEF |
|---|---:|---:|---:|---:|---:|
| Testabilidade/controle por agente | 20 | 5 | 4 | 2 | 4 |
| Latência e qualidade do canvas | 15 | 4 | 5 | 4 | 5 |
| Produtividade do agente / velocidade de desenvolvimento | 15 | 5 | 4 | 2 | 3 |
| Escopo funcional "dia 1" (reuso) | 10 | 2 | 2 | 5 | 2 |
| UI rica (docking, texto JP, a11y, timeline) | 10 | 5 | 2 | 4 | 5 |
| Performance do compositor/playback | 10 | 4 | 4 | 2 | 4 |
| Licença/comercialização | 5 | 5 | 5 | 4 | 5 |
| Risco de limitar o futuro | 10 | 4 | 3 | 2 | 3 |
| Multiplataforma futura | 5 | 3 | 4 | 4 | 4 |
| **Total (máx. 100)** | 100 | **85** | **74** | **60** | **77** |

Sensibilidade (I): dobrar o peso de "escopo dia 1" leva C a ~69 e A a ~87 — A continua primeiro. Se "UI rica" cair para 3 e "texto JP" for removido dos requisitos, B sobe para ~78 e fica próximo de A. O critério "Risco de limitar o futuro" segue a preferência registrada do usuário de não escolher soluções que virem limitadores.

### 7.6 Riscos principais (A) e mitigações

| # | Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|---|
| R1 | Latência do traço aquém do esperado / airspace ruim (flicker, resize, DPI) | Média | Alto | Spike S1/S2 antes de qualquer feature; fallback D (CEF-em-textura) ou B para o shell |
| R2 | Churn de dependências (wgpu, Vello 0.x, winit beta, Tauri 3) | Alta | Médio | Fixar versões; camada de abstração fina (`gpu`, `input`, `ui-bridge`); atualizar em janelas planejadas (a memória do usuário mostra migração wgpu 22→30) |
| R3 | Bug de pressão no winit / divergência Wintab×Ink | Alta | Alto | Entrada própria com `GetPointerFramePenInfoHistory`; Wintab opcional; matriz de tablets de teste |
| R4 | Compilação lenta, disco cheio e OOM do rustc | Alta (histórico) | Médio | `CARGO_INCREMENTAL=0` em CI, limpar `deps\*.pdb`, `target` em disco com espaço, crates de laço pesado com `opt-level 3`, `cargo-nextest` |
| R5 | FFmpeg/GPL/codec | Média | Alto | Build LGPL dinâmico ou sidecar; sem x264 embutido; lista de codecs aprovada; revisão jurídica antes de distribuir |
| R6 | Escopo (compositor + pintura + áudio + storyboard + roteiro + edição) | Alta | Alto | Fatia vertical de validação (seção 7.7); módulos como crates com contratos estáveis; storyboard/roteiro depois do núcleo de desenho |
| R7 | Texto vertical japonês no webview ≠ no export de vídeo | Média | Baixo | Texto de roteiro fica na UI; texto no vídeo (legendas/títulos) renderizado por pipeline próprio (HarfBuzz/Skia ou rasterização via webview offscreen) |
| R8 | Dependência de WebView2 (atualização, regressões de input) | Média | Médio | Fixed-version runtime empacotado; testes de regressão de input |
| R9 | IA/ML: DirectML em manutenção, Windows ML exige 24H2+ para EPs otimizados | Média | Baixo | Plugin opcional; ORT com EPs selecionáveis |

### 7.7 Spikes recomendados (primeiros ~2–3 ciclos)

- **S1 — Latência de caneta ponta-a-ponta:** WM_POINTER com histórico → traço → swapchain (flip, latency=1), contra um canvas WebView2 com `pointerrawupdate`+`getCoalescedEvents`+`getPredictedEvents`. Medir com carimbo de tempo de entrada e câmera de alta velocidade (ou fotodiodo).
- **S2 — Airspace:** child window wgpu sob/sobre WebView2 durante resize, minimizar, DPI misto, multi-monitor.
- **S3 — Traço vetorial:** pressão→perfil de largura→Vello; comparação CPU×GPU com nv-flip.
- **S4 — Xsheet com 5.000 cels × 30 colunas:** modelo esparso + grade em canvas; scrubbing a 60 fps.
- **S5 — Cache/playback:** 1080p, 24 fps, 3 camadas + glow; L1 RAM + L2 disco; áudio mestre.
- **S6 — Exportação:** FFmpeg LGPL dinâmico + NVENC/AMF/QSV; ProRes/H.264/ H.265; verificação de licença do build.
- **S7 — Harness de agente:** CLI headless + MCP + replay de entrada + golden em WARP.

### 7.8 Recomendação final

Adotar **A**, com **D** como plano de evolução e **B** como plano de contingência para a casca de UI. Reutilizar do OpenToonz o *conhecimento de domínio* (modelo de xsheet, paletas indiretas, XDTS), e do Graphite/Vello/Figma os *padrões de engenharia* (núcleo headless, grafo de nós, golden CPU×GPU). Não forkar código GPL (Krita/Blender/Natron/Synfig/Pencil2D) para dentro do produto.

---

## 8. Repositórios-chave a estudar/reutilizar (dados verificados em 2026-10-15)

| Repo | Licença | ★ | Último push / release | Uso |
|---|---|---:|---|---|
| opentoonz/opentoonz | BSD-3-Clause | 7.828 | 2026-10-15; v1.8.0 (2026-06-19) | Domínio de xsheet, tcomposer, formatos |
| tahoma2d/tahoma2d | Modified BSD | 673 | 2026-09-24; v1.6.3 (2026-09-01) | UX de stop-motion, fork ativo |
| KDE/krita | GPL-3.0 | 10.551 | 2026-10-14; 5.3/6.0 (2026-03-24) | Arquitetura de strokes/tiles (leitura) |
| blender/blender | GPL | 20.933 | 2026-10-15 | Oráculo/protótipo (Grease Pencil, bpy) |
| synfig/synfig | GPL-3.0 | 2.314 | 2026-10-12; v1.5.5 | Tweening vetorial (leitura) |
| pencil2d/pencil | GPL-2.0 | 1.812 | 2026-10-12; v0.7.2 | UX simples (leitura) |
| NatronGitHub/Natron | GPL-2.0 | 5.583 | 2026-07-24; estável v2.5.0 (2022) | Arquitetura de compositor OFX |
| olive-editor/olive | GPL-3.0 | 9.134 | 2024-12-05 | Estagnado |
| mltframework/mlt | LGPL-2.1 | 1.863 | 2026-10-15; v7.42.0 | Motor de edição alternativo |
| KDE/kdenlive; mltframework/shotcut | GPL-3.0 | 5.870; 15.429 | 2026-10-15 | UX de edição |
| Ardour/ardour | GPL | 5.352 | 2026-10-15 | Áudio |
| wonderunit/storyboarder | (sem licença detectada) | 3.879 | 2024-03-17 | Estagnado |
| cgwire/kitsu, cgwire/zou | AGPL-3.0 | 734; 200 | 2026-10-15 | Gestão de produção (integração) |
| GraphiteEditor/Graphite | Apache-2.0 | 27.622 | 2026-10-15 | Rust+wgpu+Vello+nós+UI web |
| linebender/vello | Apache-2.0/MIT | 4.421 | 2026-10-15; v0.11.0 (2026-10-02) | Render vetorial |
| linebender/kurbo, parley, xilem | Apache-2.0 | 1.016; 760; 5.571 | 2026-10-08; 10-14; 09-14 | Geometria, texto, UI |
| linebender/tiny-skia | BSD-3 | 1.661 | 2026-08-20 | Referência CPU |
| gfx-rs/wgpu | Apache-2.0 | 18.299 | 2026-10-14; v30.0.1 | GPU |
| rust-windowing/winit | Apache-2.0 | 6.205 | 2026-10-15; 0.31.0-beta.3 | Janelas/entrada |
| tauri-apps/tauri; wry; tao | Apache-2.0 | 111.933; 5.012; 2.216 | 2026-10-15; v2.12.4 / v3.0.0-alpha.5 | Casca |
| hypothesi/mcp-server-tauri | MIT | 320 | 2026-10-13 | MCP para Tauri v2 |
| emilk/egui | Apache-2.0/MIT | 31.478 | 2026-10-15; 0.37.0 | Alternativa B |
| rerun-io/egui_tiles | Apache-2.0 | 591 | 2026-10-06 | Docking egui |
| slint-ui/slint | GPLv3/Royalty-free/comercial | 24.188 | 2026-10-15; v1.18.1 | Alternativa (licença) |
| zmwangx/rust-ffmpeg (ffmpeg-next) | WTFPL | 2.014 | 2026-10-09; v9.0.0 | Vídeo |
| nathanbabcock/ffmpeg-sidecar | MIT | 543 | 2026-10-14 | FFmpeg por processo |
| RustAudio/cpal; pdeljanov/Symphonia | Apache-2.0; MPL-2.0 | 3.972; 3.438 | 2026-10-13; 2026-08-13 | Áudio |
| AcademySoftwareFoundation/OpenColorIO, OpenImageIO, openexr, OpenTimelineIO, openfx | BSD-3 / Apache-2.0 | 2.134; 2.386; 1.868; 2.021; 579 | todos ≥ 2026-10-01 | Cor, imagem, interchange, OFX |
| mypaint/libmypaint; mypaint/mypaint-brushes | ISC; CC0-1.0 | 386; 44 | 2026-04-14 (release 2020); 2024-09-25 | Pincéis |
| microsoft/onnxruntime; pykeio/ort; tracel-ai/burn; huggingface/candle | MIT/Apache-2.0 | 22.088; 2.564; 16.113; 21.173 | 2026-10 | ML |
| bytecodealliance/wasmtime; extism/extism; PyO3/pyo3; mlua-rs/mlua | Apache-2.0 / BSD-3 / Apache-2.0 / MIT(M) | 18.725; 5.798; 16.229; 2.901 | 2026-10 | Plugins/scripting |
| loro-dev/loro; automerge/automerge | MIT | 6.220; 6.659 | 2026-10-15 | Colaboração (adiar) |
| dockview/dockview; xyflow/xyflow; TanStack/virtual | MIT | 3.471; 38.683; 7.145 | 2026-10-15 | UI web |
| KDAB/cxx-qt | (API: none) | 1.556 | 2026-09-29; v0.10.0 | Rust+Qt (observar) |

---

## 9. Papers e referências técnicas por algoritmo difícil

Todos os arXiv abaixo foram lidos (título e resumo) via busca de papers nesta sessão (F, confiança alta quanto à existência e ao tema; não li os PDFs completos).

**Renderização de traços e vetores na GPU**
- *GPU-friendly Stroke Expansion* — Levien et al., arXiv:2405.00127. Expansão de traço totalmente paralela (compute shader) com espirais de Euler; base do Vello.
- *Windfoil: Closed-Form Coverage for Real-Time and Differentiable Vector Graphics* — arXiv:2610.02468 (out/2026; muito recente, **baixa confiança de maturidade**).
- *Bézier Splatting for Fast and Differentiable Vector Graphics Rendering* — arXiv:2503.16424.
- Clássicos (M, não verificados nesta sessão): Loop & Blinn (2005) "Resolution independent curve rendering using programmable graphics hardware"; Nehab & Hoppe (2008) "Random-access rendering of general vector graphics"; Schneider (1990) ajuste de curvas Bézier (Graphics Gems); Casiez et al. (2012) "1€ Filter" para suavização de caneta.

**Colorização/preenchimento (paint bucket)**
- *LazyBrush: Flexible Painting Tool for Hand-drawn Cartoons* — Sýkora, Dingliana, Collins, CGF 28(2), 2009 (F).
- *No Pixel Left Behind: Filling Gaps in Anime Colorization* (GapFill) — arXiv:2609.00800.
- *Learning Inclusion Matching for Animation Paint Bucket Colorization* — arXiv:2403.18342; *Paint Bucket Colorization Using Anime Character Color Design Sheets* — arXiv:2410.19424.
- *DACoN* — arXiv:2509.14685; *PeCA* — arXiv:2608.00903.
- *Deep Line Art Video Colorization with a Few References* — arXiv:2003.10685.
- Geração: *AniDoc* — arXiv:2412.14173; *LVCD* — arXiv:2409.12960; *ToonComposer* — arXiv:2508.10881; *AnimeColor* — arXiv:2507.20158; *MangaDiT* — arXiv:2508.09709; *OmniColor* — arXiv:2603.27531; *Cobra* — arXiv:2504.12240.

**Inbetweening**
- *Deep Animation Video Interpolation in the Wild (AnimeInterp)* — arXiv:2104.02495.
- *Deep Geometrized Cartoon Line Inbetweening (AnimeInbet)* — arXiv:2309.16643 (grafos de vértices; trata *line art*, o caso real de produção).
- *Sketch-Aware Interpolation Network (SAIN)* — arXiv:2308.13273; *Thin-Plate Spline-based Interpolation for Animation Line Inbetweening* — arXiv:2408.09131.
- *ToonCrafter: Generative Cartoon Interpolation* — arXiv:2405.17933; *Generative Inbetweening through Frame-wise Conditions-Driven Video Generation (FCVG)* — arXiv:2412.11755.
- Clássico (M): Whited et al. (2010) "BetweenIT" — interactive inbetweening.

**Bloom/glow e efeitos de pós**
- Jimenez, *Next Generation Post Processing in Call of Duty: Advanced Warfare*, SIGGRAPH 2014 — pirâmide de downsample com média ponderada por luminância (anti-"fireflies") e upsample tent 3×3 (F, via <https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare>). Para anime, o glow costuma ser *screen/add* sobre camadas isoladas, então implementar como nó com máscara e curva de limiar (I).
- Kawase/dual-filter blur (M, não verificado).

**Arquitetura e engenharia**
- <https://www.figma.com/blog/figma-rendering-powered-by-webgpu/>; <https://docs.krita.org/en/untranslatable_pages/strokes_documentation.html>; <https://natron.readthedocs.io/en/rb-2.6/maintainers/rendering.html>.

---

## 10. Lacunas e itens não verificados

A cota de buscas web da sessão esgotou antes de eu concluir:

1. **Latência medida** de WebView2 vs. swapchain nativo, e se `desynchronized` funciona hoje no WebView2/Windows — nenhuma fonte com números; spike S1 resolve.
2. **Parley/cosmic-text vertical**: confirmei apenas issues abertas; não li o código-fonte para ver suporte parcial (ex. orientação de glifos).
3. **Status de merge do MR do Krita** (troca WinTab/WinInk via Qt6) e a qualidade real de Qt 6.12 no Windows para caneta.
4. **Licenças marcadas NOASSERTION na API** que citei de memória: lyon (MIT/Apache), Halide (MIT), HarfBuzz, miniaudio, mlua (MIT), yjs (MIT), JUCE (AGPL/comercial), Ardour, Tgfx, dssim (AGPL-3.0 já confirmado pela API).
5. **NVENC/AMF/QSV e licença do FFmpeg** — li texto de LICENSE.md de espelhos/commits antigos; confirmar na versão que for empacotada. ProRes: confirmar requisitos legais de distribuição.
6. **Estado atual do desktop do Graphite** (última versão estável do app) e da reputação do `cef-rs` com forks.
7. **OpenToonz**: afirmações sobre render CPU-first e XDTS vieram de leitura parcial do repositório e de changelog de terceiros — validar rodando `tcomposer` e inspecionando `stdfx`.
8. **Avalonia/Flutter/Electron/.NET** apenas dimensionados por estrelas/licença; sem avaliação técnica de caneta.
9. **Blender 5.2/estado atual de Grease Pencil e compositor GPU**: li notas 5.0 e 5.1; não verifiquei 5.2.
10. **Clássicos acadêmicos marcados (M)** (Loop-Blinn, Nehab-Hoppe, Schneider, 1€ Filter, BetweenIT, Kawase) não foram reconfirmados.

---

## 11. Fontes (principais)

- GitHub API (`gh api repos/<owner>/<repo>`) em 2026-10-15 para estrelas, licenças, pushes e releases dos repositórios da seção 8.
- <https://github.com/opentoonz/opentoonz> — README, LICENSE.txt, doc/how_to_build_win.md, doc/ai_assisted_development_checklist.md.
- <https://github.com/tahoma2d/tahoma2d>
- <https://linuxiac.com/krita-5-3-and-6-0-released-with-new-text-engine-and-wayland-hdr-support/>
- <https://docs.krita.org/Tablet_Settings>; <https://invent.kde.org/graphics/krita/-/merge_requests/2341>
- <https://docs.krita.org/en/untranslatable_pages/strokes_documentation.html>
- <https://developer.blender.org/docs/release_notes/5.0/grease_pencil/>; <https://developer.blender.org/docs/release_notes/5.1/grease_pencil/>
- <https://github.com/linebender/vello> (README, `vello_tests`, `fuzz`)
- <https://github.com/GraphiteEditor/Graphite> (`Cargo.toml`, `desktop/ui/Cargo.toml`)
- <https://github.com/gfx-rs/wgpu> (`docs/testing.md`, `.github/workflows/ci.yml`, `wgpu-types/src/surface.rs`)
- <https://github.com/rust-windowing/winit/issues/4741>; <https://github.com/rust-windowing/winit/releases/tag/v0.31.0-beta.1>
- <https://github.com/tauri-apps/tauri/issues/10636>; <https://github.com/tauri-apps/wry/issues/1836>
- <https://github.com/Fuzzyzilla/octotablet>
- <https://github.com/hypothesi/mcp-server-tauri>
- <https://www.figma.com/blog/figma-rendering-powered-by-webgpu/>
- <https://natron.readthedocs.io/en/rb-2.6/maintainers/overview.html>; <https://natron.readthedocs.io/en/rb-2.6/maintainers/rendering.html>
- <https://learn.foundry.com/NUKE/content/getting_started/managing_scripts/image_caching.html>
- <https://www.pugetsystems.com/blog/2025/02/24/upcoming-boost-to-after-effects-preview-playback-with-ram-and-disk-cache/>
- <https://openfx.readthedocs.io/en/main/ReleaseNotes/relnotes-1.5.html>
- <https://github.com/microsoft/DirectML>; <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/overview>
- <https://www.qt.io/qt-licensing>; <https://github.com/slint-ui/slint>
- <https://ffmpeg.org/doxygen/3.4/md_LICENSE.html> (versão antiga; ver ressalva)
- <https://developer.chrome.com/blog/desynchronized>; <https://www.w3.org/TR/pointerevents3/>
- <https://github.com/pop-os/cosmic-text/issues/11>; <https://github.com/linebender/parley/issues/301>; <https://github.com/linebender/parley/issues/255>
- <https://dcgi.fel.cvut.cz/home/sykorad/lazybrush.html>; <https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare>
- arXiv: 2405.00127, 2610.02468, 2503.16424, 2104.02495, 2309.16643, 2308.13273, 2408.09131, 2405.17933, 2412.11755, 2412.14173, 2409.12960, 2508.10881, 2507.20158, 2508.09709, 2603.27531, 2504.12240, 2609.00800, 2403.18342, 2410.19424, 2509.14685, 2608.00903, 2003.10685.
- Memória do projeto do usuário (Smart Canvas): compilação/disco, MCP interno, harness de input, goldens, fila de cena, viewport híbrido (não é fonte externa).
