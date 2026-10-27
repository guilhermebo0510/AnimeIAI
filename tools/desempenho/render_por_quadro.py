"""S23: tempo de render por quadro, headless, numa CÓPIA do curta (nunca no projeto versionado).

uso: python render_por_quadro.py <copia.aiai> <saida.json>
Cada chamada do aiai-mcp abre o projeto do zero; a linha de base (QUERY stats) mede esse custo
e é descontada para estimar o render em si.
"""
import json, os, subprocess, sys, time, statistics

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MCP = os.path.join(ROOT, "app", "target", "release", "aiai-mcp.exe")
proj, out = sys.argv[1], sys.argv[2]
ids = json.load(open(os.path.join(ROOT, "producoes", "micro-anime", "ids.json"), encoding="utf-8"))
tmp = os.path.join(os.path.dirname(out), "_quadros")
os.makedirs(tmp, exist_ok=True)


def call(action, params, png=None):
    cmd = [MCP, "call", action, json.dumps(params), "--project", proj]
    if png:
        cmd += ["--png", png]
    t = time.perf_counter()
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8")
    dt = (time.perf_counter() - t) * 1000
    if r.returncode != 0:
        raise SystemExit(f"{action} falhou: {r.stderr.strip()}")
    return dt, r.stdout


base = [call("QUERY", {"query": {"kind": "stats"}})[0] for _ in range(5)]
res = {"baseline_ms": [round(x, 1) for x in base], "cuts": []}
b = statistics.median(base)
for i, cut in enumerate(ids["cuts"]):
    node = json.loads(call("QUERY", {"query": {"kind": "node", "id": cut}})[1])
    frames = int(node["data"]["duration"])
    picks = sorted({0, frames // 2, max(frames - 1, 0)})
    rows = []
    for f in picks:
        png = os.path.join(tmp, f"c{i}_f{f}.png")
        dt, _ = call("RENDER", {"cut": cut, "frame": f}, png)
        rows.append({"frame": f, "wall_ms": round(dt, 1), "render_est_ms": round(dt - b, 1)})
    res["cuts"].append({"cut": i + 1, "id": cut, "frames": frames, "renders": rows})
    print(i + 1, rows, flush=True)
allr = [r["render_est_ms"] for c in res["cuts"] for r in c["renders"]]
res["render_est_ms"] = {"mediana": round(statistics.median(allr), 1), "max": max(allr), "min": min(allr), "n": len(allr)}
json.dump(res, open(out, "w", encoding="utf-8"), indent=1)
print(json.dumps(res["render_est_ms"]), "base", round(b, 1))
