//! Internacionalização: português (padrão) e inglês, troca em tempo real. A tabela é Rust (chave, pt, en);
//! os testes garantem que nenhuma chave falta em um idioma e que toda chave usada no código existe.
//! Japonês entra com a fonte Noto Sans JP já embutida (`theme::install_fonts`), quando houver textos.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Pt,
    En,
}

impl Lang {
    /// `pt`, `pt-BR`, `en` (insensível a maiúsculas).
    pub fn from_code(code: &str) -> Option<Lang> {
        match code.to_ascii_lowercase().as_str() {
            "pt" | "pt-br" | "pt_br" => Some(Lang::Pt),
            "en" | "en-us" | "en_us" => Some(Lang::En),
            _ => None,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Pt,
    }
}

/// Traduz `key` no idioma atual. Chave desconhecida devolve a própria chave (visível na UI = fácil de notar;
/// o teste `used_keys_exist` impede isso em produção).
pub fn tr(key: &str) -> &str {
    tr_in(lang(), key)
}

fn index() -> &'static std::collections::HashMap<&'static str, (&'static str, &'static str)> {
    static IDX: std::sync::OnceLock<std::collections::HashMap<&'static str, (&'static str, &'static str)>> = std::sync::OnceLock::new();
    IDX.get_or_init(|| TABLE.iter().map(|(k, pt, en)| (*k, (*pt, *en))).collect())
}

pub fn tr_in(lang: Lang, key: &str) -> &str {
    if let Some((pt, en)) = index().get(key) {
        return match lang {
            Lang::Pt => pt,
            Lang::En => en,
        };
    }
    key
}

/// (chave, pt, en)
pub const TABLE: &[(&str, &str, &str)] = &[
    // Workspaces
    ("workspace.roteiro", "Roteiro", "Script"),
    ("workspace.personagens", "Personagens", "Characters"),
    ("workspace.storyboard", "Storyboard", "Storyboard"),
    ("workspace.layout", "Layout", "Layout"),
    ("workspace.animacao", "Animação", "Animation"),
    ("workspace.pintura", "Pintura", "Painting"),
    ("workspace.cenario", "Cenário", "Backgrounds"),
    ("workspace.composicao", "Composição", "Compositing"),
    ("workspace.cor", "Cor", "Color"),
    ("workspace.audio", "Áudio", "Audio"),
    ("workspace.edicao", "Edição", "Editing"),
    ("workspace.entrega", "Entrega", "Delivery"),
    ("workspace.producao", "Produção", "Production"),
    // Ferramentas
    ("tool.move", "Mover", "Move"),
    ("tool.brush", "Pincel", "Brush"),
    ("tool.pen", "Caneta vetorial", "Vector pen"),
    ("tool.eraser", "Borracha", "Eraser"),
    ("tool.fill", "Balde de tinta", "Fill bucket"),
    ("tool.lasso", "Seleção laço", "Lasso select"),
    ("tool.eyedropper", "Conta-gotas", "Eyedropper"),
    ("tool.hand", "Mão", "Hand"),
    ("tool.zoom", "Zoom", "Zoom"),
    // Painéis: título e estado vazio
    ("panel.canvas.title", "Canvas", "Canvas"),
    ("panel.canvas.empty", "O canvas de desenho entra na S03. O contorno marca o quadro da câmera.", "The drawing canvas arrives in S03. The outline marks the camera frame."),
    ("panel.canvas.camera", "Câmera", "Camera"),
    ("panel.layers.title", "Camadas", "Layers"),
    ("panel.layers.empty", "Nenhuma camada. As camadas do desenho aparecerão aqui.", "No layers. The drawing's layers will appear here."),
    ("panel.properties.title", "Propriedades", "Properties"),
    ("panel.properties.empty", "Nada selecionado.", "Nothing selected."),
    ("panel.palette.title", "Cores", "Colors"),
    ("panel.palette.empty", "A paleta do projeto está vazia.", "The project palette is empty."),
    ("panel.navigator.title", "Navegador", "Navigator"),
    ("panel.navigator.empty", "Sem quadro aberto para navegar.", "No frame open to navigate."),
    ("panel.timeline.title", "Linha do tempo", "Timeline"),
    ("panel.timeline.empty", "A linha do tempo está vazia.", "The timeline is empty."),
    ("panel.script.title", "Roteiro", "Script"),
    ("panel.script.empty", "Este projeto ainda não tem roteiro. Crie um para começar a escrever cenas, ações e falas.", "This project has no script yet. Create one to start writing scenes, actions and dialogue."),
    ("panel.outline.title", "Estrutura", "Outline"),
    ("panel.outline.empty", "Sem roteiro neste projeto. Use Novo roteiro para criar um.", "No script in this project. Use New script to create one."),
    ("panel.characters.title", "Personagens", "Characters"),
    ("panel.characters.empty", "Nenhum personagem criado.", "No characters created."),
    ("panel.sheet.title", "Folha de modelo", "Model sheet"),
    ("panel.sheet.empty", "A folha de modelo do personagem (frente, perfil e costas) entra aqui.", "The character model sheet (front, side and back) goes here."),
    ("panel.shots.title", "Cenas", "Shots"),
    ("panel.shots.empty", "Sem storyboard neste projeto. Use Novo storyboard para criar um.", "No storyboard in this project. Use New storyboard to create one."),
    ("panel.board.title", "Prancha", "Board"),
    ("panel.board.empty", "Sem storyboard neste projeto. Crie um para ver a prancha de painéis.", "No storyboard in this project. Create one to see the panel board."),
    ("panel.preview.title", "Monitor", "Monitor"),
    ("panel.preview.empty", "Sem vídeo para exibir.", "No video to show."),
    ("panel.media.title", "Mídia", "Media"),
    ("panel.media.empty", "Nenhuma mídia importada.", "No media imported."),
    ("panel.mixer.title", "Mixer", "Mixer"),
    ("panel.mixer.empty", "Nenhuma faixa de áudio.", "No audio tracks."),
    ("panel.presets.title", "Predefinições", "Presets"),
    ("panel.presets.empty", "Nenhuma predefinição de exportação.", "No export presets."),
    ("panel.queue.title", "Fila de exportação", "Export queue"),
    ("panel.queue.empty", "Nenhuma exportação na fila.", "No exports in the queue."),
    ("panel.tasks.title", "Tabela de cortes", "Cut sheet"),
    ("panel.tasks.empty", "Nenhum corte no projeto. Crie cortes no Storyboard (Gerar cortes) ou no painel Projeto.", "No cuts in this project. Create cuts in the Storyboard (Generate cuts) or in the Project panel."),
    ("panel.episodes.title", "Episódios", "Episodes"),
    ("panel.episodes.empty", "Nenhum corte cadastrado ainda.", "No cuts registered yet."),
    // Menus
    ("menu.soon", "disponível em breve", "coming soon"),
    ("menu.file", "Arquivo", "File"),
    ("menu.file.new", "Novo", "New"),
    ("menu.file.open", "Abrir", "Open"),
    ("menu.file.save", "Salvar", "Save"),
    ("menu.file.save_as", "Salvar como", "Save as"),
    ("menu.file.export", "Exportar", "Export"),
    ("menu.file.quit", "Sair", "Quit"),
    ("menu.edit", "Editar", "Edit"),
    ("menu.edit.undo", "Desfazer", "Undo"),
    ("menu.edit.redo", "Refazer", "Redo"),
    ("menu.edit.prefs", "Preferências", "Preferences"),
    ("menu.edit.nothing", "Nada para desfazer", "Nothing to undo"),
    ("menu.view", "Exibir", "View"),
    ("menu.view.theme", "Tema", "Theme"),
    ("menu.view.theme.light", "Claro", "Light"),
    ("menu.view.theme.dark", "Escuro", "Dark"),
    ("menu.view.language", "Idioma", "Language"),
    ("menu.view.zoom", "Zoom da interface", "Interface zoom"),
    ("menu.window", "Janela", "Window"),
    ("menu.window.reset", "Restaurar layout", "Reset layout"),
    ("menu.window.reset_all", "Restaurar todos os layouts", "Reset all layouts"),
    ("menu.help", "Ajuda", "Help"),
    ("menu.help.about", "Sobre o AnimeIAI", "About AnimeIAI"),
    ("lang.pt", "Português (Brasil)", "Português (Brasil)"),
    ("lang.en", "English", "English"),
    // Barra de status
    ("status.project", "Projeto", "Project"),
    ("status.no_project", "sem projeto", "no project"),
    ("status.memory", "Memória", "Memory"),
    ("status.bridge", "Bridge", "Bridge"),
    ("status.bridge.tip", "Bridge de automação ativa (somente loopback, com token). As ações entram no histórico e podem ser desfeitas.", "Automation bridge active (loopback only, token-protected). Actions enter the history and can be undone."),
    ("status.revision", "revisão", "revision"),
    ("doc.untitled", "Sem título", "Untitled"),
    ("dialog.open_title", "Abrir projeto (pasta .aiai)", "Open project (.aiai folder)"),
    ("dialog.save_title", "Salvar projeto como (pasta .aiai)", "Save project as (.aiai folder)"),
    ("confirm.title", "Alterações não salvas", "Unsaved changes"),
    ("confirm.text", "O projeto \"{name}\" tem alterações não salvas.", "Project \"{name}\" has unsaved changes."),
    ("confirm.save", "Salvar", "Save"),
    ("confirm.discard", "Descartar", "Discard"),
    ("confirm.cancel", "Cancelar", "Cancel"),
    ("msg.new", "Novo projeto", "New project"),
    ("msg.opened", "Projeto aberto", "Project opened"),
    ("msg.saved", "Projeto salvo", "Project saved"),
    ("msg.error", "Erro", "Error"),
    ("msg.undo", "Desfeito", "Undone"),
    ("msg.redo", "Refeito", "Redone"),
    // Sobre
    ("about.title", "Sobre", "About"),
    ("about.version", "Versão", "Version"),
    ("about.tagline", "Estúdio de animação 2D com ferramentas próprias.", "2D animation studio with its own tools."),
    ("about.credits", "© 2026 AnimeIAI. Fontes Noto Sans e Noto Sans JP, licença SIL OFL 1.1.", "© 2026 AnimeIAI. Noto Sans and Noto Sans JP fonts, SIL OFL 1.1 license."),
    // Preferências
    ("prefs.title", "Preferências", "Preferences"),
    ("prefs.theme", "Tema", "Theme"),
    ("prefs.language", "Idioma", "Language"),
    ("prefs.zoom", "Zoom da interface", "Interface zoom"),
    ("prefs.file", "Arquivo", "File"),
    ("prefs.no_session", "Sessão desativada: nada é salvo.", "Session disabled: nothing is saved."),
    // Editor (S03 B2): canvas, camadas, cores, linha do tempo, propriedades
    ("panel.project.title", "Projeto", "Project"),
    ("panel.project.empty", "O projeto está vazio. Crie um episódio, uma cena e um corte.", "The project is empty. Create an episode, a scene and a cut."),
    ("blend.normal", "Normal", "Normal"),
    ("blend.pass_through", "Passar através", "Pass through"),
    ("blend.multiply", "Multiplicar", "Multiply"),
    ("blend.screen", "Tela", "Screen"),
    ("blend.overlay", "Sobrepor", "Overlay"),
    ("blend.soft_light", "Luz suave", "Soft light"),
    ("blend.hard_light", "Luz forte", "Hard light"),
    ("blend.add", "Adicionar", "Add"),
    ("blend.subtract", "Subtrair", "Subtract"),
    ("blend.darken", "Escurecer", "Darken"),
    ("blend.lighten", "Clarear", "Lighten"),
    ("blend.difference", "Diferença", "Difference"),
    ("blend.color", "Cor", "Color"),
    ("blend.hue", "Matiz", "Hue"),
    ("blend.saturation", "Saturação", "Saturation"),
    ("blend.luminosity", "Luminosidade", "Luminosity"),
    ("blend.behind", "Atrás", "Behind"),
    ("canvas.fit", "Ajustar à janela (Ctrl+0)", "Fit to window (Ctrl+0)"),
    ("canvas.actual", "Tamanho real, 100% (Ctrl+1)", "Actual size, 100% (Ctrl+1)"),
    ("canvas.rotate_left", "Girar a visão 15° para a esquerda (R + arrastar gira livremente)", "Rotate the view 15° left (R + drag rotates freely)"),
    ("canvas.rotate_right", "Girar a visão 15° para a direita (R + arrastar gira livremente)", "Rotate the view 15° right (R + drag rotates freely)"),
    ("canvas.flip", "Espelhar a visão (não altera o desenho)", "Flip the view (does not change the drawing)"),
    ("canvas.grid", "Grade de pixels", "Pixel grid"),
    ("canvas.camera", "Quadro da câmera e área segura", "Camera frame and safe area"),
    ("canvas.camera_label", "Câmera", "Camera"),
    ("canvas.guides", "Mostrar camadas de guia", "Show guide layers"),
    ("canvas.reference", "Mostrar camadas de referência", "Show reference layers"),
    ("canvas.empty_title", "Nenhum corte para desenhar", "No cut to draw on"),
    ("canvas.empty_hint", "Crie um corte no painel Projeto ou comece com um clique.", "Create a cut in the Project panel or start with one click."),
    ("cel.role.key", "Chave", "Key"),
    ("cel.role.breakdown", "Breakdown", "Breakdown"),
    ("cel.role.inbetween", "Intercalação", "Inbetween"),
    ("cmd.add_layer", "Adicionar camada", "Add layer"),
    ("cmd.exposure", "Exposição no timesheet", "Timesheet exposure"),
    ("cmd.quick_start", "Criar episódio, cena e corte", "Create episode, scene and cut"),
    ("err.no_layer", "Selecione uma camada no painel Camadas.", "Select a layer in the Layers panel."),
    ("err.need_vector", "A caneta vetorial desenha em camada vetorial. Selecione ou crie uma.", "The vector pen draws on a vector layer. Select or create one."),
    ("err.need_raster", "O pincel desenha em camada raster. Selecione ou crie uma.", "The brush paints on a raster layer. Select or create one."),
    ("err.eraser_layer", "A borracha atua em camadas vetoriais, raster e de preenchimento.", "The eraser works on vector, raster and fill layers."),
    ("err.move_layer", "Mover atua em camadas vetoriais e raster.", "Move works on vector and raster layers."),
    ("err.layer_locked", "A camada está travada (cadeado).", "The layer is locked."),
    ("err.layer_hidden", "A camada está oculta (olho).", "The layer is hidden."),
    ("err.no_fill_layer", "Crie uma camada de preenchimento para usar o balde.", "Create a fill layer to use the bucket."),
    ("err.no_cel", "A camada não expõe nenhum cel neste quadro.", "The layer exposes no cel at this frame."),
    ("err.no_cut", "Corte não encontrado.", "Cut not found."),
    ("err.no_palette", "Paleta não encontrada.", "Palette not found."),
    ("fill.lint_result", "{n} região(ões) fechada(s) sem tinta (de {total})", "{n} closed region(s) without paint (of {total})"),
    ("layer.kind.vector", "Vetor", "Vector"),
    ("layer.kind.raster", "Raster", "Raster"),
    ("layer.kind.fill", "Preenchimento", "Fill"),
    ("layer.kind.group", "Pasta", "Group"),
    ("layer.kind.guide", "Guia", "Guide"),
    ("layer.kind.reference", "Referência", "Reference"),
    ("layer.kind.effect", "Efeito", "Effect"),
    ("layer.kind.particles", "Partículas", "Particles"),
    ("layer.kind.text", "Texto", "Text"),
    ("layer.kind.adjustment", "Ajuste", "Adjustment"),
    ("layers.add", "Nova camada", "New layer"),
    ("layers.up", "Subir camada", "Move layer up"),
    ("layers.down", "Descer camada", "Move layer down"),
    ("layers.delete", "Apagar camada", "Delete layer"),
    ("layers.move_to_group", "Mover para uma pasta", "Move into a group"),
    ("layers.to_cut", "Fora de pastas (raiz do corte)", "Out of groups (cut root)"),
    ("layers.visible", "Mostrar/ocultar", "Show/hide"),
    ("layers.locked", "Travar/destravar", "Lock/unlock"),
    ("layers.no_cut", "Sem corte ativo. Crie um corte no painel Projeto.", "No active cut. Create a cut in the Project panel."),
    ("layers.settings", "Camada ativa", "Active layer"),
    ("layers.opacity", "Opacidade", "Opacity"),
    ("layers.blend", "Mistura", "Blend"),
    ("layers.clip", "Recortar pela camada de baixo", "Clip to layer below"),
    ("layers.clip.short", "Recortar", "Clip"),
    ("layers.line_refs", "Linhas", "Lines"),
    ("layers.line_refs.none", "nenhuma", "none"),
    ("layers.line_refs.empty", "Não há camadas vetoriais ou raster para usar como linha.", "There are no vector or raster layers to use as line art."),
    ("layers.cels", "Cels da camada", "Layer cels"),
    ("layers.cel", "Cel", "Cel"),
    ("layers.add_cel", "Novo cel", "New cel"),
    ("layers.duplicate_cel", "Duplicar o cel escolhido", "Duplicate the selected cel"),
    ("layers.cel_hint", "clique no cel e depois nas células da linha do tempo", "pick a cel, then click timeline cells"),
    ("opt.color", "Cor", "Color"),
    ("opt.color.tip", "Cor atual do desenho", "Current drawing color"),
    ("opt.width", "Largura", "Width"),
    ("opt.tip", "Ponta", "Tip"),
    ("opt.stabilizer", "Estabilizador", "Stabilizer"),
    ("opt.aliased", "Sem antisserrilhado", "Aliased (no anti-aliasing)"),
    ("opt.autoclose", "Fechar ao voltar", "Close on return"),
    ("opt.autoclose.tip", "Terminar o traço perto de onde começou fecha a forma (o balde enxerga regiões fechadas)", "Ending the stroke near where it started closes the shape (the bucket sees closed regions)"),
    ("opt.preset", "Pincel", "Brush"),
    ("opt.size", "Tamanho", "Size"),
    ("opt.opacity", "Opacidade", "Opacity"),
    ("opt.gap", "Fechar vãos", "Close gaps"),
    ("opt.gap.tip", "Tamanho do vão nas linhas que o balde ainda considera fechado", "Gap size in the lines that the bucket still treats as closed"),
    ("opt.expand", "Expandir", "Expand"),
    ("opt.expand.tip", "Quanto a tinta avança por baixo da linha", "How far the paint extends under the line"),
    ("opt.lint", "Verificar", "Check"),
    ("opt.lint.tip", "Conta as regiões fechadas ainda sem tinta", "Counts closed regions that are still unpainted"),
    ("opt.select_mode", "Forma", "Shape"),
    ("opt.move_apply", "Aplicar", "Apply"),
    ("opt.move_apply.tip", "Aplica o movimento ao desenho (Enter)", "Apply the move to the drawing (Enter)"),
    ("opt.move_cancel", "Cancelar", "Cancel"),
    ("opt.move_cancel.tip", "Cancela o movimento e a seleção (Esc)", "Cancel the move and the selection (Esc)"),
    ("opt.move_hint", "Arraste para mover a seleção (ou a camada toda). Enter aplica; Esc cancela.", "Drag to move the selection (or the whole layer). Enter applies; Esc cancels."),
    ("opt.select_hint", "Arraste no canvas para selecionar; depois use Mover. Um clique limpa a seleção.", "Drag on the canvas to select; then use Move. A click clears the selection."),
    ("opt.eraser_hint", "Em camada vetorial apaga os traços que o cursor toca; em raster apaga pixels; em preenchimento tira a região.", "On a vector layer it removes the strokes the cursor touches; on raster it erases pixels; on fill it removes the region."),
    ("opt.fill_hint", "Clique numa região fechada. Usa as linhas da camada de preenchimento; sem ligação, usa a camada vetorial vizinha.", "Click a closed region. It uses the fill layer's lines; without links, it uses the neighbouring vector layer."),
    ("opt.eyedropper_hint", "Clique para copiar a cor do canvas. Alt + clique funciona em qualquer ferramenta.", "Click to copy a color from the canvas. Alt + click works with any tool."),
    ("opt.hand_hint", "Arraste para mover a visão. Espaço + arrastar ou botão do meio funcionam em qualquer ferramenta.", "Drag to pan the view. Space + drag or the middle button work with any tool."),
    ("opt.zoom_hint", "Clique aproxima, Shift + clique afasta. A roda do mouse dá zoom no cursor.", "Click zooms in, Shift + click zooms out. The mouse wheel zooms at the cursor."),
    ("stab.off", "Desligado", "Off"),
    ("stab.average", "Média móvel", "Moving average"),
    ("stab.lazy", "Corda (lazy)", "Lazy rope"),
    ("stab.window.tip", "Quantas amostras entram na média", "How many samples enter the average"),
    ("stab.radius.tip", "Raio da corda em pontos da tela", "Rope radius in screen points"),
    ("sel.rect", "Retângulo", "Rectangle"),
    ("sel.lasso", "Laço", "Lasso"),
    ("sel.ellipse", "Elipse", "Ellipse"),
    ("tip.g_pen", "Pena G", "G pen"),
    ("tip.maru_pen", "Pena Maru", "Maru pen"),
    ("tip.marker", "Marcador", "Marker"),
    ("tip.pencil", "Lápis", "Pencil"),
    ("palette.palettes", "Paletas", "Palettes"),
    ("palette.name", "Paleta", "Palette"),
    ("palette.color", "Cor", "Color"),
    ("palette.new", "Nova paleta", "New palette"),
    ("palette.none", "nenhuma", "none"),
    ("palette.direct", "Cor direta", "Direct color"),
    ("palette.linked", "Da paleta", "From palette"),
    ("palette.linked.tip", "Trocar esta cor na paleta recolore tudo que a usa", "Changing this color in the palette recolors everything that uses it"),
    ("palette.add_to", "Adicionar à paleta", "Add to palette"),
    ("palette.need_palette", "Crie uma paleta primeiro", "Create a palette first"),
    ("palette.add_color", "Adicionar a cor atual à paleta", "Add the current color to the palette"),
    ("palette.rename", "Editar nome e cor", "Edit name and color"),
    ("palette.delete_color", "Apagar cor", "Delete color"),
    ("palette.edit", "Editar cor da paleta", "Edit palette color"),
    ("palette.color_name", "Nome", "Name"),
    ("palette.color_value", "Cor", "Color"),
    ("project.add_episode", "Novo episódio", "New episode"),
    ("project.add_scene", "Nova cena", "New scene"),
    ("project.add_cut", "Novo corte", "New cut"),
    ("project.need_episode", "Selecione um episódio (ou uma cena) na árvore", "Select an episode (or a scene) in the tree"),
    ("project.need_scene", "Selecione uma cena (ou um corte) na árvore", "Select a scene (or a cut) in the tree"),
    ("project.cut_duration", "Duração do corte", "Cut duration"),
    ("project.cut_size", "Tamanho próprio", "Custom size"),
    ("project.quick_start", "Começar: episódio, cena e corte", "Start: episode, scene and cut"),
    ("project.empty", "Nenhum episódio ainda.", "No episodes yet."),
    ("project.characters", "Personagens", "Characters"),
    ("project.documents", "Documentos", "Documents"),
    ("project.settings", "Configurações do projeto", "Project settings"),
    ("project.name", "Nome", "Name"),
    ("project.frame_size", "Tamanho do quadro", "Frame size"),
    ("project.fps", "Quadros por segundo", "Frames per second"),
    ("project.rename", "Renomear", "Rename"),
    ("project.delete", "Apagar", "Delete"),
    ("tree.episode", "Episódio", "Episode"),
    ("tree.scene", "Cena", "Scene"),
    ("tree.cut", "Corte", "Cut"),
    ("props.layer", "Camada", "Layer"),
    ("props.cut", "Corte", "Cut"),
    ("props.name", "Nome", "Name"),
    ("props.type", "Tipo", "Type"),
    ("props.role", "Papel", "Role"),
    ("props.depth", "Profundidade", "Depth"),
    ("props.offset", "Deslocamento", "Offset"),
    ("props.canvas", "Canvas", "Canvas"),
    ("props.duration", "Duração", "Duration"),
    ("role.none", "Nenhum", "None"),
    ("role.layout", "Layout", "Layout"),
    ("role.genga", "Genga", "Genga"),
    ("role.douga", "Douga", "Douga"),
    ("role.shiage", "Shiage", "Shiage"),
    ("role.shadow", "Sombra", "Shadow"),
    ("role.highlight", "Realce", "Highlight"),
    ("role.background", "Fundo", "Background"),
    ("role.book", "Book", "Book"),
    ("role.effect", "Efeito", "Effect"),
    ("role.correction", "Correção", "Correction"),
    ("role.construction", "Construção", "Construction"),
    ("timeline.no_cut", "Sem corte ativo. Escolha um corte no painel Projeto.", "No active cut. Pick a cut in the Project panel."),
    ("timeline.play", "Tocar (Espaço sobre a linha do tempo)", "Play (Space over the timeline)"),
    ("timeline.pause", "Pausar", "Pause"),
    ("timeline.step_back", "Quadro anterior (seta esquerda)", "Previous frame (left arrow)"),
    ("timeline.step_forward", "Próximo quadro (seta direita)", "Next frame (right arrow)"),
    ("timeline.loop", "Repetir no laço", "Loop playback"),
    ("timeline.frame", "Quadro", "Frame"),
    ("timeline.duration", "Duração", "Duration"),
    ("timeline.loop_range", "Laço", "Loop"),
    ("timeline.selected_cel", "Cel escolhido:", "Selected cel:"),
    ("timeline.static", "fixo", "static"),
    ("timeline.ctx.none", "Clique com o botão direito numa célula.", "Right-click a cell."),
    ("timeline.ctx.expose", "Expor cel", "Expose cel"),
    ("timeline.ctx.at", "no quadro", "at frame"),
    ("timeline.ctx.hold", "Segurar (remove a mudança)", "Hold (remove the change)"),
    ("timeline.ctx.clear", "Quadro vazio", "Empty frame"),
    ("timeline.ctx.loop_in", "Início do laço aqui", "Loop start here"),
    ("timeline.ctx.loop_out", "Fim do laço aqui", "Loop end here"),
    ("outline.new_script", "Novo roteiro", "New script"),
    ("outline.add_scene", "Nova cena", "New scene"),
    ("outline.delete_scene", "Apagar a cena", "Delete scene"),
    ("outline.scene_up", "Subir a cena", "Move scene up"),
    ("outline.scene_down", "Descer a cena", "Move scene down"),
    ("outline.no_script", "(sem roteiro)", "(no script)"),
    ("outline.scene_word", "Cena", "Scene"),
    ("outline.scenes", "cenas", "scenes"),
    ("outline.elements", "elementos", "elements"),
    ("outline.untitled_scene", "(cena sem cabeçalho)", "(scene without heading)"),
    ("outline.untitled_script", "(roteiro sem título)", "(untitled script)"),
    ("outline.beats", "Beats", "Beats"),
    ("outline.default_name", "Roteiro", "Script"),
    ("outline.default_heading", "INT. LOCAL - DIA", "INT. PLACE - DAY"),
    ("script.kind.action", "Ação", "Action"),
    ("script.kind.dialogue", "Fala", "Dialogue"),
    ("script.kind.parenthetical", "Parentético", "Parenthetical"),
    ("script.kind.transition", "Transição", "Transition"),
    ("script.kind.note", "Nota", "Note"),
    ("script.all_scenes", "Todas as cenas", "All scenes"),
    ("script.no_scenes", "O roteiro não tem cenas.", "The script has no scenes."),
    ("script.scene_empty", "Cena vazia. Adicione uma ação ou uma fala.", "Empty scene. Add an action or a line."),
    ("script.no_character", "(sem personagem)", "(no character)"),
    ("script.delete_element", "Apagar o elemento", "Delete element"),
    ("script.element_up", "Subir o elemento", "Move element up"),
    ("script.element_down", "Descer o elemento", "Move element down"),
    ("script.text_hint", "Texto no idioma original", "Text in the original language"),
    ("script.translation_hint", "Tradução", "Translation"),
    ("script.emotion", "Emoção", "Emotion"),
    ("script.direction", "Direção", "Direction"),
    ("script.add_action", "+ Ação", "+ Action"),
    ("script.add_dialogue", "+ Fala", "+ Line"),
    ("script.add_transition", "+ Transição", "+ Transition"),
    ("script.add_note", "+ Nota", "+ Note"),
    ("script.props", "Roteiro", "Script"),
    ("script.props_none", "Sem roteiro aberto.", "No script open."),
    ("script.title", "Título", "Title"),
    ("script.language", "Idioma", "Language"),
    ("script.translation_language", "Tradução", "Translation"),
    ("script.apply_timing", "Aplicar estimativa de tempo", "Apply time estimate"),
    ("script.apply_timing.tip", "Grava os segundos estimados nas ações e falas que ainda não têm tempo", "Writes the estimated seconds on actions and lines that have no time yet"),
    ("shots.shot.establishing", "Plano de situação", "Establishing"),
    ("shots.shot.wide", "Plano geral", "Wide"),
    ("shots.shot.full", "Plano inteiro", "Full"),
    ("shots.shot.medium", "Plano médio", "Medium"),
    ("shots.shot.medium_close", "Médio fechado", "Medium close-up"),
    ("shots.shot.close", "Close", "Close-up"),
    ("shots.shot.extreme_close", "Close extremo", "Extreme close-up"),
    ("shots.shot.detail", "Detalhe", "Detail"),
    ("shots.shot.over_shoulder", "Sobre o ombro", "Over the shoulder"),
    ("shots.angle.eye", "Altura dos olhos", "Eye level"),
    ("shots.angle.high", "De cima", "High angle"),
    ("shots.angle.low", "De baixo", "Low angle"),
    ("shots.angle.dutch", "Inclinado", "Dutch angle"),
    ("shots.angle.bird", "Vista de pássaro", "Bird's eye"),
    ("shots.angle.worm", "Vista de minhoca", "Worm's eye"),
    ("shots.default_name", "Storyboard", "Storyboard"),
    ("shots.no_storyboard", "(sem storyboard)", "(no storyboard)"),
    ("shots.new_storyboard", "Novo storyboard", "New storyboard"),
    ("shots.add_panel", "Novo painel", "New panel"),
    ("shots.delete_panel", "Apagar o painel (e o desenho dele)", "Delete panel (and its drawing)"),
    ("shots.panel_up", "Subir o painel", "Move panel up"),
    ("shots.panel_down", "Descer o painel", "Move panel down"),
    ("shots.renumber", "Renumerar os cortes 1, 2, 3...", "Renumber cuts 1, 2, 3..."),
    ("shots.panels", "painéis", "panels"),
    ("shots.no_action", "(sem ação)", "(no action)"),
    ("shots.props", "Painel", "Panel"),
    ("shots.props_none", "Selecione um painel do storyboard.", "Select a storyboard panel."),
    ("shots.cut_number", "Corte nº", "Cut no."),
    ("shots.shot", "Plano", "Shot"),
    ("shots.angle", "Ângulo", "Angle"),
    ("shots.seconds", "Duração", "Duration"),
    ("shots.action", "Ação", "Action"),
    ("shots.dialogue", "Fala", "Dialogue"),
    ("shots.sfx", "Efeito sonoro", "Sound effect"),
    ("board.open_drawing", "Abrir desenho", "Open drawing"),
    ("board.open_drawing.tip", "Abre o desenho do painel no workspace Layout (ou dê duplo clique no painel)", "Opens the panel drawing in the Layout workspace (or double-click the panel)"),
    ("board.no_drawing", "Este painel não tem corte de desenho.", "This panel has no drawing cut."),
    ("board.animatic", "Animatic", "Animatic"),
    ("board.animatic.tip", "Gera o vídeo do storyboard em saida/animatic.mp4, na pasta do projeto", "Renders the storyboard video to saida/animatic.mp4 in the project folder"),
    ("board.animatic_done", "Animatic gerado", "Animatic rendered"),
    ("board.cuts", "Gerar cortes", "Generate cuts"),
    ("board.cuts.tip", "Cria cenas e cortes no primeiro episódio a partir dos painéis (um passo de desfazer)", "Creates scenes and cuts in the first episode from the panels (one undo step)"),
    ("board.cuts_done", "cortes criados", "cuts created"),
    ("board.save_first", "Salve o projeto antes: o animatic vai para a pasta saida ao lado dele.", "Save the project first: the animatic goes to the saida folder next to it."),
    ("board.need_episode", "Crie um episódio no painel Projeto antes de gerar os cortes.", "Create an episode in the Project panel before generating cuts."),
    ("board.no_panels", "O storyboard não tem painéis. Use Novo painel na lista de cenas.", "The storyboard has no panels. Use New panel in the shots list."),
    ("prod.stage.layout", "LO", "LO"),
    ("prod.stage.genga", "Genga", "Genga"),
    ("prod.stage.douga", "Douga", "Douga"),
    ("prod.stage.shiage", "Shiage", "Shiage"),
    ("prod.stage.background", "BG", "BG"),
    ("prod.stage.satsuei", "Satsuei", "Satsuei"),
    ("prod.stage.color", "Cor", "Color"),
    ("prod.stage.sound", "Som", "Sound"),
    ("prod.status.not_started", "Não iniciado", "Not started"),
    ("prod.status.in_progress", "Em andamento", "In progress"),
    ("prod.status.review", "Para revisão", "For review"),
    ("prod.status.approved", "Aprovado", "Approved"),
    ("prod.status.retake", "Retake", "Retake"),
    ("prod.all_cuts", "Todos os cortes", "All cuts"),
    ("prod.summary", "Etapas (aprovados)", "Stages (approved)"),
    ("prod.open_corrections", "Correções abertas", "Open corrections"),
    ("prod.open_retakes", "Retakes abertos", "Open retakes"),
    ("prod.report", "Relatório de produção", "Production report"),
    ("prod.report.tip", "Grava o relatório (Markdown) em saida/producao.md, na pasta do projeto", "Writes the report (Markdown) to saida/producao.md in the project folder"),
    ("prod.report_done", "Relatório gravado", "Report written"),
    ("prod.suggest", "Sugerir etapas", "Suggest stages"),
    ("prod.suggest.tip", "Avança as etapas do corte pelo conteúdo (camadas, traços, lint); nunca mexe em aprovadas ou retakes", "Advances the cut's stages from its content (layers, strokes, lint); never touches approved stages or retakes"),
    ("prod.suggest_none", "Nenhuma etapa a avançar neste corte", "No stage to advance in this cut"),
    ("prod.suggest_done", "etapa(s) avançada(s)", "stage(s) advanced"),
    ("prod.lint", "Lint do corte", "Lint cut"),
    ("prod.lint.tip", "Procura regiões sem cor em todas as camadas de preenchimento, em todos os quadros expostos", "Looks for unpainted regions in every fill layer, in every exposed frame"),
    ("prod.lint_clean", "sem regiões sem cor", "no unpainted regions"),
    ("prod.lint_dirty", "Regiões sem cor", "Unpainted regions"),
    ("prod.lint_section", "Lint", "Lint"),
    ("prod.lint_stale", "O corte mudou desde este lint.", "The cut changed since this lint."),
    ("prod.lint_no_fill", "O corte não tem camada de preenchimento.", "The cut has no fill layer."),
    ("prod.regions", "regiões", "regions"),
    ("prod.cels", "cels", "cels"),
    ("prod.open_cut", "Abrir na Animação", "Open in Animation"),
    ("prod.open_cut.tip", "Abre o corte selecionado no workspace Animação (duplo clique na linha faz o mesmo)", "Opens the selected cut in the Animation workspace (double-clicking the row does the same)"),
    ("prod.none_in_filter", "Nenhum corte neste filtro.", "No cuts in this filter."),
    ("prod.corrections_short", "correção(ões)", "correction(s)"),
    ("prod.retakes_short", "retake(s)", "retake(s)"),
    ("prod.chip.tip", "Clique: avançar o estado. Botão direito: escolher.", "Click: advance the status. Right-click: choose."),
    ("prod.props_none", "Selecione um corte na tabela.", "Select a cut in the table."),
    ("prod.stages", "Etapas", "Stages"),
    ("prod.corrections", "Correções", "Corrections"),
    ("prod.no_corrections", "Sem correções neste corte.", "No corrections in this cut."),
    ("prod.goto_frame", "Abrir este quadro na Animação", "Open this frame in Animation"),
    ("prod.note_hint", "Nota da correção", "Correction note"),
    ("prod.add_correction", "Anotar", "Add note"),
    ("prod.correction_hint", "A correção cria a camada vermelha (correção) no quadro; desenhe nela pela Animação.", "The correction creates the red layer (correção) on that frame; draw on it in Animation."),
    ("prod.retakes", "Retakes", "Retakes"),
    ("prod.no_retakes", "Sem retakes neste corte.", "No retakes in this cut."),
    ("prod.reason_hint", "Motivo do retake", "Retake reason"),
    ("prod.add_retake", "Pedir retake", "Request retake"),
    ("prod.add_retake.tip", "Guarda o render atual do quadro como o antes, para comparar depois da refeitura", "Keeps the current render of the frame as the before, to compare after the redo"),
    ("prod.compare", "Comparar", "Compare"),
    ("prod.compare.tip", "Antes (snapshot) | depois (render atual) | diferença", "Before (snapshot) | after (current render) | difference"),
    ("prod.compare_stale", "O corte mudou desde esta comparação.", "The cut changed since this comparison."),
    ("prod.changed", "Pixels alterados", "Changed pixels"),
    ("prod.mean_diff", "diferença média", "mean difference"),
    ("prod.frame", "Quadro", "Frame"),
    ("prod.frame_current", "Atual", "Current"),
    ("prod.frame_current.tip", "Usa o quadro atual do editor", "Uses the editor's current frame"),
    ("unit.frames", "quadros", "frames"),
    ("unit.frames_short", "qd", "fr"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::PanelKind;
    use crate::workspace::{ToolId, WorkspaceId};
    use std::collections::HashSet;

    #[test]
    fn no_key_is_missing_or_empty_in_either_language() {
        let mut seen = HashSet::new();
        for (k, pt, en) in TABLE {
            assert!(seen.insert(*k), "chave duplicada: {k}");
            assert!(!k.is_empty() && !pt.trim().is_empty() && !en.trim().is_empty(), "texto vazio em {k}");
            assert!(k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'), "chave fora do padrão: {k}");
        }
    }

    #[test]
    fn lookup_switches_language_and_reports_unknown_keys() {
        assert_eq!(tr_in(Lang::Pt, "menu.file"), "Arquivo");
        assert_eq!(tr_in(Lang::En, "menu.file"), "File");
        assert_eq!(tr_in(Lang::En, "nao.existe"), "nao.existe");
        set_lang(Lang::En);
        assert_eq!(tr("workspace.animacao"), "Animation");
        set_lang(Lang::Pt);
        assert_eq!(tr("workspace.animacao"), "Animação");
    }

    #[test]
    fn language_codes_parse() {
        assert_eq!(Lang::from_code("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_code("EN"), Some(Lang::En));
        assert_eq!(Lang::from_code("jp"), None);
    }

    #[test]
    fn enum_keys_exist() {
        let exists = |k: &str| TABLE.iter().any(|(key, _, _)| *key == k);
        for w in WorkspaceId::ALL {
            assert!(exists(w.title_key()), "{}", w.title_key());
        }
        for t in ToolId::ALL {
            assert!(exists(t.title_key()), "{}", t.title_key());
        }
        for p in PanelKind::ALL {
            assert!(exists(p.title_key()) && exists(p.empty_key()), "{p:?}");
        }
    }

    /// Varre os fontes: toda chave em `tr("...")` e todo literal com prefixo de chave conhecido precisa existir na tabela.
    #[test]
    fn used_keys_exist() {
        const SOURCES: &[(&str, &str)] = &[
            ("app.rs", include_str!("app.rs")),
            ("app/doc.rs", include_str!("app/doc.rs")),
            ("app/bridge_host.rs", include_str!("app/bridge_host.rs")),
            ("app/ui_state.rs", include_str!("app/ui_state.rs")),
            ("app/commands.rs", include_str!("app/commands.rs")),
            ("app/editor.rs", include_str!("app/editor.rs")),
            ("app/render_service.rs", include_str!("app/render_service.rs")),
            ("app/tools.rs", include_str!("app/tools.rs")),
            ("app/canvas.rs", include_str!("app/canvas.rs")),
            ("app/ui_kit.rs", include_str!("app/ui_kit.rs")),
            ("app/actions.rs", include_str!("app/actions.rs")),
            ("app/panels/board.rs", include_str!("app/panels/board.rs")),
            ("app/panels/outline.rs", include_str!("app/panels/outline.rs")),
            ("app/panels/script.rs", include_str!("app/panels/script.rs")),
            ("app/panels/shots.rs", include_str!("app/panels/shots.rs")),
            ("app/panels/production.rs", include_str!("app/panels/production.rs")),
            ("app/panels/colors.rs", include_str!("app/panels/colors.rs")),
            ("app/panels/layers.rs", include_str!("app/panels/layers.rs")),
            ("app/panels/project.rs", include_str!("app/panels/project.rs")),
            ("app/panels/properties.rs", include_str!("app/panels/properties.rs")),
            ("app/panels/timeline.rs", include_str!("app/panels/timeline.rs")),
            ("layout.rs", include_str!("layout.rs")),
            ("workspace.rs", include_str!("workspace.rs")),
            ("brand.rs", include_str!("brand.rs")),
            ("theme.rs", include_str!("theme.rs")),
            ("cli.rs", include_str!("cli.rs")),
            ("prefs.rs", include_str!("prefs.rs")),
        ];
        const PREFIXES: &[&str] = &["workspace.", "tool.", "panel.", "menu.", "status.", "about.", "prefs.", "lang.", "doc.", "dialog.", "confirm.", "msg.", "blend.", "canvas.", "cel.", "cmd.", "err.", "fill.", "layer.", "layers.", "opt.", "palette.", "project.", "props.", "role.", "sel.", "stab.", "timeline.", "tip.", "tree.", "unit.", "outline.", "script.", "shots.", "board.", "prod."];
        let known: HashSet<&str> = TABLE.iter().map(|(k, _, _)| *k).collect();
        let mut used = 0;
        for (file, src) in SOURCES {
            // só o código de produção (o módulo de testes cita chaves inexistentes de propósito)
            let src = src.split("#[cfg(test)]").next().unwrap_or(src);
            let mut rest = src;
            while let Some(i) = rest.find('"') {
                let after = &rest[i + 1..];
                let Some(j) = after.find('"') else { break };
                let lit = &after[..j];
                rest = &after[j + 1..];
                let is_tr_arg = src[..src.len() - rest.len() - lit.len() - 2].ends_with("tr(");
                let looks_like_key = PREFIXES.iter().any(|p| lit.starts_with(p)) && !lit.contains(' ') && !lit.contains('{') && !lit.ends_with(".json");
                if is_tr_arg || looks_like_key {
                    used += 1;
                    assert!(known.contains(lit), "{file}: chave usada e inexistente: {lit}");
                }
            }
        }
        assert!(used > 60, "varredura achou poucas chaves ({used}); o teste quebrou");
    }
}
