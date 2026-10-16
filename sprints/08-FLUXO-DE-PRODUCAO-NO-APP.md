# Fluxo de produção dentro do app (como o Claude produz um anime)

Este é o roteiro que a S22 executa e que todas as sprints de ferramenta precisam tornar possível. Cada passo usa só o app (MCP ou mouse). Ao lado, o workspace e as ferramentas principais.

| # | Etapa | Workspace | O que o agente faz | Saída no projeto |
|---|---|---|---|---|
| 1 | Conceito | Roteiro | Escreve logline, sinopse, mundo, tom; define duração-alvo | Bíblia |
| 2 | Estrutura | Roteiro | Beats em cartões (abertura, conflito, virada, fim) | Outline |
| 3 | Roteiro | Roteiro | Cenas, ações e falas (JA + tradução PT) com emoção e direção | Script |
| 4 | Personagens | Personagens | Ficha; model sheet: grade de cabeças, turnaround em 5 vistas desenhado em vetor, usando o manequim 3D posado em cada vista como guia; expressões; bocas A/I/U/E/O/fechada; olhos | Character + assets |
| 5 | Settei de cor | Personagens | Paleta nomeada normal/sombra/realce/linha por parte; balde no model sheet; conferir contraste | ColorSet |
| 6 | Vozes | Áudio | Escolhe voz VOICEVOX por personagem, ajusta tom/velocidade/entonação, gera falas de teste | VoiceProfile |
| 7 | Cenários-base | Cenário + Layout | Blocos 3D do lugar, câmera e lente; render de linhas de guia; régua de perspectiva casada; pintura raster com pincéis/texturas; céu; luz | BackgroundAssets |
| 8 | Storyboard | Storyboard | Painéis por corte: esboço rápido, quadro de câmera, setas, fala/SE, duração | Storyboard |
| 9 | Animatic | Storyboard + Áudio | Gera falas TTS, toca painéis no tempo, ajusta durações | Animatic |
| 10 | Cortes | Produção | Gera cortes a partir dos painéis (duração, falas vinculadas, câmera inicial) | Cuts |
| 11 | Layout | Layout | Por corte: posiciona cenário, manequim/guia 3D na pose, réguas; desenha o layout (rascunho de posição e tamanho) | Camada LO |
| 12 | Genga | Animação | Desenhos-chave em vetor sobre o layout, usando o model sheet e partes reutilizáveis; timing no timesheet | Cels key |
| 13 | Douga | Animação | Breakdowns e intercalação vetorial automática + correção manual; limpeza de linha; boca pelo lip-sync; piscar | Cels completos |
| 14 | Shiage | Pintura | Linhas de cor de sombra (色トレス), balde com paleta do settei em todos os cels, lint até zero | Preenchimentos |
| 15 | Bijutsu do corte | Cenário | Ajusta o BG ao corte (recorte, BOOK para sobreposição, luz da cena) | Camadas BG/BOOK |
| 16 | Satsuei | Composição | Câmera (pan/zoom/multiplano), difusão, glow, luz transmitida, sombra de luz (パラ), partículas, linhas de velocidade, tipografia | Grafo de composição |
| 17 | Cor | Cor | Grade do corte; LUT do episódio; scopes | Grade/LUT |
| 18 | Render do corte | Entrega | Render da faixa de quadros, cache | Sequência/vídeo do corte |
| 19 | Som | Áudio | Falas finais, SFX sintetizados/CC0, BGM no sequenciador, mix, ducking, loudness | Trilhas |
| 20 | Edição | Edição | Monta cortes, transições, legendas PT, créditos | EditTimeline |
| 21 | Entrega | Entrega | Export MP4 e WAV; verificação de loudness/true peak/formato | Arquivo final |
| 22 | Revisão | Produção | Confere cada corte (camada de correção), retakes onde a régua de consistência falhar | Retakes resolvidos |

## Laço de verificação em cada etapa visual

```
desenhar/pintar (comandos) → render CPU do quadro → olhar a imagem (e recortes ampliados)
→ medir (proporção vs. model sheet, ΔE vs. settei, lint, comparação com a referência no N1)
→ corrigir (editar pontos, warp, repintar) → repetir até passar
```

## Como o agente desenha com precisão

- Trabalha em coordenadas do canvas (pixels do quadro 1920×1080). Pode sobrepor uma grade de coordenadas ao render para ler posições.
- Desenha por **curvas de pontos de passagem** com perfil de largura (não por milhares de amostras), formas e simetria.
- Reaproveita partes do model sheet (cabeça, olho, mão) com transformação e warp em vez de redesenhar do zero, como o 兼用 dos estúdios.
- Usa o manequim/guia 3D para acertar ângulo e escorço.
- Constrói de grosso para fino: silhueta/formas básicas (camada de construção) → linha limpa → cor → sombra → efeitos.
