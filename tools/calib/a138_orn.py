"""Corte A (#138): ornamentos dourados do papel de parede (damasco), camada BG por baixo do desfoque. Contornos DIGITADOS pelo
agente (leitura em recorte com grade, vértice a cada ~20 px; metade direita das peças simétricas e espelho no eixo do
motivo), refinados por MEASURE_EDGE (decisão 22) e pintados com FILL_SHAPE na cor medida (PROBE_COLOR na peça).
uso: python a138_orn.py [nomes...]"""
import json, os, sys
sys.path.insert(0, os.path.dirname(__file__))
from a138 import d, ids, F, REF
import meas

HERE = os.path.dirname(os.path.abspath(__file__))


def mirror(half, axis):
    """Peça simétrica: meia volta digitada (do topo no eixo até a base no eixo, pelo lado direito) + espelho."""
    back = [(2 * axis - x, y) for x, y in reversed(half[1:-1])]
    return list(half) + back


AX_B = 80.5  # eixo da flor em leque (motivo B, embaixo à esquerda)
FLOR_B = {
    "b_petala_topo": mirror([(80.5, 699), (91, 701), (98, 708), (100, 722), (99, 746), (96, 770), (91, 792), (86, 808), (80.5, 817)], AX_B),
    "b_oval": meas.circle(81, 860, 21, 35, n=24),
    "b_p1_d": [(101, 822), (104, 800), (111, 775), (125, 755), (142, 742), (158, 736), (167, 741), (168, 750), (162, 757),
               (147, 766), (132, 780), (118, 800), (108, 822)],
    "b_p2_d": [(104, 838), (118, 815), (138, 797), (160, 786), (180, 783), (196, 786), (204, 796), (199, 805), (182, 808),
               (162, 812), (143, 820), (126, 832), (113, 843)],
    "b_p3_d": [(110, 857), (130, 840), (155, 827), (180, 823), (200, 825), (214, 833), (220, 848), (215, 858), (198, 858),
               (175, 857), (152, 862), (132, 870), (117, 877)],
}
# metades esquerdas das pétalas laterais (espelho)
for k in ("b_p1", "b_p2", "b_p3"):
    FLOR_B[k + "_e"] = [(2 * AX_B - x, y) for x, y in reversed(FLOR_B[k + "_d"])]

PIECES = dict(FLOR_B)
OUT = os.path.join(HERE, "a138_orn_medido.json")


def measure(names):
    done = json.load(open(OUT)) if os.path.exists(OUT) else {}
    for n in names:
        pts = meas.densify([list(p) for p in PIECES[n]], 3.0, closed=True)
        pts = [p for p in pts if p[0] >= 1.0] or pts  # pontos fora do quadro não medem
        # vãos entre pétalas ~8 px: busca curta para não pular para a borda vizinha
        done[n] = meas.measure_edge(d, REF, pts, passes=((7.0, 0.03), (3.0, 0.03)))
        json.dump(done, open(OUT, "w"))
    return done


def paint(done, gold):
    shapes = [{"points": [[round(x, 2), round(y, 2)] for x, y in pts], "geometry": "polyline"} for pts in done.values()]
    return d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F, "layer": ids["bg"], "shapes": shapes,
                                 "color": {"type": "direct", "rgba": list(gold) + [1.0]}}).get("description")


def gold_color():
    pts = [[81, 860], [81, 720], [150, 760], [180, 800], [190, 840]]
    pr = meas.first_list(d.call("PROBE_COLOR", {"reference": REF, "points": pts, "radius": 3}))
    cs = sorted([p["rgb"] for p in pr], key=sum)
    return cs[len(cs) // 2]


if __name__ == "__main__":
    names = sys.argv[1:] or list(PIECES)
    done = measure(names)
    g = gold_color()
    print("ouro", g)
    print(paint(done, g))
