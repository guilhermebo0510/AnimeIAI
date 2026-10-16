"""COMPARE do corte D contra a referência (piso de ruído medido 0,0047). uso: python d218_compare.py [quadro] [tentativa]"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import d, ids

frame = int(sys.argv[1]) if len(sys.argv) > 1 else 0
tag = sys.argv[2] if len(sys.argv) > 2 else "t"
ref = rf"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_{frame + 1:04d}.png"
out = rf"C:\AnimeIAI\sprints\registro\calibracao\D218\tentativas\{tag}_q{frame:02d}"
r = d.call("COMPARE", {"cut": ids["cut"], "frame": frame, "reference": ref, "noise_floor": 0.0047, "out_dir": out})
rep = r.get("report", r)
keep = {k: rep[k] for k in rep if not isinstance(rep[k], (list, dict))}
print(json.dumps({"pass": r.get("pass"), "flip_limit": r.get("flip_limit"), "flip_hard_limit": r.get("flip_hard_limit"),
                  "ssim_limit": r.get("ssim_limit"), "edge_f1_limit": r.get("edge_f1_limit"), "verdict": rep.get("verdict"),
                  "line_width_ok": r.get("line_width_ok"), **keep}, indent=1))
print("arquivos:", r.get("files"))
