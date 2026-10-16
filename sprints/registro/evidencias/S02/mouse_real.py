#!/usr/bin/env python3
"""Roteiro de mouse/teclado REAIS (SendInput) no AnimeIAI Studio, confirmado por GET_UI_STATE / GET_STATUS.

Pre-requisitos: app aberto com a bridge (tools\\relanca.ps1) e `tools\\harness\\watch_input.ps1` rodando em paralelo
(o vigia diz se algum evento FISICO de mouse contaminou a corrida). As coordenadas dos alvos vem de
GET_UI_STATE.widgets[...].center_screen (pixels fisicos da tela; conferido contra GetClientRect/ClientToScreen).
"""
import json
import subprocess
import sys
from pathlib import Path

MCP = r"C:\AnimeIAI\app\target\debug\aiai-mcp.exe"
INPUT = r"C:\AnimeIAI\tools\harness\input.ps1"
EVID = Path(r"C:\AnimeIAI\sprints\registro\evidencias\S02")
LOG = []


def call(action, params=None):
    p = subprocess.run([MCP, "call", action, json.dumps(params or {})], capture_output=True, text=True, encoding="utf-8")
    if p.returncode != 0:
        raise SystemExit(f"{action}: {p.stderr}")
    return json.loads(p.stdout)


def ui(ops):
    p = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", INPUT, "-S", ops], capture_output=True, text=True, encoding="utf-8")
    LOG.append(f"- `input.ps1 -S \"{ops}\"` -> exit {p.returncode}: {(p.stdout or '').strip().splitlines()[-1] if p.stdout.strip() else ''} {p.stderr.strip()}")
    if p.returncode != 0:
        raise SystemExit(f"input.ps1 falhou ({p.returncode}): {p.stdout}\n{p.stderr}")


def center(name):
    w = call("GET_UI_STATE")["widgets"][name]["center_screen"]
    return round(w[0]), round(w[1])


def check(cond, msg):
    LOG.append(f"- {'OK ' if cond else 'FALHOU '} {msg}")
    if not cond:
        raise SystemExit(f"FALHOU: {msg}")


def main():
    ui("foco")
    s0 = call("GET_UI_STATE")
    LOG.append(f"Estado inicial: workspace={s0['workspace']}, janela={s0['window']['size_physical']}, origem cliente={s0['window']['client_origin_screen_physical']}, titulo={s0['window']['title']!r}")

    # 1) abas de workspace por clique real
    for ws in ("cenario", "animacao", "roteiro", "pintura"):
        x, y = center(f"ws_tab:{ws}")
        ui(f"foco; click {x} {y}; wait 350")
        got = call("GET_UI_STATE")["workspace"]
        check(got == ws, f"clique real na aba '{ws}' em ({x},{y}) -> workspace '{got}'")
        if ws == "cenario":
            ui(f"cap {EVID / 'mouse-real-2-cenario.png'}")

    # 2) menu Arquivo por clique real: abre, lista os itens, captura, fecha com Esc
    x, y = center("menu:file")
    ui(f"foco; click {x} {y}; wait 400")
    st = call("GET_UI_STATE")
    items = [k for k in st["widgets"] if k.startswith("menu_item:file.")]
    check({"menu_item:file.new", "menu_item:file.open", "menu_item:file.save", "menu_item:file.save_as"} <= set(items), f"menu Arquivo aberto por clique real; itens: {sorted(items)}")
    ui(f"cap {EVID / 'mouse-real-3-menu-arquivo.png'}")
    ui("key esc; wait 300")
    st = call("GET_UI_STATE")
    check(not [k for k in st["widgets"] if k.startswith("menu_item:file.")], "Esc fecha o menu Arquivo")

    # 3) Editar > Desfazer por clique real; o render prova que desfez
    before = call("GET_STATUS")
    check(before["can_undo"], "ha o que desfazer")
    x, y = center("menu:edit")
    ui(f"foco; click {x} {y}; wait 400")
    x, y = center("menu_item:edit.undo")
    ui(f"click {x} {y}; wait 600")
    after = call("GET_STATUS")
    check(after["can_redo"] and after["undo_steps"] == before["undo_steps"] - 1, f"Editar > Desfazer por clique real: undo_steps {before['undo_steps']} -> {after['undo_steps']}, can_redo={after['can_redo']}")
    ui(f"cap {EVID / 'mouse-real-4-apos-desfazer.png'}")

    # 4) atalhos de teclado reais (SendInput): Ctrl+Shift+Z refaz, Ctrl+Z desfaz, Ctrl+Y refaz
    ui("foco; key ctrl+shift+z; wait 400")
    s = call("GET_STATUS")
    check(s["undo_steps"] == before["undo_steps"] and not s["can_redo"], f"Ctrl+Shift+Z real refez (undo_steps={s['undo_steps']})")
    ui("key ctrl+z; wait 400")
    s = call("GET_STATUS")
    check(s["undo_steps"] == before["undo_steps"] - 1, "Ctrl+Z real desfez")
    ui("key ctrl+y; wait 400")
    s = call("GET_STATUS")
    check(s["undo_steps"] == before["undo_steps"], "Ctrl+Y real refez")
    ui(f"cap {EVID / 'mouse-real-5-final.png'}")

    # 5) ferramenta na prateleira por clique real
    x, y = center("tool:pen")
    ui(f"foco; click {x} {y}; wait 300")
    check(call("GET_UI_STATE")["active_tool"] == "pen", "clique real na ferramenta Caneta vetorial")

    (EVID / "mouse-real.log.md").write_text("# S02 - roteiro de mouse/teclado reais (SendInput)\n\nGerado por `mouse_real.py`; vigia: `mouse-real-watch.json`.\n\n" + "\n".join(LOG) + "\n", encoding="utf-8")
    print("\n".join(LOG))


if __name__ == "__main__":
    main()
