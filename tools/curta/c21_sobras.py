"""Fecha as sobras do balde (frestas de poucos px entre linhas: cantos dos olhos, gola) em todos os cels expostos das
camadas de cor dos personagens: cada fresta recebe a cor pintada mais próxima (lida do render da própria camada)."""
import math
from PIL import Image
from curta import d, ids, OUT

layers = [(int(k[1]) - 1, k) for k in ids if k.endswith("_cor") and k[:2] in ("c2", "c3", "c4", "c5", "c6") and not k.endswith("_c1")]
total = 0
for GAP, ci, key in [(g, ci, key) for g in (4, 3) for ci, key in layers]:  # 4 = balde do curta, 3 = lint_cut
    cut, lay = ids["cuts"][ci], ids[key]
    xs = d.query({"kind": "node", "id": cut})["data"]["xsheet"]["tracks"]
    t = next((t for t in xs if t.get("layer") == lay), None)
    frames = {}
    for f, c in sorted(((int(f), c) for f, c in (t["cells"].items() if t else [("0", {"number": 1})])), key=lambda x: x[0]):
        frames.setdefault(c.get("number"), f)  # um quadro por cel
    for num, f in frames.items():
        r = d.call("LINT", {"cut": cut, "frame": f, "layer": lay, "gap": GAP, "min_area": 1})
        if not r["unpainted"]:
            continue
        path = f"{OUT}/sobras_tmp.png"
        d.call("RENDER", {"cut": cut, "frame": f, "camera": False, "layers": [lay], "out": path})
        img = Image.open(path).convert("RGBA")
        W, H = img.size
        px = img.load()
        seeds = []
        for u in r["unpainted"]:
            x0, y0 = u["point"]
            best = None
            for rad in range(2, 25):
                for dx in range(-rad, rad + 1):
                    for dy in (-rad, rad):
                        for x, y in ((x0 + dx, y0 + dy), (x0 + dy, y0 + dx)):
                            if 0 <= x < W and 0 <= y < H and px[x, y][3] > 250:
                                dd = math.hypot(x - x0, y - y0)
                                if best is None or dd < best[0]:
                                    best = (dd, px[x, y])
                if best:
                    break
            if best:
                c = best[1]
                seeds.append({"x": x0, "y": y0, "color": {"type": "direct", "rgba": [c[0] / 255, c[1] / 255, c[2] / 255, 1.0]}})
        if seeds:
            d.call("FILL", {"cut": cut, "frame": f, "layer": lay, "seeds": seeds, "gap": GAP})
            total += len(seeds)
            left = d.call("LINT", {"cut": cut, "frame": f, "layer": lay, "gap": GAP, "min_area": 1})["unpainted"]
            print(key, "cel", num, "sobras", len(seeds), "restam", len(left))
print("total de frestas preenchidas:", total)
for i, c in enumerate(ids["cuts"]):
    r = d.call("PRODUCTION", {"op": "lint_cut", "cut": c})
    print(f"C00{i + 1}", "limpo" if r["clean"] else "SUJO", r["unpainted_regions"], r["unpainted_area"])
