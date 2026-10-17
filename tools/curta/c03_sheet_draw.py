"""Desenha os model sheets (frente, 3/4, costas) de Hana e Ren com o rig, pinta com o settei e exporta a folha."""
import json
from curta import d, ids, OUT, ROOT, ensure_layers, paint
import rig

for who in ("hana", "ren"):
    cut = ids["sheet_" + who]
    ch = d.query({"kind": "node", "id": ids["char_" + who]})["data"]["measurements"]
    slot, top, H = ch["slot_w"], ch["top_px"], ch["head_px"]
    L = ensure_layers("sh_" + who, cut, {"cor": ids["sheet_" + who + "_fill"], "linha": ids["sheet_" + who + "_line"]})
    for i, k in enumerate(["sh_" + who + "_sombra", "sh_" + who + "_olhos", None, "sh_" + who + "_brilho"]):
        pass
    # ordem: guias, cor, sombra, olhos, linha, brilho
    tree = d.query({"kind": "node", "id": cut})["children"]
    order = [c for c in tree if c not in L.values()] + [L["cor"], L["sombra"], L["olhos"], L["linha"], L["brilho"]]
    for i, nid in enumerate(order):
        d.apply({"kind": "move_node", "id": nid, "parent": cut, "index": i})
    for k in ("cor", "sombra", "olhos", "linha", "brilho"):
        cel = d.query({"kind": "node", "id": L[k]})["children"][0]
        d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
    drawings = []
    for i, view in enumerate(("front", "threequarter", "back")):
        h = rig.Head(slot * (i + 0.5), top, H, 30.0 if view == "threequarter" else 0.0)
        st, sd, sh = rig.figure(h, who, view)
        drawings.append((who, st, sd, sh, top, H))
    lint = paint(cut, 0, L, drawings)
    print(who, "regiões sem cor:", len(lint["unpainted"]), [u["point"] for u in lint["unpainted"]][:8])
    d.call("RENDER", {"cut": cut, "frame": 0, "camera": False, "background": "#ffffff", "out": f"{OUT}/sheet_{who}.png"})
    d.call("RENDER", {"cut": cut, "frame": 0, "camera": False, "background": "#ffffff", "include_guides": True, "out": f"{OUT}/sheet_{who}_guias.png"})
