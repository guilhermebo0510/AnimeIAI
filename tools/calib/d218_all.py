"""Corte D: refaz um desenho inteiro na ordem certa e compara. uso: python d218_all.py TAG [--linhas] [--des N]
(--des N: redesenho inteiro N (7/8, tremor) em cels próprios de todas as camadas; padrão 1)"""
import json, os, subprocess, sys

HERE = os.path.dirname(__file__)
tag = sys.argv[1] if len(sys.argv) > 1 else "t"
des = sys.argv[sys.argv.index("--des") + 1] if "--des" in sys.argv else "1"
FIRST = {"1": 0, "7": 60, "8": 63}
env = dict(os.environ, PYTHONIOENCODING="utf-8", D218_DES=des)


def run(script, *args):
    r = subprocess.run([sys.executable, os.path.join(HERE, script), *args], env=env, capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        print(r.stdout[-2000:], r.stderr[-3000:])
        raise SystemExit(f"{script} falhou")


steps = (["d218_face.py", "d218_hair.py"] if "--linhas" in sys.argv else ["d218_hair.py --so-fecho"]) + [
    "d218_color.py", "d218_bg.py", "d218_shade.py", "d218_bangs.py", "d218_rim.py", "d218_low.py", "d218_details.py", "d218_focus.py", f"d218_dyn.py {des}"]
for s in steps:
    name, *a = s.split()
    run(name, *a)
fr = FIRST[des]
run("d218_compare.py", str(fr), tag)
rep = json.load(open(rf"C:\AnimeIAI\sprints\registro\calibracao\D218\tentativas\{tag}_q{fr:02d}\q{fr:04d}_relatorio.json", encoding="utf-8"))
print(tag, "flip", round(rep["flip"]["mean"], 4), "p50", round(rep["flip"]["p50"], 4), "ssim", round(rep["ssim"], 4),
      "dE", round(rep["delta_e"]["overall_mean"], 2), "F1", round(rep.get("edges", {}).get("f1", rep.get("edge_f1", 0)), 4))
