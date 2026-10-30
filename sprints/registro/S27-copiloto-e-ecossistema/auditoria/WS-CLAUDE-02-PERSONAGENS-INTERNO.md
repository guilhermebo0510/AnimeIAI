# WS-CLAUDE-02 — Personagens: auditoria interna (leitura de código, antes da rodada com o Codex)

> 30/10 de manhã, com o ChatGPT no limite de uso (sem app e sem GPU). Base `ec0a63a`. Arquivo:
> `aiai-studio/src/app/panels/characters.rs` (877 linhas; a pasta de materiais em `char_folder.rs` e a referência em
> `char_ref.rs` ficam para a rodada). É a base da fase 4 (geração que mantém o personagem).

| Id | Gravidade | Achado | Prova | Correção esperada |
|---|---|---|---|---|
| I-P01 | ~~bloqueia~~ melhoria (fase 4); **corrigido no app, ver abaixo** | A ficha não tem **imagens de referência do design** marcadas por vista (frente, 3/4, perfil, costas) nem por parte (rosto, mãos, roupa). A "folha de modelo" gerada é de **proporção** (manequim e linhas-guia), não o desenho do personagem; o desenho do artista entra como material solto. Sem isso, a geração controlada pelo personagem (IP-Adapter, LoRA) não tem de onde ler. | `make_sheet` → `MODEL_SHEET` (guias); `CharTab::Material` | "Referências do design": escolher, entre os materiais, a imagem de cada vista e de cada parte; guardadas na ficha. |
| I-P02 | bloqueia (produção) | A paleta de settei é só **uma paleta ligada**: não há cores nomeadas por parte (cabelo, pele, olhos, roupa) com as variantes de anime (normal, sombra, luz; dia e noite). O `PaletteData` tem `active_variant`, mas a aba não mostra nem edita isso. É o que o pintor usa e o que a geração precisaria. | `palette_tab` (combo + "nova paleta") | Tabela de settei: parte × (normal, sombra, luz), com as variantes; editar ali, não só nas Cores. |
| I-P03 | não bloqueia | Voz: o botão cria sempre o falante 8 do VOICEVOX; o falante se escolhe **pelo número** (0 a 999), sem a lista dos falantes instalados nem o botão **ouvir**. O motor aparece mas não se troca (o Kokoro existe no MCP). | `voice` (`json!({"engine":"voicevox","speaker":8,…})`, `DragValue 0..=999`) | Lista de falantes do motor (com nome e crédito), escolher o motor, "Ouvir" com uma fala do personagem. |
| I-P04 | não bloqueia | Apagar personagem pelo menu de contexto, sem confirmar. Desfaz, mas as falas do roteiro e o elenco que o citam ficam com a referência quebrada (o roteiro cai no nome solto). | `list` → `delete_node` | Confirmar mostrando onde ele é usado (falas, cortes, materiais). |
| I-P05 | **errado (já existe; ver abaixo)** | Sem **folha de expressões** e sem a **tabela de bocas** (A, I, U, E, O, fechada) do personagem, que o lip sync (`aiai_lipsync`) precisa. | abas: Ficha, Paleta, Consistência, Voz, Material | Abas Expressões e Bocas, ligadas ao lip sync. |
| I-P06 | **errado em parte (figurino existe; ver abaixo)** | Ficha curta: nome, idade, cabeças e perfil livre. Faltam figurinos (衣装) e variações, acessórios e relações. | `settei_tab` | Campos de figurino e variação (cada uma com a sua paleta e referências). |
| I-P07 | melhoria | A consistência mede proporção na vertical (topo, queixo, olhos, ombros, pés). Não confere largura (ombros, cintura) nem o rosto (olhos, boca), que é onde o desenho mais escapa do settei. | `consistency`, `CONS_LINES` | Medidas horizontais e do rosto; comparar com a referência do design (I-P01). |

## Conferido no app (30/10, ~08:32, release `ec0a63a`, cópia própria do episódio "A última rosa")

Telas em `calibracao/tmp/claude-ws-roteiro-telas/1*_personagens*.png`. **A leitura de código errou em três itens:**

- **I-P05 e I-P06 estavam errados:** a Pasta do personagem já cria **Expressões, Boca, Olhos e Figurino** (e Modelo e
  Folha), e o episódio tem "Expressões e bocas" da Hana. Eu li só `characters.rs`; isso mora em `char_folder.rs`.
- **I-P01 estava quase todo errado:** a folha de modelo da Hana é o **design de verdade**, colorido, de frente, 3/4 e
  costas, numa pasta "Folha de modelo". O que sobra para a fase 4 é pequeno: marcar qual material é a referência de cada
  vista e de cada parte, para a geração ler (hoje é uma pasta de desenhos, sem esse papel).
- **I-P02 confirmado:** a aba Paleta só liga a paleta ("Settei Hana · 12 cores; edite no painel Cores"), e o painel
  Cores ao lado mostra a roda de cor, não as amostras por parte.
- **I-P03 confirmado:** Voz = "Estilo (id)" numérico e "Nome da voz" digitado; sem lista e sem ouvir.

**Para a rodada WS-R02 com o Codex:** conferir estes sete no app, e o que eu não li (`char_folder.rs`: importar, renomear
e abrir materiais; `char_ref.rs`; o MCP `aiai_model_sheet`, `aiai_character_material`, `aiai_voice_profile`,
`aiai_consistency`).
