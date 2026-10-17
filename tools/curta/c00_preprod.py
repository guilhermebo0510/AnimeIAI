"""Pré-produção: projeto, bíblia, beats, roteiro (JA + PT) com tempo, personagens, paletas de settei e perfis de voz."""
import os
from curta import d, ids, node, save_ids, PROJ, rgba

if not os.path.exists(PROJ + "/project.json"):
    d.call("NEW_PROJECT", {"name": "最後の一輪 — A última rosa", "path": PROJ,
                           "settings": {"width": 1920, "height": 1080, "fps_num": 24000, "fps_den": 1001}})
d.apply({"kind": "set_project_props", "patch": {
    "settings": {"dialogue_language": "ja", "subtitle_language": "pt-BR"},
    "bible": {
        "logline": "Dois jovens disputam a última rosa do jardim e descobrem que querem dá-la à mesma pessoa.",
        "synopsis": "Fim de tarde num jardim. Hana, jardineira, e Ren, aprendiz de confeiteiro, estendem a mão para a mesma rosa, "
                    "a última da estação. Cada um quer levá-la para alguém; descobrem que é a avó que cuidou dos dois. Riem, "
                    "decidem entregá-la juntos, e o vento leva pétalas para o céu.",
        "world": "Vila pequena no interior, fim de verão; jardim de rosas atrás de uma casa de madeira com varanda.",
        "themes": ["gratidão", "rivalidade que vira cumplicidade", "fim de estação"],
        "tone": "comédia leve e calorosa; luz dourada de pôr do sol; um momento de gag no meio",
    }}})

# beats
if "beats" not in ids:
    ids["beats"] = d.call("BEATS", {"op": "create", "name": "Beats"})["doc"]; save_ids()
    d.call("BEATS", {"op": "edit", "doc": ids["beats"], "ops": [
        {"op": "add_act", "name": "Início"}, {"op": "add_act", "name": "Virada"}, {"op": "add_act", "name": "Fim"},
        {"op": "add_beat", "act": 0, "title": "A última rosa", "summary": "Jardim ao pôr do sol; uma rosa sobrou.", "target_seconds": 4},
        {"op": "add_beat", "act": 0, "title": "Duas mãos", "summary": "Hana e Ren tocam a rosa ao mesmo tempo.", "target_seconds": 3},
        {"op": "add_beat", "act": 1, "title": "Disputa", "summary": "Cada um defende seu direito à rosa.", "target_seconds": 7},
        {"op": "add_beat", "act": 1, "title": "Revelação", "summary": "Os dois querem dar à avó — choque cômico.", "target_seconds": 2.5},
        {"op": "add_beat", "act": 2, "title": "Juntos", "summary": "Hana sorri e propõe entregar juntos.", "target_seconds": 5},
        {"op": "add_beat", "act": 2, "title": "Pétalas", "summary": "O vento leva pétalas para o céu.", "target_seconds": 5},
    ]})

# personagens + paletas de settei
CHARS = {
    "hana": ("Hana", "16 anos", 5.5, "Jardineira determinada e impulsiva; cabelo castanho-avermelhado curto com presilha verde, avental verde-musgo.",
             [("pele", "normal", "#f6d5c3"), ("pele", "sombra", "#e0ad9a"), ("cabelo", "normal", "#9b4a32"), ("cabelo", "sombra", "#6e2f20"),
              ("cabelo", "realce", "#c97a5c"), ("olho", "normal", "#3f8a55"), ("presilha", "normal", "#5fae6a"), ("avental", "normal", "#5d7048"),
              ("avental", "sombra", "#435234"), ("blusa", "normal", "#f3efe4"), ("blusa", "sombra", "#d6cfbd"), ("linha", "linha", "#3a2420")]),
    "ren": ("Ren", "17 anos", 6.0, "Aprendiz de confeiteiro gentil e teimoso; cabelo preto bagunçado, bandana branca, avental creme.",
            [("pele", "normal", "#f3d0bb"), ("pele", "sombra", "#d9a690"), ("cabelo", "normal", "#2b2a35"), ("cabelo", "sombra", "#1a1922"),
             ("cabelo", "realce", "#4f5068"), ("olho", "normal", "#b07a2a"), ("bandana", "normal", "#f7f5ef"), ("avental", "normal", "#efe2c4"),
             ("avental", "sombra", "#d2c19c"), ("camisa", "normal", "#6f8fb3"), ("camisa", "sombra", "#526f90"), ("linha", "linha", "#2a2026")]),
}
for key, (name, age, heads, profile, colors) in CHARS.items():
    pal = node(f"pal_{key}", f"Settei {name}", {"type": "palette", "colors": [
        {"name": f"{g} {r}", "group": g, "role": r, "rgba": rgba(h)} for g, r, h in colors]})
    node(f"char_{key}", name, {"type": "character", "profile": profile, "age": age, "heads_tall": heads, "palette": pal})
VOICES = {"hana": (8, "春日部つむぎ"), "ren": (12, "白上虎太郎")}
for key, (sp, nm) in VOICES.items():
    d.call("VOICE_PROFILE", {"character": ids[f"char_{key}"], "speaker": sp, "speaker_name": nm})

# roteiro (JA + PT)
if "script" not in ids:
    H, R = ids["char_hana"], ids["char_ren"]
    s = d.call("SCRIPT", {"op": "create", "name": "Roteiro", "title": "最後の一輪 — A última rosa", "scenes": [
        {"heading": "EXT. JARDIM DE ROSAS — PÔR DO SOL", "elements": [
            {"kind": "action", "text": "夕暮れの庭。季節最後のバラが一輪だけ咲いている。", "translation": "Jardim ao entardecer. Só resta uma rosa da estação."},
            {"kind": "action", "text": "二つの手が同時にバラへ伸び、触れ合う。", "translation": "Duas mãos alcançam a rosa ao mesmo tempo e se tocam."},
            {"kind": "dialogue", "character": H, "speaker": "ハナ", "text": "それ、私が先に見つけたの！", "translation": "Eu vi primeiro!", "emotion": "brava"},
            {"kind": "dialogue", "character": R, "speaker": "レン", "text": "僕だって、ばあちゃんに渡したいんだ。", "translation": "Eu também quero dar pra vovó.", "emotion": "firme"},
            {"kind": "dialogue", "character": H, "speaker": "ハナ", "text": "…ばあちゃん？", "translation": "…Vovó?", "emotion": "surpresa"},
        ]},
        {"heading": "EXT. VARANDA DA CASA — PÔR DO SOL", "elements": [
            {"kind": "action", "text": "ハナがレンの方へ振り向き、笑う。", "translation": "Hana se vira para Ren e sorri."},
            {"kind": "dialogue", "character": H, "speaker": "ハナ", "text": "じゃあ、一緒に渡そう。", "translation": "Então vamos entregar juntos.", "emotion": "carinho"},
            {"kind": "action", "text": "風が吹き、花びらが空へ舞い上がる。", "translation": "O vento sopra e as pétalas sobem ao céu."},
        ]},
    ]})
    ids["script"] = s["doc"]; save_ids()
    d.call("SCRIPT", {"op": "apply_timing", "doc": ids["script"]})
t = d.call("SCRIPT", {"op": "timing", "doc": ids["script"]})
print("roteiro: %.1f s estimados" % t["total_seconds"])
for sc in t["scenes"]:
    print(" ", sc["heading"], round(sc["seconds"], 1), [round(e["seconds"], 2) for e in sc["elements"]])
print(d.call("BEATS", {"op": "read", "doc": ids["beats"]})["total_target_seconds"], "s de beats")
