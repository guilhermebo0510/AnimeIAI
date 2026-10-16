# Lip-sync S17 — tabela de quadros

Gerado por `cargo test -p aiai-tools --test audio s17_evidence -- --ignored`. Projeto a 24000/1001 fps (23,976). Quadros 0-based como no núcleo (o timesheet de papel mostra 1-based). Mapeamento de cels: closed/n=1, a=2, i=3, u=4, e=5, o=6.

Regras: boca mínima de 2 quadros; pausa de até 2 quadros mantém a forma; pausa maior fecha; vogal → forma (a,i,u,e,o), ん/っ → fechada.

## Fala 1 — Haru (VOICEVOX:春日部つむぎ)

Texto: 「おはよう、今日もいい天気だね。」 — voz 春日部つむぎ; início da fala no quadro 36.

Moras (início–fim em s, relativo ao início do clipe): オ[0.043–0.117] ハ[0.117–0.267] ヨ[0.267–0.427] オ[0.427–0.576] 、[0.576–0.981] キョ[0.981–1.141] オ[1.141–1.205] モ[1.205–1.333] イ[1.333–1.429] イ[1.429–1.515] テ[1.515–1.611] ン[1.611–1.696] キ[1.696–1.792] ダ[1.792–1.899] ネ[1.899–2.123]

Exposições gravadas na coluna de boca (quadro absoluto → cel): 36→1, 37→6, 39→2, 42→6, 50→1, 60→6, 68→3, 72→5, 75→1, 77→3, 79→2, 82→5, 87→1

| quadros | tempo (s) | forma | moras no intervalo |
|---|---|---|---|
| 36 | 0.000–0.042 | rest | — |
| 37–38 | 0.042–0.125 | o | オハ |
| 39–41 | 0.125–0.250 | a | ハ |
| 42–49 | 0.250–0.584 | o | ハヨオ |
| 50–59 | 0.584–1.001 | rest | キョ |
| 60–67 | 1.001–1.335 | o | キョオモイ |
| 68–71 | 1.335–1.501 | i | イイ |
| 72–74 | 1.501–1.627 | e | イテン |
| 75–76 | 1.627–1.710 | n | ンキ |
| 77–78 | 1.710–1.793 | i | キダ |
| 79–81 | 1.793–1.919 | a | ダネ |
| 82–86 | 1.919–2.127 | e | ネ |
| 87–89 | 2.127–2.252 | rest | — |

## Fala 2 — Mio (VOICEVOX:雨晴はう)

Texto: 「うん、バラがきれいに咲いたよ。」 — voz 雨晴はう; início da fala no quadro 104.

Moras (início–fim em s, relativo ao início do clipe): ウ[0.053–0.128] ン[0.128–0.405] 、[0.405–0.789] バ[0.789–0.917] ラ[0.917–1.056] ガ[1.056–1.216] キ[1.216–1.355] レ[1.355–1.451] イ[1.451–1.568] ニ[1.568–1.717] サ[1.717–1.877] イ[1.877–1.984] タ[1.984–2.123] ヨ[2.123–2.421]

Exposições gravadas na coluna de boca (quadro absoluto → cel): 104→1, 105→4, 107→1, 123→2, 133→3, 136→5, 139→3, 145→2, 149→3, 152→2, 155→6, 162→1

| quadros | tempo (s) | forma | moras no intervalo |
|---|---|---|---|
| 104 | 0.000–0.042 | rest | — |
| 105–106 | 0.042–0.125 | u | ウ |
| 107–113 | 0.125–0.417 | n | ウン |
| 114–122 | 0.417–0.792 | rest | バ |
| 123–132 | 0.792–1.210 | a | バラガ |
| 133–135 | 1.210–1.335 | i | ガキ |
| 136–138 | 1.335–1.460 | e | キレイ |
| 139–144 | 1.460–1.710 | i | イニ |
| 145–148 | 1.710–1.877 | a | ニサ |
| 149–151 | 1.877–2.002 | i | サイタ |
| 152–154 | 2.002–2.127 | a | タヨ |
| 155–161 | 2.127–2.419 | o | ヨ |
| 162–165 | 2.419–2.586 | rest | ヨ |

## Variante de 3 formas (fechada/meia/aberta) — fala 1

Cels: fechada=1, meia=3, aberta=2. Não aplicada ao corte de prova.

| quadros | tempo (s) | forma | moras no intervalo |
|---|---|---|---|
| 36 | 0.000–0.042 | rest | — |
| 37–49 | 0.042–0.584 | open | オハヨオ |
| 50–59 | 0.584–1.001 | rest | キョ |
| 60–67 | 1.001–1.335 | open | キョオモイ |
| 68–78 | 1.335–1.793 | half | イイテンキダ |
| 79–81 | 1.793–1.919 | open | ダネ |
| 82–86 | 1.919–2.127 | half | ネ |
| 87–89 | 2.127–2.252 | rest | — |
