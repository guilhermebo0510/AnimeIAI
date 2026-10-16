"""Gráficos de evidência da S17 (rodar depois de `cargo test -p aiai-tools --test audio s17_evidence -- --ignored`).

Uso: python plot_s17.py    (lê sidecars/out/s17/*.wav e os JSON desta pasta; grava os PNG aqui)
Também roda o FFmpeg (processo externo) com o filtro ebur128 como conferência independente do medidor do app.
"""
import json
import re
import subprocess
import sys
import wave
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np

matplotlib.rcParams["font.sans-serif"] = ["Yu Gothic", "Meiryo", "MS Gothic", "Segoe UI", "DejaVu Sans"]
matplotlib.rcParams["font.family"] = "sans-serif"

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
OUT = ROOT / "sidecars" / "out" / "s17"


def read_wav(p):
    with wave.open(str(p), "rb") as w:
        n, ch, sw, sr = w.getnframes(), w.getnchannels(), w.getsampwidth(), w.getframerate()
        raw = w.readframes(n)
    assert sw == 2, "esperado PCM16"
    x = np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0
    return sr, x.reshape(-1, ch)


def main():
    meas = json.loads((HERE / "medicoes.json").read_text(encoding="utf-8"))
    loud = json.loads((HERE / "loudness_relatorio.json").read_text(encoding="utf-8"))
    ls = json.loads((HERE / "lipsync_quadros.json").read_text(encoding="utf-8"))
    sr, mix = read_wav(OUT / "mix_normalizada_48k.wav")
    t = np.arange(mix.shape[0]) / sr
    fps = ls["fps"]
    stems = {k: read_wav(OUT / f"stem_{k}.wav")[1] for k in ("fala", "sfx", "bgm")}

    fig, ax = plt.subplots(5, 1, figsize=(14, 16), constrained_layout=True)
    # 1) forma de onda da mix com as posições dos elementos
    ax[0].plot(t, mix[:, 0], lw=0.4, color="#1f77b4", label="L")
    ax[0].plot(t, mix[:, 1], lw=0.4, color="#d62728", alpha=0.6, label="R")
    ax[0].axhline(10 ** (-1 / 20), color="k", ls=":", lw=0.8)
    ax[0].axhline(-(10 ** (-1 / 20)), color="k", ls=":", lw=0.8)
    for name, f, c in [("fala 1", ls["fala1"]["inicio"], "#2ca02c"), ("fala 2", ls["fala2"]["inicio"], "#2ca02c"), ("kira", ls["kira"], "#ff7f0e")] + [("passo", p, "#9467bd") for p in ls["passos"]]:
        ax[0].axvline(f / fps, color=c, lw=1.0, ls="--")
        ax[0].text(f / fps, 0.93, name, rotation=90, va="top", ha="right", fontsize=8, color=c)
    ax[0].set_title("Mix final normalizada (PCM16, 48 kHz) — linhas pontilhadas: ±(-1 dBFS)")
    ax[0].set_ylim(-1, 1)
    ax[0].legend(loc="lower right")
    # 2) trilhas separadas (envelope RMS 50 ms em dBFS)
    win = int(sr * 0.05)
    for k, c in [("fala", "#2ca02c"), ("sfx", "#ff7f0e"), ("bgm", "#1f77b4")]:
        x = stems[k].mean(axis=1)
        n = len(x) // win
        rms = np.sqrt((x[: n * win].reshape(n, win) ** 2).mean(axis=1) + 1e-12)
        ax[1].plot(np.arange(n) * 0.05, 20 * np.log10(rms), color=c, label=k, lw=1.0)
    ax[1].set_ylim(-80, 0)
    ax[1].set_ylabel("dBFS (RMS 50 ms)")
    ax[1].set_title("Trilhas antes do ducking/normalização (mesmos ganhos)")
    ax[1].legend()
    # 3) loudness momentâneo e curto prazo + integrado
    rep = loud["final_arquivo_pcm16"]
    m = np.array([np.nan if v is None else v for v in rep["momentary_series"]])
    s = np.array([np.nan if v is None else v for v in rep["short_term_series"]])
    ax[2].plot(np.arange(len(m)) * 0.1 + 0.4, m, color="#1f77b4", label="momentâneo (400 ms)")
    ax[2].plot(np.arange(len(s)) * 0.1 + 3.0, s, color="#d62728", label="curto prazo (3 s)")
    ax[2].axhline(rep["integrated_lufs"], color="k", lw=1.2, label=f"integrado {rep['integrated_lufs']:.2f} LUFS")
    ax[2].axhline(-24, color="#2ca02c", ls="--", lw=1, label="alvo -24 LKFS (ARIB)")
    ax[2].axhspan(-25, -23, color="#2ca02c", alpha=0.12)
    ax[2].set_ylim(-60, -10)
    ax[2].set_ylabel("LUFS")
    ax[2].set_title(f"Loudness BS.1770 — true peak {rep['true_peak_dbtp']:.2f} dBTP (teto -1), LRA {rep['lra_lu']:.1f} LU")
    ax[2].legend(loc="lower right")
    # 4) espectrograma
    mono = mix.mean(axis=1)
    ax[3].specgram(mono, NFFT=2048, Fs=sr, noverlap=1536, cmap="magma", vmin=-130, vmax=-30)
    ax[3].set_ylim(0, 12000)
    ax[3].set_ylabel("Hz")
    ax[3].set_title("Espectrograma da mix")
    # 5) boca por quadro
    names = ["rest", "n", "a", "i", "u", "e", "o"]
    for row, key, y in [("Haru (fala 1)", "fala1", 1), ("Mio (fala 2)", "fala2", 0)]:
        formas = ls[key]["formas"]
        f0 = ls[key]["inicio"]
        for i, f in enumerate(formas):
            idx = names.index(f) if f in names else 0
            ax[4].add_patch(plt.Rectangle(((f0 + i) / fps, y + 0.05), 1 / fps, 0.9, color=plt.cm.tab10(idx / 10), lw=0))
            if i == 0 or formas[i - 1] != f:
                ax[4].text((f0 + i) / fps + 0.5 / fps, y + 0.5, f if f != "rest" else "", fontsize=8, ha="left", va="center", color="white")
        ax[4].text(0.02, y + 0.5, row, transform=ax[4].get_yaxis_transform(), fontsize=9)
    ax[4].set_xlim(0, t[-1])
    ax[4].set_ylim(0, 2)
    ax[4].set_yticks([])
    ax[4].set_xlabel("tempo (s)")
    ax[4].set_title("Forma de boca por quadro (a/i/u/e/o; ん = n; vazio = repouso)")
    for a in ax[:4]:
        a.set_xlim(0, t[-1])
        a.grid(alpha=0.25)
    fig.savefig(HERE / "mix_analise.png", dpi=90)
    plt.close(fig)

    # Painel das falas: forma de onda + moras + boca (um painel por fala, zoom)
    fig, ax = plt.subplots(2, 1, figsize=(14, 7), constrained_layout=True)
    for a, (key, f0, who) in zip(ax, [("fala1", ls["fala1"]["inicio"], "Haru — VOICEVOX:春日部つむぎ"), ("fala2", ls["fala2"]["inicio"], "Mio — VOICEVOX:雨晴はう")]):
        n = len(ls[key]["formas"])
        t0, t1 = f0 / fps, (f0 + n) / fps
        a0, a1 = int(t0 * sr), int(t1 * sr)
        x = stems["fala"][a0:a1].mean(axis=1)
        a.plot(np.arange(len(x)) / sr + t0, x, lw=0.5, color="#2ca02c")
        for i, f in enumerate(ls[key]["formas"]):
            idx = names.index(f) if f in names else 0
            a.axvspan((f0 + i) / fps, (f0 + i + 1) / fps, ymin=0, ymax=0.1, color=plt.cm.tab10(idx / 10))
            if i == 0 or ls[key]["formas"][i - 1] != f:
                a.text((f0 + i) / fps, 0.0, f if f != "rest" else "·", fontsize=9, transform=a.get_xaxis_transform(), va="bottom")
        a.set_title(who)
        a.set_xlim(t0, t1)
    fig.savefig(HERE / "falas_lipsync.png", dpi=90)
    plt.close(fig)

    # Conferência independente: FFmpeg ebur128 (processo externo, só medição).
    try:
        r = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-i", str(OUT / "mix_normalizada_48k.wav"), "-af", "ebur128=peak=true", "-f", "null", "-"], capture_output=True, text=True, timeout=120)
        txt = r.stderr
        i = txt.rfind("Summary:")
        (HERE / "ffmpeg_ebur128.txt").write_text(txt[i:] if i >= 0 else txt[-2000:], encoding="utf-8")
        print((txt[i:] if i >= 0 else txt[-1500:]))
    except Exception as e:  # noqa: BLE001
        print("ffmpeg indisponível:", e, file=sys.stderr)


if __name__ == "__main__":
    main()
