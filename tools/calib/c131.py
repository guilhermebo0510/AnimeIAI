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
FLOOR = float(os.environ.get("C131_PISO", "0.9"))
WSCALE = float(os.environ.get("C131_LARG", "1.0"))
QCOR = float(os.environ.get("C131_QCOR", "0.125"))
MODE = os.environ.get("C131_MODO", "meia")  # meia altura + âncora: F1 0,6614 (faixa 0,6565)
ANCHOR = os.environ.get("C131_ANCORA", "1") == "1"
FINE = os.environ.get("C131_FINO", "0") == "1"  # busca curta: F1 0,674 (pior que 0,691)
QLINE = {}  # cor por linha: quantil próprio (linhas vermelhas suaves entre pétalas)
QCOR_VERM = float(os.environ.get("C131_QCOR_V", "0.125"))  # mediana (0,5) e 0,3 pioraram o F1


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        json.dump(ids, open(IDS, "w"), indent=1)
    return ids[key]


ORDER = ["bg", "letreiro", "caules", "moldura_cor", "moldura_luz", "moldura_linha", "linha_suave", "fx", "foco", "foco_mascara", "fecho"]


def reorder():
    present = [k for k in ORDER if k in ids]
    for i, k in enumerate(present):
        d.apply({"kind": "move_node", "id": ids[k], "parent": ids["cut"], "index": i})


def measured(name, poly, w_nom, closed=False, step=3, search=(16.0, 10.0), ink=INK, tol=0.12, mode=None, anchor=None, fine=None):
    """Centro e largura da tinta ao longo da estimativa (paquímetro de faixa na cor da tinta)."""
    mode = MODE if mode is None else mode
    anchor = ANCHOR if anchor is None else anchor
    fine_v = (ink != INK and FINE) if fine is None else fine
    if mode == "meia":  # meia altura do contraste (lados parecidos: pétala dos dois lados)
        ps = ((6.0, 0.06), (3.0, 0.06)) if fine_v else ((14.0, 0.06), (8.0, 0.06))  # busca curta: não pula para a pétala vizinha
        pts, ws = meas.measure_line(d, REF, meas.densify(poly, step, closed=closed), closed=closed, passes=ps, smooth=1)
    else:
        pts, ws = meas.measure_band(d, REF, meas.densify(poly, step, closed=closed), ink, tol, closed=closed,
                                    passes=(8.0, 5.0) if fine_v else search, smooth=1)
    if anchor:  # bordas medidas dos dois lados (MEASURE_EDGE): as arestas do traço caem nas arestas da referência
        for sr in ((3.0, 2.0) if fine_v else (6.0, 3.0)):
            pts, ws = meas.anchor_line(d, REF, pts, [max(wv or 0.0, FLOOR * w_nom) for wv in ws], w_nom, closed=closed, search=sr,
                                       w_range=(2.5, 26.0))
    # largura lida por ponto oscila (1–10 px) onde a normal cruza a linha de viés ou pega a ponta: piso na largura nominal
    ws = [max(wv or 0.0, FLOOR * w_nom) * WSCALE for wv in ws]
    # cor própria da linha: mediana medida nos centros (as linhas de dentro das rosas são vermelho-escuras, não pretas)
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": REF, "points": [[round(p[0]), round(p[1])] for p in pts[::2]], "radius": 0}))
    cs = sorted([p["rgb"] for p in pr], key=sum)
    q = QLINE.get(name, QCOR_VERM if ink != INK else QCOR)  # linhas vermelho-escuras (de dentro das rosas): cor mediana, contraste baixo como na ref.
    col = list(cs[int(len(cs) * q)]) if cs else [0x1a / 255, 0x05 / 255, 0x08 / 255]  # oitavo mais escuro: o centro da tinta
    c = meas.curve_from_measure(pts, ws, {"type": "direct", "rgba": col + [1.0]}, closed=closed, every=1)
    c["width"] = max(c["width"], 0.6 * w_nom * WSCALE)
    c["profile"] = [[t, max(0.4, min(1.6, w))] for t, w in c["profile"]]
    return c


def render(name):
    out = rf"C:\AnimeIAI\sprints\registro\calibracao\C131\{name}.png"
    d.call("RENDER", {"cut": ids["cut"], "frame": F, "out": out})
    return out
