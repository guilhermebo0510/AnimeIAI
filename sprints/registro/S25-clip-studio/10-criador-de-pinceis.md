# Criador de pincéis: Clip Studio × AnimeIAI (seção 3 do plano, 28/10/2026)

Fonte: o artigo oficial "Mastering the Advanced Tool Settings palette ② Drawing tools" (tips.clip-studio.com, 563)
e a paleta Configuração de sub-ferramenta do Clip Studio 5.1.5 instalado. Abaixo, categoria por categoria, o que o
Clip Studio deixa ajustar e onde está no nosso editor (Configurações avançadas do pincel / Criar pincel).
**=** já existia e aparece no editor, **+** existia no motor e passou a aparecer no editor nesta seção,
**novo** = feito nesta seção (motor + editor), **—** não feito (com o motivo).

## Dinâmicas (o ícone ao lado de cada valor)

| Clip Studio | AnimeIAI |
|---|---|
| Pressão (mínimo + curva), inclinação, velocidade, aleatório | = editor de dinâmica (pressão, velocidade, inclinação 4.1, aleatório) em tamanho, opacidade, fluxo e espessura |

## Tamanho do pincel

| Clip Studio | AnimeIAI |
|---|---|
| Tamanho, dinâmica | = |
| Tamanho na tela (o tamanho vale em px da tela) | novo "Tamanho na tela" (o contorno do cursor acompanha) |
| Pelo menos 1 pixel | novo "Pelo menos 1 px" |

## Tinta

| Clip Studio | AnimeIAI |
|---|---|
| Opacidade, modo de mesclagem | = (modos ampliados: escurecer, clarear, luz suave/forte, subtrair, diferença, cor, matiz, saturação, luminosidade) |
| Misturar cores: Mesclar / Cor contínua / Borrar; quantidade e densidade de tinta, esticar cor, intensidade do desfoque | + bloco "Mistura de tinta" (desligada / misturar / borrar; quantidade, densidade, esticar, desfoque) |
| Variação de cor (matiz, saturação, brilho) | = |
| Misturar com a cor secundária, aleatório por traço | novo "Variar para a secundária" e "Sortear por traço" |
| Cor em degradê ao longo do traço | = degradê como cor no painel Cores |

## Suavização

| Clip Studio | AnimeIAI |
|---|---|
| 4 níveis | = |

## Forma do pincel

| Clip Studio | AnimeIAI |
|---|---|
| Predefinições de forma (registrar a forma de um pincel e aplicar noutro) | novo "Copiar forma" / "Colar forma" |

## Ponta do pincel

| Clip Studio | AnimeIAI |
|---|---|
| Círculo / material (vários), dureza, espessura, ângulo | = (29 formas desenhadas + imagem própria; várias pontas em sequência ou sorteio) |
| Direção da espessura (horizontal/vertical) | = ângulo 90° |
| Densidade da ponta | = fluxo |
| Ajustar densidade pelo intervalo | novo "Densidade pelo intervalo" |

## Efeito spray

| Clip Studio | AnimeIAI |
|---|---|
| Tamanho da partícula (com dinâmica), densidade, desvio | + bloco "Spray" (tamanho, acompanhar o pincel, menor tamanho, redondeza, concentração, densidade, alcance) |
| Direção da partícula | novo "Partículas seguem o traço" |

## Traço

| Clip Studio | AnimeIAI |
|---|---|
| Intervalo, pulverização contínua, fita, repetição | = |
| Misturar pontas com escurecer | + "Misturar pontas com escurecer" (tinta pelo máximo) |

## Textura e grão

| Clip Studio | AnimeIAI |
|---|---|
| Papel, densidade, inverter, realçar, escala, modo | = (+ modo e inverter no editor) |
| Rotação da textura | novo "Rotação" |
| Aplicar a cada ponta (grão que anda) | + bloco "Grão" (tipo, tamanho, profundidade, contraste, brilho, subtrair, grão que anda) |

## Pincel duplo, borda de aquarela, apagar

| Clip Studio | AnimeIAI |
|---|---|
| 2ª forma do pincel | = pincel duplo |
| Borda de aquarela: opacidade, escuridão, largura do desfoque | = intensidade; novo "Escurecer a borda" e "Largura da borda" |
| Apagar; borracha vetorial; todas as camadas | + "Borracha vetorial" e "Todas as camadas" |

## Correção, início e fim

| Clip Studio | AnimeIAI |
|---|---|
| Estabilização, pela velocidade, correção posterior, ajuste por zoom | = |
| Ajustar à régua | + "Encaixar na régua" |
| Entrada/saída por comprimento, pela velocidade | = |
| Entrada/saída por porcentagem do traço | — a saída em % só se sabe quando o traço acaba; o traço ficaria invisível enquanto se desenha (fica a saída em px, com prévia ao vivo) |
| Anti-transbordo (não passar das linhas da camada de referência) | = "Não passar da linha" (auditoria 4.5m) |

## Também feito

- `set_brush` da ponte (e `aiai_ui` no MCP) aceita `custom`: qualquer campo do pincel por cima de um preset (vira a
  cópia do usuário, como nas Configurações avançadas). `BrushPreset::field_names()` lista os campos válidos.
- Teste do motor: `aiai-paint/tests/criador_csp_s3.rs`. Conferência no app: `tools/csp/brush_options_test.py`
  (prancha pintada com as opções novas e fotos de cada categoria das Configurações avançadas).
- Conferindo a prancha no app, a linha de ~1 px de pressão leve não aparecia: a ponta clássica com menos de ~1,4 px
  amostrava só o centro do pixel e sumia quando o eixo caía na divisa entre pixels. Abaixo de 0,71 px de raio a tinta
  do disco agora é repartida entre os 4 pixels mais perto (mesma quantidade de tinta, nunca some).
- Configurações avançadas: a lista de categorias fica parada e só os parâmetros rolam (cada categoria com a sua
  rolagem), como no Clip Studio; antes, rolar levava a lista junto e o fim de "Tinta" ficava escondido.
