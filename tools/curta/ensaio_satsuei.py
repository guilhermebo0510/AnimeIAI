"""Ensaio da auditoria 3.9 (satsuei nos personagens) numa CÓPIA do projeto de produção (calibracao/tmp/curta-ensaio.aiai;
o arquivo do curta não é tocado): no C003 (Hana) e no C004 (Ren), contra o pôr do sol, a camada de cor do personagem
ganha luz de recorte do lado do sol, light wrap (a luz do céu vaza para a borda) e uma "para" quente (degradê do lado
do sol em luz suave). Renderiza o mesmo quadro antes e depois.

Uso: python ensaio_satsuei.py   (grava calibracao/tmp/ensaio/satsuei_<corte>_<antes|depois>.png)
"""
import json
import os
import shutil
import sys

SRC = "C:/AnimeIAI/producoes/micro-anime"
DST = "C:/AnimeIAI/calibracao/tmp/curta-ensaio.aiai"
OUT = "C:/AnimeIAI/calibracao/tmp/ensaio"
if not os.path.exists(DST + "/project.json"):
    shutil.copytree(SRC + "/ultima-rosa.aiai", DST, ignore=shutil.ignore_patterns("*.lock", "lock"))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "calib"))
from drive import Drive  # noqa: E402

d = Drive(DST)
ids = json.load(open(SRC + "/ids.json", encoding="utf-8"))
os.makedirs(OUT, exist_ok=True)

LAYERS = ("cor", "sombra", "olhos", "linha", "brilho")


def char_group(cut, prefix):
    """As cinco camadas do personagem num grupo (mistura normal: o grupo é composto e recebe os efeitos na silhueta
    inteira; na camada de cor sozinha a luz de recorte acendia cada buraco sob os traços)."""
    key = prefix + "_grupo"
    if key in ids:
        return ids[key]
    q = d.call("QUERY", {"query": {"kind": "node", "id": cut}})
    kids = q.get("node", q).get("children", [])
    first = min(kids.index(ids[f"{prefix}_{k}"]) for k in LAYERS if ids[f"{prefix}_{k}"] in kids)
    g = d.create("personagem", {"type": "layer", "kind": "group", "blend": "normal"}, cut)
    for i, k in enumerate(LAYERS):
        d.apply({"kind": "move_node", "id": ids[f"{prefix}_{k}"], "parent": g, "index": i})
    d.apply({"kind": "move_node", "id": g, "parent": cut, "index": first})
    ids[key] = g
    return g


# (corte, prefixo das camadas do personagem, quadro, ângulo do sol)
SHOTS = [("C003", ids["cuts"][2], "c3_hana", 40, 200.0), ("C004", ids["cuts"][3], "c4_ren", 40, 340.0)]
for name, cut, prefix, frame, sun in SHOTS:
    cor = char_group(cut, prefix)
    d.call("EFFECTS", {"op": "set", "layer": cor, "effects": []})
    d.call("RENDER", {"cut": cut, "frame": frame, "out": f"{OUT}/satsuei_{name}_antes.png"})
    left = sun > 90 and sun < 270
    fx = [
        {"effect": {"type": "rim_light", "angle": sun, "width": 9.0, "color": [1.0, 0.78, 0.48], "intensity": 0.9}, "enabled": True},
        {"effect": {"type": "light_wrap", "radius": 18.0, "amount": 0.45}, "enabled": True},
        {"effect": {"type": "para", "from": [0.0 if left else 1920.0, 300.0], "to": [1920.0 if left else 0.0, 900.0],
                    "color_from": [1.0, 0.62, 0.32, 0.45], "color_to": [0.45, 0.35, 0.65, 0.25], "mode": "soft_light"}, "enabled": True},
    ]
    r = d.call("EFFECTS", {"op": "set", "layer": cor, "effects": fx})
    print(name, "efeitos:", [e["effect"]["type"] for e in fx])
    print(d.call("RENDER", {"cut": cut, "frame": frame, "out": f"{OUT}/satsuei_{name}_depois.png"})["path"])
