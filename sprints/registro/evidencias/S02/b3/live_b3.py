"""Live check B3: gag shot (gag BG + focus lines + ドン + petals + SFX) via `aiai-mcp call` headless; exports 2 s MP4."""
import json, subprocess, sys, tempfile, os, shutil

BIN = r"C:\AnimeIAI\app\target\release\aiai-mcp.exe"
EVID = r"C:\AnimeIAI\sprints\registro\evidencias\S02\b3"
os.makedirs(EVID, exist_ok=True)
proj = os.path.join(tempfile.gettempdir(), "aiai-b3-live.aiai")
shutil.rmtree(proj, ignore_errors=True)

def call(action, params=None, extra=()):
    r = subprocess.run([BIN, "call", action, json.dumps(params or {}), "--project", proj, "--timeout", "600", *extra], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        print("FALHA", action, r.stderr)
        sys.exit(1)
    return json.loads(r.stdout)

def node(name, parent, data):
    c = {"kind": "create_node", "name": name, "data": data}
    if parent:
        c["parent"] = parent
    return call("APPLY_COMMAND", {"command": c})["created"][0]

call("NEW_PROJECT", {"name": "Gag B3", "settings": {"width": 960, "height": 540}})
ep = node("EP01", None, {"type": "episode", "number": 1})
sc = node("Cena", ep, {"type": "scene", "number": 1})
cut = node("C001", sc, {"type": "cut", "number": 1, "duration": 48})
bg = node("FUNDO", cut, {"type": "layer", "kind": "raster"})
pet = node("PETALAS", cut, {"type": "layer", "kind": "particles"})
lin = node("FOCO", cut, {"type": "layer", "kind": "vector"})
txt = node("ONOMATOPEIA", cut, {"type": "layer", "kind": "raster"})
out = node("CONTORNO", cut, {"type": "layer", "kind": "vector"})
for l in (bg, lin, txt, out):
    node("c1", l, {"type": "cel", "number": 1})

call("GAG_BG", {"cut": cut, "layer": bg, "pattern": {"type": "radial_stripes", "center": [480.0, 270.0], "count": 28, "colors": ["#fff6c2", "#ffcf3a"]}})
call("LINES", {"cut": cut, "layer": lin, "kind": {"type": "focus", "center": [480.0, 270.0], "clear": [230.0, 150.0]}, "count": 110, "length": [120.0, 700.0], "width": [2.0, 9.0], "color": {"type": "direct", "rgba": [0.05, 0.03, 0.05, 1]}, "seed": 11})
call("TEXT", {"cut": cut, "fill_layer": txt, "outline_layer": out, "text": "ドン", "size": 330.0, "center": [480.0, 285.0], "rotation": -6.0,
              "color": {"type": "direct", "rgba": [1.0, 0.18, 0.12, 1]}, "outline_width": 14.0, "outline_color": {"type": "direct", "rgba": [0.05, 0.02, 0.05, 1]}, "roughness": 4.0, "seed": 3})
p = call("PARTICLES", {"layer": pet, "preset": "petalas", "overrides": {"rate": 45.0, "size": [14.0, 30.0]}, "seed": 7})
print("particles", p["particles"])
s = call("SFX", {"preset": "impact", "seed": 4, "cut": cut, "start_frame": 6, "gain_db": -2.0})
print("sfx", s["duration_s"], s["placement"])
mix = call("MIX_CUT", {"cut": cut, "preset": "streaming"})
print("mix loudness", mix["loudness"]["integrated_lufs"], mix["normalize"]["after_lufs"])

# frame PNG (frame 20: petals already falling)
r = call("RENDER", {"cut": cut, "frame": 20, "background": "#ffffff"}, ["--png", os.path.join(EVID, "gag_q20.png")])
print("render", r["width"], r["height"], r["warnings"])
mp4 = os.path.join(EVID, "gag_2s.mp4")
e = call("EXPORT_VIDEO", {"cut": cut, "out": mp4, "crf": 26, "preset": "streaming"})
print("export", json.dumps({k: e[k] for k in ("frames", "expected_frames", "size", "probe", "audio", "warnings", "elapsed_s")}, ensure_ascii=False))
shutil.copytree(proj, os.path.join(EVID, "gag.aiai.json-only"), ignore=shutil.ignore_patterns("blobs", "*.lz4"), dirs_exist_ok=True) if False else None
print("mp4 bytes", os.path.getsize(mp4))
