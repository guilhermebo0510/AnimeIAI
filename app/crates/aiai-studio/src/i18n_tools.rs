//! Textos das ferramentas funcionais do S25 (à moda do Clip Studio): Seleção, Seleção automática, Conta-gotas, Balde, Mover/Operação e Texto. Tabela à parte do `TABLE`
//! principal (mesmo formato chave, pt, en), consultada pelo `tr` depois dela.

pub const TABLE: &[(&str, &str, &str)] = &[
    // Seleção: sub-ferramentas novas
    ("sel.pen", "Caneta de seleção", "Selection pen"),
    ("sel.erase", "Apagar seleção", "Erase selection"),
    ("sel.shrink", "Encolher seleção", "Shrink selection"),
    ("sel.magnetic", "Laço magnético", "Magnetic lasso"),
    ("sel.shrink.empty", "Não há desenho dentro do laço para encolher a seleção", "There is no drawing inside the lasso to shrink the selection to"),
    ("opt.sel_brush", "Tamanho", "Size"),
    ("opt.sel_brush.tip", "Diâmetro do pincel que pinta ou apaga a seleção (px do desenho). Alt com a caneta apaga", "Diameter of the brush that paints or erases the selection (drawing px). Alt with the pen erases"),
    ("opt.sel_magnet", "Alcance", "Reach"),
    ("opt.sel_magnet.tip", "Até onde o laço procura a borda do desenho para grudar (px do desenho)", "How far the lasso looks for the drawing's edge to stick to (drawing px)"),
    // Seleção automática
    ("wand.ref.editing", "Só a camada em edição", "Editing layer only"),
    ("wand.ref.all", "Todas as camadas", "All layers"),
    ("wand.ref.reference", "Camadas de referência", "Reference layers"),
    ("wand.no_reference", "Nenhuma camada de referência neste corte: ligue a referência do balde numa camada de linha (painel Camadas)", "No reference layer in this cut: turn on the bucket reference on a line layer (Layers panel)"),
    ("opt.wand_ref", "Referência", "Refer to"),
    ("opt.wand_gap", "Fechar intervalo", "Close gap"),
    ("opt.wand_gap.tip", "Frestas da linha até esta largura (px) contam como fechadas: a seleção não vaza por elas", "Gaps in the line up to this width (px) count as closed: the selection does not leak through them"),
    ("opt.wand_scale", "Dimensionar área", "Area scaling"),
    ("opt.wand_scale.tip", "Expande (positivo) ou contrai (negativo) a área escolhida, em px", "Expands (positive) or shrinks (negative) the picked area, in px"),
    // Conta-gotas
    ("eyedrop.shown", "Cor exibida", "Pick displayed color"),
    ("eyedrop.layer", "Cor da camada", "Pick color from layer"),
    // Balde
    ("fillm.others", "Referir outras camadas", "Refer other layers"),
    ("fillm.editing", "Só a camada em edição", "Refer only editing layer"),
    ("fillm.enclose", "Cercar e preencher", "Enclose and fill"),
    ("fillm.unfilled", "Pintar área não preenchida", "Paint unfilled area"),
    ("fillm.err_layer", "O balde pinta em camada raster, vetorial ou de preenchimento", "The bucket paints on a raster, vector or fill layer"),
    ("opt.fill_tolerance", "Tolerância", "Tolerance"),
    ("opt.fill_tolerance.tip", "0 a 255: quanto a cor pode diferir da do pixel clicado (por canal)", "0 to 255: how much a color may differ from the clicked pixel (per channel)"),
    ("opt.fill_brush", "Tamanho", "Size"),
    ("opt.fill_brush.tip", "Diâmetro do pincel que procura as áreas sem tinta (px do desenho)", "Diameter of the brush that looks for unpainted areas (drawing px)"),
    ("opt.fill_scale", "Dimensionar área", "Area scaling"),
    ("opt.fill_scale.tip", "Só a camada em edição: expande (positivo) ou contrai (negativo) a área pintada, em px", "Editing layer only: expands (positive) or shrinks (negative) the painted area, in px"),
    ("opt.fill_mode_hint.others", "Clique numa região: as linhas das camadas de referência delimitam e a cor vai para a camada de preenchimento", "Click a region: the reference layers' lines bound it and the color goes to the fill layer"),
    ("opt.fill_mode_hint.editing", "Clique numa região: as cores da camada ativa delimitam e a cor entra nela (pixels na raster)", "Click a region: the active layer's colors bound it and the color goes into it (pixels on raster)"),
    ("opt.fill_mode_hint.enclose", "Laço em volta: as regiões fechadas que ele cerca são pintadas", "Lasso around: the closed regions it encloses are painted"),
    ("opt.fill_mode_hint.unfilled", "Pincel por cima: as regiões ainda sem tinta que ele toca são pintadas", "Brush over: the still unpainted regions it touches are painted"),
    // Mão e Mover
    ("hand.rotate", "Girar", "Rotate"),
    ("move.pick_layer", "Selecionar camada", "Select layer"),
    ("text.circle", "Texto circular", "Circular text"),
    ("move.pick_none", "Nenhuma camada desenhada nesse ponto", "No layer is drawn at that point"),
];
