# S05 — Desenho vetorial (referência: Clip Studio Paint)

## Objetivo
Pincéis e ferramentas vetoriais com qualidade de linha de anime, editáveis depois, operáveis com precisão pelo agente (curvas por pontos de passagem com perfil de largura) e pelo mouse.

## Pesquisa obrigatória
Manual do CSP (pincel vetorial, borracha vetorial "até interseção", corrigir espessura, operação de objeto, régua de curva), Illustrator (largura variável), papers de ajuste de curva (Schneider), offset de curvas com largura variável (kurbo), estabilização (lazy brush, SAI).

## Tarefas
- [ ] Modelo de stroke: amostras → curva (ajuste de Bézier com tolerância) → perfil de largura (função de pressão e de parâmetro) → ponta (redonda, G-pen afinada nas pontas, maru-pen fina, marcador uniforme, lápis vetorial com textura P1).
- [ ] Rasterização conforme decisão da S03; sem AA (cel) e com AA; junções e pontas corretas em curvas fechadas e apertadas.
- [ ] Ferramenta pincel vetorial (mouse/caneta) com estabilização configurável e pós-correção.
- [ ] Ferramentas: linha, polilinha, curva por pontos de passagem (Catmull-Rom/centrípeta com tensão), Bézier com alças, elipse, retângulo, polígono, arco.
- [ ] Simetria (linear e radial) aplicada no comando.
- [ ] Edição: selecionar traço, mover/escalar/rotacionar traço, editar pontos, mover trecho (puxar), corrigir espessura (pincel que engrossa/afina), afinar pontas, conectar traços próximos, simplificar, suavizar, mudar cor/índice.
- [ ] Borracha vetorial: traço inteiro, até interseção, por área.
- [ ] Cor por índice de paleta (já ligada à S07) e cor direta.
- [ ] Comandos MCP curados: `aiai_draw_curve` (pontos, tensão, larguras início/meio/fim ou perfil, ponta, cor, camada, cel, fechar), `aiai_draw_shape`, `aiai_draw_stroke` (amostras com pressão, para replay), `aiai_edit_stroke`, `aiai_erase_vector`.
- [ ] Replay de traço: JSONL de amostras → mesmo resultado que o desenho ao vivo.

## Testes
- Golden: G-pen em S, círculo fechado, curva apertada, cruzamento com borracha até interseção, linha sem AA (confere ausência de cinzas), traço muito fino (0,5 px) e muito grosso (40 px).
- Teste de precisão para o agente: desenhar por MCP um rosto simples (contorno, olhos, cabelo em 12–20 curvas) e medir espessura e posição contra a especificação.
- Mouse: desenhar traço com arrasto real (SendInput, passos de 2 px), editar ponto, apagar até interseção; comparar com o mesmo traço feito por comando.

## Critério de pronto
- [ ] Prova de uso: o agente desenha via MCP a linha de um busto de personagem anime simples (cabeça, cabelo, olhos, pescoço, gola) com G-pen e larguras variáveis; render em evidências com recortes ampliados das pontas e junções.
- [ ] Qualidade de linha comparada visualmente com um traço de referência do episódio (recorte de cabelo) e espessura medida (S04).
