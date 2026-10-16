# /// script
# requires-python = ">=3.10,<3.14"
# dependencies = ["kokoro-onnx==0.6.1", "numpy"]
# ///
"""Kokoro-82M (ONNX) -> WAV, local e gratuito (S17). Vozes PT-BR (pf_dora, pm_alex, pm_santa) e EN.

Modelo: Kokoro-82M (Apache-2.0, hexgrad) em ONNX via kokoro-onnx (MIT). Fonemização: espeak-ng (GPL-3, biblioteca carregada por
este script; o app chama o script como PROCESSO separado, nunca liga a biblioteca - regra 4).

Uso:
  uv run --script tools/kokoro/kokoro_tts.py --text "Ola, bom dia." --voice pf_dora --lang pt-br --speed 1.0 --out saida.wav
  uv run --script tools/kokoro/kokoro_tts.py --list-voices
Modelos em sidecars/kokoro/ (kokoro-v1.0.onnx + voices-v1.0.bin); --model-dir muda a pasta.
Saida: WAV PCM16 mono 24 kHz. NAO e bit a bit deterministico (o vocoder do modelo usa ruido sem semente; a duracao varia ~10 ms
entre execucoes): o WAV guardado no projeto e a fonte da verdade, regerar produz uma variacao equivalente.
Sem fonemas/duracoes no kokoro-onnx: o lip-sync de falas Kokoro usa o fallback por energia.
"""
import argparse
import json
import sys
import wave
from pathlib import Path

import numpy as np


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    ap = argparse.ArgumentParser()
    ap.add_argument("--text", help="texto a falar")
    ap.add_argument("--voice", default="pf_dora")
    ap.add_argument("--lang", default="pt-br", help="pt-br, en-us, en-gb, ...")
    ap.add_argument("--speed", type=float, default=1.0)
    ap.add_argument("--out", help="WAV de saida")
    ap.add_argument("--model-dir", default=str(root / "sidecars" / "kokoro"))
    ap.add_argument("--list-voices", action="store_true")
    a = ap.parse_args()

    from kokoro_onnx import Kokoro

    md = Path(a.model_dir)
    model, voices = md / "kokoro-v1.0.onnx", md / "voices-v1.0.bin"
    for f in (model, voices):
        if not f.is_file():
            print(f"arquivo ausente: {f}", file=sys.stderr)
            return 2
    k = Kokoro(str(model), str(voices))
    if a.list_voices:
        print(json.dumps(sorted(k.get_voices()), ensure_ascii=False))
        return 0
    if not a.text or not a.out:
        ap.error("--text e --out sao obrigatorios")
    samples, sr = k.create(a.text, voice=a.voice, speed=a.speed, lang=a.lang)
    pcm = (np.clip(samples, -1.0, 1.0) * 32767.0).round().astype("<i2")
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    with wave.open(a.out, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(int(sr))
        w.writeframes(pcm.tobytes())
    print(json.dumps({"out": a.out, "sample_rate": int(sr), "seconds": len(pcm) / sr, "voice": a.voice, "lang": a.lang}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
