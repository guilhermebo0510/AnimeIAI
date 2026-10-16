"""Calibração do corte D (#218, chibi). Projeto, camadas e desenho medido do quadro 0 (desenho 1)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive
import meas

PROJ = r"C:\AnimeIAI\calibracao\projetos\d218.aiai"
# desenho em trabalho (variável D218_DES; padrão 1): as camadas "fixas" ganham um cel por desenho só quando o desenho é um
# redesenho inteiro (tremor dos desenhos 7 e 8); REF0/F0/CEL apontam para a referência, o primeiro quadro e o cel dele
FIRST = {1: 0, 2: 6, 3: 9, 4: 14, 5: 54, 6: 57, 7: 60, 8: 63}
# exposição das camadas fixas: quadro → desenho; desenho sem cel próprio na camada cai no cel 1
STATIC_EXPOSURE = {0: 1, 60: 7, 63: 8, 66: 1, 69: 7, 72: 8, 75: 1, 78: 7, 81: 8, 84: 1}
CEL = int(os.environ.get("D218_DES", "1"))
F0 = FIRST[CEL]
REF0 = rf"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_{F0 + 1:04d}.png"
IDS = os.path.join(os.path.dirname(PROJ), "d218_ids.json")
d = Drive(PROJ)
ids = json.load(open(IDS)) if os.path.exists(IDS) else {}


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        json.dump(ids, open(IDS, "w"), indent=1)
    return ids[key]


_FIELD = None


def W(pts):
    """Leva estimativas do desenho 1 para o desenho atual pelo campo de deslocamento medido (d218_campo.py) — média gaussiana
    dos pontos de controle (σ 60 px). Desenhos sem campo (1–6): identidade. As medições depois ajustam cada ponto."""
    global _FIELD
    path = os.path.join(os.path.dirname(__file__), f"d218_campo_{CEL}.json")
    if CEL == 1 or not os.path.exists(path):
        return [tuple(p) for p in pts]
    if _FIELD is None:
        _FIELD = json.load(open(path))
    out = []
    for x, y in pts:
        ws = sx = sy = 0.0
        for cx, cy, dx, dy, _ in _FIELD:
            w = 2.718281828 ** (-((x - cx) ** 2 + (y - cy) ** 2) / (2 * 60.0 ** 2)) + 1e-12
            ws += w; sx += w * dx; sy += w * dy
        nx, ny = x + sx / ws, y + sy / ws
        # pontos na borda do quadro (ou além) continuam nela: a forma não pode descolar da borda
        if x <= 0 or x >= 1919:
            nx = x
        if y <= 0 or y >= 1079:
            ny = y
        out.append((nx, ny))
    return out


_REFIMG = None


def edge_score(pts):
    """Contraste médio na referência entre 2 px de cada lado da borda fechada `pts` (borda sobre uma aresta real → alto).
    Leitura de pixels usada só para escolher entre estimativas do agente."""
    global _REFIMG
    if _REFIMG is None:
        from PIL import Image
        _REFIMG = Image.open(REF0).convert("RGB")
    dense = meas.densify([list(p) for p in pts], 4, closed=True)
    tot = n = 0
    for (x, y), (nx, ny) in zip(dense, meas._normals(dense, closed=True)):
        p1, p2 = (round(x + nx * 2), round(y + ny * 2)), (round(x - nx * 2), round(y - ny * 2))
        if all(0 <= q[0] < _REFIMG.width and 0 <= q[1] < _REFIMG.height for q in (p1, p2)):
            tot += sum((a - b) ** 2 for a, b in zip(_REFIMG.getpixel(p1), _REFIMG.getpixel(p2))) ** 0.5
            n += 1
    return tot / max(1, n)


def color_score(pts, color, tol):
    """Fração dos pontos (centro medido de uma linha) cuja cor na referência fica a menos de `tol` de `color`."""
    edge_score([(0, 0), (1, 0), (1, 1)])  # carrega a referência
    c = [int(color[i:i + 2], 16) for i in (1, 3, 5)]
    ok = n = 0
    for x, y in pts:
        px, py = round(x), round(y)
        if 0 <= px < _REFIMG.width and 0 <= py < _REFIMG.height:
            ok += sum((a - b) ** 2 for a, b in zip(_REFIMG.getpixel((px, py)), c)) ** 0.5 < tol
            n += 1
    return ok / max(1, n)


def candidates(poly, field_first=True):
    """Estimativas a testar no desenho atual: nos redesenhos com campo, a levada pelo campo e a do desenho 1."""
    a = [tuple(p) for p in poly]
    b = W(poly)
    return [a] if b == a else ([b, a] if field_first else [a, b])  # empate fica com a primeira


def ck(key):
    """Chave (em ids) do cel do desenho atual na camada fixa `key`; cria o cel e a trilha do timesheet se faltar."""
    k = f"{key}_c{CEL}"
    if k not in ids:
        node(k, str(CEL), {"type": "cel", "number": CEL}, ids[key])
    if CEL != 1:
        static_track(key)
    return k


def static_track(key):
    """Trilha da camada fixa: cada desenho com cel próprio nessa camada aparece nos quadros dele; o resto usa o cel 1."""
    layer = ids[key]
    cut = d.query({"kind": "node", "id": ids["cut"]})
    cells = {str(f): {"type": "cel", "number": k if f"{key}_c{k}" in ids else 1} for f, k in STATIC_EXPOSURE.items()}
    for t in cut["data"].get("xsheet", {}).get("tracks", []):
        if t.get("layer") == layer:
            if t.get("cells") == cells:
                return
            d.apply({"kind": "remove_track", "cut": ids["cut"], "track": t["id"]})
    d.apply({"kind": "add_track", "cut": ids["cut"], "track": {"id": "", "name": key.upper()[:6], "kind": "drawing", "layer": layer, "cells": cells}})


def setup():
    if not os.path.exists(os.path.join(PROJ, "project.json")):
        d.call("NEW_PROJECT", {"name": "Calibração D218", "path": PROJ, "settings": {"width": 1920, "height": 1080, "fps_num": 24000, "fps_den": 1001}})
    ep = node("ep", "EP01", {"type": "episode", "number": 1})
    sc = node("scene", "Cena chibi", {"type": "scene", "number": 1}, ep)
    cut = node("cut", "C218", {"type": "cut", "number": 218, "duration": 87}, sc)
    for key, name, kind in [("bg", "BG", "raster"), ("cor", "Cor", "fill"), ("realce", "Realce", "raster"), ("linha", "Linha", "vector"), ("fx", "Satsuei", "effect")]:
        node(key, name, {"type": "layer", "kind": kind}, cut)
    for key in ["bg", "cor", "realce", "linha"]:
        node(key + "_c1", "1", {"type": "cel", "number": 1}, ids[key])
    d.apply({"kind": "set_props", "id": ids["cor"], "data": {"line_refs": [ids["linha"]]}})
    d.apply({"kind": "set_props", "id": ids["realce"], "data": {"clip_to_below": True}})
    return cut


# ordem canônica das camadas do corte (de baixo para cima); só as que existem são movidas
ORDER = ["bg", "bg_dir", "bg_dir_luz", "cor", "sombra", "realce", "rim", "rim_r", "franja", "franja_luz", "pele", "pele_luz", "cor_rosto", "boca", "gota_r", "detalhe", "baixo", "linha_massa", "linha", "sobrancelha",
         "brilho", "gota", "gota_r2", "fx", "foco", "foco_tras", "foco_no", "foco_mascara", "foco_tras_mascara", "foco_no_mascara", "franja_mascara", "fecho"]


def reorder():
    present = [k for k in ORDER if k in ids]
    for i, k in enumerate(present):
        d.apply({"kind": "move_node", "id": ids[k], "parent": ids["cut"], "index": i})


def probe(points, radius=5):
    return [p for p in meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": points, "radius": radius}))]


def draw(curves):
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": F0, "layer": ids["linha"], "curves": curves})


def render(name, frame=0):
    out = rf"C:\AnimeIAI\sprints\registro\calibracao\D218\{name}.png"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    d.call("RENDER", {"cut": ids["cut"], "frame": frame, "out": out})
    return out


if __name__ == "__main__":
    setup()
    print(json.dumps(ids, indent=1))
