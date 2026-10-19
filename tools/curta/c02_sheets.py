"""Model sheets de Hana e Ren com grade de cabeças e manequim de guia; renderiza com as guias.
As vistas do manequim são as mesmas que o rig desenha em c03 (frente 0°, 3/4 a 30°, costas 180°): antes o manequim
estava em [0, 45, 90] e a terceira coluna mostrava perfil embaixo de um desenho de costas."""
from curta import d, ids, save_ids, OUT
VIEWS = [0, 30, 180]
for key, heads in (("hana", 5.5), ("ren", 6.0)):
    k = "sheet_" + key
    if k in ids:
        ch = d.query({"kind": "node", "id": ids["char_" + key]})["data"].get("measurements", {})
        if ch.get("views") != VIEWS:  # folha antiga com outras vistas: refaz do zero
            d.apply({"kind": "delete_node", "id": ids[k]})
            for kk in [x for x in ids if x == k or x.startswith(k + "_") or x.startswith("sh_" + key + "_")]:
                del ids[kk]
            save_ids()
    if k not in ids:
        r = d.call("MODEL_SHEET", {"character": ids["char_" + key], "name": f"Model sheet", "heads_tall": heads, "figure_px": 800, "views": VIEWS})
        ids[k], ids[k + "_line"], ids[k + "_fill"] = r["cut"], r["line_layer"], r["fill_layer"]; save_ids()
    print(key, d.call("RENDER", {"cut": ids[k], "frame": 0, "include_guides": True, "camera": False, "background": "#ffffff", "out": f"{OUT}/sheet_{key}_guias.png"}).get("width"))
