"""Hook de Stop do trabalho contínuo (noite de 27/10/2026): enquanto `sprints/.overnight-done` não existir e a sessão
estiver no projeto AnimeIAI, impede o Claude de parar e lembra a ordem do trabalho (o plano vivo fica em
`sprints/registro/S25-clip-studio/08-plano-continuo.md`). Para encerrar: criar o arquivo `.overnight-done` ou tirar o
hook de ~/.claude/settings.json."""
import json
import os
import sys

ROOT = r"C:\AnimeIAI"
DONE = os.path.join(ROOT, "sprints", ".overnight-done")
PLAN = os.path.join(ROOT, "sprints", "registro", "S25-clip-studio", "08-plano-continuo.md")

try:
    data = json.load(sys.stdin)
except Exception:
    data = {}
cwd = str(data.get("cwd") or os.getcwd()).replace("/", "\\").lower()
if "animeiai" not in cwd or os.path.exists(DONE):
    sys.exit(0)

reason = (
    "Trabalho contínuo (pedido do usuário, sem esperar validação): siga o plano em "
    "sprints/registro/S25-clip-studio/08-plano-continuo.md, na ordem, um item por vez, commit a cada item. "
    "Ordem: 1) todos os itens abertos das auditorias 1 e 2 (06 e 07-status); 2) calibrar as ferramentas com o "
    "Clip Studio Paint (o que faltar) e conferir que todas funcionam; 3) estudar a criação de pincéis/canetas do "
    "CSP (internet + o programa) e melhorar o nosso criador para fazer praticamente qualquer caneta; 4) melhorar a "
    "workspace de Modelagem (Blender e ANIGO, que é código nosso); 5) revisar Edição, Áudio e Roteiro; "
    "6) SÓ DEPOIS de tudo isso, ler a pasta 'ferramenta de Ia' e implementar. Atualize o plano ao concluir cada "
    "item. Quando tudo estiver feito, crie sprints/.overnight-done."
)
print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=True))
