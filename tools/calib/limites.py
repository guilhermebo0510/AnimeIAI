"""Leitura: limites da régua (decisão 24) para quadros de qualquer corte, num projeto vazio só para isso.
uso: python limites.py corte quadro [quadro ...]"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

corte = int(sys.argv[1])
PROJ = rf"C:\AnimeIAI\calibracao\projetos\limites_{corte}.aiai"
d = Drive(PROJ)
if not os.path.exists(os.path.join(PROJ, "project.json")):
    d.call("NEW_PROJECT", {"name": f"Limites {corte}", "path": PROJ, "settings": {"width": 1920, "height": 1080, "fps_num": 24000, "fps_den": 1001}})
ep = d.create("EP", {"type": "episode", "number": 1})
sc = d.create("SC", {"type": "scene", "number": 1}, ep)
cut = d.create("C", {"type": "cut", "number": corte, "duration": 400}, sc)
for q in sys.argv[2:]:
    q = int(q)
    ref = rf"C:\AnimeIAI\calibracao\ref\cut_{corte}\frames\f_{q + 1:04d}.png"
    r = d.call("COMPARE", {"cut": cut, "frame": q, "reference": ref, "noise_floor": 0.01, "out_dir": rf"C:\AnimeIAI\calibracao\tmp\lim_{corte}_{q}"})
    th = r.get("report", r)["thresholds"]
    print(corte, q, "flip", round(th["flip_mean_max"], 4), "ssim", round(th["ssim_min"], 4), "F1", round(th["edge_f1_min"], 4), flush=True)
