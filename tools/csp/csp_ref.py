"""Desenha a folha de teste padrão no Clip Studio Paint e salva o canvas como referência (zoom 100%).

Pré-requisitos (feitos uma vez, à mão ou pelo harness): documento de 900x700 px aberto a 100%, a camada raster escolhida e a
sub-ferramenta escolhida no painel. O script só muda o tamanho pela paleta "Tamanho do pincel", limpa a camada, desenha e captura.

Uso:
    python tools/csp/csp_ref.py --name g-pen --sizes 3,10,30 --out calibracao/csp/ref/caneta
"""
import argparse
import os
import subprocess
import sys

from PIL import Image

sys.path.insert(0, os.path.dirname(__file__))
from csp_sheet import sheet, mix_sheet, to_pen_file, W, H, COR_A, COR_B  # noqa: E402

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
INPUT = os.path.join(ROOT, "tools", "harness", "input.ps1")
PEN = os.path.join(ROOT, "tools", "harness", "pen.ps1")
CAP_OFF = (-9, -9)  # a captura da janela maximizada começa 9 px fora da tela

# Paleta "Tamanho do pincel" (layout de fábrica): colunas e linhas em coordenadas de tela.
PAL_X = [76, 115, 156, 196, 237, 278, 318]
PAL_Y = [638, 688, 738, 788]
PAL_V = [[0.7, 1, 1.5, 2, 2.5, 3, 4], [5, 6, 7, 8, 10, 12, 15], [17, 20, 25, 30, 40, 50, 60], [70, 80, 100, 120, 150, 170, 200]]


def ps(script, *args):
    r = subprocess.run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, *args], capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"falhou: {script} {args}\n{r.stdout}\n{r.stderr}")
    return r.stdout


def csp(cmds):
    return ps(INPUT, "-Process", "CLIPStudioPaint", "-TitlePrefix", "CLIP", "-S", cmds)


FILL_CMD = (661, 76)  # ícone "Preencher" da barra de comandos

# Aba "Controles de cor" (RGB): campos de valor R, G, B em coordenadas de tela.
RGB_TAB = (184, 816)
RGB_FIELDS = [(317, 852), (317, 883), (317, 914)]


def set_color(rgb):
    cmds = [f"foco; click {RGB_TAB[0]} {RGB_TAB[1]}; wait 300"]
    for (x, y), v in zip(RGB_FIELDS, rgb):
        cmds.append(f"dclick {x} {y}; wait 150; key ctrl+a; type {v}; key enter; wait 150")
    csp("; ".join(cmds))


def size_cell(size):
    for r, row in enumerate(PAL_V):
        for c, v in enumerate(row):
            if abs(v - size) < 1e-6:
                return PAL_X[c], PAL_Y[r]
    raise SystemExit(f"tamanho {size} não está na paleta de fábrica")


def _csp_on_top(im, x0, y0):
    """Os quatro pontos logo fora do canvas têm o cinza escuro do fundo do Clip Studio (nenhuma janela por cima)."""
    for x, y in ((x0 - 6, y0 - 6), (x0 + W + 5, y0 - 6), (x0 - 6, y0 + H + 5), (x0 + W + 5, y0 + H + 5)):
        r, g, b = im.getpixel((x, y))[:3]
        if not (abs(r - g) < 6 and abs(g - b) < 6 and r < 90):
            return False
    return True


def capture(origin, path, tmp):
    x0, y0 = origin[0] - CAP_OFF[0], origin[1] - CAP_OFF[1]
    for tentativa in range(4):
        csp(f"foco; move 1650 950; wait 400; cap {tmp}")
        im = Image.open(tmp).convert("RGB")
        if _csp_on_top(im, x0, y0):
            im.crop((x0, y0, x0 + W, y0 + H)).save(path)
            return
        print(f"outra janela por cima do Clip Studio; tentando de novo ({tentativa + 1})")
        csp("wait 1500")
    raise SystemExit("captura falhou: o canvas do Clip Studio não está visível")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--name", required=True)
    ap.add_argument("--sizes", default="3,10,30")
    ap.add_argument("--out", required=True)
    ap.add_argument("--origin", default="374,185", help="canto do canvas na tela, a 100%%")
    ap.add_argument("--keep", action="store_true", help="não limpa a camada antes (para testar cruzamento com o anterior)")
    ap.add_argument("--mix", action="store_true", help="teste de mistura: faixas na cor A, traços na cor B por cima")
    ap.add_argument("--fill-base", action="store_true", help="enche a camada de preto (Ctrl+A, Preencher) antes da folha: teste de borracha")
    ap.add_argument("--grid-base", action="store_true", help="só desenha a grade de mistura com a ferramenta atual e sai (base para misturadores)")
    a = ap.parse_args()
    origin = tuple(int(v) for v in a.origin.split(","))
    os.makedirs(a.out, exist_ok=True)
    tmp = os.path.join(a.out, "_cap.png")
    penf = os.path.join(a.out, "_pen.txt")
    for size in [float(v) for v in a.sizes.split(",")]:
        x, y = size_cell(size)
        clear = "" if a.keep else "key ctrl+a; wait 150; key del; wait 150; key ctrl+d; wait 150; "
        csp(f"foco; click {x} {y}; wait 250; {clear}move 1650 950")
        tag = f"{size:g}".replace(".", "_")
        if a.grid_base:
            base, cruz = mix_sheet(size)
            set_color(COR_A)
            to_pen_file(base, origin[0], origin[1], penf)
            ps(PEN, "-File", penf)
            set_color(COR_B)
            to_pen_file(cruz, origin[0], origin[1], penf)
            ps(PEN, "-File", penf)
            set_color((0, 0, 0))
            print("grade desenhada")
            continue
        if a.fill_base:
            set_color((0, 0, 0))
            csp(f"foco; key ctrl+a; wait 200; click {FILL_CMD[0]} {FILL_CMD[1]}; wait 400; key ctrl+d; wait 200; move 1650 950")
        if a.mix:
            base, cruz = mix_sheet(size)
            set_color(COR_A)
            to_pen_file(base, origin[0], origin[1], penf)
            ps(PEN, "-File", penf)
            set_color(COR_B)
            to_pen_file(cruz, origin[0], origin[1], penf)
            ps(PEN, "-File", penf)
            set_color((0, 0, 0))
            out = os.path.join(a.out, f"{a.name}_mix_s{tag}.png")
        else:
            to_pen_file(sheet(size), origin[0], origin[1], penf)
            ps(PEN, "-File", penf)
            out = os.path.join(a.out, f"{a.name}_s{tag}.png")
        capture(origin, out, tmp)
        print(out)
    for f in (tmp, penf):
        if os.path.exists(f):
            os.remove(f)


if __name__ == "__main__":
    main()
