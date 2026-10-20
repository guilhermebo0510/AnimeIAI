#!/usr/bin/env python3
"""Cliente Python (somente biblioteca padrão) da bridge do AnimeIAI Studio (ADR-004).

O app aberto com ``--bridge`` grava porta e token em ``%LOCALAPPDATA%\\AnimeIAI\\bridge\\bridge.json``
(``AIAI_BRIDGE_DISCOVERY`` troca o caminho; ``AIAI_BRIDGE_ADDR`` + ``AIAI_BRIDGE_TOKEN`` fixam o destino).
O arquivo é relido a cada chamada: sobrevive a reinício e recompilação do app.

Uso em scripts (lotes, calibração)::

    import aiai
    aiai.call("PING")
    out = aiai.call("APPLY_COMMAND", {"command": {"kind": "create_node", "name": "EP01",
                                                  "data": {"type": "episode", "number": 1}}})
    aiai.screenshot(r"C:\\AnimeIAI\\sprints\\registro\\evidencias\\S02\\tela.png")

Uso na linha de comando::

    python tools/aiai.py ACTION '{"json": "params"}' [--png out.png]
    python tools/aiai.py PING

Saída: o campo ``data`` da resposta, em JSON. Erro: mensagem no stderr e código de saída 1 (2 = uso incorreto).
Só fala com o app aberto; para o modo headless (sem app) use ``aiai-mcp call``.
"""

from __future__ import annotations

import base64
import http.client
import itertools
import json
import os
import sys
from pathlib import Path
from typing import Any

__all__ = ["BridgeError", "BridgeUnavailable", "call", "default_timeout", "export_video", "batch", "screenshot", "render", "ui_state", "click_widget", "discovery_path", "is_live"]

_MAX_RESPONSE = 32 << 20
_ids = itertools.count(1)


class BridgeError(RuntimeError):
    """O app respondeu com erro. ``code`` é o código da bridge (ex.: -32002 timeout da UI, -32010 revisão velha)."""

    def __init__(self, message: str, code: int | None = None, action: str | None = None):
        super().__init__(message if code is None else f"[{code}] {message}")
        self.message = message
        self.code = code
        self.action = action


class BridgeUnavailable(BridgeError):
    """Não foi possível falar com o app (sem arquivo de descoberta ou conexão recusada)."""


def discovery_path() -> Path:
    env = os.environ.get("AIAI_BRIDGE_DISCOVERY")
    if env:
        return Path(env)
    base = os.environ.get("LOCALAPPDATA")
    if base:
        return Path(base) / "AnimeIAI" / "bridge" / "bridge.json"
    return Path.home() / ".local" / "share" / "animeiai" / "bridge" / "bridge.json"


def _target() -> tuple[str, int, str]:
    """(host, porta, token) da próxima chamada."""
    addr = os.environ.get("AIAI_BRIDGE_ADDR", "").strip()
    if addr:
        host, _, port = addr.rpartition(":")
        if host not in ("127.0.0.1", "localhost") or not port.isdigit():
            raise BridgeUnavailable(f"AIAI_BRIDGE_ADDR inválido ou fora do loopback: {addr!r}")
        return "127.0.0.1", int(port), os.environ.get("AIAI_BRIDGE_TOKEN", "")
    path = discovery_path()
    try:
        d = json.loads(path.read_text(encoding="utf-8-sig"))
        host, _, port = str(d["addr"]).rpartition(":")
        if host != "127.0.0.1":
            raise ValueError(f"endereço fora do loopback: {d['addr']}")
        return host, int(port), str(d["token"])
    except (OSError, ValueError, KeyError) as e:
        raise BridgeUnavailable(f"AnimeIAI Studio não está aberto com --bridge (descoberta {path}: {e})") from e


def default_timeout(action: str) -> float:
    """Prazo de leitura por ação (um pouco acima do que o app dá à UI): ferramentas de produção demoram mais."""
    if action == "EXPORT_VIDEO":
        return 1200.0
    if action in ("BGM", "TTS_LINE", "TTS_KOKORO", "MIX_CUT", "LOUDNESS", "AUDIO_EDIT", "AUDIO_ANALYZE"):
        return 130.0
    if action in ("SKY", "PATTERN_FILL", "FILL_SHAPE", "EDIT_SET", "TEXT", "GAG_BG", "GUIDE3D", "MODEL_SHEET", "INBETWEEN", "PROPAGATE_FILLS", "LIPSYNC", "COMPARE", "RENDER"):
        return 65.0
    return 30.0


def call(action: str, params: dict[str, Any] | None = None, timeout: float | None = None) -> Any:
    """Uma ação pela bridge (HTTP ``POST /rpc`` com token Bearer). Devolve ``data``; levanta ``BridgeError`` em erro.

    Qualquer ação de ferramenta vale (SKY, PATTERN_FILL, TEXT, LINES, GAG_BG, PARTICLES, GUIDE3D, PERSPECTIVE, MODEL_SHEET,
    CHARACTER_MATERIAL, CONSISTENCY, INBETWEEN, PROPAGATE_FILLS, FILL_SHAPE, TTS_LINE, TTS_KOKORO, VOICE_PROFILE, LIPSYNC, SFX, BGM, MIX_CUT, LOUDNESS, AUDIO_EDIT, AUDIO_ANALYZE,
    COMPARE, PROBE_COLOR, MEASURE_LINE, MEASURE_EDGE, EDIT_SET, EXPORT_VIDEO...). ``timeout`` omitido = ``default_timeout(action)``."""
    if timeout is None:
        timeout = default_timeout(action)
    host, port, token = _target()
    body = json.dumps({"id": f"py-{next(_ids)}", "action": action, "params": params or {}}).encode("utf-8")
    headers = {"Content-Type": "application/json", "Authorization": f"Bearer {token}"}
    conn = http.client.HTTPConnection(host, port, timeout=timeout)
    try:
        conn.request("POST", "/rpc", body=body, headers=headers)
        resp = conn.getresponse()
        raw = resp.read(_MAX_RESPONSE + 1)
    except OSError as e:
        raise BridgeUnavailable(f"AnimeIAI Studio não responde em {host}:{port}: {e}; abra o app com --bridge") from e
    finally:
        conn.close()
    if len(raw) > _MAX_RESPONSE:
        raise BridgeError(f"resposta grande demais (> {_MAX_RESPONSE} bytes)", action=action)
    try:
        msg = json.loads(raw)
    except ValueError as e:
        raise BridgeError(f"resposta não é JSON (HTTP {resp.status}): {raw[:200]!r}", action=action) from e
    if msg.get("success"):
        return msg.get("data")
    raise BridgeError(str(msg.get("error", "erro desconhecido")), msg.get("code"), action)


def batch(calls: list[tuple[str, dict[str, Any] | None]], stop_on_error: bool = True) -> list[Any]:
    """Várias ações em sequência. Com ``stop_on_error=False`` os erros entram na lista como ``BridgeError``."""
    out: list[Any] = []
    for action, params in calls:
        try:
            out.append(call(action, params))
        except BridgeError as e:
            if stop_on_error:
                raise
            out.append(e)
    return out


def screenshot(path: str | os.PathLike[str], params: dict[str, Any] | None = None) -> dict[str, Any]:
    """SCREENSHOT da janela (ou de ``panel``/``region`` em ``params``) gravado em ``path``. Devolve o ``data`` sem a imagem."""
    data = call("SCREENSHOT", {**(params or {}), "inline": True}, timeout=45.0)
    b64 = data.pop("png_base64", None) if isinstance(data, dict) else None
    if not b64:
        raise BridgeError("a resposta de SCREENSHOT não trouxe png_base64", action="SCREENSHOT")
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(base64.b64decode(b64))
    data["saved_to"] = str(target)
    return data


def render(path: str | os.PathLike[str], cut: str, frame: int = 0, **params: Any) -> dict[str, Any]:
    """RENDER (motor CPU real) de ``cut``/``frame`` gravado em ``path``. Extras: ``layers``, ``background``, ``size``, ``camera``..."""
    data = call("RENDER", {"cut": cut, "frame": frame, "inline": True, **params}, timeout=120.0)
    b64 = data.pop("png_base64", None)
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(base64.b64decode(b64))
    data["saved_to"] = str(target)
    return data


def export_video(out: str | os.PathLike[str], **params: Any) -> dict[str, Any]:
    """EXPORT_VIDEO: ``cut`` (com ``start``/``end``), ``episode`` ou ``edit``; ``codec``, ``crf``, ``preset``, ``subtitles``... Devolve frames e ffprobe."""
    return call("EXPORT_VIDEO", {"out": str(out), **params})


def ui_state() -> dict[str, Any]:
    """GET_UI_STATE: workspace, painéis, ``widgets`` (retângulos lógicos/físicos/de tela), diálogos, DPI, janela."""
    return call("GET_UI_STATE")


def click_widget(name: str, button: str = "primary") -> dict[str, Any]:
    """Clique injetado no egui (sem tocar no cursor do SO) no centro do elemento ``name`` de ``ui_state()['widgets']``."""
    w = ui_state()["widgets"].get(name)
    if w is None:
        raise BridgeError(f"elemento desconhecido ou invisível: {name}")
    x, y = (w["rect_logical"][0] + w["rect_logical"][2] / 2, w["rect_logical"][1] + w["rect_logical"][3] / 2)
    ev = [{"kind": "mouse_move", "x": x, "y": y}, {"kind": "mouse_down", "x": x, "y": y, "button": button},
          {"kind": "mouse_up", "x": x, "y": y, "button": button}, {"kind": "wait", "frames": 2}]
    return call("INPUT", {"events": ev, "wait": True})


def is_live() -> bool:
    try:
        call("PING", timeout=2.0)
        return True
    except BridgeError:
        return False


def _main(argv: list[str]) -> int:
    # saída em UTF-8 também quando redirecionada (o padrão do Windows para pipes é a página de código local)
    for stream in (sys.stdout, sys.stderr):
        if not stream.isatty() and hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    args = list(argv)
    png = None
    if "--png" in args:
        i = args.index("--png")
        try:
            png = args[i + 1]
        except IndexError:
            print("aiai.py: --png exige um caminho", file=sys.stderr)
            return 2
        del args[i : i + 2]
    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return 0 if args else 2
    action = args[0]
    if not action.replace("_", "").isalpha() or not action.isupper():
        print(f"aiai.py: ação inválida {action!r} (esperado MAIUSCULAS_COM_SUBLINHADO)", file=sys.stderr)
        return 2
    params: dict[str, Any] = {}
    if len(args) > 1:
        raw = args[1]
        if raw.startswith("@"):
            raw = Path(raw[1:]).read_text(encoding="utf-8-sig")
        try:
            params = json.loads(raw)
        except ValueError as e:
            print(f"aiai.py: params não é JSON válido: {e}", file=sys.stderr)
            return 2
        if not isinstance(params, dict):
            print("aiai.py: params deve ser um objeto JSON", file=sys.stderr)
            return 2
    try:
        if png:
            result = screenshot(png, params) if action == "SCREENSHOT" else call(action, params)
            if action != "SCREENSHOT" and isinstance(result, dict) and "png_base64" in result:
                Path(png).parent.mkdir(parents=True, exist_ok=True)
                Path(png).write_bytes(base64.b64decode(result.pop("png_base64")))
                result["saved_to"] = png
        else:
            result = call(action, params)
    except BridgeError as e:
        print(f"aiai.py: {e}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(_main(sys.argv[1:]))
