# A1 — Pipeline de produção de anime no Japão (estado 2025–2026)

Data da pesquisa: 2026-10-15. Frente A1 da fase 1 (pesquisa) do projeto "programa único de produção de anime".

## 0. Como ler este documento

**Legenda de confiança**
- **[alto]**: fonte primária (relatório de governo/associação lido no PDF original, documento oficial do fabricante, entrevista com o próprio estúdio) ou dois ou mais achados independentes concordando.
- **[médio]**: fonte única plausível, resumo de imprensa, ou dado antigo (anterior a 2020) que provavelmente ainda vale.
- **[baixo]**: fonte fraca, indireta, promocional, ou vinda só de resumo de busca que não consegui abrir.
- **[inferência]**: conclusão minha, não afirmada por nenhuma fonte.
- **[conhecimento geral]**: termo ou prática que eu conheço do meio, mas que não consegui confirmar em fonte aberta nesta rodada. Tratar como hipótese a validar.

**Limites desta rodada (honestidade sobre o que falta)**
1. O orçamento de buscas web do turno acabou antes de eu cobrir tudo. Ficaram **fracos ou sem cobertura**: som (afureko/MA), edição, entrega/master, Madhouse, Wit, Sunrise/Bandai Namco Filmworks (só o sistema de gestão), Shaft (só um trecho de imprensa), e a estrutura do timesheet em si (colunas, 144 quadros por folha). Esses pontos estão marcados.
2. Várias páginas foram lidas por uma ferramenta que devolve resumo gerado, não o texto cru. Para as fontes mais importantes eu baixei o PDF e extraí o texto: o relatório METI/AJA de mar/2023 (000254.pdf), os slides METI de jan/2025, o deck da Celsys de 2018 e o caso Wacom/Trigger. Essas citações são de leitura direta.
3. Parte do material é **conteúdo patrocinado ou de fabricante** (entrevista Celsys sobre "Kado" marcada [PR], matéria ufotable com HP/NVIDIA, casos Wacom, recrutamento da Toon Boom, site do OpenToonz). Está sinalizado onde aparece.
4. Os kanji/romanizações dos termos vêm em parte de fontes japonesas lidas e em parte do meu conhecimento; a coluna "origem" distingue quando importa.

---

## 1. Panorama: a estrutura que condiciona o pipeline

**Três blocos** (planejamento/pré-produção, produção, pós-produção) é a divisão usada tanto por material do governo quanto por imprensa: pré = roteiro, storyboard, design de personagem e arte; produção = genga, douga, fundos, shiage, satsuei; pós = edição, som, entrega. [alto] — slides METI jan/2025 (https://www.meti.go.jp/shingikai/mono_info_service/entertainment_creative/pdf/003_04_02.pdf) e animeanime https://animeanime.jp/article/2022/02/04/67348.html.

**Estrutura industrial** [alto, METI jan/2025, https://www.meti.go.jp/shingikai/mono_info_service/entertainment_creative/pdf/003_04_02.pdf]:
- Modelo de comitê de produção (製作委員会 seisaku iinkai) financia; o estúdio "motoukē" (元請, contratante principal) recebe a verba de produção e subcontrata (下請) seções inteiras (douga, shiage, satsuei, 3DCG) e freelancers individuais. O METI descreve isso como "múltiplas camadas de subcontratação, baixa margem".
- O estúdio de produção (só produção) fica em ~10% do faturamento total do mercado de anime. ~31% das produtoras operam no prejuízo; entre contratantes principais, 34%; entre estúdios de especialidade subcontratados, 27%.
- Mão de obra: 49% empregados, 30,8% freelancers. Renda média de quem faz anime 4,555 milhões de ienes/ano (pesquisa de 2022); 20–24 anos: 1,966 milhão. Jornada média 2.379–2.623 h/ano. [alto]
- O número de séries de TV por ano oscila na faixa dos 300 desde 2014 (pico em 2016); a oferta é limitada por falta de gente, não de verba. [alto]

**Escala do trabalho** (para dimensionar um software):
- ~300 cuts (カット) por episódio de TV de 30 min. [médio: aparece em dois resumos, ver https://shingakunet.com/journal/career/20230603000001/ e o blog antigo https://washiblog.wordpress.com/2011/01/18/anime-production-detailed-guide-to-how-anime-is-made-and-the-talent-behind-it/; a mesma fonte antiga fala em ~3.000 desenhos por episódio e ~20 genga-man]
- "~4.000 ou mais desenhos coloridos por episódio" segundo o material da Sony/CloverWorks de 2024. [médio-alto, https://www.itmedia.co.jp/news/article/2409/06/1240906086/2]
- Volume de dados num episódio na CloverWorks: ~300–400 GB; fundos de 200–300 MB cada, centenas por episódio; scans de vários MB por folha, até dezenas de milhares por episódio; uma simulação 3D pode passar de 1 TB. [médio-alto, entrevista do dirigente da CloverWorks, https://www.gizmodo.jp/article/hpe-directplus-online-store/ — de 2021]
- Escala de um estúdio verticalizado: Kyoto Animation roda ~6–8 key animators por episódio (Dragon Maid S, média de menos de 7); o estudo citado no mesmo texto fala de ~9 animation directors e ~33 key animators em episódio "típico" do mercado. [médio, https://blog.sakugabooru.com/2021/10/31/rebuilding-kyoani-unique-pipeline-miss-kobayashis-dragon-maid-s-production-notes/]

### 1.1 O cut (カット) como unidade de trabalho

- O cut é a unidade de planejamento, atribuição, cobrança e rastreio. Cada cut tem número no episódio, e o ciclo todo (layout→genga→douga→shiage→satsuei) é controlado por cut. [alto: relatório METI/AJA mar/2023 e a prática do studio MOTHER abaixo; o relatório define cut bag, cut table, progress sheet]
- **Papel**: um saco de papel, o **カット袋 (kattobukuro)**, carrega layout, genga, douga e a folha de timing de cada cut; a produção (制作進行 seisaku shinkō, 制作デスク) controla entrada/saída ("IN/UP") numa **カット表 (tabela de cuts)**, **進行表 (planilha de progresso)** e **集計表 (planilha de totais/valores)**. [alto, https://www.meti.go.jp/meti_lib/report/2022FY/000254.pdf, p. 59–63]
- **Digital**: o saco virou **pasta no servidor**; num caso documentado (studio MOTHER, TV "Tensei Kizoku, Kantei Skill de Nariagaru", ACTF 2025) a pasta do cut contém exatamente **dois arquivos: .clip (Clip Studio Paint) e .tdts (timesheet digital Toei)**; o arquivo se chama p.ex. `Ks_13_002_LO` (série, episódio 13, cut 002, etapa LO), e a etapa seguinte pega os dois arquivos sem exportar nada. [alto, https://cgworld.jp/article/actf25-mother02.html]
- Dentro do .clip, a camada de "cut_info" no topo substitui o bilhete do saco de papel, e as camadas de baixo para cima são BG (fundo), LO (layout), LOEN (checagem de layout pela direção/演出), LOSA (checagem do diretor de animação/作監), G (genga). As camadas "S" (fala) e "Camera" são criadas automaticamente quando o .tdts é importado. [alto, mesma fonte] Isto mostra o **fluxo de aprovação embutido na estrutura de camadas**.
- Nomenclatura de arquivo e regras de resolução são **por empresa, por obra e por etapa**: resolução prática 150–200 dpi, tamanhos de safe-frame, furos de pino (tap) para impressão, tipo de linha (raster/vetor), estrutura de camadas e padrão de numeração do export. O conjunto circula como "注意事項" (instruções) distribuídas obra a obra. [alto, METI mar/2023 p. 59–62]

---

## 2. Documentos centrais: o timesheet (タイムシート) e o "tsuke"

### 2.1 O que é e por que é central
- O timesheet é uma tabela que expressa **o timing em número de quadros** (1 s = 24 divisões); concentra o número de genga e douga, como as cels se sobrepõem, o movimento de câmera, as instruções de repasse para o shiage e os efeitos de satsuei. É consultado por animadores, shiage e satsuei. [alto, CGWorld glossário https://cgworld.jp/terms/タイムシート.html]
- "A direção do anime depende muito do timesheet" (declaração de um animador/diretor na entrevista Celsys/Toei). Mesmo digital, na prática era "bitmap em PSD com anotações à mão" até 2017, ou seja, "analógico digitalizado". [médio: a entrevista é marcada [PR], https://cgworld.jp/interview/201706-celsys-kado-2.html]
- O timesheet é usado em layout, genga, douga, shiage e satsuei como instrução de timing das imagens e de direção da câmera. [alto, METI mar/2023 p. 55]
- Dados que o timesheet precisaria carregar (lista da entrevista Toei/Celsys de 2017): episódio, cena, número do cut, tamanho do quadro e resolução, margens, ordem de sobreposição de camadas, timing, timing das falas, deslocamentos de câmera. [médio]
- **Formato físico** (colunas AÇÃO / cels A, B, C… / BG / FALA / CAMERA, 1 folha = 6 s = 144 quadros, dividida em blocos de 3 s): [conhecimento geral, parcialmente corroborado]. O app oficial da Toei v3.8.0 adicionou "formato de 3 segundos" para impressão, e permite até 8 colunas de SOM e 8 de CÂMERA, o que sugere que o layout padrão segue essa estrutura. [alto para o app, https://clipstudio.net/ja/dl/toeianimation] Não consegui confirmar o 144 quadros em fonte aberta nesta rodada.

### 2.2 "Tsuke" (付け)
- O termo que o briefing menciona ("tsuke") não apareceu com esse nome em nenhuma das fontes abertas. O que as fontes mostram é a **função**: o animador de genga (原画マン) escreve o timing no timesheet e o diretor/diretor de animação o corrige ("2 para 3", "mais perto do 3"), e o ciclo deve circular entre os apps. [alto para a função, deck Celsys 2018 em que o exemplo de uso é "コレ→演出指示「2は3寄りに」→作監修正"; a palavra "tsuke" [conhecimento geral]: タイミング付け / タイムシートを付ける]. Quem decide o timing inicial varia (演出/storyboarder no e-conte, animador no layout, ou o próprio genga-man); não consegui fonte aberta para a divisão exata. [baixo]

### 2.3 Estado da digitalização do timesheet
Fatos [alto, salvo indicação]:
- **Ferramentas de desenho digital NÃO têm timesheet nativo**: Clip Studio Paint, TVPaint e Harmony controlam as imagens por **linha do tempo (timeline)**; só o RETAS STUDIO Stylos tinha função de timesheet. Isso é apontado como a reclamação mais antiga dos usuários aos fabricantes. [METI/AJA mar/2023 p. 55–56, 68]
- **Ponte aberta**: o **xdts** (Celsys, desde 2019; "formato aberto independente de software", texto/JSON-like, sem pixels, só nomes de cels, tempos, marcas) e o **Toei Digital Timesheet (TDTS)**, app gratuito (Windows, mais versão iPad) que mostra a timeline do CSP no leiaute de folha de papel. Versão atual **3.8.0 de 2025-01-28**. [alto, https://www.celsys.com/en/clipsolution/xdts/ e https://clipstudio.net/ja/dl/toeianimation]
- Apoio declarado ao xdts: Celsys/CSP, Toei, Nippon Animation, studio MOTHER, OpenToonz (XDTS Viewer); terceiros relatam suporte em TVPaint, Callipeg, Odyssey, Open Cell Animation (via conversor). [médio-alto, página Celsys + https://doc.praxinos.coop/v5.X.58/CollaborativeWork/XDTS.html + https://wolfinabowl.itch.io/oca-to-xdts-converter]
- O TDTS também exporta timing para o **After Effects** (satsuei). [alto, METI mar/2023 p. 55]
- **Adoção real é parcial**: muitos estúdios que desenham em CSP **continuam em timesheet de papel** porque "operar o digital parece complicado e a mão é mais segura". 3DCG de jogo nem usa timesheet. O relatório conclui que o futuro do timesheet é incerto: ou fica em papel, ou vira digital com E/S entre ferramentas, ou desaparece com a timeline. [alto, METI mar/2023 p. 55–56]
- Alternativas online: **UAT** (Universal Animation Timesheet, Arch + Yokohama Animation Lab, 2019, derivado do "りまぴん" de Nekomataya), navegador, aceita xdts e papel híbrido; alerta do próprio expositor: perde-se o backup físico e o hábito de conferir cada material. [médio-alto, https://cgworld.jp/feature/201903-actf-01-2.html]
- Roadmap Celsys 2018: CSP ganharia também câmera 2D com keyframes, entrada de áudio e exportação de vídeo com som para "senga-tori" (線撮) e previz. [alto, deck Celsys 2018]
- Ver. 5.0 do CSP (2026-03-11): no que toca animação, só "criação simples de animação" no Modo Simples com onion skin; nada novo sobre timeline/timesheet nas notas lidas. [médio-alto, https://www.clipstudio.net/ja/news/202603/11_01/]

**Inferência [inferência]**: o timesheet é o "contrato" entre direção, animação, shiage e satsuei, e a ausência de um modelo comum entre ferramentas é a principal quebra de continuidade do pipeline digital. Um programa único que tenha timesheet como estrutura de dados de primeira classe (e não como arquivo auxiliar) elimina a categoria inteira de problemas de importação/exportação.

---

## 3. Etapa por etapa

Formato de cada etapa: **Quem / Entra→Sai / Ferramenta e funções realmente usadas / Dores / Revisão e aprovação / Papel→digital**.

### 3.1 Planejamento (企画 kikaku) e comitê de produção (製作委員会)
- **Quem**: produtor (プロデューサー), editora/dona da obra, empresas do comitê (TV, gravadora, brinquedo, streaming), estúdio. O produtor cuida de planejamento, financiamento, orçamento, marketing, merchandising; no estúdio negocia custo, escolhe equipe e responde pela entrega. [médio, resumo de https://imitsu.jp/matome/movie/anime-proposal e outros; blog antigo https://washiblog.wordpress.com/2011/01/18/anime-production-detailed-guide-to-how-anime-is-made-and-the-talent-behind-it/ dá ~metade do orçamento ao estúdio]
- **Entra→Sai**: proposta (企画書), orçamento, equipe-chave, cronograma, contrato de comitê. Formato: documentos de escritório (Word/Excel/PowerPoint/PDF) [médio]. Não há ferramenta especializada documentada aqui.
- **Dores**: direitos complexos, tomada de decisão lenta, produtora quase nunca fica com PI e por isso tem pouca receita secundária. [alto, METI jan/2025] A entrada de streaming global (OTT) muda o poder de negociação das produtoras. [alto, mesma fonte]
- **Revisão**: reuniões de comitê, aprovação do produtor/licenciador. [médio]
- **Papel→digital**: já digital (documentos). O "Save Point for アニメ" (MUGENUP) foi usado por Davidproduction já no pré-produção (planejamento/setting/preparo de storyboard). [médio, https://cgworld.jp/news/event/1912-savepointfor.html e artigo ligado]

### 3.2 Composição da série (シリーズ構成 shirīzu kōsei) e roteiro (脚本 kyakuhon / "honpon")
- **Quem**: compositor de série (série), roteiristas por episódio, diretor, produtor, às vezes o autor original. [médio-alto, blog Washi 2011 + resumos de busca]
- **Entra→Sai**: sinopse por episódio, depois roteiro completo, tipicamente após 3–4 versões. [médio, blog Washi]
- **Ferramenta**: não achei fonte aberta sobre software específico de roteiro; Word/Excel é o que o relatório METI assume para a camada "alfabetização digital básica". [baixo para ferramenta; alto para a alfabetização básica, METI mar/2023 p. 31]
- **Dores**: não documentadas nas fontes; [inferência] roteiro e storyboard hoje vivem em arquivos separados sem vínculo com cuts/timing.
- **Revisão**: reuniões de roteiro (diretor, composição de série, produtor, autor). [médio]

### 3.3 Design de personagem, mecha e arte (設定 settei; キャラクターデザイン; 美術設定; 色彩設計/色指定 iro-shitei)
- **Quem**: character designer (キャラクターデザイナー), mecha/prop designers, **diretor de arte (美術監督 bijutsu kantoku)**, **designer de cor (色彩設計 shikisai sekkei)** — que define a paleta por personagem e por situação (dia, noite, interior, flashback...) e produz o **カラーモデル / 色指定表**. [médio, descrição do papel em https://www.bunka.go.jp/animeseminar/2025/course/course06.html: "responsável final por qual cor usar no shiage"; detalhes da tabela são conhecimento geral]
- **Entra→Sai**: model sheets (turnarounds, expressões), props, ambientes, arte-conceito/pranchas de cor, tabela de cores por personagem/situação (a famosa "tabela de cores" que o shiage segue); no digital, paleta/swatches em arquivo. [conhecimento geral + médio]
- **Ferramenta**: Photoshop/CSP para pranchas; para cores, no shiage digital a paleta é carregada em RETAS PaintMan/CSP/OpenToonz. Um caso do OpenToonz usa a função Palette para color design. [médio, https://opentoonz.github.io/e/usecase.html]
- **Dores**: o shiage/cor depende de **trabalho de checagem de cor por cut** ("色指定検査 iro-shitei kensa"), feito por especialistas; em obra independente que pediu ~30 modelos de cor por cena em vez dos ~5 comerciais, a carga foi muito maior. [médio, https://animeanime.jp/article/2024/10/31/87413.html]
- **Revisão**: aprovação do diretor/diretor de arte/produtor; checagem de cor por cut mais adiante (3.10).
- **Papel→digital**: digital há muito (cor sempre foi uma das primeiras etapas a digitalizar).

### 3.4 Storyboard (絵コンテ e-conte) e animatic/storyboard em vídeo (ビデオコンテ / ムービーコンテ)
- **Quem**: **コンテマン** (storyboarder), muitas vezes o próprio diretor de episódio (演出); no TV, geralmente 1 pessoa por episódio ou poucas. [médio-alto, resumos https://shingakunet.com/... e Storyboard Pro https://cgworld.jp/special-feature/202304-toonboom-cgwcc.html; "~3 semanas por episódio" é do blog antigo de 2011 [baixo-médio]]
- **Entra→Sai**: roteiro → e-conte em folha A4/A5 com colunas (nº de cut, desenho/layout, ação, fala, tempo em segundos+quadros). No TV padrão ~300 cuts. O orçamento de desenhos por cut é planejado aqui para controle de custo. [médio, blog Washi 2011]
- **Ferramenta típica hoje**:
  - Papel ainda majoritário, mas movimentos para digital: na CloverWorks, em 2021, o dirigente estimava ~30% do storyboard digital e ~70% à mão. [médio, https://www.gizmodo.jp/article/hpe-directplus-online-store/]
  - **Toon Boom Storyboard Pro** é a ferramenta mais citada para ムービーコンテ no relatório METI/AJA 2023 e em casos (Shinkai: "Suzume" foi montado em vídeo-conte com música e falas dentro do Storyboard Pro; ~2000 cuts no longa). [alto para a citação METI; https://www.meti.go.jp/meti_lib/report/2022FY/000254.pdf p. 67, 86]
  - **Clip Studio Paint** também é usado (Marza Animation Planet usa o CSP em todas as 4 etapas: roteiro-reel, painéis 1920x1080, folha de shot, workspace de storyboard; ~1.000 painéis por episódio de 10 min). [alto, https://cgworld.jp/article/202404-ACTF-marza.html]
  - Blender chega a ser usado para storyboard/layout 3D. [médio, METI mar/2023 p. 67]
- **Dores**: janelas de CSP genéricas poluem a tela do storyboarder; alinhamento manual de painéis na folha de shot; poda manual de painéis; custo de mudança de roteiro/direção tardia. [alto, Marza]
- **Revisão**: reuniões de arte e produção, diretor de episódio e diretor de série aprovam. [médio]
- **Papel→digital**: ainda misto; o vídeo-conte muda "como o timesheet e a checagem são feitos" depois dele, sem padrão único. [alto, METI mar/2023 p. 41]
- **Ligação com timing**: o vídeo-conte é o primeiro lugar onde o timing real existe; [inferência] se o conte entrar no sistema como timeline com marcações por cut, o timesheet pode ser **derivado** dele em vez de redigitado.

### 3.5 Reunião de produção (作画打ち合わせ) e layout (レイアウト)
- **Quem**: diretor de episódio (演出), animadores de layout/genga sênior, diretor de animação (作画監督, 作監), produção. O diretor/aprovador revisa o layout. [médio-alto, blog Washi 2011 + Sakuga Blog glossário]
- **Entra→Sai**: e-conte → layout (composição, posição dos personagens, câmera, a "estrutura" da animação) escrito em papel de animação com furos de pino; inclui timing aproximado; aprovado e copiado para fundos e genga. [médio]
- **Ferramenta**: layout em papel ou digital (CSP etc.); layout 3D (em 3ds Max/Blender/Maya) vem crescendo para cuts com cenário 3D: na MAPPA, para cuts com cenário 3D, "na maioria das vezes até cuts de desenho à mão têm layout feito em 3D". [médio-alto, https://cgworld.jp/feature/202104-why3dcg-anime.html]
- **Dores**: diversidade de tamanho/resolução entre etapas; mudança tardia de enquadramento; "3D layout" cria obrigação de cenário 3D por obra. [médio]
- **Revisão**: **LOEN (演出) e LOSA (作監)** como camadas no .clip, conforme a estrutura do studio MOTHER. [alto, https://cgworld.jp/article/actf25-mother02.html]
- **Papel→digital**: [alto, METI mar/2023] mesmo com digital, **layout→genga→douga** mantém a mesma ordem do papel, o que preserva compatibilidade com papel e repasse ao shiage.

### 3.6 Genga (原画) e segunda genga (第二原画 nigenga / 2nd key)
- **Quem**: genga-man (原画マン), **diretor de animação (作画監督 sakkan)** que corrige o desenho e a consistência do modelo, **diretor-chefe de animação (総作画監督)**, e **第二原画** (2ª genga), que limpa/refina a genga rascunhada de outro animador. [alto, Sakuga Blog https://blog.sakugabooru.com/2019/08/31/the-fragmentation-of-anime-production-too-many-cooks-spoil-the-broth/ e gamebiz 2026 https://gamebiz.jp/news/427905]
- **Fato quantitativo**: a 2ª genga foi formalizada com *RahXephon* (2002) e, no verão de 2019, **as 35 séries de TV novas tinham 2ª genga em praticamente todos os episódios**; o trabalho migrou de "limpeza" para "completar rascunhos muito soltos", em pouco tempo. [médio, Sakuga Blog, baseado em créditos, não em dados sistemáticos]
- **Entra→Sai**: layout aprovado + timesheet → poses-chave com timing e "inbetween chart" (instruções de espaçamento) → correções do 作監 → aprovação do diretor/演出. Formato: papel A4/B4 com furo de pino ou .clip/.tdts em pasta de cut. [alto]
- **Ferramenta**: papel+lápis ainda dominante em genga freelancer; digital: **Clip Studio Paint EX (domínio mais forte)**, também TVPaint, Toon Boom Harmony, Adobe Animate (Science Saru), e OpenToonz para pencil test (Ghibli, Trigger, Madhouse, Chizu). [alto, METI mar/2023 p. 67 + https://opentoonz.github.io/e/usecase.html + Adobe https://blog.adobe.com/en/publish/2020/05/21/how-science-saru-animation-studio-is-redefining-the-japanese-animation-industry]
- **Funções do CSP realmente citadas**: pastas de camada por etapa, timeline (ligada ao TDTS), onion skin, light table, "finger flip" (指パラ), modelos de template por estúdio, câmera 2D por keyframes, áudio na timeline. [médio-alto, https://clipstudio.net/ja/animation/]
- **Dores**: o CSP é ferramenta de ilustração/mangá "não otimizada para anime": o animador vê funções que não precisa; falta timesheet; o 作監/演出 não tem software próprio para checar (comentário livre numa pesquisa JAniCA: "há desenho digital arrumado, mas falta software adequado ao trabalho do 演出, então algumas tarefas perdem eficiência"). [alto para o CSP/AnimeCanvas; médio para o comentário JAniCA, https://www.janica.jp/survey/survey2023Report.pdf]
- **Escassez**: faltam veteranos de genga e de douga; a terceirização da douga ao exterior cresce e tira de novatos a chance de treino, o que impede formar genga: "círculo vicioso". [alto, METI mar/2023 p. 43]
- **Revisão**: cada cut volta ao 作監 (e ao 総作監), e o diretor de episódio checa; correções em vermelho viram camada no digital. [médio-alto]
- **Papel→digital**: 2023: das 15 empresas de desenho 2D consultadas pelo METI/AJA (11 contratantes e 4 subcontratadas, entre entrevistadas e membros do comitê), só 4 contratantes, 1 estúdio e 2 subcontratadas tinham toda a produção interna de desenho digital; 2 contratantes+1 subcontratada faziam também a checagem em digital ("extremamente raro"); nenhum animador individual entrevistado trabalhava sempre em digital. [alto, METI mar/2023 p. 39]
- **Casos**: Trigger/"Promare": dos 700 cuts de pós-genga, ~150 foram em digital; meta seguinte: um episódio inteiro digital. [alto, Wacom https://tablet.wacom.co.jp/biz-design/pdf/casestudy_trigger.pdf, 2019]

### 3.7 Douga (動画 dōga, in-betweens) e verificação (動画検査 dōga kensa)
- **Quem**: douga-man (動画マン), muitas vezes iniciantes; **動画検査 (dōga kensa)** é o checador de douga/inspetor, uma especialização de carreira; muito terceirizado, e o exterior (China, Coreia, Vietnã, Malásia) responde por parte crescente. [alto, METI mar/2023 p. 39–43]
- **Entra→Sai**: genga aprovada + timesheet → traço limpo e in-betweens (中割 nakawari) → douga verificada. Em digital, o douga e o shiage podem ser feitos continuamente por uma mesma pessoa em CSP. [alto, METI mar/2023 p. 41]
- **Ferramenta**: CSP; **CACANi** (inbetween assistido) é usado por alguns estúdios para fazer uma "sala de douga" digital, inclusive com 動画検査. [alto, METI mar/2023 p. 41]
- **Produtividade medida**: meta de 800 folhas/mês por pessoa (8 h/dia) em douga+shiage, real ~500 folhas/mês. Em hub no exterior: ~100 pessoas, meta de 40 mil folhas/mês. [alto, METI mar/2023 p. 40]
- **Dores**: "desenha detalhe demais no digital, eficiência cai" (relato de animadores); mistura papel+digital: **custo de conversão de ~2,2 milhões de ienes/mês** para quem mantém a produção de um episódio contínuo (estimativa de uma empresa no caminho do full digital). [alto, METI mar/2023 p. 40–42]
- **Revisão**: dōga kensa → 作監/ produção. [médio]
- **Papel→digital**: a douga digital cresce mais rápido porque novatos já saem da escola desenhando em digital; o relatório diz que a China fez a virada pós-COVID e "hoje faz tudo em digital". [alto, METI mar/2023 p. 39]

### 3.8 Shiage (仕上げ, acabamento/pintura) e cor (色指定)
- **Quem**: **色指定 (iro-shitei)** aplica o plano do designer de cor a cada cut, define as cores por região; **仕上げ (shiage)** pinta; **色トレス** limpa/ajusta o traço colorido; **仕上げ検査 / 色指定検査** confere. Muito terceirizado: "o shiage é quase sempre subcontratado, mas há tendência de internalizar". [médio, resumos de https://www.bunka.go.jp/animeseminar/2025/course/course06.html e outros; detalhes de tarefas = conhecimento geral]
- **Entra→Sai**: douga limpa (scan ou arquivo digital) + tabela de cor → cut pintado, em camadas (linha, cor, sombra, highlight) → imagem sequencial. [conhecimento geral]
- **Ferramenta**: **RETAS STUDIO PaintMan** foi o padrão por décadas; hoje tem **desenvolvimento encerrado**, não acompanha o Windows e se limita a resoluções que "fazem upconvert" para HD (não serve para além de HD). [alto, METI mar/2023 p. 68; Sony/ITmedia 2024: "cerca de 10 anos sem atualização"] Alternativas: CSP (colorir com fill), OpenToonz, **AnimeCanvas COLOR** (em desenvolvimento). CloverWorks ainda dizia usar RETAS em 2021. [médio, Gizmodo]
- **Dores**: **correção de TP** (misalignment e linhas apagadas na folha escaneada) antes de pintar; dependência de software abandonado; checagem de cor em volume. [alto, ITmedia 2024]
- **Revisão**: 色指定検査/仕上げ検査 → produção → diretor.
- **Papel→digital**: cel pintado à mão acabou em torno de 2013 (quase "extinto"), o digital shiage começou com RETAS! PRO em 1993. [médio, fonte de resumo + ITmedia 2024]

### 3.9 Fundos / arte (美術 bijutsu, 背景 haikei)
- **Quem**: **diretor de arte (美術監督)** + equipe de fundos; **美術ボード** (pranchas de ambientação), **美術設定** (design de cenário). [médio]
- **Entra→Sai**: layout aprovado → fundo pintado e entregue em arquivo grande (200–300 MB cada na CloverWorks, centenas por episódio) → composto na satsuei. "背動" (fundos em movimento), antes pintados quadro a quadro, hoje frequentemente 3D. [alto, Gizmodo]
- **Ferramenta**: **Photoshop é o padrão** (CloverWorks, Colorido); Ghibli ainda pinta fundos em papel com guache e digitaliza. [alto, https://www.filmcomment.com/art-and-craft-atsushi-okui-on-the-boy-and-the-heron/] 3D cresceu: OLM Digital/"Kusuriya" fez fluxo de fundo 3D + layout 3D + guia de desenho. [médio, anúncio de sessão, https://cgworld.jp/special/cgwcc2024/detail/olm/; detalhes técnicos não verificados] MAPPA tem equipe de arte que desenha cenários, texturas para modelos 3D e artes-conceito. [médio-baixo, https://cgworld.jp/special-feature/xencelabs-mappa-202408.html]
- **IA**: Toei Animation + Preferred Networks experimentaram ferramenta "Scenify" que converte fotos em fundos animáveis (2021) [médio, https://cgworld.jp/news/other/2103-pfnaipreferred.html — só resumo de busca].
- **Dores**: fundos pesados; coerência de cor com shiage/satsuei; para obras de estilo muito iluminado, a cor do cel deve ser planejada junto. [médio]
- **Papel→digital**: fundos migraram para digital no fim dos 90–2000s (Mononoke e "Moribito" foram marcos citados). [médio, resumo de busca]

### 3.10 Efeitos (エフェクト作画) e 3DCG integrado
- **Efeitos desenhados**: animadores de efeito (fogo, água, explosões) desenhados à mão ou em 2D digital; depois "ligados" pela satsuei. [conhecimento geral; Ghibli menciona CG combinado a efeitos desenhados]
- **3DCG**: pipeline modelagem → rigging → animação → efeitos (→ composição quando o estúdio finaliza a imagem); no híbrido 2D/3D, o resultado costuma ser entregue como imagem à satsuei, com vídeo de referência opcional. [alto, METI mar/2023 p. 50]
- **Ferramentas**: **3ds Max** é o mais comum em anime, **Maya** em estúdios como STUDIO4°C, **MotionBuilder** para poses, **ShotGrid** para progresso/versões/checagem do diretor, **Blender** crescendo, Houdini para efeitos/água. [alto, https://cgworld.jp/feature/202104-why3dcg-anime.html + METI mar/2023 p. 67]
- **Dores**: **descompasso 2D×3D** de cronograma (3D é alocado por pessoa-mês numa obra; 2D, por cut, com animadores em várias séries) e de expectativa de diretor/作監 (a 3D não segue o timesheet); falta de compreensão mútua; cada novo estilo exige refazer o pipeline; rosto e dedos em close ainda são fracos em cel-look 3D. [alto, METI mar/2023 p. 52–53 e CGWorld 2021 sobre MAPPA]
- **Revisão**: diretor/演出 no ShotGrid ou similar. [médio]
- **Exemplos**: MAPPA (CGI部 interno, 3D layout para a maioria dos cuts 3D, ShotGrid); ufotable (departamento digital com 3ds Max+V-Ray, Deadline, EmberGen, USD "um arquivo por cut"); CloverWorks (Blender, Houdini, 3ds Max). Ver §4.

### 3.11 Satsuei (撮影, fotografia/composição digital)
- **Quem**: diretor de fotografia (撮影監督), equipe de satsuei (às vezes subcontratada, p.ex. Asahi Production ou TROYCA), às vezes departamento de VFX. [médio-alto]
- **Entra→Sai**: cut pintado (imagens) + fundos + 3D + timesheet/câmera + **撮影指示 (satsuei shiji)** → imagem final do cut com câmera, brilho, desfoque, partículas, filtros. Termo de cel-era "satsuei" vem de literalmente fotografar cels no estande. [alto para a definição, gamebiz 2026 https://gamebiz.jp/news/427905]
- **Ferramenta**: **After Effects** é o padrão por larga margem; DaVinci Resolve para codificação (ufotable); Houdini para efeitos de água/fogo; plugin e scripts próprios. O TDTS exporta o timing para o AE. [alto, METI mar/2023 p. 67–68; https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html (conteúdo patrocinado)]
- **Operação real**: na satsuei chegam materiais de tamanhos diferentes (desenho, arte, 3D) e a equipe reajusta por tamanho; "a satsuei casa com o quadro do desenho". [alto, METI mar/2023 p. 84 resumo de seminário, OLM]
- **Fluxo de checagem no longa do Shinkai ("Suzume", ~2000 cuts)**: tanto papel quanto digital são "filmados" só para o diretor checar em vídeo (Premiere Pro); em vez de all-rush pós-satsuei, o diretor escreve a **撮影指示書** *antes* da satsuei, sobre quadros recortados do vídeo; **retakes** nascem aí. [alto, METI mar/2023 p. 86]
- **Dores**: carga crescente (direção quer mais detalhe); servidores críticos no fim do cronograma; falha de render/servidor é risco alto porque a satsuei é o último elo e vive atrasada. [alto, Gizmodo/CloverWorks]
- **Papel→digital**: a satsuei foi digital desde 2000s; hoje a questão é a integração com 3D (scripts que passam câmera 3ds Max→AE, e USD como ponte). [alto, ufotable]

### 3.12 Edição (編集 henshū)
- **Quem**: editor (編集), diretor, produtor; edição offline (corte de cuts em vídeo, montar timing da história) e online (versão final com efeitos, créditos, correção de cor). [baixo-médio; descrição genérica de edição de vídeo em https://www.maff.go.jp/j/nousin/seko/kyotu_siyosyo/k_tokubetu/attach/pdf/index-202.pdf e vaga da Sony PCL]
- **Entra→Sai**: cuts finalizados em imagem (do satsuei) → episódio montado em tempo de transmissão (尺, duração) → versão "branca" (白箱, p/ conferência) → "初号" (primeira cópia de visualização) → "完パケ" (pacote final). [conhecimento geral — **não confirmado** nas fontes abertas lidas; apenas os termos de dados "編集データ, 音響データ, リテイクデータ, 完パケデータ, 白箱データ" aparecem como dados gerados na pós-produção em material governamental citado por resumo de busca]
- **Ferramenta**: Premiere Pro aparece no fluxo de checagem do Shinkai; DaVinci Resolve na ufotable. Pós-produtoras (Sony PCL, IMAGICA) fazem online, remaster, conversão e QC. [médio-baixo]
- **Particularidades**: **Kyoto Animation criou sala de edição própria** e passou a fazer a pós de som internamente. [médio, https://blog.sakugabooru.com/2023/12/30/the-eyecatch-kyoto-animations-quest-to-make-their-staff-comfortable-spy-x-familys-tricky-workload-management-and-charming-pokemon-spinoffs/ — só resumo de busca]
- **Lacuna**: [baixo] não achei fonte que descreva dores específicas da edição de anime.

### 3.13 Som (音響 onkyō): afureko, dublagem, trilha, mixagem
- **Quem**: **音響監督 (onkyō kantoku)** — comanda casting, interpretação dos dubladores, efeitos e seleção de música; **アフレコ (afureko)** = gravação de dublagem sobre imagem; **効果 (efeitos)**, **選曲 (seleção/edição de trilha)**, **ミキサー (mixagem)**, **MA/ダビング (mixagem final)**. Em algumas casas (descrição da Toei) o diretor de episódio também faz papel de diretor de som; em outras o diretor de som cuida de tudo. [médio, resumos de https://www.weblio.jp/content/音響監督 e https://www.sonymusic.co.jp/Animation/Kikaider/inta/int/index09.html — só resumo de busca]
- **Ordem típica**: AR (afureko, falas) → efeitos e música sobre o vídeo só de falas → MA com mixador → entrega. [médio, mesmo resumo] A gravação costuma ocorrer **antes** da imagem estar pronta, com "線撮" (vídeo de linha) ou animatic; por isso o tempo de fala vem do timesheet/animatic. [conhecimento geral + o fato de o deck Celsys 2018 citar o uso de CSP para gerar vídeo "senga-tori"/previz com som]
- **Ferramenta**: Pro Tools como padrão de pós; troca por OMF/AAF e vídeo MOV/mp4; entrega WAV 44,1/48 kHz 16/24 bit, loudness -24 LKFS como referência comum no Japão. [baixo, página comercial de prestador de serviços, https://www.lancers.jp/client/sounds] Dolby Atmos aparece como serviço de estúdio. [baixo]
- **Dores**: pós-COVID, sessões de afureko duraram o dobro do tempo, com grupos pequenos em revezamento. [baixo-médio, resumo de busca; também citado em pesquisa JAniCA 2023]
- **Lacuna**: [baixo] sem fonte robusta de fluxo e dores de som em anime. Recomendar pesquisa complementar (entrevistas com 音響監督, JAS, Dolby, estúdios de som).

### 3.14 Entrega e master (納品 nōhin)
- **Quem**: produtor / 制作進行 / pós-produtora → emissora/streaming/comitê. [conhecimento geral]
- **Entra→Sai**: episódio final + stems/áudio mixado → master para emissora/OTT (ProRes/XDCAM/MXF etc.; antes fita HDCAM) com loudness e metadados; para OTT, pacotes tipo IMF e QC. [baixo — **não achei fonte aberta específica de anime**; o resumo de busca só trouxe prestadores genéricos de XDCAM/ProRes e a especificação EBU IMF v2.0]
- **Dores**: não documentadas. A comparação com o METI diz apenas que streaming global trouxe regras de qualidade próprias. [médio, METI jan/2025]
- **Recomendação**: pesquisar especificações reais de entrega de Netflix/Crunchyroll/Amazon para anime e as normas de TV japonesas (ARIB) em rodada posterior.

---

## 4. Estúdios: o que se sabe do ferramental e da organização

| Estúdio | O que as fontes mostram | Confiança |
|---|---|---|
| **Toei Animation** | Co-desenvolveu com a Celsys o timesheet digital (Toei Digital TimeSheet/TDTS) e o xdts; primeira série de TV "full digital" da Toei foi *Seikai Suru Kado* (abr/2017, com o animador-diretor Ryochimo, que operou o experimento enquanto produzia a série); app gratuito, uso comercial liberado, v3.8.0 em jan/2025. Em 2021 testou IA de fundos com Preferred Networks; em 2024 entrou num projeto de IA de colorização (Cinnamon AI/Geek Pictures). Algumas fontes falam de recuo público sobre IA, mas só via post de LinkedIn. | alto (TDTS/xdts), médio (IA), baixo (recuo) — https://cgworld.jp/interview/201706-celsys-kado-2.html [PR]; https://clipstudio.net/ja/dl/toeianimation; https://aismiley.co.jp/ai_news/animation-automatic-landing-ai-toei/ |
| **Kyoto Animation** | Estúdio verticalizado (equipe assalariada, treinamento interno); trabalha em "unidades" de ~5–6 pessoas (diretor, 1–2 diretores de animação, jovens, veterano); média de ~6–8 key animators/episódio; digital em prédio separado (pintura e desenho digitais, scanner e inspeção de vídeo na sede); abriu sala de edição e som próprios. **Não achei nome do software de desenho/satsuei.** | médio — https://blog.sakugabooru.com/2021/10/31/rebuilding-kyoani-unique-pipeline-miss-kobayashis-dragon-maid-s-production-notes/; https://www.japanesewiki.com/culture/Kyoto%20Animation.html |
| **ufotable** | Altamente interno (planejamento, direção, desenho, shiage, arte, 3D, satsuei; só som fora). Departamento "digital imaging" (デジタル映像部) cobre satsuei+3D; 3ds Max + V-Ray, Deadline (render), After Effects (3D layers com câmera de 3ds Max), EmberGen (fluidos), DaVinci Resolve, Python; criou exportador USD próprio (1 arquivo por cut) para não perder o caminho de texturas do FBX; visualização browser-based dos intermediários. Dores: reconverter arquivos de parceiros externos, preparar dados de revisão para cada ferramenta, esperas de render. | médio-alto (a fonte é matéria patrocinada HP/NVIDIA) — https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html; https://gendai.media/articles/-/90830 |
| **MAPPA** | Muitos projetos em paralelo, subcontrata por obra/etapa a especialistas; CGI部 interno para 3D; 3ds Max/MotionBuilder/ShotGrid; 3D layout para a maior parte dos cuts 3D; justifica 3D por previsibilidade (qualidade/prazos de desenho variam por animador). Estúdio em Sendai (digital remoto). | médio-alto — https://cgworld.jp/feature/202104-why3dcg-anime.html; https://gendai.media/articles/-/90830; METI jan/2025 |
| **Trigger** | Pioneiro em digital no desenho (Estúdio Fukuoka desde 2016, equipe contratada localmente); em *Promare* (2019) usou OpenToonz para pencil test e checagem de timing por diretor de animação/diretor; ~150 dos ~700 cuts pós-genga em digital; meta: episódio completo em digital. | alto (casos Wacom e OpenToonz são fontes de fabricante) — https://tablet.wacom.co.jp/biz-design/pdf/casestudy_trigger.pdf; https://opentoonz.github.io/e/usecase.html |
| **Sunrise / Bandai Namco Filmworks** | Sistema interno de gestão de produção **OWL VIEW** usado em todos os estúdios e títulos da empresa; equipes internas de CG e satsuei; alianças com Landoc Studio (3DCG+digital) e participação societária na Anima (CG). Faltam dados de ferramenta de desenho. | médio — METI mar/2023 p. 87; https://cgworld.jp/flashnews/2210-cgip.html |
| **Wit Studio** | Seu primeiro curta full digital de ~30 min (*Hello WeGo!*, 2019, programa Animetamago, financiado pelo governo) em Wacom Cintiq; política declarada de manter papel e digital juntos. Fonte do próprio fabricante. | médio-baixo — https://tablet.wacom.co.jp/biz-design/pdf/casestudy_witstudio.pdf (texto corrompido na extração; usei o resumo de busca) |
| **Studio Ghibli** | Genga/douga em lápis sobre papel; fundos pintados com guache; depois digitaliza: scanner próprio (GTS), **OpenToonz** (Toonz customizado, desde 1995) para pencil test, cor e composição, IwaWarper para "harikomi" de textura em objetos; efeitos novos e render HDR para Dolby Cinema (*O Menino e a Garça*). Aprovação do diretor Miyazaki ao fim. | alto para o fluxo (site OpenToonz + entrevista do diretor de imagem digital) — https://opentoonz.github.io/e/usecase.html; https://www.filmcomment.com/art-and-craft-atsushi-okui-on-the-boy-and-the-heron/ |
| **Science Saru** | Flash/Animate (Adobe) desde a fundação em 2013; "animação digitalmente assistida", combinando desenho à mão com recursos do Animate; equipe pequena. | médio — https://blog.adobe.com/en/publish/2020/05/21/how-science-saru-animation-studio-is-redefining-the-japanese-animation-industry (conteúdo Adobe) |
| **CloverWorks / A-1 Pictures** | CSP EX para desenho, RETAS STUDIO para shiage, Photoshop para fundo, After Effects para satsuei, Blender/Houdini/3ds Max para 3D (2021); storyboard ainda ~70% à mão; 300–400 GB por episódio; liderou o projeto **AnimeCanvas** com Sony (ver §5). | alto — https://www.gizmodo.jp/article/hpe-directplus-online-store/; https://cgworld.jp/article/actf25-taaf-animecanvas03.html |
| **Shaft** | Novo estúdio de Shizuoka ("AOI"): linha full digital de douga a shiage e equipe híbrida 3DCG+mão com Toon Boom Harmony. | baixo — só trecho de imprensa via busca (https://gamebiz.jp/news/350869) |
| **Madhouse** | Filme *Okko's Inn* (2018): pencil test em OpenToonz por pedido do diretor. Nada mais encontrado. | baixo — https://opentoonz.github.io/e/usecase.html |
| **Outros úteis** | **OLM / OLM Digital**: eliminou o desenho analógico dos animadores internos em 1–2 anos (tudo em CSP), mantém ferramentas abertas (OLM Peg Hole Stabilizer, Smoother), usa SavePoint for Anime; admite que o período misto "é o mais difícil". **studio MOTHER**: full digital, CSP+TDTS+pasta de cut, "Shinkō-kun" (Slack+Google Drive+planilha). **Sanjigen**: banco de dados interno (サンジゲンDB) rastreia cada cut/responsável quase em tempo real, ~15.000 checagens/mês no estúdio de Kyoto. **Colorido**: AE+Photoshop; **Studio Chizu, Ponoc**: OpenToonz. | alto/médio — https://cgworld.jp/regular/202308-chiyama-t-1-2.html; https://cgworld.jp/article/actf25-mother02.html; https://cgworld.jp/article/actf2026-in-taaf2.html |

**Fato transversal [alto]**: nenhum estúdio documentado usa **uma única ferramenta** de ponta a ponta. Padrão do relatório: "cada etapa usa uma ferramenta diferente" (desenho em CSP/RETAS/TVPaint/Harmony; shiage em RETAS PaintMan/CSP/OpenToonz; satsuei em After Effects; 3D em 3ds Max/Maya/Blender; storyboard em Storyboard Pro/CSP; ShotGrid ou sistema próprio para gestão). [METI mar/2023 p. 29, 67]

---

## 5. Migração papel → digital: linha do tempo e o que ainda é papel

**Linha do tempo [médio-alto, juntando fontes]**
- 1993: RETAS! PRO inicia a digitalização do **shiage**. (https://www.itmedia.co.jp/news/article/2409/06/1240906086/)
- Fim dos anos 90–2000s: shiage, satsuei e fundos migram. Cel pintado à mão praticamente acaba por ~2013. [médio]
- 2015: Celsys lança as funções de animação no CSP EX (https://cgworld.jp/news/tool/201510-clipstudiopaintex.html); em 2015 a descontinuação de um lápis duro muito usado ("Mitsubishi") virou alarme. [médio]
- 2016: ACTF (fórum JAniCA + Wacom + Celsys) começa; 2017: Toei faz o primeiro seriado TV full digital (*Kado*) em colaboração com Celsys; 2018–2019: xdts e TDTS saem gratuitos. [alto]
- 2020–2022: COVID empurra a mudança — pesquisa JAniCA 2023 tem várias respostas "a virada digital foi forçada pelo COVID"; relatório METI mar/2023 mostra a China virando 100% digital na douga. [alto]
- 2022–2023: 4 contratantes + 1 estúdio + 2 subcontratadas com toda a produção interna digital, num grupo de 15 empresas de desenho 2D; digital completo com checagem digital: "extremamente raro" (2 contratantes e 1 subcontratada). [alto, METI mar/2023 p. 39]
- 2024-05: Sony apresenta **AnimeCanvas**; 2025-03: protótipo KEY/DO mostrado no ACTF2025; uso interno em A-1/CloverWorks planejado para o ano fiscal de 2025; COLOR em testes operacionais no FY2025. [alto, https://cgworld.jp/article/actf25-taaf-animecanvas03.html]
- 2025-01: Toei TimeSheet 3.8.0; 2026-03: CSP 5.0. [alto]
- 2025-01: o METI reconhece que a área de desenho (a base do anime) está atrasada na digitalização e lista como causas diferenças geracionais, orçamento e **diferença de formato**. [alto, METI jan/2025]

**O que ainda é papel [alto, METI mar/2023 + JAniCA 2023]**
- Genga/layout de freelancers e animadores veteranos; checagem de diretor/演出/作監 (às vezes só em papel); cut bags em paralelo ao servidor; timesheet em papel mesmo com desenho digital.
- Fricção dos veteranos: pessoas que desenham a vida inteira em papel resistem; para o conferente veterano, o jovem mais veloz em digital aumenta a carga de checagem. [alto, METI jan/2025 slide 26]
- Voz de resistência explícita nos comentários livres JAniCA 2023: "Se o papel sumir do local de trabalho, eu saio". [médio, https://www.janica.jp/survey/survey2023Report.pdf] — valida que uma ferramenta nova precisa **preservar a sensação de papel** (a mesma conclusão do projeto AnimeCanvas).

---

## 6. Revisão e aprovação: o grafo real de checagem

[Conjunto: alto para o que foi lido em MOTHER/METI/Shinkai/KEY-DO; conhecimento geral para as setas sem fonte.]

```
Roteiro ──► E-conte / vídeo-conte ──► Reunião de produção (作画打ち合わせ)
                                           │
Layout (LO) ──► LO演出チェック (LOEN) ──► LO作監チェック (LOSA)
   │
Genga (G) ──► 作監修正 ──► (演出 check) ──► 第二原画 (se houver) ──► 作監 final / 総作監
   │
Douga ──► 動画検査 ──► (produção)
   │
Shiage (+色指定) ──► 色指定検査 / 仕上げ検査
   │
Satsuei (+fundos +3D +efeitos) ──► diretor checa em vídeo ──► リテイク ──► refaz o cut
   │
Edição ──► Som (AR→MA) ──► 初号/試写 (comitê) ──► 完パケ/納品
```

- **Diretor de episódio (演出)** e **diretor de animação (作監)** são os dois checadores centrais do desenho; o **diretor da obra/diretor-chefe** decide o geral; o **cut** é o objeto de cada aprovação. [alto]
- No **KEY/DO**: ao checar, a direção adiciona camadas vermelhas de correção e as células modificadas mudam de cor no timesheet; o botão de "submit" gera arquivos com nomes automáticos e serve como registro de que o check foi feito; etapas concluídas ficam travadas. [alto, https://cgworld.jp/article/actf25-taaf-animecanvas03.html] Isto é praticamente uma **especificação de requisitos** para o fluxo de aprovação.
- Pela Sakuga Blog (2019): cada cut passa por muito mais mãos hoje; freelancers frequentemente nem veem o resultado até a transmissão; a fragmentação reduz harmonia visual. [médio, baseado em créditos e depoimentos]

---

## 7. Gargalos e onde um software integrado ganha mais valor

Ordenados por força da evidência. "Valor" é **[inferência]**.

| # | Gargalo | Evidência | Onde um software integrado ganha |
|---|---|---|---|
| 1 | **Mistura papel+digital** (scan, impressão, conversão) | METI mar/2023: custo de ~2,2 milhões de ienes/mês de conversão para ~1 episódio contínuo; "maior problema da digitalização" [alto] | Fluxo que aceite entrada em papel (scan com alinhamento por pino, correção de TP) e saída impressa sem perder o vínculo com o cut |
| 2 | **Timesheet desconectado das ferramentas de desenho** | CSP/TVPaint/Harmony só têm timeline; timesheet digital existe como app separado; adoção parcial [alto] | Timesheet nativo ligado ao cut, ao vídeo-conte, ao áudio e à câmera, com export a AE/xdts |
| 3 | **Checagem do diretor/作監 fora das ferramentas** | Nenhum estúdio tem treino ou documentação; checagem em papel é comum; não há software para o 演出 [alto/médio] | Módulo de revisão com anotações em camada, histórico de retakes, aprovação formal por etapa, travas |
| 4 | **Formatos/specs por empresa, obra e etapa** | 150–200 dpi mas varia; "注意事項" por obra; satsuei reajusta tudo [alto] | Projeto como fonte única de spec (tamanho, resolução, camadas, nomeação), que o app aplica sozinho ao exportar |
| 5 | **Gestão de produção fragmentada** | Só poucos estúdios têm sistema integrado (próprio: OWL VIEW, NORNIR; ShotGrid; planilhas); small studios só têm planilhas; desenvolvedores externos não entendem anime [alto] | Cut table/progress/IN-UP como o próprio modelo de dados do cut; notificações; cobrança/orçamento por cut |
| 6 | **Ferramenta de shiage em fim de vida** | PaintMan sem manutenção e limitada a HD [alto] | Shiage com fill assistido e checagem de cor; resolução alta; paleta ligada ao color design |
| 7 | **Hibrido 2D×3D** | Cronogramas e instruções diferentes; 3D não segue timesheet; pipeline refeito por obra [alto] | Cut com camadas 3D+2D no mesmo timeline; câmera unificada; USD/FBX/Alembic/GLB |
| 8 | **Dados pesados e armazenamento** | 300–400 GB/episódio; renderização longa; falha de servidor no fim [médio-alto] | Armazenamento por cut com versionamento e cache; render distribuído/queue |
| 9 | **Terceirização e comunicação com freelancers** | Freelancers sem equipamento/skill; formatos próprios; douga/shiage no exterior [alto] | Portal/entrega por cut para externos, com checagem automática de spec |
| 10 | **Talento (genga/douga escassos)** | Círculo vicioso de douga exterior e falta de veteranos [alto] | Ferramenta que reduza treino e permita douga assistida sem quebrar o estilo |
| 11 | **Retakes/ciclos tardios** | Satsuei, Shinkai: retakes após "satsuei" e checagem em vídeo [alto] | Prévia contínua (line test → cor → composição) para detectar problema antes do último elo |
| 12 | **Armazenamento do conhecimento (setting/color/model)** | Pouco documentado nas fontes; [inferência] | Biblioteca de modelo/cor/fundo por obra com versionamento |

**Direções de produto visíveis nas fontes** (útil para benchmark): a Sony diz para o AnimeCanvas "não ganhar com isso, manter por cooperação entre empresas"; "manter a sensação de papel/lápis"; "formato único de arquivo e timesheet consistente desde a genga até a satsuei"; e "ambiente comum à indústria" — e a Sony admite que foi pedida API e multi-OS mas "exige cuidado com custo operacional". [alto, https://cgworld.jp/article/actf25-taaf-animecanvas03.html] Isso implica que **existe espaço competitivo** mas também um **concorrente com apoio de grupo grande já prototipando**.

---

## 8. Tabela-resumo por etapa

| Etapa (JP) | Quem | Entra → Sai | Ferramenta típica (2025–26) | Dor principal | Revisão/aprovação | Papel×digital | Confiança |
|---|---|---|---|---|---|---|---|
| Planejamento 企画 / 製作委員会 | Produtor, comitê, estúdio | Proposta, orçamento, equipe, cronograma → contrato | Escritório (Word/Excel/PPT); SavePoint em alguns | Direitos complexos, margem baixa | Comitê | Digital | médio |
| Composição de série シリーズ構成 / roteiro 脚本 | Compositor, roteiristas, diretor | Sinopse → roteiro (3–4 versões) | Não documentada | Sem vínculo com cuts [inferência] | Diretor/produtor/autor | Digital | baixo-médio |
| Settei 設定 / design / cor 色彩設計・色指定 | Character designer, diretor de arte, designer de cor | Model sheets, pranchas, paleta/tabela de cores | Photoshop/CSP; paletas no shiage | Checagem de cor por cut | Diretor/arte/cor | Digital | médio |
| E-conte 絵コンテ / vídeo-conte | Storyboarder / diretor de episódio | Roteiro → conte (~300 cuts/ep) → animatic | Storyboard Pro, CSP; ainda muito papel | Painéis manuais, mudanças tardias | Reuniões, diretores | Misto | médio-alto |
| Layout レイアウト | Layout/genga sênior, 演出, 作監 | Conte → layout aprovado | CSP; 3D layout em MAPPA etc. | Specs divergentes | LOEN → LOSA | Misto | alto |
| Genga 原画 / 第二原画 / 作監 | Genga-man, 2º genga, 作監, 総作監 | Layout+timesheet → poses-chave com timing | CSP EX, TVPaint, Harmony, Animate; papel | Falta de timesheet nativo; escassez; fragmentação | 作監修正 → 演出 | Misto, papel forte | alto |
| Douga 動画 / 動画検査 | Douga-man, inspetor | Genga → in-betweens limpos | CSP, CACANi | Conversão papel↔digital; exterior | 動画検査 | Digital crescendo | alto |
| Shiage 仕上げ / 色指定検査 | Shiage, 色指定, inspetor | Douga + cor → imagem pintada | RETAS PaintMan (EOL), CSP, OpenToonz; AnimeCanvas COLOR (em dev) | Software sem manutenção; TP correction | 色指定検査 | Digital | alto |
| Fundos 美術 / 背動 | Diretor de arte, pintores | Layout → fundo 200–300 MB | Photoshop; guache (Ghibli); 3D | Peso de arquivos | Diretor de arte | Digital (Ghibli: papel) | alto |
| Efeitos エフェクト / 3DCG | Efeitos 2D, CG | Cena → render/efeito | 3ds Max, Maya, Blender, Houdini, ShotGrid | 2D×3D (cronograma e timesheet) | ShotGrid, direção | Digital | alto |
| Satsuei 撮影 | Dir. fotografia, equipe | Tudo → cut final | After Effects, DaVinci, Houdini | Último elo, dados e render | Checagem em vídeo, retakes | Digital | alto |
| Edição 編集 | Editor | Cuts → episódio | Premiere/pós-produtora | Não documentada | Comitê (初号) | Digital | baixo |
| Som 音響 | 音響監督, dubladores, mixer | Afureko → efeitos/música → MA | Pro Tools | Pouco documentado | 音響監督/diretor | Digital | baixo |
| Entrega 納品 / master | Produtor/制作進行 | Episódio + áudio → master | ProRes/XDCAM/MXF/IMF | Pouco documentado | Emissora/OTT QC | Digital | baixo |
| **Timesheet タイムシート** | Direção, genga, produção | Documento transversal ao longo dos cuts | Papel; TDTS+xdts; UAT | Não nativo nas ferramentas | Direção/作監 | Misto | alto |
| **Gestão do cut** (カット袋/表, 制作進行) | Produção | Cut bag/tabelas → pasta+DB | Planilhas, ShotGrid, OWL VIEW, NORNIR, SavePoint, Shinkō-kun | Poucas integrações; cada estúdio resolve só | Produtor/desk | Misto | alto |

---

## 9. Lacunas e próximos passos sugeridos (para a fase 1)

1. **Timesheet**: obter a especificação oficial do xdts (Celsys) e um exemplo real do formato de folha japonesa (colunas, 144 quadros, símbolos de in-between) — não fechado nesta rodada. Também o uso do "tsuke" por quem faz.
2. **Som, edição e entrega**: faltam fontes boas (afureko e MA reais, Pro Tools, especificações de entrega Netflix/Crunchyroll e ARIB). Marcar como baixo confiança até corrigir.
3. **Estúdios** com dados fracos: Madhouse, Wit, Shaft, Sunrise/BNF (ferramenta de desenho), Kyoto Animation (software), CloverWorks pós-2021 (AnimeCanvas já está coberto). Fontes a tentar: ACTF 2020–2026 reports na CGWorld/Mynavi, Animation Obsessive (posts de estúdio), entrevistas da série "Anime Craft Weekly" da Sakuga Blog.
4. **Dados de adoção**: não encontrei percentual de "digital vs papel por etapa" em pesquisa da indústria. Os relatórios AJA do tipo "アニメーションのデジタル制作におけるアンケート" (2009–2014) são antigos; o METI mar/2023 é qualitativo. Pedir ao dono o PDF "アニメーション制作者 実態調査2023" completo e o relatório "アニメ産業レポート 2025" (AJA), que não consegui abrir.
5. **Estrutura de custo por cut**: não coletada (valor por cut/folha de douga/shiage/satsuei) — importante se o produto for medir custo.
6. **IA**: só sinais iniciais (colorização automática, fundos, in-betweens), fontes fracas e polêmica; tratar separadamente em outra frente.

---

## 10. Fontes principais (com notas de qualidade)

**Governo / associações (qualidade alta)**
- METI / Associação Japonesa de Animação (AJA), *Relatório sobre formação de pessoal para a produção digital de animação*, mar/2023 — https://www.meti.go.jp/meti_lib/report/2022FY/000254.pdf (lido por extração direta do PDF)
- METI, slides "Situação do setor e plano de ação [anime]", 17/01/2025 — https://www.meti.go.jp/shingikai/mono_info_service/entertainment_creative/pdf/003_04_02.pdf (lido por extração direta)
- JAniCA, *Pesquisa sobre criadores de animação 2023* — https://www.janica.jp/survey/survey2023Report.pdf (lido por extração direta; usei só trechos qualitativos)
- Agência de Assuntos Culturais, cursos de anime (色彩設計・仕上げ etc.) — https://www.bunka.go.jp/animeseminar/2025/course/course06.html (só a página de apresentação)

**Fabricante / ecossistema (qualidade média; interesse comercial)**
- Celsys, xdts — https://www.celsys.com/en/clipsolution/xdts/; deck 2018 — https://www.celsys.com/files/user/pdf/animation/CSP_animation_2018_9.pdf; TDTS — https://clipstudio.net/ja/dl/toeianimation; casos CSP — https://clipstudio.net/ja/animation/; notas Ver. 5.0 — https://www.clipstudio.net/ja/news/202603/11_01/
- OpenToonz (casos) — https://opentoonz.github.io/e/usecase.html
- Wacom (Trigger) — https://tablet.wacom.co.jp/biz-design/pdf/casestudy_trigger.pdf; (Wit) — https://tablet.wacom.co.jp/biz-design/pdf/casestudy_witstudio.pdf (ilegível)
- Toon Boom — https://cgworld.jp/special-feature/202603-toonboom.html ([PR] de recrutamento; nenhum estúdio japonês nomeado)

**Mídia / eventos (qualidade média a alta)**
- CGWORLD: studio MOTHER ACTF2025 — https://cgworld.jp/article/actf25-mother02.html; AnimeCanvas ACTF2025 — https://cgworld.jp/article/actf25-taaf-animecanvas03.html; Marza storyboard ACTF2024 — https://cgworld.jp/article/202404-ACTF-marza.html; ACTF2019 UAT — https://cgworld.jp/feature/201903-actf-01-2.html; Kado/Toei [PR] — https://cgworld.jp/interview/201706-celsys-kado-2.html; 3DCG×2D — https://cgworld.jp/feature/202104-why3dcg-anime.html; OLM — https://cgworld.jp/regular/202308-chiyama-t-1-2.html; ufotable [patrocinado] — https://cgworld.jp/special-feature/202410-nvidia-hp-ufotable.html; Sanjigen/ACTF2026 — https://cgworld.jp/article/actf2026-in-taaf2.html
- ITmedia (AnimeCanvas) — https://www.itmedia.co.jp/news/article/2409/06/1240906086/ e /2
- Gizmodo Japão (CloverWorks, 2021) — https://www.gizmodo.jp/article/hpe-directplus-online-store/
- Sakuga Blog — https://blog.sakugabooru.com/2021/10/31/rebuilding-kyoani-unique-pipeline-miss-kobayashis-dragon-maid-s-production-notes/ ; https://blog.sakugabooru.com/2019/08/31/the-fragmentation-of-anime-production-too-many-cooks-spoil-the-broth/
- Film Comment (Ghibli, Okui) — https://www.filmcomment.com/art-and-craft-atsushi-okui-on-the-boy-and-the-heron/
- gendai.media (ufotable×MAPPA) — https://gendai.media/articles/-/90830
- gamebiz 2026 (funções) — https://gamebiz.jp/news/427905

**Fontes fracas (usar só como pista)**
- Blog antigo Washi (2011) — https://washiblog.wordpress.com/2011/01/18/anime-production-detailed-guide-to-how-anime-is-made-and-the-talent-behind-it/ (números de cuts/desenhos e estrutura clássica, mas com 15 anos; comentários de leitores marcados como não verificados)
- Resumos de busca sem leitura da página para som, edição, entrega e algumas menções de IA.
