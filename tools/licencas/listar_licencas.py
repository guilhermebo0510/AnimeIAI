"""Lista a licença de cada crate do grafo resolvido (cargo metadata) e marca as que pedem atenção.
Uso: cargo metadata --format-version 1 --locked > meta.json; python listar_licencas.py meta.json saida.md"""
import json, sys, collections

meta = json.load(open(sys.argv[1], encoding="utf-8"))
ws = set(meta["workspace_members"])
pkgs = {p["id"]: p for p in meta["packages"]}
used = {n["id"] for n in meta["resolve"]["nodes"]}
deps = [pkgs[i] for i in used if i not in ws]
ours = [pkgs[i] for i in ws]

ATENCAO = ("GPL", "AGPL", "LGPL", "MPL", "EPL", "CDDL", "CC-BY-SA", "SSPL", "BUSL")
PERMISSIVAS = ("MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016",
               "0BSD", "BSL-1.0", "CC0-1.0", "Unlicense", "MIT-0", "OFL-1.1", "Ubuntu-font-1.0", "CDLA-Permissive-2.0", "NCSA", "bzip2-1.0.6")

def classifica(lic):
    if not lic:
        return "sem licença declarada"
    alts = [a.strip(" ()") for a in lic.replace("/", " OR ").split(" OR ")]
    # basta uma alternativa permissiva (A OR B); com AND todas valem
    for alt in alts:
        partes = [p.strip(" ()") for p in alt.split(" AND ")]
        if all(any(p.startswith(x) for x in PERMISSIVAS) or "WITH LLVM-exception" in p for p in partes):
            return "permissiva"
    if any(k in lic for k in ATENCAO):
        return "atenção (copyleft fraco/forte)"
    return "revisar"

grupos = collections.defaultdict(list)
for p in deps:
    lic = p.get("license") or (("arquivo: " + p["license_file"]) if p.get("license_file") else "")
    grupos[classifica(p.get("license"))].append((p["name"], p["version"], lic))

por_lic = collections.Counter((p.get("license") or "?") for p in deps)
out = open(sys.argv[2], "w", encoding="utf-8")
w = lambda s="": out.write(s + "\n")
w("# Licenças das dependências Rust (gerado)")
w()
w(f"Gerado por `tools/licencas/listar_licencas.py` a partir de `cargo metadata --locked`. Crates do workspace: {len(ours)}; dependências no grafo resolvido (todas as plataformas e features ativas no lock): {len(deps)}.")
w()
w("| Classe | Crates |")
w("|---|---|")
for k in ("permissiva", "atenção (copyleft fraco/forte)", "revisar", "sem licença declarada"):
    w(f"| {k} | {len(grupos[k])} |")
w()
for k in ("atenção (copyleft fraco/forte)", "revisar", "sem licença declarada"):
    if grupos[k]:
        w(f"## {k}")
        w()
        w("| Crate | Versão | Licença |")
        w("|---|---|---|")
        for n, v, l in sorted(grupos[k]):
            w(f"| {n} | {v} | {l} |")
        w()
w("## Expressões de licença (todas as dependências)")
w()
w("| Expressão | Crates |")
w("|---|---|")
for lic, n in por_lic.most_common():
    w(f"| `{lic}` | {n} |")
w()
w("## Crates do workspace")
w()
for p in sorted(ours, key=lambda p: p["name"]):
    w(f"- {p['name']} {p['version']}: `{p.get('license') or 'não declarada'}`")
out.close()
for k, v in grupos.items():
    print(k, len(v))
