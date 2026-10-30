# Qualidade do Copiloto: caminhos (pesquisa, 30/10 madrugada)

O CP1-10 do Codex ("as sugestões ainda não são uma limpeza/complementação confiável de anime") é o bloqueio que não se
resolve com correção de código. Hoje o serviço gera do zero (txt2img com ControlNet lineart anime, LCM, 2 a 8 passos),
e o desenho do artista entra só como controle. Caminhos, do mais barato ao mais caro:

| # | Caminho | O que resolve | Custo na GTX 1650 | Licença | Quando |
|---|---|---|---|---|---|
| 1 | **Partir do próprio rascunho** (img2img com a linha do artista como imagem inicial, além do controle), com a força do img2img ligada à Intensidade | Melhorar em intensidade baixa fica fiel ao traço (hoje o modelo redesenha e o filtro joga fora o que se afasta) | igual ou menor (menos passos efetivos) | — | rodada 3 |
| 2 | **Contexto fixo fora da caixa** (CP1-34) sem modelo novo: a cada passo, os latentes fora da máscara voltam aos do original com o ruído do passo (estilo RePaint/"differential diffusion") | o que está fora da caixa não muda, e a parte nova casa com a borda | igual | — | rodada 3 ou fase 4 |
| 3 | **LoRA de lineart para SD 1.5** (saída já em linha limpa, preto no branco) | menos hachura e sombra para o Anime2Sketch limpar; linhas mais firmes | +50 a 150 MB de VRAM | conferir caso a caso (Civitai: muitas "no commercial") | fase 4 (carregador de LoRA) |
| 4 | **Modo "Limpar" sem difusão**: rede de simplificação de esboço (rough → linha limpa) | limpeza fiel e rápida (< 0,5 s) | pequeno | o modelo de Simo-Serra (SIGGRAPH 2016/TOG 2018) é **não comercial**: só como opção do usuário, nunca padrão | fase 4, se achar um com licença livre (ou treinar o nosso) |
| 5 | **ControlNet de inpainting + lineart** (multi-ControlNet) | completar dentro da máscara com o resto fixo | +0,7 GB: não cabe junto com o app em 4 GB | Apache/OpenRAIL | só em placa maior |
| 6 | **Modelo próprio** treinado em pares rascunho → limpo do próprio estúdio | o "muito bom" de verdade | treino fora do notebook | nosso | depois (fase 4, item "treino") |

**Ordem proposta:** 1 e 2 juntos na rodada 3 (mexem só no `servico.py` e no mapa de Intensidade, sem modelo novo,
medidos com a bateria de qualidade e com `tempos.py`); 3 entra com o carregador de modelos/LoRA da fase 4; 4 e 6 dependem
de licença ou de treino.
