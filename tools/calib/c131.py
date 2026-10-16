"""Calibração do corte C (#131, moldura de rosas + ゴン). Desenho final (quadro 15) primeiro: linhas da moldura estimadas pelo
agente (tabelas em c131_rosas.py), medidas pelo paquímetro de faixa na cor da tinta (decisão 22); cor por balde com paleta."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive
import meas

PROJ = r"C:\AnimeIAI\calibracao\projetos\c131.aiai"
IDS = os.path.join(PROJ, "..", "c131_ids.json")
d = Drive(PROJ)
ids = json.load(open(IDS))
F = int(os.environ.get("C131_Q", "15"))
REF = rf"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_{F + 1:04d}.png"
INK = "#1a0508"


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        json.dump(ids, open(IDS, "w"), indent=1)
    return ids[key]


ORDER = ["bg", "letreiro", "caules", "moldura_cor", "moldura_luz", "moldura_linha", "fx", "fecho"]


def reorder():
    present = [k for k in ORDER if k in ids]
    for i, k in enumerate(present):
        d.apply({"kind": "move_node", "id": ids[k], "parent": ids["cut"], "index": i})


def measured(name, poly, w_nom, closed=False, step=3, search=(16.0, 10.0), ink=INK, tol=0.12):
    """Centro e largura da tinta ao longo da estimativa (paquímetro de faixa na cor da tinta)."""
    pts, ws = meas.measure_band(d, REF, meas.densify(poly, step, closed=closed), ink, tol, closed=closed, passes=search, smooth=1)
    c = meas.curve_from_measure(pts, ws, {"type": "direct", "rgba": [0x1a / 255, 0x05 / 255, 0x08 / 255, 1.0]}, closed=closed, every=1)
    c["width"] = max(c["width"], 0.6 * w_nom)
    c["profile"] = [[t, max(0.4, min(1.6, w))] for t, w in c["profile"]]
    return c


def render(name):
    out = rf"C:\AnimeIAI\sprints\registro\calibracao\C131\{name}.png"
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": out})
    return out
