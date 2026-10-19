#!/usr/bin/env python3
"""Roteiro de mouse e teclado REAIS (SendInput) pela interface do AnimeIAI Studio (S20 P6e).

Percorre os 13 workspaces clicando nas abas, troca as ferramentas pela barra e pelos atalhos de teclado, desenha um
traço no canvas, desfaz com Ctrl+Z, refaz com Ctrl+Y, faz pan com espaço+arrasto e com a mão, zoom com a roda, avança a
linha do tempo pelo botão e pela seta, abre e fecha os menus. Cada passo confere o efeito lendo o estado de volta pela
bridge (GET_UI_STATE / SCREENSHOT do canvas), nunca pelo retorno do clique; cada workspace tem uma captura de tela real
(a janela inteira, pelo DWM). Um vigia WH_MOUSE_LL roda junto (lição 21): evento físico do usuário invalida a corrida.

Uso (app aberto com --bridge numa CÓPIA do projeto — lição 61; o roteiro desenha e desfaz no canvas):

    python tools/ui/mouse_roteiro.py C:\\AnimeIAI\\sprints\\registro\\evidencias\\S20\\mouse

Saída: relatorio.json + ws_<workspace>.png + passos de canvas; código 0 se tudo passou, 1 se algum passo falhou,
3 se a corrida foi contaminada por mouse físico.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import aiai  # noqa: E402

STALLS: list[str] = []
POPUP: dict = {}
PS = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]
INPUT = str(ROOT / "tools" / "harness" / "input.ps1")
WATCH = str(ROOT / "tools" / "harness" / "watch_input.ps1")

# atalhos do editor (app/tools.rs: shortcuts)
SHORTCUTS = {"v": "move", "b": "brush", "p": "pen", "e": "eraser", "g": "fill", "l": "lasso", "i": "eyedropper", "h": "hand", "z": "zoom"}


def ps(script: str) -> str:
    r = subprocess.run(PS + [INPUT, "-S", script], capture_output=True, text=True, encoding="utf-8", errors="replace")
    if r.returncode != 0:
        raise RuntimeError(f"input.ps1 falhou ({r.returncode}): {r.stderr.strip()} | {script}")
    return r.stdout


def settle(seconds: float = 0.5) -> None:
    aiai.call("INPUT", {"events": [{"kind": "wait", "frames": 3}], "wait": True})
    time.sleep(seconds)


def st() -> dict:
    try:
        return aiai.call("GET_UI_STATE")
    except aiai.BridgeError as e:
        if e.code != -32002:
            raise
        # quadro atrasado (paginação com 8 GB de RAM): registra e tenta mais uma vez
        STALLS.append(time.strftime("%H:%M:%S"))
        time.sleep(5)
        return aiai.call("GET_UI_STATE")


def center(state: dict, widget: str) -> tuple[int, int]:
    w = state["widgets"].get(widget)
    if not w:
        raise KeyError(f"widget '{widget}' não está na tela")
    x, y = w["center_screen"]
    return int(round(x)), int(round(y))


def canvas_px(path: Path) -> np.ndarray:
    # cursor fora do canvas: o anel do pincel e a leitura x/y do rodapé não entram na comparação
    x, y = center(st(), "statusbar")
    ps(f"foco; move {x} {y}")
    settle(0.3)
    aiai.screenshot(path, {"panel": "canvas"})
    return np.asarray(Image.open(path).convert("RGB")).astype(np.int16)


def diff(a: np.ndarray, b: np.ndarray) -> int:
    """Pixels que mudaram mais de 24 níveis em algum canal (mesmo tamanho exigido)."""
    if a.shape != b.shape:
        return -1
    # o rodapé do canvas (zoom, ângulo, latência do último traço) muda a cada gesto: fora da conta
    return int((np.abs(a[:-40] - b[:-40]).max(axis=2) > 24).sum())


class Run:
    def __init__(self, out: Path):
        self.out = out
        self.steps: list[dict] = []

    def check(self, name: str, ok: bool, **info) -> bool:
        self.steps.append({"step": name, "ok": bool(ok), **info})
        print(f"[{'ok' if ok else 'FALHA'}] {name} {json.dumps(info, ensure_ascii=False)}", flush=True)
        return ok


def workspaces(run: Run) -> None:
    s = st()
    for ws in s["workspaces"]:
        try:
            x, y = center(st(), f"ws_tab:{ws}")
            ps(f"foco; click {x} {y}")
            settle(1.2)
            s2 = st()
        except Exception as e:
            run.check(f"aba {ws}", False, erro=str(e))
            continue
        png = run.out / f"ws_{ws}.png"
        ps(f"cap {png}")
        panels = [p["name"] for p in s2["panels"] if p.get("visible", True)]
        run.check(f"aba {ws}", s2["workspace"] == ws, ativo=s2["workspace"], paineis=panels, captura=png.name)


def tools_and_shortcuts(run: Run) -> None:
    x, y = center(st(), "ws_tab:animacao")
    ps(f"foco; click {x} {y}")
    settle(1.0)
    s = st()
    for t in s["tools"]:
        x, y = center(st(), f"tool:{t}")
        ps(f"foco; click {x} {y}")
        settle(0.2)
        run.check(f"botão da ferramenta {t}", st()["active_tool"] == t, ativa=st()["active_tool"])
    # o teclado só vale com o ponteiro sobre o canvas e sem campo de texto em foco
    cx, cy = center(st(), "canvas:area")
    ps(f"foco; move {cx} {cy}")
    for k, t in SHORTCUTS.items():
        ps(f"foco; key {k}")
        settle(0.2)
        a = st()["active_tool"]
        run.check(f"atalho {k.upper()} -> {t}", a == t, ativa=a)


def canvas_steps(run: Run) -> None:
    s = st()
    ed = s["editor"]
    # camada vetorial "linha" do corte aberto
    idx = next((i for i, l in enumerate(ed["layers"]) if l["kind"] == "vector" and not l["locked"]), None)
    if idx is None:
        run.check("camada vetorial no corte", False)
        return
    x, y = center(s, f"layers:row:{idx}")
    ps(f"foco; click {x} {y}")
    settle(0.3)
    run.check("clique na linha da camada seleciona", st()["editor"]["layer"] == ed["layers"][idx]["id"], camada=ed["layers"][idx]["name"])
    x, y = center(st(), "tool:pen")
    ps(f"foco; click {x} {y}")
    settle(0.2)

    before = canvas_px(run.out / "canvas_0_antes.png")
    rev0 = st()["project"]["revision"]
    ar = st()["widgets"]["canvas:area"]["rect_screen"]
    x0, y0 = int(ar[0] + ar[2] * 0.30), int(ar[1] + ar[3] * 0.25)
    x1, y1 = int(ar[0] + ar[2] * 0.70), int(ar[1] + ar[3] * 0.70)
    ps(f"foco; drag {x0} {y0} {x1} {y1} 40")
    settle(0.8)
    drawn = canvas_px(run.out / "canvas_1_traco.png")
    rev1 = st()["project"]["revision"]
    d1 = diff(before, drawn)
    run.check("arrasto da caneta desenha no canvas", rev1 > rev0 and d1 > 200, revisao=[rev0, rev1], pixels_mudados=d1)

    ps("foco; key ctrl+z")
    settle(0.8)
    undone = canvas_px(run.out / "canvas_2_desfeito.png")
    du = diff(before, undone)
    run.check("Ctrl+Z desfaz o traço", 0 <= du <= 20, pixels_diferentes_do_inicio=du)

    ps("foco; key ctrl+y")
    settle(0.8)
    redone = canvas_px(run.out / "canvas_3_refeito.png")
    dr = diff(drawn, redone)
    run.check("Ctrl+Y refaz o traço", 0 <= dr <= 20, pixels_diferentes_do_traco=dr)
    ps("foco; key ctrl+z")
    settle(0.5)

    # pan: espaço + arrasto e ferramenta mão
    cx, cy = int(ar[0] + ar[2] / 2), int(ar[1] + ar[3] / 2)
    o0 = st()["editor"]["canvas"]["origin_logical"]
    ps(f"foco; move {cx} {cy}; keydown space; drag {cx} {cy} {cx + 120} {cy + 60} 20; keyup space")
    settle(0.4)
    o1 = st()["editor"]["canvas"]["origin_logical"]
    run.check("espaço + arrasto faz pan", abs(o1[0] - o0[0]) > 40 and abs(o1[1] - o0[1]) > 20, origem=[o0, o1])
    x, y = center(st(), "tool:hand")
    ps(f"foco; click {x} {y}; drag {cx} {cy} {cx - 120} {cy - 60} 20")
    settle(0.4)
    o2 = st()["editor"]["canvas"]["origin_logical"]
    run.check("ferramenta mão faz pan", abs(o2[0] - o1[0]) > 40, origem=[o1, o2])

    x, y = center(st(), "canvas:fit")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    z0 = st()["editor"]["canvas"]["zoom"]
    ps(f"foco; move {cx} {cy}; wheel 3")
    settle(0.4)
    z1 = st()["editor"]["canvas"]["zoom"]
    run.check("roda do mouse aproxima", z1 > z0 * 1.05, zoom=[z0, z1])
    x, y = center(st(), "canvas:fit")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    z2 = st()["editor"]["canvas"]["zoom"]
    run.check("botão enquadrar volta o zoom", abs(z2 - z0) < 0.02, zoom=[z1, z2])
    ps(f"cap {run.out / 'canvas_4_enquadrado.png'}")


def timeline_steps(run: Run) -> None:
    # o corte aberto pode ter 1 quadro (ensaio): abrir o C001 pela árvore do Projeto
    cut0 = st()["editor"]["cut"]
    x, y = center(st(), "project:node:C001")
    ps(f"foco; click {x} {y}")
    settle(0.6)
    if st()["editor"]["cut"] == cut0:
        ps(f"foco; dclick {x} {y}")
        settle(0.8)
    run.check("clique no C001 da árvore abre o corte", st()["editor"]["cut"] != cut0, corte=[cut0, st()["editor"]["cut"]])
    f0 = st()["editor"]["frame"]
    x, y = center(st(), "timeline:step_forward")
    ps(f"foco; click {x} {y}")
    settle(0.3)
    f1 = st()["editor"]["frame"]
    run.check("botão avançar quadro", f1 == f0 + 1, quadro=[f0, f1])
    cx, cy = center(st(), "canvas:area")
    ps(f"foco; move {cx} {cy}; key right; key right")
    settle(0.3)
    f2 = st()["editor"]["frame"]
    run.check("seta para a direita avança", f2 == f1 + 2, quadro=[f1, f2])
    ps("foco; key home")
    settle(0.3)
    f3 = st()["editor"]["frame"]
    run.check("Home volta ao início", f3 == 0, quadro=[f2, f3])


def menus(run: Run) -> None:
    for m in ["file", "edit", "view", "window", "help"]:
        base, opened, closed = (run.out / f"menu_{m}_{k}.png" for k in ("antes", "aberto", "esc"))
        ps(f"cap {base}")
        x, y = center(st(), f"menu:{m}")
        ps(f"foco; click {x} {y}")
        settle(0.4)
        ps(f"cap {opened}")
        POPUP["aberto"] = st().get("popup_open")
        ps("foco; key esc")
        settle(0.4)
        ps(f"cap {closed}")
        POPUP["fechado"] = st().get("popup_open")
        a, b, c = (np.asarray(Image.open(f).convert("L")).astype(np.int16) for f in (base, opened, closed))
        d_open = int((np.abs(b - a) > 24).sum()) if a.shape == b.shape else -1
        d_left = int((np.abs(c - a) > 24).sum()) if a.shape == c.shape else -1
        # o veredito vem do estado (popup aberto no egui); os pixels ficam como registro (o canvas muda sozinho ao carregar)
        run.check(f"menu {m} abre", d_open > 2000 and POPUP.get("aberto") is True, pixels_do_menu=d_open, popup=POPUP.get("aberto"))
        if not run.check(f"Esc fecha o menu {m}", POPUP.get("fechado") is False, pixels_que_ficaram=d_left, popup=POPUP.get("fechado")):
            ps(f"foco; click {x} {y}")  # fecha pelo próprio botão para não contaminar o próximo
            settle(0.4)
        for f in (base, closed):
            f.unlink(missing_ok=True)


def discard_if_asked() -> None:
    """Aviso de alterações não salvas (cópia de teste): Descartar."""
    if "confirm" in st()["dialogs"]:
        x, y = center(st(), "confirm:discard")
        ps(f"foco; click {x} {y}")
        settle(0.8)


def start_screen(run: Run) -> None:
    """Arquivo > Início, projeto novo vertical 30 qd/s pelo mouse, depois reabre o projeto anterior pelos Recentes."""
    prev = st()["project"]
    x, y = center(st(), "menu:file")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    x, y = center(st(), "menu_item:file.start")
    ps(f"foco; click {x} {y}")
    settle(0.6)
    run.check("Arquivo > Início abre a tela inicial", "start" in st()["dialogs"], dialogos=st()["dialogs"])
    ps(f"cap {run.out / 'inicio_aberta.png'}")
    x, y = center(st(), "start:name")
    ps(f"foco; click {x} {y}; key ctrl+a; type Teste do mouse")
    x, y = center(st(), "start:format")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    x, y = center(st(), "start:format:3")
    ps(f"foco; click {x} {y}")
    settle(0.3)
    x, y = center(st(), "start:rate")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    x, y = center(st(), "start:rate:4")
    ps(f"foco; click {x} {y}")
    settle(0.3)
    ps(f"cap {run.out / 'inicio_preenchida.png'}")
    x, y = center(st(), "start:create")
    ps(f"foco; click {x} {y}")
    settle(1.0)
    discard_if_asked()
    q = aiai.call("QUERY", {"query": {"kind": "project"}})
    sets = q.get("settings", {})
    ok = st()["project"]["name"] == "Teste do mouse" and (sets.get("width"), sets.get("height"), sets.get("fps_num"), sets.get("fps_den")) == (1080, 1920, 30, 1)
    run.check("Criar projeto usa nome, formato vertical e 30 qd/s", ok, nome=st()["project"]["name"], settings={k: sets.get(k) for k in ("width", "height", "fps_num", "fps_den")})
    ps(f"cap {run.out / 'inicio_projeto_novo.png'}")
    # Recentes: o projeto aberto no começo (pela linha de comando) está no topo
    x, y = center(st(), "menu:file")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    x, y = center(st(), "menu_item:file.start")
    ps(f"foco; click {x} {y}")
    settle(0.6)
    ps(f"cap {run.out / 'inicio_recentes.png'}")
    x, y = center(st(), "start:recent:0")
    ps(f"foco; click {x} {y}")
    settle(2.0)
    discard_if_asked()
    now = st()["project"]
    run.check("Recente reabre o projeto anterior", now["path"] == prev["path"] and "start" not in st()["dialogs"], projeto=now["name"])


def prefs_and_keys(run: Run) -> None:
    """Editar > Preferências, aba Atalhos: troca o balde para K, dá o V do mover ao pincel (conflito), usa as teclas
    novas no canvas, confere a dica da prateleira e restaura tudo."""
    x, y = center(st(), "ws_tab:animacao")
    ps(f"foco; click {x} {y}")
    settle(0.8)
    x, y = center(st(), "menu:edit")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    x, y = center(st(), "menu_item:edit.prefs")
    ps(f"foco; click {x} {y}")
    settle(0.6)
    run.check("Editar > Preferências abre", "prefs" in st()["dialogs"], dialogos=st()["dialogs"])
    for tab in ("general", "folders", "performance", "keys"):
        x, y = center(st(), f"prefs:tab:{tab}")
        ps(f"foco; click {x} {y}")
        settle(0.4)
        ps(f"cap {run.out / f'prefs_{tab}.png'}")
        run.check(f"aba {tab} das Preferências", st()["prefs_tab"] == tab, aba=st()["prefs_tab"])
    x, y = center(st(), "keys:chord:tool.fill")
    ps(f"foco; click {x} {y}")
    settle(0.3)
    cap_on = st()["key_capture"]
    ps("foco; key k")
    settle(0.4)
    s = st()
    run.check("captura: K vira o atalho do balde", cap_on == "tool.fill" and s["shortcuts"]["tool.fill"] == "K" and s["key_capture"] is None, captura=cap_on, balde=s["shortcuts"]["tool.fill"])
    x, y = center(st(), "keys:chord:tool.brush")
    ps(f"foco; click {x} {y}")
    settle(0.3)
    ps("foco; key v")
    settle(0.4)
    s = st()
    ps(f"cap {run.out / 'prefs_keys_conflito.png'}")
    run.check("conflito: V vai para o pincel e o mover fica sem atalho", s["shortcuts"]["tool.brush"] == "V" and s["shortcuts"]["tool.move"] == "", pincel=s["shortcuts"]["tool.brush"], mover=s["shortcuts"]["tool.move"])
    aiai.call("UI_ACTION", {"action": "close_dialog", "dialog": "prefs"})  # o X da janela não tem retângulo registrado
    settle(0.4)
    cx, cy = center(st(), "canvas:area")
    ps(f"foco; move {cx} {cy}; key k")
    settle(0.3)
    a1 = st()["active_tool"]
    ps("foco; key v")
    settle(0.3)
    a2 = st()["active_tool"]
    run.check("teclas novas no canvas: K balde, V pincel", (a1, a2) == ("fill", "brush"), ativas=[a1, a2])
    x, y = center(st(), "tool:fill")
    ps(f"foco; move {x} {y}; wait 900")
    ps(f"cap {run.out / 'dica_balde_K.png'}")
    aiai.call("UI_ACTION", {"action": "open_dialog", "dialog": "prefs", "tab": "keys"})
    settle(0.5)
    x, y = center(st(), "keys:reset_all")
    ps(f"foco; click {x} {y}")
    settle(0.4)
    s = st()
    run.check("Restaurar todos volta ao padrão", (s["shortcuts"]["tool.fill"], s["shortcuts"]["tool.brush"], s["shortcuts"]["tool.move"]) == ("G", "B", "V"), atalhos={k: s["shortcuts"][k] for k in ("tool.fill", "tool.brush", "tool.move")})
    aiai.call("UI_ACTION", {"action": "close_dialog", "dialog": "prefs"})
    settle(0.3)


def main() -> int:
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    out.mkdir(parents=True, exist_ok=True)
    stop = out / "watch.stop"
    stop.unlink(missing_ok=True)
    watch_json = out / "watch.json"
    watcher = subprocess.Popen(PS + [WATCH, "-Out", str(watch_json), "-StopFile", str(stop), "-MaxSeconds", "900"])
    time.sleep(1.5)
    run = Run(out)
    s0 = st()
    lang0, ws0 = s0["language"], s0["workspace"]
    t0 = time.time()
    try:
        ps("foco; max")
        settle(1.5)
        run.check("janela maximizada e em foco", st()["window"]["maximized"], titulo=st()["window"]["title"])
        phases = (workspaces, tools_and_shortcuts, canvas_steps, timeline_steps, menus, start_screen, prefs_and_keys)
        only = set(sys.argv[2].split(",")) if len(sys.argv) > 2 else None  # ex.: prefs_and_keys
        for phase in [p for p in phases if only is None or p.__name__ in only]:
            try:
                phase(run)
            except Exception as e:  # passo que quebra vira falha registrada, o resto segue
                run.check(f"fase {phase.__name__}", False, erro=str(e))
    finally:
        stop.touch()
        watcher.wait(timeout=30)
        aiai.call("UI_ACTION", {"action": "set_language", "language": lang0})
        aiai.call("UI_ACTION", {"action": "set_workspace", "workspace": ws0})
    w = json.loads(watch_json.read_text(encoding="utf-8-sig")) if watch_json.exists() else {"erro": "vigia sem relatório"}
    fails = [s for s in run.steps if not s["ok"]]
    report = {
        "quando": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "segundos": round(time.time() - t0, 1),
        "projeto": s0["project"]["path"],
        "janela": st()["window"],
        "passos": run.steps,
        "falhas": len(fails),
        "ui_sem_resposta_10s": STALLS,
        "vigia": w,
    }
    (out / "relatorio.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"{len(run.steps)} passos, {len(fails)} falhas; vigia: {w}")
    if w.get("contaminated"):
        return 3
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
