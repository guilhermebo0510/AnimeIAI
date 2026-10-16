"""Corte D, quadro 0: profundidade de campo do satsuei — o alto (y < ~120) e a base do quadro estão desfocados na
referência. Camada de efeito `foco` (desfoque gaussiano) com máscara `foco_mascara` (degradê vertical, oculta).
uso: python d218_focus.py [raio] [y_topo_nitido] [y_base_nitido]"""
import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from d218 import reorder, d, ids, node, ck, CEL, F0, W


def run(radius=4.0, top=110, bottom=2000):
    m = node("foco_mascara", "Máscara do foco", {"type": "layer", "kind": "raster"}, ids["cut"])
    ck("foco_mascara")
    f = node("foco", "Profundidade de campo", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids[ck("foco_mascara")], "data": {"type": "cel", "number": CEL}})
    # o plano de foco é inclinado: o desfoque do alto desce até y≈190 na esquerda e só até y≈70 na direita (leitura das bordas
    # que sobram/faltam). Degradê ao longo da normal dessa reta: branco 80 px acima dela, preto sobre ela.
    return d.call("GRADIENT", {"cut": ids["cut"], "frame": F0, "layer": m, "kind": "linear", "from": [960, 50], "to": [965, 130],
                               "stops": [(0.0, "#ffffff"), (1.0, "#000000")]})


# cabelo de trás, sob o queixo à esquerda: camada de trás desfocada na referência (as linhas não dão borda nítida)
TRAS = [(450, 790), (545, 800), (600, 815), (650, 850), (700, 880), (760, 912), (840, 950), (930, 975), (942, 1082), (440, 1082)]


def tras(radius=12.0):
    m = node("foco_tras_mascara", "Máscara do foco de trás", {"type": "layer", "kind": "raster"}, ids["cut"])
    ck("foco_tras_mascara")
    f = node("foco_tras", "Desfoque do cabelo de trás", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids[ck("foco_tras_mascara")], "data": {"type": "cel", "number": CEL}})
    return d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F0, "layer": m, "shapes": [{"points": [list(p) for p in W(TRAS)], "geometry": "polyline"}],
                                 "color": {"type": "direct", "rgba": [1, 1, 1, 1]}})


def no_laco(radius=0.3):  # desfoque desligado: na maioria dos quadros o nó tem borda detectável
    """O nó do laço tem bordas macias na referência (fora de foco): desfoque só nele, por máscara."""
    m = node("foco_no_mascara", "Máscara do nó", {"type": "layer", "kind": "raster"}, ids["cut"])
    ck("foco_no_mascara")
    f = node("foco_no", "Desfoque do nó do laço", {"type": "layer", "kind": "effect"}, ids["cut"])
    d.apply({"kind": "set_props", "id": m, "data": {"visible": False}})
    d.apply({"kind": "set_props", "id": f, "data": {"mask": m, "params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})
    reorder()
    d.apply({"kind": "replace_data", "id": ids[ck("foco_no_mascara")], "data": {"type": "cel", "number": CEL}})
    return d.call("FILL_SHAPE", {"cut": ids["cut"], "frame": F0, "layer": m,
                                 "shapes": [{"points": [[1100, 986], [1140, 984], [1143, 1054], [1104, 1056]], "geometry": "polyline"}],
                                 "color": {"type": "direct", "rgba": [1, 1, 1, 1]}})


def satsuei(radius=1.8):
    """Suavização geral do satsuei (camada `fx`): 1,8 px escolhido por varredura (1,5 → SSIM 0,9626; 2,0 → F1 cai)."""
    return d.apply({"kind": "set_props", "id": ids["fx"], "data": {"params": {"effects": [{"effect": {"type": "gaussian_blur", "radius": radius}}]}}})


if __name__ == "__main__":
    satsuei()
    a = [float(x) for x in sys.argv[1:4]]
    print(run(*a).get("description"))
    print(tras().get("description"))
    print(no_laco().get("description"))
