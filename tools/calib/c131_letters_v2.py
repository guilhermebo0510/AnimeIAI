"""Corte C — letreiro ゴン, passada 2: o contorno estimado (passada 1) é densificado e cada ponto é MEDIDO na
referência (MEASURE_EDGE, decisão 22); a forma é redesenhada com FILL_SHAPE a partir dos pontos medidos."""
import json, os, sys, math
sys.path.insert(0, os.path.dirname(__file__))
from drive import Drive
from c131_go_v1 import GO
from c131_n_v1 import DASH, HOOK

PROJ = r"C:\AnimeIAI\calibracao\projetos\c131.aiai"
REF = r"C:\AnimeIAI\calibracao\ref\cut_131\frames\f_0016.png"
ids = json.load(open(os.path.join(PROJ, "..", "c131_ids.json")))
d = Drive(PROJ)


def densify(poly, step=6.0):
    out = []
    n = len(poly)
    for i in range(n):
        a, b = poly[i], poly[(i + 1) % n]
        L = math.hypot(b[0] - a[0], b[1] - a[1])
        k = max(1, int(L // step))
        for j in range(k):
            t = j / k
            out.append([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    return out


def measured(poly):
    pts = densify(poly)
    r = d.call("MEASURE_EDGE", {"reference": REF, "points": pts, "search": 10.0, "min_strength": 0.08})
    res = r["results"] if isinstance(r, dict) and "results" in r else r
    edges, moved, miss = [], [], 0
    for m in res:
        if m["strength"] > 0:
            edges.append(m["edge"]); moved.append(m["moved"])
        else:
            edges.append(m["given"]); miss += 1
    print(f"  {len(pts)} pontos, sem borda {miss}, deslocamento médio {sum(moved)/max(1,len(moved)):.2f} px, máx {max(moved or [0]):.1f}")
    return edges


if __name__ == "__main__":
    # limpa o cel do letreiro (passada 1) e redesenha
    d.apply({"kind": "replace_data", "id": ids["letreiro_c1"], "data": {"type": "cel", "number": 1}})
    shapes = []
    for name, poly in [("ゴ", GO), ("ン traço", DASH), ("ン gancho", HOOK)]:
        print(name)
        shapes.append({"points": measured(poly), "geometry": "polyline"})
    probe = d.call("PROBE_COLOR", {"reference": REF, "points": [[740, 600], [800, 650], [1300, 440], [1300, 750]], "radius": 6})
    res = probe["results"] if isinstance(probe, dict) and "results" in probe else probe
    print("cores:", [p["hex"] for p in res])
    rgb = [sum(p["rgb"][k] for p in res) / len(res) for k in range(3)]
    color = {"type": "direct", "rgba": rgb + [1.0]}
    print(d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 15, "layer": ids["letreiro"], "shapes": shapes, "color": color}).get("description"))
    json.dump(shapes, open(os.path.join(PROJ, "..", "c131_letters_shapes.json"), "w"))
