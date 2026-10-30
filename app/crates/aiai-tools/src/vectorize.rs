//! Vetorizador de lineart (Copiloto de traço, passo 1): imagem de linhas (preto sobre branco ou sobre transparente) →
//! caminhos pela LINHA CENTRAL de cada traço, com a largura medida ponto a ponto. Não traça o contorno (o "Potrace"
//! devolveria uma fita dupla); separa os cruzamentos em X pela continuidade da direção e corta os esporões que o
//! afinamento cria nas bordas.
//!
//! Etapas: tinta → máscara → distância até o fundo (EDT exata) → esqueleto de 1 px (Zhang-Suen + limpeza de degraus)
//! → grafo (pontas, junções, arestas) → poda de esporões → pareamento nas junções (menor desvio de direção) →
//! caminhos → posição suavizada, pontas estendidas pelo raio, largura pela tinta integrada na normal → simplificação.

use aiai_core::{ColorRef, Id, Stroke, StrokeGeometry, StrokePoint, VectorTip, WidthProfile};

/// Ajustes do vetorizador.
#[derive(Clone, Debug)]
pub struct VectorizeOptions {
    /// Tinta mínima (0..1) para um pixel ser linha.
    pub threshold: f32,
    /// Caminhos abertos mais curtos que isto (px) são descartados.
    pub min_length: f32,
    /// Esporão: ramo solto mais curto que `spur` × raio da junção (+2 px) sai.
    pub spur: f32,
    /// Desvio máximo (graus) para dois ramos de uma junção seguirem como o mesmo traço.
    pub join_angle_deg: f32,
    /// Tolerância (px) da simplificação Ramer-Douglas-Peucker.
    pub simplify: f32,
    /// Meia janela da média móvel das posições (0 = sem suavizar).
    pub smooth: usize,
}

impl Default for VectorizeOptions {
    fn default() -> Self {
        Self { threshold: 0.5, min_length: 3.0, spur: 1.6, join_angle_deg: 45.0, simplify: 0.4, smooth: 2 }
    }
}

/// Um traço achado: pontos (px da imagem, origem no canto superior esquerdo) e a largura total em cada ponto.
#[derive(Clone, Debug, PartialEq)]
pub struct InkPath {
    pub points: Vec<[f32; 2]>,
    pub widths: Vec<f32>,
    pub closed: bool,
}

impl InkPath {
    /// O mesmo caminho só com os pontos que a simplificação (Ramer-Douglas-Peucker, posição e largura) precisa, com
    /// tolerância `tol` px. Para quem reamostrou o caminho (cortar na borda de uma caixa) e quer devolver poucos pontos.
    pub fn simplified(&self, tol: f32) -> InkPath {
        if self.points.len() <= 2 {
            return self.clone();
        }
        let keep = rdp(&self.points, &self.widths, tol, self.closed);
        InkPath { points: keep.iter().map(|&k| self.points[k]).collect(), widths: keep.iter().map(|&k| self.widths[k]).collect(), closed: self.closed }
    }

    pub fn length(&self) -> f32 {
        let mut l: f32 = self.points.windows(2).map(|w| dist(w[0], w[1])).sum();
        if self.closed && self.points.len() > 2 {
            l += dist(self.points[0], self.points[self.points.len() - 1]);
        }
        l
    }
}

/// Tinta de uma imagem RGBA: escuro e opaco = tinta (`alpha × (1 − luminância)`); serve para preto sobre branco e
/// para preto sobre transparente.
pub fn ink_from_rgba(img: &image::RgbaImage) -> Vec<f32> {
    img.pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            let lum = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
            (a as f32 / 255.0) * (1.0 - lum)
        })
        .collect()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Grade com borda de 1 px de fundo (o afinamento e a EDT não precisam testar limites).
struct Grid {
    w: usize,
    h: usize,
}

impl Grid {
    #[inline]
    fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }
    /// Os 8 vizinhos na ordem P2..P9 de Zhang-Suen (N, NE, E, SE, S, SW, W, NW).
    #[inline]
    fn n8(&self, i: usize) -> [usize; 8] {
        let w = self.w;
        [i - w, i - w + 1, i + 1, i + w + 1, i + w, i + w - 1, i - 1, i - w - 1]
    }
}

/// Distância euclidiana exata (ao quadrado) de cada pixel até o fundo mais próximo (Felzenszwalb–Huttenlocher).
fn edt_sq(g: &Grid, fg: &[bool]) -> Vec<f32> {
    const INF: f32 = 1e20;
    let (w, h) = (g.w, g.h);
    let mut f: Vec<f32> = fg.iter().map(|&v| if v { INF } else { 0.0 }).collect();
    let n = w.max(h);
    let (mut d, mut v, mut z, mut col) = (vec![0f32; n], vec![0usize; n], vec![0f32; n + 1], vec![0f32; n]);
    let mut pass = |f: &mut [f32], len: usize, get: &dyn Fn(usize) -> usize| {
        for q in 0..len {
            col[q] = f[get(q)];
        }
        let mut k = 0usize;
        v[0] = 0;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..len {
            loop {
                let p = v[k];
                let s = ((col[q] + (q * q) as f32) - (col[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
                if s <= z[k] && k > 0 {
                    k -= 1;
                    continue;
                }
                if s <= z[k] {
                    // k == 0: substitui
                    v[0] = q;
                    z[0] = -INF;
                    z[1] = INF;
                    break;
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = INF;
                break;
            }
        }
        k = 0;
        for q in 0..len {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let p = v[k];
            d[q] = (q as f32 - p as f32).powi(2) + col[p];
        }
        for q in 0..len {
            f[get(q)] = d[q];
        }
    };
    for x in 0..w {
        pass(&mut f, h, &|q| q * w + x);
    }
    for y in 0..h {
        pass(&mut f, w, &|q| y * w + q);
    }
    f
}

/// Afinamento de Zhang-Suen até 1 px, depois tira os degraus (pixel cujos dois vizinhos de lado já se tocam pela
/// diagonal), para cada pixel do meio ter exatamente dois vizinhos.
fn thin(g: &Grid, mask: &mut [bool]) {
    let mut del = Vec::new();
    loop {
        let mut changed = false;
        for step in 0..2 {
            del.clear();
            for y in 1..g.h - 1 {
                for x in 1..g.w - 1 {
                    let i = g.idx(x, y);
                    if !mask[i] {
                        continue;
                    }
                    let nb = g.n8(i).map(|j| mask[j]);
                    let b = nb.iter().filter(|&&v| v).count();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let a = (0..8).filter(|&k| !nb[k] && nb[(k + 1) % 8]).count();
                    if a != 1 {
                        continue;
                    }
                    let (p2, p4, p6, p8) = (nb[0], nb[2], nb[4], nb[6]);
                    let ok = if step == 0 { !(p2 && p4 && p6) && !(p4 && p6 && p8) } else { !(p2 && p4 && p8) && !(p2 && p6 && p8) };
                    if ok {
                        del.push(i);
                    }
                }
            }
            for &i in &del {
                mask[i] = false;
            }
            changed |= !del.is_empty();
        }
        if !changed {
            break;
        }
    }
    // degraus: um pixel sai se os vizinhos dele continuam ligados entre si sem ele (um só grupo, contando a diagonal)
    // e ele não é ponta. Em escada, cada degrau tinha três vizinhos e parecia junção.
    const OFF: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];
    for _ in 0..4 {
        let mut changed = false;
        for y in 1..g.h - 1 {
            for x in 1..g.w - 1 {
                let i = g.idx(x, y);
                if !mask[i] {
                    continue;
                }
                let nb: Vec<usize> = (0..8).filter(|&k| mask[g.n8(i)[k]]).collect();
                if nb.len() < 2 {
                    continue;
                }
                // grupos de vizinhos (adjacentes entre si em 8 direções)
                let mut group = vec![usize::MAX; nb.len()];
                let mut groups = 0;
                for s0 in 0..nb.len() {
                    if group[s0] != usize::MAX {
                        continue;
                    }
                    group[s0] = groups;
                    let mut stack = vec![s0];
                    while let Some(u) = stack.pop() {
                        for v in 0..nb.len() {
                            let (du, dv) = (OFF[nb[u]], OFF[nb[v]]);
                            if group[v] == usize::MAX && (du.0 - dv.0).abs() <= 1 && (du.1 - dv.1).abs() <= 1 {
                                group[v] = groups;
                                stack.push(v);
                            }
                        }
                    }
                    groups += 1;
                }
                if groups == 1 {
                    mask[i] = false;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Nó do grafo: um ponto solto, uma ponta ou um grupo de pixels de junção.
struct Node {
    pixels: Vec<usize>,
    center: [f32; 2],
    radius: f32,
    edges: Vec<(usize, bool)>, // (aresta, este nó é o começo dela)
}

struct Edge {
    pixels: Vec<usize>,
    a: usize,
    b: usize,
    alive: bool,
}

/// Vetoriza a tinta `ink` (w × h, 0..1).
pub fn vectorize(w: u32, h: u32, ink: &[f32], o: &VectorizeOptions) -> Vec<InkPath> {
    let (w, h) = (w as usize, h as usize);
    assert_eq!(ink.len(), w * h, "tinta do tamanho da imagem");
    let debug = std::env::var_os("AIAI_VEC_DEBUG").is_some();
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let g = Grid { w: w + 2, h: h + 2 };
    let at = |x: usize, y: usize| ink[y * w + x];
    let mut mask = vec![false; g.w * g.h];
    for y in 0..h {
        for x in 0..w {
            mask[g.idx(x + 1, y + 1)] = at(x, y) >= o.threshold;
        }
    }
    let d2 = edt_sq(&g, &mask);
    let mut skel = mask.clone();
    thin(&g, &mut skel);
    let pos = |i: usize| [(i % g.w) as f32 - 0.5, (i / g.w) as f32 - 0.5]; // centro do pixel na imagem original
    let rad = |i: usize| (d2[i].sqrt() - 0.5).max(0.5);
    let deg = |i: usize| g.n8(i).iter().filter(|&&j| skel[j]).count();

    // nós: pontas (grau ≠ 2) e grupos de junção (grau ≥ 3 ligados)
    let mut node_of = vec![usize::MAX; skel.len()];
    let mut nodes: Vec<Node> = Vec::new();
    for i in 0..skel.len() {
        if !skel[i] || node_of[i] != usize::MAX || deg(i) == 2 {
            continue;
        }
        let id = nodes.len();
        let mut pixels = vec![i];
        node_of[i] = id;
        if deg(i) >= 3 {
            let mut k = 0;
            while k < pixels.len() {
                let p = pixels[k];
                for j in g.n8(p) {
                    if skel[j] && node_of[j] == usize::MAX && deg(j) >= 3 {
                        node_of[j] = id;
                        pixels.push(j);
                    }
                }
                k += 1;
            }
        }
        let n = pixels.len() as f32;
        let center = pixels.iter().fold([0.0, 0.0], |c, &p| [c[0] + pos(p)[0] / n, c[1] + pos(p)[1] / n]);
        let radius = pixels.iter().map(|&p| rad(p)).fold(0.0, f32::max);
        nodes.push(Node { pixels, center, radius, edges: Vec::new() });
    }

    // arestas: de cada nó, segue os pixels de grau 2 até outro nó
    let mut used = vec![false; skel.len()];
    let mut edges: Vec<Edge> = Vec::new();
    let mut direct: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for nid in 0..nodes.len() {
        let starts: Vec<usize> = nodes[nid].pixels.clone();
        for s in starts {
            for q in g.n8(s) {
                if !skel[q] || node_of[q] == nid {
                    continue;
                }
                if node_of[q] != usize::MAX {
                    // dois nós encostados: aresta de 2 pixels
                    let key = (s.min(q), s.max(q));
                    if direct.insert(key) {
                        edges.push(Edge { pixels: vec![s, q], a: nid, b: node_of[q], alive: true });
                    }
                    continue;
                }
                if used[q] {
                    continue;
                }
                let mut px = vec![s, q];
                used[q] = true;
                let (mut prev, mut cur) = (s, q);
                let end = loop {
                    // o próximo: vizinho na linha que não é o anterior (lado antes da diagonal)
                    let nbs = g.n8(cur);
                    let next = [0usize, 2, 4, 6, 1, 3, 5, 7]
                        .iter()
                        .map(|&k| nbs[k])
                        .find(|&j| skel[j] && j != prev && !px.contains(&j) && !(node_of[j] == nid && px.len() < 4) && (node_of[j] != usize::MAX || !used[j]));
                    match next {
                        Some(j) if node_of[j] != usize::MAX => {
                            px.push(j);
                            break node_of[j];
                        }
                        Some(j) => {
                            used[j] = true;
                            px.push(j);
                            prev = cur;
                            cur = j;
                        }
                        None => break usize::MAX,
                    }
                };
                if end == usize::MAX {
                    // linha que não fecha num nó (não deveria acontecer): vira ponta solta
                    let last = *px.last().unwrap();
                    let id = nodes.len();
                    node_of[last] = id;
                    nodes.push(Node { pixels: vec![last], center: pos(last), radius: rad(last), edges: Vec::new() });
                    edges.push(Edge { pixels: px, a: nid, b: id, alive: true });
                } else {
                    edges.push(Edge { pixels: px, a: nid, b: end, alive: true });
                }
            }
        }
    }
    // laços sem nó (um anel fechado): cada um vira caminho fechado
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for i in 0..skel.len() {
        if !skel[i] || used[i] || node_of[i] != usize::MAX {
            continue;
        }
        let mut px = vec![i];
        used[i] = true;
        let mut cur = i;
        loop {
            let nbs = g.n8(cur);
            match [0usize, 2, 4, 6, 1, 3, 5, 7].iter().map(|&k| nbs[k]).find(|&j| skel[j] && !used[j]) {
                Some(j) => {
                    used[j] = true;
                    px.push(j);
                    cur = j;
                }
                None => break,
            }
        }
        if px.len() >= 3 {
            rings.push(px);
        }
    }
    for (eid, e) in edges.iter().enumerate() {
        nodes[e.a].edges.push((eid, true));
        nodes[e.b].edges.push((eid, false));
    }

    // poda de esporões: ramo que termina numa ponta solta, curto perto de uma junção
    for _ in 0..3 {
        let mut pruned = false;
        for eid in 0..edges.len() {
            if !edges[eid].alive {
                continue;
            }
            let (a, b) = (edges[eid].a, edges[eid].b);
            let alive_deg = |n: usize| nodes[n].edges.iter().filter(|(e, _)| edges[*e].alive).count();
            let (da, db) = (alive_deg(a), alive_deg(b));
            let (tip, junction) = if da == 1 && db >= 3 {
                (a, b)
            } else if db == 1 && da >= 3 {
                (b, a)
            } else {
                continue;
            };
            let _ = tip;
            let len = edges[eid].pixels.len() as f32;
            if len < o.spur * nodes[junction].radius + 2.0 {
                edges[eid].alive = false;
                pruned = true;
            }
        }
        if !pruned {
            break;
        }
    }

    // laço pequeno: duas linhas que se encontram quase paralelas deixam um buraquinho entre elas, e o esqueleto o
    // contorna com duas ligações entre as mesmas junções; a curta sai (virava um caroço no traço)
    {
        let mut by_pair: std::collections::HashMap<(usize, usize), Vec<usize>> = std::collections::HashMap::new();
        for (eid, e) in edges.iter().enumerate() {
            if e.alive && e.a != e.b {
                by_pair.entry((e.a.min(e.b), e.a.max(e.b))).or_default().push(eid);
            }
        }
        for ((a, b), mut list) in by_pair {
            if list.len() < 2 {
                continue;
            }
            list.sort_by_key(|&e| edges[e].pixels.len());
            let limit = 3.0 * (nodes[a].radius + nodes[b].radius) + 6.0;
            for &e in &list[..list.len() - 1] {
                if (edges[e].pixels.len() as f32) < limit {
                    edges[e].alive = false;
                }
            }
        }
    }

    // junções vizinhas: o afinamento parte o cruzamento em X em dois nós de grau 3 ligados por um trecho curto
    loop {
        let alive_deg = |n: usize, nodes: &Vec<Node>, edges: &Vec<Edge>| nodes[n].edges.iter().filter(|(e, _)| edges[*e].alive).count();
        let mut merged = false;
        for eid in 0..edges.len() {
            if !edges[eid].alive {
                continue;
            }
            let (a, b) = (edges[eid].a, edges[eid].b);
            if a == b {
                if (edges[eid].pixels.len() as f32) <= 6.0 * nodes[a].radius + 6.0 {
                    edges[eid].alive = false;
                    merged = true;
                }
                continue;
            }
            if alive_deg(a, &nodes, &edges) < 3 || alive_deg(b, &nodes, &edges) < 3 {
                continue;
            }
            if (edges[eid].pixels.len() as f32) > nodes[a].radius + nodes[b].radius + 3.0 {
                continue;
            }
            edges[eid].alive = false;
            let moved: Vec<(usize, bool)> = std::mem::take(&mut nodes[b].edges);
            for &(e2, start) in &moved {
                if start {
                    edges[e2].a = a;
                } else {
                    edges[e2].b = a;
                }
            }
            nodes[a].edges.extend(moved);
            let px = std::mem::take(&mut nodes[b].pixels);
            nodes[a].pixels.extend(px);
            let n = nodes[a].pixels.len() as f32;
            nodes[a].center = nodes[a].pixels.iter().fold([0.0, 0.0], |c, &p| [c[0] + pos(p)[0] / n, c[1] + pos(p)[1] / n]);
            nodes[a].radius = nodes[a].pixels.iter().map(|&p| rad(p)).fold(0.0, f32::max);
            merged = true;
        }
        if !merged {
            break;
        }
    }

    if debug {
        for (i, n) in nodes.iter().enumerate() {
            let live: Vec<_> = n.edges.iter().filter(|(e, _)| edges[*e].alive).map(|(e, s)| (*e, *s, edges[*e].pixels.len())).collect();
            if !live.is_empty() {
                eprintln!("nó {i} centro {:?} raio {:.1} px {} ramos {:?}", n.center, n.radius, n.pixels.len(), live);
            }
        }
    }
    // direção de saída de cada ramo numa junção
    let leave_dir = |e: &Edge, from_start: bool, node: &Node| -> [f32; 2] {
        let reach = (2.5 * node.radius + 3.0).clamp(4.0, 14.0);
        let seq: Box<dyn Iterator<Item = &usize>> = if from_start { Box::new(e.pixels.iter()) } else { Box::new(e.pixels.iter().rev()) };
        let mut far = node.center;
        for &p in seq {
            far = pos(p);
            if dist(far, node.center) >= reach {
                break;
            }
        }
        let v = [far[0] - node.center[0], far[1] - node.center[1]];
        let l = v[0].hypot(v[1]).max(1e-6);
        [v[0] / l, v[1] / l]
    };
    // pareamento: em cada nó, os dois ramos mais "em linha reta" seguem juntos
    let max_dev = o.join_angle_deg.to_radians().cos();
    let mut link: std::collections::HashMap<(usize, bool), (usize, bool)> = std::collections::HashMap::new();
    for node in &nodes {
        let live: Vec<(usize, bool)> = node.edges.iter().copied().filter(|(e, _)| edges[*e].alive).collect();
        if live.len() < 2 {
            continue;
        }
        let dirs: Vec<[f32; 2]> = live.iter().map(|&(e, s)| leave_dir(&edges[e], s, node)).collect();
        let mut cand: Vec<(f32, usize, usize)> = Vec::new();
        for i in 0..live.len() {
            for j in i + 1..live.len() {
                if live[i].0 == live[j].0 {
                    continue; // a mesma aresta voltando ao nó (laço): fecha pelo próprio caminho
                }
                // seguir reto = direções opostas: −cos
                let straight = -(dirs[i][0] * dirs[j][0] + dirs[i][1] * dirs[j][1]);
                cand.push((straight, i, j));
            }
        }
        cand.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut taken = vec![false; live.len()];
        for (straight, i, j) in cand {
            if taken[i] || taken[j] {
                continue;
            }
            if live.len() > 2 && straight < max_dev {
                continue;
            }
            taken[i] = true;
            taken[j] = true;
            link.insert(live[i], live[j]);
            link.insert(live[j], live[i]);
        }
    }

    // caminhos: começa numa ponta sem par e segue os pares; o que sobrar todo pareado é fechado
    let mut visited = vec![false; edges.len()];
    let mut paths: Vec<(Vec<[f32; 2]>, Vec<f32>, bool, [bool; 2])> = Vec::new();
    let node_at = |e: &Edge, start: bool| if start { e.a } else { e.b };
    let walk = |first: (usize, bool), visited: &mut Vec<bool>| -> (Vec<usize>, Vec<usize>, bool, usize) {
        // devolve pixels, os nós de junção atravessados, se fechou e o nó do fim
        let mut px: Vec<usize> = Vec::new();
        let mut through: Vec<usize> = Vec::new();
        let (mut e, mut from_start) = first;
        loop {
            visited[e] = true;
            let ed = &edges[e];
            let seq: Vec<usize> = if from_start { ed.pixels.clone() } else { ed.pixels.iter().rev().copied().collect() };
            for p in seq {
                if px.last() != Some(&p) {
                    px.push(p);
                }
            }
            let far_end = (e, !from_start);
            match link.get(&far_end) {
                Some(&(e2, s2)) if !visited[e2] => {
                    through.push(node_at(ed, !from_start));
                    e = e2;
                    from_start = s2;
                }
                Some(&(e2, _)) if e2 == first.0 => return (px, through, true, node_at(ed, !from_start)),
                _ => return (px, through, false, node_at(ed, !from_start)),
            }
        }
    };
    for eid in 0..edges.len() {
        for start in [true, false] {
            if !edges[eid].alive || visited[eid] || link.contains_key(&(eid, start)) {
                continue;
            }
            let end_node = node_at(&edges[eid], start);
            let (px, through, closed, last_node) = walk((eid, start), &mut visited);
            let free = |n: usize| nodes[n].edges.iter().filter(|(e, _)| edges[*e].alive).count() == 1;
            paths.push(finish(&px, &through, closed, [free(end_node), free(last_node)], &nodes, &pos, &rad));
        }
    }
    for eid in 0..edges.len() {
        if edges[eid].alive && !visited[eid] {
            let (px, through, _, _) = walk((eid, true), &mut visited);
            paths.push(finish(&px, &through, true, [false, false], &nodes, &pos, &rad));
        }
    }
    for r in &rings {
        paths.push(finish(r, &[], true, [false, false], &nodes, &pos, &rad));
    }

    // posições suavizadas, pontas estendidas, largura medida, simplificação
    let ink_at = |x: f32, y: f32| -> f32 {
        // bilinear no centro dos pixels; fora = sem tinta
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let get = |xx: f32, yy: f32| -> f32 {
            if xx < 0.0 || yy < 0.0 || xx >= w as f32 || yy >= h as f32 {
                0.0
            } else {
                at(xx as usize, yy as usize)
            }
        };
        let a = get(x0, y0) * (1.0 - tx) + get(x0 + 1.0, y0) * tx;
        let b = get(x0, y0 + 1.0) * (1.0 - tx) + get(x0 + 1.0, y0 + 1.0) * tx;
        a * (1.0 - ty) + b * ty
    };
    let mut out = Vec::new();
    for (pts, mut radii, closed, free) in paths {
        if pts.len() < 2 {
            continue;
        }
        let mut pts = smooth(&pts, o.smooth, closed);
        if !closed {
            // o esqueleto para antes da ponta de verdade (e a ponta afinada some do limiar): segue a direção do fim
            // enquanto houver tinta
            for end in [1usize, 0] {
                if !free[end] || pts.len() < 2 {
                    continue;
                }
                let n = pts.len();
                let back = 4.min(n - 1);
                let (tip, inner) = if end == 1 { (pts[n - 1], pts[n - 1 - back]) } else { (pts[0], pts[back]) };
                let l = dist(tip, inner).max(1e-6);
                let d = [(tip[0] - inner[0]) / l, (tip[1] - inner[1]) / l];
                let r = if end == 1 { radii[radii.len() - 1] } else { radii[0] };
                let mut last = tip;
                let mut s = 0.5;
                while s <= 3.0 * r + 8.0 {
                    let q = [tip[0] + d[0] * s, tip[1] + d[1] * s];
                    if ink_at(q[0], q[1]) < 0.12 {
                        break;
                    }
                    last = q;
                    s += 0.5;
                }
                if dist(last, tip) > 0.25 {
                    if end == 1 {
                        pts.push(last);
                        radii.push(0.0);
                    } else {
                        pts.insert(0, last);
                        radii.insert(0, 0.0);
                    }
                }
            }
        }
        let n = pts.len();
        let mut widths = Vec::with_capacity(n);
        for k in 0..n {
            let (a, b) = if closed { (pts[(k + n - 1) % n], pts[(k + 1) % n]) } else { (pts[k.saturating_sub(1)], pts[(k + 1).min(n - 1)]) };
            let t = [b[0] - a[0], b[1] - a[1]];
            let l = t[0].hypot(t[1]).max(1e-6);
            let nrm = [-t[1] / l, t[0] / l];
            let r_edt = radii[k.min(radii.len() - 1)];
            // tinta somada na normal, contínua a partir do centro: largura com precisão de subpixel; o centro de
            // massa dessa tinta recentra o ponto (o esqueleto fica num pixel inteiro, a linha não)
            let mut sum = 0.0f32;
            let mut moment = 0.0f32;
            let step = 0.25f32;
            let reach = 2.0 * r_edt + 3.0;
            let mut clean = true;
            for side in [1.0f32, -1.0] {
                let mut s = if side > 0.0 { 0.0 } else { step };
                let mut low = false;
                while s <= reach {
                    let v = ink_at(pts[k][0] + side * s * nrm[0], pts[k][1] + side * s * nrm[1]);
                    if v < 0.02 {
                        low = true;
                        break;
                    }
                    sum += v * step;
                    moment += side * s * v * step;
                    s += step;
                }
                clean &= low;
            }
            let wv = if clean {
                let off = (moment / sum.max(1e-6)).clamp(-1.0, 1.0);
                pts[k] = [pts[k][0] + off * nrm[0], pts[k][1] + off * nrm[1]];
                Some(sum)
            } else {
                None // encostado em outra linha (junção, cruzamento): a medida atravessaria a outra
            };
            widths.push(wv);
        }
        // onde a medida não vale, a largura vem dos vizinhos limpos ao longo do traço (sem caroço nas junções)
        let widths: Vec<f32> = if widths.iter().all(|v| v.is_none()) {
            radii.iter().map(|r| 2.0 * r).collect()
        } else {
            let known: Vec<usize> = (0..n).filter(|&k| widths[k].is_some()).collect();
            (0..n)
                .map(|k| {
                    if let Some(v) = widths[k] {
                        return v;
                    }
                    let prev = known.iter().rev().find(|&&j| j < k).copied();
                    let next = known.iter().find(|&&j| j > k).copied();
                    let (prev, next) = if closed { (prev.or(known.last().copied()), next.or(known.first().copied())) } else { (prev, next) };
                    match (prev, next) {
                        (Some(a), Some(b)) if a != b => {
                            let span = if b > a { b - a } else { b + n - a };
                            let off = if k > a { k - a } else { k + n - a };
                            let t = off as f32 / span as f32;
                            widths[a].unwrap() + (widths[b].unwrap() - widths[a].unwrap()) * t
                        }
                        (Some(a), _) | (_, Some(a)) => widths[a].unwrap(),
                        _ => 2.0 * radii[k.min(radii.len() - 1)],
                    }
                })
                .collect()
        };
        let widths = median3(&widths, closed);
        // duas linhas que se encostam de raspão viram um trecho grosso: pico de largura bem acima da vizinhança
        // (janela de ±6) volta para a mediana dela. Afinamentos (para baixo) ficam.
        let widths: Vec<f32> = (0..widths.len())
            .map(|k| {
                let n = widths.len();
                let mut win: Vec<f32> = (-6i64..=6)
                    .filter_map(|d| {
                        let j = k as i64 + d;
                        if closed {
                            Some(widths[j.rem_euclid(n as i64) as usize])
                        } else if j >= 0 && (j as usize) < n {
                            Some(widths[j as usize])
                        } else {
                            None
                        }
                    })
                    .collect();
                win.sort_by(f32::total_cmp);
                let med = win[win.len() / 2];
                if widths[k] > med * 1.4 + 0.5 { med } else { widths[k] }
            })
            .collect();
        let keep = rdp(&pts, &widths, o.simplify, closed);
        let path = InkPath { points: keep.iter().map(|&k| pts[k]).collect(), widths: keep.iter().map(|&k| widths[k]).collect(), closed };
        if path.points.len() >= 2 && (path.closed || path.length() >= o.min_length) {
            out.push(path);
        }
    }
    out
}

/// Pixels do caminho → pontos (centro dos pixels) e raios; perto das junções atravessadas, troca o trecho torto do
/// esqueleto por uma ponte reta (o afinamento entorta os dois ramos na direção um do outro).
fn finish(px: &[usize], through: &[usize], closed: bool, free: [bool; 2], nodes: &[Node], pos: &dyn Fn(usize) -> [f32; 2], rad: &dyn Fn(usize) -> f32) -> (Vec<[f32; 2]>, Vec<f32>, bool, [bool; 2]) {
    let mut pts: Vec<[f32; 2]> = Vec::with_capacity(px.len());
    let mut radii: Vec<f32> = Vec::with_capacity(px.len());
    for &p in px {
        let q = pos(p);
        // dentro do raio de uma junção atravessada: fica de fora (vira ponte)
        if through.iter().any(|&n| dist(q, nodes[n].center) < nodes[n].radius * 1.2 + 1.0) {
            continue;
        }
        pts.push(q);
        radii.push(rad(p));
    }
    if pts.len() < 2 {
        pts = px.iter().map(|&p| pos(p)).collect();
        radii = px.iter().map(|&p| rad(p)).collect();
    }
    // ponte reta: pontos novos nos buracos deixados pelas junções (passo de 1 px)
    let mut bp = Vec::with_capacity(pts.len());
    let mut br = Vec::with_capacity(pts.len());
    for k in 0..pts.len() {
        if k > 0 {
            let (a, b) = (pts[k - 1], pts[k]);
            let d = dist(a, b);
            if d > 1.6 {
                let n = d.ceil() as usize;
                for s in 1..n {
                    let t = s as f32 / n as f32;
                    bp.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                    br.push(radii[k - 1] + (radii[k] - radii[k - 1]) * t);
                }
            }
        }
        bp.push(pts[k]);
        br.push(radii[k]);
    }
    (bp, br, closed, free)
}

fn smooth(p: &[[f32; 2]], half: usize, closed: bool) -> Vec<[f32; 2]> {
    if half == 0 || p.len() < 3 {
        return p.to_vec();
    }
    let n = p.len() as isize;
    (0..p.len())
        .map(|i| {
            if !closed && (i == 0 || i == p.len() - 1) {
                return p[i];
            }
            let mut h = half as isize;
            if !closed {
                h = h.min(i as isize).min(n - 1 - i as isize);
            }
            let (mut sx, mut sy) = (0.0, 0.0);
            for k in -h..=h {
                let j = ((i as isize + k).rem_euclid(n)) as usize;
                sx += p[j][0];
                sy += p[j][1];
            }
            let c = (2 * h + 1) as f32;
            [sx / c, sy / c]
        })
        .collect()
}

fn median3(v: &[f32], closed: bool) -> Vec<f32> {
    let n = v.len();
    if n < 3 {
        return v.to_vec();
    }
    (0..n)
        .map(|i| {
            if !closed && (i == 0 || i == n - 1) {
                return v[i];
            }
            let mut t = [v[(i + n - 1) % n], v[i], v[(i + 1) % n]];
            t.sort_by(f32::total_cmp);
            t[1]
        })
        .collect()
}

/// Índices que ficam na simplificação de Ramer-Douglas-Peucker.
fn rdp(p: &[[f32; 2]], wv: &[f32], tol: f32, closed: bool) -> Vec<usize> {
    let n = p.len();
    if n <= 2 || tol <= 0.0 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    let seg_d = |q: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        let (vx, vy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = vx * vx + vy * vy;
        if l2 < 1e-12 {
            return dist(q, a);
        }
        let t = (((q[0] - a[0]) * vx + (q[1] - a[1]) * vy) / l2).clamp(0.0, 1.0);
        dist(q, [a[0] + vx * t, a[1] + vy * t])
    };
    let mut stack = Vec::new();
    if closed {
        // fechado: parte no ponto mais longe do primeiro
        let far = (1..n).max_by(|&a, &b| dist(p[a], p[0]).total_cmp(&dist(p[b], p[0]))).unwrap();
        keep[0] = true;
        keep[far] = true;
        stack.push((0, far));
        stack.push((far, n));
    } else {
        keep[0] = true;
        keep[n - 1] = true;
        stack.push((0, n - 1));
    }
    while let Some((a, b)) = stack.pop() {
        let pb = p[b % n];
        let (wa, wb) = (wv[a], wv[b % n]);
        let mut best = (0.0f32, 0usize);
        for k in a + 1..b {
            // a largura também conta: um ponto fica se a posição OU a largura fugirem da reta entre os vizinhos
            let t = (k - a) as f32 / (b - a) as f32;
            let dw = (wv[k] - (wa + (wb - wa) * t)).abs() * 0.5;
            let d = seg_d(p[k], p[a], pb).max(dw);
            if d > best.0 {
                best = (d, k);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    (0..n).filter(|&k| keep[k]).collect()
}

/// Um caminho achado vira traço do app: largura base = mediana medida, cada ponto com o seu multiplicador (a ponta
/// não afina por conta própria: a largura já vem do desenho). `scale` e `offset` levam da imagem ao canvas.
pub fn path_to_stroke(p: &InkPath, tip: VectorTip, color: ColorRef, scale: f32, offset: [f32; 2]) -> Stroke {
    let mut ws = p.widths.clone();
    ws.sort_by(f32::total_cmp);
    let base = ws.get(ws.len() / 2).copied().unwrap_or(1.0).max(0.5);
    let n = p.points.len();
    let mut s = Stroke { taper: None,
        id: Id(String::new()),
        points: p
            .points
            .iter()
            .zip(&p.widths)
            .map(|(q, &wv)| StrokePoint { x: q[0] * scale + offset[0], y: q[1] * scale + offset[1], pressure: 1.0, tilt: None, t: None, corner: false, width: wv / base, id: 0, handle: None })
            .collect(),
        geometry: StrokeGeometry::Spline,
        tension: 0.0,
        tip,
        width: base * scale,
        profile: WidthProfile::default(),
        color,
        opacity: 1.0,
        closed: p.closed,
        aliased: false,
        color_trace: false,
        group: String::new(),
    };
    // as pontas medidas já trazem o afinamento do desenho: sem isto a ponta G-Pen (e as outras que afinam) afinava
    // de novo por cima, e traços curtos saíam finos demais (achado desenhando a cena 15)
    if !p.closed && n >= 2 {
        s.taper = Some(aiai_core::StrokeTaper::NONE);
    }
    s
}

/// Como a largura dos traços achados é decidida (Copiloto de traço, seção 6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WidthMode {
    /// A largura medida no desenho, ponto a ponto (padrão).
    #[default]
    Drawing,
    /// Todos com `width`; a ponta da caneta decide o afinamento.
    Pen,
    /// Todos com `width`, sem variação.
    Fixed,
}

/// De onde vem a lineart: um arquivo de imagem ou uma camada raster do mesmo corte (o cel exposto no quadro).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VectorizeSource {
    /// Caminho absoluto; a imagem é ajustada ao canvas como na importação (`fit`).
    Image {
        path: String,
        #[serde(default)]
        fit: crate::import::ImageFit,
    },
    /// Camada raster: o cel exposto no `frame`.
    Layer { layer: Id },
}

/// Pedido: vetorizar a lineart da fonte e pôr os traços no cel exposto da camada vetorial `layer` no `frame`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct VectorizeRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada VETORIAL que recebe os traços.
    pub layer: Id,
    pub source: VectorizeSource,
    #[serde(default)]
    pub tip: VectorTip,
    #[serde(default)]
    pub color: ColorRef,
    #[serde(default)]
    pub width_mode: WidthMode,
    /// Largura em px para `pen` e `fixed`.
    #[serde(default)]
    pub width: Option<f32>,
    /// Tinta mínima 0..1 (padrão 0,5).
    #[serde(default)]
    pub threshold: Option<f32>,
    /// Tolerância da simplificação em px (padrão 0,4).
    #[serde(default)]
    pub simplify: Option<f32>,
    /// Caminhos abertos mais curtos que isto (px) saem (padrão 3).
    #[serde(default)]
    pub min_length: Option<f32>,
}

/// O comando que insere os traços achados e um resumo (quantos, tempo).
pub fn vectorize_command(p: &aiai_core::Project, blobs: &aiai_core::BlobStore, req: &VectorizeRequest) -> crate::ToolResult<(aiai_core::Command, serde_json::Value)> {
    use crate::context::*;
    use crate::ToolError;
    let cut = cut_data(p, &req.cut)?;
    let target = layer_data(p, &req.layer)?;
    if target.kind != aiai_core::LayerType::Vector {
        return Err(ToolError::Invalid("a camada que recebe os traços precisa ser vetorial".into()));
    }
    if target.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let cel = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe nenhum cel no quadro {} (crie o cel e a célula do timesheet)", req.frame)))?;
    let aiai_core::NodeKind::Cel(cd) = &cel.data else { unreachable!() };
    let (w, h) = canvas_size(p, cut);
    let rgba: image::RgbaImage = match &req.source {
        VectorizeSource::Image { path, fit } => {
            let img = crate::import::fitted_image(path, w, h, *fit)?;
            to_rgba8(&img)
        }
        VectorizeSource::Layer { layer } => {
            let l = layer_data(p, layer)?;
            if l.kind != aiai_core::LayerType::Raster {
                return Err(ToolError::Invalid("a fonte precisa ser uma camada raster (ou um arquivo de imagem)".into()));
            }
            let src = exposed_cel(p, cut, layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada fonte não expõe cel no quadro {}", req.frame)))?;
            let aiai_core::NodeKind::Cel(sc) = &src.data else { unreachable!() };
            let name = p.node(layer).map(|n| n.name.clone()).unwrap_or_default();
            let (img, _) = aiai_render::compose::render_cel_image(p, blobs, sc, false, (w, h), &name);
            to_rgba8(&img)
        }
    };
    let mut o = VectorizeOptions::default();
    if let Some(t) = req.threshold {
        o.threshold = t.clamp(0.05, 0.95);
    }
    if let Some(s) = req.simplify {
        o.simplify = s.clamp(0.0, 10.0);
    }
    if let Some(m) = req.min_length {
        o.min_length = m.max(0.0);
    }
    let t0 = std::time::Instant::now();
    let paths = vectorize(w, h, &ink_from_rgba(&rgba), &o);
    let ms = t0.elapsed().as_secs_f32() * 1000.0;
    if paths.is_empty() {
        return Err(ToolError::Invalid("nenhuma linha achada na fonte (imagem vazia ou tinta abaixo do limiar)".into()));
    }
    let mut idx = cd.strokes.len();
    let mut items = Vec::with_capacity(paths.len());
    for path in &paths {
        let mut s = path_to_stroke(path, req.tip, req.color.clone(), 1.0, [0.0, 0.0]);
        if req.width_mode != WidthMode::Drawing {
            s.width = req.width.unwrap_or(s.width).max(0.1);
            for q in &mut s.points {
                q.width = 1.0;
            }
            s.taper = None; // modo caneta: a ponta da caneta decide o afinamento
        }
        if req.width_mode == WidthMode::Fixed {
            s.tip = VectorTip::Marker;
        }
        items.push(aiai_core::command::IndexedStroke { index: idx, stroke: s });
        idx += 1;
    }
    let report = serde_json::json!({ "strokes": items.len(), "closed": paths.iter().filter(|p| p.closed).count(), "ms": ms.round(), "canvas": [w, h] });
    Ok((aiai_core::Command::InsertStrokes { cel: cel.id.clone(), items }, report))
}

fn to_rgba8(img: &aiai_paint::Image) -> image::RgbaImage {
    image::RgbaImage::from_raw(img.width, img.height, img.to_rgba8()).expect("tamanho da imagem")
}
