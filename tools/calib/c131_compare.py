"""COMPARE do corte C contra a referência. uso: python c131_compare.py [quadro] [tentativa]"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

PROJ = r"C:\AnimeIAI\calibracao\projetos\c131.aiai"
ids = json.load(open(os.path.join(PROJ, "..", "c131_ids.json")))
d = Drive(PROJ)
frame = int(sys.argv[1]) if len(sys.argv) > 1 else 15
tag = sys.argv[2] if len(sys.argv) > 2 else "t"
ref = rf"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_{frame + 1:04d}.png"
out = rf"C:\AnimeIAI\sprints\registro\calibracao\C131\tentativas\{tag}_q{frame:02d}"
r = d.call("COMPARE", {"cut": ids["cut"], "frame": frame, "reference": ref, "noise_floor": 0.0075, "out_dir": out})
rep = r.get("report", r)
th = rep.get("thresholds", {})
print(json.dumps({"pass": r.get("pass"), "flip": round(rep["flip"]["mean"], 4), "flip_lim": round(th.get("flip_mean_max", 0), 4),
                  "ssim": round(rep["ssim"], 4), "ssim_lim": round(th.get("ssim_min", 0), 4), "f1": round(rep["edges"]["f1"], 4),
                  "f1_lim": round(th.get("edge_f1_min", 0), 4), "dE": round(rep["delta_e"]["overall_mean"], 2)}, indent=1))
print("arquivos:", r.get("files"))
