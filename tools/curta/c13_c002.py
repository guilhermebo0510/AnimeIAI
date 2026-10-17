"""C002 (DET): duas mãos alcançam a última rosa e se tocam. A rosa em primeiro plano nítido sobre a roseira desfocada;
os braços (Hana pela esquerda, manga creme; Ren pela direita, manga azul dobrada) são desenhados uma vez e entram por
deslize de camada (hiki), se tocam no quadro ~32 e recuam um pouco."""
import math, os
from curta import d, ids, node, OUT, rgba, ensure_layers, paint
import rig

cut = ids["cuts"][1]
DUR = 72
DRAW = os.environ.get("SKIP_DRAW") != "1"
INK = rig.INK


def C(h, a=1.0): return {"type": "direct", "rgba": rgba(h, a)}


def layer(key, kind, index):
    lid = node(f"c2_{key}", key, {"type": "layer", "kind": kind}, cut)
    cel = node(f"c2_{key}_c1", "1", {"type": "cel", "number": 1}, lid)
    if DRAW:
        d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    d.apply({"kind": "move_node", "id": lid, "parent": cut, "index": index})
    return lid


# ---------- fundo desfocado e a rosa ----------
bg = layer("bg", "raster", 1)
rosa = layer("rosa", "raster", 2)
if DRAW:
    d.call("GRADIENT", {"cut": cut, "frame": 0, "layer": bg, "kind": "linear", "from": [0, 0], "to": [0, 1080],
                        "stops": [[0, "#e9a070ff"], [0.35, "#8a9a58ff"], [1, "#2f4a2cff"]]})
    for i, (cx, cy, R, col) in enumerate(((300, 700, 420, "#3c5a34"), (1500, 650, 460, "#46663a"), (900, 1000, 520, "#2f4a2c"))):
        d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": bg, "color": C(col), "shapes": [{"geometry": "spline", "points": [
            [cx + R * (1 + 0.12 * math.sin(6 * t / 28 * math.tau + i)) * math.cos(t / 28 * math.tau), cy + R * 0.55 * (1 + 0.12 * math.sin(4 * t / 28 * math.tau)) * math.sin(t / 28 * math.tau)] for t in range(28)]}]})
        d.call("PAINT", {"cut": cut, "frame": 0, "layer": bg, "preset": "Folhagem", "preset_overrides": {"size": 120}, "color": C("#7a9a50"),
                         "strokes": [{"points": [[cx - R * 0.8 + j * R * 0.4, cy - R * 0.3 + 30 * (j % 2), 1.0] for j in range(5)]}], "seed": 90 + i})
    d.call("EFFECTS", {"op": "set", "layer": bg, "effects": [{"effect": {"type": "gaussian_blur", "radius": 22}, "enabled": True}]})
    # rosa: haste, folhas, flor em camadas de pétalas
    RX, RY = 960, 330
    d.call("PAINT", {"cut": cut, "frame": 0, "layer": rosa, "preset": "Redondo duro", "preset_overrides": {"size": 16}, "color": C("#355a2c"),
                     "strokes": [{"points": [[RX + 20, 1090, 1], [RX + 6, 800, 1], [RX - 4, 560, 1], [RX, RY + 60, 1]]}], "seed": 1})
    for sx, y, ang in ((1, 720, -0.5), (-1, 860, 0.4)):
        L0 = [RX + 8 * sx, y]
        pts = [L0, [L0[0] + sx * 120, y - 70], [L0[0] + sx * 210, y - 60], [L0[0] + sx * 120, y - 10]]
        d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": rosa, "color": C("#4f7a3a"), "shapes": [{"geometry": "spline", "points": pts}]})
        d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": rosa, "color": C("#6f9a4a"), "shapes": [{"geometry": "spline", "points": [L0, [L0[0] + sx * 120, y - 68], [L0[0] + sx * 200, y - 58], [L0[0] + sx * 110, y - 40]]}]})
    # flor de perfil: pétalas de trás, abertura em espiral, taça da frente com borda recortada, sépalas
    def F(pts, col, a=1.0):
        d.call("FILL_SHAPE", {"cut": cut, "frame": 0, "layer": rosa, "color": C(col, a), "shapes": [{"geometry": "spline", "points": [[RX + x, RY + y] for x, y in pts]}]})
    F([(-118, -10), (-128, -70), (-90, -128), (0, -150), (92, -126), (126, -70), (116, -10), (60, 40), (0, 52), (-60, 40)], "#7a1424")
    F([(-82, -96), (-40, -122), (20, -124), (74, -102), (60, -78), (0, -70), (-60, -76)], "#a01e30")
    F([(-46, -100), (-10, -114), (34, -108), (40, -92), (0, -84), (-38, -88)], "#c02c3c")
    F([(-14, -100), (8, -106), (18, -96), (0, -90)], "#5a0c18")
    F([(-132, -46), (-92, -66), (-48, -48), (0, -66), (48, -48), (92, -66), (132, -46), (110, 20), (50, 66), (0, 76), (-50, 66), (-110, 20)], "#c42e3e")
    F([(-120, -40), (-92, -58), (-52, -42), (-30, 10), (-60, 50), (-100, 16)], "#e0525a")
    F([(-2, -62), (40, -46), (88, -58), (96, -20), (60, 30), (10, 50), (20, -10)], "#a8202f")
    for pts in ([(-40, -44), (-10, 20), (-30, 60)], [(46, -44), (30, 20), (40, 58)]):
        d.call("PAINT", {"cut": cut, "frame": 0, "layer": rosa, "preset": "Redondo duro", "preset_overrides": {"size": 5}, "color": C("#6a1020"),
                         "strokes": [{"points": [[RX + x, RY + y, 0.8] for x, y in pts]}], "seed": 3})
    for sx in (-1, 1):
        F([(sx * 10, 62), (sx * 70, 70), (sx * 96, 92), (sx * 40, 84), (0, 90)], "#3f6a32")
    F([(-60, -120), (-20, -134), (10, -130), (-30, -118)], "#ffc8a0", 0.45)


# ---------- braços ----------
def arm(who, wrist, side, ang, s):
    """Braço estendido com a mão apontando (indicador esticado), em px locais (mão para +x), espelhado por `side`."""
    ink = INK[who]
    ca, sa = math.cos(math.radians(ang)), math.sin(math.radians(ang))
    def P(x, y):  # gira (y para baixo; ângulo negativo aponta para cima) e espelha em x
        X = x * ca - y * sa
        Y = x * sa + y * ca
        return [wrist[0] + side * X * s, wrist[1] + Y * s]

    def curve(pts, w, closed=False, widths=None, geometry="spline"):
        c = {"points": [P(*q) for q in pts], "width": round(2.6 * w * s / 1.6, 2), "tip": "g_pen", "geometry": geometry, "color": {"type": "direct", "rgba": rig.hexrgba(ink)}, "closed": closed}
        if widths:
            c["widths"] = widths
        return c

    def seed(x, y, name):
        X, Y = P(x, y)
        return {"x": int(X), "y": int(Y), "name": name}

    st, sd = [], []
    # mão (contorno fechado): dorso, indicador esticado, dedos dobrados por baixo
    st.append(curve([(0, -36), (55, -44), (108, -36), (160, -30), (206, -25), (220, -17), (207, -8), (160, -10), (118, -8),
                     (128, 6), (122, 20), (126, 30), (112, 42), (70, 46), (20, 42), (0, 36)], 1.1, closed=True))
    st.append(curve([(112, 2), (124, 8)], 0.5, widths=[0.3, 1.0, 0.3]))                      # nós dos dedos dobrados
    st.append(curve([(112, 22), (122, 24)], 0.5, widths=[0.3, 1.0, 0.3]))
    st.append(curve([(36, 30), (80, 33), (108, 34), (116, 30), (104, 24), (64, 22)], 0.8, closed=True))  # polegar, dentro da mão
    st.append(curve([(150, -27), (152, -13)], 0.4, widths=[0.2, 1.0, 0.2]))                  # junta do indicador
    sd += [seed(60, -10, "pele"), seed(185, -17, "pele"), seed(90, 29, "pele"), seed(116, 14, "pele")]
    if who == "hana":
        # manga da blusa com punho
        st.append(curve([(-6, -48), (-60, -52), (-60, 52), (-6, 48)], 1.0, closed=True, geometry="polyline"))
        st.append(curve([(-60, -52), (-300, -60), (-560, -66), (-700, -70), (-700, 70), (-560, 66), (-300, 60), (-60, 52)], 1.0, closed=True))
        st.append(curve([(-300, -60), (-260, -20), (-330, 20)], 0.5, widths=[0.2, 1.0, 0.2]))  # dobra do tecido
        sd += [seed(-30, 0, "blusa"), seed(-400, 0, "blusa"), seed(-200, 30, "blusa")]
    else:
        # antebraço nu e manga azul dobrada
        st.append(curve([(0, -36), (-120, -44), (-260, -50)], 1.0))
        st.append(curve([(0, 36), (-120, 44), (-260, 50)], 1.0))
        st.append(curve([(-250, -64), (-330, -66), (-330, 66), (-250, 64)], 1.0, closed=True, geometry="polyline"))  # dobra
        st.append(curve([(-330, -66), (-480, -70), (-620, -73), (-700, -74), (-700, 74), (-620, 73), (-480, 70), (-330, 66)], 1.0, closed=True))
        st.append(curve([(-260, -50), (-250, -64)], 1.0, geometry="polyline"))
        st.append(curve([(-260, 50), (-250, 64)], 1.0, geometry="polyline"))
        sd += [seed(-130, 0, "pele"), seed(-290, 0, "camisa"), seed(-380, 0, "camisa")]
    # sombra de cel: faixa de baixo do braço e da mão
    sh_name = "pele_sombra"
    shapes = [(sh_name, [P(10, 26), P(70, 34), P(110, 40), P(70, 46), P(20, 42), P(0, 36)])]
    shapes.append(("blusa_sombra" if who == "hana" else "camisa_sombra", [P(-60, 30), P(-700, 40), P(-700, 70), P(-60, 52)] if who == "hana" else [P(-330, 40), P(-700, 46), P(-700, 74), P(-330, 66)]))
    return st, sd, shapes


S = 1.6
arms = {"hana": ((628, 596), 1, -12.0), "ren": ((1292, 600), -1, -12.0)}
Ls = {}
for i, (who, (wrist, side, ang)) in enumerate(arms.items()):
    L = ensure_layers(f"c2_{who}", cut)
    for j, k in enumerate(["cor", "sombra", "olhos", "linha", "brilho"]):
        d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": 3 + 5 * i + j})
    Ls[who] = L
    if DRAW:
        for k in L:
            cel = d.query({"kind": "node", "id": L[k]})["children"][0]
            d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
        st, sd, sh = arm(who, wrist, side, ang, S)
        lint = paint(cut, 0, L, [(who, st, sd, sh, -10000, 0)])
        print(who, "sem cor:", len(lint["unpainted"]), [u["point"] for u in lint["unpainted"]][:6])

# ---------- movimento: entram deslizando, tocam, recuam ----------
moves = {"hana": (-760, 50, 0, 30, -28), "ren": (760, 40, 4, 32, 28)}
for who, (dx, dy, f0, f1, back) in moves.items():
    for k, lid in Ls[who].items():
        d.call("LAYER_MOTION", {"op": "keys", "layer": lid, "property": "offset_x", "keys": [
            {"frame": f0, "value": dx, "ease": "ease_out"}, {"frame": f1, "value": 0, "ease": "hold"},
            {"frame": 38, "value": 0, "ease": "ease_out"}, {"frame": 48, "value": back, "ease": "linear"}]})
        d.call("LAYER_MOTION", {"op": "keys", "layer": lid, "property": "offset_y", "keys": [
            {"frame": f0, "value": dy, "ease": "ease_out"}, {"frame": f1, "value": 0, "ease": "linear"}]})
for f in (0, 14, 30, 34, 50, 71):
    print(d.call("RENDER", {"cut": cut, "frame": f, "out": f"{OUT}/c002_q{f}.png"})["path"])
