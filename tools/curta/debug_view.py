"""Depuração: desenha uma vista da figura sozinha no corte de ensaio. uso: python debug_view.py ren front"""
import sys
from curta import d, ids, node, OUT, ensure_layers, paint
import rig
who, view = sys.argv[1], sys.argv[2]
cut = ids["cut_ensaio"]
L = ensure_layers("dbg", cut)
tree = d.query({"kind": "node", "id": cut})["children"]
for nid in tree:
    if nid not in L.values():
        d.apply({"kind": "set_props", "id": nid, "data": {"visible": False}})
for i, k in enumerate(["cor", "sombra", "olhos", "linha", "brilho"]):
    d.apply({"kind": "move_node", "id": L[k], "parent": cut, "index": len(tree) - 5 + i})
    cel = d.query({"kind": "node", "id": L[k]})["children"][0]
    d.apply({"kind": "replace_data", "id": cel, "data": {"type": "cel", "number": 1}})
H = 160
h = rig.Head(960, 60, H, 30.0 if view == "threequarter" else 0.0)
st, sd, sh = rig.figure(h, who, view)
pal_bad = [s for s in sd if s["name"] == "pele"]
print("sementes de pele:", [(s["x"], s["y"]) for s in pal_bad])
lint = paint(cut, 0, L, [(who, st, sd, sh, 60, H)])
print("sem cor:", len(lint["unpainted"]))
print(d.call("RENDER", {"cut": cut, "frame": 0, "background": "#00ff00", "out": f"{OUT}/dbg_{who}_{view}.png"})["path"])
