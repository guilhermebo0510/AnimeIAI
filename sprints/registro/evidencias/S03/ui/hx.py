#!/usr/bin/env python3
"""Ajudantes do roteiro de mouse REAL do S03/B2 (SendInput via tools/harness/input.ps1) e de leitura do estado do app.

Coordenadas: GET_UI_STATE devolve pontos LOGICOS da janela; a tela usa pixels FISICOS:
  fisico = origem_cliente_fisica + logico * pixels_per_point   (licao 22: nada de 1,25 fixo).
`canvas_to_screen(x, y)` converte um ponto do DESENHO (pixels do canvas) para pixels fisicos da tela, usando a matriz que o proprio app
publica em GET_UI_STATE.editor.canvas (origin_logical + m), o que prova a conversao tela<->desenho no caminho do usuario.
"""
import json
import subprocess
import time
from pathlib import Path

MCP = r"C:\AnimeIAI\app\target\debug\aiai-mcp.exe"
INPUT = r"C:\AnimeIAI\tools\harness\input.ps1"
EVID = Path(r"C:\AnimeIAI\sprints\registro\evidencias\S03\ui")
LOG = []
MIN_CONSOLE = """
Add-Type -TypeDefinition 'using System;using System.Text;using System.Runtime.InteropServices;public class FW3{[DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();[DllImport("user32.dll",CharSet=CharSet.Unicode)]public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);[DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int c);}'
$h=[FW3]::GetForegroundWindow(); $sb=New-Object System.Text.StringBuilder 256; [void][FW3]::GetWindowText($h,$sb,256)
if ($sb.ToString() -like '*aiai-studio.exe*') { [void][FW3]::ShowWindow($h,6) }
$p = Get-Process aiai-studio | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($p) { (New-Object -ComObject WScript.Shell).AppActivate($p.Id) | Out-Null }
"""


def call(action, params=None, png=None):
    cmd = [MCP, "call", action, json.dumps(params or {})]
    if png:
        cmd += ["--png", str(png)]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if p.returncode != 0:
        raise SystemExit(f"{action}: {p.stdout} {p.stderr}")
    return json.loads(p.stdout)


def data(action, params=None):
    return call(action, params)


def ui(ops, quiet=False):
    for attempt in range(6):
        p = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", INPUT, "-S", ops], capture_output=True, text=True, encoding="utf-8", errors="replace")
        last = (p.stdout or "").strip().splitlines()[-1] if (p.stdout or "").strip() else ""
        LOG.append(f"- `input.ps1 -S \"{ops}\"` -> exit {p.returncode} {last} {p.stderr.strip()}")
        if p.returncode == 0:
            return
        # outra janela na frente (lição 20): espera e tenta focar de novo antes de qualquer clique
        if "foco" in (p.stderr or "") + (p.stdout or "") and attempt < 5:
            # a janela de console do exe (Windows Terminal aberto pelo Start-Process) pode estar na frente: minimiza-a e ativa o app
            subprocess.run(["powershell", "-NoProfile", "-Command", MIN_CONSOLE], capture_output=True)
            time.sleep(1.5)
            continue
        raise SystemExit(f"input.ps1 falhou ({p.returncode}): {p.stdout} {p.stderr}")


def state():
    return data("GET_UI_STATE")


def center(name, st=None):
    st = st or state()
    w = st["widgets"].get(name)
    if not w:
        raise SystemExit(f"sem widget {name!r}: {sorted(k for k in st['widgets'] if name.split(':')[0] in k)[:30]}")
    return round(w["center_screen"][0]), round(w["center_screen"][1])


def click(name, wait=350, st=None):
    x, y = center(name, st)
    ui(f"click {x} {y}; wait {wait}")
    return x, y


def to_screen(st, lx, ly):
    o = st["window"]["client_origin_screen_physical"]
    ppp = st["pixels_per_point"]
    return round(o[0] + lx * ppp), round(o[1] + ly * ppp)


def canvas_to_screen(x, y, st=None):
    st = st or state()
    c = st["editor"]["canvas"]
    ox, oy = c["origin_logical"]
    a, b, cc, d = c["m"]
    return to_screen(st, ox + a * x + b * y, oy + cc * x + d * y)


def check(cond, msg):
    LOG.append(f"- {'OK ' if cond else 'FALHOU '} {msg}")
    print(("OK   " if cond else "FALHOU ") + msg)
    if not cond:
        raise SystemExit(f"FALHOU: {msg}")


def cap(name):
    ui(f"cap {EVID / name}")


POLY = r"C:\AnimeIAI\sprints\registro\evidencias\S03\ui\polydrag.ps1"


def poly(points, step=4, delay=12, button="left", keys=""):
    """Arrasto real por um caminho: lista de (x, y) em pixels fisicos; um unico traco no canvas."""
    pts = ";".join(f"{x},{y}" for x, y in points)
    cmd = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", POLY, "-Points", pts, "-Step", str(step), "-Delay", str(delay), "-Button", button]
    if keys:
        cmd += ["-Keys", keys]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    LOG.append(f"- polydrag {len(points)} pontos ({button}{'+' + keys if keys else ''}) -> exit {p.returncode} {(p.stdout or '').strip()} {p.stderr.strip()}")
    if p.returncode != 0:
        raise SystemExit(f"polydrag falhou: {p.stdout} {p.stderr}")
    time.sleep(0.25)


def poly_canvas(points, **kw):
    st = state()
    poly([canvas_to_screen(x, y, st) for x, y in points], **kw)
