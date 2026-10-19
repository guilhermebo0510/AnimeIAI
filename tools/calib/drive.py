"""Cliente da calibração: chama `aiai-mcp call` (headless) num projeto .aiai próprio. Cada chamada é um comando real do
app (mesmo caminho do MCP); o projeto guarda o log de comandos como evidência.

    from drive import Drive
    d = Drive(r"C:\\AnimeIAI\\calibracao\\projetos\\c131.aiai")
    d.call("QUERY", {"query": {"kind": "stats"}})
"""
import json
import time
import os
import subprocess
import tempfile

MCP = os.environ.get("AIAI_MCP_EXE", r"C:\AnimeIAI\app\target\release\aiai-mcp.exe")


class DriveError(RuntimeError):
    pass


class Drive:
    def __init__(self, project_dir, exe=MCP):
        self.project = project_dir
        self.exe = exe
        self.calls = 0

    def call(self, action, params=None, png=None, timeout=600):
        params = params or {}
        fd, path = tempfile.mkstemp(suffix=".json")
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(params, f, ensure_ascii=False)
        cmd = [self.exe, "call", action, "@" + path, "--project", self.project]
        if png:
            cmd += ["--png", png]
        env = dict(os.environ)
        env.setdefault("AIAI_MCP_ALLOWED_DIRS", r"C:\AnimeIAI\producoes;C:\AnimeIAI\calibracao;C:\AnimeIAI\sprints\registro\evidencias;C:\AnimeIAI\sprints\registro\calibracao")
        try:
            # arquivo do projeto preso por outro processo (indexador/antivírus: erros 1224/32/33 do Windows) além do
            # que o próprio app espera: o salvamento é atômico (project.json só troca no fim), então repetir é seguro
            for attempt in range(4):
                r = subprocess.run(cmd, capture_output=True, timeout=timeout, env=env)
                err = r.stdout.decode("utf-8", "replace") + r.stderr.decode("utf-8", "replace")
                if r.returncode == 0 or not any(f"os error {c})" in err for c in (1224, 32, 33)):
                    break
                time.sleep(2 + 2 * attempt)
        finally:
            os.unlink(path)
        self.calls += 1
        out = r.stdout.decode("utf-8", "replace").strip()
        if r.returncode != 0:
            raise DriveError(f"{action} falhou ({r.returncode}): {out} {r.stderr.decode('utf-8', 'replace')}")
        try:
            return json.loads(out) if out else None
        except json.JSONDecodeError:
            return out

    # atalhos
    def apply(self, command):
        return self.call("APPLY_COMMAND", {"command": command})

    def create(self, name, data, parent=None):
        cmd = {"kind": "create_node", "name": name, "data": data}
        if parent:
            cmd["parent"] = parent
        return self.apply(cmd)["created"][0]

    def query(self, q):
        return self.call("QUERY", {"query": q})
