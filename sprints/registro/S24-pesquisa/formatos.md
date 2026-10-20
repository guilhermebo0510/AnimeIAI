# S24 — Pesquisa: formato de projeto, pincéis e biblioteca de recursos do AnimeIAI

Data: 2026-10-19. Só leitura de código; nenhum arquivo-fonte foi alterado. Cada afirmação de fora do código traz a fonte; o que **não** consegui confirmar está marcado como **[não verificado]** (e não entra como base de decisão).

Resumo da recomendação:

1. **Projeto**: arquivo único `.aiaiproj` = **banco SQLite** (rusqlite `bundled`), com a pasta `.aiai` atual continuando a abrir para sempre.
2. **Pincel/material**: arquivos **zip** pequenos e portáteis (`.aiaibrush`, `.aiaibrushes`, `.aiaimat`...) com `manifest.json` comum (autor, licença SPDX, fonte, versão, hashes).
3. **Biblioteca**: pasta visível `Documentos\AnimeIAI\` onde a **pasta é a verdade** e o índice (tags, miniaturas) é só cache reconstruível.

---

## (a) Estado atual e lacunas

### a.1 Como funciona hoje (lido do código)

**Projeto** (`app/crates/aiai-core/src/persist.rs`, `blob.rs`, `session.rs`; `aiai-studio/src/app/{doc,autosave,start}.rs`):

- O projeto é uma **pasta** `nome.aiai/` com `project.json` (documento canônico, indentado), `blobs/xx/<blake3>` (um arquivo por tile 256x256, máscara, imagem, áudio), `wal.ndjson` e `nome.aiaiproj`. Esse `.aiaiproj` é um JSON minúsculo `{aiai_project, name, format}` que só aponta para a própria pasta (`write_opener`, `resolve_project_dir`).
- **Salvar** (`persist::save`):
  - confere espaço livre (mín. 512 MB, `MIN_FREE_BYTES`);
  - grava em paralelo os blobs que ainda não existem em disco (`write_blob` retorna cedo se `path.exists()`). Cada blob vai por `write_atomic_with(durable=true)`: arquivo `.tmp-write`, `sync_all`, confere o tamanho, `rename`;
  - grava `project.json` pelo mesmo caminho atômico;
  - recria o WAL com cabeçalho `wal_base_revision` e roda a coleta de lixo da memória.
  - `retry_io` repete por ~1,5 s em erros de bloqueio do Windows (5, 32, 33, 1224).
- **WAL** (`Wal::append`): a cada comando aplicado (`session.record`) escreve uma linha NDJSON com `LogEntry{seq, revision, op, description, author, time_ms, command}` e um `sync_data`. Os blobs novos são gravados **sem fsync** (`durable=false`) e anotados antes por linhas `{"hash":...}`.
- **Abrir** (`persist::open`):
  - lê `project.json`, recusa `format > FORMAT_VERSION` (que vale **1**; não existe nenhuma função de migração);
  - lê **todos** os blobs referenciados para a memória, conferindo o BLAKE3 (`insert_clean`);
  - reaplica o WAL com `revision > saved_rev`, parando na primeira linha ilegível (`wal_truncated_at`) ou no primeiro erro de replay (`replay_error`);
  - o documento fica "não salvo" e o undo dos comandos reaplicados continua valendo (`adopt_document`).
- **Autosave** (`autosave.rs`): só existe para **projeto sem pasta**. A cada 120 s, com alterações, `persist::snapshot` grava uma pasta `.aiai` em `%LOCALAPPDATA%\AnimeIAI\recovery\<unix>-<pid>.aiai`, numa thread, com clone do `Project` e do `BlobStore` (os blobs são `Arc`). Saída normal apaga a cópia. Na próxima execução a tela inicial lista as sobras ("Recuperar / Descartar") e abrir uma delas desanexa o WAL e marca como não salvo (`doc.rs:153`). Projeto **com** pasta não tem timer: depende só do WAL, e o `project.json` só muda no Salvar manual.
- **Pincéis** (`aiai-studio/src/app/brushes.rs`, `aiai-paint/src/brush.rs`, `brush_import.rs`):
  - os do usuário ficam em `%APPDATA%\AnimeIAI\brushes.json`, um array JSON de `BrushPreset` (sem envelope nem versão);
  - as imagens de ponta/textura ficam em `brush_assets\<blake3>.aim` (máscara "AIM1": largura, altura e LZ4);
  - `BrushPreset` tem `name`, `category`, `source` (texto livre: arquivo de origem) e todos os parâmetros do motor. **Não** tem id estável, autor, licença, versão, datas, tags nem miniatura;
  - a miniatura é desenhada pelo motor a cada sessão;
  - `save()` grava `brushes.json.tmp` e renomeia, sem fsync;
  - `remove_user` apaga por **índice**;
  - o importador (`brush_import.rs`) lê `.gbr/.gih/.abr/.brush/.brushset/.bundle/imagens`, guarda só `source` e recusa `.sut`. O comentário diz que "a licença é a do autor do pacote", mas **nenhum campo de licença é guardado**, então o aviso não acompanha o pincel.

**Corpus real** (`producoes/micro-anime/ultima-rosa.aiai`): 234 MB em disco, **4.833 arquivos de blob** (4.065 com menos de 64 KB; o maior tem 6,4 MB), `project.json` de 11,5 MB (14.662 revisões), `wal.ndjson` de 0,5 MB. Serve de caso de teste e de referência de desempenho.

### a.2 O que falta (lacunas, da mais grave à menos)

| # | Lacuna | Por quê importa |
|---|---|---|
| G1 | Blobs do WAL gravados **sem fsync** (`write_blob(..., false)`: arquivo temporário + rename, sem `sync`) | Numa queda de energia o NTFS pode ter o rename já gravado e os dados não: sobra um blob vazio/truncado com nome de hash. Na abertura, `insert_clean` falha por hash diferente e, para blob **referenciado pelo `project.json`**, o `open` inteiro aborta (`blobs.insert_clean(h, bytes)?`). No replay do WAL o erro é ignorado (`let _ =`), então o tile some em silêncio. |
| G2 | `write_blob` considera "já gravado" qualquer arquivo que exista (`path.exists()`) | Um blob corrompido nunca é reparado: o salvamento seguinte "confia" nele. |
| G3 | **Sem trava** de "projeto aberto em outro lugar" | Duas instâncias (app + MCP/headless) abrem a mesma pasta e as duas anexam ao `wal.ndjson`: o diário intercala e o replay quebra. |
| G4 | **Sem backup nem versões**: Salvar sobrescreve `project.json`; não há `.bak`, nem "versão anterior", nem cópia ao abrir | Salvar uma edição ruim, ou um bug do app, perde o estado anterior sem volta. |
| G5 | Recuperação depende de **determinismo do replay** e para na primeira linha ruim | Uma linha corrompida no meio do WAL descarta tudo o que vem depois, sem avisar o que se perdeu além do `wal_truncated_at`. |
| G6 | `brushes.json` ilegível ⇒ o app segue com os de fábrica e **o próximo "salvar pincel" sobrescreve o arquivo** | Perda total dos pincéis do usuário por um erro de leitura passageiro (ex.: antivírus). Sem `.bak`, sem fsync. |
| G7 | Pasta com milhares de arquivos | Copiar, compactar, mandar por e-mail/nuvem e antivírus ficam lentos; o Explorer mostra uma pasta, não um arquivo com miniatura; `sync_all` por arquivo custa dezenas de ms cada. |
| G8 | Abrir lê **tudo** para a memória | 234 MB hoje; projeto de vários GB não abre. |
| G9 | Sem miniatura embutida, sem `application_id`/tipo, sem verificação de integridade global | O Explorer não mostra prévia; não há "verificar projeto"; o `format` do opener não é validado contra o `project.json`. |
| G10 | Versionamento mínimo: só `format: u32` e `FORMAT_VERSION = 1`; campos desconhecidos são descartados pelo serde ao regravar | Um app antigo que abre arquivo novo e salva destrói dados do novo. Falta `min_reader` e infraestrutura de migração. |
| G11 | Autosave do projeto com pasta não gera ponto de restauração (só o WAL contínuo) | O `project.json` pode ficar horas defasado; qualquer falha do WAL vira perda longa. |
| G12 | Pincéis sem autor/licença/id/versão | Importar pincel de terceiro e exportar de novo perde a atribuição; impossível detectar duplicata ou atualização. |

O que já está bom e deve ser **preservado**: endereçamento por conteúdo (BLAKE3) e verificação de hash na leitura, deduplicação, coleta de lixo com idade mínima, `retry_io`, checagem de espaço livre, comandos aplicáveis e reproduzíveis (base do diário), preset gravado inteiro dentro do traço.

---

## (b) Formato de projeto proposto

### b.1 Comparação das três opções de contêiner

| Critério | **Zip** (store/deflate) | **SQLite** (rusqlite bundled) | **Binário próprio em blocos** |
|---|---|---|---|
| Precedentes | Krita `.kra`, OpenRaster `.ora`, Procreate `.procreate`, `.bundle` | Clip Studio `.clip` (SQLite embutido num contêiner de blocos), Fossil, GeoPackage, MBTiles | PSD/PSB, TVPaint `.tvpp` |
| Queda no meio do salvamento | Sem transação: precisa gravar arquivo novo + rename (reescreve GBs a cada Salvar) ou arriscar corromper o diretório central | **Atômico por transação** (diário/WAL); a recuperação de queda é do próprio SQLite | Tem de ser inventado e testado (diário, checksums, fsync) |
| Salvar incremental | Não de verdade: adicionar entradas exige reescrever o diretório central; atualizar uma entrada exige reescrever o arquivo | Só as páginas alteradas; um blob novo é um `INSERT`; Salvar = 1 transação = 1 fsync (hoje: 1 fsync **por tile**) | Possível, mas é trabalho próprio |
| Acesso aleatório | Sim (diretório central), mas sem índice próprio | Sim, por chave (hash de 32 bytes) com B-tree; leitura sob demanda | Depende do desenho |
| Diário de comandos dentro do arquivo | Não cabe | Tabela `journal` na mesma transação dos tiles | Inventar |
| Ferramentas / depuração | 7-zip abre; é o mais "aberto" | `sqlite3` CLI, DB Browser, `.recover`, `PRAGMA integrity_check`; `file(1)` reconhece via `application_id` | Só o nosso programa |
| Custo de dependência | Já temos leitor de zip (`brush_import::read_zip`) e `flate2` | `rusqlite` com `bundled` (compila C; **verificar no PC com C: cheio**) | Zero, mas o maior custo de engenharia |
| Tamanho | Deflate de PNG não ganha nada | "Praticamente igual ao zip" (±1%) segundo o SQLite | Controlável |

Fatos verificados que sustentam a escolha (fontes ao final):

- O SQLite documenta como vantagens de formato de aplicativo: transações atômicas contra queda de energia, **atualização incremental** (só as páginas mudadas), pilha de undo/redo que pode persistir, formato legível a longo prazo (compatível desde 2004, recomendado pela Biblioteca do Congresso dos EUA), e arquivo único com `application_id` de 4 bytes no cabeçalho. Fonte: <https://sqlite.org/appfileformat.html>.
- Blobs pequenos lidos/gravados mais rápido que arquivos soltos: ~35% (Android) até ~5x no Windows 10, em blobs de ~10 KB; gravar com fsync em arquivos soltos fica ~10x ou mais mais lento que no SQLite; a vantagem encolhe conforme o blob cresce (a página compara com blobs de 8–12 KB, não dá número para 64 KB+). Uso de disco ~20% menor que a pasta (preenchimento de bloco do sistema de arquivos). Fonte: <https://www.sqlite.org/fasterthanfs.html>. Nossos tiles são quase todos < 64 KB (4.065 de 4.833 no corpus), então estamos na faixa favorável; **o ganho exato tem de ser medido no corpus** (spike da S24).
- Mecanismo de atomicidade e ressalvas honestas (fsync/`FlushFileBuffers` pode não valer em alguns discos e configurações do Windows; não renomear/apagar o diário "quente"; evitar sistemas de arquivos de rede). Fonte: <https://www.sqlite.org/atomiccommit.html>.
- Modo WAL: leitores não bloqueiam o escritor, escrita mais rápida e sequencial, menos fsync; mas cria `-wal`/`-shm` ao lado, **não funciona em sistema de arquivos de rede**, e a própria documentação diz que esses arquivos persistentes deixam o SQLite "menos adequado como formato de arquivo de aplicativo". Fonte: <https://www.sqlite.org/wal.html>.
- O Clip Studio usa SQLite: o tipo MIME registrado lista `CSFCHUNK` como número mágico e diz que não há especificação publicada; bibliotecas de terceiros leem só os metadados SQLite embutidos e descomprimem os pixels por bloco de 256x256 sob demanda. Fontes: <https://ftp.iana.org/assignments/media-types/image/vnd.clip>, <https://pypi.org/project/clipparse/>. **[não verificado]**: nomes exatos dos blocos (`CHNKHead`, `CHNKExta`, `CHNKSQLi`, `CHNKFoot`); só consegui confirmar que há blocos com tag de 4 letras, tipo e tamanho de 64 bits (commit do PhotoRec: <https://gitea.soloconlinux.org.es/luisgulo/testdisk/commit/35042ac9fcfa021a68b143f8a53c2e27a781ba0f>). Tem o mesmo desenho que propomos (metadados relacionais + tiles por bloco).
- Krita `.kra`: zip "vagamente baseado no padrão OpenDocument" com `mergedimage.png` (RGBA, a imagem como aparece na tela); `.krz` = `.kra` sem `mergedimage.png` e sempre comprimido. Arquivos típicos citados: `mimetype`, `maindoc.xml`, `documentinfo.xml`, `preview.png`, pasta de camadas (esses nomes vêm de relatos de usuários, **[não verificado]** em especificação oficial; a ordem "mimetype primeiro" é confirmada só para o `.ora`). Fontes: <https://docs.krita.org/en/general_concepts/file_formats/file_kra.html>, <https://forum.kde.org/viewtopic.php?f=139&t=129597>.
- OpenRaster `.ora`: zip com `mimetype` **primeiro e sem compressão**, `stack.xml`, `data/`, `Thumbnails/thumbnail.png` (obrigatório, PNG 8 bits não entrelaçado, até 256x256) e `mergedimage.png` (obrigatório desde 0.0.2). Fonte: <https://www.openraster.org/baseline/file-layout-spec.html>.
- Procreate `.procreate`: zip padrão com pasta de UUID contendo arquivos `.chunk` (pixels, formato inferido por engenharia reversa) e `QuickLook/Thumbnail.png` ("captura de baixa qualidade"); formato fechado. Fontes: <https://github.com/Avarel/silicate>, <https://github.com/devlinman/Procreate-thumbnailer>. O nome `Document.archive` vem de site de extensões de arquivo (baixa confiança, **[não verificado]**).
- PSD/PSB: assinatura `8BPS` + versão de 2 bytes (1 = PSD, 2 = PSB); PSD até 30.000 px por lado, PSB até 300.000 px; cinco seções (cabeçalho fixo, dados do modo de cor, recursos de imagem, camadas/máscaras, dados de imagem); só o cabeçalho tem tamanho fixo. Fonte: <https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/>. Útil só como lição: **campo de versão logo no início e campos de tamanho que dão para pular seções desconhecidas**.
- TVPaint `.tvpp`: o layout interno **não é documentado** nos resultados (**[não verificado]**; só o formato de painéis `.tvpx` e exportações são documentados): <https://doc.tvpaint.com/docs/custom-panels/tvpx-format>. Toon Boom Harmony: a **cena é uma pasta** (`elements/` com desenhos por camada, `palette-library/`, `audio/`, `frames/`, `stage/` com um arquivo de estágio por versão da cena, `timings/`), com desenhos e paletas como arquivos ligados fora do projeto, e a documentação avisa que para arquivar é preciso levar a pasta toda. Fonte: <https://docs.toonboom.com/help/harmony-25/premium/project-creation/about-file-structure.html>. Ou seja, o nosso formato atual (pasta) é o modelo da animação profissional de estúdio, mas o aplicativo de ilustração/animação "de mesa" (CSP, Krita, Procreate) entrega **arquivo único**; o usuário pediu esse.

**Decisão: SQLite como formato de trabalho**, zip como formato de **intercâmbio** (pincéis, materiais, "empacotar projeto para enviar"). Razões, na ordem do pedido:

1. **Robustez a queda**: a transação do SQLite faz o que hoje fazemos à mão (temporário + fsync + rename) para o arquivo todo de uma vez, sem janela em que blob e documento discordem, e sem o problema G1/G2.
2. **Incremental em GBs**: Salvar = INSERT dos blobs novos + atualização do documento numa transação; nada de reescrever o arquivo. Zip não faz isso.
3. **Acesso aleatório**: busca por hash de 32 bytes; abre sem ler tudo (resolve G8, com mudança de API do `BlobStore`, ver risco R1).
4. **Ferramentas**: `sqlite3`, `PRAGMA integrity_check`, `.recover`; qualquer pessoa inspeciona e recupera.
5. Uma só base para diário (WAL de comandos), versões, miniatura, trava, índice.

Por que não o binário próprio: reimplementaria diário, checksums, recuperação e ferramentas de reparo que o SQLite já tem, para um ganho marginal. Por que não zip como formato de trabalho: sem atomicidade nem incremental (Krita aceita isso e mitiga com autosave e cópias `~`; <https://docs.krita.org/en/user_manual/autosave.html>).

### b.2 Extensão e identificação

- **Extensão: `.aiaiproj`** (reaproveita a que já existe). Os `.aiaiproj` antigos são JSON de poucos bytes; o novo é SQLite. A abertura **fareja o conteúdo**: cabeçalho `SQLite format 3\0` + `application_id` → novo; JSON com `aiai_project` → legado (resolve para a pasta, exatamente como hoje). Vantagens: nenhum conflito de nome com a pasta `nome.aiai/` (o arquivo convertido fica ao lado dela, não dentro); a associação do Windows que já estava prevista para `.aiaiproj` vale para os dois mundos; quem clica em um opener antigo continua abrindo.
- `PRAGMA application_id = 0x41494149` ("AIAI"; é qualquer inteiro de 32 bits gravado no offset 68; o registro de IDs é só o `magic.txt` do SQLite, sem processo formal: <https://www.sqlite.org/src/artifact?name=magic.txt&ci=trunk>). `PRAGMA user_version` = versão do esquema SQL.

### b.3 Layout interno (esquema)

```
meta           (key TEXT PRIMARY KEY, value)             -- formato, app, datas, flags
  format_major, format_minor, min_reader  (inteiros; ver b.4)
  project_uuid, name, created_at, modified_at
  writer = "AnimeIAI 0.x.y", created_by
  saved_revision, clean_shutdown (0/1), session_id
doc            (id INTEGER PRIMARY KEY CHECK(id=1), revision, codec, data BLOB)
                 -- fase 1: o project.json canônico inteiro, comprimido (lz4_flex/flate2 já estão no workspace)
                 -- fase 2: tabela node(id, kind, rev, codec, data) com um nó por linha -> Salvar grava só os nós mudados
blob           (hash BLOB(32) PRIMARY KEY, kind, size, codec, data BLOB) WITHOUT ROWID
                 -- chave = BLAKE3 do conteúdo cru; blob > 256 MB (áudio longo) partido em blob_part
journal        (seq INTEGER PRIMARY KEY, revision, ts, op, author, description, command_json)
                 -- = o wal.ndjson de hoje; só entradas depois de saved_revision
snapshot       (id INTEGER PRIMARY KEY, created_at, kind('save'|'auto'|'open'), revision, label, codec, doc BLOB, refs BLOB)
                 -- pontos de restauração: documento comprimido + lista de hashes (os tiles são compartilhados)
thumb          (id, size, png BLOB)                       -- 256x256 e 1024 (quadro-chave do corte atual)
lock           (id=1, pid, host, user, started_at, heartbeat_at, exe)  -- "aberto em outro lugar"
```

- `PRAGMA page_size`: ~16 KB como ponto de partida (tile típico < 64 KB; **medir** 8/16/32 KB no corpus), `auto_vacuum=INCREMENTAL` e `incremental_vacuum` ao fechar bem, para o arquivo encolher depois da coleta de lixo.
- A **coleta de lixo** deixa de apagar arquivos: `DELETE FROM blob WHERE hash NOT IN (referenciados pelo doc, pelo diário e pelos snapshots retidos)`.
- Tudo o que hoje é um arquivo da pasta vira linha: `project.json` → `doc`; `blobs/` → `blob`; `wal.ndjson` → `journal`; `.aiaiproj` → o próprio arquivo.

### b.4 Manifesto e versões (compatibilidade)

- `format_major`: muda só quando uma versão antiga não consegue ler de jeito nenhum. `format_minor`: acréscimos que versão antiga pode ignorar. `min_reader`: menor versão do app que pode **ler e regravar sem perda**.
- Regras de abertura:
  - `format_major` maior que o do app → recusa com mensagem ("feito por AnimeIAI X, atualize");
  - `min_reader` maior que o app → abre **somente leitura** (nunca regrava; resolve G10);
  - `format_major` menor → migra dentro de uma transação, **depois** de gravar um backup `nome.aiaiproj.v<N>.bak` (ou snapshot `kind='open'`).
- O app grava `writer`/`app_version` a cada Salvar. Campos desconhecidos do documento: preservar num `serde_json::Value` extra, em vez de descartá-los.
- Lição dos formatos estudados: PSD põe versão logo depois da assinatura de 4 bytes e usa tamanhos de 8 bytes no PSB; `.ora` pede `mimetype` primeiro e muda o conjunto obrigatório por versão (`mergedimage.png` desde 0.0.2); o `.bundle` do Krita tem versão no `meta.xml`.

### b.5 Miniatura e prévia

- `thumb` guarda PNG 256x256 (limite do `.ora` e tamanho típico do Windows) e uma prévia maior (≈1024) do quadro escolhido (primeiro corte ou o último visto). Regenerada em todo Salvar/checkpoint, fora da thread de interface.
- **Alça de miniatura do Windows** (opcional, depois): `IThumbnailProvider` (CLSID `{E357FCCD-A995-4576-B01F-234630154E96}`) registrada em `.aiaiproj\ShellEx\`; roda em processo separado por padrão; o Windows guarda cache em 32/96/256/1024 px e só chama de novo se a data de modificação do arquivo for mais nova; não preencher nem desenhar sombra (o Windows faz). Fonte: <https://learn.microsoft.com/en-us/windows/win32/shell/thumbnail-providers>. Implementar em Rust (`windows` crate) numa DLL pequena que abre o arquivo **somente leitura e imutável** (`file:...?immutable=1`, evita mexer em arquivo aberto no app) e lê `thumb`. **Estimativa M–L; não é bloqueante.** Alternativa barata para a primeira versão: ícone do tipo de arquivo + a miniatura só dentro do app (tela inicial/recentes).

### b.6 Como o WAL, o autosave e o Salvar se mapeiam

| Hoje | No `.aiaiproj` |
|---|---|
| Comando aplicado → linha no `wal.ndjson` + `sync_data` | Comando aplicado → `INSERT INTO journal` + blobs novos (`INSERT OR IGNORE INTO blob`) **na mesma transação**; sem os blobs "soltos antes do diário" (acaba G1/G2) |
| Salvar = blobs + `project.json` + recria WAL | Salvar = uma transação: grava `doc` novo, `saved_revision = revision`, apaga `journal` ≤ revisão, cria `snapshot(kind='save')`, atualiza `thumb`; depois, fora da transação, `wal_checkpoint` |
| Autosave só para projeto sem pasta (120 s) | Projeto novo **já nasce arquivo** em `%LOCALAPPDATA%\AnimeIAI\recovery\<uuid>.aiaiproj` e é gravado continuamente pelo diário; "Salvar" pergunta onde e **move** (mesmo volume) ou copia+verifica+apaga. O caso especial `snapshot()`/`recovery` some. |
| Sem ponto de restauração no projeto com pasta | `snapshot(kind='auto')` a cada 5 min **se** houver alterações (CSP deixa 5–60 min; Krita usa 15) |

Modo de diário do SQLite: durante a edição, `journal_mode=WAL` + `synchronous=FULL` no commit do Salvar e `NORMAL` nos comandos do diário **(decisão a medir)**; ao fechar normalmente, `wal_checkpoint(TRUNCATE)` e `journal_mode=DELETE`, para o arquivo em repouso ser **um arquivo só** (a ressalva da documentação do WAL). Se o caminho é de rede (UNC) ou pasta de nuvem, cair para `journal_mode=DELETE` e avisar. Cópia do arquivo **enquanto aberto** não é suportada; o app oferece "Salvar cópia" (API de backup online do SQLite).

### b.7 Fluxo de recuperação de queda

1. Ao abrir: `lock` presente e o processo `pid` não existe (ou `heartbeat_at` velho) + `clean_shutdown = 0` ⇒ houve queda. Se o processo existe ⇒ "Já aberto em outro lugar (PID, usuário, desde hh:mm)": **Abrir só leitura** / **Cancelar** (resolve G3). O SQLite já faz sozinho a recuperação do seu diário/WAL ao abrir.
2. `PRAGMA quick_check` em segundo plano; falha ⇒ oferecer o snapshot mais recente, depois o backup externo, depois "salvar o que der" (`sqlite3 .recover` equivalente ou leitura tolerante bloco a bloco).
3. `journal` com `revision > saved_revision` ⇒ reaplicar como hoje, mas **em transação que reporta o que falhou** (não para na primeira entrada ruim: mostra "N ações recuperadas, M não aplicadas") (melhora G5).
4. Diálogo único: *"O AnimeIAI foi fechado de forma inesperada. Recuperado até hh:mm (N ações)."* Botões: **Continuar de onde parou** (documento "não salvo", undo mantido) / **Voltar ao último salvo** / **Abrir como cópia** / **Ver versões**.
5. Projeto sem destino: a tela inicial lista os `recovery\*.aiaiproj` (como hoje), com miniatura e data.

Referências de UX: Krita recupera arquivo nomeado perguntando "abrir o autosave?" ao reabrir o original e oferece restaurar/apagar o sem nome na inicialização; fecha-bem ou salvar apaga o autosave (<https://docs.krita.org/en/user_manual/autosave.html>). Clip Studio: "Habilitar recuperação de tela" a cada 5–60 min (<https://support.clip-studio.com/en-us/faq/articles/20200118> e resumo em <https://tips.clip-studio.com/ja-jp/articles/4264?org=1>).

### b.8 Backups e versões

- **Em arquivo** (`snapshot`): últimos 10 saves manuais + 1 por dia nos últimos 7 dias + os `auto` das últimas 2 h. Custam pouco (documento comprimido + lista de hashes; os tiles são compartilhados). Janela "Versões" para pré-visualizar e restaurar (ou abrir como cópia). O GC de blobs respeita os snapshots retidos.
- **Cópia externa inteira**, no estilo `InitialBackup`/`DocumentBackup` do Clip Studio (cópia ao abrir e ao sobrescrever; fica em pasta própria, "Maintenance > abrir pasta de backup"; o número de gerações **não** é dito na página: <https://support.clip-studio.com/en-us/faq/articles/20190029>) e `~` do Krita (cópia do arquivo anterior ao lado do original, sobrescrita a cada salvamento; backup incremental numerado e "versão incremental" são opcionais: <https://docs.krita.org/en/user_manual/autosave.html>): cópia `%LOCALAPPDATA%\AnimeIAI\backups\<project_uuid>\<data>.aiaiproj` **ao abrir** (1 por sessão e só se mudou desde a última) e **diária**, mantendo as 5 últimas; para arquivos muito grandes (> X GB, configurável) pular e usar só snapshots em arquivo. Feita com a API de backup online do SQLite, sem fechar o projeto.
- **Trava e salvamento "seguro" fora do SQLite** (Salvar como, converter, exportar): arquivo temporário no **mesmo volume**, `sync_all`, reabrir e verificar, depois `rename` por cima. No Windows, `std::fs::rename` usa `MoveFileExW`, com semântica POSIX a partir do Windows 10 1607 se o sistema de arquivos permitir; a documentação **não** promete atomicidade perante queda (<https://doc.rust-lang.org/std/fs/fn.rename.html>, discussão em <https://users.rust-lang.org/t/fs-rename-overwriting-existing-file/17314>). `ReplaceFileW` preserva ACL/data de criação e aceita arquivo de backup, mas exige o mesmo volume e `REPLACEFILE_WRITE_THROUGH` "não é suportado" (<https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew>). Manter o `retry_io` atual.

### b.9 Trava de "aberto em outro lugar"

Dupla: (1) o próprio SQLite com `locking_mode=EXCLUSIVE` não serve bem com o WAL e leitor de miniatura, então (2) a linha `lock` (pid/host/usuário/início + `heartbeat_at` a cada 15 s) é o mecanismo de aviso, e o fcntl/`LockFileEx` do SQLite protege a escrita de verdade. Para a pasta legada: arquivo `nome.aiai\.lock` com o mesmo conteúdo, criado com `CreateNew`. Pastas de nuvem/rede: avisar que a trava é só de cortesia.

### b.10 Migração das pastas `.aiai` (continuam abrindo)

1. Camada de armazenamento atrás de um trait (`ProjectStore`: `open`, `save`, `append_journal`, `get_blob`, `put_blob`, `snapshot`, `lock`) com dois backends: `FolderStore` (código atual de `persist.rs`, intocado) e `SqliteStore`. `persist::open/save` do bridge/MCP/headless chamam o trait.
2. Abrir: escolha pelo conteúdo (pasta com `project.json`; `.aiaiproj` JSON ⇒ pasta; `.aiaiproj` SQLite ⇒ novo). O WAL pendente da pasta é reaplicado como hoje.
3. **Conversão**: ao Salvar um projeto de pasta, perguntar "Converter para arquivo único `.aiaiproj`?" (padrão sim). Cria `nome.aiaiproj` **ao lado** da pasta, em arquivo temporário; verifica relendo (JSON canônico idêntico, todos os hashes de blob, hash do render do primeiro quadro) e só então renomeia. **A pasta antiga nunca é apagada pelo programa**; no máximo "mover para a Lixeira" por pedido explícito. Comando equivalente no headless/MCP (`aiai convert`) e teste automático com o corpus `ultima-rosa.aiai`.
4. Caminho inverso: "Exportar como pasta (depuração/git)" gera o formato legado, para quem quer diff legível (a pasta atual está no git do repositório: o arquivo SQLite binário perde isso, então o export de pasta e o `project.json` canônico continuam existindo como ferramenta).
5. Manter o `FolderStore` com gravação por pelo menos duas sprints após o `SqliteStore` virar padrão.

### b.11 Associação de arquivo no Windows (opcional/depois)

Por usuário, sem administrador: `HKCU\Software\Classes\.aiaiproj` (valor padrão = ProgID `AnimeIAI.Project`), `...\AnimeIAI.Project\DefaultIcon`, `...\shell\open\command` = `"...\animeiai.exe" "%1"`, e o mesmo para `.aiaibrush`/`.aiaibrushes`; avisar o shell com `SHChangeNotify(SHCNE_ASSOCCHANGED)`. O app precisa de **instância única**: o segundo processo passa o caminho ao primeiro (mutex + pipe nomeado) e sai. A alça de miniatura (b.5) se registra em `ShellEx`. O comportamento `ThumbnailCutoff`/`TypeOverlay`/`Treatment` está em <https://learn.microsoft.com/en-us/windows/win32/shell/thumbnail-providers>. **[não verificado]**: páginas de associação (`fa-intro`) não foram lidas; confirmar no momento de implementar.

---

## (c) Formato de pincel proposto

Não repete o SQLite: pincel é pequeno, compartilhado entre pessoas e precisa ser inspecionável. **Zip** (como `.bundle` do Krita, `.brushset` do Procreate, `.ora`).

### c.1 Referências verificadas (autor, licença, miniatura, versão)

| Formato | Contêiner | Autor/licença/fonte | Miniatura | Versão |
|---|---|---|---|---|
| Krita `.bundle` | zip, `mimetype` (`application/x-krita-resourcebundle`), `META-INF/manifest.xml` (por tipo de recurso: id, **MD5**, caminho original, nome no pacote), `meta.xml`, pastas por tipo (`paintoppresets/`, `brushes/`, `patterns/`...) | `meta.xml` com `generator`, autor, título, descrição, criador inicial, data de criação, e `meta:meta-userdefined` para **e-mail, licença, website** e `tag`s; o assistente do Krita 5.3 pede título, ícones e dados do autor | `preview.png` 256x256 (sem imagem = quadrado vermelho) | `meta:version` (padrão "1") |
| Procreate `.brush`/`.brushset` | zip com `Shape.png`, `Grain.png`, `Brush.archive` (plist binário) | **sem campo de licença verificado** | `QuickLook/Thumbnail.png` no `.procreate` (projeto) | **[não verificado]** |
| Photoshop `.abr` | binário: versão de 2 bytes + subversão; seções `8BIM`+chave de 4 letras+tamanho (`samp` = pontas, `patt` = texturas, `desc` = descritores, `phry` = grupos); v1/v2 antigas e v6–v10 modernas | sem campo de licença; "o ABR moderno não é especificado em sua totalidade" | embutida só como miniatura no descritor **[não verificado]** | versões 1, 2, 6, 7, 9, 10 |
| MyPaint `.myb` | JSON + `_prev.png` ao lado | **[não verificado]** (a wiki não pôde ser lida) | `<nome>_prev.png` | **[não verificado]** |
| Clip Studio `.sut`/`.sutg` | "arquivo de configurações de ferramenta"; exportar por ferramenta (`.sut`) ou por grupo (`.sutg`); importa `.abr` | a ajuda não menciona autor/licença; o formato interno é SQLite próprio **[não verificado]** (já recusado em `brush_import.rs`) | **[não verificado]** | **[não verificado]** |
| GIMP `.gbr/.gih/.vbr` | binários simples | **[não verificado]** | — | — |

Fontes: <https://lxr.kde.org/source/graphics/krita/libs/resources/KoResourceBundle.cpp>, <https://docs.krita.org/reference_manual/resource_management.html>, <https://bugs.kde.org/show_bug.cgi?id=350115> (manifesto listando recurso que não está no pacote: o Krita passou a validar), <https://github.com/Avarel/silicate>, <https://pub.dev/packages/abrkit>, <https://docs.krita.org/reference_manual/resource_management/paintoppresets.html> (menção ao `.myb` + `_prev`), <https://help.clip-studio.com/en-us/manual_en/150_tools/Importing_and_exporting_tools.htm>, <https://manpages.ubuntu.com/manpages/questing/man1/gimp.1.html>.

Lição: ninguém armazena licença de forma padronizada; o Krita guarda como "campo livre do usuário". Fazemos melhor: **identificador SPDX** (lista oficial com id curto, nome, texto e URL permanente, ex. `CC-BY-4.0`, `CC0-1.0`, `MIT`: <https://spdx.org/licenses/>) **ou texto livre** quando não existe id (`LicenseRef-...` é a convenção do SPDX para licença própria; **[não verificado]** nesta pesquisa: a página da lista não descreve o `LicenseRef`; conferir na especificação SPDX).

### c.2 Arquivos

- **`.aiaibrush`**: um pincel. **`.aiaibrushes`**: pacote (coleção/biblioteca), o mesmo contêiner.
- zip, `mimetype` primeiro e sem compressão (`application/x-animeiai-brush` / `-brushes`); demais entradas com deflate (PNG já comprimido: *store*).

```
mimetype
manifest.json            <- obrigatório (campos abaixo)
brush.json               <- BrushPreset (serde do motor) + "preset_schema"
assets/tip.png           <- ponta: PNG 8 bits cinza (ou RGBA→máscara), <= 512 px de lado
assets/texture.png       <- opcional (grão/papel), cinza, repete no canvas
thumbnail.png            <- 256x256 (traço de exemplo desenhado pelo próprio motor)
LICENSE.txt              <- texto integral, obrigatório se a licença é texto livre
README.md                <- opcional
```

`.aiaibrushes`:

```
mimetype / manifest.json (pacote) / thumbnail.png / LICENSE.txt
brushes/<id>/brush.json + thumbnail.png      <- um por pincel, cada um pode ter licença própria
assets/<blake3>.png                          <- pontas e texturas compartilhadas, endereçadas por conteúdo
```

### c.3 `manifest.json`

```json
{
  "format": "animeiai-brush",
  "format_version": 1,
  "min_reader": 1,
  "id": "0191f3a2-...",                // UUID v7 estável entre versões do mesmo pincel
  "name": "Nuvem cirro",
  "description": "...",
  "author":  { "name": "...", "email": "...", "url": "https://..." },
  "license": { "spdx": "CC-BY-4.0", "text_file": null, "url": "https://creativecommons.org/...", "attribution": "Nome do autor" },
  "source":  { "url": "https://...", "imported_from": "photoshop-abr", "original_file": "nuvens.abr", "original_author": "..." },
  "version": "1.2.0",
  "created": "2026-10-19T12:00:00Z",
  "modified": "2026-10-19T13:30:00Z",
  "app_version": "AnimeIAI 0.24.0",
  "preset_schema": 1,
  "category": "cenario",
  "tags": ["nuvem", "céu"],
  "assets": [ { "path": "assets/tip.png", "role": "tip", "blake3": "...", "width": 256, "height": 256 } ],
  "thumbnail": "thumbnail.png",
  "extra": { }                           // preservado ao regravar
}
```

Regras: `license` é **obrigatório** (id SPDX *ou* `text_file`); pincel criado pelo usuário recebe `LicenseRef-Proprietary` ("todos os direitos reservados") por padrão ou o que ele escolher nas preferências; pincel vindo de formato de terceiro recebe `license: {"spdx": null, "note": "desconhecida: a do autor do pacote original"}` e `source.imported_from` preenchido (substitui o campo `source` de texto livre atual, mantido como legado).

### c.4 Validação (na importação)

- `mimetype` primeiro e correto; entradas ≤ 1.000; cada entrada ≤ 64 MB e total descomprimido ≤ 256 MB (reaproveita `MAX_ENTRY`/`MAX_PIXELS` de `brush_import.rs`); recusar caminho com `..`, absoluto, `\` ou nome duplicado (zip slip e zip bomba).
- Todo `assets[].blake3` confere com o conteúdo; todo arquivo citado existe e todo arquivo presente é citado (o Krita teve bug de manifesto listando recurso ausente: <https://bugs.kde.org/show_bug.cgi?id=350115>).
- `brush.json` passa por `BrushPreset::validate()` e por limites (tamanho, espaçamento 0,02–2, ponta ≤ 512 px).
- `format_version`/`min_reader`: maior que o app → recusa com motivo; campos desconhecidos em `extra`/`brush.json` preservados.
- Mostrar **antes** de instalar: nome, autor, licença (SPDX com link), origem. Licença `LicenseRef`/desconhecida ou "importado de formato de terceiro" exige confirmação ("a licença é a do autor do pacote original").

### c.5 Importação e exportação

- **Importar**: `.aiaibrush(es)` (novo, sem perda), mais os formatos de terceiros já lidos (`.abr`, `.brush(set)`, `.bundle`, `.gbr/.gih`, imagens): cada pincel externo vira um `.aiaibrush` na biblioteca, com `source` preenchido, e o **arquivo original é guardado** em `Pincéis\_Importados\` (prova de origem/licença). Duplicata: mesmo `id` + `version` ⇒ ignorar; mesmo `id` com versão maior ⇒ perguntar **Substituir / Manter os dois**; mesmo hash de conteúdo com `id` diferente ⇒ avisar. `.sut` continua recusado, com a razão.
- **Exportar**: um pincel, uma categoria ou a biblioteca inteira. Se o pincel tem origem de terceiro com licença desconhecida, o diálogo avisa e exige o aceite de que o usuário tem o direito de redistribuir; a licença e a atribuição **sempre** viajam no manifesto.
- **Projeto**: continua gravando o preset **inteiro** no traço; pincel e projeto não dependem um do outro (já é o princípio hoje). O projeto também guarda `brush_ref {id, version}` só para o "Atualizar do pincel da biblioteca" opcional.
- **Migração do que existe**: na primeira abertura do app novo, cada entrada de `brushes.json` vira um `.aiaibrush` em `Pincéis\<categoria>\` com `id` novo, `author` do perfil do usuário e licença padrão; o `brushes.json` antigo e `brush_assets` são **renomeados para `.migrado`**, não apagados. Antes disso, corrigir G6 (item F3 do backlog): nunca sobrescrever arquivo ilegível.

---

## (d) Pasta de recursos do usuário

### d.1 Layout proposto: `Documentos\AnimeIAI\`

```
Documentos\AnimeIAI\
  .biblioteca.json        versão do layout, uuid da biblioteca, preferências de ordenação
  Projetos\               destino padrão dos novos .aiaiproj
  Pincéis\<Categoria>\    *.aiaibrush  (e pacotes .aiaibrushes instalados como subpasta)
      _Importados\        originais .abr/.brushset/.bundle/.gbr (prova de origem)
  Materiais\<Tipo>\       *.aiaimat — Fundos, Balões, Onomatopeias, Efeitos, Ícones, Tramas
  Modelos 3D\             *.aiai3d  — .glb + manifesto (licença/atribuição) + miniatura
  Texturas\               *.aiaitex — papel, grão, tramas (PNG cinza/RGBA + manifesto)
  Paletas\                *.aiaipal — JSON (importa .gpl/.ase/.aco)
  Fontes\                 .ttf/.otf + .aiaifont.json (licença da fonte); carregadas só no app
  Layouts\                *.aiailayout — arranjo de painéis, atalhos, espaço de trabalho
  Modelos\                *.aiaitemplate — modelos de projeto (formato, taxa, camadas iniciais)
```

- Os nomes seguem o idioma do app, mas a **estrutura é identificada por `kind` no manifesto**, não por nome de pasta (renomear pasta não quebra nada).
- `Documentos` do Windows é localizado e pode ser redirecionado (OneDrive): usar `SHGetKnownFolderPath(FOLDERID_Documents)`, nunca montar o caminho à mão, e deixar o local configurável nas preferências. **Risco**: a nuvem sincronizando milhares de arquivos pequenos; a biblioteca de arquivos zip pequenos sofre menos que a pasta de blobs atual.
- Recursos de **fábrica** ficam na pasta de instalação (somente leitura) e aparecem na mesma lista com o selo "de fábrica"; o usuário duplica para editar. Cópias do usuário ganham `id` próprio.

### d.2 Indexação

- A **pasta é a verdade**; o índice é **cache** em `%LOCALAPPDATA%\AnimeIAI\library-index.sqlite` (nome, tags, categoria, tipo, autor, licença, hash do arquivo, mtime/tamanho, miniatura 128 px), **reconstruível a qualquer hora** sem perder nada.
- Varredura na abertura (compara `mtime`+tamanho; só relê o que mudou) e vigia de pasta (`notify`) em segundo plano; entrada com manifesto inválido vai para a lista "com problema" (nome do arquivo + motivo), nunca some em silêncio.
- Tags e categoria vivem **dentro do manifesto de cada arquivo** (viajam com o arquivo, ao contrário do Krita, onde tags e relações ficam num cache SQLite e são perdidas se ele for apagado).
- Busca por nome/tag/autor/licença; coleções ("Favoritos", "Recentes") só no índice/preferências.

### d.3 Comparação

| Programa | Onde fica | Como organiza | Observação |
|---|---|---|---|
| **Krita** | `%APPDATA%\krita\` (Windows; Linux `~/.local/share/krita/`; macOS `~/Library/Application Support/Krita/`; configurável) | Pastas por tipo (`paintoppresets/`, `brushes/`, `patterns/`, `bundles/`...), **cache SQLite** com tags, nomes e versões; pacotes `.bundle` ativáveis/desativáveis; recursos versionados (a pasta guarda versões antigas; para voltar atrás, mover todas as versões e apagar da pasta) | Tags só no cache (apagar o cache apaga as tags). <https://docs.krita.org/reference_manual/resource_management.html>, <https://krita-artists.org/t/some-brushes-dont-show-up/34559> |
| **Clip Studio Paint** | Backups em `Documentos\CELSYS\CLIPStudioPaintData` (≤ 1.10.12) ou pelo menu Manutenção (3.0+); ferramentas pela **paleta Material** (pasta Download) e `.sut`/`.sutg` | Conteúdo por "Materiais" e nuvem (Clip Studio Assets) | Nome exato da pasta de Material **[não verificado]**. <https://support.clip-studio.com/en-us/faq/articles/20190029>, <https://help.clip-studio.com/en-us/manual_en/150_tools/Importing_and_exporting_tools.htm> |
| **Photoshop** | Presets (pincéis `.abr`, padrões, gradientes, formas) numa pasta de Presets do app/usuário | Painéis agrupam por arquivo; `.abr` por grupo | Caminho exato e política de licença **[não verificado]**; o `.abr` moderno não tem especificação completa (<https://pub.dev/packages/abrkit>) |
| **GIMP** | Pasta pessoal do perfil + pasta do sistema; subpastas `brushes`, `patterns`, `gradients`, `palettes`, `dynamics`, `tool-presets` com caminhos de busca em `gimprc` | Arquivos simples por tipo (`.gbr/.gih/.vbr/.gpl/.pat`) | Caminho Windows e etiquetas (**tags**) **[não verificados]**. <https://www.mankier.com/5/gimprc-3.0>, <https://manpages.ubuntu.com/manpages/questing/man1/gimp.1.html> |
| **Toon Boom Harmony** | A **cena** é a pasta; `palette-library/` dentro dela | Paletas e desenhos como arquivos ligados; levar a pasta toda para arquivar | <https://docs.toonboom.com/help/harmony-25/premium/project-creation/about-file-structure.html> |
| **AnimeIAI (proposta)** | `Documentos\AnimeIAI\` visível | Pasta por tipo, **um arquivo por recurso com manifesto**, índice-cache reconstruível, tags no arquivo | Visível para o usuário copiar/fazer backup; licença/autor em todo recurso |

---

## (e) Mesma abordagem para os outros recursos

**Envelope comum** (`manifest.json`, o mesmo do pincel com `kind`, e um `payload` por tipo), implementado numa biblioteca única (`aiai-assets`) que o pincel passa a usar:

```json
{ "format": "animeiai-asset", "format_version": 1, "min_reader": 1, "kind": "brush|material|model3d|texture|palette|font|layout|template",
  "id": "...", "name": "...", "description": "...", "author": {...}, "license": {...}, "source": {...},
  "version": "1.0.0", "created": "...", "modified": "...", "app_version": "...",
  "category": "...", "tags": [], "thumbnail": "thumbnail.png",
  "files": [ { "path": "...", "role": "...", "blake3": "...", "bytes": 0 } ],
  "payload": { } }
```

| Tipo | Extensão | Conteúdo | `payload` | Observações |
|---|---|---|---|---|
| Material (fundo, balão, onomatopeia, efeito) | `.aiaimat` | zip: PNG/SVG + `manifest.json` | tamanho nativo, ponto de ancoragem, `repeat`/ladrilho, camadas | Mesma validação; limite de pixels |
| Modelo 3D (guia de pose/cena) | `.aiai3d` | zip: `.glb` + texturas + licenças | unidades, escala, esqueleto/poses, atribuição | **Licença crítica** (CC-BY exige atribuição; `attribution` obrigatório) |
| Textura | `.aiaitex` | zip: 1 PNG + manifesto | repetição, escala em px, canal | Compartilha o cache de máscara "AIM1" |
| Paleta | `.aiaipal` | **JSON puro** (sem zip: poucos bytes), mesmo envelope | lista de cores `[{name, srgb, ...}]` | Importa `.gpl`, `.ase`, `.aco` (formatos simples) |
| Fonte | `.ttf/.otf` + `.aiaifont.json` | arquivo + manifesto | família, estilo, licença da fonte (OFL etc.) | Carregar privada ao processo; **respeitar as flags de embutir** se algum dia fontes forem embutidas no projeto |
| Layout/área de trabalho | `.aiailayout` | JSON puro | painéis, atalhos | Versão e `min_reader` iguais |
| Modelo de projeto | `.aiaitemplate` | `.aiaiproj` vazio + manifesto | formato, taxa, camadas iniciais | Abre como projeto novo |

Os mesmos três contratos valem para todos: **manifesto com versão e `min_reader`**, **hash por arquivo (BLAKE3)** e **licença obrigatória**. Quando um recurso é usado num projeto, o projeto grava uma **cópia** (como já faz com o preset do traço) mais `{asset_id, version, license}` para inventário de licenças do corte ("relatório de créditos" exportável: útil para publicar).

---

## (f) Backlog priorizado

Tamanhos: **S** ≤ 1 dia, **M** 2–4 dias, **L** > 4 dias / várias etapas.

### P0 — consertar o formato atual (barato, protege dados já, independe do SQLite)

| Item | Tam. | Notas |
|---|---|---|
| F1 `write_blob`: se o arquivo existe, conferir tamanho (e hash por amostragem/no GC); gravar com `durable=true` quando o blob é referenciado pelo diário depois da queda **ou** `sync` por lote no WAL | S | Fecha G1/G2 |
| F2 `open` tolerante: blob referenciado corrompido ⇒ relatório + o tile fica ausente/placeholder, em vez de abortar tudo | S | Reaproveita `OpenReport` |
| F3 `brushes.json`: nunca sobrescrever arquivo ilegível (renomear para `.corrupto-<data>`), gravar com `sync_all`, manter `.bak`, envelope `{ "format_version":1, "brushes":[...] }`, apagar por **id**/nome em vez de índice | S | Fecha G6 |
| F4 Trava de pasta (`.lock` com pid/host) + aviso "aberto em outro lugar" + abrir somente leitura | S–M | Fecha G3 na pasta legada |
| F5 `project.json.bak` (e rotação de 5) antes de sobrescrever; cópia ao abrir | S | Parte de G4 |
| F6 Miniatura `thumbnail.png` na pasta e no `.aiaiproj` JSON; mostrar na tela inicial | S | |

### P1 — formato novo

| Item | Tam. | Notas |
|---|---|---|
| P1 **Spike com o corpus `ultima-rosa.aiai`**: criar `.aiaiproj` SQLite, medir tamanho vs pasta, tempo de Salvar completo e de "1 pincelada", abertura com e sem carga total, `page_size` 8/16/32 KB, modo WAL vs DELETE, `sync` por comando; decidir 3 pontos em aberto de b.3/b.6 | M | **Fazer primeiro; condiciona o resto.** Inclui compilar `rusqlite bundled` neste PC |
| P2 Trait `ProjectStore` + `FolderStore` (código atual) + `SqliteStore` (meta, doc comprimido, blob, journal, lock) | L | Toca `persist.rs`, `session.rs`, `doc.rs`, bridge/MCP |
| P3 Abertura por conteúdo (farejar SQLite/JSON/pasta) e `format_major/minor/min_reader` + modo somente leitura | M | |
| P4 Conversão pasta → `.aiaiproj` com verificação, comando headless/MCP e UI "Converter" | M | Não apaga a pasta |
| P5 Projeto novo nasce em `recovery\*.aiaiproj`; Salvar/Salvar como move; remove `snapshot()` e `autosave.rs` especial | M | |
| P6 Recuperação de queda: diálogo de 4 opções, replay tolerante que reporta falhas, `quick_check`, tela de recuperação com miniatura | M | |
| P7 Snapshots em arquivo + janela "Versões" (restaurar/abrir como cópia) + backup externo ao abrir/diário | M–L | GC respeitando snapshots |
| P8 `BlobStore` com leitura sob demanda (LRU) em vez de carga total | L | Mudança de API em tudo que chama `blobs.get`; ganho de memória em GBs |
| P9 Documento por nó (`node`) para Salvar incremental do documento | M–L | Só se o `doc` de ~11 MB comprimido pesar no corpus |
| P10 Instância única + associação de arquivo por usuário (`HKCU`) | M | Instalador |
| P11 Alça de miniatura do Windows (`IThumbnailProvider` em DLL Rust) | L | Opcional; ver b.5 |

### P2 — pincéis e biblioteca

| Item | Tam. | Notas |
|---|---|---|
| B1 Crate `aiai-assets`: envelope, validação (zip slip/bomba, hashes, versões), leitura/escrita zip | M | Base de todos os recursos |
| B2 `.aiaibrush` / `.aiaibrushes`: exportar e importar, diálogo de licença/autor, deduplicação | M | |
| B3 Biblioteca em `Documentos\AnimeIAI\Pincéis\` + migração do `brushes.json` + índice-cache e varredura | M–L | Depende de B1; fazer F3 antes |
| B4 Campos de autor/licença/fonte no editor de pincel e na importação de formatos de terceiros (`source` estruturado, originais em `_Importados\`) | S–M | |
| B5 Materiais, Texturas, Paletas, Fontes, Layouts, Modelos 3D sobre o mesmo envelope | L (em partes S–M cada) | Paletas e Texturas primeiro (mais simples) |
| B6 Relatório de créditos/licenças do projeto (inventário dos recursos usados) | M | Útil para publicar |
| B7 Importar `.sut`/`.sutg` do Clip Studio | M–L, incerto | Formato SQLite próprio, **sem especificação verificada**; só vale com amostras reais |

### Riscos

| # | Risco | Mitigação |
|---|---|---|
| R1 | **Carga sob demanda (P8) é mudança estrutural**: `BlobStore` é um `HashMap` em memória usado por todo o motor | Fazer P1–P7 primeiro mantendo carga total (já funciona), medir no spike; só então P8 |
| R2 | `rusqlite bundled` compila C: build lento e o **C: vive cheio** | Verificar no spike; fixar a versão; usar `CARGO_TARGET_DIR` fora do C: se preciso |
| R3 | Arquivo único binário: corrupção atinge o projeto todo (a pasta isolava um blob) | Snapshots + backup externo + `quick_check` + `.recover`; blobs verificáveis pelo BLAKE3; export de pasta como saída de emergência |
| R4 | Perde-se o diff legível no git (`project.json`) | Manter export de pasta/JSON canônico e o `FolderStore` por um tempo; arquivos de teste continuam em pasta |
| R5 | Modo WAL: `-wal`/`-shm` ao lado durante a edição; cópia do arquivo aberto perde commits; rede e nuvem | `TRUNCATE` + `journal_mode=DELETE` ao fechar; trava; `DELETE` em caminho de rede/nuvem; "Salvar cópia" pela API de backup |
| R6 | fsync pode não valer em alguns discos/configurações do Windows | Documentado pelo SQLite; não prometer o impossível; manter snapshots em arquivo e backup externo |
| R7 | Tamanho do arquivo não diminui sozinho; fragmentação | `auto_vacuum=INCREMENTAL` + `incremental_vacuum` ao fechar; `VACUUM INTO` como "Otimizar" |
| R8 | Migração de pincéis/projetos perde dado do usuário | Verificação por releitura, originais **renomeados, não apagados**, testes com corpus e com `brushes.json` real |
| R9 | Licença: o app facilita redistribuir pincel de terceiro | Licença obrigatória, aviso ao importar/exportar, original guardado; a decisão jurídica é do usuário (o app não valida licença) |
| R10 | `LicenseRef`/lista SPDX e especificação de vários formatos de terceiros **não** foram verificados | Tratar como campo de texto livre até conferir; só implementar leitores de `.sut` com amostras reais |

---

## Fontes (URLs)

- SQLite como formato de aplicativo: <https://sqlite.org/appfileformat.html>
- SQLite mais rápido que o sistema de arquivos: <https://www.sqlite.org/fasterthanfs.html>
- Commit atômico do SQLite: <https://www.sqlite.org/atomiccommit.html>
- Modo WAL do SQLite: <https://www.sqlite.org/wal.html>
- `application_id` (magic.txt): <https://www.sqlite.org/src/artifact?name=magic.txt&ci=trunk>
- Krita `.kra`: <https://docs.krita.org/en/general_concepts/file_formats/file_kra.html>, <https://forum.kde.org/viewtopic.php?f=139&t=129597>
- Krita autosave e backups: <https://docs.krita.org/en/user_manual/autosave.html>
- Krita recursos/bundles: <https://docs.krita.org/reference_manual/resource_management.html>, <https://lxr.kde.org/source/graphics/krita/libs/resources/KoResourceBundle.cpp>, <https://bugs.kde.org/show_bug.cgi?id=350115>, <https://krita-artists.org/t/some-brushes-dont-show-up/34559>
- OpenRaster: <https://www.openraster.org/baseline/file-layout-spec.html>
- Procreate (engenharia reversa): <https://github.com/Avarel/silicate>, <https://github.com/devlinman/Procreate-thumbnailer>
- Clip Studio `.clip`: <https://ftp.iana.org/assignments/media-types/image/vnd.clip>, <https://pypi.org/project/clipparse/>, <https://github.com/saitolume/clipstudio.js>, <https://github.com/dobrokot/clip_to_psd>, <https://gitea.soloconlinux.org.es/luisgulo/testdisk/commit/35042ac9fcfa021a68b143f8a53c2e27a781ba0f>
- Clip Studio backup/recuperação/ferramentas: <https://support.clip-studio.com/en-us/faq/articles/20190029>, <https://support.clip-studio.com/en-us/faq/articles/20200118>, <https://tips.clip-studio.com/ja-jp/articles/4264?org=1>, <https://help.clip-studio.com/en-us/manual_en/150_tools/Importing_and_exporting_tools.htm>
- PSD/PSB: <https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/>
- ABR: <https://pub.dev/packages/abrkit>, <https://community.adobe.com/t5/photoshop-ecosystem-discussions/where-to-find-abr-brushes-file-format-specification/td-p/3537426>
- TVPaint: <https://doc.tvpaint.com/docs/custom-panels/tvpx-format>; Toon Boom: <https://docs.toonboom.com/help/harmony-25/premium/project-creation/about-file-structure.html>
- MyPaint/Krita (`.myb` + `_prev.png`): <https://docs.krita.org/reference_manual/resource_management/paintoppresets.html>
- GIMP: <https://www.mankier.com/5/gimprc-3.0>, <https://manpages.ubuntu.com/manpages/questing/man1/gimp.1.html>, <https://docs.gimp.org/3.0/sl/gimp-prefs-folders-data.html>
- Alça de miniatura do Windows: <https://learn.microsoft.com/en-us/windows/win32/shell/thumbnail-providers>
- `ReplaceFileW`: <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew>; `std::fs::rename`: <https://doc.rust-lang.org/std/fs/fn.rename.html>, <https://users.rust-lang.org/t/fs-rename-overwriting-existing-file/17314>
- SPDX: <https://spdx.org/licenses/>

### Itens que **não** consegui confirmar (não usados como base)

Nomes exatos dos blocos do `.clip` (`CHNKHead/Exta/SQLi/Foot`); estrutura SQLite interna do `.sut`; `Document.archive` do Procreate e campos de autor/licença nos formatos Procreate/ABR/`.myb`; esquema JSON do `.myb` (a wiki do MyPaint não pôde ser lida); caminhos de Presets do Photoshop e da pasta de Material do Clip Studio; caminho do GIMP no Windows e suas tags; ordem "mimetype primeiro" no `.kra` (confirmada só para `.ora`); convenção `LicenseRef-*` do SPDX; documentação de associação de arquivo do Windows (`fa-intro`); desempenho real de SQLite com os nossos tiles (precisa do spike P1).
