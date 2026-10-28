"""Roteiro (seção 5) numa cópia do projeto do usuário: clica no texto da última fala da cena 1, Ctrl+Enter cria a
fala seguinte (do outro personagem da conversa) com o cursor nela, digita, e o painel Propriedades mostra o quadro de
personagens e os botões de Fountain. Foto em calibracao/tmp/script_flow.png.

Uso: python tools/csp/script_flow_test.py [--keep]
"""
import argparse
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(__file__))
import calib_app as ca  # noqa: E402
import aiai  # noqa: E402


def widgets():
    return aiai.call("GET_UI_STATE").get("widgets", {})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=os.path.join(ca.ROOT, "app", "target", "debug", "aiai-studio.exe"))
    ap.add_argument("--src", default=os.path.join(ca.ROOT, "producoes", "micro-anime", "ultima-rosa.aiai"))
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()
    dst = os.path.join(ca.TMP, "script_flow.aiai")
    if os.path.exists(dst):
        shutil.rmtree(dst)
    shutil.copytree(a.src, dst, ignore=shutil.ignore_patterns("*.lock", "lock"))
    proc = ca.launch_hidden([a.exe, "--bridge", "--no-session", "--no-splash", "--size", "1500x980", "--project", dst, "--workspace", "roteiro"])
    res = {}
    try:
        ca.wait_ready(proc)
        ca.settle(20)
        ca.click_widget_if("warn:ok")
        ca.PROJECT = dst
        ws = widgets()
        texts = sorted(k for k in ws if k.startswith("script:text:0:"))
        res["before"] = len(texts)
        last = texts[-1]
        r = ws[last]["rect_logical"]
        x, y = r[0] + r[2] - 20, r[1] + r[3] / 2
        aiai.call("INPUT", {"wait": True, "events": [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": "primary"},
                                                    {"kind": "mouse_up", "x": x, "y": y, "button": "primary"}, {"kind": "wait", "frames": 3},
                                                    {"kind": "key", "key": "Enter", "modifiers": ["ctrl"]}, {"kind": "wait", "frames": 4},
                                                    {"kind": "text", "text": "大丈夫、二人で渡そう。"}, {"kind": "wait", "frames": 3}]})
        ca.settle(8)
        # a pausa de 2 s na digitação grava sem tirar o foco (Esc descartaria: é o cancelar do campo)
        ca.settle(200)
        ws = widgets()
        texts2 = sorted(k for k in ws if k.startswith("script:text:0:"))
        res["after"] = len(texts2)
        doc = aiai.call("QUERY", {"query": {"kind": "documents"}}) if False else None
        p = os.path.join(ca.TMP, "script_flow.png")
        aiai.call("SCREENSHOT", {"out": p})
        res["shot"] = p
        res["widgets"] = [k for k in ws if k.startswith("script:fountain")]
        dump = json.dumps(aiai.call("SCRIPT", {"op": "list"}) if False else {}, ensure_ascii=False)
        res["checks"] = {"Ctrl+Enter cria o próximo elemento": res["after"] == res["before"] + 1, "botões Fountain": len(res["widgets"]) == 2}
        print(json.dumps(res, ensure_ascii=False, indent=1))
        if not all(res["checks"].values()):
            raise SystemExit("FALHOU")
        print("ok: roteiro")
    finally:
        if not a.keep:
            proc.kill()


if __name__ == "__main__":
    main()
