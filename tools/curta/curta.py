"""Micro anime 「最後の一輪」: projeto e ids. Tudo passa pelo aiai-mcp (mesmo Command Bus das ferramentas MCP)."""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "calib"))
from drive import Drive

ROOT = "C:/AnimeIAI/producoes/micro-anime"
PROJ = ROOT + "/ultima-rosa.aiai"
OUT = ROOT + "/saida"
IDS_F = ROOT + "/ids.json"
d = Drive(PROJ)
ids = json.load(open(IDS_F, encoding="utf-8")) if os.path.exists(IDS_F) else {}


def save_ids():
    json.dump(ids, open(IDS_F, "w", encoding="utf-8"), indent=1, ensure_ascii=False)


def node(key, name, data, parent=None):
    if key not in ids:
        ids[key] = d.create(name, data, parent)
        save_ids()
    return ids[key]


def rgba(h, a=1.0):
    return [int(h[i:i + 2], 16) / 255 for i in (1, 3, 5)] + [a]


def col(h, a=1.0):
    return {"type": "direct", "rgba": rgba(h, a)}
