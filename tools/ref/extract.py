#!/usr/bin/env python3
"""Extracao e catalogo do episodio de referencia (somente calibracao; saidas em calibracao/, fora do git).

Uso (a partir de C:\\AnimeIAI):
    uv run --project tools python tools/ref/extract.py detect            # catalogo de cortes -> calibracao/ref/cortes.json + miniaturas
    uv run --project tools python tools/ref/extract.py sheets            # folhas de contato do episodio inteiro
    uv run --project tools python tools/ref/extract.py --cut 12          # quadros PNG + audio WAV + exposure.json + folha de contato do corte 12
    (--cut N equivale ao subcomando "cut N"; --video para outro arquivo)

Deteccao de cortes (duas fontes, depois refinamento):
  1. ffmpeg: filtro `select='gt(scene,0.30)'` + `showinfo`/`metadata` (escore de cena do ffmpeg), reduzido a 320x180.
  2. Em paralelo, os quadros sao lidos a 160x90 RGB e para cada par consecutivo calculamos:
       - diferenca de histograma (16 bins por canal, L1/2, em [0,1]);
       - diferenca absoluta media de luma (0..255).
  Um corte no quadro i (primeiro quadro do novo plano) e aceito se
       (ffmpeg scene >= 0.30  E  hist >= 0.10)   -> concordancia das duas fontes
    OU hist >= 0.45                              -> mudanca de histograma inequivoca mesmo que o ffmpeg tenha falhado.
  Refinamento:
    - flash/blink: se o quadro i-1 e o quadro i+1 sao parecidos (hist(i-1,i+1) < 0.5*hist(i-1,i)), o "corte" e um
      clarao de 1 quadro e e rejeitado (no corte real i-1 e i+1 tambem diferem).
    - planos com menos de MIN_SHOT frames (4) sao fundidos ao vizinho (clarao/ruido de compressao).
  A conferencia final e visual (miniaturas + folhas de contato).

Mapa de exposicao (--cut): para cada quadro (exceto o primeiro) diz se o DESENHO mudou em relacao ao quadro anterior.
  Metrica: luma a 960x540, suavizada com gaussiana sigma=1.2 (mata grao e ruido de compressao do HEVC);
  fracao de pixels com |diff| > PIX_THR (10 niveis em 255). O quadro e "novo" se essa fracao >= FRAC_THR (0.15% = ~780 px).
  Os valores brutos (frac, mad) vao para exposure.json para o limiar ser auditado; em reposicao de desenho
  (anime em 2s/3s) a fracao cai para ~0 (ruido) e nos quadros novos fica muito acima do limiar (distribuicao bimodal).
  Saida tambem inclui o "padrao" (comprimento de cada exposicao, ex.: 2,2,3,2...) e sua moda (1s/2s/3s).
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import glob
import json
import math
import os
import re
import subprocess
import sys
import threading
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(r"C:\AnimeIAI")
CAL = ROOT / "calibracao" / "ref"
FPS_NUM, FPS_DEN = 24000, 1001
FPS = FPS_NUM / FPS_DEN

SCENE_THR = 0.30
HIST_CONFIRM = 0.10
HIST_ALONE = 0.45
MIN_SHOT = 4
PIX_THR = 10
FRAC_THR = 0.0015
SMALL_W, SMALL_H = 160, 90


def find_video(arg: str | None) -> Path:
    if arg:
        return Path(arg)
    vids = sorted(glob.glob(str(ROOT / "anime de referencia" / "*.mkv")))
    if not vids:
        sys.exit("nenhum .mkv em 'anime de referencia'")
    return Path(vids[0])


def frame_time(n: int) -> float:
    return n * FPS_DEN / FPS_NUM


# ------------------------------------------------------------------ deteccao
def ffmpeg_scene_scores(video: Path, thr: float) -> dict[float, float]:
    """Roda `select=gt(scene,thr)` + showinfo + metadata; devolve {pts_time: scene_score}."""
    vf = f"scale=320:180:flags=fast_bilinear,select='gt(scene,{thr})',metadata=print:file=-:direct=1,showinfo"
    cmd = ["ffmpeg", "-hide_banner", "-nostats", "-i", str(video), "-map", "0:v:0", "-an", "-sn", "-vf", vf, "-f", "null", "-"]
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    out: dict[float, float] = {}
    last_pts = None
    for line in (p.stdout + "\n" + p.stderr).splitlines():
        m = re.search(r"pts_time:([0-9.]+)", line)
        if m and "frame:" in line:
            last_pts = float(m.group(1))
            continue
        m = re.search(r"lavfi\.scene_score=([0-9.]+)", line)
        if m and last_pts is not None:
            out[last_pts] = float(m.group(1))
            last_pts = None
    return out


def read_small_frames(video: Path):
    """Gera (indice, ndarray HxWx3 uint8) a 160x90 para todo o video."""
    vf = f"scale={SMALL_W}:{SMALL_H}:flags=area,format=rgb24"
    cmd = ["ffmpeg", "-hide_banner", "-nostats", "-loglevel", "error", "-i", str(video), "-map", "0:v:0", "-an", "-sn",
           "-vf", vf, "-fps_mode", "passthrough", "-f", "rawvideo", "-"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, bufsize=1 << 24)
    size = SMALL_W * SMALL_H * 3
    i = 0
    while True:
        buf = p.stdout.read(size)
        if len(buf) < size:
            break
        yield i, np.frombuffer(buf, np.uint8).reshape(SMALL_H, SMALL_W, 3)
        i += 1
    p.wait()


def hist48(f: np.ndarray) -> np.ndarray:
    h = np.concatenate([np.bincount((f[..., c] >> 4).ravel(), minlength=16) for c in range(3)]).astype(np.float32)
    return h / h.sum()  # soma total 1 (3 canais x 16 bins)




def detect(video: Path, force: bool = False) -> list[dict]:
    CAL.mkdir(parents=True, exist_ok=True)
    cache = CAL / "framestats.npz"
    if cache.exists() and not force:
        z = np.load(cache)
        hd, mad, lum_means, hists = z["hd"], z["mad"], z["lum"], z["hists"]
        scene = json.loads((CAL / "scene_scores.json").read_text())
        scene = {float(k): v for k, v in scene.items()}
    else:
        result: dict = {}
        t = threading.Thread(target=lambda: result.setdefault("scene", ffmpeg_scene_scores(video, 0.15)))
        t.start()
        hists_l, mad_l, lum_l = [], [], []
        prev = None
        for i, f in read_small_frames(video):
            h = hist48(f)
            hists_l.append(h)
            y = f.astype(np.float32) @ np.array([0.2126, 0.7152, 0.0722], np.float32)
            lum_l.append(float(y.mean()))
            mad_l.append(0.0 if prev is None else float(np.abs(y - prev).mean()))
            prev = y
            if i % 5000 == 0:
                print(f"  quadros lidos: {i}", flush=True)
        t.join()
        hists = np.stack(hists_l)
        hd = np.zeros(len(hists), np.float32)
        hd[1:] = np.abs(hists[1:] - hists[:-1]).sum(axis=1) * 0.5
        mad = np.array(mad_l, np.float32)
        lum_means = np.array(lum_l, np.float32)
        scene = result["scene"]
        np.savez_compressed(cache, hd=hd, mad=mad, lum=lum_means, hists=hists)
        (CAL / "scene_scores.json").write_text(json.dumps({str(k): v for k, v in scene.items()}))
    n = len(hd)
    print(f"{n} quadros ({n / FPS:.1f} s); candidatos ffmpeg scene>0.15: {len(scene)}")

    # mapa quadro -> score do ffmpeg
    fscore = np.zeros(n, np.float32)
    for t_, s in scene.items():
        k = int(round(t_ * FPS))
        if 0 <= k < n:
            fscore[k] = s
    # o ffmpeg reporta o pts do primeiro quadro novo; tolerar +-1 quadro
    fs2 = np.maximum(fscore, np.maximum(np.roll(fscore, 1), np.roll(fscore, -1)) * 0.0)

    cuts = [0]
    for i in range(1, n):
        a = (fs2[i] >= SCENE_THR and hd[i] >= HIST_CONFIRM) or hd[i] >= HIST_ALONE
        if not a:
            continue
        # flash/blink: i-1 e i+1 parecidos
        if 0 < i < n - 1:
            d_skip = float(np.abs(hists[i + 1] - hists[i - 1]).sum() * 0.5)
            if d_skip < 0.5 * hd[i] and hd[i + 1] >= 0.5 * hd[i]:
                continue
        cuts.append(i)
    # fundir planos curtos demais
    merged = [cuts[0]]
    for c in cuts[1:]:
        if c - merged[-1] < MIN_SHOT:
            # mantem o corte mais forte dos dois
            if hd[c] > hd[merged[-1]] and len(merged) > 1:
                merged[-1] = c
            continue
        merged.append(c)
    bounds = merged + [n]
    shots = []
    for k in range(len(merged)):
        s, e = bounds[k], bounds[k + 1] - 1
        shots.append({
            "index": k,
            "start_frame": int(s),
            "end_frame": int(e),
            "frames": int(e - s + 1),
            "start_time": round(frame_time(s), 4),
            "duration_s": round((e - s + 1) * FPS_DEN / FPS_NUM, 4),
            "thumb": f"thumbs/{k:04d}.jpg",
            "cut_hist": round(float(hd[s]), 4),
            "cut_scene": round(float(fs2[s]), 4),
        })
    return shots


def make_thumbs(video: Path, shots: list[dict], workers: int = 6):
    tdir = CAL / "thumbs"
    tdir.mkdir(parents=True, exist_ok=True)

    def one(sh):
        out = CAL / sh["thumb"]
        if out.exists():
            return
        mid = sh["start_frame"] + min(sh["frames"] - 1, max(1, sh["frames"] // 2))
        t = frame_time(mid) + 0.001
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-ss", f"{t:.4f}", "-i", str(video), "-frames:v", "1",
                        "-vf", "scale=320:-2:flags=area", "-q:v", "3", "-y", str(out)], check=False)

    with cf.ThreadPoolExecutor(workers) as ex:
        list(ex.map(one, shots))


def contact_sheet(items: list[tuple[str, Path]], out: Path, cols: int, tw: int = 320, label_h: int = 18):
    """items = [(rotulo, caminho_da_imagem)]"""
    th = int(tw * 9 / 16)
    rows = math.ceil(len(items) / cols)
    sheet = Image.new("RGB", (cols * tw, rows * (th + label_h)), (20, 20, 20))
    d = ImageDraw.Draw(sheet)
    for k, (label, p) in enumerate(items):
        r, c = divmod(k, cols)
        x, y = c * tw, r * (th + label_h)
        try:
            im = Image.open(p).convert("RGB").resize((tw, th), Image.LANCZOS)
            sheet.paste(im, (x, y + label_h))
        except Exception:
            pass
        d.text((x + 3, y + 3), label, fill=(255, 255, 0))
    out.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out, quality=85)


def fmt_t(t: float) -> str:
    return f"{int(t // 60)}:{t % 60:05.2f}"


def cmd_detect(a):
    video = find_video(a.video)
    shots = detect(video, force=a.force)
    (CAL / "cortes.json").write_text(json.dumps(shots, indent=1, ensure_ascii=False), encoding="utf-8")
    print(f"{len(shots)} cortes -> {CAL / 'cortes.json'}")
    make_thumbs(video, shots)
    cmd_sheets(a, shots)


def cmd_sheets(a, shots=None):
    if shots is None:
        shots = json.loads((CAL / "cortes.json").read_text(encoding="utf-8"))
    per = 80
    for k in range(0, len(shots), per):
        chunk = shots[k:k + per]
        items = [(f"#{s['index']} {fmt_t(s['start_time'])} {s['duration_s']:.1f}s", CAL / s["thumb"]) for s in chunk]
        out = CAL / "sheets" / f"episodio_{k // per:02d}.jpg"
        contact_sheet(items, out, cols=8, tw=240)
    print("folhas:", CAL / "sheets")


# ------------------------------------------------------------------ corte
def cmd_cut(a):
    video = find_video(a.video)
    shots = json.loads((CAL / "cortes.json").read_text(encoding="utf-8"))
    sh = shots[a.cut]
    d = CAL / f"cut_{a.cut:03d}"
    fdir = d / "frames"
    fdir.mkdir(parents=True, exist_ok=True)
    n = sh["frames"]
    # mkv guarda pts em ms (arredondado a +-0,5 ms): buscar 5 ms ANTES do instante nominal garante pts >= ss para este quadro
    # e exclui o anterior (41,7 ms antes). (Meio quadro DEPOIS pularia o primeiro quadro: bug visto no corte 138.)
    t0 = frame_time(sh["start_frame"]) - 0.005
    if not a.skip_frames:
        for f in fdir.glob("*.png"):
            f.unlink()
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-ss", f"{t0:.5f}", "-i", str(video), "-map", "0:v:0", "-frames:v", str(n),
                        "-vf", f"setpts=N/({FPS_NUM}/{FPS_DEN})/TB", "-fps_mode", "passthrough", "-y", str(fdir / "f_%04d.png")], check=True)
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-ss", f"{t0:.5f}", "-t", f"{n * FPS_DEN / FPS_NUM:.5f}",
                        "-i", str(video), "-map", "0:a:0", "-vn", "-c:a", "pcm_s16le", "-y", str(d / "audio.wav")], check=True)
    files = sorted(fdir.glob("f_*.png"))
    print(f"corte {a.cut}: {len(files)} quadros extraidos (esperado {n})")
    exp = exposure(files)
    exp["cut"] = a.cut
    exp["start_frame"] = sh["start_frame"]
    exp["start_time"] = sh["start_time"]
    (d / "exposure.json").write_text(json.dumps(exp, indent=1), encoding="utf-8")
    print("padrao de exposicao:", exp["pattern"], "moda:", exp["mode_hold"], "| novos desenhos:", exp["n_new_drawings"], "de", len(files))
    # folha de contato (tudo ou amostra de no maximo 96)
    step = max(1, math.ceil(len(files) / 96))
    items = [(f"f{i}{'*' if i == 0 or exp['changed'][i] else ''}", files[i]) for i in range(0, len(files), step)]
    cols = 8
    contact_sheet(items, d / "contact.jpg", cols=cols, tw=240)


def exposure(files: list[Path]) -> dict:
    import cv2
    prev = None
    fracs, mads, changed, bboxes = [], [], [], []
    for i, f in enumerate(files):
        im = cv2.imread(str(f), cv2.IMREAD_COLOR)
        y = cv2.cvtColor(im, cv2.COLOR_BGR2GRAY)
        y = cv2.resize(y, (960, 540), interpolation=cv2.INTER_AREA)
        y = cv2.GaussianBlur(y, (0, 0), 1.2).astype(np.float32)
        if prev is None:
            fracs.append(None)
            mads.append(None)
            changed.append(True)
            bboxes.append(None)
        else:
            dd = np.abs(y - prev)
            fr = float((dd > PIX_THR).mean())
            fracs.append(round(fr, 6))
            mads.append(round(float(dd.mean()), 4))
            changed.append(bool(fr >= FRAC_THR))
            ys, xs = np.where(dd > PIX_THR)
            # caixa (x0,y0,x1,y1) em pixels de 1920x1080 da regiao que mudou (onde o desenho se moveu)
            bboxes.append([int(xs.min()) * 2, int(ys.min()) * 2, int(xs.max()) * 2 + 1, int(ys.max()) * 2 + 1] if len(xs) and fr >= FRAC_THR else None)
        prev = y
    # padrao: duracao de cada exposicao
    starts = [i for i, c in enumerate(changed) if c]
    lens = [(starts[k + 1] if k + 1 < len(starts) else len(files)) - starts[k] for k in range(len(starts))]
    mode = max(set(lens), key=lens.count) if lens else None
    return {
        "method": f"luma 960x540 gauss sigma 1.2; changed if frac(|diff|>{PIX_THR}) >= {FRAC_THR}",
        "pix_thr": PIX_THR, "frac_thr": FRAC_THR,
        "frames": len(files),
        "changed": changed,
        "frac": fracs, "mad": mads, "change_bbox": bboxes,
        "drawing_starts": starts,
        "pattern": lens,
        "mode_hold": mode,
        "n_new_drawings": len(starts),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", nargs="?", choices=["detect", "sheets", "cut"], default=None)
    ap.add_argument("cut_arg", nargs="?", type=int)
    ap.add_argument("--cut", type=int, help="extrai quadros/audio/exposicao do corte N")
    ap.add_argument("--video")
    ap.add_argument("--force", action="store_true", help="refaz a leitura de todos os quadros")
    ap.add_argument("--skip-frames", action="store_true", help="--cut: nao reextrai, so recalcula exposure")
    a = ap.parse_args()
    if a.command == "cut":
        a.cut = a.cut_arg
    if a.cut is not None:
        return cmd_cut(a)
    if a.command == "sheets":
        return cmd_sheets(a)
    return cmd_detect(a)


if __name__ == "__main__":
    main()
