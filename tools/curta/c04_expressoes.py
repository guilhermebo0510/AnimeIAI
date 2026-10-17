"""Folha de expressões e bocas (settei) de Hana e Ren: 4 expressões em cima, 6 bocas do lip-sync embaixo."""
from curta import d, ids, node, OUT, ensure_layers, paint
import rig

MOODS = [("neutral", "closed"), ("angry", "shout"), ("surprised", "small_o"), ("happy", "smile")]
MOUTHS = ["closed", "a", "i", "u", "e", "o"]
for who in ("hana", "ren"):
    cut = node("expr_" + who, "Expressões e bocas", {"type": "cut", "number": 1, "duration": 1, "canvas": [1920, 1080]}, ids["char_" + who])
    L = ensure_layers("ex_" + who, cut)
    for i, k in enumerate(["cor", "sombra", "olhos", "linha", "brilho"]):
        d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": i})
        cel = d.query({"kind": "node", "id": L[k]})["children"][0]
        d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    drawings = []
    H1 = 300
    for i, (mood, m) in enumerate(MOODS):
        h = rig.Head(240 + i * 480, 80, H1, 0.0)
        st, sd, sh = (rig.face_hana if who == "hana" else rig.face_ren)(h, mood, 1.0, m)
        drawings.append((who, st, sd, sh, 80, H1))
    H2 = 210
    for i, m in enumerate(MOUTHS):
        h = rig.Head(160 + i * 320, 470 + 120, H2, 0.0)
        st, sd, sh = (rig.face_hana if who == "hana" else rig.face_ren)(h, "neutral", 1.0, m)
        drawings.append((who, st, sd, sh, 590, H2))
    lint = paint(cut, 0, L, drawings)
    # rótulos
    lab = node("ex_" + who + "_txt", "rótulos", {"type": "layer", "kind": "raster"}, cut)
    node("ex_" + who + "_txt_c1", "1", {"type": "cel", "number": 1}, lab)
    d.apply({"kind": "replace_data", "id": ids["ex_" + who + "_txt_c1"], "data": {"type": "cel", "number": 1}})
    for i, (mood, _) in enumerate(MOODS):
        d.call("TEXT", {"cut": cut, "frame": 0, "fill_layer": lab, "text": {"neutral": "normal", "angry": "brava" if who == "hana" else "firme", "surprised": "surpresa", "happy": "feliz"}[mood],
                        "size": 34, "center": [240 + i * 480, 470], "color": {"type": "direct", "rgba": [0.2, 0.2, 0.25, 1]}, "font": "regular"})
    for i, m in enumerate(MOUTHS):
        d.call("TEXT", {"cut": cut, "frame": 0, "fill_layer": lab, "text": {"closed": "fechada"}.get(m, m.upper()), "size": 34,
                        "center": [160 + i * 320, 1050], "color": {"type": "direct", "rgba": [0.2, 0.2, 0.25, 1]}, "font": "regular"})
    print(who, "regiões sem cor:", len(lint["unpainted"]))
    d.call("RENDER", {"cut": cut, "frame": 0, "camera": False, "background": "#f6f3ee", "out": f"{OUT}/expressoes_{who}.png"})
