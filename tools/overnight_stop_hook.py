"""Hook de Stop do trabalho contínuo. Enquanto `sprints/.overnight-done` não existir e a sessão estiver no projeto
AnimeIAI, ele impede o Claude de parar e lembra a ordem do trabalho. Plano vivo desde 29/10/2026:
`sprints/registro/S27-copiloto-e-ecossistema/00-ROTEIRO.md`. Para encerrar, crie o arquivo `.overnight-done` ou tire o
hook de ~/.claude/settings.json."""
import json
import os
import sys

ROOT = r"C:\AnimeIAI"
DONE = os.path.join(ROOT, "sprints", ".overnight-done")
PLAN = os.path.join(ROOT, "sprints", "registro", "S27-copiloto-e-ecossistema", "00-ROTEIRO.md")

try:
    data = json.load(sys.stdin)
except Exception:
    data = {}
cwd = str(data.get("cwd") or os.getcwd()).replace("/", "\\").lower()
if "animeiai" not in cwd or os.path.exists(DONE):
    sys.exit(0)

reason = (
    "Trabalho continuo pedido pelo dono (29/10, noite), sem esperar validacao. Siga "
    "sprints/registro/S27-copiloto-e-ecossistema/00-ROTEIRO.md na ordem, um item por vez, com commit a cada item. "
    "Fase 1: Copiloto de traco em nivel profissional. O foco e a geracao parcial de IA que ajuda a desenhar, nao "
    "intercalacao. Rodadas de auditoria profunda com o ChatGPT/Codex ate 'NIVEL PROFISSIONAL: SIM' e a sua auditoria "
    "interna concordar. Vigie o ChatGPT a cada 10 min (tools/revisao/peek.ps1) e clique em 'Tentar novamente' se ele "
    "parar. Exija dezenas de itens por rodada. Inclua o relatorio de tempo das geracoes. Fase 2: auditoria de todos os "
    "workspaces com o ChatGPT. Fase 3: correcoes e melhorias (Modelagem/rig/animacao 3D de referencia, Composicao "
    "nivel After Effects). Fase 4: ferramentas de IA (personagem controlado por design sheet/LoRA, pintura, cenarios). "
    "Se o ChatGPT estiver trabalhando numa rodada, espere a resposta com um laco em segundo plano de no maximo 10 min, "
    "olhe a janela e retome. Se o ChatGPT bater o limite de 5 horas, anote a hora em que volta, siga com o que nao "
    "depende dele e retome nesse horario. Atualize a tabela 'Estado' do roteiro. Quando tudo estiver feito, crie "
    "sprints/.overnight-done."
)
print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=True))
