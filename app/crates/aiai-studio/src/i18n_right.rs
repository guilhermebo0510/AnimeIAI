//! Textos do lado direito e do topo das áreas de desenho (S25): barra de comandos, filtros, Navegador, Camadas,
//! Histórico, Propriedades da camada, barra de materiais e lançador da seleção. Tabela à parte do `TABLE` principal
//! (mesmo formato chave, pt, en), consultada pelo `tr` depois dela.

pub const TABLE: &[(&str, &str, &str)] = &[
    // menu e barra de comandos
    ("menu.filter", "Filtro", "Filter"),
    ("cmdbar.new", "Novo projeto", "New project"),
    ("cmdbar.open", "Abrir projeto", "Open project"),
    ("cmdbar.save", "Salvar", "Save"),
    ("cmdbar.undo", "Desfazer", "Undo"),
    ("cmdbar.redo", "Refazer", "Redo"),
    ("cmdbar.clear", "Limpar: o que está dentro da seleção (Delete no canvas), ou a camada inteira sem seleção", "Clear: what is inside the selection (Delete on the canvas), or the whole layer without one"),
    ("cmdbar.clear_outside", "Limpar fora da seleção", "Clear outside the selection"),
    ("cmdbar.fill_selection", "Preencher a seleção com a cor atual (camada raster)", "Fill the selection with the current color (raster layer)"),
    ("cmdbar.transform", "Transformar: escala e rotação da seleção ou da camada", "Transform: scale and rotate the selection or the layer"),
    ("cmdbar.deselect", "Cancelar a seleção", "Deselect"),
    ("cmdbar.invert_selection", "Inverter a seleção", "Invert the selection"),
    ("cmdbar.selection_border", "Mostrar a borda da seleção", "Show the selection border"),
    ("cmdbar.ruler_snap", "Encaixar os traços nas réguas (perspectiva, paralelas, radial, simetria, grade)", "Snap strokes to rulers (perspective, parallel, radial, symmetry, grid)"),
    ("cmdbar.filter", "Filtros da camada ativa", "Filters of the active layer"),
    ("cmdbar.grow", "Expandir a seleção 4 px", "Expand the selection by 4 px"),
    ("cmdbar.shrink", "Contrair a seleção 4 px", "Shrink the selection by 4 px"),
    ("cmdbar.copy_to_layer", "Copiar a seleção para uma camada nova", "Copy the selection to a new layer"),
    // filtros (efeitos da camada)
    ("filter.group.quick", "Efeitos rápidos", "Quick effects"),
    ("filter.group.correction", "Correção de cor", "Color correction"),
    ("filter.group.blur", "Desfoque", "Blur"),
    ("filter.group.light", "Luz e brilho", "Light and glow"),
    ("filter.group.style", "Estilo", "Style"),
    ("filter.group.lens", "Lente e filme", "Lens and film"),
    ("filter.group.render", "Renderizar", "Render"),
    ("filter.focus_lines", "Linhas de foco", "Focus lines"),
    ("filter.speed_lines", "Linhas de velocidade", "Speed lines"),
    ("filter.levels", "Níveis", "Levels"),
    ("filter.curves", "Curvas", "Curves"),
    ("filter.hue_saturation", "Matiz, saturação e luminosidade", "Hue, saturation and lightness"),
    ("filter.color_balance", "Equilíbrio de cor", "Color balance"),
    ("filter.brightness_contrast", "Brilho e contraste", "Brightness and contrast"),
    ("filter.gradient_map", "Mapa de degradê", "Gradient map"),
    ("filter.threshold", "Limiar", "Threshold"),
    ("filter.posterize", "Posterizar", "Posterize"),
    ("filter.no_layer", "Escolha uma camada para aplicar o filtro", "Pick a layer to apply the filter to"),
    ("filter.edit", "Ajustar os filtros da camada…", "Adjust the layer's filters…"),
    ("filter.window", "Filtro", "Filter"),
    // Navegador
    ("nav.zoom_out", "Afastar", "Zoom out"),
    ("nav.zoom_in", "Aproximar", "Zoom in"),
    ("nav.rot_reset", "Sem rotação", "Reset rotation"),
    ("nav.flip_v", "Espelhar a visão na vertical (o desenho não muda)", "Flip the view vertically (the drawing does not change)"),
    // Camadas
    ("layers.new_raster", "Nova camada raster", "New raster layer"),
    ("layers.new_vector", "Nova camada vetorial", "New vector layer"),
    ("layers.new_folder", "Nova pasta", "New folder"),
    ("layers.lock_active", "Travar a camada ativa", "Lock the active layer"),
    ("layers.fill_ref", "Referência do balde: o balde da camada de preenchimento usa esta camada como linha", "Fill reference: the fill layer's bucket uses this layer as line art"),
    ("layers.fill_ref.none", "Não há camada de preenchimento no corte para usar esta linha", "There is no fill layer in the cut to use this line art"),
    ("layers.rename", "Renomear", "Rename"),
    // Histórico
    ("hist.start", "Estado inicial", "Initial state"),
    ("hist.batch", "{n} comandos", "{n} commands"),
    ("hist.strokes", "Traço", "Stroke"),
    ("hist.erase_strokes", "Apagar traço", "Erase stroke"),
    ("hist.edit_strokes", "Editar traços", "Edit strokes"),
    ("hist.paint", "Pintura", "Painting"),
    ("hist.fill", "Balde", "Fill"),
    ("hist.restore", "Restaurar", "Restore"),
    ("hist.delete", "Apagar", "Delete"),
    ("hist.move", "Mover", "Move"),
    ("hist.visible", "Mostrar/ocultar", "Show/hide"),
    ("hist.lock", "Travar/destravar", "Lock/unlock"),
    ("hist.opacity", "Opacidade", "Opacity"),
    ("hist.blend", "Modo de mistura", "Blend mode"),
    ("hist.clip", "Recortar na de baixo", "Clip to below"),
    ("hist.effects", "Filtros", "Filters"),
    ("hist.rename", "Renomear", "Rename"),
    ("hist.props", "Propriedades", "Properties"),
    ("hist.edit", "Editar", "Edit"),
    ("hist.project", "Projeto", "Project"),
    ("hist.cells", "Folha de exposição", "Exposure sheet"),
    ("hist.track", "Trilha", "Track"),
    ("hist.new_layer", "Nova camada", "New layer"),
    ("hist.new_cel", "Novo cel", "New cel"),
    ("hist.new_cut", "Novo corte", "New cut"),
    ("hist.new_palette", "Nova paleta", "New palette"),
    ("hist.new_node", "Criar", "Create"),
    // Propriedades da camada
    ("lprops.filters", "Filtros", "Filters"),
    ("lprops.no_filters", "Sem filtros. Use o menu Filtro ou o botão abaixo.", "No filters. Use the Filter menu or the button below."),
    ("lprops.filter_toggle", "Ligar/desligar o filtro", "Turn the filter on/off"),
    ("lprops.filter_edit", "Ajustar os parâmetros", "Adjust the parameters"),
    ("lprops.filter_remove", "Tirar o filtro", "Remove the filter"),
    // barra de materiais
    ("mat.textures", "Texturas", "Textures"),
    ("mat.patterns", "Padrões", "Patterns"),
    ("mat.images", "Imagens", "Images"),
    ("mat.materials", "Materiais 3D", "3D materials"),
    ("mat.models", "Modelos 3D", "3D models"),
    ("mat.poses", "Poses", "Poses"),
    ("mat.brushes", "Pincéis", "Brushes"),
    ("mat.palettes", "Paletas e degradês", "Palettes and gradients"),
    ("mat.characters", "Personagens do projeto", "Project characters"),
    ("mat.empty", "Nada nesta categoria. Importe na Biblioteca (marque com a etiqueta \"padrão\" ou \"imagem\" para separar das texturas).", "Nothing in this category. Import in the Library (tag with \"pattern\" or \"image\" to set it apart from textures)."),
    ("mat.use_tip", "Duplo clique ou arraste para o canvas para usar", "Double-click or drag onto the canvas to use"),
    ("mat.no_characters", "O projeto ainda não tem personagens.", "The project has no characters yet."),
    ("mat.char_tip", "Duplo clique: a paleta de settei do personagem fica ativa nas Cores", "Double-click: the character's settei palette becomes active in Colors"),
    ("mat.char_palette", "Paleta de {name} ativa", "{name}'s palette is active"),
    ("mat.char_no_palette", "{name} não tem paleta de settei", "{name} has no settei palette"),
    // operações com a seleção
    ("selop.no_layer", "Escolha uma camada", "Pick a layer"),
    ("selop.locked", "A camada está travada", "The layer is locked"),
    ("selop.wrong_layer", "Funciona em camada raster ou vetorial", "Works on a raster or vector layer"),
    ("selop.no_cel", "A camada não tem desenho neste quadro", "The layer has no drawing on this frame"),
    ("selop.nothing", "Nada para mudar", "Nothing to change"),
    ("selop.need_selection", "Faça uma seleção antes", "Make a selection first"),
    ("selop.need_area", "Precisa de uma área selecionada", "Needs a selected area"),
    ("selop.clear_layer", "Limpar a camada", "Clear the layer"),
    ("selop.clear_selection", "Limpar a seleção", "Clear the selection"),
    ("selop.clear_outside", "Limpar fora da seleção", "Clear outside the selection"),
    ("selop.fill", "Preencher a seleção", "Fill the selection"),
    ("selop.fill_raster", "Preencher a seleção pinta pixels: escolha uma camada raster", "Filling the selection paints pixels: pick a raster layer"),
    ("selop.shrunk_away", "A seleção contraiu até sumir", "The selection shrank away"),
    ("selop.copy_suffix", "(cópia)", "(copy)"),
    ("selop.copy_to_layer", "Copiar para camada nova", "Copy to new layer"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Toda chave com os prefixos deste lado citada nos fontes dele existe (o teste geral só olha `tr("...")` e os
    /// prefixos antigos).
    #[test]
    fn keys_used_on_the_right_side_exist() {
        const SOURCES: &[(&str, &str)] = &[
            ("app/workbar.rs", include_str!("app/workbar.rs")),
            ("app/sel_ops.rs", include_str!("app/sel_ops.rs")),
            ("app/materials.rs", include_str!("app/materials.rs")),
            ("app/layer_thumbs.rs", include_str!("app/layer_thumbs.rs")),
            ("app/panels/history.rs", include_str!("app/panels/history.rs")),
            ("app/panels/layer_props.rs", include_str!("app/panels/layer_props.rs")),
            ("app/panels/navigator.rs", include_str!("app/panels/navigator.rs")),
            ("app/panels/layers.rs", include_str!("app/panels/layers.rs")),
        ];
        const PREFIXES: &[&str] = &["cmdbar.", "filter.", "nav.", "hist.", "lprops.", "mat.", "selop.", "layers.", "menu.", "canvas.", "lib.", "panel.", "fx.e."];
        let known: std::collections::HashSet<&str> = super::super::all().map(|(k, _, _)| *k).collect();
        let mut used = 0;
        for (file, src) in SOURCES {
            let src = src.split("#[cfg(test)]").next().unwrap_or(src);
            let mut rest = src;
            while let Some(i) = rest.find('"') {
                let after = &rest[i + 1..];
                let Some(j) = after.find('"') else { break };
                let lit = &after[..j];
                rest = &after[j + 1..];
                let key_like = PREFIXES.iter().any(|p| lit.starts_with(p)) && !lit.contains(' ') && !lit.contains('{') && !lit.ends_with('.') && lit.len() > 4;
                if key_like {
                    used += 1;
                    assert!(known.contains(lit), "{file}: chave usada e inexistente: {lit}");
                }
            }
        }
        assert!(used > 50, "varredura achou poucas chaves ({used})");
        for (k, pt, en) in TABLE {
            assert!(!pt.trim().is_empty() && !en.trim().is_empty(), "{k}");
        }
    }
}
