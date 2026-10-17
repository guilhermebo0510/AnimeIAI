"""C006 (MS, varanda): Hana vira o rosto para Ren (chaves A/B do rig + 6 intercalações pelo aiai_inbetween, cor por
aiai_propagate_fills; olhos, sombras e brilhos interpolados ponto a ponto nos mesmos tempos), sorri e diz
「じゃあ、一緒に渡そう。」 (lip-sync f6 sobre bocas felizes). Ren olha para ela, pisca e sorri no fim."""
import math, os
from curta import d, ids, node, OUT, ensure_layers, paint, PALETTE, rgba
import corte, rig

cut = ids["cuts"][5]
DUR = 108
START = 36
DRAW = os.environ.get("SKIP_DRAW") != "1"
ORDER = corte.LAYER_ORDER

# ---------- ordem das camadas: parede, Hana, Ren, varanda (na frente), guia ----------
Lh = ensure_layers("c6_hana", cut)
Lr = ensure_layers("c6_ren", cut)
seq = [ids["c6_bg_parede"]] + [Lh[k] for k in ORDER] + [Lr[k] for k in ORDER] + [ids["c6_bg_varanda"], ids["c6_guia3d"]]
for i, lid in enumerate(seq):
    d.apply({"kind": "move_node", "id": lid, "parent": cut, "index": i + 1})


def cels(L, nums):
    """Garante cels com esses números (zerados) em todas as camadas do personagem."""
    for k in ORDER:
        have = {}
        for c in d.query({"kind": "node", "id": L[k]})["children"]:
            have[d.query({"kind": "node", "id": c})["data"].get("number")] = c
        for n in nums:
            if n not in have:
                have[n] = d.create(str(n), {"type": "cel", "number": n}, L[k])
            d.apply({"kind": "replace_data", "id": have[n], "data": {"type": "cel", "number": n}})


def shape_layer(L, name):
    return L["brilho"] if name == "brilho" else (L["sombra"] if name.endswith("_sombra") or name.endswith("_realce") else L["olhos"])


H = 170
HANA = (700, 300)
REN = (1235, 300 - H // 2)
YAW_A, YAW_B = -4.0, 30.0
T = [0.5 - 0.5 * math.cos(math.pi * k / 7) for k in range(1, 7)]  # ease in-out das 6 intercalações
HAPPY = {m: 101 + i for i, m in enumerate(corte.MOUTHS)}            # bocas felizes (closed → sorriso)


def hana(yaw, mood="neutral", mouth="closed"):
    h = rig.Head(HANA[0], HANA[1], H, yaw)
    return rig.bust(h, "hana", mood, 1.0, mouth, bottom=(1078 - HANA[1]) / H)


if DRAW:
    # painel de madeira sob o corrimão (os personagens ficam atrás dele)
    d.call("PATTERN_FILL", {"cut": cut, "frame": 0, "layer": ids["c6_bg_varanda"], "texture": {"spec": {"type": "wood", "scale": 6, "rings": 0.2},
                            "dark": [0.36, 0.23, 0.17], "light": [0.52, 0.34, 0.24], "seed": 12, "contrast": 0.6},
                            "mapping": {"type": "flat"}, "region": {"type": "rect", "rect": [100, 994, 1720, 90]}})
    # ---------- Hana: chaves A (1) e B (8), bocas felizes 101..106 ----------
    nums = [1, 8] + list(HAPPY.values())
    cels(Lh, nums)
    corte.set_track(cut, Lh["linha"], "linha", "drawing", {i: n for i, n in enumerate(nums)})
    for k in ORDER:
        corte.set_track(cut, Lh[k], k, "drawing", {i: n for i, n in enumerate(nums)})
    drawings = [hana(YAW_A), hana(YAW_B)] + [hana(YAW_B, "happy", "smile" if m == "closed" else m) for m in corte.MOUTHS]
    for i, (st, sd, sh) in enumerate(drawings):
        lint = paint(cut, i, Lh, [("hana", st, sd, sh, HANA[1], H)])
        print(f"  Hana cel {nums[i]}: sem cor {len(lint['unpainted'])}")
    shapes_A, shapes_B = drawings[0][2], drawings[1][2]
    # ---------- intercalação (linha + cor) ----------
    for k in ("linha", "cor"):  # refazer do zero: some com as intercalações de uma execução anterior
        for c in d.query({"kind": "node", "id": Lh[k]})["children"]:
            if 2 <= (d.query({"kind": "node", "id": c})["data"].get("number") or 0) <= 7:
                d.apply({"kind": "delete_node", "id": c})
    r = d.call("INBETWEEN", {"cut": cut, "layer": Lh["linha"], "key_a": 1, "key_b": 8, "count": 6, "times": T, "first_number": 2,
                             "start_frame": 12, "hold": 2, "fill_layer": Lh["cor"]})
    print("intercalação:", {k: v for k, v in r.items() if k in ("description", "created")} if isinstance(r, dict) else r)
    # cor das intercalações: as sementes nomeadas do rig, interpoladas nos mesmos tempos (o PROPAGATE_FILLS casou o rosto
    # com uma mecha de cabelo em 2 de 6 quadros nesta virada; ver LACUNAS)
    seeds_A, seeds_B = drawings[0][1], drawings[1][1]
    assert [a["name"] for a in seeds_A] == [b["name"] for b in seeds_B]
    for j, t in enumerate(T):
        fs = [{"x": a["x"] + (b["x"] - a["x"]) * t, "y": a["y"] + (b["y"] - a["y"]) * t, "color": {"type": "direct", "rgba": rig.hexrgba(PALETTE["hana"][a["name"]])}}
              for a, b in zip(seeds_A, seeds_B) if a["name"] in PALETTE["hana"] and 0 <= a["y"] < 1080 and 0 <= b["y"] < 1080]
        d.call("FILL", {"cut": cut, "frame": 14 + 2 * j, "layer": Lh["cor"], "seeds": fs, "gap": 4})
        lint = d.call("LINT", {"cut": cut, "frame": 14 + 2 * j, "layer": Lh["cor"], "gap": 4, "min_area": 30})
        small = [u for u in lint["unpainted"] if u["area"] < 4000]  # mesma regra de sobras das chaves (curta.paint)
        if small:
            d.call("FILL", {"cut": cut, "frame": 14 + 2 * j, "layer": Lh["cor"], "gap": 4, "seeds": [{"x": u["point"][0], "y": u["point"][1],
                    "color": {"type": "direct", "rgba": rig.hexrgba(PALETTE["hana"]["cabelo" if u["point"][1] < HANA[1] + H else "blusa"])}} for u in small]})
            lint = d.call("LINT", {"cut": cut, "frame": 14 + 2 * j, "layer": Lh["cor"], "gap": 4, "min_area": 30})
        print(f"  intercalação {j + 2}: sem cor {len(lint['unpainted'])}", [u["point"] for u in lint["unpainted"]][:5])
    # alças do avental: na escala do plano as pontas mal cruzam a linha do ombro e, nas intercalações, a região da alça se
    # junta ao ombro; as alças entram como forma (entre as duas linhas da alça) na camada de detalhes, abaixo da linha
    def strap_polys(st):
        polys = []
        for i in range(len(st) - 1):
            a, b = st[i]["points"], st[i + 1]["points"]
            if len(a) == 3 and len(b) == 3 and abs(a[0][1] - b[0][1]) < 1 and 4 < abs(a[0][0] - b[0][0]) < 0.08 * H and a[0][1] > a[2][1] + 0.3 * H:
                polys.append(a + b[::-1])
        return polys[:2]
    straps_A, straps_B = strap_polys(drawings[0][0]), strap_polys(drawings[1][0])
    print("alças:", len(straps_A), len(straps_B))
    # camadas raster: cels 2..7 com as formas interpoladas
    for k in ("sombra", "olhos", "brilho"):
        have = {d.query({"kind": "node", "id": c})["data"].get("number"): c for c in d.query({"kind": "node", "id": Lh[k]})["children"]}
        for n in range(2, 8):
            if n not in have:
                d.create(str(n), {"type": "cel", "number": n}, Lh[k])
            else:
                d.apply({"kind": "replace_data", "id": have[n], "data": {"type": "cel", "number": n}})
    turn = {12: 1, 14: 2, 16: 3, 18: 4, 20: 5, 22: 6, 24: 7, 26: 8}
    for k in ("sombra", "olhos", "brilho"):
        corte.set_track(cut, Lh[k], k, "drawing", {0: 1, **turn})
    for j, t in enumerate(T):
        groups = {}
        for (na, pa), (nb, pb) in zip(shapes_A, shapes_B):
            pts = [[a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t] for a, b in zip(pa, pb)]
            groups.setdefault((shape_layer(Lh, na), na), []).append({"points": pts})
        for (lay, name), shp in groups.items():
            d.call("FILL_SHAPE", {"cut": cut, "frame": 14 + 2 * j, "layer": lay, "shapes": shp, "color": {"type": "direct", "rgba": rig.hexrgba(PALETTE["hana"][name])}})
        straps = [[[a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t] for a, b in zip(pa, pb)] for pa, pb in zip(straps_A, straps_B)]
        d.call("FILL_SHAPE", {"cut": cut, "frame": 14 + 2 * j, "layer": Lh["olhos"], "shapes": [{"points": q, "geometry": "spline"} for q in straps],
                              "color": {"type": "direct", "rgba": rig.hexrgba(PALETTE["hana"]["avental"])}})
    # ---------- Ren: aberto 1, meio 11, fechado 21, sorriso 31 ----------
    rn = [1, 11, 21, 31]
    cels(Lr, rn)
    for k in ORDER:
        corte.set_track(cut, Lr[k], k, "drawing", {i: n for i, n in enumerate(rn)})
    for i, (op, mood, m) in enumerate(((1.0, "neutral", "closed"), (0.45, "neutral", "closed"), (0.0, "neutral", "closed"), (1.0, "happy", "smile"))):
        h = rig.Head(REN[0], REN[1], H, -24.0)
        st, sd, sh = rig.bust(h, "ren", mood, op, m, bottom=(1078 - REN[1]) / H)
        lint = paint(cut, i, Lr, [("ren", st, sd, sh, REN[1], H)])
        print(f"  Ren cel {rn[i]}: sem cor {len(lint['unpainted'])}")

# ---------- fala e timesheet final ----------
pl = corte.place_dialogue(cut, "tts_f6", START, "char_hana")
r = d.call("LIPSYNC", {"cut": cut, "placement": pl, "mouth_layer": Lh["linha"], "mapping": HAPPY, "lead_frames": 1})
for t in d.query({"kind": "node", "id": cut})["data"]["xsheet"]["tracks"]:
    if t.get("kind") == "mouth":
        d.apply({"kind": "remove_track", "cut": cut, "track": t["id"]})
hana_cells = {0: 1, 12: 1, 14: 2, 16: 3, 18: 4, 20: 5, 22: 6, 24: 7, 26: 8, 32: 101}
hana_cells.update({int(f): int(n) for f, n in r["exposures"] if int(f) >= START})
last = max(int(f) for f, _ in r["exposures"])
hana_cells[min(DUR - 1, last + 1)] = 101
for k in ORDER:
    corte.set_track(cut, Lh[k], k, "drawing", hana_cells)
ren_cells = {0: 1, 6: 11, 7: 21, 8: 11, 9: 1, 60: 11, 61: 21, 62: 11, 63: 1, 92: 31}
for k in ORDER:
    corte.set_track(cut, Lr[k], k, "drawing", ren_cells)
print("Hana:", hana_cells)
for f in (0, 14, 18, 22, 26, 34, 50, 70, 100):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c006_q{f}.png"})["path"])
