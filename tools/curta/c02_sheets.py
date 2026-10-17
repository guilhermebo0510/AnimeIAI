"""Model sheets de Hana e Ren (frente, 3/4, perfil) com grade de cabeças e manequim de guia; renderiza com as guias."""
import sys
from curta import d, ids, save_ids, OUT
for key, heads in (("hana", 5.5), ("ren", 6.0)):
    k = "sheet_" + key
    if k not in ids:
        r = d.call("MODEL_SHEET", {"character": ids["char_" + key], "name": f"Model sheet", "heads_tall": heads, "figure_px": 800, "views": [0, 45, 90]})
        ids[k], ids[k + "_line"], ids[k + "_fill"] = r["cut"], r["line_layer"], r["fill_layer"]; save_ids()
    print(key, d.call("RENDER", {"cut": ids[k], "frame": 0, "include_guides": True, "camera": False, "background": "#ffffff", "out": f"{OUT}/sheet_{key}_guias.png"}).get("width"))
