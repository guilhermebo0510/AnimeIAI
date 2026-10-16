"""Corte D, quadro 0: fundo. Degradê vertical medido na coluna esquerda (camada `bg`) e bloco direito com degradê da
coluna direita (`bg_dir` base retangular + `bg_dir_luz` recortada) — o fundo direito é mais claro no alto."""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
import meas
from d218 import reorder, d, ids, node, REF0


def column_stops(x, ys, x_low=None, y_low=1081):
    """Paradas de degradê medidas numa coluna; abaixo de `y_low` usa a coluna `x_low` (evita objeto desfocado do fundo)."""
    pts = [[x if (x_low is None or y < y_low) else x_low, y] for y in ys]
    r = meas.first_list(d.call("PROBE_COLOR", {"reference": REF0, "points": pts, "radius": 6}))
    st = [(round(y / 1080, 4), c["hex"]) for y, c in zip(ys, r)]
    return [(0.0, st[0][1])] + st + [(st[-1][0] + 0.001, st[-1][1])]


def run():
    base = node("bg_dir", "BG direita", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("bg_dir_c1", "1", {"type": "cel", "number": 1}, base)
    luz = node("bg_dir_luz", "BG direita degradê", {"type": "layer", "kind": "raster"}, ids["cut"])
    node("bg_dir_luz_c1", "1", {"type": "cel", "number": 1}, luz)
    d.apply({"kind": "set_props", "id": luz, "data": {"clip_to_below": True}})
    reorder()
    for k in ("bg_c1", "bg_dir_c1", "bg_dir_luz_c1"):
        d.apply({"kind": "replace_data", "id": ids[k], "data": {"type": "cel", "number": 1}})
    # transições do fundo são quase nítidas (y≈130 e y≈650): paradas densas perto delas
    ys = sorted(set(list(range(15, 1080, 30)) + list(range(110, 160, 5)) + list(range(625, 700, 5))))
    d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": ids["bg"], "kind": "linear", "from": [0, 0], "to": [0, 1080],
                        "stops": column_stops(60, ys, x_low=140, y_low=880)})
    d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": 0, "layer": base, "shapes": [{"points": [[1560, 0], [1920, 0], [1920, 720], [1560, 720]],
                                                                                    "geometry": "polyline"}],
                          "color": {"type": "direct", "rgba": [1, 1, 1, 1]}})
    ys_r = sorted(set(list(range(15, 700, 30)) + list(range(110, 160, 5)) + list(range(640, 700, 5))))
    return d.call("GRADIENT", {"cut": ids["cut"], "frame": 0, "layer": luz, "kind": "linear", "from": [0, 0], "to": [0, 1080],
                               "stops": column_stops(1885, ys_r)})


def ornamento():
    """Aro dourado desfocado no canto inferior esquerdo (móvel do cenário): traço grosso na `bg`, desfocado depois pelo
    efeito do fundo. Forma lida no recorte ampliado."""
    return d.call("PAINT", {"cut": ids["cut"], "frame": 0, "layer": ids["bg"], "preset": "Redondo macio", "preset_overrides": {"size": 16}, "color": {"type": "direct",
                            "rgba": [0x7a / 255, 0x55 / 255, 0x22 / 255, 1.0]},
                            "strokes": [{"points": [[8, 1080, 1], [10, 1010, 1], [22, 955, 1], [42, 925, 1], [60, 940, 1], [72, 985, 1], [78, 1040, 1],
                                                    [80, 1080, 1]], "smooth": True}]})


if __name__ == "__main__":
    print(run().get("description"))
    # ornamento(): o aro desfocado do canto ficou deslocado e piorou as bordas; fica de fora até ser relido
