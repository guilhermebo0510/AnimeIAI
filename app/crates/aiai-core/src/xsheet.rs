//! Timesheet (タイムシート). Quadros 0-based internamente; a UI mostra 1-based como no papel.
//!
//! Cada trilha guarda só as **mudanças**: uma entrada no quadro f vale até a próxima entrada (exposição por referência).
//! "A1 em 2s" vira entradas em 0, 2, 4…; segurar um desenho é simplesmente não ter entrada.

use crate::ids::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CellRef {
    /// Número do cel da camada da trilha (como "A3").
    Cel { number: u32 },
    /// Quadro vazio (nada daquela trilha aparece).
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    #[default]
    Drawing,
    Background,
    Book,
    Camera,
    Effect,
    Dialogue,
    Mouth,
    Eyes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub id: Id,
    /// Nome da coluna ("A", "B", "BG"…).
    pub name: String,
    #[serde(default)]
    pub kind: TrackKind,
    /// Camada cujos cels esta trilha expõe (trilhas de desenho, BG, BOOK, boca, olhos).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<Id>,
    /// Quadro → mudança de exposição.
    #[serde(default)]
    pub cells: BTreeMap<u32, CellRef>,
}

impl Track {
    pub fn new(id: Id, name: &str, kind: TrackKind, layer: Option<Id>) -> Self {
        Track { id, name: name.to_string(), kind, layer, cells: BTreeMap::new() }
    }

    /// O que está exposto no quadro `frame` (a última entrada em ou antes dele).
    pub fn exposed_at(&self, frame: u32) -> Option<CellRef> {
        self.cells.range(..=frame).next_back().map(|(_, c)| *c)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct XSheet {
    #[serde(default)]
    pub tracks: Vec<Track>,
}

impl XSheet {
    pub fn track(&self, id: &Id) -> Option<&Track> {
        self.tracks.iter().find(|t| &t.id == id)
    }

    pub fn track_mut(&mut self, id: &Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| &t.id == id)
    }

    /// Trilha que expõe a camada `layer`, se houver.
    pub fn track_for_layer(&self, layer: &Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.layer.as_ref() == Some(layer))
    }

    /// Quadros (0-based, < `duration`) em que alguma trilha muda o que expõe — o "mapa de exposição" do corte.
    pub fn change_frames(&self, duration: u32) -> Vec<u32> {
        let mut frames: Vec<u32> = self
            .tracks
            .iter()
            .flat_map(|t| {
                let mut prev: Option<CellRef> = None;
                let mut out = Vec::new();
                for (&f, &c) in t.cells.range(..duration) {
                    if prev != Some(c) {
                        out.push(f);
                    }
                    prev = Some(c);
                }
                out
            })
            .collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }
}

/// Utilitário para a notação de exposição: `cels` expostos em sequência, cada um por `hold` quadros, a partir de `start`.
/// Ex.: `[1,2,3,4]`, hold 2, start 0 → {0:A1, 2:A2, 4:A3, 6:A4}.
pub fn exposure_entries(cels: &[u32], hold: u32, start: u32) -> Vec<(u32, CellRef)> {
    let hold = hold.max(1);
    cels.iter().enumerate().map(|(i, &n)| (start + i as u32 * hold, CellRef::Cel { number: n })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_in_twos_and_holds() {
        let mut t = Track::new(Id::from("t"), "A", TrackKind::Drawing, None);
        for (f, c) in exposure_entries(&[1, 2, 3], 2, 0) {
            t.cells.insert(f, c);
        }
        assert_eq!(t.exposed_at(0), Some(CellRef::Cel { number: 1 }));
        assert_eq!(t.exposed_at(1), Some(CellRef::Cel { number: 1 }));
        assert_eq!(t.exposed_at(3), Some(CellRef::Cel { number: 2 }));
        assert_eq!(t.exposed_at(100), Some(CellRef::Cel { number: 3 }), "o último cel fica segurado");
        let xs = XSheet { tracks: vec![t] };
        assert_eq!(xs.change_frames(10), vec![0, 2, 4]);
    }
}
