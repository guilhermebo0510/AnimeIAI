# S04 - Catalogo de cortes do episodio de referencia (resumo)

Material: `anime de referencia/[Erai-raws] ... - 01 ... .mkv` (HEVC 1920x1080, 24000/1001 fps). Uso exclusivo de calibracao: quadros, miniaturas e audio ficam em `calibracao/ref/` (fora do git). Aqui so ha numeros e uma folha de contato reduzida (636 px de largura) dos 5 candidatos, material de QA.

Ferramenta: `tools/ref/extract.py detect` (leitura de todos os 34 046 quadros a 160x90 + escore de cena do ffmpeg; regra de aceitacao, filtro de clarao e fusao de planos < 4 quadros documentados no cabecalho do script). Catalogo completo em `calibracao/ref/cortes.json` (327 entradas com inicio/fim/duracao/miniatura) e folhas de contato `calibracao/ref/sheets/episodio_0N.jpg`. Conferencia visual feita nas folhas de contato: nenhuma fusao ou divisao erradas encontradas nas miniaturas examinadas, mas nao revi cada um dos 327 cortes individualmente.

## Numeros

| Item | Valor |
|---|---|
| Quadros lidos / duracao | 34 046 quadros, 1420,0 s (o container declara 1421,49 s) |
| Candidatos do ffmpeg (scene > 0,15) | 457; os 327 cortes finais vem da regra de aceitacao (ffmpeg + histograma, ou histograma muito alto), filtro de clarao e fusao de planos curtos |
| Cortes | **327** |
| Duracao media / mediana | 4,34 s / 3,63 s |
| P10 / P90 | 1,75 s / 7,88 s |
| Minimo / maximo | 0,42 s (cut 130) / 16,64 s (cut 244) |
| Cortes < 1 s / >= 5 s | 6 / 111 |
| Corpo do episodio (cortes 47-295) | 249 cortes, 1129,0 s, mediana 3,84 s |

## Estrutura (aproximada, identificada nas miniaturas)

| Trecho | Cortes | Inicio - fim | Duracao |
|---|---|---|---|
| Prologo (flashback: menino na lousa) | 0-9 | 0:00,00 - 0:52,14 | 52,1 s |
| **OP** (abertura + creditos de abertura; a cancao em si deve ser ~1:30, os cortes 38-46 sao a coda com creditos) | 10-46 | 0:52,14 - 2:39,03 | 106,9 s |
| Cartao "Capitulo 1" | 47 | 2:39,03 - 2:42,04 | 3,0 s |
| **Corpo do episodio** | 48-295 | 2:42,04 - 21:28,0 | ~18:46 |
| **ED** (creditos sobre ilustracoes, 90,0 s) | 296-317 | 21:28,00 - 22:58,00 | 90,0 s |
| Epilogo / cena apos o ED | 318-325 | 22:58,0 - 23:35,0 | 37 s |
| Cartao "Capitulo 2" (chamada) | 326 | 23:35,00 - 23:40 | 5,0 s |

OP, ED, prologo e cartoes ficam fora da calibracao (protocolo, secao 1). O inicio exato do OP (corte 10, 0:52,14) e o fim do ED (corte 317) conferem com a mudanca visual; o limite entre "cancao" e "coda" do OP e uma estimativa visual.

## Candidatos dos 5 eixos do protocolo

Todos estao no corpo do episodio (fora de OP/ED). Os indices sao os de `cortes.json`; quadro = indice de quadro do episodio.

| Eixo | Corte | Inicio | Duracao | Quadros (inicio-fim) | Por que |
|---|---|---|---|---|---|
| **A** Personagem | **#138** | 9:02,42 | 1,50 s | 36 (13005-13040) | Menino loiro de frente, plano medio-proximo, boca fechada, fundo liso (papel de parede/porta). Curtissimo e com pouco movimento: so o conjunto cabeca/corpo e um piscar. E exatamente o "menino loiro na sala ~9:00" do protocolo. Alternativa: #148 (9:33,78; 1,3 s; 30 quadros; close falando). |
| **B** Cenario | **#245** | 17:17,70 | 2,75 s | 66 (24880-24945) | Plano de estabelecimento externo: palacio ao por do sol, jardim em primeiro plano, ceu em degrade e luz; sem personagem em cena. Perspectiva, pintura raster, ceu e luz. Alternativas: #121 (8:10,0; ceu azul + predio, com cartao de texto), #229 (15:41,8; estufa/jardim), #53 (3:01,2; palacio com rosas e personagens pequenos). |
| **C** Efeitos | **#131** | 8:37,77 | 0,71 s | 17 (12414-12430) | Onomatopeia "ゴン" sobre moldura de rosas (tipografia + formas de petalas). Curto (17 quadros); 5-30% da imagem muda a cada 3 quadros. Alternativas: #130 (8:37,35; 0,4 s; petalas sobre preto), #217 (14:48,0; 1,8 s; frasco com brilhos/sparkles), #292 (petalas + desfoque de profundidade). |
| **D** Gag/chibi | **#218** | 14:49,72 | 3,63 s | 87 (21332-21418) | Rosto chibi em primeiro plano (olhos redondos brancos, boca pequena, gota de suor): formas simples e chapadas, treme a cada 3 quadros no fim. Alternativas: #167 (11:01,0; 2,9 s; chibi na sala) e #76 (4:51,4; 5,3 s; chibi com pedras e linhas de velocidade radiais, a sugestao original do protocolo, mais dificil). |
| **E** Composicao completa | **#292** | 21:02,14 | 2,00 s | 48 (30261-30308) | Perfil do menino em primeiro plano desfocado + parede de tijolo/porta ao fundo (profundidade de campo) + petalas de rosa caindo + luz. Personagem, cenario, efeito e camera num corte de so 48 quadros. Alternativa mais ampla/dificil: #295 (21:21,1; 6,9 s; menina flutuando, rosas, predio, ceu, raios de luz). |

Folha de contato reduzida dos 5 candidatos: [`candidatos-contato.jpg`](candidatos-contato.jpg).

### Padrao de exposicao medido nos candidatos (`exposicao-corte-N.json`)

| Corte | Quadros | Exposicoes (quadros mantidos por desenho) | Moda | Leitura |
|---|---|---|---|---|
| #138 (A) | 36 | 3, 3, 3, 3, 9, 3, 3, 9 | **3** | ver abaixo |
| #245 (B) | 66 | 2 x9, 4, 2, 14, 2, 2, 2, 22 | 2 | mudancas minusculas (fracao de pixels 0,2%, quase no limiar de 0,15%): provavel deriva de camera/brilho em 2s, com repousos de 14 e 22 quadros. **Medida fraca, tratar com cautela.** |
| #131 (C) | 17 | 3, 3, 3, 3, 3, 2 | 3 | 5-30% da imagem muda a cada 3 quadros (animacao de impacto em 3s) |
| #218 (D) | 87 | 6, 3, 5, 40, 3 x11 | 3 | repouso de 40 quadros e depois tremida em 3s (fracao 11-17% da imagem) |
| #292 (E) | 48 | 1, 2 x10, 1, 1, 2 x13, 1 | 2 | petalas caindo em 2s (fracao ~0,8% da imagem) |

## Corte A (#138): exposicao em detalhe

Quadros extraidos em `calibracao/ref/cut_138/frames/` (36 PNG 1920x1080) + `audio.wav`. Metodo e limiar em `tools/ref/extract.py` (cabecalho): luma 960x540 suavizada (sigma 1,2), "novo desenho" se mais de 0,15% dos pixels mudam mais de 10 niveis.

- Valores medidos: nos quadros repetidos a fracao e exatamente 0,0 (e a diferenca media 0,02-0,12 de 255), nos desenhos novos fica entre 0,014 e 0,096; distribuicao claramente bimodal, margem de ~10x sobre o limiar.
- **Padrao:** desenhos novos nos quadros 0, 3, 6, 9, 12, 21, 24, 27 (de 0 a 35). Ou seja, **tudo em 3s** (8 quadros/s), com **repousos de 9 quadros** (quadros 12-20 e 27-35).
- Quadros 0-12 (4 trocas): a regiao que muda cobre o menino inteiro (x 198-1267 de 1920, altura toda): leve movimento de cabeca/corpo (respiracao) em 3s; fundo estatico.
- Quadros 21-27 (3 trocas): regiao pequena em volta dos olhos (x 506-967, y 280-495): **piscar** (olhos fechados no 21, aberto no 24, ajuste no 27), tambem em 3s.
- Boca: nao muda em nenhum quadro. O audio do trecho (`audio.wav`, 1,50 s) tem nivel baixo (media -30,0 dB, pico -16,9 dB), provavelmente so ambiente/BGM; nao analisei se ha fala.
- Para o app: o mapa de exposicao a reproduzir e `[3,3,3,3,9,3,3,9]` com 0 quadros de erro (protocolo, linha "Timing").

Observacao de processo: a primeira versao da extracao pulava o primeiro quadro do corte (o mkv guarda pts em ms arredondados e o `-ss` ficava meio quadro depois); corrigido para buscar 5 ms antes do instante nominal, e conferido que os 36 quadros extraidos terminam no ultimo quadro do corte (sem vazar para o corte seguinte).
