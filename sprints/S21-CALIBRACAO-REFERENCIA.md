# S21 — Calibração contra o episódio de referência

## Objetivo
Provar e endurecer as ferramentas reproduzindo cortes do episódio de referência dentro da régua do `07-PROTOCOLO-DE-VALIDACAO.md`: mínimo cortes A (personagem), B (cenário) e C (efeitos) aprovados no N1 e A no N2; meta de 5 cortes (A–E) sem bloqueio de ferramenta.

Esta sprint é um **laço**, não uma lista: cada tentativa revela lacunas, que voltam para as sprints de ferramenta, e o corte é tentado de novo.

## Preparação
- [ ] Catálogo de cortes (S04) revisado; escolher os cortes concretos de cada eixo pelas regras do protocolo §3 (curtos e simples primeiro). Registrar a escolha e a ficha de cada um em `registro/calibracao/escolha.md`.
- [ ] Para cada corte: quadros PNG, mapa de exposição, trajetória de câmera, paleta, máscara personagem/fundo, falas e tempos (das legendas e da forma de onda).

## Laço por corte

```
1. Planejar camadas: o que é BG, BOOK, personagem (partes), sombra, efeito; que ferramentas serão usadas
2. Criar o projeto de calibração em calibracao/projetos/<corte>.aiai (fora do git)
3. Settei: paleta medida da referência vira settei do corte
4. Desenhar (N1, referência como camada de mesa de luz, sem conversão automática):
   layout → BG → linha do personagem por cel → shiage → sombras → efeitos → câmera → cor
5. Render CPU de todos os quadros → COMPARE com a referência → relatório da tentativa
6. Se reprovou: localizar a causa (métrica e região) e classificar:
   (a) execução (o agente desenhou errado) → corrigir com as ferramentas
   (b) ferramenta fraca ou faltando → registrar em LACUNAS.md, voltar à sprint de origem, implementar, testar, voltar aqui
   (c) régua injusta (ex.: grão/compressão da referência) → justificar e registrar em DECISOES.md antes de mudar qualquer limiar
7. Repetir até aprovar. Registrar número de tentativas, tempo e lacunas resolvidas
```

- [ ] Áudio do corte: falas com vozes próprias nos tempos da referência, SFX recriados, mix no nível medido; régua de áudio do protocolo.
- [ ] N2 do corte A: desenhar outro quadro do personagem sem mesa de luz, usando o model sheet montado no N1; medir consistência.

## Lacunas esperadas (hipóteses a confirmar)
Brilho e reflexo nos olhos (camadas e degradês pequenos), cabelo com muitas mechas e realces, babados e rendas da roupa, arquitetura ornamentada do palácio (exige muitos traços: considerar ferramentas de repetição/simetria/padrão ao longo de curva), pétalas com desfoque de profundidade, grão e compressão da fonte (pode exigir pré-filtro igual nos dois lados da comparação, registrado).

## Critério de pronto
- [ ] Cortes A, B e C aprovados no N1 (relatórios finais com métricas por quadro, lado a lado e vídeo comparativo).
- [ ] Corte A aprovado no N2.
- [ ] Meta: D e E aprovados; última tentativa de cada corte sem ferramenta nova.
- [ ] `registro/LACUNAS.md` com todas as lacunas e seu estado (resolvida/adiada com motivo).
