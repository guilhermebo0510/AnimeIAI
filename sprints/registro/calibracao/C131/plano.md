# Calibração — corte C (#131, 8:37.77, 17 quadros, onomatopeia "ゴン" com moldura de rosas)

## Análise da referência
- Exposição em 3s: desenhos mudam nos quadros 1, 4, 7, 10, 13, 16 (6 desenhos; 0-based: 0, 3, 6, 9, 12, 15).
- Camadas prováveis:
  1. **BG**: creme quente com degradê radial (centro claro amarelado, bordas rosadas mais escuras), leves anéis concêntricos (banding/aquarela).
  2. **Letreiro "ゴン"**: massa vermelha escura chapada sem contorno, bordas irregulares de pincel, textura levemente manchada (pontos mais escuros). Estático nos 6 desenhos (conferir).
  3. **Moldura**: ~16 rosas (contorno preto grosso ~5–7 px, 3 tons: sombra vermelho escuro, base vermelho, luz laranja-avermelhado), folhas roxas (contorno preto, nervura, metade clara/metade escura), caules roxos com espinhos. A moldura **cresce** do quadro 0 ao 15 (rosas entram/escala).
  4. **Satsuei**: leve desfoque/difusão geral e grão; vinheta nas bordas.
- Piso de ruído: medir FLIP entre f_0002 e f_0003 (mesma exposição).

## Ferramentas a usar (todas via MCP)
- BG: GRADIENT radial (camada raster) + vinheta (efeito) + textura de aquarela leve (PATTERN_FILL watercolor em overlay/multiplicar, baixa opacidade).
- Letreiro: FILL_SHAPE (contornos medidos com MEASURE_EDGE) com roughness; textura: PAINT com pincel de textura em multiplicar recortado (clip) na camada do letreiro.
- Rosas: DRAW_CURVES (contorno e pétalas) com pontos medidos por MEASURE_LINE; FILL por paleta (3 tons) numa camada fill com line_refs; repetição/transformação para rosas parecidas só se a medição confirmar formas iguais (兼用).
- Folhas e caules: DRAW_CURVES + FILL; espinhos como pequenas curvas fechadas preenchidas.
- Crescimento (desenhos 0–12): cels por exposição; partir do desenho final e transformar/recortar por rosa conforme a referência de cada exposição.
- Efeitos: difusão leve, vinheta, grão (EFFECTS na camada de efeito).
- Avaliação: COMPARE por quadro com `noise_floor` do corte; relatórios em `tentativas/NN/`.

## Ordem de trabalho
1. Piso de ruído e paleta (PROBE_COLOR em regiões chapadas de cada tom).
2. Quadro 15 (desenho final) completo → COMPARE → corrigir até passar.
3. Desenhos 0, 3, 6, 9, 12 → COMPARE de todos os 17 quadros.
4. Relatório final e lacunas.

## Atualização 2026-10-16
- Letreiro ゴン: passada 2 com MEASURE_EDGE em duas iterações (busca 16 → 6 px, mediana dos deslocamentos) — sobreposição sem fantasma visível.
- Rosas: ~12 rosas com 6–10 linhas cada e espessura variável. A leitura da topologia por recortes é lenta; para validar primeiro o ciclo completo (desenho → COMPARE → aprovação) o corte D (#218, chibi, traço simples) passa a ser feito antes, conforme o protocolo §3 ("começar por cortes curtos e com pouca animação"). O corte C continua obrigatório e volta em seguida.
