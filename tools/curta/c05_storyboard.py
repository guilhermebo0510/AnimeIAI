"""Storyboard / e-conte do curta: 7 painéis desenhados (rascunho a traço) com plano, ângulo, ação, falas ligadas ao
roteiro, SE, segundos e movimento de câmera; folha de e-conte e animatic com legenda."""
import math
from curta import d, ids, save_ids, OUT, ROOT
import rig

SB_INK = "#2b2b33"


def ln(pts, w=1.6, closed=False, geometry="spline"):
    return {"points": [list(p) for p in pts], "width": w, "tip": "g_pen", "geometry": geometry, "closed": closed,
            "color": {"type": "direct", "rgba": rig.hexrgba(SB_INK)}}


def ellipse(cx, cy, rx, ry, n=10, w=1.2):
    return ln([(cx + rx * math.cos(t), cy + ry * math.sin(t)) for t in [k * math.tau / n for k in range(n)]], w, closed=True)


def rose(cx, cy, r=10):
    out = [ellipse(cx, cy, r, r * 0.85, 9, 1.4)]
    out.append(ln([(cx - r * 0.5, cy), (cx, cy - r * 0.45), (cx + r * 0.5, cy + r * 0.1)], 1.0))
    out.append(ln([(cx, cy + r * 0.85), (cx - 2, cy + r * 3.2)], 1.2))
    return out


def head_sketch(who, cx, top, H, yaw=0.0, mood="neutral", mouth="closed", body=True):
    h = rig.Head(cx, top, H, yaw)
    st, _, _ = rig.bust(h, who, mood, 1.0, mouth, bottom=2.3) if body else (rig.face_hana if who == "hana" else rig.face_ren)(h, mood, 1.0, mouth)
    for c in st:
        c["color"] = {"type": "direct", "rgba": rig.hexrgba(SB_INK)}
        c["width"] = max(0.9, c["width"] * 0.8)
    return st


def panel1():  # EST jardim
    s = [ln([(0, 150), (120, 140), (260, 146), (480, 136)], 1.4)]  # horizonte (morros)
    s += [ln([(330, 150), (330, 92), (430, 92), (430, 150)], 1.5, geometry="polyline"), ln([(320, 94), (380, 58), (440, 94)], 1.5, geometry="polyline")]
    s += [ln([(352, 150), (352, 118), (372, 118), (372, 150)], 1.2, geometry="polyline")]  # porta
    for x in range(10, 480, 40):
        s.append(ln([(x, 205), (x + 1, 170)], 1.4, geometry="polyline"))  # cerca
    s.append(ln([(0, 178), (480, 176)], 1.2))
    for cx in (60, 140, 220, 380, 450):
        s.append(ellipse(cx, 232, 38, 20, 10, 1.2))  # roseiras (sem flor)
    s.append(ellipse(300, 230, 40, 22, 10, 1.2))
    s += rose(302, 205, 8)  # a última rosa
    s.append(ln([(0, 262), (480, 258)], 1.0))
    return s


def panel2():  # duas mãos
    s = rose(240, 112, 16)
    s.append(ellipse(240, 200, 70, 30, 12, 1.0))  # roseira
    # mão da esquerda (Hana, manga creme) e da direita (Ren, manga azul dobrada)
    s += [ln([(0, 210), (150, 168), (205, 146)], 1.6), ln([(0, 250), (160, 196), (212, 166)], 1.6)]
    s += [ln([(205, 146), (226, 136), (238, 140), (232, 152), (220, 160), (212, 166)], 1.5)]
    s += [ln([(480, 205), (330, 172), (276, 150)], 1.6), ln([(480, 248), (322, 198), (268, 170)], 1.6)]
    s += [ln([(276, 150), (254, 138), (242, 142), (248, 155), (258, 163), (268, 170)], 1.5)]
    s.append(ln([(395, 186), (400, 222)], 1.2))  # dobra da manga
    for k in range(4):  # brilho do toque
        a = k * math.pi / 2 + 0.4
        s.append(ln([(240 + 14 * math.cos(a), 146 + 14 * math.sin(a)), (240 + 24 * math.cos(a), 146 + 24 * math.sin(a))], 1.0))
    return s


def panel3():
    return head_sketch("hana", 240, 30, 200, 8.0, "angry", "shout")


def panel4():
    return head_sketch("ren", 240, 40, 190, -8.0, "angry", "e")


def panel5():
    s = head_sketch("hana", 150, 60, 105, 10.0, "surprised", "small_o") + head_sketch("ren", 335, 55, 105, -10.0, "surprised", "small_o")
    for k in range(28):  # linhas de foco
        a = k * math.tau / 28
        r0, r1 = 300, 210
        s.append(ln([(240 + r0 * math.cos(a), 135 + r0 * 0.62 * math.sin(a)), (240 + r1 * math.cos(a), 135 + r1 * 0.62 * math.sin(a))], 1.0, geometry="polyline"))
    return s


def panel6():
    s = [ln([(0, 34), (480, 22)], 1.6), ln([(0, 46), (480, 34)], 1.2), ln([(52, 40), (52, 270)], 1.5, geometry="polyline"),
         ln([(430, 26), (430, 270)], 1.5, geometry="polyline"), ln([(0, 250), (480, 246)], 1.2)]
    s += head_sketch("hana", 175, 85, 92, 25.0, "happy", "smile") + head_sketch("ren", 320, 78, 96, -18.0, "neutral", "smile")
    s += rose(250, 225, 7)
    return s


def panel7():
    s = [ln([(0, 236), (140, 226), (300, 232), (480, 222)], 1.4), ln([(0, 254), (480, 250)], 1.0)]
    for i, (x, y) in enumerate([(80, 200), (130, 160), (190, 120), (240, 175), (270, 90), (320, 140), (360, 60), (400, 110), (210, 50), (440, 170)]):
        s.append(ellipse(x, y, 5, 3, 6, 1.0))
        s.append(ln([(x - 14, y + 10), (x - 6, y + 4)], 0.8))  # trilha do vento
    s.append(ellipse(250, 48, 30, 30, 12, 1.0))  # sol baixo/brilho
    return s


SC1, SC2 = None, None


def main():
    global SC1, SC2
    s = d.call("SCRIPT", {"op": "read", "doc": ids["script"]})["script"]
    SC1, SC2 = s["scenes"][0]["id"], s["scenes"][1]["id"]
    el = {e["text"][:6]: e["id"] for sc in s["scenes"] for e in sc["elements"]}
    f3, f4, f5, f6 = el["それ、私が先"], el["僕だって、ば"], el["…ばあちゃん"], el["じゃあ、一緒"]
    if "storyboard" not in ids:
        ids["storyboard"] = d.call("STORYBOARD", {"op": "create", "name": "E-conte", "title": "最後の一輪"})["storyboard"]
        save_ids()
    sb = ids["storyboard"]
    PANELS = [
        ("p1", panel1, {"shot": "establishing", "angle": "eye", "action": "Jardim ao pôr do sol; câmera desliza até a última rosa.", "seconds": 4.5, "scene": SC1,
                        "sfx": "vento leve, pássaros", "camera_move": {"kind": "pan", "from": {"center": [0.36, 0.5], "zoom": 1.3}, "to": {"center": [0.64, 0.55], "zoom": 1.3}}}),
        ("p2", panel2, {"shot": "detail", "angle": "eye", "action": "Duas mãos alcançam a mesma rosa e se tocam.", "seconds": 3.0, "scene": SC1, "sfx": "farfalhar; 'tic' do toque"}),
        ("p3", panel3, {"shot": "close", "angle": "low", "action": "Hana, brava.", "seconds": 3.0, "scene": SC1, "dialogue_refs": [f3], "dialogue_text": "それ、私が先に見つけたの！"}),
        ("p4", panel4, {"shot": "close", "angle": "eye", "action": "Ren, firme.", "seconds": 3.5, "scene": SC1, "dialogue_refs": [f4], "dialogue_text": "僕だって、ばあちゃんに渡したいんだ。"}),
        ("p5", panel5, {"shot": "medium", "angle": "eye", "action": "Choque cômico: os dois se olham, fundo de gag e linhas de foco.", "seconds": 2.5, "scene": SC1,
                        "dialogue_refs": [f5], "dialogue_text": "…ばあちゃん？", "sfx": "impacto cômico", "camera_move": {"kind": "shake"}}),
        ("p6", panel6, {"shot": "medium", "angle": "eye", "action": "Varanda: Hana vira o rosto para Ren e sorri.", "seconds": 4.5, "scene": SC2, "dialogue_refs": [f6], "dialogue_text": "じゃあ、一緒に渡そう。"}),
        ("p7", panel7, {"shot": "wide", "angle": "low", "action": "Vento; pétalas sobem ao céu; câmera inclina para cima.", "seconds": 5.5, "scene": SC2, "sfx": "rajada de vento, música sobe",
                        "camera_move": {"kind": "tilt", "from": {"center": [0.5, 0.62], "zoom": 1.25}, "to": {"center": [0.5, 0.40], "zoom": 1.25}}}),
    ]
    for key, fn, spec in PANELS:
        k = "panel_" + key
        if k not in ids:
            a = d.call("STORYBOARD", {"op": "panel_add", "storyboard": sb, "spec": spec})
            ids[k], ids[k + "_cut"], ids[k + "_layer"], ids[k + "_cel"] = a["panel"], a["drawing"]["cut"], a["drawing"]["layer"], a["drawing"]["cel"]
            save_ids()
        else:
            patch = {kk: v for kk, v in spec.items()}
            d.call("STORYBOARD", {"op": "panel_update", "storyboard": sb, "panel": ids[k], "patch": patch})
        d.apply({"kind": "replace_data", "id": ids[k + "_cel"], "data": {"type": "cel", "number": 1}})
        d.call("DRAW_CURVES", {"cut": ids[k + "_cut"], "frame": 0, "layer": ids[k + "_layer"], "curves": fn()})
    rd = d.call("STORYBOARD", {"op": "read", "storyboard": sb})
    print("painéis:", len(rd["doc"]["panels"]), "total", rd["total_seconds"], "s")
    print(d.call("STORYBOARD", {"op": "sheet", "storyboard": sb, "out": ROOT + "/econte.png"})["path"])
    a = d.call("ANIMATIC", {"storyboard": sb, "out": ROOT + "/animatic.mp4", "width": 960, "height": 540, "captions": True, "cut_numbers": True})
    print("animatic", a["frames"], "quadros", round(a["seconds"], 2), "s")


if __name__ == "__main__":
    main()
