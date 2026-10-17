"""Som do curta: SFX por corte (procedurais) e uma trilha musical original (partitura escrita aqui, renderizada pelo
aiai_bgm) colocada em cada corte com trim_start = início do corte na edição, para soar contínua; a música para no gag
(C005) e volta no C006."""
from curta import d, ids, save_ids

FPS = 24000 / 1001
cuts = ids["cuts"]
durs = [d.query({"kind": "node", "id": c})["data"]["duration"] for c in cuts]
starts = [sum(durs[:i]) for i in range(len(durs))]
print("durações:", durs, "total", sum(durs), "quadros =", round(sum(durs) / FPS, 2), "s")


def clear_audio(cut, tracks):
    data = d.query({"kind": "node", "id": cut})["data"]
    keep = [a for a in data.get("audio", []) if a.get("track", "dialogue") not in tracks]
    d.apply({"kind": "set_props", "id": cut, "data": {"audio": keep}})


for c in cuts:
    clear_audio(c, ("sfx", "bgm", "ambience"))

# ---------- SFX ----------
SFX = [
    (0, "wind", 0, {"duration": 4.5, "intensity": 0.35, "brightness": 0.3, "gain_db": -16, "fade_in": 0.8, "fade_out": 0.8, "seed": 3}),
    (0, "chime", 70, {"duration": 2.0, "pitch": 1.6, "gain_db": -20, "seed": 4}),
    (1, "cloth", 6, {"duration": 0.6, "gain_db": -12, "seed": 5}),
    (1, "cloth", 14, {"duration": 0.5, "gain_db": -13, "seed": 6, "pan": 0.4}),
    (1, "pop", 33, {"duration": 0.25, "pitch": 1.4, "gain_db": -10, "seed": 7}),
    (4, "impact", 0, {"duration": 0.9, "intensity": 0.9, "gain_db": -6, "seed": 8}),
    (4, "whoosh", 0, {"duration": 0.5, "pitch": 1.3, "gain_db": -12, "seed": 9}),
    (5, "chime", 4, {"duration": 2.5, "pitch": 1.8, "gain_db": -18, "seed": 10, "pan": -0.5}),
    (6, "wind", 8, {"duration": 4.8, "intensity": 0.8, "brightness": 0.55, "gain_db": -10, "fade_in": 0.3, "fade_out": 1.2, "seed": 11}),
    (6, "magic_swell", 30, {"duration": 3.0, "gain_db": -18, "seed": 12}),
]
for ci, preset, f, kw in SFX:
    r = d.call("SFX", {"preset": preset, "cut": cuts[ci], "start_frame": f, **kw})
    print(f"C00{ci + 1} {preset} @{f}: {r.get('duration_s', '?'):.2f} s" if isinstance(r.get("duration_s"), float) else (ci, preset, r))

# ---------- BGM: partitura original (F maior, 84 bpm, 4/4) ----------
BPM, BARS = 84, 11
chords = [("F", [53, 57, 60]), ("C/E", [52, 55, 60]), ("Dm", [50, 53, 57]), ("Bb", [46, 50, 53]),
          ("F", [53, 57, 60]), ("C", [48, 52, 55]), ("Bb", [46, 50, 53]), ("C", [48, 52, 55]),
          ("Dm", [50, 53, 57]), ("Bb", [46, 50, 53]), ("F", [53, 57, 60])]
melody = [  # (compasso, tempo no compasso, nota MIDI, duração em tempos)
    (0, 0, 72, 1.5), (0, 1.5, 74, 0.5), (0, 2, 72, 1), (0, 3, 69, 1),
    (1, 0, 67, 2), (1, 2, 69, 1), (1, 3, 72, 1),
    (2, 0, 74, 1.5), (2, 1.5, 72, 0.5), (2, 2, 69, 2),
    (3, 0, 70, 2), (3, 2, 69, 1), (3, 3, 67, 1),
    (4, 0, 65, 1), (4, 1, 69, 1), (4, 2, 72, 2),
    (5, 0, 76, 1.5), (5, 1.5, 74, 0.5), (5, 2, 72, 2),
    (6, 0, 74, 1), (6, 1, 72, 1), (6, 2, 70, 1), (6, 3, 69, 1),
    (7, 0, 67, 3), (7, 3, 72, 1),
    (8, 0, 74, 2), (8, 2, 77, 2),
    (9, 0, 76, 1), (9, 1, 74, 1), (9, 2, 72, 1), (9, 3, 70, 1),
    (10, 0, 72, 4),
]
piano, strings, bass = [], [], []
for b, (_, ch) in enumerate(chords):
    t0 = 4 * b
    strings.append([{"pitch": p, "start_beat": t0, "length_beats": 4, "velocity": 52} for p in ch])
    bass.append({"pitch": ch[0] - 12, "start_beat": t0, "length_beats": 2, "velocity": 70})
    bass.append({"pitch": ch[0] - 12 + 7, "start_beat": t0 + 2, "length_beats": 2, "velocity": 62})
    arp = [ch[0], ch[1], ch[2], ch[1] + 12, ch[2], ch[1], ch[2], ch[0] + 12]
    for k, p in enumerate(arp):
        piano.append({"pitch": p, "start_beat": t0 + 0.5 * k, "length_beats": 0.6, "velocity": 48 + 8 * (k % 2 == 0)})
mel = [{"pitch": n, "start_beat": 4 * b + t, "length_beats": l, "velocity": 82} for b, t, n, l in melody]
song = {"tempo_bpm": BPM, "beats_per_bar": 4, "bars": BARS, "looped": False, "tracks": [
    {"name": "melodia", "program": 73, "volume": 0.75, "pan": 0.1, "reverb": 0.35, "notes": mel},           # flauta
    {"name": "piano", "program": 0, "volume": 0.6, "pan": -0.15, "reverb": 0.3, "notes": piano},
    {"name": "cordas", "program": 48, "volume": 0.45, "pan": 0.0, "reverb": 0.45, "notes": [n for c in strings for n in c]},
    {"name": "baixo", "program": 32, "volume": 0.55, "pan": 0.0, "reverb": 0.1, "notes": bass},
]}
for tr in song["tracks"]:  # canal MIDI: volume, pan e reverb em 0..127
    tr["volume"], tr["pan"], tr["reverb"] = round(tr["volume"] * 127), round(64 + tr["pan"] * 63), round(tr["reverb"] * 127)
r = d.call("BGM", {"song": song, "cut": cuts[0], "start_frame": 0, "gain_db": -14, "fade_in": 1.0})
ids["bgm"] = r["asset"]
save_ids()
print("BGM:", r.get("engine"), round(r.get("duration_s", 0), 2), "s")
# a mesma música nos outros cortes, continuando de onde parou (sem a música no gag)
clear_audio(cuts[0], ("bgm",))
for i, c in enumerate(cuts):
    if i == 4:
        continue
    data = d.query({"kind": "node", "id": c})["data"]
    audio = data.get("audio", [])
    pl = {"id": f"bgm_c{i + 1}", "asset": ids["bgm"], "start_frame": 0, "track": "bgm", "trim_start": starts[i] / FPS,
          "duration": durs[i] / FPS, "gain_db": -14 if i != 6 else -12}
    if i == 0:
        pl["fade_in"] = 1.0
    if i == 3:
        pl["fade_out"] = 0.15  # corta seco para o gag
    if i == 5:
        pl["fade_in"] = 0.6
    if i == 6:
        pl["fade_out"] = 2.0
    d.apply({"kind": "set_props", "id": c, "data": {"audio": audio + [pl]}})
# loudness: conferido na edição inteira (aiai_edit_mix, c17_edicao.py); MIX por corte exige o app aberto
