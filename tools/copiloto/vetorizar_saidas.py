"""Último passo da bancada: as linhas (Anime2Sketch) entram no vetorizador do app (VECTORIZE) numa camada vetorial,
e o resultado é renderizado pelo motor do app para comparar com a entrada.

Uso: python tools/copiloto/vetorizar_saidas.py --entrada <pasta de linhas> --saida <pasta> [--limiar 0.3] [--exe <aiai-mcp>]
"""
import argparse, glob, json, os, shutil, sys, time
sys.path.insert(0, r"C:\AnimeIAI\tools\calib")
os.environ["AIAI_MCP_ALLOWED_DIRS"] = r"C:\AnimeIAI\calibracao"
from drive import Drive
from PIL import Image

ap = argparse.ArgumentParser()
ap.add_argument("--entrada", required=True)
ap.add_argument("--saida", required=True)
ap.add_argument("--limiar", type=float, default=0.3)
ap.add_argument("--exe", default=r"C:\AnimeIAI\app\target\debug\aiai-mcp.exe")
a = ap.parse_args()
os.makedirs(a.saida, exist_ok=True)
proj = r"C:\AnimeIAI\calibracao\tmp\copiloto-vet.aiai"
res = []
for f in sorted(glob.glob(os.path.join(a.entrada, "*.png"))):
    w, h = Image.open(f).size
    if os.path.exists(proj):
        shutil.rmtree(proj)
    d = Drive(proj, exe=a.exe)
    d.call("NEW_PROJECT", {"path": proj, "name": "vet", "settings": {"width": w, "height": h}})
    ep = d.create("EP", {"type": "episode"})
    sc = d.create("S", {"type": "scene"}, ep)
    cut = d.create("C", {"type": "cut", "duration": 1}, sc)
    lay = d.create("V", {"type": "layer", "kind": "vector"}, cut)
    d.create("V1", {"type": "cel", "number": 1}, lay)
    d.apply({"kind": "add_track", "cut": cut, "track": {"id": "t", "name": "t", "kind": "drawing", "layer": lay, "cells": {}}})
    d.apply({"kind": "set_cells", "cut": cut, "track": "t", "cells": [{"frame": 0, "cell": {"type": "cel", "number": 1}}]})
    t0 = time.perf_counter()
    r = d.call("VECTORIZE", {"cut": cut, "frame": 0, "layer": lay, "source": {"image": {"path": os.path.abspath(f), "fit": "actual"}}, "tip": "g_pen",
                             "threshold": a.limiar, "color": {"type": "direct", "rgba": [0.1, 0.1, 0.1, 1.0]}})
    out = os.path.join(a.saida, os.path.basename(f))
    d.call("RENDER", {"cut": cut, "frame": 0, "out": out, "background": "#ffffff", "camera": False})
    res.append({"arquivo": os.path.basename(f), "tracos": r.get("strokes"), "ms_vetorizar": r.get("ms")})
    print(json.dumps(res[-1], ensure_ascii=False))
json.dump(res, open(os.path.join(a.saida, "vetorizacao.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
