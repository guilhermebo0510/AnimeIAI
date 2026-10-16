# Protocolo de validação

O usuário pediu: reproduzir cenas do episódio de referência com até ~5% de diferença (desenho, shader, traço, áudio), no mínimo 3 cenas (meta 5), e depois produzir um micro anime original de 30–60 s. Este documento transforma isso em régua objetiva. A escolha das métricas é do executor e pode ser refinada (registrar em `registro/DECISOES.md`), mas **nunca afrouxada sem registro e justificativa**.

## 1. Material de referência

- Arquivo: `anime de referencia/[Erai-raws] Jishou Akuyaku Reijou na Konyakusha no Kansatsu Kiroku - 01 [...].mkv` — HEVC 1920×1080, 24000/1001 fps, 1421,49 s, AAC estéreo 48 kHz em japonês, legendas ASS (inclui PT-BR).
- Uso: só calibração das ferramentas. Frames e trechos extraídos ficam em `calibracao/ref/` (fora do git). Nada da referência entra no produto, nos assets ou no micro anime.
- Abertura e encerramento (OP/ED) ficam fora da calibração.

## 2. Preparação da referência (S04 e S21)

1. Detecção de cortes (diferença de histograma + bordas; conferência visual) → catálogo `calibracao/ref/cortes.json` com início, fim, duração, miniatura.
2. Para cada corte candidato: extrair todos os quadros em PNG; **mapa de exposição** (quadros onde o desenho muda — revela 1s/2s/3s e as chaves); medir movimento de câmera (fase/optical flow global); extrair paleta (k-means em Lab sobre regiões chapadas); separar aproximadamente personagem × fundo (máscara por análise de cor/bordas, revisada).
3. Ficha do corte: personagens, tipo de plano, câmera, efeitos visíveis, camadas prováveis, dificuldade (1–5).

## 3. Escolha dos cortes de calibração

Obrigatório cobrir os três eixos, em ordem crescente de dificuldade:

| Eixo | O que valida | Candidato inicial (confirmar no catálogo; tempos aproximados da folha de contato) |
|---|---|---|
| A. Personagem | traço vetorial, proporção, balde, sombra de cel, olhos, cabelo, lip-sync, piscar | close/médio de diálogo do menino loiro na sala (~9:00) |
| B. Cenário | perspectiva, pintura raster, texturas, céu, luz, parallax | plano de estabelecimento externo (jardim/palácio, ~3:00) ou céu+jardim mais simples |
| C. Efeitos | partículas, glow/difusão, desfoque de profundidade, tipografia | pétalas de rosa / campo de rosas desfocado (~20:15) ou onomatopeia "ゴ" com pétalas (~8:15) |
| D. Gag/chibi (meta) | formas simples, fundo de gag, linhas de velocidade, impacto | cena chibi com pedras voando (~4:30) |
| E. Composição completa (meta) | personagem + cenário + efeito + câmera no mesmo corte | escolher no catálogo |

Começar por cortes curtos (1–3 s) e com pouca animação; subir a dificuldade depois que a régua passar.

## 4. Níveis de calibração

| Nível | Regra | Para quê |
|---|---|---|
| **N1 — calibração de ferramenta** | O frame de referência pode ficar visível como camada de referência (mesa de luz) e pode ser medido (grade de coordenadas, conta-gotas de cor, régua). **Proibido** vetorizar, recortar, copiar pixels ou converter o frame automaticamente em arte. Cada traço, preenchimento e pincelada é colocado pelo agente com as ferramentas do app. É o equivalente digital do decalque (トレス) que os estúdios fazem. | Calibrar pincéis, espessuras, balde, sombras, texturas, efeitos, câmera e timing contra um alvo exato |
| **N2 — generalização** | Sem a referência embaixo. Usando só o model sheet/settei e o layout criados no N1, desenhar outro quadro do mesmo personagem/cenário (outra pose, outro ângulo ou o quadro seguinte do corte). | Provar que as ferramentas de personagem e layout servem para criar, não só para decalcar |
| **N3 — original** | Micro anime (seção 7). Nenhuma referência visual do episódio. | Validação final |

## 5. Régua dos 5% (N1)

Comparação sempre entre o **render CPU determinístico** do app e o frame de referência, em 1920×1080, por quadro, sobre todos os quadros do corte. "5%" é aplicado de forma consistente: cada métrica normalizada tem de ficar a no máximo 5% do ideal.

| Métrica | Mede | Aprovação por quadro |
|---|---|---|
| **FLIP** (NVIDIA ꟻLIP, média) | Diferença perceptual geral | ≤ 0,05 |
| **SSIM** (luma, janela gaussiana 11 px) | Estrutura/forma | ≥ 0,95 |
| **ΔE2000** médio nas regiões chapadas (máscara de cel) | Cor da pintura | ≤ 5,0; e cada cor do settei do corte casa com a da referência com ΔE ≤ 3 |
| **F1 de bordas** (mapa de linha extraído dos dois, tolerância de 2 px) | Traço: posição e cobertura das linhas | ≥ 0,95 |
| **Espessura de linha** (distribuição da largura medida por transformada de distância) | Peso do traço | mediana dentro de ±5% |
| **Timing** | Quadros em que o desenho muda; posição da câmera | Mapa de exposição idêntico (0 quadros de erro); erro de câmera ≤ 0,5% da largura do quadro |

**Corte aprovado** quando ≥ 95% dos quadros passam em todas as métricas e nenhum quadro tem FLIP > 0,10. Um relatório por tentativa: `sprints/registro/calibracao/<corte>/<tentativa>/` com referência, render, mapa de diferença FLIP, overlay de bordas, tabela de números e a lista de comandos usados.

### Áudio na calibração

As vozes são originais por regra, então timbre não é comparado. A régua de áudio vale para o que pode ser igual:
- início e fim de cada fala: ±1 quadro (≈42 ms) em relação à referência;
- loudness do diálogo: ±1 LU em relação ao mesmo trecho da referência; relação diálogo/fundo: ±1 dB;
- SFX recriados no app: início ±1 quadro, envelope (RMS por 10 ms) com correlação ≥ 0,95;
- lip-sync: abertura de boca dos quadros sincronizada com as moras geradas (sem quadro de boca aberta em silêncio > 2 quadros);
- voz do personagem coerente com o tipo (faixa de F0 compatível com a da referência ±10%).
A música de fundo da referência não é recriada (direito autoral); para medir mixagem, usa-se uma BGM própria com o mesmo nível.

### N2

Proporções medidas contra o model sheet (altura em cabeças, posição dos olhos, largura dos ombros): erro ≤ 5%. Cores contra o settei: ΔE ≤ 2. Zero regiões não pintadas (lint). Avaliação visual registrada com imagem.

## 6. Quantidade

- **Mínimo:** cortes A, B e C aprovados no N1, e A no N2.
- **Meta:** 5 cortes (A–E) no N1, sem bloqueio de ferramenta (ou seja, a última tentativa de cada um não precisou de nenhuma ferramenta nova).
- Cada falha gera uma entrada em `registro/LACUNAS.md` (ferramenta faltando, ferramenta fraca, bug, ergonomia para o agente) e volta para a sprint de origem.

## 7. Aceite do micro anime (N3)

| Item | Critério |
|---|---|
| Duração e formato | 30–60 s; MP4 H.264/AAC 1920×1080 23,976 fps; estéreo 48 kHz |
| História | Roteiro original com começo, meio e fim; bíblia e fichas de personagem no projeto |
| Personagens | ≥ 2, cada um com model sheet (turnaround), expressões, bocas e settei de cor feitos no app |
| Cortes | ≥ 6, vindos do storyboard/e-conte do app, com animatic |
| Cenários | ≥ 3, pintados no app; ≥ 1 com régua de perspectiva ou guia 3D; texturas e pincéis de cenário usados |
| Animação | ≥ 1 corte com animação de personagem de verdade (chaves + intercalação, ≥ 8 desenhos); piscar; lip-sync nas falas |
| Câmera e satsuei | ≥ 1 movimento de câmera com multiplano; ≥ 2 tipos de efeito (ex.: difusão/glow e partículas) |
| Cor | Grade por corte ou LUT aplicada |
| Som | Vozes em japonês (VOICEVOX) com legenda em português; SFX e BGM feitos no app; -24 LKFS ±1, true peak ≤ -1 dBTP |
| Consistência | Proporções ≤ 5% de erro vs. model sheet; cores ΔE ≤ 2 vs. settei; lint = 0 |
| Origem | Todo desenho, pintura e efeito vem do log de comandos do projeto; nenhuma imagem externa além de texturas CC0 registradas; nenhum material do episódio de referência |
| Créditos | Créditos de voz exigidos pelo VOICEVOX e lista de texturas CC0 no final do vídeo |

## 8. Evidência final (S23)

Vídeo do micro anime, vídeo lado a lado dos cortes de calibração (referência | app | diferença), tabelas de métricas, relatório de lacunas abertas, e uma página publicada com tudo para o usuário revisar.
