"""Leitor do banco de ferramentas do Clip Studio Paint (EditImageTool.todb, SQLite) — só leitura.

Para que serve: levantar os NÚMEROS das sub-ferramentas de fábrica (tamanho, dureza, espaçamento, suavização de serrilhado,
estabilização, entrada/saída, espessura/ângulo da ponta, dinâmicas de pressão/velocidade) e reproduzir o COMPORTAMENTO no
nosso motor. Nada de imagem da CELSYS é lido nem copiado: as pontas e texturas de material aparecem só como referência.

Uso:
    python tools/csp/csp_tooldb.py tree [--db CAMINHO]                     árvore de ferramentas (nome, variante)
    python tools/csp/csp_tooldb.py dump "G-pen" [--db CAMINHO] [--all]     parâmetros da sub-ferramenta (JSON)
    python tools/csp/csp_tooldb.py pens [--db CAMINHO] [--out ARQ]         as canetas/marcadores da calibração S25 (JSON)

Formato dos blobs `*Effector` (dinâmica de um parâmetro), deduzido comparando as sub-ferramentas de fábrica (big-endian):
    cabeçalho de 44 ou 48 bytes, int32:
      [0] tamanho do cabeçalho      [1] versão/sinalizadores (0xF0, 0x1F0)
      [2] controles ligados: 0x10 pressão, 0x20 inclinação (provável), 0x40 velocidade, 0x80 aleatório
      [3] mínimo % da pressão       [4] mínimo % da inclinação   [5] mínimo % da velocidade   [6] mínimo % do aleatório
      [7] ?                         [8] bytes da curva da pressão [9] bytes da curva da inclinação
      [10] parâmetro da inclinação (?)                            [11] bytes da curva da velocidade
    depois, as curvas na ordem pressão, inclinação, velocidade; cada uma: int32 12 (cabeçalho), int32 N pontos,
    int32 16 (bytes por ponto) e N pares de double (x, y) em 0..1.
    Evidência: Caneta feltro/Caneta Milli/Caneta rugosa (0x40) afinam no traço rápido das referências; as que só têm 0x10
    não mudam com a velocidade.
`BrushInOutTarget`: int32 12, N, 12 e N trios (alvo, ligado, mínimo %); alvos 1001 = tamanho, 1021 = opacidade (?).
`BrushRotationEffector` é um inteiro de bits: 0x10 direção do traço (?), 0x20 inclinação (?), 0x80 aleatório.
"""
import argparse
import json
import os
import sqlite3
import struct
import sys

DEFAULT_DB = r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\Settings\PAINT\Tool\portuguese\EditImageTool.todb"
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

# nome no banco (pt-BR de fábrica) -> chave da calibração (nomes dos arquivos em calibracao/csp/ref/caneta)
PENS = {
    "G-pen": "g-pen",
    "G-pen real": "real-g-pen",
    "Caneta de mapeamento": "mapping-pen",
    "Caneta nanquin": "turnip-pen",
    "Caligrafia": "calligraphy",
    "Caneta texturizada": "textured-pen",
    "Caneta rugosa": "caneta-rugosa",
    "Caneta Milli": "milli-pen",
    "Caneta feltro": "felt-pen",
    "Caneta de Pixel": "dot-pen",
    "Marcador plano": "flat-marker",
}

CONTROLS = [(0x10, "pressure", 3), (0x20, "tilt", 4), (0x40, "velocity", 5), (0x80, "random", 6)]
INOUT_TARGETS = {1001: "size", 1021: "opacity", 1041: "flow", 1051: "thickness"}


def connect(db):
    if not os.path.exists(db):
        raise SystemExit(f"banco não encontrado: {db}")
    uri = "file:" + db.replace("\\", "/") + "?mode=ro"
    return sqlite3.connect(uri, uri=True)


def decode_effector(b):
    """Dinâmica de um parâmetro: controles ligados com mínimo e curva (ou o blob cru se o formato não bate)."""
    if not isinstance(b, (bytes, bytearray)) or len(b) < 44:
        return None
    hsize = struct.unpack(">i", b[:4])[0]
    if hsize not in (44, 48) or len(b) < hsize:
        return {"raw": bytes(b).hex()}
    h = list(struct.unpack(">%di" % (hsize // 4), b[:hsize]))
    curves, off = [], hsize
    while off + 12 <= len(b):
        rh, n, ps = struct.unpack(">3i", b[off:off + 12])
        if rh != 12 or ps != 16 or n < 0 or off + 12 + 16 * n > len(b):
            return {"header": h, "raw": bytes(b).hex(), "erro": f"curva ilegível no byte {off}"}
        curves.append([list(struct.unpack(">2d", b[off + 12 + 16 * i:off + 28 + 16 * i])) for i in range(n)])
        off += 12 + 16 * n
    # as curvas estão na ordem pressão, inclinação, velocidade, cada uma só se o tamanho dela no cabeçalho for > 0
    sizes = {"pressure": h[8], "tilt": h[9], "velocity": h[11] if len(h) > 11 else 0}
    by_name, k = {}, 0
    for name in ("pressure", "tilt", "velocity"):
        if sizes[name] > 0 and k < len(curves):
            by_name[name] = [[round(x, 4), round(y, 4)] for x, y in curves[k]]
            k += 1
    out = {"flags": h[2], "header": h, "controls": {}}
    for bit, name, idx in CONTROLS:
        c = {"on": bool(h[2] & bit), "min": h[idx] / 100.0}
        if name in by_name:
            c["curve"] = by_name[name]
        if c["on"] or "curve" in c:
            out["controls"][name] = c
    if len(h) > 10:
        out["tilt_param"] = h[10]
    return out


def decode_inout(b):
    if not isinstance(b, (bytes, bytearray)) or len(b) < 16:
        return None
    t = struct.unpack(">%di" % (len(b) // 4), b)
    trips = [t[i:i + 3] for i in range(3, len(t) - 2, 3)]
    return [{"target": INOUT_TARGETS.get(a, a), "on": bool(on), "min": m / 100.0} for a, on, m in trips if on or m]


def decode_pattern_array(b):
    """Lista de materiais (só os identificadores; a imagem fica no banco de materiais e não é lida)."""
    if not isinstance(b, (bytes, bytearray)):
        return None
    # int32 8, N, bytes, ... e os nomes em UTF-16LE (".:Paint106:<uuid>:data:material_0.layer")
    txt = bytes(b[16:]).decode("utf-16-le", errors="ignore")
    ids = ["".join(ch for ch in s if ch.isascii() and ch.isprintable()) for s in txt.replace("\x00", " ").split() if ":" in s]
    return {"bytes": len(b), "materials": ids}


def nodes(c):
    cols = [d[1] for d in c.execute("pragma table_info(Node)")]
    rows = [dict(zip(cols, r)) for r in c.execute("select * from Node")]
    return {r["NodeUuid"]: r for r in rows}


def tree(c):
    by = nodes(c)
    root = c.execute("select RootUuid from Manager").fetchone()[0]
    out = []

    def walk(uuid, depth):
        seen = set()
        while uuid and uuid in by and uuid not in seen:
            seen.add(uuid)
            n = by[uuid]
            out.append({"depth": depth, "name": n["NodeName"], "variant": n["NodeVariantID"], "init_variant": n["NodeInitVariantID"]})
            walk(n["NodeFirstChildUuid"], depth + 1)
            uuid = n["NodeNextUuid"]

    walk(root, 0)
    return out


def variant(c, vid, all_cols=False):
    cols = [d[1] for d in c.execute("pragma table_info(Variant)")]
    r = c.execute("select * from Variant where VariantID=?", (vid,)).fetchone()
    if r is None:
        return None
    out = {}
    for k, v in zip(cols, r):
        if v is None:
            continue
        if k.endswith("Effector") and isinstance(v, bytes):
            d = decode_effector(v)
            if all_cols or (d and (d.get("flags") or any(c.get("on") for c in d.get("controls", {}).values()))):
                out[k] = d
        elif k == "BrushInOutTarget":
            out[k] = decode_inout(v)
        elif k.endswith("PatternImageArray"):
            out[k] = decode_pattern_array(v)
        elif isinstance(v, bytes):
            if all_cols:
                out[k] = {"bytes": len(v), "hex": v[:64].hex()}
        else:
            out[k] = v
    return out


def find(c, name):
    for r in nodes(c).values():
        if r["NodeName"] == name:
            return r
    raise SystemExit(f"sub-ferramenta não encontrada: {name!r}")


def dump(c, name, all_cols=False):
    n = find(c, name)
    return {"name": n["NodeName"], "variant": n["NodeVariantID"], "init_variant": n["NodeInitVariantID"],
            "params": variant(c, n["NodeVariantID"], all_cols), "factory_params": variant(c, n["NodeInitVariantID"], all_cols)}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["tree", "dump", "pens"])
    ap.add_argument("name", nargs="?")
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--all", action="store_true", help="inclui as colunas desligadas e os blobs crus")
    ap.add_argument("--out", help="grava o JSON neste arquivo (padrão: saída padrão)")
    a = ap.parse_args()
    c = connect(a.db)
    if a.cmd == "tree":
        res = tree(c)
    elif a.cmd == "dump":
        if not a.name:
            raise SystemExit("dump precisa do nome da sub-ferramenta")
        res = dump(c, a.name, a.all)
    else:
        res = {}
        for name, key in PENS.items():
            d = dump(c, name, a.all)
            d["key"] = key
            res[key] = d
    text = json.dumps(res, ensure_ascii=False, indent=1)
    if a.out:
        os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
        with open(a.out, "w", encoding="utf-8") as f:
            f.write(text)
        print(f"gravado {a.out}", file=sys.stderr)
    else:
        sys.stdout.reconfigure(encoding="utf-8")
        print(text)


if __name__ == "__main__":
    main()
