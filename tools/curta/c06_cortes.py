"""Gera as cenas e os cortes de produção a partir do storyboard (um passo de undo), num episódio próprio do curta."""
from curta import d, ids, node, save_ids
ep = node("ep_curta", "EP01 — 最後の一輪", {"type": "episode", "number": 1})
d.apply({"kind": "set_props", "id": ids["ep"], "name": "Ensaios"})
if "cuts" not in ids:
    r = d.call("CUTS_FROM_STORYBOARD", {"storyboard": ids["storyboard"], "episode": ep, "scene_per_panel_group": True, "script": ids["script"]})
    ids["scenes"] = r["scenes"]
    ids["cuts"] = [c["cut"] for c in r["cuts"]]
    save_ids()
t = d.call("PRODUCTION", {"op": "cut_table"})
for row in t["cuts"]:
    if row["cut"] in ids["cuts"]:
        print(row["number"], row["name"], row["scene_name"], row["duration"], "quadros")
print("total", round(sum(r["duration"] for r in t["cuts"] if r["cut"] in ids["cuts"]) / (24000 / 1001), 2), "s")
