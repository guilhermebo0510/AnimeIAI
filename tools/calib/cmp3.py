"""Leitura: render | referência | bordas do COMPARE num recorte. uso: python cmp3.py tag quadro x0 y0 x1 y1 saida [escala] [corte]"""
import sys
from PIL import Image
tag, q = sys.argv[1], int(sys.argv[2])
x0, y0, x1, y1 = (int(v) for v in sys.argv[3:7])
out = sys.argv[7]
sc = float(sys.argv[8]) if len(sys.argv) > 8 else 1.0
corte = sys.argv[9] if len(sys.argv) > 9 else "131"
base = {"131": "C131", "218": "D218"}[corte]
d = rf"C:\AnimeIAI\sprints\registro\calibracao\{base}\tentativas\{tag}_q{q:02d}"
R = Image.open(rf"{d}\q{q:04d}_render.png").convert("RGB")
F = Image.open(rf"C:\AnimeIAI\calibracao\ref\cut_{corte}\frames\f_{q + 1:04d}.png").convert("RGB")
B = Image.open(rf"{d}\q{q:04d}_bordas.png").convert("RGB")
w, h = x1 - x0, y1 - y0
o = Image.new("RGB", ((w + 6) * 3, h), (0, 255, 255))
for i, I in enumerate((R, F, B)):
    o.paste(I.crop((x0, y0, x1, y1)), (i * (w + 6), 0))
o.resize((int(o.width * sc), int(o.height * sc))).save(out)
