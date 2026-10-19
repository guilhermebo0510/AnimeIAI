"""S23 (auditoria 3.2): repinta o curta sem o remendo que adivinhava a cor das sobras.

Roda de novo, na ordem, os scripts que pintam personagens (model sheets, expressões, C003, C004, C005, C002, C006) com
o balde corrigido (regiões estreitas com rótulo próprio) e `close_leftovers` só para frestas. Depois tira o corte de
ensaio C999 do projeto de produção e grava o lint de cada corte (com as regiões da borda) em `repintura.json`.
Uso: python c22_repintura.py [scripts...]   (sem argumentos = todos)
"""
import json
import runpy
import sys
import time

from curta import d, ids, ROOT, save_ids

ORDER = ["c02_sheets", "c03_sheet_draw", "c04_expressoes", "c10_c003", "c11_c004", "c12_c005", "c13_c002", "c14_c006"]
todo = sys.argv[1:] or ORDER
log = {}
for name in todo:
    t0 = time.time()
    print(f"==== {name}", flush=True)
    runpy.run_path(f"{name}.py", run_name="__main__")
    log[name] = round(time.time() - t0, 1)
    print(f"==== {name}: {log[name]} s", flush=True)

# corte de ensaio (C999 / episódio "Ensaios") fora do arquivo de produção
if "ep" in ids:
    try:
        d.apply({"kind": "delete_node", "id": ids["ep"]})
        print("removido o episódio de ensaio (C999)")
    except Exception as e:
        print("episódio de ensaio já não estava lá:", e)
    for k in [k for k in ids if k in ("ep", "sc_ensaio", "cut_ensaio") or k.startswith("ens_")]:
        del ids[k]
    save_ids()

rep = {"tempos_s": log, "lint": []}
for i, c in enumerate(ids["cuts"]):
    r = d.call("PRODUCTION", {"op": "lint_cut", "cut": c})
    rep["lint"].append({"corte": f"C00{i + 1}", "sem_tinta": r["unpainted_regions"], "area": r["unpainted_area"],
                        "borda_sem_tinta": r.get("border_unpainted_regions"), "limpo": r["clean"]})
    print(rep["lint"][-1])
json.dump(rep, open(f"{ROOT}/repintura.json", "w", encoding="utf-8"), ensure_ascii=False, indent=1)
