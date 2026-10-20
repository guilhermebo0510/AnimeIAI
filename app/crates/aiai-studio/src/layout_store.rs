//! Layouts do dock guardados fora do projeto (preferência do usuário): `%APPDATA%\AnimeIAI\layouts.json`, ao lado de
//! `prefs.json`. Por workspace: a árvore em uso quando difere da fábrica, a revisão de fábrica que o usuário viu e os
//! layouts com nome. Com `--no-session` nada é lido nem gravado; com `--session-file x.json` o arquivo é
//! `x-layouts.json` ao lado dele; `--layouts-file y.json` escolhe o arquivo à força (vale mesmo com `--no-session`: é
//! o que os testes visuais usam para ver um layout voltar depois de reabrir o app sem tocar no do usuário).
//! As janelas soltas (`dock_float::SavedFloat`) entram no layout do workspace e nos layouts com nome.

use crate::dock_float::SavedFloat;
use crate::layout::{self, LayoutVariant, SavedNode};
use crate::workspace::WorkspaceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

/// Um layout com nome, por workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedLayout {
    pub name: String,
    pub tree: SavedNode,
    /// Janelas soltas do layout (Janela > Destacar painel).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floating: Vec<SavedFloat>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WsLayouts {
    /// Revisão de fábrica (`layout::factory_rev`) vigente quando o usuário personalizou ou decidiu sobre o aviso.
    pub factory_rev_seen: u32,
    /// Árvore em uso quando difere da fábrica (`None` = fábrica).
    pub current: Option<SavedNode>,
    /// Layout com nome aplicado por último (o status mostra o nome enquanto a árvore for igual a ele).
    pub active_name: Option<String>,
    pub named: Vec<NamedLayout>,
    /// Janelas soltas em uso (posição, tamanho e monitor de cada uma; ver `dock_float`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub floating: Vec<SavedFloat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutStore {
    pub version: u32,
    /// Chave do workspace (`WorkspaceId::key`, com o sufixo da variante: `animacao.douga`) → layouts dele.
    pub workspaces: BTreeMap<String, WsLayouts>,
    /// Variante em uso por workspace (vem da fase do corte; não é gravada).
    #[serde(skip)]
    variants: BTreeMap<String, LayoutVariant>,
}

impl Default for LayoutStore {
    fn default() -> Self {
        LayoutStore { version: VERSION, workspaces: BTreeMap::new(), variants: BTreeMap::new() }
    }
}

impl LayoutStore {
    /// Variante em uso no workspace (a base, se nunca foi trocada).
    pub fn variant(&self, ws: WorkspaceId) -> LayoutVariant {
        self.variants.get(ws.key()).copied().unwrap_or_default()
    }

    /// Troca a variante em uso: dali em diante `get`/`entry`/fábrica falam da variante nova.
    pub fn set_variant(&mut self, ws: WorkspaceId, v: LayoutVariant) {
        self.variants.insert(ws.key().to_owned(), v);
    }

    /// Chave do layout em uso do workspace (com o sufixo da variante: `animacao.douga`).
    pub fn slot(&self, ws: WorkspaceId) -> String {
        format!("{}{}", ws.key(), self.variant(ws).suffix())
    }

    /// Fábrica da variante em uso.
    pub fn factory(&self, ws: WorkspaceId) -> SavedNode {
        layout::factory_saved_v(ws, self.variant(ws))
    }

    pub fn get(&self, ws: WorkspaceId) -> Option<&WsLayouts> {
        self.workspaces.get(&self.slot(ws))
    }

    pub fn entry(&mut self, ws: WorkspaceId) -> &mut WsLayouts {
        let k = self.slot(ws);
        self.workspaces.entry(k).or_default()
    }

    /// O usuário tem layout próprio feito sobre uma fábrica mais velha que a atual: mostrar o aviso (nunca trocar sozinho).
    pub fn factory_update_pending(&self, ws: WorkspaceId) -> bool {
        self.get(ws).is_some_and(|e| e.current.is_some() && e.factory_rev_seen < layout::factory_rev(ws))
    }

    /// Guarda a árvore em uso: igual à fábrica = `None`. Com o aviso de fábrica nova pendente a revisão vista não muda
    /// (o aviso continua até o usuário escolher). Devolve se algo mudou.
    pub fn record_current(&mut self, ws: WorkspaceId, saved: Option<SavedNode>) -> bool {
        let pending = self.factory_update_pending(ws);
        let factory = self.factory(ws);
        let custom = saved.filter(|s| !layout::same_layout(s, &factory));
        let e = self.entry(ws);
        let changed = e.current != custom;
        if changed {
            e.current = custom;
        }
        if !pending && e.factory_rev_seen != layout::factory_rev(ws) && e.current.is_some() {
            e.factory_rev_seen = layout::factory_rev(ws);
            return true;
        }
        changed
    }

    /// Volta à fábrica (os layouts com nome ficam).
    pub fn reset(&mut self, ws: WorkspaceId) {
        let e = self.entry(ws);
        e.current = None;
        e.floating.clear();
        e.active_name = None;
        e.factory_rev_seen = layout::factory_rev(ws);
    }

    /// Salva com nome (nome repetido no mesmo workspace é substituído). Devolve se substituiu.
    pub fn save_named(&mut self, ws: WorkspaceId, name: &str, tree: SavedNode) -> bool {
        let e = self.entry(ws);
        e.active_name = Some(name.to_owned());
        if let Some(n) = e.named.iter_mut().find(|n| n.name == name) {
            n.tree = tree;
            return true;
        }
        e.named.push(NamedLayout { name: name.to_owned(), tree, floating: Vec::new() });
        false
    }

    pub fn named(&self, ws: WorkspaceId, name: &str) -> Option<&SavedNode> {
        self.get(ws)?.named.iter().find(|n| n.name == name).map(|n| &n.tree)
    }

    pub fn delete_named(&mut self, ws: WorkspaceId, name: &str) -> bool {
        let e = self.entry(ws);
        let before = e.named.len();
        e.named.retain(|n| n.name != name);
        if e.active_name.as_deref() == Some(name) {
            e.active_name = None;
        }
        e.named.len() != before
    }

    /// Rótulo do layout em uso: `None` = fábrica; `Some(nome)` quando a árvore é a de um layout com nome; `Some("")` =
    /// personalizado sem nome.
    pub fn label(&self, ws: WorkspaceId) -> Option<String> {
        let e = self.get(ws)?;
        let cur = e.current.as_ref()?;
        let name = e.active_name.as_ref().filter(|n| e.named.iter().any(|x| &x.name == *n && layout::same_layout(&x.tree, cur)));
        Some(name.cloned().unwrap_or_default())
    }
}

/// `%APPDATA%\AnimeIAI\layouts.json`, ou `<sessão>-layouts.json` ao lado do `--session-file`; `--layouts-file`
/// (`forced`) ganha de tudo, inclusive do `--no-session`.
pub fn path_for(no_session: bool, session_file: Option<&str>, forced: Option<&str>) -> Option<PathBuf> {
    if let Some(f) = forced {
        return Some(PathBuf::from(f));
    }
    if no_session {
        return None;
    }
    if let Some(s) = session_file {
        let p = PathBuf::from(s);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "sessao".into());
        return Some(p.with_file_name(format!("{stem}-layouts.json")));
    }
    let base = std::env::var_os("APPDATA").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(base).join("AnimeIAI").join("layouts.json"))
}

/// Lê o arquivo. Ausente = vazio; ilegível = vazio, registrado, e o arquivo ruim é copiado para `.bad` antes de qualquer
/// gravação (o layout do usuário nunca some em silêncio).
pub fn load(path: &Path) -> LayoutStore {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<LayoutStore>(&s) {
            Ok(st) => st,
            Err(e) => {
                let bad = path.with_extension("json.bad");
                tracing::warn!("layouts ilegíveis em {}: {e}; usando os de fábrica (cópia em {})", path.display(), bad.display());
                let _ = std::fs::copy(path, &bad);
                LayoutStore::default()
            }
        },
        Err(_) => LayoutStore::default(),
    }
}

/// Grava de forma atômica (arquivo temporário + rename).
pub fn save(path: &Path, store: &LayoutStore) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(store).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{default_tree, find_pane, saved_from_tree, PanelKind};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aiai-layouts-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("layouts.json")
    }

    fn custom(ws: WorkspaceId) -> SavedNode {
        let mut t = default_tree(ws);
        let id = find_pane(&t, PanelKind::Properties).or_else(|| find_pane(&t, PanelKind::Layers)).unwrap();
        t.tiles.set_visible(id, false);
        saved_from_tree(&t).unwrap()
    }

    #[test]
    fn factory_tree_is_not_stored_and_custom_is() {
        let mut s = LayoutStore::default();
        let ws = WorkspaceId::Pintura;
        assert!(!s.record_current(ws, saved_from_tree(&default_tree(ws))), "a fábrica não vira layout próprio");
        assert_eq!(s.get(ws).and_then(|e| e.current.clone()), None);
        assert!(s.record_current(ws, Some(custom(ws))));
        assert_eq!(s.get(ws).unwrap().factory_rev_seen, layout::factory_rev(ws));
        assert_eq!(s.label(ws), Some(String::new()), "personalizado sem nome");
        s.reset(ws);
        assert_eq!(s.label(ws), None);
    }

    #[test]
    fn newer_factory_never_overwrites_the_users_layout() {
        let ws = WorkspaceId::Layout;
        let mut s = LayoutStore::default();
        let mine = custom(ws);
        // layout próprio feito sobre a fábrica antiga
        s.entry(ws).current = Some(mine.clone());
        s.entry(ws).factory_rev_seen = layout::factory_rev(ws) - 1;
        assert!(s.factory_update_pending(ws));
        // mexer no layout não apaga o aviso nem a árvore
        s.record_current(ws, Some(mine.clone()));
        assert!(s.factory_update_pending(ws));
        assert_eq!(s.get(ws).unwrap().current.as_ref(), Some(&mine));
        // decidir (manter o meu) = revisão vista atualizada
        s.entry(ws).factory_rev_seen = layout::factory_rev(ws);
        assert!(!s.factory_update_pending(ws));
    }

    #[test]
    fn named_layouts_save_replace_apply_and_delete() {
        let ws = WorkspaceId::Animacao;
        let mut s = LayoutStore::default();
        let a = custom(ws);
        assert!(!s.save_named(ws, "Genga", a.clone()));
        assert!(s.save_named(ws, "Genga", a.clone()), "mesmo nome substitui");
        assert_eq!(s.get(ws).unwrap().named.len(), 1);
        s.record_current(ws, Some(a.clone()));
        assert_eq!(s.label(ws).as_deref(), Some("Genga"));
        assert_eq!(s.named(ws, "Genga"), Some(&a));
        assert!(s.delete_named(ws, "Genga"));
        assert!(!s.delete_named(ws, "Genga"));
        assert_eq!(s.label(ws), Some(String::new()));
    }

    #[test]
    fn file_round_trip_and_bad_file_is_kept_aside() {
        let p = tmp("rt");
        let mut s = LayoutStore::default();
        s.record_current(WorkspaceId::Cenario, Some(custom(WorkspaceId::Cenario)));
        s.save_named(WorkspaceId::Cenario, "Fundo largo", custom(WorkspaceId::Cenario));
        save(&p, &s).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"cenario\"") && text.contains("\"panel\": \"canvas\""), "chaves de texto: {text}");
        assert_eq!(load(&p), s);
        std::fs::write(&p, "{ quebrado").unwrap();
        assert_eq!(load(&p), LayoutStore::default());
        assert!(p.with_extension("json.bad").exists(), "o arquivo ilegível fica guardado");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn each_variant_keeps_its_own_layout() {
        let ws = WorkspaceId::Animacao;
        let mut s = LayoutStore::default();
        // a fábrica do Douga não vira layout próprio estando no Douga
        s.set_variant(ws, LayoutVariant::Douga);
        let douga = layout::default_tree_v(ws, LayoutVariant::Douga);
        assert!(!s.record_current(ws, saved_from_tree(&douga)));
        assert_eq!(s.label(ws), None);
        // personalizar o Douga não mexe no Genga
        let mut t = douga;
        let id = find_pane(&t, PanelKind::Properties).unwrap();
        t.tiles.set_visible(id, false);
        let mine = saved_from_tree(&t).unwrap();
        assert!(s.record_current(ws, Some(mine.clone())));
        s.set_variant(ws, LayoutVariant::Base);
        assert_eq!(s.label(ws), None, "o Genga continua de fábrica");
        assert!(s.workspaces.contains_key("animacao.douga"));
        s.set_variant(ws, LayoutVariant::Douga);
        assert_eq!(s.get(ws).and_then(|e| e.current.as_ref()), Some(&mine));
        // a variante não vai para o arquivo
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("variants"), "{json}");
    }

    #[test]
    fn no_session_has_no_file() {
        assert_eq!(path_for(true, Some("x.json"), None), None);
        assert_eq!(path_for(false, Some(r"C:\t\sessao.json"), None), Some(PathBuf::from(r"C:\t\sessao-layouts.json")));
        assert_eq!(path_for(true, None, Some(r"C:\t\l.json")), Some(PathBuf::from(r"C:\t\l.json")), "--layouts-file vale com --no-session");
    }

    #[test]
    fn floating_windows_round_trip_in_the_file_and_reset_drops_them() {
        use crate::dock_float::{IRect, SavedFloat, SavedMonitor};
        let p = tmp("float");
        let ws = WorkspaceId::Animacao;
        let mut s = LayoutStore::default();
        s.record_current(ws, Some(custom(ws)));
        let f = SavedFloat {
            tree: SavedNode::Pane { panel: "timeline".into(), hidden: false },
            pos: Some([2100, 80]),
            size: [1200, 360],
            monitor: Some(SavedMonitor { name: r"\\.\DISPLAY2".into(), work: IRect::new(1920, 0, 1920, 1032) }),
            home: None,
        };
        s.entry(ws).floating = vec![f.clone()];
        s.save_named(ws, "Dois monitores", custom(ws));
        s.entry(ws).named[0].floating = vec![f.clone()];
        save(&p, &s).unwrap();
        let back = load(&p);
        assert_eq!(back, s);
        assert_eq!(back.get(ws).unwrap().floating, vec![f.clone()]);
        assert_eq!(back.get(ws).unwrap().named[0].floating, vec![f]);
        // arquivo de antes das janelas soltas (sem o campo) continua legível
        let old = r#"{"version":1,"workspaces":{"pintura":{"factory_rev_seen":1,"current":null,"active_name":null,"named":[{"name":"a","tree":{"type":"pane","panel":"canvas"}}]}}}"#;
        let st: LayoutStore = serde_json::from_str(old).unwrap();
        assert!(st.get(WorkspaceId::Pintura).unwrap().floating.is_empty());
        // sem janela solta, o campo nem aparece no arquivo
        assert!(!serde_json::to_string(&st).unwrap().contains("floating"));
        let mut s2 = back;
        s2.reset(ws);
        assert!(s2.get(ws).unwrap().floating.is_empty(), "voltar à fábrica reencaixa tudo");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
