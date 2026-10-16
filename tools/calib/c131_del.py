"""Remove entradas de LINES em c131_rosas.py pelo nome (com as linhas de continuação). uso: python c131_del.py nome [nome ...]"""
import ast, sys
p = "c131_rosas.py"
src = open(p, encoding="utf-8").read()
tree = ast.parse(src)
lines = src.split("\n")
kill = set()
for node in ast.walk(tree):
    if isinstance(node, ast.Dict):
        for k, v in zip(node.keys, node.values):
            if isinstance(k, ast.Constant) and k.value in sys.argv[1:]:
                for i in range(k.lineno - 1, v.end_lineno):
                    kill.add(i)
open(p, "w", encoding="utf-8").write("\n".join(l for i, l in enumerate(lines) if i not in kill))
print("removidas", len(kill), "linhas")
