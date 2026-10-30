# Personagem controlado pela design sheet: caminhos (pesquisa, 30/10 manhã)

Fase 4 do roteiro: a geração (o Copiloto e as ferramentas novas) tem de manter **o personagem** (rosto, cabelo, roupa,
proporções) a partir da design sheet, da paleta de settei e, depois, de um modelo treinado. Limite: GTX 1650 com 4 GB,
onde o UNet, o ControlNet e o VAE já ocupam ~2,5 GB e o app mais uns 300 MB.

| # | Caminho | Como entra | Memória | Licença / disponibilidade | Avaliação |
|---|---|---|---|---|---|
| 1 | **IP-Adapter SD 1.5** (`h94/IP-Adapter`, `ip-adapter_sd15`) | a imagem da design sheet vira embeddings de imagem que entram junto do prompt (compatível com ControlNet, diffusers `load_ip_adapter`) | o codificador de imagem (ViT-H, ~632 M parâmetros) não cabe junto: **rodar na CPU uma vez por referência** e guardar os embeddings no projeto; na GPU fica só a projeção (~22 M) | Apache-2.0 | o primeiro a testar: barato por pedido, uma referência por personagem |
| 2 | **Referência por atenção** ("reference-only", sem modelo) | a referência passa pelo UNet a cada passo e as camadas de atenção leem dela | sem pesos novos, mas **dobra o custo do passo** | código livre (diffusers community) | bom para o estilo de traço; caro na 1650 |
| 3 | **LoRA do personagem** (treinada a partir da design sheet e dos cels aprovados) | um LoRA por personagem, carregado pelo gerenciador de modelos | +~10–40 MB por LoRA | nosso | o "muito bom" de verdade; treino lento na 1650 (horas, 512 px, gradient checkpointing): fase 4, item treino |
| 4 | **AnimeAdapter** (arXiv 2605.20237, maio de 2026) | adaptador de aparência para anime a partir de uma ou mais referências; compatível com ControlNet e LoRA; ~24,7 M parâmetros novos | pequeno | **pesos ainda não publicados** ("will be released") | acompanhar; se sair com licença livre, testar contra o 1 |

**Ordem proposta:** 1 primeiro (encode na CPU, embeddings no projeto, escala ligada à Intensidade), medido com a régua
de 18 casos da rodada 3 e uma régua de identidade (mesmo personagem em poses diferentes). 3 entra com o treino. 2 só se
1 não segurar o estilo do traço. 4 quando sair.

**Ligação com o Copiloto:** é o mesmo canal do CP1-14 (contexto além da camada): a caixa poderia ler "personagem X" do
Personagens e usar a referência dele. Começa depois de o Copiloto fechar em SIM.
