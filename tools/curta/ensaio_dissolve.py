"""Ensaio da auditoria 3.11 (C005 → C006: o dissolver de hoje sobrepõe os personagens dos dois planos, uma dupla
exposição fantasma) na CÓPIA do projeto (calibracao/tmp/curta-ensaio.aiai; o arquivo do curta não é tocado): a edição
com o dissolver de 8 quadros, com um mergulho no branco de 10 (gag → calma sem fantasma) e com a dupla exposição
aditiva de 12, os quadros em volta do corte numa folha cada.

Uso: python ensaio_dissolve.py   (grava calibracao/tmp/ensaio/dissolve_<tipo>.png)
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
cuts = ids["cuts"]
for kind, frames in (("dissolve", 8), ("dip_white", 10), ("additive_dissolve", 12)):
    clips = [{"cut": c} for c in cuts]
    clips[0]["transition_in"] = {"kind": "fade_black", "frames": 12}
    clips[4]["transition_in"] = {"kind": "flash", "frames": 3}
    clips[5]["transition_in"] = {"kind": kind, "frames": frames}
    r = d.call("EDIT_SET", {"id": ids["edit"], "name": "最後の一輪 — edição", "clips": clips, "credits": [], "credits_seconds": 1.0, "fade_out_end": 12})
    items = r["timeline"].get("clips") or r["timeline"].get("items")
    # o item de imagem do C006 (a lista v2 mistura imagem e som)
    it = next(i for i in items if isinstance(i, dict) and i.get("source", {}).get("type") == "cut" and i["source"].get("cut") == cuts[5])
    s = it["start"]
    at = [s - 6, s - 3, s, s + 3, s + 6]
    out = f"{OUT}/dissolve_{kind}.png"
    d.call("EDIT_PREVIEW", {"edit": ids["edit"], "frames": at, "columns": 5, "thumb_width": 384, "burn_subtitles": False, "out": out})
    print(kind, "corte no quadro", s, "->", out)
