//! Textos das ferramentas funcionais do S25 (à moda do Clip Studio): Régua (linear, curva, figura, caneta de régua, especial, guia, simétrica). Tabela à parte do `TABLE`
//! principal (mesmo formato chave, pt, en), consultada pelo `tr` depois dela.

pub const TABLE: &[(&str, &str, &str)] = &[
    // tipos novos (sub-ferramentas da Régua, menu + e tipo no painel)
    ("ruler.kind.linear", "Régua linear", "Linear ruler"),
    ("ruler.kind.curve", "Régua curva", "Curve ruler"),
    ("ruler.kind.fig_rect", "Figura: retângulo", "Figure: rectangle"),
    ("ruler.kind.fig_ellipse", "Figura: elipse", "Figure: ellipse"),
    ("ruler.kind.fig_polygon", "Figura: polígono", "Figure: polygon"),
    ("ruler.kind.pen", "Caneta de régua", "Ruler pen"),
    ("ruler.kind.concentric", "Círculos concêntricos", "Concentric circles"),
    ("ruler.kind.guide", "Guia", "Guide"),
    // como criar
    ("ruler.curve_hint", "Clique os pontos da curva; Enter ou duplo clique termina, Backspace tira o último, Esc cancela", "Click the curve points; Enter or double-click finishes, Backspace removes the last one, Esc cancels"),
    ("ruler.curve_need_two", "A régua curva precisa de 2 pontos ou mais", "The curve ruler needs 2 points or more"),
    ("ruler.pen_hint", "Desenhe à mão: o traço vira uma régua curva", "Draw freehand: the stroke becomes a curve ruler"),
    ("ruler.figure_hint", "Arraste a caixa da figura (Shift: quadrado/círculo); o polígono vai do centro até um vértice", "Drag the figure box (Shift: square/circle); the polygon goes from the center to a vertex"),
    ("ruler.guide_hint", "Clique ou arraste: arrasto na horizontal faz guia horizontal, na vertical faz guia vertical", "Click or drag: a horizontal drag makes a horizontal guide, a vertical drag a vertical one"),
    ("ruler.snap_tip_s25", "A caneta, o pincel e a borracha encaixam nas réguas: linear, curva, figura e guia pegam o traço que começa a até 40 px da tela delas (a mais perto ganha) e o levam para cima delas; perspectiva, paralelas, radial, concêntricos e grade valem em todo o canvas", "The pen, the brush and the eraser snap to rulers: linear, curve, figure and guide take a stroke that starts within 40 screen px of them (the nearest wins) and put it on them; perspective, parallel, radial, concentric and grid work on the whole canvas"),
    // propriedades
    ("ruler.point_a", "Ponta A", "End A"),
    ("ruler.point_b", "Ponta B", "End B"),
    ("ruler.length", "Comprimento", "Length"),
    ("ruler.points", "Pontos", "Points"),
    ("ruler.closed", "Fechada", "Closed"),
    ("ruler.closed_tip", "O último ponto volta ao primeiro (precisa de 3 pontos)", "The last point joins the first (needs 3 points)"),
    ("ruler.size", "Tamanho", "Size"),
    ("ruler.sides", "Lados", "Sides"),
    ("ruler.ring_spacing_tip", "Distância entre os círculos mostrados (o traço segue o círculo onde começa)", "Distance between the circles shown (the stroke follows the circle it starts on)"),
    ("ruler.orientation", "Orientação", "Orientation"),
    ("ruler.horizontal", "Horizontal", "Horizontal"),
    ("ruler.vertical", "Vertical", "Vertical"),
    ("ruler.position", "Posição", "Position"),
];
