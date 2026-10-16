# A2 — Inventário de features das ferramentas de desenho/animação 2D

Data da pesquisa: 2026-10-15. Frente A2 da Fase 1 (pesquisa) do projeto "programa único de produção de anime".
Escopo: Clip Studio Paint EX, PaintTool SAI, TVPaint, Toon Boom Harmony/Storyboard Pro, OpenToonz/Tahoma2D, RETAS Studio, CACANi, Krita, Blender Grease Pencil, Synfig, Pencil2D, Moho, Adobe Animate, Procreate Dreams, Live2D.

## 0. Como ler este documento

Legenda de confiança:
- **[alto]**: lido em página oficial do fabricante, release notes, API do GitHub ou documentação técnica, nesta rodada.
- **[médio]**: fonte secundária reputada (CG Channel, Wikipedia, imprensa especializada), ou fonte única, ou fonte oficial com ambiguidade.
- **[baixo]**: agregador, blog, ou memória do pesquisador sem fonte lida nesta rodada.

Marcadores de natureza:
- **FATO**: afirmação com fonte citada.
- **INFERÊNCIA**: conclusão minha a partir dos fatos. Não é verificada.
- **CONHECIMENTO PRÉVIO (não verificado)**: feature que conheço do mercado, mas cuja página oficial não consegui ler nesta rodada. Deve ser revalidada antes de virar requisito.

Limitações honestas desta rodada:
- As ferramentas de fetch devolvem resumos de um modelo auxiliar, não a página crua. Preços e datas podem ter sofrido distorção. Onde duas fontes divergem, registro as duas.
- Páginas oficiais da TVPaint (store), CACANi, Live2D (comparativo FREE/PRO), manual do Blender (seção de modificadores) e manual do Krita (animação) não foram lidas (404, truncamento ou 403). Esses itens aparecem como lacunas na seção 9.
- Nenhuma medição própria foi feita (não instalei nem testei os programas). Pontos fortes/fracos são o que fontes relatam, não benchmark.

---

## 1. Resumo executivo

1. **Estado do mercado em out/2026 [alto, ver seções por programa]:** Clip Studio Paint está na Ver. 5.0 (lançada em 11/03/2026, patch 5.0.4); Harmony na 25 (jun/2025, nenhum "26" encontrado); TVPaint na 12.x (12.0 em abr/2024, 12.1.0 com Timesheet/XDTS); OpenToonz 1.8.0 (19/06/2026, primeira versão estável em ~3 anos); Tahoma2D 1.6.3 (01/09/2026); Krita 5.3/6.0 (05/02/2026); Blender 5.2.2 LTS (15/09/2026); Moho 14.5; Pencil2D 0.7.2 (26/03/2026); Synfig 1.5.5 (dev, mar/2026); Procreate Dreams 2.1; Live2D Cubism 5.3.x. SAI Ver.2 ainda em *technical preview*. RETAS e CACANi estão parados (RETAS 6.6.0 sem data; CACANi 2.0 de 2019).
2. **Não existe um único programa que cubra tudo.** O fluxo japonês real (inferência baseada em fontes secundárias) mistura: desenho/animação em CSP (ou RETAS legado, TVPaint) + folha de exposição digital (XDTS/TDTS da Toei) + composição em After Effects/outros. Harmony domina o fluxo ocidental de TV/cut-out. Isso importa porque o formato de troca que o estúdio japonês exige (XDTS, PSD/sequência por cel) é parte do "conjunto mínimo".
3. **O que mais pesa tecnicamente [inferência, apoiada em fontes]:** (a) motor de pincel com sensação "de papel" + estabilização de baixa latência; (b) vetor com espessura variável editável por ponto; (c) tinta-e-pinta automático robusto (fechamento de gap, preenchimento propagado entre quadros); (d) timeline/xsheet com reaproveitamento de cels (exposição por referência, não por cópia); (e) interoperabilidade (XDTS, PSD por cel, OCA, EXR) — formatos nativos de todos os grandes são proprietários e sem especificação pública.
4. **Interoperabilidade aberta existente:** XDTS (Celsys, 2019, "aberto" mas especificação só mediante formulário), OCA (JSON + PNG/EXR, comunidade RxLaboratory), PSD, sequências PNG/TGA/TIFF/EXR. Nenhum formato nativo (.clip, .tvpp, .xstage/.tvg, .sai2, .pli/.tlv, .moho, .moc3) tem especificação aberta oficial. Os que têm leitores comunitários são .clip (SQLite embutido) e .xstage (XML legível).
5. **Janela de oportunidade [inferência]:** Adobe Animate está em modo de manutenção (sem features novas); RETAS morto; CACANi parado; OpenToonz renascendo mas com UI datada. Harmony é caro e fechado; TVPaint é raster-only. Ninguém combina de verdade: pincel de nível CSP/SAI + xsheet de nível Harmony/TVPaint + tinta-e-pinta com auto-paint + XDTS nativo.

---

## 2. Perfis por programa

### 2.1 Clip Studio Paint EX (Celsys)

**Versão/estado [alto]:** Ver. 5.0 lançada em 11/03/2026 (anúncio em jan/2026); ~400 novidades e melhorias; patch 5.0.4 citado pela CG Channel. Celsys declarou vendas acima de 150% da meta. Fontes: https://www.celsys.com/en/topic/20260311 ; https://www.cgchannel.com/2026/05/celsys-releases-clip-studio-paint-5/ ; https://www.celsys.com/en/topic/2026032502
**Plataformas [alto]:** Windows 10+, macOS 14+, iPadOS/iOS 18+, Android 13+ (CG Channel), mais Galaxy e ChromeOS segundo https://www.clipstudio.net/en/features/ .
**Licença/preço [alto, com divergência de granularidade]:**
- Perpétua (Win/Mac): PRO US$63,00; EX US$277,00 (Celsys e CG Channel concordam). Upgrades da 4.0: PRO US$28,99; EX US$82,99; PRO→EX US$242,99 (https://www.celsys.com/en/topic/20260311). Atualizações de recursos exigem "Update Pass" anual para donos de perpétua (https://www.clipstudio.net/en/purchase/ ; Wikipedia descreve o modelo desde 2023: https://en.wikipedia.org/wiki/Clip_Studio_Paint).
- Assinatura desktop (CG Channel): PRO US$4,49/mês ou 26,99/ano; EX US$8,99/mês ou 76,99/ano. A página de comparação do fabricante cita "EX a partir de US$2,49/mês" (provável preço de plano mobile), e "PRO a partir de US$0,99/mês" (https://clipstudio.net/en/animation/apps-software). Preços variam por número de dispositivos.
- Diferença PRO x EX para animação: PRO limitado a animação simples de até 24 quadros; EX tem ferramentas de animação "de longa duração" (https://www.clipstudio.net/en/promotion/trial_tour/animation/ ; confirmado também em https://tips.clip-studio.com/en-us/articles/2047, fonte secundária de tips). EX também tem páginas múltiplas/livro, 3D "converter em linhas e tons" etc.

**Animação (EX) [alto]** — fontes: https://help.clip-studio.com/en-us/manual_en/600_animation/Animating_in_Clip_Studio_Paint.htm (via busca), https://tips.clip-studio.com/en-us/articles/9951 , https://help.clip-studio.com/en-us/manual_en/600_animation/Export_animation.htm
- Paleta **Timeline** com **trilhas**; a pasta de animação (animation cel folder) no painel de camadas é uma "trilha" na timeline; cada camada/pasta dentro dela pode receber cels atribuídos na trilha. Um arquivo novo só tem o cel "1" ocupando o clipe inteiro; cels são colocados com "Specify cels" (exposição por referência).
- **Pasta de câmera 2D** (2D camera folder): trilha especial com keyframes (posição, rotação, zoom, opacidade) e interpolação (ex.: "Smooth" para pans). Elementos que devem se mover com a câmera vão dentro da pasta.
- **Onion skin** (cels antes/depois, cor e quantidade configuráveis) e **light table** (camada de referência registrada na linha de baixo da paleta Animation cels) para intercalação.
- Áudio como camada, *audio scrubbing*, importação de gravação como camada de áudio inclusive em mobile (https://digitalproduction.com/2025/01/07/celsys-announces-clip-studio-paint-4-0-a-feature-rich-update/ via busca). A 4.0 adicionou cor por etiqueta para camadas e cels na timeline e melhor nomeação de arquivos exportados; a 5.0 adicionou seleção em lote e edição de rótulos de trilha (https://help.clip-studio.com/en-us/manual_en/030_new/).
- **Simple Mode** ganhou animação na 5.0 (https://www.celsys.com/en/topic/20260311).
- Limitações relatadas (fonte secundária, [médio-baixo]): pouca interpolação de keyframes de desenho; timeline "linear" onde reordenar quadros é incômodo; onion skin básico; sem rigging cut-out de verdade (apenas hierarquia de camadas/pais); efeitos limitados. Fonte: https://clipstudio.net/en/animation/apps-software (a própria Celsys admite que "pode não oferecer todos os recursos avançados de software dedicado") e roundups tipo https://darkskiesfilm.com/?p=815 [baixo].

**Exportação [alto]** (https://help.clip-studio.com/en-us/manual_en/600_animation/Export_animation.htm):
- Sequência de imagens: BMP, JPEG, PNG, WebP, TIFF, Targa; opções de rascunho, perfil ICC, área de desenho, aplicar efeitos de câmera 2D, intervalo e fps.
- GIF animado (256 cores, dithering, transparência), APNG, WebP animado, filme (AVI/MP4 no Windows; MOV/MP4 em Mac/iPad).
- **Exportar cels de animação (EX)** em BMP/JPEG/PNG/TIFF/TGA, com opção **"Export for PaintMan"** (compatibilidade RETAS — ainda existe em 2026) e cor de linha.
- **Exportar cena OpenToonz** (Windows/macOS; exige OpenToonz instalado).
- **Folha de exposição** em CSV (só leitura, não reabre) ou **XDTS** (https://help.clip-studio.com/en-us/manual_en/600_animation/Importing_and_exporting_a_timeline_(EX).htm ; importa XDTS via Arquivo > Importar > Folha de exposição).
- Áudio WAV/OGG.
- OCA e PSD-por-cel não aparecem nessas páginas (OCA tem importador comunitário para CSP segundo RxLaboratory: https://rxlaboratorio.org/?p=893 [médio]).

**Vetor [alto]** (https://tips.clip-studio.com/en-us/articles/7506 e vizinhos, via busca): camada vetorial; linha = caminho + pontos de controle; ferramenta **Correct Line** (Y): editar pontos, "pinch", "simplify", "connect" (fecha gap), **correct line width** (engrossar/afinar por ponto ou linha inteira), "redraw vector width" com pressão; borracha vetorial com 3 modos (parte tocada / até a interseção / linha inteira). Limite: fill/gradient não funcionam *na* camada vetorial (a tinta é feita em camada raster abaixo); borracha raster comum não remove pontos vetoriais. A 4.0 adicionou "object snapping" (vetores/réguas/texto alinham a grades, guias, bordas) (https://clipstudio.net/en/news/202503/12_01 via busca).
**Estabilização/correção [alto]:** parâmetros de estabilização e "correction" por ferramenta, com custo de CPU proporcional (https://support.clip-studio.com/en-us/faq/articles/20200030 e https://tips.clip-studio.com/en-us/articles/17052). Pressão: assistente de calibração (https://www.clipstudio.net/en/characterart/tool/brushes.html). Dinâmica: pressão, inclinação, velocidade, aleatório (via busca; velocidade chegou em 2025 para assinantes). Na 5.0: opção de "stroke preview" para reduzir lag, anti-aliasing com opções de velocidade/qualidade, mistura "Running color" com qualidade de blend (https://help.clip-studio.com/en-us/manual_en/030_new/) e **Smart Shape** (segure a caneta no fim do traço para endireitar/arredondar; não suaviza durante o desenho) (https://www.cgchannel.com/2026/05/celsys-releases-clip-studio-paint-5/).
**Réguas/perspectiva [médio]:** réguas de perspectiva e simetria, Puppet Warp, Liquify, mesh transform (https://www.clipstudio.net/en/features/). Detalhes de ponto de fuga/grade: CONHECIMENTO PRÉVIO (não verificado).
**3D [alto]:** importar modelos 3D, figura de desenho 3D, mocap como pose (4.0), **modelo de mão 3D** com 7 presets e Hand Scanner (webcam), PBR glTF/VRM, névoa por altura, pintar em materiais 3D, agrupar modelos, converter 3D em linhas e tons (EX) (https://help.clip-studio.com/en-us/manual_en/030_new/ ; https://www.cgchannel.com/2026/05/celsys-releases-clip-studio-paint-5/). Companheiro: Clip Studio Modeler (Wikipedia).
**Tons/screentone, assets [médio/CONHECIMENTO PRÉVIO]:** tons, balões, painéis, páginas múltiplas (EX), Clip Studio Assets (loja/biblioteca de materiais) — a página oficial confirma tons, painéis e balões (https://www.clipstudio.net/en/features/); detalhes de linhas-por-polegada e conversão de gradiente em tom não foram lidos.
**Formato .clip [alto]:** introduzido na 1.5.4 (março/2016), substituiu .lip e é onde a animação frame-a-frame foi adicionada (Wikipedia + fileinfo). Registro IANA tem número mágico `CSFCHUNK`, afirma que **não é padrão aberto** e não publica especificação (https://ftp.iana.org/assignments/media-types/image/vnd.clip). Leitores comunitários: `clipparse` (C++17, lê o SQLite embutido, pixels em blocos de 256x256 descomprimidos sob demanda, round-trip byte-idêntico testado na 5.0.4) https://pypi.org/project/clipparse/ ; `clipstudio.js`; `unclip` (.NET). [médio]
**Uso na indústria [médio]:** a Wikipedia japonesa lista Toei Animation, Nippon Animation, Production I.G, Kamikaze Douga, Colorido, entre outros, como usuários do CSP em produção (via busca em https://ja.wikipedia.org/wiki/2D%E3%82%A2%E3%83%8B%E3%83%A1%E3%83%BC%E3%82%B7%E3%83%A7%E3%83%B3%E5%88%B6%E4%BD%9C%E3%82%BD%E3%83%95%E3%83%88%E4%B8%80%E8%A6%A7); a Celsys posiciona o CSP como sucessor do RETAS. Não achei pesquisa independente de market share [baixo para "dominante"].
**Pontos fortes (relatados):** motor de pincel e ferramentas de desenho, multiplataforma incluindo tablet/celular, preço baixo, ecossistema de materiais (https://clipstudio.net/en/animation/apps-software).
**Pontos fracos (relatados):** ver "Limitações" acima; sem cut-out/rigging real; interpolação de desenho inexistente.
**Mais difícil de reproduzir:** (1) o motor de pincel + estabilização + correção com baixa latência; (2) vetor com espessura editável por ponto + borracha vetorial + "Correct Line"; (3) o ecossistema de assets e o formato .clip (compatibilidade); (4) 3D de referência integrado. [INFERÊNCIA]

### 2.2 PaintTool SAI (Systemax) — v1 e v2

**Estado [alto/médio]:** a página oficial (https://www.systemax.jp/en/sai/) cita um "stable technical preview" de 2026-07-02 e uma "major renovated technical preview" de 2026-10-11, e diz que a **Ver.2 será upgrade gratuito** para licenciados da v1. Wikipedia lista "Beta 3 / 25/09/2026" (https://en.wikipedia.org/wiki/PaintTool_SAI) — divergência de data não resolvida. Ou seja, **SAI2 ainda não saiu como versão final** em 15/out/2026.
**Licença/preço [alto]:** v1: ¥5.500 com impostos, licença cobre 1.x.x; só Windows (XP a 11, 32-bit); "não suporta monitor de alto DPI"; exige digitalizador Wintab com pressão.
**Ferramentas/o que o torna querido [médio]:** estabilizador de traço configurável (S-1..S-7 ou 1–15 conforme fonte), pen vetorial com edição de pontos e **pressão por ponto**, camada "Linework", mistura de cores (binário/aquarela etc. — CONHECIMENTO PRÉVIO não verificado), baixa latência. Fonte parcial: wiki da comunidade https://painttoolsai.readthedocs.io/en/latest/ — SAI2 trouxe camadas nativas de Texto, Shape e Linework com edição de caminho, réguas de perspectiva com 1–3 pontos de fuga como objetos de camada, canvas até 100.000 x 100.000 px em 64-bit, formato `.sai`/`.sai2`, importa/exporta PSD. [médio, baseado em preview de 2024-11-23]
**Mais difícil de reproduzir:** a *sensação* do estabilizador e do pincel (latência e curva de pressão) — é um ativo de ajuste fino, não de algoritmo exótico. [INFERÊNCIA]
**Animação:** nenhuma. É ferramenta de ilustração/linha.

### 2.3 TVPaint Animation (TVPaint Développement, França)

**Versão/estado [alto/médio]:** TVPaint Animation 12 lançada em 28/04/2024 (Wikipedia: https://en.wikipedia.org/wiki/TVPaint_Animation). Changelogs listam 12.0.1 a 12.0.6 e **12.1.0** (https://doc.tvpaint.com/docs/changelogs/12-1-0 — sem data publicada). Nenhum "12.5" ou "13" encontrado. Roadmap 2026+ citado em palestra RADI-RAF (dez/2025): TimeSheet, SDK, Python, nova base de código; fim do dongle de licença (https://3dvf.com/35-ans-de-tvpaint-lhistoire-dun-outil-incontournable-et-son-avenir/ — só títulos de capítulo) [médio].
**Edições/preço [médio]:** Standard (animação 2D: interface modular, pastas de camadas, pincéis custom, light table) e Professional (+ storyboard, camada Puppet bitmap, câmera 2D, colorização CTG, biblioteca de imagens, export de sequência) (https://www.tvpaint.com/). Compra única (perpétua). Preços de varejo (abr/2026, fonte de revendedor francês): ~€650 Standard e ~€1.600 Professional, sem impostos — [baixo-médio], a página oficial não lista preços.
**Plataformas [alto]:** Windows 10/11 (sem ARM), macOS Big Sur–Sequoia (Intel) e até Tahoe (Apple Silicon), Linux Ubuntu 22.04/24.04 (X11), e Android (a partir de 12.0.1) (https://www.tvpaint.com/ ; https://doc.tvpaint.com/docs/changelogs/12-0-1).
**Novidades da 12.0 [alto]** (https://tvpaint.com/en/blog/tvpaint-animation-12-nouveautes): interface modular com painéis destacáveis; **pastas de camadas**; algoritmo de nitidez em transformações; **Dual Papers** (pincéis multitexturados); pincel de seleção; **câmera 2D baseada em keys** com curva de timing na timeline; **bitmap rigging** (marionetes + interpolação automática entre poses) — primeira vez no TVPaint; TVPaint Converter com versão cliente-servidor.
**Novidades da 12.1.0 [alto]:** Painel **Timesheet** (layouts japonês e ocidental, memo, ondas de áudio, colunas de câmera; papéis de quadro Keyframe/Breakdown/Inbetween/Special Inbetween; **exporta PDF e XDTS, importa XDTS**); novo painel de busca; **SDK em GitHub**; pre/post behavior em pastas; loop com range; tipos de movimento de câmera "japoneses"; biblioteca com sons; rigging (salvar/importar rigs completos, seletor de poses, constraints de osso, converter Puppet em Animation layer); export Apple ProRes oficial; comandos George novos (`tv_TimesheetExportXDTS`, `tv_LayerRoleSet/Get`, `tv_TransformIntoCTG`...).
**Ferramentas/conceitos [médio-alto]:** raster puro (bitmap), por camada/quadro; camadas de animação com "pre/post behavior" (loop, ping-pong, hold); light table com out-of-pegs; **Scan Cleaner** (modos soft/strong, remove brancos/cinzas) (https://doc.tvpaint.com/docs/import-export/handmade-drawings/clean-up via busca); **CTG Layers** (Colors & Textures Generator, só Pro): pintura por "squiggles" com **Speedfill** — a cor se espalha até os contornos; "o maior squiggle vence"; squiggles de exclusão para evitar vazamento; copiar squiggles entre quadros; gaps remanescentes exigem correção manual (https://doc.tvpaint.com/docs/colorize-texturize/colorize-with-ctg-layers/application-methods); gradientes por opções de preenchimento; paletas por camada CTG; FX stack; storyboard (Pro). **George** é a linguagem de script (comandos `tv_*` aparecem nos changelogs).
**Formatos [médio]:** projeto `.tvpp` (atual), `.tvp` (legado pré-9.5), importa `.tvpp/.tvp/.mir/.aura/.psd` (tutorial Elephorm); exporta MOV/MP4/AVI/GIF e sequências PNG/JPG/TIF (**sequência só na Professional**); PSD com camadas pela aba Layer; CSV+PNG-folder (o Krita notes que só o TVPaint exporta isso) (https://doc.tvpaint.com/docs/import-export/export-files/overview ; https://docs.krita.org/en/general_concepts/file_formats/file_csv.html). `.tvpp` não tem especificação pública (nenhuma encontrada) [médio].
**Textura analógica:** o apelo central — papel/pincel "dual paper", pincéis custom com grão; relatado como "um dos melhores para textura, caráter de linha e timing de desenho à mão" (https://www.startanimating.com/blog/2d-animation-software-comparison e https://studioliddell.com/news/best-2d-animation-software-top-picks) [baixo-médio, listicles].
**Produções [alto]:** Song of the Sea, Wolfwalkers, Red Turtle, Kitbull, Samurai Jack T5, Primal, Undone, Inside Out 2 (dois personagens) (Wikipedia).
**Pontos fracos:** raster-only (sem vetor), rigging bitmap só chegou na 12, preço, curva de aprendizado ("pensa em keys e inbetweens, não em rig") [baixo-médio].
**Mais difícil de reproduzir:** motor de pincel com papel/textura + a latência de pintura bitmap em resoluções altas; CTG (preenchimento propagado robusto); a nitidez de transformações repetidas em bitmap (reamostragem sem degradação). [INFERÊNCIA]

### 2.4 Toon Boom Harmony (Essentials/Advanced/Premium) e Storyboard Pro

**Versão/estado [alto]:** Harmony 25 e Storyboard Pro 25 lançados em jun/2025 (MIFA/Annecy). Nenhum "Harmony 26" encontrado em busca. Fontes: https://docs.toonboom.com/help/harmony-25/essentials/release-notes/harmony/harmony-25-release-notes.html ; https://cartoonbrew.com/sponsored-by-toon-boom/with-harmony-25-toon-boom-animation-introduces-helpful-tools-across-the-2d-animation-pipeline-247741.html.
**Novidades da 25 [alto]:** **Breakdown Pose Assistant** (Premium; controla a que key o breakdown se inclina); importação de **pincéis ABR do Photoshop** (vetor ou bitmap, também em borracha); **USD** (importar/exportar/round-trip de cenas); **Compact File Structure** (desenhos e paletas num único `.vdb` por cena; cenas não abrem em versões antigas; carga até ~3x mais rápida segundo a Toon Boom); lock do timeline ruler; anti-aliasing OpenGL com modo Performance; presets com extensões `.tbr/.tpr/.tbx`; EXR com compressão DWAA; scripting (`Timeline::layerEnabled`). **Ember** (suite opt-in de IA, licença extra): beta anunciado com Script Breakdown, geração de máscaras, Image Eraser, Expand Image, Increase Resolution e trilha de áudio rough (https://www.toonboom.com/?p=18179 via busca) — nenhuma menção a inbetween ou lip-sync por IA [médio].
**Preço [divergência, médio]:** a página da loja (https://shop.toonboom.com/en/products/harmony) mostrou, numa leitura, Essentials US$23/mês, US$181/ano, US$517/3 anos; Advanced US$55/mês, US$424/ano, US$1.209/3 anos; Premium US$100/mês, US$813/ano, US$2.316/3 anos; estudante: US$4,50 / 8,00 / 14,25 por mês. Outra leitura de https://shop.toonboom.com/en/subscriptions/harmony-advanced via busca trouxe valores maiores (Essentials US$30/mês, Advanced US$77, Premium US$139). **Não resolvi a divergência**; a loja provavelmente mostra preço regional/promocional diferente. Assinatura apenas (sem perpétua nova).
**Diferença entre planos [alto]** (https://shop.toonboom.com/en/products/harmony):
- *Essentials:* desenho vetorial, câmera até HD/2K, 7 camadas de efeito, pincel com pressão, onion skin, ink & paint, hierarquia de camadas, **Bone deformers**, câmera **multiplano** (eixo Z).
- *Advanced:* + desenho bitmap, scan e vetorização avançados, câmera 4K, 12 efeitos, **Shift & Trace**, **linhas de lápis com largura variável**, pincéis/lápis vetoriais com textura, multiusuário, render em lote.
- *Premium:* + câmera 8K+, todos os efeitos/nós, **partículas**, **OpenFX**, **Node View** (rigging e FX avançados), **deformers de curva/malha**, **Master Controllers**, rotação 3D (X/Y), importar/manipular modelos 3D, composição *deep image*, **SDK C++**.
**Ferramentas de desenho [alto]** (https://docs.toonboom.com/help/harmony-24/essentials/drawing/about-drawing-tool.html via busca): Select, **Cutter**, Reposition All Drawings, **Contour Editor** (pontos/Bézier e deformação do contorno vetorial), **Centerline Editor**, Smooth Editor, Perspective, Envelope, Edit Gradient/Texture, Brush, Pencil, Eraser, Text, Line/Rect/Ellipse/Polyline, **Paint, Paint Unpainted, Unpaint, Stroke, Close Gap**, Eyedropper. Contour Editor só funciona em vetor; em camada bitmap várias ferramentas ficam inativas.
**Tinta-e-pinta [alto]:** Paint (preenche tudo), Paint Unpainted (só zonas vazias), Unpaint; **Close Gap** na ferramenta Paint de 0 a 10 px (presets: 0, 2, 4, 8) — o helpcenter recomenda começar pelo menor para evitar traços indesejados; Close Gap manual (reta) e **Clean Up > Close Gaps** em lote (https://docs.toonboom.com/help/harmony-24/essentials/reference/tool-properties/paint-tool-properties.html ; https://helpcentre.toonboom.com/hc/en-ca/articles/48839955334547-How-do-I-close-a-vector-shape). **Camadas de arte** por desenho: Line Art e Colour Art (padrão; a cor fica sob a linha, permitindo repintar linhas), mais Underlay e Overlay (modo avançado em Preferences) (https://docs.toonboom.com/help/harmony-22/premium/paperless-animation/about-art-layer.html). Paletas/estilos com cores nomeadas ligadas (padrão de mercado; CONHECIMENTO PRÉVIO sobre "palette styles" não lido nesta rodada, mas o fluxo é descrito na trilha de aprendizado https://learn.toonboom.com/modules/colour-styling/...).
**Rigging cut-out [alto]** (https://docs.toonboom.com/help/harmony-25/premium/getting-started/deformation.html): 6 tipos de deformador — Bone, Game Bone, Curve, Envelope, Free Form, Shape-Aware; ferramenta Rigging cria Bone/Curve automaticamente; Node View para cadeias complexas (Premium); drawing substitution, auto lip-sync por fonemas (https://docs.toonboom.com/help/harmony-22/premium/cut-out-animation/animate-lip-sync.html); Attribute Controllers (nó novo). 
**Xsheet/Timeline/Node View:** Harmony tem Timeline e Xsheet (CONHECIMENTO PRÉVIO), e Node View (grafo de composição/rig).
**Formatos/interop [médio]:** cena `.xstage` (XML legível, em pasta com `elements/`, `palette-library`, `stage`...) — drawings e paletas ficam em arquivos ligados (https://helpcentre.toonboom.com/hc/en-ca/articles/48953146993555-About-saving-options-in-Harmony ; https://fileinfo.com/extension/xstage); desenho vetorial `.tvg` **proprietário e não documentado** — decodificador comunitário em https://github.com/Alex313031/tvg-dev admite engenharia reversa parcial (largura de traço só parcialmente); utilitários oficiais `utransform` e `pix2vec` convertem TVG <-> bitmap; export PSD layout; export EXR/PNG/TGA/MOV etc. via nó Write; USD. **XDTS/OCA: não encontrei suporte nativo documentado** — só conversores de terceiros (https://wolfinabowl.itch.io/oca-to-xdts-converter) [médio].
**Storyboard Pro 25 [médio]:** thumbnailing rápido, efeitos não destrutivos (pilha de efeitos), pincéis Photoshop, arquivo único compacto, animatic com câmera/3D (USD), export para cenas Harmony. Preço divergente (US$87/mês e US$752/ano na loja; US$75/mês e US$588/ano no lançamento, via Kidscreen) (https://shop.toonboom.com/en/products/storyboard-pro ; https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-release-notes.html).
**Pontos fortes (relatados):** padrão de produção de TV/filme ocidental; cut-out/rigging mais sofisticado; reutilização de arte; pipeline (batch render, multiusuário, SDK) (https://www.startanimating.com/blog/2d-animation-software-comparison [baixo]).
**Pontos fracos:** preço, curva de aprendizado, desktop-only, formato fechado; no Japão adoção em estúdio não confirmada (fonte japonesa de blog: "individuais usam, estúdios não está claro") [baixo].
**Mais difícil de reproduzir:** (1) o grafo de nós com deformadores encadeados (Node View) e o núcleo de deformação com "shape-aware"; (2) vetor com **largura variável** + pincéis vetoriais texturizados; (3) tinta-e-pinta com regiões fechadas automaticamente por camada de cor separada; (4) escala de produção (banco de dados Control Center/multiusuário). [INFERÊNCIA]

### 2.5 OpenToonz e Tahoma2D

**OpenToonz estado [alto]:** v1.8.0 lançada em **19/06/2026** (API do GitHub); anterior v1.7.0/1.7.1 em maio/2023 (primeiro estável em ~3 anos). Repositório: ~7,8 mil estrelas, 892 forks, 229 issues abertas, último push 15/10/2026 (https://api.github.com/repos/opentoonz/opentoonz). Licença marcada como "Other/NOASSERTION" no GitHub; Wikipedia diz BSD (https://en.wikipedia.org/wiki/OpenToonz). Código aberto em mar/2016 pela Dwango, em cooperação com Digital Video (Toonz) e Studio Ghibli; a Digital Video vende o "Toonz Premium".
**Novidades 1.8 [alto]** (https://github.com/opentoonz/opentoonz/releases): melhorias em Assistant, Brush, Paintbrush, Fill, Finger, Geometric, Tape; **replicação de traço** no Brush; **Screentones**; navegação de sub-xsheet; **Smoother Fx Iwa**, fx "naru lazy brush", cage draggable no corner pin; **importação OCA e integração XDTS**; Custom Panel Editor; ferramentas de navegação no viewer; Windows Thumbnail Provider. 1.7: painéis custom, Preproduction Board, render em ponto flutuante/espaço linear, **export OCA**, MyPaint brushes, etc.
**Capacidades [alto]** (manual https://opentoonz.readthedocs.io/en/latest/): **Xsheet** (exposição, sub-xsheet, trilhas de som, lip-sync, memos), onion skin, shift and trace, level strip; **Plastic tool** (malha + esqueleto, rigidez, hooks/vértices); **Skeleton tool** (cut-out com IK); **FX schematic** (macro FX, partículas); **cleanup de scan** (autocentragem por furos de peg, processamento de linha, redimensionamento) e GTS externo; **stage schematic** (pegbars), **function editor** (curvas/planilha); paletas e estilos (paleta de estúdio, paletas animadas); **Toonz Farm** (render distribuído), FFmpeg, ToonzScript, controle de versão, rooms customizáveis.
**Tipos de level [alto]** (https://opentoonz.readthedocs.io/en/latest/drawing_animation_levels.html): **Toonz Vector** (.pli; resolução infinita), **Toonz Raster** (.tlv; bitmap com **vínculo à paleta** — trocar a cor do estilo repinta o desenho; linhas "Line" via Brush e áreas "Area" via Paint Brush separadas; Fill com "Fill Depth", "Selective", "Segment"), **Raster padrão** (TIF por quadro, sem vínculo de paleta). O Toonz Raster é a solução histórica (usada na Ghibli) para tinta-e-pinta em bitmap com recolor posterior. (https://morevnaproject.org/2017/03/17/toonz-raster-level)
**GTS [médio]:** "Ghibli Toonz Scanner" — ferramenta de scan interna do Ghibli (Windows, TWAIN; Epson DS-50000 recomendado), cenas por número de intercalação, parâmetros salvos para re-scan (https://www.404techsupport.com/2016/03/30/opentoonz/amp/ e https://toonzpremium.com/scanandcleanup). 
**Uso [médio]:** Studio Ghibli desde *Princesa Mononoke*; Rough Draft Studios (Futurama), Steven Universe, etc. (Wikipedia). 
**Interop [alto]:** XDTS (viewer XDTS feito pela comunidade OpenToonz, https://www.celsys.com/en/clipsolution/xdts/), OCA (import 1.8/export 1.7), CSP EX exporta "OpenToonz Scene File".
**Pontos fortes/fracos [baixo-médio, listicles]:** gratuito, plugin-able, credencial de estúdio; UI "menos moderna", desempenho em projetos complexos, curva de aprendizado (https://studioliddell.com/news/best-2d-animation-software-top-picks ; https://www.startanimating.com/blog/2d-animation-software-comparison).

**Tahoma2D [alto]:** fork focado no animador (API do GitHub https://api.github.com/repos/tahoma2d/tahoma2d): v1.6 (01/03/2026), 1.6.1 (01/05), 1.6.2 (01/07), **1.6.3 (01/09/2026, última)**; ~673 estrelas, 103 forks, 641 issues abertas; um commit recente "Add AI policy" (nightly). Como as datas são todas dia 1 de meses alternados, **confirmar nas releases** antes de citar [médio]. Novidades da 1.6 (https://tahoma2d.readthedocs.io/en/latest/whats_new.html): pontas de pincel (brush tips) para raster/Smart Raster, popups de configuração do stylus, **Column Alpha Lock**, **Loop Frames**, **Paint Behind** para a ferramenta raster, modo Selective em Smart Raster, estilos vetoriais "marker" e "soft round", suporte a inclinação no MyPaint, Preproduction Board refatorado, reconhecedor de lip-sync Rhubarb, **captura por câmera** (Canon SDK/gphoto) — recurso de stop motion que o OpenToonz não tem.
**Mais difícil de reproduzir (OpenToonz):** o **Toonz Raster** com vínculo de paleta (e o par Line/Area), o cleanup de scan, a Plastic tool e o motor de FX com render distribuído. [INFERÊNCIA]

### 2.6 RETAS Studio (Celsys)

**Estado [médio]:** Suite RETAS (Stylos = vetorial/raster para keyframes e intercalação, onion skin, edição vetorial; TraceMan = scan e vetorização por traçado; PaintMan = pintura sobre imagens do Stylos/TraceMan; CoreRETAS = composição/render com simulação de câmera de animação, pan, multiplano; export Flash/QuickTime/AVI). Lançado como RETAS STUDIO em dez/2008 empacotando os quatro. Wikipedia: vendas encerradas em 2015, suporte de fórum encerrado em nov/2019 (https://en.wikipedia.org/wiki/RETAS). A página da Celsys ainda oferece download/trial da **Ver. 6.6.0** (sem data; Win XP–10, macOS 10.4–10.9), sem aviso de fim de vida e sem preço, e apresenta o CSP como sucessor (https://www.clipstudio.net/ja/retasstudio/) [alto para o conteúdo da página; a divergência com a Wikipedia sobre "vendas" não foi resolvida].
**O que aconteceu:** foi o padrão histórico do anime digital (Wikipedia/blogs japoneses: "Stylos era a escolha única" há alguns anos); estagnou (sem suporte a OS atual), e a Celsys migrou os usuários para o CSP. O CSP EX ainda exporta "for PaintMan" e a folha de exposição lê "RETAS STUDIO" em CSV [alto] — herança de compatibilidade. Análise: o ecossistema (cel de papel digital, "time sheet", scan por peg-holes, pintura de cel) é o modelo mental dos animadores japoneses mais velhos. [INFERÊNCIA]
**Mais difícil de reproduzir:** o fluxo scan→traço vetorial automático (TraceMan) com qualidade industrial e o PaintMan com preenchimento automático e marcação de áreas por cel. [INFERÊNCIA/CONHECIMENTO PRÉVIO — detalhes de algoritmos não documentados nas fontes lidas]

### 2.7 CACANi

**Estado [médio]:** Windows only; versões 2.0 (maio/2019) e "2.1" promovida no site; sem lançamento encontrado em 2025/2026 (https://www.cacani.sg/ ; wiki em https://ipfs.aleph.im/ipfs/QmXoypizjW3WknFiJnKLwHCnL72vedxjQkDDP1mXWo6uco/wiki/Cacani.html). Origem: Computational Arts Group da NTU Singapura com a AIC (https://www3.ntu.edu.sg/home/ashsseah/cacani.htm). Preço: licença de ~US$299 única e/ou assinatura de US$15–129/mês (AlternativeTo; data desconhecida) [baixo].
**Features [alto, site oficial]:** **auto-intercalação** (assisted tweening) com ajuste do timing por traço; desenho vetorial com suavização; **auto-paint** (pinte um quadro e a cor é aplicada a outros); **boundary/occluded strokes** (esconder partes de desenho sem ordenar camadas); **stroke taper** em todos os quadros; **stroke rematching** (corrigir a correspondência da intercalação direto no canvas) — v2.0.
**Como funciona [INFERÊNCIA, apoiada por AnimeInbet e outras]:** correspondência entre traços/vértices dos dois desenhos-chave + interpolação; falha onde há oclusão e movimento não linear (ver §4).
**Pontos fracos:** Windows-only, comunidade pequena, sem notícias. **Mais difícil de reproduzir:** a correspondência robusta de traços entre keys com oclusão — problema de pesquisa em aberto (§4.2).

### 2.8 Krita (animação)

**Versão [alto]:** Krita 5.3 e 6.0 lançados em 05/02/2026 (https://krita.org/en/release-notes/krita-5-3-release-notes/); 6.0 = port Qt6 (acesso antecipado); 5.3.3 "recomendada para produção"; 5.3.4/6.0.4 restauram export de vídeo no Android. Licença GPL.
**Desenho [alto]:** Fill tool com **Close Gaps**; nova **Knife tool**; estabilizadores com modo "pixel art smoothing" e suavização dependente da velocidade; novos modos no motor de pincel (Soft Texturing no Pattern, Marker blend, auto-inverter para borracha); transform com Liquify mais rápido; Multibrush "Copy Translate at Intervals"; texto reescrito (no canvas, em caminho). 18 motores de pincel (Bristle, Chalk, Clone, Color Smudge, Curve, Deform, Dyna, Filter, Grid, Hatching, MyPaint, Particle, Pixel, Quick, Shape, Sketch, Spray, Tangent Normal) (https://docs.krita.org/Category:Brush_Engines). O motor MyPaint usa libmypaint (ISC) (https://docs.krita.org/en/reference_manual/brushes/brush_engines/mypaint_engine.html).
**Animação [médio]:** frame-a-frame raster com timeline; **tweening** de posição/rotação/escala/cisalhamento de camadas desde a 5.0 (via busca); onion skin; áudio; export de vídeo via FFmpeg (HDR corrigido na 5.3.3); notas de 5.3 quase não tocam em animação. **OCA**: exportador comunitário (RxLaboratory, codeberg) — exporta grupos, labels, visibilidade, keyframes e durações, opacidade, blend modes etc.; vetor não suportado no OCA (https://codeberg.org/OCA/OCA-Krita). CSV+PNG também.
**Fraquezas [médio]:** sem xsheet de verdade, sem rigging, sem vetor-animação; animação é "adicionada" ao editor de imagem. **Mais difícil de reproduzir:** a variedade de motores de pincel e a gestão de cor (OCIO/ICC/HDR) em canvas grande. Mas é **código aberto e portanto referência/fonte de estudo** (inferência: pode ser lido e talvez licenciado conforme GPL).

### 2.9 Blender — Grease Pencil 3 (GPv3)

**Versão [alto]:** Blender 5.2.2 LTS (15/09/2026) (https://www.blender.org/download/); a documentação já tem 5.4 em dev. Grease Pencil 3 chegou no 4.3 e é o motor atual (CONHECIMENTO PRÉVIO).
**Novidades [alto]:** 5.0 — painel Curve Data, nova **Pen tool** (Bézier), interpolação com Bézier/Catmull-Rom, Join Strokes, **motion blur**, caps/cantos configuráveis, export SVG com curvas (largura uniforme) (https://developer.blender.org/docs/release_notes/5.0/grease_pencil/). 5.1 — **preenchimento (Fill) por operadores** (Stroke/Fill/Both) em vez de via material, **fills com buracos**, busca no menu Move to Layer (https://developer.blender.org/docs/release_notes/5.1/grease_pencil/ via busca). Nós de geometria para Grease Pencil (cores, softness, depth, merge layers) existem no manual 5.2/5.4 [alto].
**Conceitos [CONHECIMENTO PRÉVIO, não verificado nesta rodada]:** camadas e quadros (dopesheet em modo GP), onion skin, multiframe edit, materiais de traço/fill, **modificadores** (Line Art, Build, Time Offset, Noise, Smooth, Thickness, Tint, Subdivide...) e **efeitos de shader**, câmera 3D, composição e render EEVEE/Cycles. Pontos fortes: 2D dentro de 3D (cenário 3D + personagem 2D, câmera multiplano real, luz e render). Fracos: sem xsheet/timesheet japonês, sem tinta-e-pinta no sentido de paleta por estilo (o "material" faz o papel parcialmente), sem camada de arte linha/cor automática, fluxo de intercalação manual.
**Mais difícil de reproduzir:** a integração com o grafo 3D/render e o sistema de modificadores/nós sobre traços.

### 2.10 Synfig Studio

**Versão [médio]:** 1.4.x estável (1.4.5 em mai/2024); desenvolvimento 1.5.4 (jan/2026) e 1.5.5 (22/03/2026); nenhum 1.6 encontrado (https://www.synfig.org/2026/03/). Licença GPL.
**Features [médio]:** animação **vetorial por keyframe com interpolação automática ("tweening")**; sistema de **bones** para vetores/bitmaps (agregador) (https://www.lafabriquedunet.fr/logiciel/synfig-studio); composição em camadas (CONHECIMENTO PRÉVIO: camadas tipo "Region/Outline/Advanced Outline" com largura por ponto, e exporta `.sif/.sifz/.sfg`). Foco em motion-graphic vetorial, não em quadro-a-quadro; a Celsys o descreve como "vetor e bitmap, tweening vetorial e bones" (https://clipstudio.net/en/animation/apps-software). **Fraquezas relatadas:** UI pouco amigável, não é ferramenta de desenho à mão.

### 2.11 Pencil2D

**Versão [alto]:** v0.7.2 "Stability and Polish" (26/03/2026), v0.7.1 RC em fev/2026; open source, gratuito, Win/macOS/Linux/FreeBSD (https://www.pencil2d.org/). 0.7.0 trouxe seleção/colagem/re-exposição de vários keyframes, duplicar camada, opacidade de keyframe/camada, overlays de perspectiva, bucket com opções, câmera retrabalhada (https://flathub.org/apps/org.pencil2d.Pencil2D). Alterna raster e vetor. Escala pequena; não é concorrente de produção.

### 2.12 Moho (Lost Marble)

**Versão [alto]:** Moho 14.5 "SHOWTIME" (atualização gratuita para donos do 14); webcam/gamepad para controle de personagem em tempo real, **Paint Bucket refeito**, **Magnetic Onion Skins**, **Shift & Trace**, Liquid Shapes, Batch Exporter, **export EXR/PNG com perfil de cor**, video tracking; licença perpétua; macOS e Windows (https://moho.lostmarble.com/). Preços (agregadores, [baixo]): Debut ~US$59,99, Pro ~US$399,99; upgrade 13.5 Pro→14 Pro US$119 (página oficial).
**Conceitos [médio, histórico]:** vetor otimizado para animação e rigging; **Smart Bones** (controles que movem/deformam múltiplos pontos, introduzidos no Anime Studio 9, 2012) e Vitruvian Bones; física/partículas; importação de PSD (Debut com limite de 10 camadas); mesh bitmap-puppet; câmera 3D (via agregadores https://en.wikipedia.org/wiki/Moho_(software)). 
**Mais difícil de reproduzir:** Smart Bones (ações com mapeamento de pose → deformação de vários pontos) — ver §4.

### 2.13 Adobe Animate

**Estado [alto/médio]:** última versão com features: 24.0 (out/2023). Em 02/02/2026 a Adobe anunciou fim de vendas (01/03/2026) e suporte (01/03/2027; enterprise 2029); após reação da indústria, **reverteu**: o app continua disponível em **modo de manutenção** (só correções de segurança/bugs, sem novas features), sem data de término (https://gigazine.net/gsc_news/en/20260203-adobe-animate-shut-down ; https://www.neowin.net/news/adobe-animate-is-not-dead-company-reverses-action-after-widespread-backlash/ ; https://www.techspot.com/news/111183-adobe-tries-kill-animate-reverses-course-after-industry.html — a página helpx retornou 403; a data de revisão "9/jun/2026" vem de resumo de busca). Disponível via Creative Cloud Pro.
**Features [CONHECIMENTO PRÉVIO]:** vetor com timeline por camadas/keyframes/tweens (clássico, clássico/motion/shape), rigging por ossos (asset warp, ossos IK), lip-sync, Lottie/HTML5 Canvas/SVG export, importação PSD/AI. Relevância para anime: quase nula; relevante como modelo de timeline vetorial com symbols/nesting.

### 2.14 Procreate Dreams

**Versão [médio]:** Dreams 2 (lançamento 2026), 2.0.2 (fev/2026), 2.0.3 (17/03), 2.0.4 (01/04), **2.1** (https://help.procreate.com/articles/cw4pY8-whats-new-dreams-2-1). iPad only; compra única (US$12,99 para novos; atualização gratuita) (https://creativebloq.com/art/animation/procreate-dreams-2-makes-the-ipad-animation-app-even-more-powerful-and-versatile , preço via busca [médio]).
**Features [alto/médio]:** timeline em três modos (**Compose / Perform / Keyframe**); Flipbook com várias trilhas, blend modes e máscaras; multi-seleção; Seleção & Transformação por quadros; export GIF e mais ajustes; na 2.1: Flipbook flutuante, Scrub Lock, easing visível entre keyframes. Mercado: ilustrador de tablet; sem xsheet japonês.

### 2.15 Live2D Cubism

**Versão [médio]:** Cubism Editor 5.3 (5.3.03 em 16/06/2026); SDK "Cubism 5 SDK R5"; SDKs para Unity/Native/Web/Java/Unreal/Cocos + MotionSync; FREE e PRO (trial de 42 dias; PRO por assinatura anual/3 anos; descontos estudantis 76–80%) (https://www.live2d.com/en/cubism/about/ ; https://koubo.jp/article/99033). Preço de tabela não verificado.
**Conceitos [CONHECIMENTO PRÉVIO + fonte parcial]:** ArtMesh (malha 2D deformável por textura), **deformadores warp e rotation**, **parâmetros** com keyforms interpolados, física, lip-sync, glue; animação por parâmetros; export `.moc3` + `motion3.json` (formatos do runtime). A página lida confirma física, lip-sync e SDKs. A 5.2 trouxe controle IK de parâmetros (https://www.moguravr.com/?p=263677 via busca).
**Relevância:** *não* é ferramenta de anime cel — é rigging 2D de ilustração para VTuber/jogo. **Mais difícil de reproduzir:** o runtime/SDK (moc3), mas a ideia (malha de textura + parâmetros com keyforms) é conhecida.

---

## 3. Formatos e interoperabilidade (consolidado)

| Formato | Dono | Aberto? | Quem lê/escreve | Fonte |
|---|---|---|---|---|
| **XDTS** (folha de exposição digital) | Celsys (2019), usado pela Toei (Toei Animation Digital Time Sheet) | "Aberto, independente de software"; spec (PDF EN/JA, v1.0) só após formulário de registro | CSP EX (import/export), TVPaint 12.1 (import/export), OpenToonz 1.8 + XDTS Viewer, Toei TimeSheet, Callipeg; Nippon Animation e MOTHER adotaram | https://www.celsys.com/en/clipsolution/xdts/ ; https://www.celsys.com/en/xdts/ ; https://doc.tvpaint.com/docs/timesheet/timesheet-import-export |
| **OCA** (Open Cel Animation) | Comunidade (RxLaboratory) | Sim: JSON + PNG/EXR (spec detalhada não lida nesta rodada) | OpenToonz (export 1.7, import 1.8), Tahoma2D, Krita (add-on), Blender, After Effects/Fusion (add-ons), CSP (add-on), Animation Paper | https://codeberg.org/OCA/OCA-Krita ; https://rxlaboratorio.org/?p=893 ; https://animationpaper.com/help/animation-paper-oca-export |
| **PSD** | Adobe | Spec pública mas complexa | CSP, SAI, TVPaint (por camada), Krita, Harmony (PSD layout), Moho (import) | várias |
| **.clip** | Celsys | Não (IANA: sem spec; SQLite + chunks, `CSFCHUNK`) | só CSP; leitores comunitários | https://ftp.iana.org/assignments/media-types/image/vnd.clip |
| **.tvpp / .tvp** | TVPaint | Não | só TVPaint (+ TVPaint Converter) | https://doc.tvpaint.com/docs/import-export/export-files/overview |
| **.xstage / .tvg / .vdb** | Toon Boom | `.xstage` XML legível, `.tvg` não documentado | só Harmony/SBP; utilitários oficiais de conversão | https://github.com/Alex313031/tvg-dev |
| **.sai / .sai2** | Systemax | Não | SAI | https://painttoolsai.readthedocs.io/en/latest/ |
| **.pli / .tlv / TIF** | Toonz / OpenToonz | Código aberto, leitura pelo próprio código | OpenToonz/Tahoma2D | https://opentoonz.readthedocs.io/en/latest/drawing_animation_levels.html |
| **Sequências PNG/TGA/TIFF/EXR** | — | Sim | todos os programas | CSP: https://help.clip-studio.com/en-us/manual_en/600_animation/Export_animation.htm |
| **USD** | Pixar/AOUSD | Sim | Harmony 25, Storyboard Pro 25 | https://cartoonbrew.com/sponsored-by-toon-boom/with-harmony-25-toon-boom-animation-introduces-helpful-tools-across-the-2d-animation-pipeline-247741.html |
| **moc3 / motion3** | Live2D | Runtime proprietário | Cubism SDK | https://www.live2d.com/en/cubism/about/ |

Observações:
- **PSD-por-cel (um PSD por desenho, camadas = linha/cor/fundo)** é o formato de entrega dos animadores a estúdios de pós-produção japoneses [CONHECIMENTO PRÉVIO, não verificado nesta rodada]; os programas acima exportam sequência por cel (CSP: "Export animation cels"; TVPaint: PSD por camada).
- XDTS ligando CSP–TVPaint–OpenToonz–Toei é o único elo comum "japonês" encontrado; Harmony e Krita não têm suporte nativo documentado.
- Todos os formatos nativos podem ser considerados "armadilhas" de migração; o novo programa precisa importar/exportar **sem perda de timing** pelo menos XDTS + sequência por cel + PSD.

---

## 4. Conceitos transversais e o que é tecnicamente mais difícil

Tabela com **dificuldade de reproduzir** (escala 1–5, onde 5 = pesquisa em aberto ou anos de engenharia). A coluna "evidência" indica a fonte de apoio. A dificuldade em si é **INFERÊNCIA**.

| Conceito | Quem faz melhor (relatado) | Dif. | Evidência / observação |
|---|---|---|---|
| **Motor de pincel** (dinâmicas, textura, mistura, latência) | CSP, SAI, Krita (18 motores), TVPaint (dual paper) | 4 | libmypaint é ISC e reutilizável (https://docs.krita.org/en/reference_manual/brushes/brush_engines/mypaint_engine.html); o difícil é a *sensação* e o desempenho em canvas grande (https://tips.clip-studio.com/en-us/articles/17052 mostra lag como problema real) |
| **Estabilização / correção de traço** | SAI, CSP | 3 | parâmetro por ferramenta; CSP 5 adicionou "stroke preview" contra lag |
| **Vetor com largura variável editável por ponto** | CSP (Correct Line), Harmony (Contour/Centerline/Pencil editor), SAI Linework, OpenToonz Toonz Vector | 4 | Harmony TVG: decodificador comunitário não recuperou largura completa (https://github.com/Alex313031/tvg-dev) — mostra quão pouco documentado é; booleana vetorial (borracha por interseção) é parte do custo |
| **Tinta-e-pinta com camadas de arte (linha/cor separadas)** | Harmony, OpenToonz Toonz Raster/Vector, CACANi | 4 | Toonz Raster mantém vínculo cor↔estilo no bitmap (https://morevnaproject.org/2017/03/17/toonz-raster-level) |
| **Fechamento de gap / auto-paint** | Harmony (0–10 px), Krita 5.3 (Close Gaps), TVPaint CTG (speedfill), CSP (fill com "close gap" — CONHECIMENTO PRÉVIO), Tahoma2D (Paint Behind) | 4 | TVPaint admite que gaps restantes precisam de correção manual; Harmony alerta contra valores altos |
| **Auto-paint entre quadros** (propagar cor) | CACANi, TVPaint CTG (copiar squiggles) | 5 | pesquisas recentes tratam como problema ML: DACoN (https://arxiv.org/pdf/2509.14685), PeCA (https://arxiv.org/pdf/2608.00903 — o ID é de agosto/2026) — indica problema ainda ativo |
| **Auto-intercalação** | CACANi (correspondência de traços); pesquisas: AnimeInbet (grafo de vértices, ICCV 2023), ToonCrafter (difusão, SIGGRAPH Asia 2024; 16 quadros 512x320, flicker) | 5 | https://arxiv.org/html/2309.16643 ; https://arxiv.org/html/2405.17933v1; survey https://arxiv.org/pdf/2501.06250 — nenhuma solução geral confiável em produção encontrada |
| **Xsheet/timesheet com exposição por referência, loops, pre/post behavior** | TVPaint 12.1, Harmony, OpenToonz, CSP (trilhas) | 3 | exposição por referência é modelo de dados, não algoritmo; difícil é a ergonomia e as operações em lote |
| **Timesheet japonês (XDTS) e papéis de quadro (key/breakdown/inbetween)** | TVPaint 12.1, CSP, Toei TS | 2–3 | formato JSON (INFERÊNCIA não verificada), spec sob registro |
| **Rigging cut-out com deformadores** (bone/curve/envelope/mesh) e nós | Harmony Premium, Moho, OpenToonz Plastic, TVPaint 12 bitmap rig, Live2D | 4 | Harmony tem 6 deformadores; Moho tem Smart Bones; TVPaint o fez só em 2024 |
| **Câmera multiplano** | Harmony, CoreRETAS, TVPaint 12 (2D camera key-based), CSP (2D camera folder), Blender | 3 | camada com Z real vs. paralaxe fake |
| **Onion skin / light table / shift-and-trace** | Todos | 2 | Moho 14.5 trouxe "Magnetic Onion Skins" |
| **Cleanup de scan** | OpenToonz, TVPaint Scan Cleaner, TraceMan, Harmony | 3 | menos relevante em 2026 (digital) |
| **Efeitos/composição** (FX, nós, partículas, deep image) | Harmony Premium, OpenToonz FX schematic, After Effects (fora do escopo) | 4 | grafo de nós GPU; OpenFX |
| **3D de referência** | CSP (3D figure, hand model), Blender | 3 | depende de assets |
| **Interop/gestão de cor** (ICC/OCIO/EXR) | Krita, Moho 14.5 (EXR com perfil), Harmony (EXR DWAA) | 3 | |

---

## 5. Pontos fortes e fracos relatados (síntese)

- **CSP EX:** forte em desenho/brush/preço/multiplataforma; fraco em timeline para produção grande e sem rigging (fontes: https://clipstudio.net/en/animation/apps-software ; roundups).
- **SAI:** estabilizador e simplicidade; sem animação; Windows-only e sem HiDPI no v1.
- **TVPaint:** "feel" de mão e textura, usado em filmes autorais e TV; sem vetor; preço alto; roadmap com SDK/Python/Timesheet.
- **Harmony:** padrão de TV/filme, rigging, pipeline; caro, fechado, curva íngreme.
- **OpenToonz/Tahoma2D:** grátis, credencial Ghibli/Futurama; UI datada, performance/estabilidade em projetos grandes, comunidade média (7,8 mil estrelas no OpenToonz).
- **Krita:** melhor ferramenta de desenho aberta; animação básica.
- **Blender GP:** 2D no 3D e render; sem fluxo de cel tradicional.
- **Moho/Synfig/Pencil2D:** nichos (cut-out/vetor/hobby).
- **Adobe Animate:** congelado em manutenção.
- **Procreate Dreams, Live2D:** nichos (tablet/VTuber).

Fontes de opinião (todas listicles/blogs, [baixo-médio]): https://www.startanimating.com/blog/2d-animation-software-comparison ; https://studioliddell.com/news/best-2d-animation-software-top-picks ; https://bloopanimation.com/toon-boom-harmony-vs-tv-paint . Não encontrei threads recentes de animadores profissionais (fóruns/Reddit/Discord de estúdio); isto é uma **lacuna** (§9).

---

## 6. Matriz comparativa de features

Legenda: **S** = sim/maduro; **P** = parcial/básico; **N** = não; **?** = não verificado. "Verif." indica se a célula veio de fonte lida (V) ou conhecimento prévio (C).

| Feature | CSP EX | SAI | TVPaint | Harmony Prem. | OpenToonz | Tahoma2D | Krita | Blender GP | Moho Pro | CACANi | Synfig | Pencil2D | Animate | Dreams | Live2D |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Animação frame-a-frame | S (V) | N | S (V) | S (V) | S (V) | S (V) | S (V) | S (C) | S (V) | S (V) | P | S (V) | S (C) | S (V) | N |
| Xsheet/timesheet formal | P (trilhas+XDTS) (V) | N | S (V, 12.1) | S (C) | S (V) | S (C) | N (timeline) | P (dopesheet) | P | ? | N | N | N | N | N |
| Importa/exporta XDTS | S (V) | N | S (V) | ? (não achado) | S (V, 1.8) | ? | N | N | ? | ? | N | N | N | N | N |
| OCA | add-on (V) | N | ? | N | S (V) | S (C) | add-on (V) | add-on (V) | ? | ? | N | N | N | N | N |
| Onion skin / light table | S (V) | N | S (V) | S (V) | S (V) | S (V) | S (C) | S (C) | S (V) | S (C) | S | S | S | S | N |
| Vetor com largura variável | S (V) | S (V, Linework) | N | S (V, Adv/Prem) | S (Toonz Vector) (V) | S (V) | P (camadas vetoriais, C) | S (C, strokes) | S (V) | S (V) | S (C) | P | S (C) | N | N |
| Raster/bitmap | S | S | S | S (Adv+) | S | S | S | P (texturas) | P | S (2.0+) | P | S | P | S | S (textura) |
| Tinta-e-pinta c/ paleta ligada | N (cor por camada) (C) | N | N (CTG tem paleta por camada) (V) | S (V) | S (Toonz Raster) (V) | S | N | P (materiais) | P | S (auto-paint) | N | N | N | N | N |
| Fechamento de gap / auto-fill | S (C) | ? | S (CTG) (V) | S (V, 0–10px) | S (Fill Depth/auto) (V parcial) | S (Paint Behind) (V) | S (V, 5.3) | P (5.1 fills) (V) | S (14.5 Paint Bucket) (V) | S (V) | N | P | S (C) | N | N |
| Estabilizador de traço | S (V) | S (V) | S (V, line smoothing) | S (C) | S (C) | S (C) | S (V) | S (C) | S (C) | S (V) | N | N | N | N | N |
| Pressão + inclinação | S (V) | S (C) | S (C) | S (C) | S (C, MyPaint tilt) | S (V) | S (C) | S (C) | P | S | N | S | S | S | N |
| Motor de pincel avançado | S (V) | S (C) | S (V) | P (texturas vetoriais) | P (MyPaint) | P (MyPaint, tips) | S (V, 18 motores) | P | P | P | N | N | P | S | N |
| Rigging cut-out | N (hierarquia) | N | P (12.0 bitmap) | S (V) | S (Plastic/Skeleton) (V) | S | N | S (armature, C) | S (V) | N | S (bones) | N | S (C) | N | S (C) |
| Deformers/mesh warp | P (Puppet Warp) (V) | N | P (rig) | S (V, 6 tipos) | S (Plastic) | S | P (Liquify) | S | S | N | S | N | S | N | S (C) |
| Câmera 2D/multiplano | S (2D camera folder, sem Z real) (V) | N | S (V, key-based) | S (V, Z) | S (C) | S | N | S (3D real) | S | N | S | P | S | P | N |
| FX/composição de nós | N | N | S (FX stack) (V) | S (V) | S (V) | S | N | S | P | N | S | N | P | P | N |
| 3D/modelos | S (V) | N | N | S (V, importa 3D) | N | N | N | S | P | N | N | N | N | N | N |
| Scripting/SDK | N/limitado (C) | N | S (George+SDK GitHub) (V) | S (JS/C++) (V) | S (ToonzScript) (V) | S | S (Python) (C) | S (Python) (C) | S (Lua) (C) | N | S | N | S (JS) | N | S (SDK) |
| Colorização assistida por IA | N | N | N | Ember (parcial, sem inbetween) (V) | N | N | N | N | N | N (auto-paint clássico) | N | N | N | N | N |
| Plataformas | Win/Mac/iPad/iPhone/Android (V) | Win | Win/Mac/Linux/Android (V) | Win/Mac/Linux (V parcial) | Win/Mac/Linux (V) | Win/Mac/Linux (V) | Win/Mac/Linux/Android (V) | Win/Mac/Linux (C) | Win/Mac (V) | Win | Win/Mac/Linux | Win/Mac/Linux/BSD (V) | Win/Mac (C) | iPad | Win/Mac |
| Licença | perp.+assin. (V) | perp. (V) | perp. (V) | assinatura (V) | open source | open source | GPL | GPL | perp. | perp./assin. (?) | GPL | open source | assinatura CC | compra única (V) | FREE/PRO (V) |

Observação: células marcadas (C) vêm de conhecimento prévio e **não devem** ser usadas como fato sem revalidação.

---

## 7. Conjunto mínimo priorizado para um estúdio adotar um programa novo

Critério: o que, se faltar, faz o estúdio **não migrar** (INFERÊNCIA minha, baseada nos fluxos acima; deve ser validada com entrevistas de animadores — lacuna §9). P0 = bloqueador absoluto; P1 = necessário para uso diário; P2 = necessário para escala; P3 = diferencial.

**P0 — sem isto ninguém adota**
1. **Desenho com sensação de classe CSP/SAI/TVPaint**: pincel com pressão/inclinação, baixa latência, estabilização configurável, borracha, desfazer rápido; canvas HD/4K e superior sem lag. (Evidência: todos os concorrentes têm; lag é o relato típico, https://tips.clip-studio.com/en-us/articles/17052.)
2. **Linha limpa**: raster de alta qualidade **e** vetor com espessura variável editável (pelo menos ajuste por ponto, "simplify/connect", borracha até interseção) — modelo CSP/Harmony.
3. **Timeline/xsheet com exposição por referência**: cels por trilha, loop/hold, reposição de quadros, dobradiça de fps/duração, **onion skin** configurável e **light table**; áudio com scrubbing e waveform.
4. **Camadas de cel + camadas de papel (layout/fundo) + câmera 2D** com keyframes e easing, pan/zoom/rotação.
5. **Importação/exportação sem perda**: **PSD** (camadas), **sequências PNG/TGA/TIFF/EXR**, **XDTS** (folha de exposição japonesa) e vídeo (MP4/MOV/ProRes). [evidência: XDTS em CSP/TVPaint/OpenToonz; sequência em todos]
6. **Pintura de cel (tinta-e-pinta)**: preencher com fechamento de gap ajustável, paletas/estilos nomeados reutilizáveis por personagem, repintar sem refazer linhas.

**P1 — uso diário de estúdio**
7. **Camadas de arte separadas linha/cor (+ underlay/overlay)**, "paint behind".
8. **Ferramentas de transformação**: transformar/warp/mesh/perspective por quadro e em lote (Harmony Contour/Envelope, CSP Puppet Warp/Liquify, TVPaint transformação nítida).
9. **Biblioteca de assets** (pincéis, paletas, personagens, fundos) e atalhos personalizáveis; undo ilimitado; autosave contínuo (CSP 5 fez "continuous save").
10. **Renderização de composição básica**: blend modes, mascaramento, glow/blur, **EXR** com perfil de cor, render em lote.
11. **Quadros-chave/breakdown/inbetween** como "papéis" do quadro (TVPaint 12.1) e marcação por cor para fluxo de equipe (CSP 4.0).
12. **Linha de tempo de storyboard/animatic simples** (opcional se ficar em outro módulo do programa único).

**P2 — escala de produção**
13. **Fluxo de equipe**: cena como pasta com arquivos separados e/ou banco de dados, bloqueio de arquivos, versionamento (Harmony multiusuário, OpenToonz VCS).
14. **Cut-out/rigging** com ossos + deformadores de curva/malha e drawing substitution + lip-sync por fonemas (Harmony/Moho/OpenToonz Plastic).
15. **Nós/FX**: grafo de nós com FX GPU, partículas (OpenFX opcional).
16. **Scripting/SDK** (Python recomendado) para pipelines; TVPaint abriu SDK em GitHub.
17. **Scan/cleanup** de papel (ainda relevante para parte do anime e ex-RETAS).

**P3 — diferenciais (não bloqueiam)**
18. **Auto-intercalação** assistida (traços vetoriais, tipo CACANi) e **auto-paint propagado**: nenhuma ferramenta sólida existe; ótimo diferencial mas não condição de adoção.
19. **3D de referência** (pose, mão, perspectiva) integrado, conforme CSP.
20. **Câmera multiplano 3D**, importação USD/3D, composição deep (Harmony Premium).
21. **IA** (máscara, remoção, inbetween generativo) — opcional e opt-in, como a Toon Boom fez com o Ember, para não conflitar com a resistência do setor (INFERÊNCIA).

Notas de risco: (a) o estúdio não troca de formato nativo sem ponte; (b) a adoção japonesa depende de **XDTS** e "papéis" de quadro; (c) a barreira de entrada de preço é baixa (CSP EX ≈ US$277 perpétua ou ≈ US$77/ano), o que força o produto novo a competir em **valor** e não em preço.

---

## 8. Recomendações imediatas para a Fase 2 (INFERÊNCIA)

1. Entrevistar 5–10 animadores/estúdios (JP e global) para validar §7; sem isso, a lista é hipótese.
2. Obter e ler a **especificação XDTS 1.0** (formulário em https://www.celsys.com/en/xdts/) e a **spec do OCA** (rxlaboratory.org/tools/oca) antes de desenhar o modelo de dados.
3. Fazer protótipos medidos (latência pincel→pixel, estabilização, fill com gap) comparando a CSP/TVPaint/Krita em hardware de referência.
4. Avaliar libmypaint (ISC) e o motor do Krita (GPL) como referência/base de pincel; checar compatibilidade de licença.
5. Mapear o formato `.tvpp`, `.clip` (via clipparse) e `.xstage` para **importadores** em vez de tentar substituí-los.
6. Tratar auto-intercalação e auto-paint como P3 com protótipo de pesquisa separado.

---

## 9. Lacunas e divergências conhecidas

- **Preços:** Harmony (duas leituras da loja divergem ~30%), Storyboard Pro (3 valores), TVPaint (só varejo), CACANi (agregador), Moho (agregador), Live2D (só promoções). Todos exigem confirmação na loja oficial.
- **Datas:** SAI2 (Systemax: preview de 2026-10-11; Wikipedia: Beta 3 de 2026-09-25); Tahoma2D (datas 01/mês parecem padronizadas — checar); TVPaint 12.1.0 sem data oficial; Animate (data de revisão da página de "novidades" não confirmada diretamente).
- **Fontes não lidas:** loja TVPaint e documentação George/rigging (404); página de comparação FREE/PRO da Live2D; manual do Blender (modificadores GP — truncado); manual do Krita (animação — 404); páginas helpx da Adobe (403); spec XDTS e spec OCA (não baixadas).
- **Features marcadas CONHECIMENTO PRÉVIO:** estrutura do XDTS (JSON), réguas de perspectiva do CSP, tons/assets do CSP, camadas e "Binary pen" do SAI, modificadores/onion do Blender, Smart Bones do Moho 14, bones do Synfig, Animate tweens, Live2D ArtMesh/Warp. Devem ser revalidados.
- **Adoção industrial:** nenhuma pesquisa independente de market share encontrada (só marketing da Celsys e uma lista da Wikipedia japonesa).
- **Opinião de animadores profissionais:** só listicles e blogs; sem fóruns/entrevistas.
- **Harmony e XDTS:** não encontrei suporte nativo; se existir, mudaria a matriz.
- **Ember (Toon Boom):** só o conjunto de funções anunciado na beta; nenhuma verificação de qualidade.

---

## 10. Lista de URLs-chave (por programa)

- CSP: https://www.celsys.com/en/topic/20260311 ; https://www.cgchannel.com/2026/05/celsys-releases-clip-studio-paint-5/ ; https://help.clip-studio.com/en-us/manual_en/030_new/ ; https://help.clip-studio.com/en-us/manual_en/600_animation/Export_animation.htm ; https://help.clip-studio.com/en-us/manual_en/600_animation/Importing_and_exporting_a_timeline_(EX).htm ; https://tips.clip-studio.com/en-us/articles/9951 ; https://www.clipstudio.net/en/features/ ; https://ftp.iana.org/assignments/media-types/image/vnd.clip ; https://pypi.org/project/clipparse/
- XDTS/OCA: https://www.celsys.com/en/clipsolution/xdts/ ; https://www.celsys.com/en/xdts/ ; https://codeberg.org/OCA/OCA-Krita ; https://rxlaboratorio.org/?p=893
- SAI: https://www.systemax.jp/en/sai/ ; https://painttoolsai.readthedocs.io/en/latest/ ; https://en.wikipedia.org/wiki/PaintTool_SAI
- TVPaint: https://tvpaint.com/en/blog/tvpaint-animation-12-nouveautes ; https://doc.tvpaint.com/docs/changelogs/12-0-1 ; https://doc.tvpaint.com/docs/changelogs/12-1-0 ; https://doc.tvpaint.com/docs/timesheet/timesheet-import-export ; https://www.tvpaint.com/ ; https://en.wikipedia.org/wiki/TVPaint_Animation
- Toon Boom: https://docs.toonboom.com/help/harmony-25/essentials/release-notes/harmony/harmony-25-release-notes.html ; https://shop.toonboom.com/en/products/harmony ; https://docs.toonboom.com/help/harmony-24/essentials/reference/tool-properties/paint-tool-properties.html ; https://docs.toonboom.com/help/harmony-25/premium/getting-started/deformation.html ; https://docs.toonboom.com/help/harmony-22/premium/paperless-animation/about-art-layer.html ; https://helpcentre.toonboom.com/hc/en-ca/articles/48953146993555-About-saving-options-in-Harmony ; https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-release-notes.html
- OpenToonz/Tahoma2D: https://github.com/opentoonz/opentoonz/releases ; https://api.github.com/repos/opentoonz/opentoonz ; https://opentoonz.readthedocs.io/en/latest/ ; https://opentoonz.readthedocs.io/en/latest/drawing_animation_levels.html ; https://tahoma2d.readthedocs.io/en/latest/whats_new.html ; https://api.github.com/repos/tahoma2d/tahoma2d/releases ; https://en.wikipedia.org/wiki/OpenToonz
- RETAS: https://www.clipstudio.net/ja/retasstudio/ ; https://en.wikipedia.org/wiki/RETAS
- CACANi: https://www.cacani.sg/ ; https://www3.ntu.edu.sg/home/ashsseah/cacani.htm
- Krita: https://krita.org/en/release-notes/krita-5-3-release-notes/ ; https://docs.krita.org/Category:Brush_Engines
- Blender: https://www.blender.org/download/ ; https://developer.blender.org/docs/release_notes/5.0/grease_pencil/ ; https://developer.blender.org/docs/release_notes/5.1/grease_pencil/
- Synfig: https://www.synfig.org/2026/03/ ; Pencil2D: https://www.pencil2d.org/ ; Moho: https://moho.lostmarble.com/ ; Animate: https://gigazine.net/gsc_news/en/20260203-adobe-animate-shut-down ; https://www.neowin.net/news/adobe-animate-is-not-dead-company-reverses-action-after-widespread-backlash/ ; Procreate: https://help.procreate.com/articles/cw4pY8-whats-new-dreams-2-1 ; Live2D: https://www.live2d.com/en/cubism/about/
- Pesquisa de inbetweening/colorização: https://arxiv.org/html/2309.16643 ; https://arxiv.org/html/2405.17933v1 ; https://arxiv.org/pdf/2501.06250 ; https://arxiv.org/pdf/2509.14685 ; https://arxiv.org/pdf/2608.00903
