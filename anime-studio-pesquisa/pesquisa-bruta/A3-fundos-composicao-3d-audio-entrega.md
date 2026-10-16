# A3 — Fundos, Composição/Fotografia (撮影), 3DCG, Efeitos, Cor, Áudio e Entrega

Pesquisa bruta da fase 1 (programa único de produção de anime). Data da pesquisa: 2026-10-15. Frente A3.

## 0. Como ler este relatório

**Confiança**: `[alto]` = fonte primária ou várias fontes concordantes; `[médio]` = fonte secundária única, resumo de ferramenta de busca, ou dado possivelmente datado; `[baixo]` = fonte fraca, blog/fórum, ou lembrança minha não confirmada.

**Rótulos**:
- **FATO** = afirmado por uma fonte lida/consultada nesta sessão (URL ao lado).
- **INFERÊNCIA** = conclusão minha a partir dos fatos.
- **CONHECIMENTO GERAL (não verificado)** = o que sei de formação técnica e não consegui confirmar aqui. Tratar como hipótese a validar.

**Aviso metodológico**: várias respostas de busca vieram resumidas por um modelo intermediário; quando a página original não pôde ser lida (PDFs binários, páginas da Netflix que redirecionam para um "Knowledge Hub" vazio), marquei `[médio]`. Pelo menos um PDF primário (o handout do GDC do Guilty Gear Xrd) eu extraí e li inteiro com `pdftotext`; isso está marcado como `[alto]`.

### Resumo executivo (o que mais importa para o produto)

1. **Composição (撮影) é o coração técnico** desta frente. É feita quase universalmente em **After Effects (AE)**, que virou "língua comum" entre equipes 2D e 3D no Japão. O paradigma é **por camadas + linha do tempo**, com plugins. Um programa novo deve oferecer **UI em camadas (estilo AE)** sobre um **motor de grafo de nós em GPU float** por baixo.
2. O "look de anime" na composição é feito de **um conjunto pequeno e bem definido de operações** (blur por cor/camada, glow multi-escala, 透過光, halation, difusão, light wrap, gradiente/atmosfera, aberração cromática, grão, flare, shake, retime em 1s/2s/3s). Seção 2.4 traz a biblioteca com algoritmos.
3. Há um achado forte: **cels de anime são cores chapadas sem anti-aliasing**, e os plugins japoneses (PSOFT CelMX/CelFX, OLM) existem justamente porque blur/glow genéricos do AE **contaminam cores e perdem áreas pequenas**. O programa novo deve tratar "seleção por cor" e "blur consciente de cor" como cidadãos de primeira classe.
4. **3D em anime**: o Guilty Gear Xrd (handout primário lido) dá a receita mais clara e documentada de toon shading; produção de TV usa 3ds Max/Maya + Pencil+ 4 e cada vez mais **Blender**; linhas são cada vez mais **ajustadas na composição** (Pencil+ 4 Line for AE), não finalizadas no 3D.
5. **Cor**: o anime japonês historicamente tem **pouca gestão de cor**; o padrão prático é sRGB/Rec.709, SDR. Plataformas pedem HDR/IMF só em títulos específicos. **AE 26.5 (set/2026) e Blender 5.0 agora trabalham nativamente em ACES/OCIO**, então um programa novo pode adotar OCIO desde o início sem ficar isolado.
6. **Entrega**: loudness é um ponto **crítico e fácil de errar**: TV japonesa (ARIB TR-B32) usa **-24 LKFS**; Netflix usa **-27 LKFS dialog-gated ±2 LU, pico real -2 dBTP**. As duas não coincidem.
7. **Áudio/edição**: no Japão o fluxo é **アフレコ (afureko) → efeitos + seleção/edição de música → MA/mixagem**, historicamente em **Pro Tools**. O MVP deve ser "sincronizar áudio com a imagem + medir loudness + exportar", não uma DAW completa.

---

## 1. FUNDOS (美術 / bijutsu)

### 1.1 Papel e fluxo geral

- **FATO** `[médio]`: a arte de fundo é feita em etapas: ajustes de composição/layout, rough de luz e sombra, base de cores, detalhamento e acabamento, exportação para composição. Os livros e cursos de fundo japoneses cobrem **Photoshop e CLIP STUDIO PAINT**. Um livro da Kadokawa informa que, em TV anime, pode-se exigir mais de **10 fundos por dia** por artista, o que explica o uso de partes/pincéis reaproveitáveis. https://www.kadokawa.co.jp/product/322105000636/
- **FATO** `[alto]`: a técnica tradicional dos fundos é **tinta guache japonesa "poster color"** (ex.: Nicker Poster Color), em papel grosso, trabalhada de grandes massas para o detalhe; o "olhar aquarela" vem de gradações bem misturadas, não de material especial. Exemplo: Kazuo Oga (Studio Ghibli), 24 cores de Nicker, papel TMK 400 lb. https://www.openculture.com/2021/01/a-look-inside-the-painting-process-of-the-studio-ghibli-artist-kazuo-oga.html
- **FATO** `[médio]`: a Toei Animation introduziu pen displays (Cintiq) na arte de fundo, chegando a 100% de adoção (estudo de caso Wacom; ano não confirmado). https://tablet.wacom.co.jp/biz-design/pdf/casestudy_toei_animation.pdf
- **FATO** `[baixo]`: recriar o "poster color look" digitalmente é ensinado como combinação de **camadas, modos de mistura, pincéis texturizados e paleta limitada**, em qualquer app de pintura (curso de terceiro, sem parâmetros). https://syntetyc.gumroad.com/l/anime-ghibli-illustration-course-1 (não adotar como referência técnica; serve só de indício.)

### 1.2 3D como base de layout e de fundo (tendência forte)

- **FATO** `[alto]` ufotable (Demon Slayer): o Castelo Infinito foi **inteiramente modelado em 3ds Max**; testaram cerca de 30 versões do layout; vários prédios são feitos em CG primeiro e **os artistas desenham os fundos à mão a partir dos modelos**, mantendo perspectiva consistente. Plugins citados: V-Ray, PhoenixFD, tyFlow, ForestPack, RailClone, GrowFX, HairFarm, Pencil+. Usar o modelo CG como base permite que animadores comecem com ~60% de acabamento. https://area.autodesk.jp/case/animation/kimetsu-01/
- **FATO** `[médio]`: OLM Digital + ARED (Kusuriya no Hitorigoto) fizeram palestra sobre **processo de fundo 3D → layout 3D com os dados do fundo 3D → guia de desenho (作画ガイド)**. Um diretor de arte do ARED construiu um fluxo de fundos **baseado em Maya** na OLM. https://cgworld.jp/special/cgwcc2024/detail/olm/
- **FATO** `[médio]`: a CGWORLD vol.311 (jun/2024) traz uma matéria de 54 páginas "Layout 3D de anime", com casos MAPPA (Shingeki no Kyojin Final Season), OLM/ARED (Kusuriya) e Polygon Pictures (BLOODY ESCAPE), este último com a técnica de **alternar fundo pintado e fundo 3D**. Os detalhes do texto não foram lidos. https://cgworld.jp/flashnews/cgw311-pr.html e https://cgworld.jp/regular/202406-cgw311-ab.html
- **FATO** `[médio]` Quunplant (estúdio de fundos 3D+2D): usou **Blender 4.2 LTS + Cycles + After Effects 2024** nos fundos 3D e vídeos de fundo de *Chou Kaguya-hime!*; a sua direção de arte propõe fazer a **arte de ambientação em 3D e usá-la direto como LO de fundo**; a equipe migrou de **SketchUp para Blender**. https://www.moguravr.com/quunplant-kaguyahime-3d-sample/ e https://cgworld.jp/special/masterclass-online/vol16/takahara/
- **FATO** `[médio]` técnica **camera mapping / projeção**: pinta-se o fundo 2D (ex.: no Krita), projeta-se sobre geometria 3D simples no Blender e anima-se a câmera para obter paralaxe. https://www.blendernation.com/2024/10/11/creating-anime-backgrounds-in-blender-using-projection-mapping/
- **FATO** `[médio]` cursos ocidentais (Gnomon) ensinam "bloquear em Blender, detalhar com photobashing/pintura em Photoshop, e usar passes de render para gerar canais alfa". Não são estúdios japoneses. https://www.thegnomonworkshop.com/workshops/designing-animation-backgrounds-using-blender-photoshop
- **FATO** `[médio]` Clip Studio Paint está na **versão 5.x** (5.0 em 11/03/2026; 5.1.0 em jul/2026), com recursos 3D (modelo de mão 3D etc.). Não confirmei o estado de réguas de perspectiva/importação 3D para fundo. https://www.clipstudio.net/en/news/202601/28_01/ e https://clipstudio.net/en/dl/release_note/latest/
- **CONHECIMENTO GERAL (não verificado)**: o CSP importa modelos 3D (FBX/OBJ/.c3o) e tem régua de perspectiva, usados por muitos artistas de fundo e mangaká como base de layout. Confirmar antes de basear decisões.

### 1.3 Fluxo "cut a cut" (síntese)

- **INFERÊNCIA** a partir de ufotable/OLM/Sol Levante: (1) layout (LO) vindo da animação/diretor → (2) fundo 3D opcional como guia de perspectiva e câmera → (3) fundo pintado **maior que o quadro** quando há pan (a Netflix relatou que **render de planos com pan maior que 4K atrasou o projeto por meses**) → (4) entrega em **PSD em camadas** para a composição. O Sol Levante liberou PSDs e projetos AE como material aberto. https://opencontent.netflix.com/ (Sol Levante)
- **CONHECIMENTO GERAL (não verificado)**: o fundo costuma ser entregue por cut, com camadas separadas (céu, plano médio, "BOOK"/primeiro plano) para permitir blur por profundidade, parallax e luz.

### 1.4 O que um programa integrado precisa para fundos

| Item | Imprescindível? | Nota |
|---|---|---|
| Pintura raster em camadas, pincéis com textura e mistura, modos de mistura, tablet com pressão | Sim | Equivalente a Photoshop/CSP; é a fatia mais cara de reproduzir bem |
| Importar/exportar **PSD** (camadas) | Sim | É o formato de troca real (Sol Levante) |
| Canvas grande (maior que o quadro) e sub-regiões de pan | Sim | Pan > 4K foi o maior gargalo do Sol Levante |
| Base 3D (layout, perspectiva, câmera) com **render só de passes** (cor chapada/escala de cinza/profundidade) | Desejável (v2) | Confirma a tendência 3D→pintura |
| Camera mapping/projeção de fundo 2D em geometria simples | Desejável (v2) | Paralaxe barata |
| "Look" de poster color (textura de papel, pincel opaco, bordas irregulares) | Raro/difícil | É estilo, não algoritmo; exige banco de pincéis e papel |

---

## 2. COMPOSIÇÃO / FOTOGRAFIA (撮影 / satsuei)

### 2.1 O que a equipe de satsuei realmente faz

- **FATO** `[alto]`: 撮影 é a etapa que **junta elementos de vários departamentos (fundos, animação-chave, cores, efeitos) em uma imagem final e aplica filtragem para que pareçam existir no mesmo mundo**. O nome é histórico (câmera em mesa de animação sobre cel e fundo); hoje é composição digital, e o **After Effects virou o padrão no Japão**. https://cgworld.jp/terms/%E6%92%AE%E5%BD%B1.html e as demais fontes desta seção.
- **FATO** `[médio]` o diretor Kenichi Kutsuna (ufotable) define satsuei como montar a tela a partir de materiais como desenhos e arte e **adicionar efeitos visuais**. https://www.sportskeeda.com/anime/demon-slayer-director-reveals-secrets-behind-show-s-jaw-dropping-animation
- **FATO** `[médio]` Fate/kaleid liner Prisma Illya (Asahi Production): os valores de gradação e brilho dependem do tamanho do personagem; define-se um **valor de referência por composição e ajusta-se por cut**; o AE "adiciona informação" a materiais renderizados em cor chapada para chegar à imagem final. https://cgworld.jp/feature/201512-prisma-illya-cgw208t2.html (conteúdo lido via resumo; a página 2 não carregou.)
- **FATO** `[médio]`: terminologia clássica de câmera/efeitos da era do cel, em fontes japonesas:
  - Câmera: pan, tilt, zoom, **TU/TB (track up/back, ampliar/reduzir o conjunto)** e **DTU/DTB** (digitais), dolly, crane, rotação.
  - Cel work: slide (SL), rolling, **shake (balanço)**, **波送り (namiokuri, mover a crista de onda ao longo do material: capas, chamas, cabelo)**.
  - Efeitos: fade, overlap (fusão), **ぼかし (blur/difusão)**, **透過光 / T光 (luz transmitida)**, **WXP/ダブラシ (semi-transparência por dupla exposição)**, **フェアリング (ease in/out de pan)**.
  - Transições: íris, focus in/out, wipe, cutback.
  - **マルチプレーン**: vidros em distâncias diferentes para paralaxe.
  - Fontes: https://www.soumu.go.jp/johotsusintokei/whitepaper/ja/h11/press/japanese/housou/0626j14.htm e https://scrapbox.io/maxcaffydotwork/%F0%9F%91%A9%E2%80%8D%F0%9F%8E%A4Anime:_%E3%82%A2%E3%83%8B%E3%83%A1%E9%96%A2%E9%80%A3_%2F_%E3%83%86%E3%82%AF%E3%83%8B%E3%83%83%E3%82%AF (a segunda é nota pessoal; `[baixo]` para os termos T光/WXP/DTU).
- **FATO** `[médio]`: sobre透過光 e ハレーション, a CGWorld descreve na prática flare/bloom extraídos de EXR e somados (adição) para dar "ar", mais lens flare/vazamento de luz para atmosfera. https://cgworld.jp/regular/201602-garyu-067-2.html
- **CONHECIMENTO GERAL (não verificado)**: "tsuya" (艶, brilho especular do cabelo) costuma ser desenhado como região de cor própria no acabamento/animação e, na composição, apenas suavizado/gradado com seleção por cor. Termos como "パラ撮影" e "マルチ" não foram confirmados em fonte; não afirmo seu significado.

### 2.2 Ferramentas e paradigmas

| Ferramenta | Paradigma | Estado 2026 | Notas |
|---|---|---|---|
| **After Effects** | Camadas + timeline + plugins; câmera 2D/3D, expressions | **26.5 (set/2026)**: ACES 2.0 nativo com OpenColorIO 2.5.1 (duas configs: "ACES 2.0 CG v4.0" e "Studio v4.0"); 26.3 trouxe Depth of Field no renderer 3D avançado e tracker de máscara até 5x mais rápido `[médio]` | Padrão da indústria japonesa de 撮影. Fonte: https://www.provideocoalition.com/after-effects-portal-june-2026/ e Adobe "what's new" (a página oficial retornou 403; dados via resumo) |
| **Nuke** | Grafo de nós, linear/float, 3D integrado, deep | Nuke 17.0 lançado 26/02/2026 `[médio]` | Padrão em VFX de cinema; **Non-Commercial** limitado a HD e sem plugins de terceiros. https://broadcastbeat.com/news/foundry-releases-nuke-17-0 |
| **DaVinci Resolve/Fusion** | Nós (Fusion) + cor + edição + áudio (Fairlight) | **Resolve 21 (jun/2026), 21.1 (set/2026)**; versão gratuita até UHD 60 fps com Fusion completo; Studio US$295 perpétua `[médio]` | Candidato a "ferramenta única" de pós. https://www.newsshooter.com/2026/06/02/davinci-resolve-21-final-release/ |
| **Natron** | Nós, código aberto (GPLv2, OFX) | Mantido por comunidade, estável 2.5 `[médio]` | Confirmar no GitHub antes de depender. https://github.com/NatronGitHub/Natron/releases |
| **Blender compositor** | Nós; GPU | **Blender 5.0 (18/11/2025)**: pipeline ACES, asset shelf com presets (vignette, grão, aberração cromática), Convert Colorspace/Convert to Display, árvore do compositor reutilizável nas strips do Sequencer `[médio]` | https://alternativeto.net/news/2025/11/blender-5-0-launches-with-aces-color-enhanced-rendering-and-tighter-vfx-integration/ |

- **FATO** `[médio]`: por que o AE em anime 2D: grande base de usuários, "linguagem comum" entre equipes 2D e 3D; efeitos 3D costumam ser renderizados em **passes separados** e geridos por take/material. https://cgworld.jp/feature/202102-houdini-nrp.html (página sumarizada).
- **FATO** `[médio]` fluxo 2D/3D: previs e frame-dropping/câmera 2D são mais fáceis no AE do que no 3D; dor típica: **cada cut chega com linhas, cores, sombras e fundos em arquivos separados que alguém organiza à mão no AE**. (resumo de busca, fonte original primeiro hit: https://7545845019560.gumroad.com/l/2DCompositing e artigo citado na busca) `[baixo]`.
- **INFERÊNCIA (importante para o produto)**: organizar automaticamente os arquivos de um cut (linha, cor, sombra, highlight, fundo, passes) numa composição pronta é uma dor documentada e **uma funcionalidade de alto valor** que um programa integrado resolve "de graça" (porque os departamentos já estão no mesmo projeto).

**Layer-based vs. node-based**
- Camadas: curva de aprendizado baixa, cada cut tem dezenas de camadas empilhadas, expressions e keyframes por camada; o time de satsuei japonês pensa assim. Ruim para reuso de "receitas" e para pipeline automatizado.
- Nós: reuso, passes, deep/linear, automação, melhor para 3D/VFX; mais difícil para quem vem do AE.
- **INFERÊNCIA**: oferecer **timeline em camadas como UI padrão** e permitir "abrir o grafo" de cada camada/efeito (como o Fusion dentro do Resolve) e guardar **presets de look por cena/sequência** (o que o satsuei faz manualmente via templates).

### 2.3 Plugins comuns (e o que dizem sobre o que o AE nativo não faz)

- **PSOFT CelMX** (8 efeitos) otimizado para imagens **sem anti-aliasing**: BlurCelLayer (blur referenciando as cores de outra camada), **BlurCelMulti** (blur hierárquico que expande cores sem perder pixels de áreas pequenas), BlurCelRadial (radial em fronteiras de cor, preservando um lado da borda), BlurPath (arrasto ao longo de caminho), ChromaticAberrations (com "Steps"), **Glow** (barra de gradação + dados de profundidade), GradientMap, InnerShadow (cria luz de contorno/"light wrap"). https://www.psoft.co.jp/en/product/celmx/ `[alto]`
- **PSOFT CelFX** (11 efeitos): BlurCel (suaviza fronteiras entre cores), BlurRGB, BoundaryLine, **ColorSelection** (extrai cores para ajuste ou track matte, com presets), Deband, DeltaFX, **Extend** (expande contornos e simula pressão de pincel), Fill, Gradient, ReplaceColor, Texture. https://www.psoft.co.jp/en/product/celfx/ `[alto]`
- **OLM Digital** (R&D): mais de 40 plugins internos de AE; abertos: OLM Blur, Radial Blur, Directional Blur (gratuitos, preservam intensidade de cor e detalhe, podem adicionar ruído para gerar linhas de movimento), Color Key. Em desenvolvimento/produção: **OLM Lens Flare (11 tipos; o AE nativo tem 3)**, Light Ray, Particles, Cellular Noise e **OLM Toon AE (ajusta contornos toon na composição com dados do Maya)**. https://www.cgchannel.com/?p=89339 e https://speakerdeck.com/olmdrd/matsuri2022-aeplugin `[alto]`
- **PSOFT Pencil+ 4 Line for After Effects**: edita linhas toon renderizadas em 3ds Max/Maya via arquivos **PLD**; modos Offset e Replace, Rendering Scale, Map Layers, Z Depth. Caso: **Production I.G, ending da 2ª temporada de Kaiju No. 8** (linhas ajustadas no AE, partículas do 3ds Max exportadas como PLD). https://psoft.co.jp/en/product/pencil/ae/ e https://psoft.co.jp/en/product/gallery/20260127-kaiju-no-08/ `[alto]`
- **Genéricos de AE**: Deep Glow (glow fisicamente motivado, threshold HDR) `[médio]` https://aescripts.com/deep-glow/ ; Video Copilot **Saber** (gratuito) e **Optical Flares** (flares); **Red Giant/Maxon** Trapcode, Magic Bullet (2026.5 em jul/2026) `[médio]` https://www.motionmedia.com/mm-blog/whats-new-red-giant-2026-5 ; **Boris FX Sapphire 2026.5** (flares, glows, FilmBurn; a partir de US$545/ano) e Continuum, em AE/Nuke/Resolve/Premiere `[médio]` https://blog.borisfx.com/sapphire-2026-an-ode-to-physical-media ; "Variable Diffusion" (difusão em realces preservando faixa dinâmica) `[médio]` https://cgworld.jp/news/tool/1502-after-effectsvariable-diffusion.html
- **INFERÊNCIA**: a existência desses plugins japoneses mostra o que o AE não oferece bem: **blur/glow conscientes de região de cor, "extensão de cor" sem franjas, e seleção/substituição por cor exata**. Isso é onde um programa nativo pode ganhar.

### 2.4 Biblioteca concreta de efeitos de composição para o "look de anime" (com algoritmo)

> As colunas "algoritmo" são, em sua maior parte, **conhecimento técnico geral** (CG/vídeo em tempo real) e **INFERÊNCIA** sobre como mapear para anime; as fontes citadas confirmam o efeito ou a família do algoritmo. Custo: **G** = GPU trivial (um passe), **G+** = vários passes/pirâmide, **C** = melhor/único em CPU ou pré-processamento.

| # | Efeito | O que faz no anime | Algoritmo (resumo) | Custo / notas |
|---|---|---|---|---|
| 1 | **Blur gaussiano separável** | Base de quase tudo (difusão, ボケ, glow) | Dois passes 1D (horizontal e vertical); para sigma grande, **downsample + blur + upsample** ou 3 box blurs/IIR recursivo | G; trabalhar em **alfa pré-multiplicado** para não gerar franja escura |
| 2 | **Blur por camada/profundidade (ボケ de câmera)** | Fundo desfocado, primeiro plano desfocado | Raio da camada = f(\|z − z_foco\|); a camada inteira é borrada com o kernel (disco/gaussiano) e recomposta de trás para frente; versão refinada: máscara de profundidade por pixel | G+; em anime é por **camada**, não por pixel, na maioria dos casos (Inferência) |
| 3 | **Bokeh por disco/polígono** | Pontos de luz desfocados com formato de diafragma | Convolução com kernel disco/polígono (gather em shader, ou FFT para raios grandes); highlights HDR realçam o disco | G+ ; o Blender tem Bokeh Blur node https://docs.blender.org/api/5.0/bpy.types.CompositorNodeBokehBlur.html `[médio]` |
| 4 | **Blur consciente de cor (CelBlur)** | Suaviza fronteiras entre cores chapadas sem sujar a cor nem apagar áreas pequenas | Blur **hierárquico/pirâmide** com ponderação por similaridade de cor ou por camada de referência; "expande" cores em vez de misturar com vizinhos | G+; é o que PSOFT BlurCelMulti/BlurCelLayer e OLM Blur fazem (alto) |
| 5 | **Glow / bloom multi-escala** | Brilho em luzes, magia, olhos, janelas | threshold (HDR) → **cadeia de downsample** (pirâmide mip) → blur em cada nível → **upsample somando com pesos** (queda ~exponencial) → adicionar (add/screen) ao original. Família "pirâmide" documentada por Jimenez (CoD: AW) | G+; barato e estável no tempo; **Deep Glow** usa threshold suave p/ evitar flicker `[médio]` https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare/ |
| 6 | **Difusão (ディフュージョン)** | Suaviza contraste, "névoa de lente", aura | Cópia borrada misturada com **screen/lighten** apenas nos realces (máscara por luminância), preservando HDR | G+ ; plugin Variable Diffusion `[médio]` |
| 7 | **Halation (ハレーション)** | Realces vazam em halo avermelhado/quente em volta | threshold de realces → tint (laranja/vermelho) → blur grande → add. Também pode ser localizado por seleção de cor | G+ |
| 8 | **透過光 / T光 (luz transmitida)** | Luz atravessando cel/vidro/folhas; "sol por trás" | Matte de uma região de cor/camada → blur com queda → **add/screen** sobre o resto, opcionalmente com gradiente direcional; PSOFT Glow usa barra de gradação + profundidade; OLM tem "Distance Gradation" (aura de luz) | G+ ; conceito (T光 = glow) em nota pessoal no scrapbox citada na seção 2.1 e no artigo do Soumu (透過光撮影 em cel); uso digital inferido (`[médio]`/`[baixo]`) |
| 9 | **Luz de contorno / contra-luz (逆光, rim)** | Borda iluminada do personagem pelo fundo | **Light wrap**: fundo muito borrado × máscara de borda (alfa borrado − alfa original, ou inner-glow) → somar ao personagem; rim direcional: deslocar o alfa na direção da luz e subtrair | G+ ; algoritmo do LightWrap do Nuke/Natron (Diffuse, FGBlur, BGBlur, merge "plus"): https://learn.foundry.com/NUKE/content/reference_guide/draw_nodes/lightwrap.html `[alto]` |
| 10 | **Inner shadow / inner glow** | Volume e luz de borda sem 3D | Distância/erosão do alfa → gradiente interno | G; PSOFT InnerShadow |
| 11 | **Gradiente / perspectiva atmosférica** | Escurecer/clarear por profundidade, névoa | Gradiente linear/radial multiplicado ou somado por camada (modo Soft Light/Overlay); PSOFT Gradient/GradientMap | G |
| 12 | **Gradient map / tinta por luminância** | Look de cena (cor do dia/noite) | LUT 1D de luminância → cor; ou LUT 3D completo | G |
| 13 | **Correção de cor** | Look por sequência | Curvas, lift/gamma/gain, CDL (slope/offset/power/sat), **LUT 3D** (tetraédrica) | G ; **hardware precisa de LUT 3D** `[médio]` |
| 14 | **Seleção por cor / replace color** | Mattes por cor chapada, trocar cores em fundo diurno→noturno | Distância de cor com tolerância zero/baixa (sem anti-aliasing), saída como alfa; presets | G ; PSOFT ColorSelection/ReplaceColor |
| 15 | **Aberração cromática** | Foco "de lente", impacto | Escalar/deslocar canais R e B radialmente (ou lateralmente) em relação ao centro; versão em "passos" para não borrar | G ; PSOFT ChromaticAberrations (Steps) |
| 16 | **Grão de filme** | Textura, esconder banding | Ruído (gaussiano/azul) ponderado por luminância, temporalmente variável; no anime muitas vezes sutil/estático | G ; AE "Add Grain" |
| 17 | **Deband / dither** | Evita faixas em gradações suaves (crítico em 8 bits e HDR) | Ruído de dither antes da quantização; ou suavização local | G ; PSOFT Deband |
| 18 | **Lens flare** | Sol, luzes, brilho de espada | Sprites ao longo da linha da fonte ao centro (ghosts), streak anamórfico (blur 1D horizontal), anéis; 11 tipos no OLM | G ; Sapphire/Optical Flares/OLM |
| 19 | **Raios de luz (god rays)** | Sol entre árvores | Radial blur acumulativo a partir da fonte (ponto) sobre máscara de brilho | G+ ; OLM Light Ray |
| 20 | **Blur direcional/radial/zoom** | Velocidade, impacto, linhas de movimento | Amostragem ao longo de direção/raio; OLM adiciona **ruído para formar linhas de movimento**, preservando intensidade | G |
| 21 | **Shake (sacudida) em tempo anime** | Impacto, terremoto | Offsets em x/y/rot com ruído (Perlin/wiggle) **amostrado em 1s/2s/3s** (posterizar o tempo), nunca "liso" | G |
| 22 | **Retime / hold / posterize time** | Manter as camadas em 2s/3s mesmo com câmera suave | Congelar quadros por N; **sem blend de quadros** | trivial |
| 23 | **Parallax / multiplano / câmera 3D** | Profundidade em pan/dolly | Camadas 2D com z, câmera de perspectiva, escala/posição por profundidade; **importar câmera de USD** (ufotable usa Python+módulo USD no AE para criar keyframes de câmera e camadas 3D) | G ; https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html `[alto]` |
| 24 | **Máscaras, track mattes, alfa/luma matte, tracking** | Isolar elementos | Rasterização de splines com feather, mattes empilháveis, tracking de pontos/planos | G/C |
| 25 | **Distorção (calor, ondulação, lente)** | Ar quente, água, fisheye | Deslocamento por mapa (displacement) animado; distorção de lente polinomial | G |
| 26 | **Partículas** | Pétalas, faíscas, neve | Sistema CPU/GPU com emissores; OLM Particles (texturas e glow) | G/C |
| 27 | **Normal Shading / Reduce Details (para efeitos 3D)** | Fazer simulação 3D parecer desenhada | Sombrear a partir da normal em cel (2 tons), reduzir ruído, **posterizar o tempo** | G ; OLM Toon FX `[alto]` https://speakerdeck.com/olmdrd/matsuri2020-toonfx |
| 28 | **Suavização de linhas e anti-aliasing "pós"** | Cels vêm sem AA | Supersample ou AA morfológico/MSAA no final; preservar alfa | G ; CelMX é "otimizado para imagens não suavizadas" |
| 29 | **Fade-to-white em HDR** | Transições | Em HDR o fade total para branco "queima"; Netflix/I.G **sobrepuseram uma camada branca** em vez de elevar o branco | G ; https://www.redsharknews.com/this-is-how-netflix-is-adapting-anime-to-modern-technology `[médio]` |

**Requisitos de performance (resumo)**
- **FATO** `[médio]` Sol Levante: o gargalo mais difícil foi **renderizar planos finais com pans em imagens maiores que 4K**; adiou o projeto meses. https://about.netflix.com/en/news/4-k-short-sol-levante-combines-art-and-technology-to-deliver-best-in-class-anime
- **FATO** `[alto]` ufotable testou RTX 6000 Ada (48 GB) e RTX 5000 Ada (32 GB) em HP Z6 G5 (Threadripper PRO 7965WX, 128 GB RAM) e relatou que **48 GB de VRAM permitiu upscaling de 4x no EmberGen (antes 2x)**, e que trocas de efeito/transição em DaVinci Resolve rodam em tempo real. https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html
- **INFERÊNCIA**: os efeitos acima são quase todos **fragment shaders de poucas passadas, limitados por largura de banda de memória**; o custo real é **o número de camadas** (50 a 200 por cut) em **float16/float32 em 1080p–4K**. Estratégias: cache por camada estática, composição em tile/região de interesse, pré-multiplicação consistente, formato de trabalho float16 e render em segundo plano. CPU: decodificar sequências TGA/PNG/PSD/EXR e I/O de disco (sequência por cut).

### 2.5 O que é imprescindível, o que é raro

- **Imprescindível**: camadas com alfa pré-multiplicado, 2D+3D camera/parallax, blur por camada, glow multi-escala, add/screen/soft light e multiply, seleção por cor exata, gradientes, correção de cor, blur direcional/radial, shake/hold em tempo anime, importação de sequências (TGA/PNG/EXR/PSD), exportação por cut.
- **Muito útil**: light wrap/rim, halation, difusão, aberração cromática, grão, deband, lens flare, partículas, câmera de USD/FBX, presets de look por cena.
- **Raro/difícil**: reproduzir os **ajustes finos por cut** de um diretor de fotografia (é julgamento artístico), **blur de cor sem franjas** em cels sem AA, tratar **HDR** sem "queimar" (fade branco), planos com **pan gigante**, e a automação de **organização dos arquivos de cada cut**.

---

## 3. 3DCG EM ANIME

### 3.1 Receita documentada: Guilty Gear Xrd (Arc System Works)

**Fonte primária lida inteira**: handout do GDC 2015 de Junya Christopher Motomura, extraído de https://www.ggxrd.com/Motomura_Junya_GuiltyGearXrd.pdf `[alto]`. Paráfrase:

- **Princípio**: "matar tudo que parece 3D"; a matemática do shader está sempre "correta", mas **cada coisa na tela deve ser decisão do artista**. Pré-visualização do shader em tempo real **no software de modelagem** era essencial.
- **Modelos**: cerca de **40.000 triângulos** por personagem (para close-ups em cutscenes); detalhe modelado em geometria; **sem normal maps**; dados guardados em **vertex normals, vertex colors e UVs**, porque são independentes de resolução (textura estoura em close-ups).
- **Sombreamento**: cel shading binário com função degrau; só três coisas importam: **limiar, vetor de luz e normal**.
  - Limiar: um canal de **vertex color como offset do limiar** (zero = sempre sombra; serve de oclusão manual).
  - Luz: cada personagem tem **seu próprio vetor de luz dedicado** (sem iluminação global), que **só se anima nas cutscenes**.
  - Normais: **editadas à mão** nas principais feições (rosto especialmente) para eliminar manchas indesejadas.
  - Cor: duas texturas, **base (lit)** e **tint (sombra)**; sombra = base × tint (a sombra "diz o quão sólido é o material": pele ganha tom avermelhado). Texturas são só cores chapadas de lookup.
- **Contornos externos**: **inverted hull** gerado no shader, expandido ao longo da normal; largura variável **controlada por vertex color**, pode sumir em partes; visível no viewport de modelagem. Preferido ao post-process porque dá controle por vértice e previsão no editor.
- **Linhas internas**: não funciona com inverted hull; usam **UVs especiais** em que linhas são **vigas alinhadas a eixo** numa textura e o UV sobrepõe a viga em maior ou menor grau (controla espessura) → **linhas sem serrilhado nem em close-up**; UV muito distorcido, aceitável porque não há detalhe na textura.
- **Animação**: **animação limitada** (sem interpolação entre chaves; cada quadro é uma chave, como stop-motion), rig com **~500 ossos** por personagem, **sem simulação**, uso massivo de **animação de escala** (squash/stretch, esconder/mostrar), e **deformar a malha a cada chave** para quebrar a perfeição da perspectiva ("pense em 2D"; expressividade sobre precisão). Engine: **Unreal Engine 3**; DCC: **Softimage**.
- **Strive**: o diretor de arte Hidehiko Sakamura declarou que o objetivo foi parecer o máximo possível com animação 2D desenhada à mão e incluir sombreamento "não normal em anime" `[baixo]` (apenas a citação; não achei material técnico de Strive).

### 3.2 Contornos: inverted hull vs. pós-processamento vs. Freestyle

- **FATO** `[médio]` (resumo de fóruns/tutoriais): inverted hull falha em arestas côncavas ou muito próximas de outras superfícies e em interseções, mas dá controle por vértice; post-process cobre toda a cena e mantém espessura constante na tela (pode ser vantagem ou defeito) e pega arestas que um detector por profundidade perde se combinar Laplaciano + profundidade; **Freestyle** é baseado em arestas (offline, lento para tempo real). Produções reais **combinam**: inverted hull para o exterior e texturas/UV para o interior (Xrd). https://www.kodeco.com/92-unreal-engine-4-toon-outlines-tutorial
- **INFERÊNCIA**: para um programa único, o caminho mais robusto é **linhas como camada separada** (passe de linha com espessura/cor/escala ajustáveis na composição), como já faz o **Pencil+ 4 Line for AE** e o **OLM Toon AE**. Isso casa com a tendência observada: "ajustar linha na composição, não finalizar no 3D" (I.G/Kaiju No. 8).

### 3.3 Integração 3D → 2D em estúdios

- **ufotable**: personagens principalmente à mão, mas híbridos (corpo CG + rosto desenhado), corvos quase todos em CG, adereços em CG; decisão **2D vs CG tomada cedo** em reuniões de produção; 1–2 meses de P&D para cada técnica nova; **água do Breath of Water majoritariamente à mão**, só um pouco de 3D (entrevista ANN). https://area.autodesk.jp/case/animation/kimetsu-01/ `[alto]` e https://www.animenewsnetwork.com/feature/2019-08-28/interview-demon-slayer-producer-yuma-takahashi/.149177 `[médio]`. Outras páginas "explicando" que a água é 3D simulada contradizem a produtora e **não devem ser usadas** (blogs/AI-SEO).
- **OLM Digital** (Pokémon, Kusuriya etc.): P&D próprio em **Maya** (Bifrost, Arnold) + plugins de AE (acima).
- **Orange** (Houseki no Kuni, Beastars): base 3D com aparência quase de animação à mão, mocap feito pelo CEO em Houseki no Kuni; fundos pintados à mão. `[médio]` https://www.animeherald.com/2025/05/17/the-studio-orange-staff-talks-with-anime-herald/ (sem detalhes de pipeline/fps).
- **B-water** (Blender): palestra na Blender Conference 2024 sobre pipeline e shading toon NPR proceduralizado para série de TV. https://conference.blender.org/2024/presentations/1979/ `[médio]`
- **Quunplant**: Blender para fundos 3D (acima).
- **Polygon Pictures, Trigger, Science SARU, Sublimation**: **não achei documentação técnica** pública nesta sessão (lacuna). Polygon Pictures aparece só em matéria da CGWORLD (fundos 3D vs. pintados) e slides de 2016 `[baixo]`.
- **FATO** `[médio]` Ken Anjyo (OLM) publicou em 2024 um levantamento sobre a evolução do toon shading e técnicas relacionadas, incluindo GPU e IA, com foco em **animação híbrida 2D+3D**. https://olm.co.jp/rd/journal-of-ieice-202402/?lang=en (a página direta falhou; dado da busca)
- **FATO** `[médio]` Takeshi Okuya (ASC – Anime System Community) apresentou no SIGGRAPH Asia 2024 "Integrating OpenUSD and Real-time Rendering for Japanese 2D Anime Production", adaptando **OpenUSD** a fluxos de anime. ufotable já exporta USD do 3ds Max e usa Omniverse para layout 3D. https://asia.siggraph.org/2024/presenter/index581f.html e https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html
- **INFERÊNCIA**: **USD (ou FBX/glTF) para câmera/cena** é a ponte plausível entre departamentos; importar câmeras e props de qualquer DCC sem reimplementar.

### 3.4 Ferramentas 3D relevantes (estado 2026)

- **Blender 5.0** (nov/2025) com ACES; sem modo toon nativo ("não há modo toon embutido"); comunidade usa Shader-to-RGB/EEVEE e forks (Goo Engine) `[médio]` https://www.strayspark.studio/blog/best-blender-toon-shader-addons-2026.
- **Projeto oficial NPR do Blender**: começou em jul/2024 com a Dillon Goo Studio; o protótipo foi redesenhado: **filtros e modificação de cor vão para um compositing multi-estágio anexado ao objeto/material** (recebe cor + AOVs e devolve cor; viaja com o asset), e **recursos de shading ficam no sistema de materiais** (Ray Queries, Portal BSDF, Custom Shading, Depth Offset). Desenvolvimento previsto **depois do Blender 5.0**; sem versão-alvo. https://code.blender.org/?p=12670 `[alto]`. Isso é um sinal de arquitetura: **"material + estágio de composição por objeto"**.
- **Pencil+ 4** (3ds Max, Maya, Blender) e **Pencil+ 4 Line for AE**: linhas toon de produção (3ds Max ainda tem mais recursos, ex.: stroke maps/hachura, que no Maya). https://psoft.co.jp/en/support/faq/pencil4-comparison-chart/
- **Maya + Arnold/Bifrost** (OLM); **3ds Max + V-Ray + plugins** (ufotable).

### 3.5 Usos típicos de 3D (veículos, multidões, fundos, câmeras)

- **FATO** `[médio]` veículos: Patlabor EZY (3D de Labor), Polygon Pictures; veículos e criaturas difíceis de desenhar de forma consistente (corvos do Demon Slayer) vão para CG. https://cgworld.jp/404/ (título/resumo) e https://area.autodesk.jp/case/animation/kimetsu-01/
- **FATO** `[médio]` fundos 3D como guia e **câmera 3D** como base do plano; 3D layout (matéria CGWORLD 311).
- **CONHECIMENTO GERAL (não verificado)**: multidões em anime costumam ser CG de baixa fidelidade, em loops/instâncias, com visual toon e geralmente **"quebra de quadro" (2s/3s)** para casar com a animação 2D. Confirmar.

### 3.6 Efeitos animados à mão e FX 3D

- **FATO** `[alto]` (OLM, R&D): efeitos de **fogo, água e fumaça** definem o look do anime, mas simulação 3D é difícil de controlar e não parece 2D; abordagens testadas: desenhar à mão sobre objetos 3D (pouco eficiente), ruído celular/Voronoi **binarizado** + glow (fogo, água), **fumaça procedural por "puffs"** (Maya Bifrost, shader toon Arnold, rim/borda), **Normal Shading + Reduce Details + CC Toner + posterização do tempo no AE** para simplificar simulações; morphing para entrecaixes (triângulos/EMD). https://speakerdeck.com/olmdrd/matsuri2020-toonfx e https://speakerdeck.com/olmdrd/matsuri2024-toonfx
- **FATO** `[médio]`: **Houdini** é usado para fumaça/fogo/água de anime e finalizado no AE (Houdini→NPR→AE; cursos de VFX supervisors de *Fire Force* e *Suzume*). https://cgworld.jp/flashnews/one-animevfx-water.html
- **FATO** `[alto]`: ufotable testa **EmberGen (simulação GPU quase em tempo real)** misturada com animação à mão; **Moon Angel** pintou por cima de efeitos 3D em sequência ou colou loops desenhados em 3D. https://cgworld.jp/feature/bsma-5.html `[médio]`
- **INFERÊNCIA**: um programa integrado deve oferecer **efeitos "sem tempo realista"**: simulação/ruído → **binarização com curva (limiar) + 2 tons + posterização temporal** → camada 2D. Isso é muito mais barato do que Houdini e cobre fogo/fumaça/água estilizados.

### 3.7 Imprescindível vs. raro (3D)

- **Imprescindível**: ler **FBX/glTF/USD**, **câmera 3D** exportada ao compositor, **toon shading em 2 tons com limiar/vetor de luz/normal editável**, **contorno separado como passe** com espessura variável, **passes** (cor chapada, luz/sombra, profundidade, ID/cryptomatte), animação sem interpolação (stepped) e escala animável.
- **Raro/difícil**: reproduzir **imperfeição intencional por chave** (deformar a malha por quadro), editar normais à mão em escala, linhas internas sem serrilhado em close-ups, **look de "hand-drawn FX" a partir de simulação**.
- **Performance**: toon + contorno rodam em **tempo real em GPU** (Xrd rodava em PS3/PS4); passes de render offline com path tracing (Cycles/Arnold/V-Ray) são **GPU/CPU pesados**; simulação (EmberGen/Houdini) pede **VRAM alta** (48 GB permitiu 4x de upscaling).

---

## 4. COR E ENTREGA

### 4.1 Gestão de cor: o que se faz hoje

- **FATO** `[médio]` (Netflix, via Red Shark e gwern): a gestão de cor é **"rara" em fluxos de anime**; emissoras japonesas ajustam a temperatura de cor do monitor para **9300 K** (mais azul que os 6500 K do resto do mundo); muitos títulos licenciados são convertidos para **D65 BT.1886** só no fim do pós. HDR é "inexistente" na produção de anime; o fade para branco total é forte demais em HDR. A maior barreira citada foi o **monitor HDR de referência acessível**. https://www.redsharknews.com/this-is-how-netflix-is-adapting-anime-to-modern-technology e https://gwern.net/doc/anime/2020-04-02-netflix-bringing4kandhdrtoanimeatnetflixwithsollevante.html
- **FATO** `[médio]`: *Sparks* (teste da Netflix) foi finalizado em **ACES** a 4000 nits; *Sol Levante* teve entregas **P3 PQ D65 (Dolby Vision 2.9)**, HDR10 1000 nits, **UHD 24 fps**, Atmos, e liberou **sessões Pro Tools, PSDs, projetos AE, TGA de entrecaixes**. Monitores usados: EIZO ColorEdge CG319X (edição) e Prominence CG3145 (referência HDR). https://opencontent.netflix.com/ e https://www.casestudies.com/company/eizo/case-study/netflix-creates-the-worlds-first-4k-hdr-hand-drawn-animation-with-eizo
- **FATO** `[médio]`: sRGB e Rec.709 têm as **mesmas primárias e branco**; diferem na curva (sRGB ≈ 2,2 com trecho linear; Rec.709 gravação ≠ exibição; BT.1886 ≈ 2,4). Um caso de longa-metragem: criar em sRGB e usar LUT 3D no pós para P3. https://docs.toonboom.com/help/harmony-24/paint/colour/about-colour-space-management.html e https://lowepost.com/forums/topic/16118-color-pipeline-of-animation-feature/
- **FATO** `[alto/médio]` OCIO/ACES: OCIO v2 tem ACES nativo; **AE 26.5 (set/2026) trabalha em ACES 2.0 com OCIO 2.5.1**; **Blender 5.0** usa pipeline ACES. https://www.provideocoalition.com/after-effects-portal-june-2026/ e https://alternativeto.net/news/2025/11/blender-5-0-launches-with-aces-color-enhanced-rendering-and-tighter-vfx-integration/
- **INFERÊNCIA**: projeto deve nascer com um **espaço de trabalho linear de cena (ACEScg ou Rec.709 linear float)** + **vista de exibição configurável** (sRGB/Rec.709 BT.1886/P3/HDR PQ), e um preset "anime SDR simples" (sem fricção) ao lado do preset ACES. **Atenção**: a arte 2D pintada em sRGB, quando entra num pipeline linear, **muda o aspecto de gradientes, glows e blends** (modos de mistura em espaço gama vs. linear dão resultados diferentes). Como os estúdios acostumaram-se ao resultado "em gama", **oferecer um modo "mistura em espaço de exibição" (compatível com AE clássico)** é um requisito de compatibilidade visual. (Inferência; confirmar com testes A/B contra AE.)

### 4.2 Formatos de intercâmbio

| Formato | Uso | Notas |
|---|---|---|
| **TGA / PNG sequências** | Cels, linhas, entrecaixes entre departamentos | Sol Levante liberou TGA; PNG ocupa bem menos que TGA em teste da CGWorld https://cgworld.jp/regular/vol14-2.html `[médio]` |
| **PSD** | Fundos, camadas | Sol Levante liberou PSD |
| **TIFF 16 bits** | Mezanino HDR para IMF | Recomendado pela Netflix (v5.1) |
| **EXR** (half/float, multicamada) | Passes 3D, composição linear | Versão antiga da Netflix (2017) aceitava DPX/EXR/TIFF não comprimidos como fonte do IMF `[médio]` |
| **DPX** | Cinema/entrega legada | — |
| **ProRes 422 HQ / 4444 / 4444 XQ** | Mezanino/turnover | 422 HQ ≈ 220 Mbps a 1080p; Netflix turnover ProRes descrito em **UHD, P3, PQ** `[médio]`; **licenciamento de ProRes em Windows é ambíguo** (fóruns), FFmpeg tem `prores_ks` mas "não é abençoado pela Apple" |
| **DNxHR** | Alternativa em Avid/Windows | MainConcept: até 4K 4:4:4 12 bits; FFmpeg: até 10 bits `[médio]` |
| **IMF (SMPTE ST 2067-21 App. #2E)** | Entrega à Netflix | Validado pelo Photon; recomenda-se gerar a partir da linha de tempo colorida ou de um mezanino. https://backlothelp.netflix.com/hc/en-us/articles/7262346654995 `[alto]` |
| **H.264/H.265** | Preview/revisão e envio a revisores | FFmpeg |

- **INFERÊNCIA**: MVP: **exportar sequências PNG/TIFF16/EXR + vídeo H.264/H.265 + ProRes via FFmpeg (com aviso de licenciamento)**; **IMF fica para depois ou delegado ao Resolve**.

### 4.3 Taxa de quadros e resolução

- **FATO** `[alto]` (ANN): animação "em três" (threes) = **8 desenhos por segundo a 24 fps**; "em dois" = 12/s; "em um" = 24/s; fours/fives em cenas estáticas (apenas boca se move). O anime de TV costuma usar mais threes, com ones/twos em ação, e a regra é uma média, não rígida. https://AnimeNewsNetwork.com/encyclopedia/lexicon.php?id=61
- **FATO** `[médio]`: uma pesquisa acadêmica descreve a técnica japonesa de variar quadros por segundo conforme a velocidade do movimento. https://cgg.cs.tsukuba.ac.jp/projects/2013/motion_frame_omission/index.html
- **FATO** `[médio]` (2020): anime de TV é geralmente **1280×720 ("half HD")**, só alguns de alto nível em 1080p; Netflix estimou que 4K de mão exige **desenho digital** e papel maior. Hoje (2026) **1080p é o padrão típico (CONHECIMENTO GERAL, não verificado nesta sessão)**. https://www.redsharknews.com/this-is-how-netflix-is-adapting-anime-to-modern-technology
- **CONHECIMENTO GERAL (não verificado)**: produção em 24 fps (23,976 progressivo); a transmissão de TV é feita com conversão para 29,97i (pulldown). Streaming mantém 23,976/24.
- **INFERÊNCIA**: o modelo temporal do programa deve ser **por quadros inteiros (24 / 23,976 / 25 / 30)**, com **tempo de exposição em "N-ones"** como conceito de primeira classe (cada desenho com duração em quadros, **timesheet / xsheet**); o formato **xdts** da Celsys é um formato de timesheet de intercâmbio `[médio]` https://www.celsys.com/en/clipsolution/actf.

### 4.4 Especificações de entrega: Netflix, Crunchyroll, TV japonesa

**Netflix (v5.1, fonte oficial lida via resumo de busca; as páginas atuais foram movidas para "studiopartner.netflix.net" e não carregaram)** `[médio/alto]`
- IMF Application 2E (SMPTE ST 2067-21:2016/2020); mezanino HDR de preferência TIFF 16 bits, UHD ou HD conforme a entrega; **um único asset long-play**.
- **Teste PSE (epilepsia fotossensível, ITU-R BT.1702-1; Harding, Tektronix ou equivalente) é obrigatório para todo conteúdo animado**.
- HDR: originais em HDR devem ser masterizados em **Dolby Vision** (4.0 aceito; 2.9 ainda suportado). Sol Levante também entregou HDR10.
- **Áudio (Sound Mix Spec v1.5)**: **-27 LKFS ±2 LU, dialog-gated (BS.1770-1), pico real ≤ -2 dBTP**; arquivos **WAV 24 bits/48 kHz**; **5.1 obrigatório, 2.0 opcional**; referência near-field 79 ou 82 dB SPL; Atmos tem regras próprias. A documentação mais antiga/QC cita ±3 LKFS e -1 dBTP (versão 1.4). https://backlothelp.netflix.com/hc/en-us/articles/360001794307-Netflix-Sound-Mix-Specifications-Best-Practices-v1-5 e https://backlothelp.netflix.com/hc/en-us/articles/360050414014-Loudness-and-True-Peaks-How-to-Measure-and-When-to-Flag
- Taxa de quadros especial para anime: **não encontrei regra publicada**; exemplos usam 23,976/24.

**Crunchyroll**: **a especificação de entrega não é pública** (nada encontrado); o que se sabe é que recebe vídeo, áudio, legendas e metadados de licenciadores/estúdios. Confirmar com o contato comercial. Lacuna. https://www.educative.io/blog/crunchyroll-system-design (genérico, `[baixo]`)

**TV japonesa (ARIB)**
- **FATO** `[alto]` ARIB TR-B32 "Operational Guidelines for Loudness of Digital Television Programs": versões 1.0 (mar/2011), 1.1 (BS.1770), 1.4 (≥ 5.1 e 3D), 1.5 (7.1 e 22.2), **1.6 (25/03/2025, áudio baseado em objetos)**; a página lista também **1.7 (datada "10.09.2026" no formato MM.DD.AAAA da própria página, isto é, 9 de outubro de 2026; acrescenta orientações de loudness fora da radiodifusão, como internet e multiplataforma)**; como a data é de seis dias antes de "hoje" e veio de resumo, **conferir manualmente**. https://www.arib.or.jp/english/std_tr/broadcasting/desc/tr-b32.html
- **FATO** `[médio]` valores: **alvo -24 LKFS**; **pico real -1 dBTP**; medição por ITU-R BS.1770; tolerância citada como **±2,0 LU** em preset de terceiros (confirmar texto oficial; a norma da NAB T032 corre em paralelo desde 10/2012). Extração do PDF oficial confirmou apenas o "-24,0 LKFS" e outros números da tabela; os textos japoneses saíram truncados. https://www.arib.or.jp/image/kikaku/kikaku_sample/sample-tr-b32-1.5.pdf e https://www.tcelectronic.com/broadcast-standards
- **EBU R128** (Europa; não anime-específico): **-23 LUFS ±0,5 LU, pico -1 dBTP** `[médio]` (fonte de terceiros).
- **INFERÊNCIA**: o programa deve ter **medidor BS.1770 (integrado, curta duração, LRA, true peak) com presets** (ARIB TR-B32, Netflix, EBU R128) e relatório de conformidade. É barato de implementar e evita rejeições.

### 4.5 HDR (Dolby Vision, HDR10) para anime

- **FATO** `[médio]`: Netflix só exige Dolby Vision em originais HDR; a produção de anime quase não usa HDR. **Inferência**: tratar HDR como v3; manter float interno e **não limitar cor a 0–1** para não fechar a porta.

---

## 5. ÁUDIO E EDIÇÃO

### 5.1 Fluxo no anime japonês

- **FATO** `[médio]` (entrevista da Sony Music, datada; texto da página truncado por codificação): ordem **アフレコ (gravação de dublagem à imagem) → efeitos sonoros e seleção/edição de música → MA/mixagem**; tudo digital em **Pro Tools (Mac)**; em TV a música é editada durante a produção (semanal), em OVA o mix pode ser feito numa sessão concentrada. https://www.sonymusic.co.jp/Animation/Kikaider/inta/int/index09.html
- **FATO** `[médio]` o 音響監督 (diretor de som) trabalha com o diretor em **casting, música e efeitos**; em alguns casos a música é encomendada **antes** do afureko; as etapas incluem afureko, efeitos, edição de música e **ダビング (dubbing/mix)**. https://realsound.jp/book/2025/03/post-1958046.html
- **FATO** `[médio]` MA: edição/limpeza de diálogo (整音), junção de sessões Pro Tools em um mix. https://www.neec.ac.jp/tuitionfreedat/syllabusdat/pdf/2020/20M2019.pdf
- **FATO** `[médio]` Sol Levante entregou **Atmos ADM/DAMF e sessões Pro Tools**; a trilha orquestral foi gravada com microfone 3D. https://opencontent.netflix.com/
- **DAWs**: **Pro Tools** (padrão do fluxo japonês `[médio]`), **Reaper** (comercial, avaliação 60 dias, licença desconto US$60 / comercial US$225), **Ardour 9** (GPLv2; tem **timeline de vídeo** embutida: **1 vídeo por vez, sem edição de vídeo**; sincroniza regiões por quadro). https://ardour.org/whatsnew-9.html e https://manual.ardour.org/video-timeline/ `[médio]`
- **CONHECIMENTO GERAL (não verificado)**: dublagem em anime japonês é feita **sobre vídeo quase final com cronometragem (queues / "wipes")**, e a **boca já está desenhada** antes da gravação; ao contrário do pre-recording típico de animação ocidental e 3D. Os atores gravam por trechos ("ringo" com marcas visuais) e o diretor de som valida. Validar em entrevistas de estúdio.

### 5.2 Lip-sync em anime

- **FATO** `[médio/baixo]` (artigo de Flash amador, **não de estúdio de TV**, portanto `[baixo]` para prática profissional): japonês cobre bem com **あ い う え お ん (5 vogais + fechada)**; inglês precisa de mais formas; **pre-recording (プレコ)** é típico de 3D de grande orçamento; no Flash limitado alterna-se **aberta/meia/fechada** em loop e congela-se no silêncio. https://news.mynavi.jp/techplus/article/kurieita-22/
- **FATO** `[médio]` pesquisa da IPA/Mito sobre gerar boca a partir de áudio e exportar em formato de **time remap** editável, notando que **reduzir naïvemente a fala real não fica "animesco"**. https://www.ipa.go.jp/archive/files/000059114.pdf
- **FATO** `[alto]` **Rhubarb Lip Sync** (CLI, Windows/macOS/Linux): 6 formas básicas **A–F** + extras **G, H, X (repouso)**; reconhecedor **PocketSphinx (inglês)** ou **fonético independente de idioma**; entrada WAV/OGG; saída **TSV/XML/JSON** e `.dat` para **Moho/OpenToonz**; integrações com **After Effects, Spine, Vegas**. https://github.com/DanielSWolf/rhubarb-lip-sync (licença/versão não confirmadas). **Papagayo-NG**: alinhamento manual de fonemas, projeto pouco ativo `[baixo]`.
- **INFERÊNCIA**: para anime, o mapa útil é **voz → timesheet de boca em 3–5 formas (japonês: vogais) com "hold" 2s/3s**, **editável**, e não animação contínua de visemas; Rhubarb serve como base de aproximação (mas o idioma japonês pede o modo fonético). Modelo de dados: coluna "boca" no xsheet com a forma por quadro e o texto da fala.

### 5.3 Edição de vídeo e conform

- **FATO** `[médio]` (CineD 2017/ITmedia): **EDIUS** e **Avid Media Composer** são citados como comuns na broadcast japonesa; **não achei** fonte específica de anime. **Premiere** aparece em usos genéricos. https://www.itmedia.co.jp/news/article/2402/03/1240203059/ `[baixo]`
- **FATO** `[médio]` **OpenTimelineIO (ASWF)**: núcleo em C++ e adaptadores em Python (EDL CMX3600 com ASC_CDL, FCP7 XML para Premiere, AAF da comunidade); desde v0.16 os adaptadores estão no pacote `OpenTimelineIO-Plugins`. https://github.com/AcademySoftwareFoundation/OpenTimelineIO e https://opentimelineio.readthedocs.io/en/latest/tutorials/adapters.html. Status exato 2026 **não confirmado**.
- **INFERÊNCIA**: o "editor" de anime é essencialmente **uma sequência de cuts (cada cut = um render ou uma composição)** com áudio; basta **timeline de cuts + áudio + marcadores + export EDL/OTIO/AAF**. Não é preciso um NLE completo.

### 5.4 Mixagem e canais

- **FATO** `[médio]`: Netflix exige **5.1** (2.0 opcional) e aceita Atmos; ARIB TR-B32 cobre medições para **5.1, 7.1, 22.2 e objeto**. Estéreo e 5.1 são os formatos úteis; Atmos/objetos são nicho.

### 5.5 O que entra no MVP e o que fica para depois

**MVP (v1)** `INFERÊNCIA`
- Importar áudio (WAV/FLAC/MP3), **timeline de áudio sincronizada à imagem com waveform**, scrubbing, vários tracks, ganho/pan/mute/solo, marcadores e **texto de diálogo ligado ao timesheet**.
- **Medidor de loudness BS.1770** (integrado, true peak, LRA) com presets **ARIB TR-B32, Netflix, EBU R128**.
- **Lip-sync assistido** (Rhubarb ou equivalente) → preenche a coluna de boca do timesheet, editável.
- **Mixdown estéreo e 5.1** (WAV 48 kHz/24 bits) e muxing de vídeo+áudio (via FFmpeg).
- Edição de sequência: justapor cuts, trims, transições simples (corte/fade/dissolve), **export EDL/OTIO**.

**Depois (v2+)**
- Gravação/afureko integrada (cue lines/wipes, takes, múltiplos atores, loops), **hospedar VST/CLAP**, efeitos de áudio (EQ, comp, reverb), biblioteca de SFX, **stems e Atmos/ADM**, **AAF para Pro Tools**, correção automática de loudness, sincronia com tempo de vídeo por LTC/MTC, editor de música/loops.
- **Não construir**: uma DAW completa; **delegar via exportação (AAF/OTIO/stems) para Pro Tools/Reaper/Ardour**.

---

## 6. Requisitos de performance consolidados (GPU vs CPU)

| Área | GPU | CPU/Disco | Observação |
|---|---|---|---|
| Composição de camadas e efeitos 2D | **Sim** (shaders, float16/32) | Decodificação de sequências, cache | Largura de banda de memória e VRAM manda; 4K com 100+ camadas é pesado |
| Blur/glow multi-escala | Sim (pirâmide) | — | Barato se bem feito; evitar kernels enormes diretos |
| Toon shading/contornos 3D | Sim (tempo real) | Preparação de malhas | Fácil em GPU; qualidade depende de dados por vértice |
| Render 3D offline (passes) | Sim/CPU | — | Cycles/Arnold/V-Ray; fora do escopo do MVP |
| Simulação FX (fumaça/fogo) | Sim (EmberGen) | Houdini CPU/GPU | VRAM grande (48 GB → 4x) |
| Áudio e medidor de loudness | — | Trivial | Latência baixa; tempo real em CPU |
| Codificação (H.264/H.265/ProRes) | NVENC opcional | CPU | Via FFmpeg |
| HDR/IMF | — | Disco/CPU | Validação com Photon |

---

## 7. Lacunas, contradições e o que verificar

1. **Crunchyroll**: nenhuma especificação pública encontrada.
2. **TV japonesa**: especificações de entrega por emissora (codec, resolução) **não encontradas**; ARIB TR-B32 tem alvo -24 LKFS confirmado; **tolerância (±1 vs ±2 LU) divergente entre fontes**. A versão **1.7** listada na página da ARIB (data 10/10/2026) precisa de checagem humana.
3. **Netflix**: páginas oficiais migraram para `studiopartner.netflix.net` e **não abriram** (apareceu um "Knowledge Hub" vazio). Os números vieram de resumos de busca das páginas antigas (v5.1 e Sound Mix v1.5). Confirmar na versão vigente.
4. **GDC do Strive** (Unreal Engine 4): sem material técnico encontrado. O handout do Xrd é o primário disponível.
5. **Polygon Pictures, Trigger, Science SARU, Sublimation**: sem documentação de pipeline obtida; matéria CGWORLD vol. 311 (Layout 3D) não lida.
6. **CGWORLD (prisma illya, houdini NPR, 画龍点睛)**: as páginas 2 carregaram sem corpo; usei os resumos. Vale ler os artigos originais para os valores concretos de gradação, ordem de efeitos e AOVs.
7. **SIGGRAPH Asia 2021 "To CG or not CG"** (PDF > 10 MB não foi lido): potencial fonte excelente sobre o pipeline de anime. Recomendo a leitura manual.
8. **Edição**: não achei fonte de qual NLE o anime japonês usa; o mais próximo é a broadcast em geral (EDIUS/Avid).
9. **Lip-sync em TV anime**: a prática (boca desenhada antes do afureko, folhas de boca/timesheet) **veio de conhecimento geral** e de um artigo de Flash amador; validar com entrevistas de animador/diretor de som.
10. **"Tsuya", "para/マルチ"** e outros termos de jargão de 撮影 não confirmados.
11. **AE 26.x**: a página oficial da Adobe deu 403; dados via ProVideoCoalition/resumo.
12. **Natron**: estado estável incerto; **Nuke 17** data 26/02/2026 vem de uma única notícia.
13. **Preços** (Sapphire, Red Giant, Resolve Studio) variam por fonte; conferir.
14. **Mistura em espaço de exibição vs. linear** (item 4.1): hipótese minha, precisa de teste A/B.

---

## 8. Fontes principais (por seção)

- Composição: https://cgworld.jp/terms/%E6%92%AE%E5%BD%B1.html · https://www.psoft.co.jp/en/product/celmx/ · https://www.psoft.co.jp/en/product/celfx/ · https://speakerdeck.com/olmdrd/matsuri2022-aeplugin · https://www.cgchannel.com/?p=89339 · https://psoft.co.jp/en/product/pencil/ae/ · https://psoft.co.jp/en/product/gallery/20260127-kaiju-no-08/ · https://learn.foundry.com/NUKE/content/reference_guide/draw_nodes/lightwrap.html · https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare/ · https://aescripts.com/deep-glow/ · https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html
- 3D: https://www.ggxrd.com/Motomura_Junya_GuiltyGearXrd.pdf · https://code.blender.org/?p=12670 · https://area.autodesk.jp/case/animation/kimetsu-01/ · https://speakerdeck.com/olmdrd/matsuri2020-toonfx · https://speakerdeck.com/olmdrd/matsuri2024-toonfx · https://asia.siggraph.org/2024/presenter/index581f.html
- Fundos: https://www.openculture.com/2021/01/a-look-inside-the-painting-process-of-the-studio-ghibli-artist-kazuo-oga.html · https://www.moguravr.com/quunplant-kaguyahime-3d-sample/ · https://cgworld.jp/special/cgwcc2024/detail/olm/ · https://www.blendernation.com/2024/10/11/creating-anime-backgrounds-in-blender-using-projection-mapping/
- Cor/entrega: https://opencontent.netflix.com/ · https://www.redsharknews.com/this-is-how-netflix-is-adapting-anime-to-modern-technology · https://backlothelp.netflix.com/hc/en-us/articles/7262346654995 · https://backlothelp.netflix.com/hc/en-us/articles/360001794307-Netflix-Sound-Mix-Specifications-Best-Practices-v1-5 · https://www.arib.or.jp/english/std_tr/broadcasting/desc/tr-b32.html · https://www.provideocoalition.com/after-effects-portal-june-2026/ · https://AnimeNewsNetwork.com/encyclopedia/lexicon.php?id=61
- Áudio/edição: https://github.com/DanielSWolf/rhubarb-lip-sync · https://manual.ardour.org/video-timeline/ · https://github.com/AcademySoftwareFoundation/OpenTimelineIO · https://www.sonymusic.co.jp/Animation/Kikaider/inta/int/index09.html · https://realsound.jp/book/2025/03/post-1958046.html
