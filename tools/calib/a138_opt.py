"""Corte A: para cada linha, escolhe a VARIANTE (paquímetro com busca longa, curta, ou a estimativa digitada sem medição)
que melhora a régua (F1 + SSIM do COMPARE no quadro 12) — só COMO medir; a geometria vem da estimativa digitada.
Avalia sem refazer o balde (as linhas mudam pouco as regiões); o balde é refeito no fim. Guarda em a138_variantes.json.
uso: python a138_opt.py [prefixo]"""
import json, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from a138_char import LINES, VAR_F

ENV = dict(os.environ, PYTHONIOENCODING="utf-8")
PY = sys.executable


def run(*a):
    r = subprocess.run([PY, os.path.join(HERE, a[0]), *a[1:]], env=ENV, capture_output=True, text=True, encoding="utf-8")
    if r.returncode:
        raise SystemExit(r.stderr[-2000:])
    return r.stdout


def score():
    run("a138_char.py", "linhas")
    out = subprocess.run([PY, "-c", "import sys,json; sys.path.insert(0,r'%s'); from a138 import compare,F; r=compare(F,'opt'); r=r.get('report',r); "
                          "print(json.dumps([r['edges']['f1'], r['ssim'], r['flip']['mean']]))" % HERE],
                         env=ENV, capture_output=True, text=True, encoding="utf-8", cwd=HERE).stdout.strip().splitlines()[-1]
    f1, ss, fl = json.loads(out)
    return f1 + ss, f1, ss, fl


pref = sys.argv[1] if len(sys.argv) > 1 else ""
var = json.load(open(VAR_F)) if os.path.exists(VAR_F) else {}
best = score()
print("início", [round(v, 4) for v in best], flush=True)
for name, (poly, w, closed, *opt) in LINES.items():
    if not name.startswith(pref):
        continue
    o = opt[0] if opt else {}
    cur = var.get(name)
    opts = ["raw", "fino", "longo"] if "raw" not in o else ["longo", "fino"]
    default = "raw" if "raw" in o else ("fino" if o.get("fino", name[:2] in ("g_", "l_", "p_", "b_", "k_")) else "longo")
    for v in opts:
        if v == (cur or default):
            continue
        var[name] = v
        json.dump(var, open(VAR_F, "w"), indent=0)
        sc = score()
        if sc[0] > best[0] + 1e-4 and sc[3] < best[3] + 0.002:
            best, cur = sc, v
            print(name, v, [round(x, 4) for x in sc], flush=True)
    if cur is None:
        var.pop(name, None)
    else:
        var[name] = cur
    json.dump(var, open(VAR_F, "w"), indent=0)
print("fim", [round(v, 4) for v in best])
