"""Ensaio da auditoria 3.8 (C002: braços rígidos como tábuas, sem flare no toque) na CÓPIA do projeto
(calibracao/tmp/curta-ensaio.aiai; o arquivo do curta não é tocado):

  * os braços giram em volta do ombro (âncora fora do quadro) enquanto entram: chegam num arco, sobem um pouco e
    assentam no toque, em vez de deslizar retos;
  * no toque (~quadro 32) uma camada de efeito com flare acende no ponto de contato e apaga em ~14 quadros.

Renderiza os mesmos quadros antes e depois numa folha (calibracao/tmp/ensaio/c002_<antes|depois>.png).
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
cut = ids["cuts"][1]
FRAMES = [8, 18, 28, 33, 40, 52]
LAYERS = ("cor", "sombra", "olhos", "linha", "brilho")
CONTACT = (960, 572)

d.call("RENDER_SHEET", {"cut": cut, "frames": FRAMES, "columns": 3, "thumb_width": 480, "out": f"{OUT}/c002_antes.png"})

# braços: giro em volta do ombro (fora do quadro), do mesmo lado de onde entram
SHOULDER = {"hana": (-260.0, 760.0, -9.0), "ren": (2180.0, 760.0, 9.0)}
TOUCH = {"hana": 30, "ren": 32}
for who, (ax, ay, start) in SHOULDER.items():
    for k in LAYERS:
        lid = ids[f"c2_{who}_{k}"]
        for prop, v in (("anchor_x", ax), ("anchor_y", ay)):
            d.call("LAYER_MOTION", {"op": "keys", "layer": lid, "property": prop, "keys": [{"frame": 0, "value": v, "ease": "hold"}]})
        t = TOUCH[who]
        d.call("LAYER_MOTION", {"op": "keys", "layer": lid, "property": "rotation", "keys": [
            {"frame": 0, "value": start, "ease": "ease_out"}, {"frame": t - 6, "value": -start * 0.15, "ease": "ease_in_out"},
            {"frame": t, "value": 0.0, "ease": "hold"}, {"frame": 38, "value": 0.0, "ease": "ease_out"}, {"frame": 50, "value": start * 0.12, "ease": "linear"}]})

# flare no toque: camada de efeito por cima de tudo, opacidade animada
if "c2_flare" not in ids:
    fl = d.create("flare do toque", {"type": "layer", "kind": "effect"}, cut)
    ids["c2_flare"] = fl
fl = ids["c2_flare"]
d.call("EFFECTS", {"op": "set", "layer": fl, "effects": [
    {"effect": {"type": "lens_flare", "position": list(CONTACT), "intensity": 1.0, "color": [1.0, 0.86, 0.62]}, "enabled": True}]})
d.call("LAYER_MOTION", {"op": "keys", "layer": fl, "property": "opacity", "keys": [
    {"frame": 0, "value": 0.0, "ease": "hold"}, {"frame": 29, "value": 0.0, "ease": "ease_out"}, {"frame": 33, "value": 1.0, "ease": "ease_in"},
    {"frame": 47, "value": 0.0, "ease": "hold"}]})

d.call("RENDER_SHEET", {"cut": cut, "frames": FRAMES, "columns": 3, "thumb_width": 480, "out": f"{OUT}/c002_depois.png"})
print("ok", f"{OUT}/c002_antes.png", f"{OUT}/c002_depois.png")
