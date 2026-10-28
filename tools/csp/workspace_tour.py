"""Passeio visual pelas workspaces numa CÓPIA do projeto do usuário (calibracao/tmp/tour.aiai; o original não é tocado):
abre cada workspace pedida, espera, fotografa e guarda o estado da interface. Para a revisão da seção 5 (Edição,
Áudio, Roteiro).

Uso: python tools/csp/workspace_tour.py [--ws roteiro,edicao,audio] [--src producoes/...aiai] [--keep]
Grava calibracao/tmp/tour_<ws>.png e tour_<ws>.json.
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--src", default=os.path.join(ca.ROOT, "producoes", "micro-anime", "ultima-rosa.aiai"))
    ap.add_argument("--ws", default="roteiro,edicao,audio")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    dst = os.path.join(ca.TMP, "tour.aiai")
    if os.path.exists(dst):
        shutil.rmtree(dst)
    shutil.copytree(a.src, dst, ignore=shutil.ignore_patterns("*.lock", "lock"))
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--project", dst])
    try:
        ca.wait_ready(proc)
        ca.settle(20)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = dst
        for ws in a.ws.split(","):
            aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": ws})
            ca.settle(90)
            p = os.path.join(ca.TMP, f"tour_{ws}.png")
            aiai.call("SCREENSHOT", {"out": p})
            st = aiai.call("GET_UI_STATE")
            keep = {k: st.get(k) for k in ["workspace", "panels", "status"] if k in st}
            keep["widgets"] = sorted(st.get("widgets", {}).keys())
            with open(os.path.join(ca.TMP, f"tour_{ws}.json"), "w", encoding="utf-8") as f:
                json.dump(keep, f, ensure_ascii=False, indent=1)
            print(ws, p)
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
