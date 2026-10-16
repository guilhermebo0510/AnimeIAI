# S02 — demo headless (aiai-mcp call, sem o app aberto)

Gerado por `scratchpad/demo.sh` em 2026-10-15 23:38. Binário: `app/target/debug/aiai-mcp.exe`. Projeto: `C:\AnimeIAI\producoes\exemplo-s01.aiai`.
Sem app (arquivo de descoberta ausente): as ações de documento rodam em processo e o projeto é salvo a cada mudança.

## 1. Novo projeto

```
aiai-mcp call NEW_PROJECT '{"name":"exemplo-s01","settings":{"width":1920,"height":1080}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","name":"exemplo-s01","path":"C:\\AnimeIAI\\producoes\\exemplo-s01.aiai","revision":0,"settings":{"audio_sample_rate":48000,"dialogue_language":"ja","fps_den":1001,"fps_num":24000,"height":1080,"subtitle_language":"pt-BR","width":1920}}
```

## 2. Episódio

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"create_node","name":"EP01","data":{"type":"episode","number":1}}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-b7dd-7736-8a2f-874739fbdbc6"],"can_redo":false,"can_undo":true,"created":["01a14293-b7dd-7736-8a2f-874739fbdbc6"],"description":"Criar episode \"EP01\"","revision":1}
```

## 3. Cena

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"create_node","name":"Cena 1","parent":"<episódio>","data":{"type":"scene","number":1}}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-b8c3-73b9-b05b-f616e6cf1f18"],"can_redo":false,"can_undo":true,"created":["01a14293-b8c3-73b9-b05b-f616e6cf1f18"],"description":"Criar scene \"Cena 1\"","revision":2}
```

## 4. Corte (48 quadros)

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"create_node","name":"C001","parent":"<cena>","data":{"type":"cut","number":1,"duration":48}}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-b98b-7308-999d-d90c9f8a4930"],"can_redo":false,"can_undo":true,"created":["01a14293-b98b-7308-999d-d90c9f8a4930"],"description":"Criar cut \"C001\"","revision":3}
```

## 5. Camada vetorial

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"create_node","name":"A","parent":"<corte>","data":{"type":"layer","kind":"vector"}}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-ba5b-74cf-956f-fe871de184c5"],"can_redo":false,"can_undo":true,"created":["01a14293-ba5b-74cf-956f-fe871de184c5"],"description":"Criar layer \"A\"","revision":4}
```

## 6. Cel A1

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"create_node","name":"A1","parent":"<camada>","data":{"type":"cel","number":1}}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-bb1c-7086-99b7-aaba5605eb0a"],"can_redo":false,"can_undo":true,"created":["01a14293-bb1c-7086-99b7-aaba5605eb0a"],"description":"Criar cel \"A1\"","revision":5}
```

## 7. Um traço no cel (insert_strokes)

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"insert_strokes","cel":"<cel>","items":[{"index":0,"stroke":{"id":"","points":[...3 pontos...],"geometry":"spline","width":6}}]}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-bb1c-7086-99b7-aaba5605eb0a"],"can_redo":false,"can_undo":true,"created":[],"description":"Desenhar 1 traço(s)","revision":6}
```

## 8. Salvar

```
aiai-mcp call SAVE_PROJECT '{}' --project <pasta>
```

Saída:

```json
{"_via":"headless","name":"exemplo-s01","path":"C:\\AnimeIAI\\producoes\\exemplo-s01.aiai","revision":6}
```

## 9. Consulta tree (outro processo lê do disco)

```
aiai-mcp call QUERY '{"query":{"kind":"tree"}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","assets":[],"characters":[],"documents":[],"episodes":[{"children":[{"children":[{"children":[{"children":[{"id":"01a14293-bb1c-7086-99b7-aaba5605eb0a","name":"A1","number":1,"strokes":1,"tiles":0,"type":"cel"}],"id":"01a14293-ba5b-74cf-956f-fe871de184c5","layer_type":"vector","name":"A","type":"layer","visible":true}],"duration":48,"id":"01a14293-b98b-7308-999d-d90c9f8a4930","name":"C001","number":1,"type":"cut"}],"id":"01a14293-b8c3-73b9-b05b-f616e6cf1f18","name":"Cena 1","type":"scene"}],"id":"01a14293-b7dd-7736-8a2f-874739fbdbc6","name":"EP01","type":"episode"}],"palettes":[]}
```

## 10. Renomear a camada A -> A-linha (set_props)

```
aiai-mcp call APPLY_COMMAND '{"command":{"kind":"set_props","id":"<camada>","name":"A-linha"}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","affected":["01a14293-ba5b-74cf-956f-fe871de184c5"],"can_redo":false,"can_undo":true,"created":[],"description":"Editar 01a14293-ba5b-74cf-956f-fe871de184c5","revision":7}
```

## 11. Estatísticas

```
aiai-mcp call QUERY '{"query":{"kind":"stats"}}' --project <pasta>
```

Saída:

```json
{"_via":"headless","blob_bytes":0,"blobs":0,"by_type":{"cel":1,"cut":1,"episode":1,"layer":1,"scene":1},"can_redo":false,"can_undo":false,"log_entries":0,"nodes":5,"revision":7,"strokes":1,"tiles":0}
```

## 12. Undo em outro processo (honesto: sem histórico entre processos)

```
aiai-mcp call UNDO '{}' --project <pasta>   # exit code 1
```

Saída:

```json
aiai-mcp: [-32000] nada para desfazer
```

## 13. Diff legível do project.json (`git diff --no-index`)

O projeto é uma pasta `.aiai` com `project.json` canônico (chaves ordenadas, indentado); renomear a camada muda uma linha de nome e a revisão.

```diff
diff --git a/headless-before.project.json b/headless-after.project.json
index 80a5b3f..8479b54 100644
--- a/headless-before.project.json
+++ b/headless-after.project.json
@@ -94,12 +94,12 @@
         "visible": true
       },
       "id": "01a14293-ba5b-74cf-956f-fe871de184c5",
-      "name": "A",
+      "name": "A-linha",
       "parent": "01a14293-b98b-7308-999d-d90c9f8a4930",
       "provenance": {
         "created_by": "agent:claude",
         "modified_by": "agent:claude",
-        "revision": 4
+        "revision": 7
       }
     },
     "01a14293-bb1c-7086-99b7-aaba5605eb0a": {
@@ -163,7 +163,7 @@
       }
     }
   },
-  "revision": 6,
+  "revision": 7,
   "roots": {
     "assets": [],
     "characters": [],
```

Arquivos: `headless-before.project.json`, `headless-after.project.json` (esta pasta). Projeto de exemplo: `producoes/exemplo-s01.aiai`.
