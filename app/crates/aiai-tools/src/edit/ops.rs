//! Operações de montagem sobre o [`EditDoc`] (puras: sem projeto, só a duração da mídia quando precisa): navalha,
//! apagar com/sem ripple, inserir e sobrescrever, mover (com vínculo e legendas presas), aparar (com ripple), roll, slip,
//! slide, transições e vínculo. Trilhas travadas não mudam. Usadas pela linha do tempo, pelo monitor de origem e pelo
//! `aiai_edit_ops` (MCP).

use super::{EditDoc, EditItem, ItemSource, TrackKind, Transition};
use aiai_core::{Key, KeyTracks};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Ponta de um item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    In,
    Out,
}

/// Duração da mídia de uma fonte em quadros (`None` = sem limite: imagem, cor, legenda).
pub type MediaLen<'a> = &'a dyn Fn(&ItemSource) -> Option<u32>;

/// Sem limite de mídia (testes, legendas).
pub fn no_media_limit(_: &ItemSource) -> Option<u32> {
    None
}

/// Chaves deslocadas `d` quadros para trás (o valor no novo quadro 0 é preservado).
fn shift_keys(keys: &KeyTracks, d: u32) -> KeyTracks {
    if d == 0 || keys.is_empty() {
        return keys.clone();
    }
    let mut out = KeyTracks::default();
    for (prop, ks) in &keys.0 {
        let mut v: Vec<Key> = Vec::new();
        if ks.first().is_some_and(|k| k.frame < d) {
            if let Some(val) = keys.eval(prop, d as f32) {
                let ease = ks.iter().rev().find(|k| k.frame <= d).map(|k| k.ease).unwrap_or_default();
                v.push(Key { frame: 0, value: val, ease });
            }
        }
        v.extend(ks.iter().filter(|k| k.frame >= d).map(|k| Key { frame: k.frame - d, ..*k }));
        v.dedup_by_key(|k| k.frame);
        out.0.insert(prop.clone(), v);
    }
    out
}

fn media_room(media: MediaLen, it: &EditItem) -> Option<i64> {
    media(&it.source).map(|m| m as i64)
}

impl EditDoc {
    pub fn is_locked(&self, track: u32) -> bool {
        self.track(track).is_none_or(|t| t.locked)
    }

    fn track_of(&self, id: u32) -> Option<u32> {
        self.item(id).map(|i| i.track)
    }

    /// Itens selecionáveis junto com `ids` (os vinculados de cada um), sem repetir.
    pub fn with_linked(&self, ids: &[u32]) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for &id in ids {
            for l in self.linked(id) {
                if !out.contains(&l) {
                    out.push(l);
                }
            }
        }
        out
    }

    /// Desloca as legendas presas a `id` (que não estejam em `except`).
    fn shift_anchored(&mut self, id: u32, delta: i64, except: &[u32]) {
        let ids: Vec<u32> = self.items.iter().filter(|c| c.anchor == Some(id) && !except.contains(&c.id) && !self.is_locked(c.track)).map(|c| c.id).collect();
        for c in ids {
            if let Some(it) = self.item_mut(c) {
                it.start = (it.start as i64 + delta).max(0) as u32;
            }
        }
    }

    fn remove_raw(&mut self, ids: &[u32]) {
        self.items.retain(|i| !ids.contains(&i.id));
        for it in &mut self.items {
            if it.anchor.is_some_and(|a| ids.contains(&a)) {
                it.anchor = None;
            }
        }
    }

    /// Divide o item no quadro `f` (dentro dele). A parte direita ganha id novo (devolvido) e herda a transição de saída;
    /// as legendas presas que começam depois de `f` passam a seguir a parte direita.
    pub fn split_item(&mut self, id: u32, f: u32) -> Option<u32> {
        let it = self.item(id)?.clone();
        if f <= it.start || f >= it.end() {
            return None;
        }
        let d = f - it.start;
        let new_id = self.next_item_id();
        let mut right = it.clone();
        right.id = new_id;
        right.start = f;
        right.dur = it.end() - f;
        right.src_in = it.src_in + (d as f64 * it.speed as f64).floor() as u32;
        right.transition_in = None;
        right.keys = shift_keys(&it.keys, d);
        for fl in &mut right.filters {
            fl.keys = shift_keys(&fl.keys, d);
        }
        {
            let left = self.item_mut(id)?;
            left.dur = d;
            left.transition_out = None;
        }
        for c in &mut self.items {
            if c.anchor == Some(id) && c.start >= f {
                c.anchor = Some(new_id);
            }
        }
        self.items.push(right);
        Some(new_id)
    }

    /// Navalha no quadro `f`: divide os itens dados (com os vinculados) ou, com `None`, tudo que atravessa `f` nas trilhas
    /// destravadas. As partes direitas de um grupo vinculado continuam vinculadas entre si. Devolve os ids novos.
    pub fn razor(&mut self, f: u32, ids: Option<&[u32]>) -> Vec<u32> {
        let targets: Vec<u32> = match ids {
            Some(ids) => self.with_linked(ids),
            None => self.items.iter().map(|i| i.id).collect(),
        };
        // navalha dirigida a itens: se o clicado (ou um vinculado a ele que atravessa o quadro) está numa trilha travada,
        // nada é cortado — cortar só a parte livre do par tiraria o áudio de sincronia em silêncio (auditoria M-03)
        if ids.is_some() && targets.iter().any(|id| self.item(*id).is_some_and(|i| f > i.start && f < i.end() && self.is_locked(i.track))) {
            return Vec::new();
        }
        let targets: Vec<u32> = targets.into_iter().filter(|id| self.item(*id).is_some_and(|i| f > i.start && f < i.end() && !self.is_locked(i.track))).collect();
        let mut relink: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut made = Vec::new();
        for id in targets {
            let link = self.item(id).and_then(|i| i.link);
            if let Some(n) = self.split_item(id, f) {
                if let Some(l) = link {
                    let next = self.next_link_id();
                    let nl = *relink.entry(l).or_insert(next);
                    if let Some(r) = self.item_mut(n) {
                        r.link = Some(nl);
                    }
                }
                made.push(n);
            }
        }
        made
    }

    /// Limpa [s, e) na trilha (como sobrescrever): apaga o que cai inteiro, apara ou divide o que atravessa. `keep` fica.
    pub fn clear_range(&mut self, track: u32, s: u32, e: u32, keep: &[u32]) {
        if e <= s {
            return;
        }
        let hit: Vec<u32> = self.items.iter().filter(|i| i.track == track && !keep.contains(&i.id) && i.start < e && s < i.end()).map(|i| i.id).collect();
        let mut gone = Vec::new();
        for id in hit {
            let it = self.item(id).expect("existe").clone();
            if it.start >= s && it.end() <= e {
                gone.push(id);
            } else if it.start < s && it.end() > e {
                if let Some(r) = self.split_item(id, e) {
                    if let Some(l) = self.item_mut(id) {
                        l.dur = s - l.start;
                    }
                    let _ = r;
                }
            } else if it.start < s {
                if let Some(l) = self.item_mut(id) {
                    l.dur = s - l.start;
                    l.transition_out = None;
                }
            } else {
                let d = e - it.start;
                if let Some(r) = self.item_mut(id) {
                    r.start = e;
                    r.dur -= d;
                    r.src_in += (d as f64 * r.speed as f64).floor() as u32;
                    r.transition_in = None;
                    r.keys = shift_keys(&r.keys, d);
                }
            }
        }
        self.remove_raw(&gone);
    }

    /// O que anda num ripple em `at`: tudo que começa em `at` ou depois nas trilhas destravadas (fora `exclude`) e os
    /// vinculados a esses (o som de um clipe que começa antes, por causa de uma sobreposição, anda junto com a imagem).
    fn ripple_set(&self, at: u32, exclude: &[u32]) -> Vec<u32> {
        let free = |i: &EditItem| !self.is_locked(i.track) && !exclude.contains(&i.id);
        let mut set: Vec<u32> = self.items.iter().filter(|i| i.start >= at && free(i)).map(|i| i.id).collect();
        let groups: Vec<u32> = set.iter().filter_map(|id| self.item(*id).and_then(|i| i.link)).collect();
        for it in &self.items {
            if it.link.is_some_and(|l| groups.contains(&l)) && free(it) && !set.contains(&it.id) {
                set.push(it.id);
            }
        }
        set
    }

    /// Espaço livre (quadros) para puxar `set` para a esquerda sem bater no que fica parado, em todas as trilhas.
    fn ripple_room(&self, set: &[u32]) -> u32 {
        let mut room = u32::MAX;
        for t in &self.tracks {
            let moving: Vec<&EditItem> = self.items.iter().filter(|i| i.track == t.id && set.contains(&i.id)).collect();
            let Some(first) = moving.iter().map(|i| i.start).min() else { continue };
            let before = self.items.iter().filter(|i| i.track == t.id && !set.contains(&i.id) && i.start < first).map(|i| i.end()).max().unwrap_or(0);
            room = room.min(first.saturating_sub(before));
        }
        room
    }

    /// Desloca tudo que começa em `at` ou depois (trilhas destravadas, fora `exclude`, com os vinculados) por `delta`
    /// quadros. Puxar para a esquerda para no que está parado antes (devolve o deslocamento feito).
    pub fn ripple_shift(&mut self, at: u32, delta: i64, exclude: &[u32]) -> i64 {
        let set = self.ripple_set(at, exclude);
        let delta = if delta < 0 { -(delta.unsigned_abs().min(self.ripple_room(&set) as u64) as i64) } else { delta };
        if delta == 0 {
            return 0;
        }
        for it in &mut self.items {
            if set.contains(&it.id) {
                it.start = (it.start as i64 + delta).max(0) as u32;
            }
        }
        delta
    }

    /// Apaga itens; com `ripple`, fecha o buraco em todas as trilhas destravadas (o quanto couber sem sobrepor).
    pub fn delete_items(&mut self, ids: &[u32], ripple: bool) {
        let ids: Vec<u32> = ids.iter().copied().filter(|id| self.item(*id).is_some_and(|i| !self.is_locked(i.track))).collect();
        if ids.is_empty() {
            return;
        }
        let s = ids.iter().filter_map(|id| self.item(*id)).map(|i| i.start).min().unwrap_or(0);
        let e = ids.iter().filter_map(|id| self.item(*id)).map(|i| i.end()).max().unwrap_or(0);
        self.remove_raw(&ids);
        if ripple {
            self.ripple_shift(e, -((e - s) as i64), &[]);
        }
    }

    /// Inserir: abre espaço em `at` nas trilhas destravadas (divide o que atravessa `at`) e põe os itens (com `start`
    /// já na edição). Devolve os ids dados aos itens.
    pub fn insert_items(&mut self, items: Vec<EditItem>, at: u32) -> Vec<u32> {
        let len = items.iter().map(|i| i.end()).max().unwrap_or(at).saturating_sub(at);
        if len == 0 {
            return Vec::new();
        }
        self.razor(at, None);
        self.ripple_shift(at, len as i64, &[]);
        self.add_items(items)
    }

    /// Sobrescrever: limpa o trecho de cada item na sua trilha e põe os itens.
    pub fn overwrite_items(&mut self, items: Vec<EditItem>) -> Vec<u32> {
        for it in &items {
            self.clear_range(it.track, it.start, it.end(), &[]);
        }
        self.add_items(items)
    }

    /// Acrescenta itens com ids novos (vínculos internos renumerados para um grupo novo por grupo dado).
    fn add_items(&mut self, items: Vec<EditItem>) -> Vec<u32> {
        let mut relink: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut ids = Vec::new();
        for mut it in items {
            it.id = self.next_item_id();
            if let Some(l) = it.link {
                let next = self.next_link_id();
                it.link = Some(*relink.entry(l).or_insert(next));
            }
            ids.push(it.id);
            self.items.push(it);
        }
        ids
    }

    /// Itens de uma fonte para pôr na edição: corte = imagem em `vtrack` + som vinculado em `atrack` (os que vierem).
    pub fn source_items(src: &ItemSource, src_in: u32, dur: u32, at: u32, vtrack: Option<u32>, atrack: Option<u32>) -> Vec<EditItem> {
        let mut v = Vec::new();
        match src {
            ItemSource::Cut { cut } => {
                if let Some(t) = vtrack {
                    let mut i = EditItem::new(0, t, at, dur, src.clone());
                    i.src_in = src_in;
                    i.link = Some(1);
                    v.push(i);
                }
                if let Some(t) = atrack {
                    let mut i = EditItem::new(0, t, at, dur, ItemSource::CutAudio { cut: cut.clone(), track: None });
                    i.src_in = src_in;
                    i.link = Some(1);
                    v.push(i);
                }
                if v.len() == 1 {
                    v[0].link = None;
                }
            }
            s => {
                let t = match s.track_kind() {
                    TrackKind::Video => vtrack,
                    TrackKind::Audio => atrack,
                    TrackKind::Subtitle => None,
                };
                if let Some(t) = t {
                    let mut i = EditItem::new(0, t, at, dur, s.clone());
                    i.src_in = src_in;
                    v.push(i);
                }
            }
        }
        v
    }

    /// Move itens `delta` quadros; os do tipo `kind` também andam `track_shift` trilhas (do mesmo tipo). No destino o
    /// que estiver por baixo é sobrescrito; legendas presas acompanham.
    pub fn move_items(&mut self, ids: &[u32], kind: TrackKind, delta: i64, track_shift: i32) -> Result<(), String> {
        let ids: Vec<u32> = ids.iter().copied().filter(|id| self.item(*id).is_some()).collect();
        if ids.is_empty() || (delta == 0 && track_shift == 0) {
            return Ok(());
        }
        if ids.iter().any(|id| self.is_locked(self.track_of(*id).unwrap_or(0))) {
            return Err("item em trilha travada".into());
        }
        let min_start = ids.iter().filter_map(|id| self.item(*id)).map(|i| i.start).min().unwrap_or(0) as i64;
        let delta = delta.max(-min_start);
        let order: Vec<u32> = self.tracks_of(kind).iter().map(|t| t.id).collect();
        let mut moved: Vec<EditItem> = Vec::new();
        for id in &ids {
            let mut it = self.item(*id).expect("existe").clone();
            if it.source.track_kind() == kind && track_shift != 0 {
                let pos = order.iter().position(|t| *t == it.track).ok_or("trilha do item sumiu")? as i32 + track_shift;
                let Some(&t) = order.get(pos.max(0) as usize).filter(|_| pos >= 0) else { return Err("não há trilha nessa direção".into()) };
                if self.is_locked(t) {
                    return Err("trilha de destino travada".into());
                }
                it.track = t;
            }
            it.start = (it.start as i64 + delta) as u32;
            moved.push(it);
        }
        // legendas presas que não estão no grupo
        let anchored: Vec<u32> = self.items.iter().filter(|c| c.anchor.is_some_and(|a| ids.contains(&a)) && !ids.contains(&c.id) && !self.is_locked(c.track)).map(|c| c.id).collect();
        for c in anchored {
            let mut it = self.item(c).expect("existe").clone();
            it.start = (it.start as i64 + delta).max(0) as u32;
            moved.push(it);
        }
        let all: Vec<u32> = moved.iter().map(|i| i.id).collect();
        self.items.retain(|i| !all.contains(&i.id));
        for it in &moved {
            self.clear_range(it.track, it.start, it.end(), &all);
        }
        self.items.extend(moved);
        Ok(())
    }

    /// Apara a entrada ou a saída dos itens `ids` em `delta` quadros (limitado pela mídia, por 1 quadro mínimo e pelos
    /// vizinhos). Com `ripple`, o resto da edição anda junto e nada se sobrepõe (ferramenta Ripple, B). Devolve o delta.
    pub fn trim(&mut self, ids: &[u32], edge: Edge, delta: i64, ripple: bool, media: MediaLen) -> i64 {
        let ids: Vec<u32> = ids.iter().copied().filter(|id| self.item(*id).is_some_and(|i| !self.is_locked(i.track))).collect();
        let mut d = delta;
        for id in &ids {
            let it = self.item(*id).expect("existe");
            let sp = it.speed as f64;
            match edge {
                Edge::In => {
                    // entrada: não antes do início da mídia nem do vizinho anterior (sem ripple), item com pelo menos 1 quadro
                    let lo_media = -((it.src_in as f64 / sp).floor() as i64);
                    let prev_end = self.items.iter().filter(|o| o.track == it.track && o.id != it.id && o.end() <= it.start).map(|o| o.end()).max().unwrap_or(0);
                    let lo = if ripple { lo_media } else { lo_media.max(prev_end as i64 - it.start as i64) };
                    d = d.clamp(lo.min(0), it.dur as i64 - 1);
                }
                Edge::Out => {
                    let mut hi = match media_room(media, it) {
                        Some(m) => ((m as f64 - it.src_in as f64) / sp).floor() as i64 - it.dur as i64,
                        None => i64::MAX / 4,
                    };
                    if !ripple {
                        if let Some(n) = self.items.iter().filter(|o| o.track == it.track && o.id != it.id && o.start >= it.end()).map(|o| o.start).min() {
                            hi = hi.min(n as i64 - it.end() as i64);
                        }
                    }
                    d = d.clamp(-(it.dur as i64 - 1), hi.max(0));
                }
            }
        }
        if d == 0 || ids.is_empty() {
            return 0;
        }
        let old_end = ids.iter().filter_map(|id| self.item(*id)).map(|i| i.end()).max().unwrap_or(0);
        if ripple {
            // o que vem depois anda para não haver buraco nem sobreposição
            let shift = match edge {
                Edge::In => -d,
                Edge::Out => d,
            };
            for id in &ids {
                let it = self.item_mut(*id).expect("existe");
                let sp = it.speed as f64;
                match edge {
                    Edge::In => {
                        it.src_in = (it.src_in as i64 + (d as f64 * sp).round() as i64).max(0) as u32;
                        it.dur = (it.dur as i64 - d) as u32;
                        it.keys = shift_keys(&it.keys, d.max(0) as u32);
                    }
                    Edge::Out => it.dur = (it.dur as i64 + d) as u32,
                }
            }
            self.ripple_shift(old_end, shift, &ids);
            return d;
        }
        for id in &ids {
            let it = self.item_mut(*id).expect("existe");
            let sp = it.speed as f64;
            match edge {
                Edge::In => {
                    it.start = (it.start as i64 + d) as u32;
                    it.src_in = (it.src_in as i64 + (d as f64 * sp).round() as i64).max(0) as u32;
                    it.dur = (it.dur as i64 - d) as u32;
                    it.keys = shift_keys(&it.keys, d.max(0) as u32);
                }
                Edge::Out => it.dur = (it.dur as i64 + d) as u32,
            }
        }
        d
    }

    /// Roll (N): move o ponto de corte entre `left` e o vizinho seguinte (e os vinculados com o mesmo ponto) `delta`
    /// quadros sem mudar a duração total. Devolve o delta aplicado.
    pub fn roll(&mut self, left: u32, delta: i64, media: MediaLen) -> i64 {
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for l in self.linked(left) {
            if let (Some(a), Some(b)) = (self.item(l), self.neighbor_after(l)) {
                if !self.is_locked(a.track) {
                    pairs.push((a.id, b.id));
                }
            }
        }
        let mut d = delta;
        for (a, b) in &pairs {
            let (a, b) = (self.item(*a).expect("existe"), self.item(*b).expect("existe"));
            let hi_a = match media_room(media, a) {
                Some(m) => ((m as f64 - a.src_in as f64) / a.speed as f64).floor() as i64 - a.dur as i64,
                None => i64::MAX / 4,
            };
            let lo_b = -((b.src_in as f64 / b.speed as f64).floor() as i64);
            d = d.clamp((-(a.dur as i64 - 1)).max(lo_b), hi_a.min(b.dur as i64 - 1));
        }
        if d == 0 {
            return 0;
        }
        for (a, b) in pairs {
            if let Some(x) = self.item_mut(a) {
                x.dur = (x.dur as i64 + d) as u32;
            }
            if let Some(x) = self.item_mut(b) {
                x.start = (x.start as i64 + d) as u32;
                x.dur = (x.dur as i64 - d) as u32;
                x.src_in = (x.src_in as i64 + (d as f64 * x.speed as f64).round() as i64).max(0) as u32;
                x.keys = shift_keys(&x.keys, d.max(0) as u32);
            }
        }
        d
    }

    /// Slip (Y): muda o trecho da mídia sem mover o item (entrada e saída andam juntas). Devolve o delta (quadros da mídia).
    pub fn slip(&mut self, ids: &[u32], delta: i64, media: MediaLen) -> i64 {
        let ids: Vec<u32> = ids.iter().copied().filter(|id| self.item(*id).is_some_and(|i| !self.is_locked(i.track) && i.caption_text().is_none())).collect();
        let mut d = delta;
        for id in &ids {
            let it = self.item(*id).expect("existe");
            let hi = match media_room(media, it) {
                Some(m) => m - it.src_out() as i64,
                None => i64::MAX / 4,
            };
            d = d.clamp(-(it.src_in as i64), hi.max(0));
        }
        for id in &ids {
            let it = self.item_mut(*id).expect("existe");
            it.src_in = (it.src_in as i64 + d) as u32;
        }
        if ids.is_empty() { 0 } else { d }
    }

    /// Slide (U): move o item entre os vizinhos; o anterior estica/encolhe a saída e o seguinte a entrada, a duração total
    /// fica igual. Sem vizinho de um lado, precisa de espaço livre ali. Vale para o item e os vinculados. Devolve o delta.
    pub fn slide(&mut self, id: u32, delta: i64, media: MediaLen) -> i64 {
        let group: Vec<u32> = self.linked(id).into_iter().filter(|i| self.item(*i).is_some_and(|x| !self.is_locked(x.track))).collect();
        let mut d = delta;
        let mut plan: Vec<(u32, Option<u32>, Option<u32>)> = Vec::new();
        for g in &group {
            let it = self.item(*g).expect("existe");
            let prev = self.neighbor_before(*g).map(|p| p.id);
            let next = self.neighbor_after(*g).map(|n| n.id);
            match prev.and_then(|p| self.item(p)) {
                Some(p) => {
                    let hi_p = match media_room(media, p) {
                        Some(m) => ((m as f64 - p.src_in as f64) / p.speed as f64).floor() as i64 - p.dur as i64,
                        None => i64::MAX / 4,
                    };
                    d = d.clamp(-(p.dur as i64 - 1), hi_p.max(0));
                }
                None => {
                    let free = it.start as i64 - self.items.iter().filter(|o| o.track == it.track && o.end() <= it.start).map(|o| o.end()).max().unwrap_or(0) as i64;
                    d = d.max(-free);
                }
            }
            match next.and_then(|n| self.item(n)) {
                Some(n) => {
                    let lo_n = -((n.src_in as f64 / n.speed as f64).floor() as i64);
                    d = d.clamp(lo_n.min(0), n.dur as i64 - 1);
                }
                None => {
                    let free = self.items.iter().filter(|o| o.track == it.track && o.start >= it.end() && o.id != it.id).map(|o| o.start).min().map(|s| s as i64 - it.end() as i64).unwrap_or(i64::MAX / 4);
                    d = d.min(free);
                }
            }
            plan.push((*g, prev, next));
        }
        if d == 0 {
            return 0;
        }
        for (g, prev, next) in plan {
            if let Some(p) = prev.and_then(|p| self.item_mut(p)) {
                p.dur = (p.dur as i64 + d) as u32;
            }
            if let Some(n) = next.and_then(|n| self.item_mut(n)) {
                n.start = (n.start as i64 + d) as u32;
                n.dur = (n.dur as i64 - d) as u32;
                n.src_in = (n.src_in as i64 + (d as f64 * n.speed as f64).round() as i64).max(0) as u32;
            }
            if let Some(it) = self.item_mut(g) {
                it.start = (it.start as i64 + d) as u32;
            }
            self.shift_anchored(g, d, &[]);
        }
        d
    }

    /// Põe (ou tira, com `None`) a transição na entrada/saída do item.
    pub fn set_transition(&mut self, id: u32, edge: Edge, tr: Option<Transition>) -> bool {
        let locked = self.item(id).is_none_or(|i| self.is_locked(i.track));
        let Some(it) = self.item_mut(id).filter(|_| !locked) else { return false };
        let tr = tr.filter(|t| t.kind != super::TransitionKind::Cut && t.frames > 0);
        match edge {
            Edge::In => it.transition_in = tr,
            Edge::Out => it.transition_out = tr,
        }
        true
    }

    /// Ponto de corte mais perto de `f` numa trilha: (item que sai, item que entra) com o quadro do corte.
    pub fn edit_point_near(&self, track: u32, f: u32, tolerance: u32) -> Option<(Option<u32>, Option<u32>, u32)> {
        let items = self.items_on(track);
        let mut best: Option<(Option<u32>, Option<u32>, u32, u32)> = None;
        for (k, it) in items.iter().enumerate() {
            for (at, a, b) in [(it.start, k.checked_sub(1).map(|j| items[j]).filter(|p| p.end() == it.start).map(|p| p.id), Some(it.id)), (it.end(), Some(it.id), items.get(k + 1).filter(|n| n.start == it.end()).map(|n| n.id))] {
                let dist = at.abs_diff(f);
                if dist <= tolerance && best.as_ref().is_none_or(|b| dist < b.3) {
                    best = Some((a, b, at, dist));
                }
            }
        }
        best.map(|(a, b, at, _)| (a, b, at))
    }

    /// Desvincula os itens (cada um fica solto).
    pub fn unlink(&mut self, ids: &[u32]) {
        let groups: Vec<u32> = ids.iter().filter_map(|id| self.item(*id).and_then(|i| i.link)).collect();
        for it in &mut self.items {
            if it.link.is_some_and(|l| groups.contains(&l)) {
                it.link = None;
            }
        }
    }

    /// Vincula os itens num grupo novo.
    pub fn link(&mut self, ids: &[u32]) {
        if ids.len() < 2 {
            return;
        }
        let l = self.next_link_id();
        for id in ids {
            if let Some(it) = self.item_mut(*id) {
                it.link = Some(l);
            }
        }
    }
}

/// Uma operação de montagem (MCP `aiai_edit_ops`; a linha do tempo usa as mesmas funções).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    /// Navalha em `frame`: só os `items` dados (com vinculados) ou, omitidos, tudo nas trilhas destravadas.
    Razor {
        frame: u32,
        #[serde(default)]
        items: Option<Vec<u32>>,
    },
    /// Apagar itens; `ripple` fecha o buraco.
    Delete {
        items: Vec<u32>,
        #[serde(default)]
        ripple: bool,
        /// Inclui os vinculados (padrão true).
        #[serde(default = "yes")]
        linked: bool,
    },
    /// Pôr uma fonte em `at` (quadro da edição): `mode` insert (empurra o resto) ou overwrite. Corte = imagem em
    /// `video_track` + som vinculado em `audio_track` (padrão: V1 e A1; null = sem).
    Place {
        source: ItemSource,
        at: u32,
        #[serde(default)]
        src_in: u32,
        dur: u32,
        #[serde(default)]
        mode: PlaceMode,
        #[serde(default)]
        video_track: Option<u32>,
        #[serde(default)]
        audio_track: Option<u32>,
    },
    /// Mover itens (com vinculados) `delta` quadros e `track_shift` trilhas (do mesmo tipo do primeiro item).
    Move {
        items: Vec<u32>,
        #[serde(default)]
        delta: i64,
        #[serde(default)]
        track_shift: i32,
    },
    /// Aparar a entrada/saída; `ripple` arrasta o resto.
    Trim {
        items: Vec<u32>,
        edge: Edge,
        delta: i64,
        #[serde(default)]
        ripple: bool,
    },
    /// Roll no ponto de corte depois de `item`.
    Roll { item: u32, delta: i64 },
    /// Slip (trecho da mídia).
    Slip { items: Vec<u32>, delta: i64 },
    /// Slide entre os vizinhos.
    Slide { item: u32, delta: i64 },
    /// Transição na entrada (`in`) ou saída (`out`) do item; `transition` null tira.
    Transition {
        item: u32,
        #[serde(default = "edge_in")]
        edge: Edge,
        transition: Option<Transition>,
    },
    Link { items: Vec<u32> },
    Unlink { items: Vec<u32> },
    /// Muda campos do item (speed, opacity, volume, pan, filters, audio_fx, keys, enabled, anchor, source de legenda).
    SetItem { item: u32, patch: serde_json::Value },
    /// Acrescenta uma trilha (`kind`); devolve o id em `created_tracks`.
    AddTrack {
        kind: TrackKind,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        lang: Option<String>,
    },
    /// Muda campos da trilha (name, locked, hidden, muted, solo, volume, lang, burn, style).
    SetTrack { track: u32, patch: serde_json::Value },
    /// Remove uma trilha vazia.
    RemoveTrack { track: u32 },
}

fn yes() -> bool {
    true
}
fn edge_in() -> Edge {
    Edge::In
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaceMode {
    Insert,
    #[default]
    Overwrite,
}

/// Mescla `patch` nos campos de um valor serializável e lê de volta (campos desconhecidos são erro).
fn patch_value<T: Serialize + serde::de::DeserializeOwned>(cur: &T, patch: &serde_json::Value, allowed: &[&str]) -> Result<T, String> {
    let mut v = serde_json::to_value(cur).map_err(|e| e.to_string())?;
    let (Some(o), Some(p)) = (v.as_object_mut(), patch.as_object()) else { return Err("patch precisa ser objeto".into()) };
    for (k, val) in p {
        if !allowed.contains(&k.as_str()) {
            return Err(format!("campo '{k}' desconhecido ou fixo (aceitos: {})", allowed.join(", ")));
        }
        o.insert(k.clone(), val.clone());
    }
    serde_json::from_value(v).map_err(|e| format!("patch inválido: {e}"))
}

impl EditDoc {
    /// Aplica uma operação. Devolve ids criados (itens ou trilhas) quando houver.
    pub fn apply_op(&mut self, op: &EditOp, media: MediaLen) -> Result<Vec<u32>, String> {
        let exists = |d: &EditDoc, ids: &[u32]| -> Result<(), String> {
            match ids.iter().find(|id| d.item(**id).is_none()) {
                Some(id) => Err(format!("item {id} não existe")),
                None => Ok(()),
            }
        };
        Ok(match op {
            EditOp::Razor { frame, items } => {
                if let Some(ids) = items {
                    exists(self, ids)?;
                }
                self.razor(*frame, items.as_deref())
            }
            EditOp::Delete { items, ripple, linked } => {
                exists(self, items)?;
                let ids = if *linked { self.with_linked(items) } else { items.clone() };
                self.delete_items(&ids, *ripple);
                Vec::new()
            }
            EditOp::Place { source, at, src_in, dur, mode, video_track, audio_track } => {
                if *dur == 0 {
                    return Err("dur precisa ser > 0".into());
                }
                let first = |k: TrackKind| self.tracks_of(k).first().map(|t| t.id);
                let vt = video_track.or_else(|| first(TrackKind::Video));
                let at_ = audio_track.or_else(|| first(TrackKind::Audio));
                for t in [vt, at_].into_iter().flatten() {
                    if self.track(t).is_none() {
                        return Err(format!("trilha {t} não existe"));
                    }
                }
                let items = match source {
                    ItemSource::Caption { .. } => {
                        let st = self.tracks_of(TrackKind::Subtitle).first().map(|t| t.id).ok_or("sem trilha de legenda")?;
                        vec![EditItem::new(0, st, *at, *dur, source.clone())]
                    }
                    s => EditDoc::source_items(s, *src_in, *dur, *at, vt, at_),
                };
                if items.is_empty() {
                    return Err("nenhuma trilha de destino para essa fonte".into());
                }
                if items.iter().any(|i| self.is_locked(i.track)) {
                    return Err("trilha de destino travada".into());
                }
                match mode {
                    PlaceMode::Insert => self.insert_items(items, *at),
                    PlaceMode::Overwrite => self.overwrite_items(items),
                }
            }
            EditOp::Move { items, delta, track_shift } => {
                exists(self, items)?;
                let kind = self.item(items[0]).map(|i| i.source.track_kind()).unwrap_or_default();
                let ids = self.with_linked(items);
                self.move_items(&ids, kind, *delta, *track_shift)?;
                Vec::new()
            }
            EditOp::Trim { items, edge, delta, ripple } => {
                exists(self, items)?;
                let ids = self.with_linked(items);
                self.trim(&ids, *edge, *delta, *ripple, media);
                Vec::new()
            }
            EditOp::Roll { item, delta } => {
                exists(self, &[*item])?;
                if self.neighbor_after(*item).is_none() {
                    return Err(format!("item {item} não tem vizinho colado depois (roll precisa de um ponto de corte)"));
                }
                self.roll(*item, *delta, media);
                Vec::new()
            }
            EditOp::Slip { items, delta } => {
                exists(self, items)?;
                let ids = self.with_linked(items);
                self.slip(&ids, *delta, media);
                Vec::new()
            }
            EditOp::Slide { item, delta } => {
                exists(self, &[*item])?;
                self.slide(*item, *delta, media);
                Vec::new()
            }
            EditOp::Transition { item, edge, transition } => {
                exists(self, &[*item])?;
                if !self.set_transition(*item, *edge, transition.clone()) {
                    return Err(format!("item {item} em trilha travada"));
                }
                Vec::new()
            }
            EditOp::Link { items } => {
                exists(self, items)?;
                self.link(items);
                Vec::new()
            }
            EditOp::Unlink { items } => {
                exists(self, items)?;
                self.unlink(items);
                Vec::new()
            }
            EditOp::SetItem { item, patch } => {
                exists(self, &[*item])?;
                let cur = self.item(*item).expect("existe").clone();
                let mut next: EditItem = patch_value(&cur, patch, &["track", "start", "dur", "src_in", "source", "speed", "opacity", "volume", "pan", "filters", "audio_fx", "keys", "link", "anchor", "transition_in", "transition_out", "enabled"])?;
                next.id = cur.id;
                if next.source.track_kind() != cur.source.track_kind() {
                    return Err("a fonte nova precisa ser do mesmo tipo de trilha".into());
                }
                *self.item_mut(*item).expect("existe") = next;
                Vec::new()
            }
            EditOp::AddTrack { kind, name, lang } => {
                let id = self.add_track(*kind);
                let t = self.track_mut(id).expect("criada");
                t.name = name.clone().unwrap_or_default();
                t.lang = lang.clone().unwrap_or_default();
                vec![id]
            }
            EditOp::SetTrack { track, patch } => {
                let cur = self.track(*track).ok_or(format!("trilha {track} não existe"))?.clone();
                let next: super::EditTrack = patch_value(&cur, patch, &["name", "locked", "hidden", "muted", "solo", "volume", "lang", "burn", "style"])?;
                *self.track_mut(*track).expect("existe") = next;
                Vec::new()
            }
            EditOp::RemoveTrack { track } => {
                if self.items.iter().any(|i| i.track == *track) {
                    return Err(format!("trilha {track} não está vazia"));
                }
                let kind = self.track(*track).ok_or(format!("trilha {track} não existe"))?.kind;
                if self.tracks_of(kind).len() <= 1 {
                    return Err("precisa ficar pelo menos uma trilha de cada tipo".into());
                }
                self.tracks.retain(|t| t.id != *track);
                Vec::new()
            }
        })
    }
}
