"""Calibração do corte D (#218, chibi). Projeto, camadas e desenho medido do quadro 0 (desenho 1)."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive
import meas

PROJ = r"C:\AnimeIAI\calibracao\projetos\d218.aiai"
REF0 = r"C:\AnimeIAI\calibracao\ref\cut_218\frames\f_0001.png"
IDS = os.path.join(os.path.dirname(PROJ), "d218_ids.json")
d = Drive(PROJ)
ids = json.load(open(IDS)) if os.path.exists(IDS) else {}


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        json.dump(ids, open(IDS, "w"), indent=1)
    return ids[key]


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
ORDER = ["bg", "bg_dir", "bg_dir_luz", "cor", "sombra", "realce", "franja", "franja_luz", "pele", "pele_luz", "cor_rosto", "detalhe", "baixo", "linha_massa", "linha",
         "brilho", "fx", "foco", "foco_tras", "foco_mascara", "foco_tras_mascara", "franja_mascara", "fecho"]


def reorder():
    present = [k for k in ORDER if k in ids]
    for i, k in enumerate(present):
        d.apply({"kind": "move_node", "id": ids[k], "parent": ids["cut"], "index": i})


def probe(points, radius=5):
    return [p for p in meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": points, "radius": radius}))]


def draw(curves):
    return d.call("DRAW_CURVES", {"cut": ids["cut"], "frame": 0, "layer": ids["linha"], "curves": curves})


def render(name, frame=0):
    out = rf"C:\AnimeIAI\sprints\registro\calibracao\D218\{name}.png"
    os.makedirs(os.path.dirname(out), exist_ok=True)
    d.call("RENDER", {"cut": ids["cut"], "frame": frame, "out": out})
    return out


if __name__ == "__main__":
    setup()
    print(json.dumps(ids, indent=1))
