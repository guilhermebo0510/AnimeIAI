"""Corte D: escolhe por forma relida por varredura (SHAPES_DES do desenho) a folga para fora (0 / 0,5 / 1 / 1,5 / 2 px) que melhora
a régua (F1 + SSIM no primeiro quadro do desenho). A forma digitada não muda; só o meio pixel de borda. uso: python d218_optgrow.py desenho"""
import json, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
des = sys.argv[1]
os.environ["D218_DES"] = des
sys.path.insert(0, HERE)
import d218_low as LO, d218_details as DT
from d218 import FIRST

GF = os.path.join(HERE, "d218_grow.json")
ENV = dict(os.environ, PYTHONIOENCODING="utf-8", D218_DES=des)
fr = FIRST[int(des)]


def run(*a):
    r = subprocess.run([sys.executable, os.path.join(HERE, a[0]), *a[1:]], env=ENV, capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise SystemExit(r.stderr[-2000:])


def score():
    run("d218_low.py")
    run("d218_details.py")
    run("d218_compare.py", str(fr), "grow")
    rep = json.load(open(rf"C:\AnimeIAI\sprints\registro\calibracao\D218\tentativas\grow_q{fr:02d}\q{fr:04d}_relatorio.json", encoding="utf-8"))
    return rep["edges"]["f1"] + rep["ssim"], rep["edges"]["f1"], rep["ssim"]


allg = json.load(open(GF)) if os.path.exists(GF) else {}
g = allg.setdefault(des, {})
best = score()
print("início", [round(v, 4) for v in best], flush=True)
names = list(LO.SHAPES_DES.get(int(des), {})) + list(DT.SHAPES_DES.get(int(des), {}))
for n in names:
    cur = g.get(n)
    for v in (0.0, 0.5, 1.5, 2.0):
        g[n] = v
        json.dump(allg, open(GF, "w"), indent=1)
        sc = score()
        if sc[0] > best[0] + 1e-4:
            best, cur = sc, v
            print(n, v, [round(x, 4) for x in sc], flush=True)
    if cur is None:
        g.pop(n, None)
    else:
        g[n] = cur
    json.dump(allg, open(GF, "w"), indent=1)
print("fim", [round(v, 4) for v in best])
