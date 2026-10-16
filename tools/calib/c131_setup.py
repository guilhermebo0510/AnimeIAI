"""Calibração do corte C (#131): estrutura do projeto e fundo. Cada passo é um comando do app via aiai-mcp (headless)."""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

PROJ = r"C:\AnimeIAI\calibracao\projetos\c131.aiai"
d = Drive(PROJ)
if not os.path.exists(os.path.join(PROJ, "project.json")):
    d.call("NEW_PROJECT", {"name": "Calibração C131", "path": PROJ, "settings": {"width": 1920, "height": 1080, "fps_num": 24000, "fps_den": 1001}})

ids_path = os.path.join(PROJ, "..", "c131_ids.json")
ids = json.load(open(ids_path)) if os.path.exists(ids_path) else {}

def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        json.dump(ids, open(ids_path, "w"), indent=1)
    return ids[key]

layer = lambda kind, **kw: {"type": "layer", "kind": kind, **kw}
cel = lambda n: {"type": "cel", "number": n}

ep = node("ep", "EP01", {"type": "episode", "number": 1})
sc = node("scene", "Cena gag", {"type": "scene", "number": 1}, ep)
cut = node("cut", "C131", {"type": "cut", "number": 131, "duration": 17}, sc)
# camadas de baixo para cima
for key, name, kind in [("bg", "BG", "raster"), ("letreiro", "Letreiro", "raster"), ("moldura_cor", "Moldura-cor", "fill"),
                        ("moldura_linha", "Moldura-linha", "vector"), ("fx", "Satsuei", "effect")]:
    node(key, name, layer(kind), cut)
for key in ["bg", "letreiro", "moldura_cor", "moldura_linha"]:
    node(key + "_c1", "1", cel(1), ids[key])
d.apply({"kind": "set_props", "id": ids["moldura_cor"], "data": {"line_refs": [ids["moldura_linha"]]}})

# fundo: degradê radial (centro amarelo-pálido → anel bege → borda rosada), medido por amostragem da referência
d.call("GRADIENT", {"cut": cut, "frame": 0, "layer": ids["bg"], "kind": "radial", "from": [975, 520], "to": [975, 1180],
                    "stops": [[0.0, "#f2f5caff"], [0.30, "#f1eec3ff"], [0.48, "#f0e2beff"], [0.62, "#efd9b8ff"], [0.80, "#ecccaeff"], [1.0, "#e6bba5ff"]]})
print(json.dumps(ids, indent=1))
