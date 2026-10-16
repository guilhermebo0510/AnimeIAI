# A4 — Pré-produção, gestão/intercâmbio e IA na produção de anime

Pesquisa bruta, fase 1. Data de referência: 2026-10-15. Idioma: português.

## 0. Como ler este relatório

**Convenções**
- Confiança: **alta** (fonte primária lida por mim), **média** (fonte secundária boa, ou primária lida via resumo automático de página), **baixa** (blog de marketing, agregador, fonte única ou conflitante).
- Marcação: **[FATO]** = visto em fonte citada; **[INFERÊNCIA]** = conclusão minha, não afirmada pela fonte; **[NÃO VERIFICADO]** = conhecimento geral que não confirmei nesta pesquisa.

**Ressalvas de método (importantes)**
- As ferramentas de leitura de página devolvem *resumos* feitos por um modelo menor. Licenças e datas de repositório vieram da API pública do GitHub (`api.github.com/repos/...`) lida em 2026-10-15/16; "último push" é a data do último commit em qualquer branch, **não** de uma release. Quando a API passou a devolver 403 (limite de taxa), usei o README/LICENSE bruto e marquei.
- A especificação do XDTS **não é pública sem cadastro** (seção 2.4). Descrevo a estrutura a partir do código de leitura do OpenToonz, lido via resumo — confiança média-alta.
- Não consegui ler dois PDFs criptografados (JST CRONOS e slides do WIPO sobre o direito autoral japonês). Usei resumos de busca para esses pontos.
- Várias comparações de modelos de vídeo fechados vêm de blogs de marketing: tratei como confiança baixa.
- Licença de **código** ≠ licença de **pesos** ≠ direitos sobre os **dados de treino**. Sempre que possível separo os três.

---

## 1. Roteiro e pré-produção

### 1.1 O fluxo japonês real (o que o módulo precisa refletir)

**[FATO, confiança média]** O fluxo corrente descrito em material de ensino e imprensa japoneses: planejamento/roteiro → configurações (設定: personagem, arte, adereços, cores) → storyboard (絵コンテ) → *演出処理* (checagem de composição e atuação pelo diretor/episódio) → layout → key animation (原画) → in-betweens (動画) → pintura digital (仕上げ) + fundos → composição/photography (撮影) → edição, som, entrega. O time sheet (タイムシート) acompanha o corte e diz qual desenho vai em qual quadro. Fontes: <https://animeanime.jp/article/2024/10/31/87413.html>, <https://www.mhlw.go.jp/content/11901000/001152417.pdf> (resultados de busca; li só os resumos).

**[FATO, confiança média]** Roteiro de anime é frequentemente feito por vários roteiristas sob um "composição da série" (シリーズ構成): um roteirista define a estrutura e outros escrevem episódios. Fonte: <https://koubo.jp/article/17953> (guia de formatação de roteiro).

### 1.2 Roteiro (honpon / 脚本) — formato

- **Estrutura de três elementos [FATO, médio]**: *hashira* (柱, cabeçalho de cena: número, local, hora), *togaki* (ト書き, ação/descrição) e *serifu* (セリフ, fala). Convenções do guia: ○ antes do local, hora entre parênteses, ação recuada em relação à fala, fala entre 「」 sem ponto final; estimativa de ~20×20 caracteres por minuto. Fonte: <https://koubo.jp/article/16798> e <https://koubo.jp/article/17953>. **Limite:** esse material é para concursos de roteiro de drama; não achei especificação de formato *de estúdio de anime*. Confiança de que seja o mesmo no anime: **baixa**.
- **[NÃO VERIFICADO]** Numeração de versões (準備稿/決定稿), a codificação "話数/Aパート/Bパート" e o hábito de escrever em Word/Ichitaro/editor próprio. Precisa ser validado com roteiristas/produtores japoneses.
- **Fountain** (texto puro, padrão aberto de fato no Ocidente): cobre cabeçalho de cena, ação, personagem, diálogo, parêntese, diálogo duplo, transição, notas, *boneyard*, seções e sinopses; última versão listada 1.1 (2014). Não declara licença na página. Fonte: <https://fountain.io/syntax/> — **alta**.
- **Final Draft 13** (FDX), WriterDuet, Celtx, Fade In, Highland 2 — preços e formatos de exportação (FDX/Fountain/PDF) segundo blogs comparativos; **baixa**: <https://digitalfilmmaker.net/best-film-screenwriting-software/>, <https://capterra.com/screenwriting-software>. O Storyboard Pro lê scripts do Final Draft (há correção de falha específica com "certos scripts do Final Draft": <https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-1-release-notes.html>).
- **[INFERÊNCIA]** Nenhum desses formatos modela: "fala em japonês + legenda/idioma alvo", furigana, formato vertical (縦書き) ou o ciclo "cena→corte". O módulo integrado deve ter um modelo próprio (cena→cortes) e *importar/exportar* Fountain/FDX/TXT como interfaces, não como formato interno.

### 1.3 Bíblia da série, settei, color script

- **[FATO, médio]** Pacote de referência (設定資料): ficha de personagem com vista tripla (frente/lado/costas), cores de cabelo/olhos/pele/figurino, escala/altura, expressões (表情集), adereços (小道具設定), tabela de cores (色指定/色設定表, normalmente numerada), e *color script*/color design (色彩設計) com o clima de cor por episódio/cena. Fonte (resumo de busca, sem página única estável): <https://dendekaden.notion.site/The-Animation-Process-5394164d204141a3a1a05d4fb05dcc9b> e resultados de busca em japonês. Papéis: designer de personagem, *settei seisaku* (制作設定, gestão de settei), diretor de animação (作画監督, consistência de traço), *shikisai sekkei* (色彩設計).
- **[INFERÊNCIA]** Settei precisa ser **entidade versionada com vínculo ao corte**: cada corte referencia a versão de settei e a tabela de cores vigentes. Hoje isso vive em PDFs/pastas — nenhum sistema que achei modela essa relação (ver seção 4).

### 1.4 Storyboard / e-conte (絵コンテ)

- **[FATO, médio]** Não há formato padronizado rígido: o que se espera é cena/corte, imagem, descrição da ação, diálogo/SE/BGM, duração. O layout típico em tabela, de cima para baixo e da esquerda para a direita: **número do corte | imagem | conteúdo | fala (e BGM/SE) | duração | total**. Fontes: <https://forest.watch.impress.co.jp/docs/review/460218.html> (editor de e-conte gratuito, resenha), <https://graphicsha.co.jp/wp/wp-content/uploads/2025/01/9784766140125.pdf> (amostra de livro: colunas "No. do corte, local/fundo, conteúdo, fala/BGM-SE, segundos"), <https://assets.clip-studio.com/en-us/detail?id=2075662> (templates de e-conte no CLIP STUDIO ASSETS).
- **Storyboard Pro 25 / 25.1 (Toon Boom)** [FATO, alta/média]: versão 25 lançada em junho de 2025 junto do Harmony 25 (thumbnailing rápido, efeitos não destrutivos); 25.1 (dez/2025) trouxe novo sistema de licença e **Ember AI ligada por padrão** (pode ser desligada). Importa/exporta XML e AAF (com tratamento de frame rate), exporta FCPXML. **Não há menção a OTIO** nas notas de versão. Fontes: <https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-1-release-notes.html>, <https://www.toonboom.com/?p=18892>. Nota: uma página de ajuda diz que no SBP 25 o Ember vem *desligado* por padrão; as fontes divergem entre versões (<https://docs.toonboom.com/help/storyboard-pro-25/getting-started/ai.html>) — confiança **baixa** sobre o padrão.
  - **Ember "Analyze Script with AI"**: recebe .txt/.rtf (<10 MB), envia ao servidor, devolve sinopse, personagens, props, sons, divisão em cenas e painéis com diálogo/notas/slugging; exporta CSV. Script Breakdown usa Google Gemini; outros provedores citados: Bria e ElevenLabs; a Toon Boom afirma que dados não treinam modelos e são apagados após o processamento (alegação da empresa, sem auditoria). Fontes: <https://docs.toonboom.com/help/storyboard-pro-25/storyboard/ai/analyze-script-with-ai.html>, <https://www.toonboom.com/toon-boom-leaders-explore-ais-role-in-animation-at-kidscreen-summit-2026>. Confiança **média-alta**.
- **Storyboarder (Wonder Unit)**: repositório com último push em 2024-03-17 e última release v2.1.0 em 2020-09-03; campo de licença nulo na API. **Projeto efetivamente parado.** <https://github.com/wonderunit/storyboarder> — confiança alta no estado; licença **não verificada**.
- **CLIP STUDIO PAINT** (Celsys): Ver. 5.0 lançada em 2026-03-11 (Smart Shape, mão 3D); 5.1.2 em 2026-08-06; 4.2.7 foi o último 4.x (2026-02-04). Datas de 5.1.0 conflitam entre idiomas (março vs 28/jul/2026). Fontes: <https://koubo.jp/press-release/prtimes/c5223_r1071>, <https://clipstudio.net/en/dl/release_note/latest/>. **Confiança média.** CSP já exporta/importa **XDTS** (seção 2.4). Em 2025-11-13 a Celsys anunciou adoção do CSP no processo de animação da SUNRISE Studios (Bandai Namco Filmworks): <https://www.celsys.com/en/topic/20251113> — alta. Em 2022 a Celsys cancelou um "paleta de IA generativa de imagens" após reação negativa e declarou que não implementaria geração de imagem; não achei mudança dessa política em 2026: <https://www.clipstudio.net/en/news/202211/29_01/> e <https://www.clipstudio.net/ja/news/202212/02_01/> — alta para 2022, **ausência de evidência** para 2026.
- **Animatic/timing**: tanto Storyboard Pro quanto CSP/TVPaint/OpenToonz mantêm timeline com áudio. **[INFERÊNCIA]** O módulo precisa tratar o **corte** como unidade (com duração em segundos+quadros) e permitir que o animatic seja *derivado* da tabela e-conte, não desenhado em outra ferramenta.

### 1.5 Funções que um módulo integrado de pré-produção deve ter (proposta)

**[INFERÊNCIA]**, baseada nos fatos acima e nas lacunas da seção 4:

1. **Roteiro estruturado** (cena→falas→ação) com import Fountain/FDX/TXT e editor que aceite texto horizontal e vertical; versões com diff.
2. **Cena→Corte (cut)**: cada corte com número estável (não renumerar ao inserir), duração, descrição, falas vinculadas a linhas do roteiro, camada de rastreio "este corte vem de onde do roteiro".
3. **E-conte como visão tabular editável** do mesmo modelo (colunas: nº, imagem, conteúdo, fala/SE/BGM, duração, total), exportável para PDF A4/papel e para formatos de troca.
4. **Animatic gerado** da e-conte, com áudio de referência e duração calculada; exportar EDL/FCPXML/OTIO.
5. **Settei como biblioteca versionada** (personagem, adereço, cenário, cor) com vínculo ao corte e alerta quando a versão muda após a aprovação.
6. **Color script** como linha do tempo de cor por episódio/cena, derivada das cores reais dos cortes.
7. **Timesheet** (seção 2) criado já na pré-produção como "esqueleto" do corte (duração, camadas previstas).
8. **Rastreabilidade bidirecional**: roteiro↔cena↔corte↔storyboard↔animatic↔layout↔original↔final, com *status* por etapa e histórico de revisões (retake).
9. **Revisão/aprovação** com anotação sobre quadro, fila de retakes (リテイク) e comparação lado a lado.

---

## 2. Gestão de produção e intercâmbio

### 2.1 Rastreadores de produção

| Ferramenta | Licença | Estado observado | Observações | Confiança |
|---|---|---|---|---|
| **Kitsu** (frontend, CGWire) | AGPL-3.0 | release v1.0.71 em 2026-10-14; último push 2026-10-15 | <https://github.com/cgwire/kitsu> | alta |
| **Zou** (API do Kitsu) | AGPL-3.0 | último push 2026-10-15; sem endpoint de "latest release" | <https://github.com/cgwire/zou> | alta |
| **AYON** (Ynput) – launcher | Apache-2.0 | release 1.6.3 em 2026-06-25; push 2026-10-15 | <https://github.com/ynput/ayon-launcher> | alta |
| AYON – backend | "Other" na API (não resolvi qual) | release 1.16.9 em 2026-10-15 | <https://github.com/ynput/ayon-backend>; add-ons pagos (Powerpack) | média |
| **Prism Pipeline** | LGPL-3.0 | release v2.1.3 em 2026-07-30 | <https://github.com/PrismPipeline/Prism> | alta |
| **Flow Production Tracking** (ex-ShotGrid, Autodesk) | proprietário | sem mudança de preço oficial achada; preços de terceiros (US$50/usuário/mês) são de concorrente | <https://autodesk.com/support/technical/article/shotgrid-rebrand-frequently-asked-questions>, <https://morphic.com/de/resources/alternatives/shotgrid-alternatives> | baixa (preço) |
| **ftrack** | proprietário | citado como adquirido pelo PSG, post sem data | <https://blog.cg-wire.com/shotgrid-to-kitsu-migration> | baixa |
| **Save Point for アニメ** (MUGENUP) | proprietário | teste em estúdios desde 2019-12-02 (Seven Arcs, OLM); status em 2026 **não confirmado** | <https://cgworld.jp/news/event/1912-savepointfor.html> | média (2019) |

**Kitsu em detalhe [FATO, média-alta]**
- Hierarquia: produção → episódio → sequência → plano (shot) → asset, mais *edit* e *concept*; revisão/playlists, agenda, orçamento, cotas, timesheets de pessoas, import/export EDL, OTIO e CSV; integrações com Blender, Toon Boom Harmony e Unreal; API (Gazu em Python). Fonte: <https://kitsu.cg-wire.com/> (resumo).
- Roadmap 2026: agenda de episódios descendo ao nível de plano; plugins; cliente JS; *links de revisão sem login*; templates de projeto; "abordagem de formato de arquivo para séries de TV" (sem detalhe). **Nada** sobre storyboard nem IA. Fonte: <https://blog.cg-wire.com/kitsu-2026-roadmap>, <https://blog.cg-wire.com/build-in-public-june-2026-update>. O blog de junho/2026 reporta queda de receita de 30% no trimestre e lançamento de "Kitsu Partners" (<https://blog.cg-wire.com/build-in-public-june-2026-update>, resumo) — **risco de longevidade do fornecedor** (inferência: AGPL mitiga, pois dá para bifurcar).
- **Adoção japonesa**: não achei nenhum estúdio japonês usando Kitsu. **[FATO negativo, média]**. Número de estúdios usuários é inconsistente entre páginas da empresa (70+ no GitHub vs 300+ em outra).
- **[INFERÊNCIA]** A unidade do Kitsu é *plano* (shot), a do anime é o *corte* (cut) dentro de *episódio* (話数); o modelo cabe, mas não há "timesheet de desenho" nem "cut bag" (カット袋) — o equivalente digital das pastas que circulam entre departamentos.

**Pesquisa e produtos japoneses**
- JST CRONOS (2025.10): projeto "高融合性ツール基盤技術によるアニメ共創環境の実現", líder Jun Kato (AIST), co-pesquisadores da OLM Digital e da Univ. de Tóquio; objetivo: plataforma que reúne informação de ferramentas de anime e base para combinar ferramentas; premissa declarada: a IA é "um meio", o alvo é apoiar a criatividade. Fonte: resumo de busca sobre <https://www.jst.go.jp/kisoken/cronos/dl/field/JPMJCS25K1_kato.pdf> (PDF criptografado, não li). Confiança **média**.
- Pesquisa de 2023 (IPA Mitou) sobre sistema de gestão com IA de busca em material intermediário: <https://www.ipa.go.jp/jinzai/mitou/advanced/2023/eid2eo00000024jj-att/seikagaiyou-sd-1.pdf> (só resultado de busca, não li).
- METI publicou em 2026-04-23 um relatório de condições de trabalho em animação: <https://www.meti.go.jp/policy/mono_info_service/contents/2026/0423/26042302.html> (não li; relevante só como contexto de pressão por eficiência).

### 2.2 Versionamento de assets pesados

- **Perforce Helix Core**: gratuito até 5 usuários e 20 áreas de trabalho, sem prazo; bloqueio exclusivo de arquivo; nuvem gerenciada a ~US$39/usuário/mês (dado do próprio fabricante). <https://perforce.com/products/helix-core/free-version-control> — **média** (marketing do fabricante).
- **Git LFS**: release v3.8.0 em 2026-08-28 (<https://github.com/git-lfs/git-lfs>) — alta. Fraqueza para artistas: UX e bloqueio frágeis; Anchorpoint (cliente Git para artistas, com bloqueio e thumbnails de PSD/FBX/GLTF) é ferramenta de terceiro, preço atual não confirmado: <https://www.anchorpoint.app/features/git-for-artists> — baixa/média.
- **DVC**: Apache-2.0, push 2026-10-12 (<https://github.com/iterative/dvc>) — alta; foco em dados de ML; **[INFERÊNCIA]** serve para *datasets/modelos* da camada de IA, não para o fluxo diário de artistas.
- **[INFERÊNCIA]** Um programa único de desktop deve ter **versionamento interno por corte/camada** (instantâneos imutáveis, hash de conteúdo) e *delegar* a sincronização pesada a Perforce/Git LFS/armazenamento compartilhado (estúdios japoneses relatam storage de rede, ex. 80 TB de NAS para 3 séries simultâneas: <https://www.cgw.com/Press-Center/Web-Exclusives/2024/Typhoon-Graphics-uses-Blackmagic-Cloud-Store-80T.aspx>, alta/média).

### 2.3 Revisão e aprovação

- **OpenRV** (ASWF): repositório ativo (push 2026-10-14), licença marcada NOASSERTION pela API; **não verifiquei o texto da licença** (<https://github.com/AcademySoftwareFoundation/OpenRV>). Contexto de 2023: liberado como parte do Open Review Initiative (<https://www.aswf.io/news/openrv/>).
- **Frame.io** (Adobe): preços de terceiros (Free 2 GB; Pro ~US$15; Team ~US$25) — **baixa**; confira a página oficial antes de citar.
- Kitsu: playlists, comentários desenhados, links de revisão para cliente sem conta (Q2/2026 no roadmap) — média.
- **[INFERÊNCIA]** O diferencial de revisão no anime é **comparar desenhos quadro a quadro com a mesma sheet de tempo** (o Save Point for アニメ lista "virar quadros no navegador" como função central, 2019).

### 2.4 Formatos de intercâmbio

#### XDTS / TDTS (timesheet japonês) — o ponto mais importante desta frente

- **O que é [FATO, alta]**: formato aberto de "exposure sheet digital" criado e mantido pela **Celsys**, oferecido desde 2019, pensado para troca entre qualquer software com timeline/exposure sheet. A página afirma que qualquer um pode implementar; **não declara licença**. <https://www.celsys.com/en/clipsolution/xdts/>
- **A especificação**: PDFs em japonês (~100 KB) e inglês (~60 KB), nome de arquivo com "ver10", **liberados após formulário de cadastro** (nome, organização, aplicação, e-mail; aceite de possível divulgação como caso). <https://www.celsys.com/en/xdts/> — **alta**. Eu **não** obtive nem li o PDF. A Celsys diz que o XDTS Viewer suporta XDTS a partir da versão 1.3 (implica que há versões ≥1.3 do formato — **[INFERÊNCIA]**).
- **Estrutura de arquivo (via código do OpenToonz) [FATO, média-alta]**: o arquivo começa com a linha `exchangeDigitalTimeSheet Save Data` e o resto é **JSON** com: `header` (`cut`, `scene`, strings de 1–4 dígitos), `version` (inteiro), `subversion` opcional, `timeTables[]` com `name`, `duration` (quadros), `fields[]` (cada um com `fieldId` e `tracks[]`, cada track com `trackNo` e `frames[]` com `frame` e `data[]`), e `timeTableHeaders[]` (`fieldId`, `names[]` = nomes das camadas). Em `data[].values[]` vão número do quadro/célula (ex. `12a`) ou símbolos especiais (célula vazia, hífen, tick); `options[]` marca keyframe e *reference frame*. Fonte: código-fonte do OpenToonz (`xdtsio.cpp`, resumo via <https://raw.githubusercontent.com/opentoonz/opentoonz/master/toonz/sources/toonz/xdtsio.cpp>). **Aviso**: confirme contra a spec oficial; campos que não aparecem no importador do OpenToonz podem existir.
- **Campos/colunas**: o CSP permite exportar a "coluna de original" (ACTION) ou a coluna de célula (CELL); o TVPaint oferece o mesmo. <https://tips.clip-studio.com/en-us/articles/1372>, <https://doc.tvpaint.com/docs/timesheet/timesheet-import-export> — média-alta. **[INFERÊNCIA]**: `fieldId` diferencia colunas de ação/célula/etc.; a spec oficial deve listar os IDs.
- **Quem implementa [FATO, média/alta]**: CSP EX (import/export), **Toei Animation Digital Time Sheet (TDTS)** — gratuito, para Windows, lançado junto do CSP v1.8.4 (<https://forest.watch.impress.co.jp/docs/news/1156072.html>), OpenToonz (e um *XDTS Viewer* próprio), Tahoma, TVPaint (import com 3 modos: manter, atualizar, apagar camadas ausentes), Callipeg e plugin Odyssey (Praxinos). Adotantes de produção citados pela Celsys: Nippon Animation e o estúdio MOTHER. Fontes: <https://www.celsys.com/en/clipsolution/xdts/>, <https://doc.praxinos.coop/v4.X.57/CollaborativeWork/XDTS.html>, <https://digitalproduction.com/2026/06/29/callipeg-studio-heads-to-desktop/>.
- **Armadilha conhecida [FATO, média]**: XDTS indexa quadros a partir de **0**; OpenToonz e o time sheet da Toei mostram a partir de **1** — o conversor comunitário do itch.io criou opção de deslocamento. <https://wolfinabowl.itch.io/csv-to-xdts-converter/devlog/328746/added-frame-offset-options>
- **OTIO**: **não achei** adaptador de XDTS no núcleo nem no ecossistema (busca negativa, não prova de inexistência). A lista de adaptadores do repositório inclui CMX3600, FCP7 XML, AAF, SVG, Maya Sequencer: <https://github.com/AcademySoftwareFoundation/OpenTimelineIO> (via API da organização). **[INFERÊNCIA]** Oportunidade clara: implementar XDTS⇄OTIO (e depois XDTS⇄EDL/FCPXML) como contribuição aberta.
- Não achei parser XDTS autônomo e consolidado em GitHub (só escritores/conversores). Confiança baixa (busca limitada).

#### OpenTimelineIO (OTIO)

- Licença Apache-2.0; último push 2026-10-14; **release mais recente verificada: v0.18.1 (2025-11-09)**, anterior 0.18.0 (2025-11-06) e 0.17.0 (2024-06-24); nome "Beta 18". Os adaptadores migraram para repositórios separados (`otio-cmx3600-adapter`, `otio-fcp-adapter`, `otio-aaf-adapter`...), instalados via pacote guarda-chuva `OpenTimelineIO-Plugins`. Roadmap 2026 (slides do ASWF, nov/2025): modelagem de pipeline de cor, transformações espaciais, mapeamento de streams/canais; Avid Media Composer 2025.6 importa/exporta OTIO; Kdenlive tem OTIO nativo. Fontes: <https://github.com/AcademySoftwareFoundation/OpenTimelineIO>, <https://tac.aswf.io/meetings/2025-11-12/OpenTimelineIO_ASWF_Project_Review_Nov_2025.pdf> (via resumo de busca). **Alta** (versão/licença), **média** (roadmap).
- **[INFERÊNCIA]** OTIO é ótimo para a linha de tempo de edição (cortes em sequência), **insuficiente** para timesheet de camadas (não modela "coluna de célula com marcas por quadro"). Usar OTIO como *exportação de edição* e XDTS como *exportação de timing de cena*.

#### Outros formatos (estado)

| Formato | Papel | Estado/Licença observado | Confiança |
|---|---|---|---|
| EDL (CMX 3600) | troca de cortes, limitado (1 trilha de vídeo no adaptador OTIO) | norma SMPTE 258M; adaptador OTIO existe | média (feature matrix: <https://opentimelineio.readthedocs.io/en/latest/tutorials/feature-matrix.html>) |
| FCP7 XML / FCPXML | Premiere/Final Cut; Storyboard Pro exporta | adaptador FCP7 no OTIO; **suporte a FCPXML em OTIO está duvidoso nas fontes** | baixa |
| AAF | Avid/Pro Tools; Storyboard Pro importa/exporta | adaptador OTIO depende de PyAAF2; sem marcadores/efeitos em versões antigas | média/baixa (docs antigas) |
| OpenColorIO | gestão de cor (ACES, etc.) | última release v2.6.0 em 2026-10-01 (API, via resumo) | média |
| OpenUSD | cena 3D/layout; Harmony 25 anuncia pipeline USD | release v26.08 (2026-07-20); licença "Tomorrow Open Source Technology License 1.0" (baseada em Apache-2.0, só a seção de marcas difere) | alta (licença), média (release) |
| OpenEXR | sequências de imagem em alta faixa | release v3.5.2 em 2026-10-03 (API, resumo) | média |
| Alembic, PSD, TGA | — | **não pesquisados** | n/a |
| C2PA (proveniência) | assinatura e histórico de mídia | `c2pa-rs` (Rust) é dual MIT/Apache-2.0, implementa subconjunto da spec **2.4** + CAWG; MSRV 1.96 | alta (README lido) |

Fontes: <https://github.com/PixarAnimationStudios/OpenUSD>, <https://github.com/AcademySoftwareFoundation/OpenColorIO>, <https://github.com/AcademySoftwareFoundation/openexr>, <https://raw.githubusercontent.com/contentauth/c2pa-rs/main/README.md>, <https://raw.githubusercontent.com/PixarAnimationStudios/OpenUSD/release/LICENSE.txt>, <https://www.cartoonbrew.com/sponsored-by-toon-boom/with-harmony-25-toon-boom-animation-introduces-helpful-tools-across-the-2d-animation-pipeline-247741.html>.

Nota de consistência: o C2PA em vídeo perde credenciais em re-encode/upload (<https://contentauthenticity.org/blog/the-state-of-content-authenticity-in-2026> e crítica em <https://truescreen.io/articles/c2pa-standard-history-limitations/>, ambos via resumo, **média**). Portanto a procedência precisa estar também **dentro do projeto** (banco de dados de proveniência) e não só no arquivo exportado.

### 2.5 Como um programa único deve modelar o projeto internamente (proposta)

**[INFERÊNCIA]** — o que segue é desenho meu, derivado dos fatos acima.

```
Projeto (série/filme)
 └─ Temporada
     └─ Episódio (話数)  ── roteiro (versões), e-conte (versões), pacote de settei vigente
         └─ Parte (A/B/OP/ED/CM-eyecatch)
             └─ Cena
                 └─ Corte (カット)      ← unidade atômica de produção e de timesheet
                     ├─ Metadados: nº do corte, duração (s+quadros), frame rate, aspect
                     ├─ Vínculos: linhas de roteiro, painel(éis) de storyboard, falas/áudio
                     ├─ Timesheet: colunas (Ação | Células A,B,C... | Fundo | Câmera | Diálogo | Som)
                     ├─ Camadas/células (levels): desenhos (original, in-between, pintura), fundo, efeitos
                     └─ Estados por etapa: layout → original → in-between → pintura → composição → retake(n)
Assets compartilhados: personagem/settei, fundos, adereços, bancos de cores, luts/OCIO
Histórico: snapshots imutáveis por corte/etapa; autoria humana vs. assistência de IA (seção 3.9)
```

Mapeamento de exportação (proposta):

| Destino | O que exportar | Formato |
|---|---|---|
| CSP EX / TDTS / OpenToonz / TVPaint | timesheet por corte | **XDTS** (cuidar do offset 0/1) |
| Premiere/Resolve/Avid | sequência de cortes montada | EDL, FCP7 XML/FCPXML, AAF, **OTIO** |
| Compositing / grade | frames finais | EXR/PNG/TGA sequences + **OCIO** config |
| Pipeline 3D/Unreal | layout/câmera/props | **USD** (e/ou Alembic) |
| Tracker (Kitsu/AYON/Flow) | cortes, estados, previews | API (Gazu/REST), CSV, OTIO |
| Entrega | mídia + proveniência | vídeo + manifesto **C2PA** e relatório próprio |

---

## 3. IA na produção de anime (estado em 2026)

### 3.1 Resumo executivo desta seção

- **Para o fluxo de produção "controlado", o estado da arte útil é pesquisa acadêmica + algumas bases abertas** (in-betweening/colorização guiadas por keyframes e referências), **não** os geradores de vídeo comerciais.
- **Custo de hardware real**: modelos de vídeo maiores (ToonComposer ~57 GB de VRAM para 61 quadros a 480p) **não rodam** em GPU de consumo comum; AniDoc declara ~14 GB; ToonCrafter ~10–12 GB (versões da comunidade) a 24 GB.
- **Licenciamento é o ponto mais perigoso**: código "Apache-2.0" frequentemente repousa em pesos base restritivos (SVD, FLUX Kontext dev) ou em dados de treino de origem duvidosa.
- **Contexto jurídico/social** (Japão, EUA, UE) é mais restritivo do que o discurso das ferramentas; ver 3.7.

### 3.2 In-betweening e colorização (fluxo 2D)

| Modelo | Função | Licença (código / pesos) | Último push visto | VRAM / runtime | Confiança |
|---|---|---|---|---|---|
| **ToonCrafter** (SIGGRAPH Asia 2024) | interpolação generativa entre dois quadros cartoon | Código: **Apache-2.0** (API). README diz ser "exploração de pesquisa, não produto comercial" e **não declara licença dos pesos**; HF sem licença no metadado | 2025-03-19 | ~24 GB na tabela oficial (A100); comunidade ~10–12 GB; 320×512, até 16 quadros; PyTorch | alta (README/API) |
| **AniDoc** (CVPR'25) | colorização guiada por referência + interpolação a partir de sketches | **Apache-2.0** declarado; depende do **Stable Video Diffusion** (HF: licença "stable-video-diffusion-community", categoria `other`) | 2025-04-15 | "~14 GB" inferência; espera 14 quadros; PyTorch | alta (README/API/HF) |
| **ToonComposer** (ICLR 2026) | in-betweening + colorização numa etapa a partir de keyframes esparsos | GitHub: "Other" (ver arquivo LICENSE); **HF: MIT**; base **Wan2.1-I2V-14B-480P** (Apache-2.0) | 2025-08-20 (pesos 2025-08-15, HF alterado em 2026-03-05) | **~57 GB** (61 quadros, 480p); 480p/608p | alta |
| **LVCD** | colorização de vídeo de lineart por referência (difusão) | código **sem licença** (API: nulo) | 2025-01-06 | não verificado | alta (ausência de licença) |
| **AnimeInbet** (ICCV'23) | in-betweening de lineart geometrizado | **sem licença** na API | 2024-04-15 | não verificado | alta |
| **AniSora V1/V2/V3.x** (Bilibili Index) | geração de anime I2V, controle por máscara, interpolação, etc. | **Apache-2.0** declarado para V2 e V3 (pesos); V1 sem licença declarada na página; base V1 CogVideoX-5B, V2 Wan2.1-14B aprimorado, V3.2 Wan2.2 | 2026-07-16 (repo); anymask 2025-10-31 | V1 em RTX 4090; V3.1 versão de 12 GB; V2 otimizado para NPU Ascend 910B | alta (README/API) |
| **MangaNinja** (CVPR'25, ali-vilab) | colorização de lineart com seguimento preciso de referência | não verificei licença | — | requer SD, CLIP, ControlNet + pesos próprios | média |
| **AnimeColor**, **SketchColour**, **DACoN**, **AniDepth**, "Workflow-friendly Anime In-betweening" | pesquisa 2025 | não verificado | — | — | média/baixa |
| **SketchKeyAnime** (arXiv 2606.19958, 2026-06-18) | síntese de animação de poucos keysketches + 1 imagem de referência | **sem código** na página | — | — | alta (resumo lido) |
| **CACANi** | software comercial de in-betweening/pintura automática, raízes na NTU (desde 1996) | proprietário; preço US$299 em listagem de terceiros | — | — | baixa. Não achei novidade de 2026 |
| **Cascadeur 2026.x** (3D) | in-betweening de pose/animação com IA | proprietário | v2026.2 em 2026-08 (resumo) | — | média (cgchannel) |

Fontes principais: <https://raw.githubusercontent.com/Doubiiu/ToonCrafter/main/README.md>, <https://raw.githubusercontent.com/ant-research/AniDoc/main/README.md>, <https://raw.githubusercontent.com/TencentARC/ToonComposer/main/README.md>, <https://huggingface.co/api/models/TencentARC/ToonComposer>, <https://huggingface.co/api/models/stabilityai/stable-video-diffusion-img2vid-xt>, <https://raw.githubusercontent.com/bilibili/Index-anisora/main/README.md>, <https://arxiv.org/abs/2606.19958>, <https://arxiv.org/html/2508.10881v1>, <https://github.com/luckyhzt/LVCD>, <https://github.com/lisiyao21/AnimeInbet>, <https://openaccess.thecvf.com/content/CVPR2025/html/Liu_MangaNinja_Line_Art_Colorization_with_Precise_Reference_Following_CVPR_2025_paper.html>, <https://www3.ntu.edu.sg/home/ashsseah/cacani.htm>.

**Observações críticas**
- **[FATO]** As afirmações de qualidade do ToonComposer vêm dos próprios autores (benchmark próprio, estudo com 47 pessoas): <https://www.aimodels.fyi/papers/arxiv/tooncomposer-streamlining-cartoon-production-with-generative-post-keyframing>. Trate como direcional.
- **[FATO]** O conjunto **Sakuga-42M** (dados de treino de várias pesquisas de animação) tem origem de clipes de anime não esclarecida e o artigo no arXiv consta como retirado; **não achei licença** do conjunto. <https://arxiv.org/abs/2405.07425v1>, <https://paperswithcode.com/dataset/sakuga-42m> — média. **[INFERÊNCIA]** Qualquer modelo treinado nele carrega risco de dados de origem duvidosa.
- **[INFERÊNCIA]** Todos esses modelos exigem o **quadro de entrada de 14–16 quadros, 320–608 px** ou semelhantes; nenhum é compatível com a prática de estúdio (4K, 24 fps em *twos*/*threes*). Útil como assistente de rascunho/esboço, não como produção final.

### 3.3 Extração de lineart, segmentação, fundos, ControlNet

| Item | Licença | Último push | Observação | Confiança |
|---|---|---|---|---|
| **Anime2Sketch** | MIT | 2026-09-08 | default 512×512; pesos em Google Drive; dados de treino não descritos | alta |
| **anime-segmentation** (SkyTNT) | Apache-2.0 | 2025-05-21 | segmentação de personagem | alta |
| **Style2Paints** | Apache-2.0 | 2023-08-01 | **abandonado** | alta |
| **ControlNet** | Apache-2.0 | 2024-02-25 | base de controle por lineart/profundidade; código estável | alta |
| **ComfyUI** | **GPL-3.0** | 2026-10-16 | grafo de nós; ecossistema de nós para ToonCrafter/Wan/AniSora | alta |
| **Krita AI Diffusion** | GPL-3.0 | 2026-10-03 | plugin de ilustração | alta |
| **FLUX.1 Kontext [dev]** | pesos **não comerciais** ("flux-1-dev-non-commercial-license") | 2026-01-01 (HF) | melhor para edição guiada de imagem, mas **não pode** ser embutido em produto comercial sem outra licença | alta (HF API) |
| **Qwen-Image-Edit-2509** | **Apache-2.0** | 2025-09-22 (HF) | edição de imagem com licença permissiva | alta (HF API) |

Fontes: <https://api.github.com/repos/Mukosame/Anime2Sketch>, <https://huggingface.co/api/models/black-forest-labs/FLUX.1-Kontext-dev>, <https://huggingface.co/api/models/Qwen/Qwen-Image-Edit-2509>, e as páginas de repositório listadas.

**[INFERÊNCIA — implicação de arquitetura]** GPL-3.0/AGPL-3.0 (ComfyUI, Krita AI Diffusion, Style-Bert-VITS2) não devem ser **linkados** a um aplicativo fechado; devem rodar como **processos separados, instalados pelo usuário**, conectados por API. Isso é decisão jurídica a validar com advogado.

### 3.4 Upscaling, restauração, interpolação de quadros

| Item | Licença | Último push | Observação |
|---|---|---|---|
| **RIFE** (ECCV2022) | MIT | 2025-09-10 | base |
| **Practical-RIFE** | MIT (inclusive pesos, pelo README) | 2026-08-27 | modelo recomendado 4.25 (v4.26 em 2024-09-21, 4.25.lite em 2024-10-20); sem modelo específico de anime |
| **Real-ESRGAN** | BSD-3-Clause | 2024-08-06 | variantes de anime existem; repo sem commits desde 2024 |
| **Real-CUGAN** (Bilibili ailab) | arquivo LICENSE presente (1 KB, **conteúdo não lido**); repo sem licença no nível raiz | 2023-08-02 | upscaler de anime |
| **Anime4K** | MIT | 2024-08-17 | upscaler em tempo real (shaders) |
| **waifu2x** | MIT | 2023-05-04 | legado |
| **rife-ncnn-vulkan** | licença **não declarada** no README lido; usa ncnn | — | alternativa Vulkan sem CUDA, roda em GPU Intel/AMD/NVIDIA e CPU |

Fontes: <https://raw.githubusercontent.com/hzwer/Practical-RIFE/main/README.md>, <https://github.com/xinntao/Real-ESRGAN>, <https://github.com/bilibili/ailab>, <https://github.com/bloc97/Anime4K>, <https://github.com/nagadomi/waifu2x>, <https://raw.githubusercontent.com/nihui/rife-ncnn-vulkan/master/README.md>. Confiança alta nas licenças/datas, média nas conclusões de uso.

**[FATO, média]** Interpolação genérica falha em animação limitada: movimento entre quadros de desenho é não linear, mistura gera contorno duplo e modo "nítido" gera quadros ondulados. Fontes: <https://svp-team.com/wiki/Watching_anime>, <https://resourcecenter.ieee.org/conferences/icip-2022/spsicip22vid0064>, <https://github.com/hyw-dev/DRBA>.
**[INFERÊNCIA]** Em animação feita em *twos/threes*, a interpolação desfaz a decisão artística de ritmo (hold-and-jump). Deve ser **opcional na entrega** e **nunca** no pipeline de produção; **o timesheet precisa preservar a cadência original**, e qualquer interpolação precisa de deduplicação prévia (prática relatada em fóruns de SVP).

### 3.5 Geração de vídeo (fechada e aberta)

| Modelo | Acesso | Licença/estado | Relevância para anime | Confiança |
|---|---|---|---|---|
| **Wan 2.1 / 2.2** | **pesos abertos, Apache-2.0** (repositórios `Wan-Video/Wan2.1`, `Wan2.2`; HF `Wan-AI/Wan2.2-I2V-A14B` Apache-2.0) | Wan2.2 push 2026-09-21; também `Wan-Animate-2` (2026-08-08) e `Wan-Dancer` (2026-07-17), ambos Apache-2.0 sobre base Wan 2.2 | base de AniSora e ToonComposer; é a trilha aberta mais viável | alta |
| **Wan 2.5 / 2.6 / 2.7 / 3.0** | **somente API**, sem pesos publicados segundo verificação de terceiro | Wan 2.5-Preview 2025-09-24; 2.6 em 2025-12; 2.7 em abr/2026; 3.0 beta em ago/2026 | fontes **conflitam** (algumas dizem Apache-2.0 aberto, mas o autor da checagem não achou pesos na HF/GitHub) | **média-baixa**: checar HF antes de apostar |
| **Sora 2** (OpenAI) | app encerrado em 2026-04-26; API encerrada em 2026-09-24; permanece como pesquisa | — | **ferramenta fechada e descontinuada**: nunca dependa dela | média-alta (The Decoder cita página de ajuda) |
| **Seedance 2.0 / 2.5** (ByteDance) | API | disputa de direitos autorais (Disney, Paramount, MPA) em fev–mar/2026; lançamento global suspenso segundo Caixin; governo japonês abriu investigação por vídeos de personagens de anime | referência comum para consistência por imagens de referência | média (fontes de imprensa) |
| **Kling 3.0**, **Veo 3.1**, **Runway Gen-4.5** | API | — | ranqueamentos vêm de blogs de marketing; **nenhum teste controlado de anime** achado; Kling é o mais citado para ação/multi-plano, Seedance para fixar personagem por referência, Runway para controle de câmera | **baixa** |

Fontes: <https://howaiworks.ai/blog/alibaba-wan-open-weights-stopped-at-2-2>, <https://api.github.com/orgs/Wan-Video/repos>, <https://huggingface.co/api/models/Wan-AI/Wan2.2-I2V-A14B>, <https://the-decoder.com/openai-sets-two-stage-sora-shutdown-with-app-closing-april-2026-and-api-following-in-september/>, <https://au.variety.com/2026/film/news/bytedance-safeguards-seedance-disney-legal-threat-ip-violations-33139/>, <https://www.caixinglobal.com/2026-03-17/bytedance-halts-global-rollout-of-seedance-20-amid-copyright-dispute-102423674.html>, <https://www.elser.ai/blog/kling-vs-seedance-vs-veo-for-anime-videos-which-ai-model-wins-in-2026>.

**[INFERÊNCIA]**: os geradores de vídeo de prompt têm **baixo controle** (consistência de personagem, timing, linhas, número de quadros) e contradizem o modelo de produção por corte/camada. Úteis (se úteis) para *previz*/animatic exploratório e *concept*, **não** para o corte final de uma série. Também nenhum entrega camadas separadas (cel/fundo), o que impede integrar com timesheet.

### 3.6 Voz, TTS, dublagem e lip-sync

| Item | Licença (código) | Observações | Confiança |
|---|---|---|---|
| **VOICEVOX Engine** | marcado "Other" na API (no site: uso comercial permitido com crédito, e cada personagem tem termos próprios) | TTS japonês de estilo anime; push 2026-10-02 | média (termos por personagem só via blogs) — <https://github.com/VOICEVOX/voicevox_engine>, <https://ondoku3.com/en/post/voicevox/> |
| **Style-Bert-VITS2** | **AGPL-3.0** | modelos padrão têm termos próprios; dados de treino precisam de permissão comercial | alta (licença), média (resto) — <https://github.com/litagin02/Style-Bert-VITS2> |
| **GPT-SoVITS** | **MIT** | clonagem com ~1 min de áudio; push 2026-10-08 | alta (licença) |
| **Fish Speech** | API: NOASSERTION; fontes secundárias: código Apache, **pesos CC-BY-NC-SA** nas versões antigas e licença customizada paga nas novas | **tratar como não comercial** | baixa/média |
| **IndexTTS2** | pesos sob licença Bilibili (comercial abaixo de limites) | só inglês/chinês nos resultados | baixa |
| **LatentSync** (lip-sync) | Apache-2.0 | push 2025-06-20 | alta |
| **MuseTalk** | "Other" | push 2025-09-26 | alta (marcação), licença real não lida |

Fontes: as páginas citadas e <https://github.com/RVC-Boss/GPT-SoVITS>, <https://github.com/fishaudio/fish-speech>, <https://github.com/bytedance/LatentSync>, <https://github.com/TMElyralab/MuseTalk>, <https://tts.ai/voices/indextts2/>.

**Posição de dubladores japoneses [FATO, média]**: a campanha **NOMORE 無断生成AI** (Oct/2024) reúne dubladores famosos e 13 organizações (inclui a Japan Actors Union); não se opõe à IA em si, mas pede rotular voz gerada, consentimento e **não usar IA gerativa em dublagem**; direito de personalidade/*publicity* não é lei escrita no Japão, daí o apelo por regra de mercado. Em 2026-05-28 um grupo de dubladores entregou documento a um grupo de estudo do Ministério da Justiça sobre responsabilidade civil por uso não autorizado de imagem e voz. Fontes: <https://automaton-media.com/en/?p=38799>, <https://newswitch.jp/p/44625>, <https://www.moj.go.jp/content/001463284.pdf> (resumos de busca). Não achei declaração datada de 2025/26 da própria JAU.
**Contexto EUA/Europa**: NAVA reporta 21% dos dubladores com perda de trabalho para IA em 2026 (14% em 2025), e dubladores franceses processaram empresas de clonagem em 2026-02: <https://www.marketscale.com/industries/pro-av/nava-calls-for-safeguards-for-voice-actors>, <https://oecd.ai/en/incidents/2026-02-03-1cf6> (baixa/média).
**Incidente Crunchyroll (2025)**: legendas geradas por IA por fornecedor terceirizado ("ChatGPT said" apareceu no texto alemão), empresa reconheceu violação de contrato; houve também recuo em plano de dublagem por IA. <https://multilingual.com/crunchyroll-ai-subtitles/>, <https://www.dexerto.com/tv-movies/crunchyroll-reverses-ai-plan-subtitles-anime-3180612/> (média).

### 3.7 Posição de estúdios, associações, plataformas e riscos legais

**Japão**
- **Lei**: o Art. 30-4 da Lei de Direitos Autorais permite uso para "análise de informação" (incluindo treino) desde que não seja para fruição do conteúdo e sem prejuízo injustificado do titular; o documento "Entendimento Geral sobre IA e Direito Autoral" (15/mar/2024, não vinculante) esclarece que o limite cai quando há "propósito de fruição" junto do analítico; saída gerada segue as regras gerais de infração; obra gerada autonomamente por IA não tem direito autoral, uso criativo humano pode ter. Fontes: <https://practiceguides.chambers.com/practice-guides/ai-intellectual-property-2026/japan>, <https://globallawexperts.com/generative-ai-copyright-japan/>, <https://www.wipo.int/documents/d/office-japan/docs-en-tatsuhiro-ueno_general-understanding-on-ai-and-copyright-in-japan_set.pdf> (PDF não lido). **Confiança média**; uma atualização de 2026 do documento foi mencionada só por fonte secundária fraca — **não verificada**. Um "Principles-Code" não vinculante para IA generativa teria sido publicado em ago/2026 (única fonte; **baixa**).
- **CODA** (36 membros em abr/2025, incluindo Ghibli, Toei, Aniplex, Shueisha, Shogakukan, Square Enix) enviou carta formal à OpenAI em 2025-10-27: pede que o conteúdo dos membros **não seja usado para treino sem permissão** e que haja resposta séria a reclamações de infração; argumenta que o regime de *opt-out* do Sora 2 é incompatível com a lei japonesa. <https://coda-cj.jp/en/news/817/> — **alta** (página primária lida). O governo (ministro de PI e IA) comentou o caso; menção a possível uso da Lei de Promoção de IA aparece só em reportagem secundária — baixa.
- **AJA** (Association of Japanese Animations) emitiu declaração conjunta com a Japan Cartoonists Association e 17 editoras (final de 2025): não é contra a IA, mas exige cumprimento da lei no treino e na saída, permissão prévia, transparência de dados e compensação. <https://dev.animationmagazine.net/?p=3280844> (resumo) e 18 empresas com a CODA em 2025-10-30 (<https://www.animenewsnetwork.com/news/2025-10-31/18-anime-manga-companies-publish-joint-statement-criticizing-use-of-generative-ai-with-sora2-program/.230524>) — média.
- **Estúdios**: WIT Studio (série *Ascendance of a Bookworm* T4, 2026) reconheceu fundos gerados por IA na abertura (produzidos por empresa terceirizada), pediu desculpas e prometeu substituir por arte desenhada à mão; alegou política geral de não usar IA gerativa em animação e atribuiu a falha a controle de qualidade/produção. Fontes: <https://automaton-media.com/en/news/ascendance-of-a-bookworm-anime-studio-acknowledges-gen-ai-use-in-opening-promises-to-replace-offending-scenes-with-hand-drawn-art/> (só manchete lida) e <https://kotaku.com/ascendance-bookworm-ai-anime-wit-crunchyroll-2000686660> (resumo) — **média**. *The Dog & the Boy* (Netflix Japão/WIT/rinna, 2023) usou IA em todos os fundos de um curta. Netflix montou "INKubator", estúdio de animação "GenAI-native", segundo vagas (Cartoon Brew, 2026-05-15): <https://www.cartoonbrew.com/artificial-intelligence/netflix-inkubator-artificial-intelligence-animation-studio-260906.html> — alta (artigo lido). A produção de *Sekiro: No Defeat* declarou não usar IA gerativa (resumo, baixa). Sem evidência de uso gerativo na Toei em 2026 (busca negativa).
- **Reação do setor**: pesquisa NAFCA 2023 mostra mais de 70% a favor de alguma regulação (<https://automaton-media.com/en/news/20230914-21651/>, média). Um estúdio de Nagoya (K&K Design) usa IA em fundos e pintura (resumo, baixa).
- **Auxílio estatal à produtividade**: JST CRONOS (seção 2.1) enquadra a IA como meio, não fim.

**EUA**
- O Copyright Office (Parte 2, jan/2025): IA como assistente não afeta registrabilidade; saída 100% gerada não é protegível; *prompts* sozinhos não bastam; partes humanas podem ser protegidas. <https://www.akingump.com/en/insights/ai-law-and-regulation-tracker/us-copyright-office-releases-part-2-of-ai-report> — média-alta. A Suprema Corte **negou certiorari** em *Thaler v. Perlmutter* em 2026-03-02, mantendo a exigência de autor humano (sem valor de precedente de mérito). <https://www.scotusblog.com/cases/thaler-v-perlmutter/> (resumo) — média. **Implicação [INFERÊNCIA]**: obra com quadros gerados em larga escala pode ter proteção reduzida; **precisa registrar, por corte, o que foi feito por humano e o que foi por IA**.
- Animation Guild (EUA): contrato de 3 anos com cláusulas de "notificação e consulta" sobre IA, contestadas por membros; SAG-AFTRA: acordos de animação com proteção sobre IA ratificados em 2023 (95,5%), possível renovação em 2026 sem cobertura achada. <https://www.hollywoodreporter.com/business/business-news/animation-guild-deal-reached-fight-over-ai-concludes-1235975581/>, <https://sagaftra.org/tv-animation-agreements> — média. Não achei posição do sindicato japonês (JAniCA) — lacuna.

**União Europeia**
- Art. 50 do AI Act (transparência/rotulagem de conteúdo sintético) vigente desde 2026-08-02; o *Digital Omnibus* teria deslocado a marcação legível por máquina (Art. 50(2)) para 2026-12-02 para sistemas já no mercado — fontes **conflitam** sobre se está formalmente adotado; um Código de Prática de rotulagem teria sido publicado em jun/2026. <https://www.jonesday.com/en/insights/2026/01/european-commission-publishes-draft-code-of-practice-on-ai-labelling-and-transparency>, <https://compliancehub.wiki/eu-ai-act-article-50-transparency-digital-omnibus-2026/> — **baixa/média**: validar no Jornal Oficial.

### 3.8 O que é aberto/local e o que não é (resumo para decisão)

- **Rodáveis localmente, licença permissiva no código (verificar pesos e base)**: RIFE/Practical-RIFE (MIT), Anime4K (MIT), Real-ESRGAN (BSD-3), Anime2Sketch (MIT), anime-segmentation (Apache-2.0), ControlNet (Apache-2.0), LatentSync (Apache-2.0), GPT-SoVITS (MIT), Wan 2.1/2.2 e derivados (Apache-2.0), AniSora V2/V3 (Apache-2.0), Qwen-Image-Edit-2509 (Apache-2.0), ToonComposer (MIT na HF, mas VRAM alta).
- **Aberto mas com restrição na base ou nos dados**: AniDoc (SVD community), ToonCrafter (pesos sem licença explícita, texto "pesquisa"), FLUX Kontext dev (não comercial), Fish Speech (pesos não comerciais), LVCD/AnimeInbet (sem licença = *todos os direitos reservados* por padrão — **[INFERÊNCIA jurídica]**).
- **Copyleft forte**: ComfyUI/Krita AI Diffusion (GPL-3.0), Style-Bert-VITS2 (AGPL-3.0). Rodar como processo externo.
- **Runtimes**: a maioria é PyTorch; o conjunto RIFE/Real-ESRGAN/Real-CUGAN tem portes `ncnn-vulkan` (sem CUDA) e há integração TensorRT/ONNX em projetos como vs-mlrt (**não consegui verificar a licença** por limite de taxa da API). **[INFERÊNCIA]** Para empacotar um produto Windows, priorizar ONNX Runtime/ncnn onde existir porte, e tratar PyTorch pesado como "extensão" baixada pelo usuário.

### 3.9 Implicações para o design do produto (derivadas)

**[INFERÊNCIA]**, com base em 3.7:

1. **IA como assistente opcional, desligada por padrão, com chave global do projeto** ("política de IA do projeto": proibido / só não gerativo / permitido com rótulo). Estúdios com política de "sem IA gerativa" (WIT, Celsys) precisam poder **garantir** isso tecnicamente (inclusive bloquear plugins).
2. **Separação "IA analítica" vs "IA gerativa"**: segmentação, extração de linhas, deduplicação de quadros, OCR de roteiro e *upscale* de entrega são aceitos com muito menos atrito do que gerar desenho/voz/fundo. A Celsys já traça essa linha (aceita colorização e detecção de pose, recusa geração de imagem).
3. **Proveniência por camada/cel/quadro**: registrar autor humano, ferramenta, modelo (nome, versão, hash de pesos, licença), parâmetros, entradas (inclusive referências) e se a saída foi editada por humano. Exportar relatório e manifesto C2PA, **mas guardar o registro principal no projeto** (C2PA se perde em re-encode).
4. **Tabela de licenças de modelo** embutida (por modelo, "uso comercial?: sim/não/desconhecido") com bloqueio do botão "exportar para entrega" quando houver saída gerada por modelo de licença incompatível. Este é um diferencial que ninguém oferece (seção 4).
5. **Dados dos clientes não vão a terceiros por padrão** (processamento local; nuvem opt-in por função). A Toon Boom já usa a promessa "não treinamos, apagamos após uso" como argumento de venda.
6. **Voz**: só clonagem com **consentimento registrado** (contrato anexado ao asset de voz) e rotulagem automática; seguir a linha da campanha NOMORE.
7. **Respeitar a gramática do anime**: nenhuma IA deve reescrever o timing (preservar *twos/threes*) nem tocar nos quadros-chave sem aprovação; resultados de IA entram como *camada candidata* versionada, nunca sobrescrevem.

---

## 4. Lacunas do mercado (oportunidade de produto)

**[INFERÊNCIA]**, apoiada nas buscas (ausências aqui significam "não achei", não "não existe").

1. **Rastreabilidade ponta a ponta roteiro→corte→animatic→timesheet→entrega.** Storyboard Pro gera cenas a partir de script (Ember), Kitsu rastreia planos, CSP/TDTS lidam com timesheet — mas **nenhum liga linha de roteiro ↔ corte ↔ camadas ↔ revisão** num único grafo. Kitsu usa *shot*, não *cut + timesheet*.
2. **Timesheet como cidadão de primeira classe fora do Japão.** O XDTS é usado por CSP, TDTS, OpenToonz, TVPaint e Callipeg, mas o resto do ecossistema (OTIO, Kitsu, Premiere) não o lê. **Oportunidade concreta: adaptador XDTS⇄OTIO e import/export de XDTS completo** (inclusive offset 0/1, símbolos, keyframe/reference marks).
3. **Settei versionado e vinculado ao corte, com alerta de mudança** — hoje PDFs/pastas; não achei ferramenta que modele isso.
4. **Gestão de retakes (リテイク) com comparação de quadros e timesheet lado a lado** — o Save Point for アニメ (2019) mirava isso, mas o status em 2026 não é confirmado e é produto proprietário.
5. **Painel de licenças e proveniência de IA** (modelo+licença+dados+consentimento) integrado ao projeto e à entrega; hoje cada ferramenta de IA vive isolada (ComfyUI, plugins) sem esse controle.
6. **IA local de produção controlada**: in-betweening/colorização guiada por keyframes (ToonComposer/AniDoc) é pesquisa que exige 14–57 GB de VRAM e roda em scripts Python; falta um **empacotamento com limites claros** (modo "rascunho 480p" local, modo "qualidade" remoto opt-in).
7. **Formato vertical/japonês em ferramentas ocidentais** — roteiros e e-contes japoneses (縦書き, colunas de corte) não são o padrão em Final Draft/Storyboard Pro; templates de e-conte do CSP existem, mas são só layout, não dados estruturados.
8. **Rastreador "de estúdio pequeno" local-first** — Kitsu é web/servidor (AGPL); AYON é focado em pipeline VFX/3D; para um programa desktop Windows único, falta algo *embutido* com sincronização opcional. Kitsu teve queda de receita em 2026 (risco de fornecedor).
9. **Entrega em padrão aberto**: exportar o projeto como pacote (OTIO + XDTS + EXR + OCIO + relatório de proveniência), para que o estúdio não fique preso ao programa.
10. **Colaboração entre departamentos sem "cut bag" físico**: a transferência digital de originais/correções entre animadores, diretores de animação e acabamento ainda é manual em muitos estúdios (inferência a partir de Save Point/TDTS).

---

## 5. Lacunas desta pesquisa (o que eu **não** consegui confirmar)

- **Texto oficial da spec XDTS** (cadastro obrigatório) e a lista de `fieldId`s. Próximo passo: baixar os PDFs em <https://www.celsys.com/en/xdts/> e ler.
- Detalhes do projeto JST CRONOS (PDF criptografado) e dos slides WIPO sobre Art. 30-4.
- Licença exata de: OpenRV, AYON backend, VOICEVOX Engine, MuseTalk, Real-CUGAN, rife-ncnn-vulkan, vs-mlrt, ToonComposer (LICENSE no GitHub vs MIT na HF), pesos do ToonCrafter.
- Status 2026 de Save Point for アニメ, uso real de Kitsu/AYON em estúdio japonês, e posição do sindicato **JAniCA**.
- Preços oficiais de Flow Production Tracking, ftrack, Frame.io V4, Anchorpoint.
- Formatos Alembic/PSD/TGA e FCPXML (versão atual) e AAF — não pesquisados a fundo.
- Situação real de **Wan 2.7/3.0** (aberto ou não) e o ranking de modelos de vídeo para anime (fontes de baixa qualidade).
- Formato real de roteiro e e-conte em estúdio japonês (usei guias de concurso e amostras de livros).
- Efeito prático de IA sobre desenho: ninguém publicou teste de produção independente dos modelos de in-betweening com material de estúdio (só benchmarks dos autores).

## 6. Índice de fontes principais

Trackers e formatos: <https://github.com/cgwire/kitsu>, <https://github.com/cgwire/zou>, <https://blog.cg-wire.com/kitsu-2026-roadmap>, <https://kitsu.cg-wire.com/>, <https://github.com/ynput/ayon-launcher>, <https://github.com/ynput/ayon-backend>, <https://github.com/PrismPipeline/Prism>, <https://github.com/AcademySoftwareFoundation/OpenTimelineIO>, <https://www.celsys.com/en/xdts/>, <https://www.celsys.com/en/clipsolution/xdts/>, <https://tips.clip-studio.com/en-us/articles/1372>, <https://doc.tvpaint.com/docs/timesheet/timesheet-import-export>, <https://github.com/opentoonz/opentoonz>.
Pré-produção: <https://fountain.io/syntax/>, <https://docs.toonboom.com/help/storyboard-pro-25/storyboard/release-notes/storyboard-pro-25-1-release-notes.html>, <https://docs.toonboom.com/help/storyboard-pro-25/storyboard/ai/analyze-script-with-ai.html>, <https://www.celsys.com/en/topic/20251113>, <https://forest.watch.impress.co.jp/docs/review/460218.html>.
IA e política: ver URLs inline nas seções 3.2–3.7.
