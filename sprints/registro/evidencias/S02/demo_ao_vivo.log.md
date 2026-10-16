# S02 - demo ao vivo (aiai-mcp call -> app aberto)

Gerado por `demo_ao_vivo.py`.

### status do app

```
aiai-mcp call GET_STATUS '{}'
```

(0.05 s, exit 0)

```json
{"bridge":{"addr":"127.0.0.1:64097","requests_total":2},"can_redo":false,"can_undo":false,"dirty":false,"engine":"AnimeIAI Studio","fps":12.341150283813477,"frame_count":12,"frame_dt_ms":188.71380615234375,"frame_ms":81.02972412109375,"gpu":"NVIDIA GeForce GTX 1650","mode":"app","name":"Sem título","nodes":0,"pixels_per_point":1.25,"project":null,"redo_steps":0,"revision":0,"theme":"dark","undo_steps":0,"version":"0.1.0","window_size":[1536.0,864.0],"window_size_physical":[1920.0,1080.0],"workspace":"roteiro"}
```

### NEW_PROJECT

```
aiai-mcp call NEW_PROJECT '{"name": "S02 ao vivo", "path": "C:\\AnimeIAI\\producoes\\s02-ao-vivo.aiai", "overwrite": true, "settings": {"width": 1920, "height": 1080}}'
```

(0.04 s, exit 0)

```json
{"name":"S02 ao vivo","path":"C:\\AnimeIAI\\producoes\\s02-ao-vivo.aiai","revision":0,"settings":{"audio_sample_rate":48000,"dialogue_language":"ja","fps_den":1001,"fps_num":24000,"height":1080,"subtitle_language":"pt-BR","width":1920}}
```

### criar EP01

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "EP01", "data": {"type": "episode", "number": 1}}}'
```

(0.04 s, exit 0)

```json
{"affected":["01a142d0-1208-73e1-9263-f3517b7fec3b"],"can_redo":false,"can_undo":true,"created":["01a142d0-1208-73e1-9263-f3517b7fec3b"],"description":"Criar episode \"EP01\"","revision":1}
```

### criar Cena 1

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "Cena 1", "data": {"type": "scene", "number": 1}, "parent": "01a142d0-1208-73e1-9263-f3517b7fec3b"}}'
```

(0.03 s, exit 0)

```json
{"affected":["01a142d0-1228-76d1-b662-b7dd3a33acff"],"can_redo":false,"can_undo":true,"created":["01a142d0-1228-76d1-b662-b7dd3a33acff"],"description":"Criar scene \"Cena 1\"","revision":2}
```

### criar C001

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "C001", "data": {"type": "cut", "number": 1, "duration": 48}, "parent": "01a142d0-1228-76d1-b662-b7dd3a33acff"}}'
```

(0.04 s, exit 0)

```json
{"affected":["01a142d0-1255-7753-aa8c-d499e0cb5932"],"can_redo":false,"can_undo":true,"created":["01a142d0-1255-7753-aa8c-d499e0cb5932"],"description":"Criar cut \"C001\"","revision":3}
```

### criar BG

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "BG", "data": {"type": "layer", "kind": "raster"}, "parent": "01a142d0-1255-7753-aa8c-d499e0cb5932"}}'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-1284-737e-8e8b-866471c0deb9"],"can_redo":false,"can_undo":true,"created":["01a142d0-1284-737e-8e8b-866471c0deb9"],"description":"Criar layer \"BG\"","revision":4}
```

### criar A-cor

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "A-cor", "data": {"type": "layer", "kind": "fill"}, "parent": "01a142d0-1255-7753-aa8c-d499e0cb5932"}}'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-12b4-735e-8d7e-eeff690b3997"],"can_redo":false,"can_undo":true,"created":["01a142d0-12b4-735e-8d7e-eeff690b3997"],"description":"Criar layer \"A-cor\"","revision":5}
```

### criar A-linha

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "A-linha", "data": {"type": "layer", "kind": "vector"}, "parent": "01a142d0-1255-7753-aa8c-d499e0cb5932"}}'
```

(0.03 s, exit 0)

```json
{"affected":["01a142d0-12d5-745e-929c-ef2a658fc2a1"],"can_redo":false,"can_undo":true,"created":["01a142d0-12d5-745e-929c-ef2a658fc2a1"],"description":"Criar layer \"A-linha\"","revision":6}
```

### criar BG1

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "BG1", "data": {"type": "cel", "number": 1}, "parent": "01a142d0-1284-737e-8e8b-866471c0deb9"}}'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-1308-76ff-96df-918bb3568df4"],"can_redo":false,"can_undo":true,"created":["01a142d0-1308-76ff-96df-918bb3568df4"],"description":"Criar cel \"BG1\"","revision":7}
```

### criar A-cor1

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "A-cor1", "data": {"type": "cel", "number": 1}, "parent": "01a142d0-12b4-735e-8d7e-eeff690b3997"}}'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-133e-75f4-8e66-6e4979a678c7"],"can_redo":false,"can_undo":true,"created":["01a142d0-133e-75f4-8e66-6e4979a678c7"],"description":"Criar cel \"A-cor1\"","revision":8}
```

### criar A-linha1

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "A-linha1", "data": {"type": "cel", "number": 1}, "parent": "01a142d0-12d5-745e-929c-ef2a658fc2a1"}}'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-136f-7712-8f1f-0bc0f1b49d3a"],"can_redo":false,"can_undo":true,"created":["01a142d0-136f-7712-8f1f-0bc0f1b49d3a"],"description":"Criar cel \"A-linha1\"","revision":9}
```

### line_refs de A-cor -> A-linha

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "set_props", "id": "01a142d0-12b4-735e-8d7e-eeff690b3997", "data": {"line_refs": ["01a142d0-12d5-745e-929c-ef2a658fc2a1"]}}}'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-12b4-735e-8d7e-eeff690b3997"],"can_redo":false,"can_undo":true,"created":[],"description":"Editar line_refs (01a142d0-12b4-735e-8d7e-eeff690b3997)","revision":10}
```

### criar Settei

```
aiai-mcp call APPLY_COMMAND '{"command": {"kind": "create_node", "name": "Settei", "data": {"type": "palette", "colors": [{"id": "linha", "name": "linha", "group": "linha", "role": "normal", "rgba": [0.22745098039215686, 0.16862745098039217, 0.16862745098039217, 1.0]}, {"id": "pele", "name": "pele", "group": "pele", "role": "normal", "rgba": [0.984313725490196, 0.8627450980392157, 0.7686274509803922, 1.0]}, {"id": "cabelo", "name": "cabelo", "group": "cabelo", "role": "normal", "rgba": [0.9098039215686274, 0.6980392156862745, 0.22745098039215686, 1.0]}, {"id": "branco", "name": "branco", "group": "branco", "role": "normal'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-13d0-7731-9c85-c4deb6d1f795"],"can_redo":false,"can_undo":true,"created":["01a142d0-13d0-7731-9c85-c4deb6d1f795"],"description":"Criar palette \"Settei\"","revision":11}
```

### GRADIENT ceu

```
aiai-mcp call GRADIENT '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-1284-737e-8e8b-866471c0deb9", "kind": "linear", "from": [0, 0], "to": [0, 760], "stops": [[0.0, "#2f6fd6ff"], [1.0, "#cfe9ffff"]]}'
```

(0.27 s, exit 0)

```json
{"affected":["01a142d0-1308-76ff-96df-918bb3568df4"],"can_redo":false,"can_undo":true,"created":[],"description":"Pintar 40 tile(s)","revision":12}
```

### GRADIENT brilho do sol

```
aiai-mcp call GRADIENT '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-1284-737e-8e8b-866471c0deb9", "kind": "radial", "from": [1560, 230], "to": [1560, 520], "opacity": 0.85, "stops": [[0.0, "#fff3c0ff"], [1.0, "#fff3c000"]]}'
```

(0.35 s, exit 0)

```json
{"affected":["01a142d0-1308-76ff-96df-918bb3568df4"],"can_redo":false,"can_undo":true,"created":[],"description":"Pintar 40 tile(s)","revision":13}
```

### PAINT colina (preset por nome)

```
aiai-mcp call PAINT '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-1284-737e-8e8b-866471c0deb9", "preset": "Guache seco", "preset_overrides": {"size": 220}, "color": {"type": "direct", "rgba": [0.35294117647058826, 0.6431372549019608, 0.4117647058823529, 1.0]}, "strokes": [{"points": [[-40, 1000], [420, 930], [940, 1000], [1460, 940], [1960, 1000]]}, {"points": [[-40, 1070], [600, 1040], [1300, 1080], [1960, 1050]]}], "seed": 7}'
```

(0.60 s, exit 0)

```json
{"affected":["01a142d0-1308-76ff-96df-918bb3568df4"],"can_redo":false,"can_undo":true,"created":[],"description":"Pintar 16 tile(s)","revision":14}
```

### RENDER so fundo

```
aiai-mcp call RENDER '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "background": "#ffffff"}' --png C:\AnimeIAI\sprints\registro\evidencias\S02\ao-vivo-1-fundo.png
```

(0.28 s, exit 0)

```json
{"bytes":315090,"cut":"01a142d0-1255-7753-aa8c-d499e0cb5932","frame":0,"height":1080,"png_base64":"<315090 bytes salvos em C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-1-fundo.png>","saved_to":"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-1-fundo.png","warnings":[],"width":1920}
```

### DRAW_CURVES cabelo, rosto, boca, nariz, pescoco

```
aiai-mcp call DRAW_CURVES '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-12d5-745e-929c-ef2a658fc2a1", "curves": [{"points": [[770, 470], [756, 340], [830, 220], [960, 176], [1090, 220], [1164, 340], [1150, 470], [1100, 418], [1040, 472], [960, 410], [880, 472], [820, 418]], "closed": true, "width": 7, "widths": [0.6, 1.0, 0.6], "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "linha"}}, {"points": [[770, 470], [790, 610], [860, 730], [960, 786], [1060, 730], [1130, 610], [1150, 470]], "width": 7, "widths": [0.7, 1.0, 0.7], "color": {"type": "palet'
```

(0.04 s, exit 0)

```json
{"affected":["01a142d0-136f-7712-8f1f-0bc0f1b49d3a"],"can_redo":false,"can_undo":true,"created":[],"description":"Desenhar 6 traço(s)","revision":15}
```

### DRAW_CURVES olhos, pupilas e sobrancelhas (mirror_x=960)

```
aiai-mcp call DRAW_CURVES '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-12d5-745e-929c-ef2a658fc2a1", "mirror_x": 960, "curves": [{"points": [[842, 568], [888, 538], [936, 566], [888, 604]], "closed": true, "width": 5, "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "linha"}}, {"points": [[876, 560], [890, 552], [904, 562], [890, 582]], "closed": true, "width": 4, "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "linha"}}, {"points": [[836, 520], [886, 500], [940, 516]], "width": 6, "widths": [0.4, 1.0, 0.5'
```

(0.05 s, exit 0)

```json
{"affected":["01a142d0-136f-7712-8f1f-0bc0f1b49d3a"],"can_redo":false,"can_undo":true,"created":[],"description":"Desenhar 6 traço(s)","revision":16}
```

### RENDER so a linha

```
aiai-mcp call RENDER '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layers": ["01a142d0-12d5-745e-929c-ef2a658fc2a1"], "background": "#ffffff"}' --png C:\AnimeIAI\sprints\registro\evidencias\S02\ao-vivo-2-linha.png
```

(0.18 s, exit 0)

```json
{"bytes":94445,"cut":"01a142d0-1255-7753-aa8c-d499e0cb5932","frame":0,"height":1080,"png_base64":"<94445 bytes salvos em C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-2-linha.png>","saved_to":"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-2-linha.png","warnings":[],"width":1920}
```

### LINT antes de pintar

```
aiai-mcp call LINT '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-12b4-735e-8d7e-eeff690b3997", "gap": 3, "min_area": 40}'
```

(0.40 s, exit 0)

```json
{"painted":0,"regions_total":8,"unpainted":[{"area":88788,"bounds":[755,179,409,290],"label":2,"point":[948,179]},{"area":85338,"bounds":[771,413,378,369],"label":3,"point":[955,413]},{"area":2848,"bounds":[845,541,88,60],"label":4,"point":[882,541]},{"area":2848,"bounds":[987,541,88,60],"label":5,"point":[1025,541]},{"area":405,"bounds":[878,554,24,26],"label":6,"point":[887,554]},{"area":405,"bounds":[1018,554,24,26],"label":7,"point":[1028,554]},{"area":974,"bounds":[920,704,80,29],"label":8,"point":[920,704]}]}
```

### FILL de todas as regioes (paleta)

```
aiai-mcp call FILL '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-12b4-735e-8d7e-eeff690b3997", "seeds": [{"x": 948, "y": 179, "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "cabelo"}}, {"x": 955, "y": 413, "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "pele"}}, {"x": 920, "y": 704, "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "boca"}}, {"x": 882, "y": 541, "color": {"type": "palette", "palette": "01a142d0-13d0-7731-9c85-c4deb6d1f795", "color": "branco"}'
```

(0.33 s, exit 0)

```json
{"affected":["01a142d0-133e-75f4-8e66-6e4979a678c7"],"can_redo":false,"can_undo":true,"created":[],"description":"Preencher (7 região(ões))","revision":17}
```

### LINT depois de pintar

```
aiai-mcp call LINT '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "layer": "01a142d0-12b4-735e-8d7e-eeff690b3997", "gap": 3, "min_area": 40}'
```

(0.35 s, exit 0)

```json
{"painted":7,"regions_total":8,"unpainted":[]}
```

### RENDER final

```
aiai-mcp call RENDER '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "background": "#ffffff"}' --png C:\AnimeIAI\sprints\registro\evidencias\S02\ao-vivo-3-final.png
```

(0.23 s, exit 0)

```json
{"bytes":377891,"cut":"01a142d0-1255-7753-aa8c-d499e0cb5932","frame":0,"height":1080,"png_base64":"<377891 bytes salvos em C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-3-final.png>","saved_to":"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-3-final.png","warnings":[],"width":1920}
```

### UI_ACTION set_workspace pintura

```
aiai-mcp call UI_ACTION '{"action": "set_workspace", "workspace": "pintura"}'
```

(0.03 s, exit 0)

```json
{"action":"set_workspace","previous":"roteiro","workspace":"pintura"}
```

### SCREENSHOT da janela

```
aiai-mcp call SCREENSHOT '{}' --png C:\AnimeIAI\sprints\registro\evidencias\S02\ao-vivo-4-janela.png
```

(0.16 s, exit 0)

```json
{"bytes":687811,"height":1080,"pixels_per_point":1.25,"png_base64":"<687811 bytes salvos em C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-4-janela.png>","saved_to":"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-4-janela.png","target":"window","width":1920,"workspace":"pintura"}
```

### status depois do desenho

```
aiai-mcp call GET_STATUS '{}'
```

(0.02 s, exit 0)

```json
{"bridge":{"addr":"127.0.0.1:64097","requests_total":28},"can_redo":false,"can_undo":true,"dirty":true,"engine":"AnimeIAI Studio","fps":37.47667694091797,"frame_count":98,"frame_dt_ms":89.6238021850586,"frame_ms":26.68326187133789,"gpu":"NVIDIA GeForce GTX 1650","mode":"app","name":"S02 ao vivo","nodes":10,"pixels_per_point":1.25,"project":"C:\\AnimeIAI\\producoes\\s02-ao-vivo.aiai","redo_steps":0,"revision":17,"theme":"dark","undo_steps":17,"version":"0.1.0","window_size":[1536.0,864.0],"window_size_physical":[1920.0,1080.0],"workspace":"pintura"}
```

### UNDO (o balde)

```
aiai-mcp call UNDO '{}'
```

(0.03 s, exit 0)

```json
{"affected":["01a142d0-133e-75f4-8e66-6e4979a678c7"],"can_redo":true,"can_undo":true,"created":[],"description":"Desfazer: Preencher (7 região(ões))","revision":18}
```

### RENDER depois do UNDO

```
aiai-mcp call RENDER '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "background": "#ffffff"}' --png C:\AnimeIAI\sprints\registro\evidencias\S02\ao-vivo-5-undo.png
```

(0.30 s, exit 0)

```json
{"bytes":375806,"cut":"01a142d0-1255-7753-aa8c-d499e0cb5932","frame":0,"height":1080,"png_base64":"<375806 bytes salvos em C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-5-undo.png>","saved_to":"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-5-undo.png","warnings":[],"width":1920}
```

### REDO

```
aiai-mcp call REDO '{}'
```

(0.04 s, exit 0)

```json
{"affected":["01a142d0-133e-75f4-8e66-6e4979a678c7"],"can_redo":false,"can_undo":true,"created":[],"description":"Refazer: Preencher (7 região(ões))","revision":19}
```

### RENDER depois do REDO

```
aiai-mcp call RENDER '{"cut": "01a142d0-1255-7753-aa8c-d499e0cb5932", "frame": 0, "background": "#ffffff"}' --png C:\AnimeIAI\sprints\registro\evidencias\S02\ao-vivo-6-redo.png
```

(0.31 s, exit 0)

```json
{"bytes":377891,"cut":"01a142d0-1255-7753-aa8c-d499e0cb5932","frame":0,"height":1080,"png_base64":"<377891 bytes salvos em C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-6-redo.png>","saved_to":"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\ao-vivo-6-redo.png","warnings":[],"width":1920}
```

### SAVE_PROJECT

```
aiai-mcp call SAVE_PROJECT '{}'
```

(0.05 s, exit 0)

```json
{"name":"S02 ao vivo","path":"C:\\AnimeIAI\\producoes\\s02-ao-vivo.aiai","revision":19}
```

## Veredito (hash dos PNG renderizados)

hash final=3fbac0720b4eeb61 | apos UNDO=e5ffdb5dd274b766 | apos REDO=3fbac0720b4eeb61 | undo muda a imagem: True | redo restitui: True

Status final do app: `{"bridge": {"addr": "127.0.0.1:64097", "requests_total": 28}, "can_redo": false, "can_undo": true, "dirty": true, "engine": "AnimeIAI Studio", "fps": 37.47667694091797, "frame_count": 98, "frame_dt_ms": 89.6238021850586, "frame_ms": 26.68326187133789, "gpu": "NVIDIA GeForce GTX 1650", "mode": "app", "name": "S02 ao vivo", "nodes": 10, "pixels_per_point": 1.25, "project": "C:\\AnimeIAI\\producoes\\s02-ao-vivo.aiai", "redo_steps": 0, "revision": 17, "theme": "dark", "undo_steps": 17, "version": "0.1.0", "window_size": [1536.0, 864.0], "window_size_physical": [1920.0, 1080.0], "workspace": "pintura"}`
