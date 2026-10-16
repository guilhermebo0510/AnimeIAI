"""Calibração do corte A (#138, menino loiro em close; fundo de damasco estático, personagem com leve movimento nos desenhos 0–12 e
boca nos 21–27). Projeto, camadas e utilidades."""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive

PROJ = r"C:\AnimeIAI\calibracao\projetos\a138.aiai"
IDS = os.path.join(os.path.dirname(PROJ), "a138_ids.json")
d = Drive(PROJ)
ids = json.load(open(IDS)) if os.path.exists(IDS) else {}
F = int(os.environ.get("A138_Q", "12"))
REF = rf"C:\AnimeIAI\calibracao\ref\cut_138\frames\f_{F + 1:04d}.png"


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        json.dump(ids, open(IDS, "w"), indent=1)
    return ids[key]


def setup():
    if not os.path.exists(os.path.join(PROJ, "project.json")):
        d.call("NEW_PROJECT", {"name": "Calibração A138", "path": PROJ, "settings": {"width": 1920, "height": 1080, "fps_num": 24000, "fps_den": 1001}})
    ep = node("ep", "EP01", {"type": "episode", "number": 1})
    sc = node("scene", "Cena do salão", {"type": "scene", "number": 1}, ep)
    cut = node("cut", "C138", {"type": "cut", "number": 138, "duration": 36}, sc)
    for key, name, kind in [("bg", "BG", "raster"), ("cor", "Cor", "fill"), ("linha", "Linha", "vector"), ("fx", "Satsuei", "effect")]:
        node(key, name, {"type": "layer", "kind": kind}, cut)
    for key in ["bg", "cor", "linha"]:
        node(key + "_c1", "1", {"type": "cel", "number": 1}, ids[key])
    d.apply({"kind": "set_props", "id": ids["cor"], "data": {"line_refs": [ids["linha"]]}})
    return cut


def compare(frame, tag):
    ref = rf"C:\AnimeIAI\calibracao\ref\cut_138\frames\f_{frame + 1:04d}.png"
    out = rf"C:\AnimeIAI\sprints\registro\calibracao\A138\tentativas\{tag}_q{frame:02d}"
    r = d.call("COMPARE", {"cut": ids["cut"], "frame": frame, "reference": ref, "noise_floor": 0.016, "out_dir": out})
    return r


if __name__ == "__main__":
    setup()
    r = compare(F, "vazio")
    rep = r.get("report", r)
    print(json.dumps({k: rep["thresholds"][k] for k in ("flip_mean_max", "ssim_min", "edge_f1_min")}, indent=1),
          rep["flip"]["mean"], rep["ssim"], rep["edges"]["f1"])
