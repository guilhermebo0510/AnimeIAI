//! Pasta do personagem no modelo: projeto antigo sem materiais carrega, materiais serializam e voltam iguais, e o
//! corte apagado sai da lista.

use aiai_core::*;
use serde_json::json;

#[test]
fn old_character_without_materials_loads_and_empty_list_is_not_written() {
    let old = json!({ "type": "character", "profile": "p", "age": "17", "heads_tall": 6.0 });
    let k: NodeKind = serde_json::from_value(old).unwrap();
    let NodeKind::Character(c) = &k else { panic!() };
    assert!(c.materials.is_empty());
    assert!(serde_json::to_value(&k).unwrap().get("materials").is_none());
}

#[test]
fn materials_round_trip_with_every_kind_and_content() {
    let mut c = CharacterData::default();
    for (i, kind) in MaterialKind::ALL.into_iter().enumerate() {
        let content = if i % 2 == 0 { MaterialContent::Sheet { cut: Id(format!("c{i}")) } } else { MaterialContent::Image { blob: BlobHash(format!("{:064x}", i)), cut: Id(format!("c{i}")) } };
        c.materials.push(CharMaterial { id: Id(format!("m{i}")), kind, name: format!("Material {i}"), content, notes: if i == 1 { "nota".into() } else { String::new() }, palette: (kind == MaterialKind::Outfit).then(|| Id("pal".into())) });
    }
    let k = NodeKind::Character(c.clone());
    let j = serde_json::to_string(&k).unwrap();
    assert!(j.contains("\"mouth_chart\"") && j.contains("\"model_sheet\"") && j.contains("\"type\":\"image\""), "{j}");
    let back: NodeKind = serde_json::from_str(&j).unwrap();
    assert_eq!(back, k);
    for kind in MaterialKind::ALL {
        assert_eq!(MaterialKind::from_key(kind.key()), Some(kind));
    }
}

#[test]
fn deleting_a_material_cut_drops_it_from_the_list_and_undo_brings_it_back() {
    let mut s = CoreSession::for_tests("t");
    let ch = s.ids.next();
    s.apply(Command::CreateNode { id: Some(ch.clone()), parent: None, index: None, name: "Hana".into(), data: NodeKind::Character(CharacterData::default()) }).unwrap();
    let cut = s.ids.next();
    s.apply(Command::CreateNode { id: Some(cut.clone()), parent: Some(ch.clone()), index: None, name: "Rosto".into(), data: NodeKind::Cut(CutData::new(1)) }).unwrap();
    let m = CharMaterial { id: Id("m".into()), kind: MaterialKind::Expressions, name: "Rosto".into(), content: MaterialContent::Sheet { cut: cut.clone() }, notes: String::new(), palette: None };
    s.apply(Command::SetProps { id: ch.clone(), name: None, data: json!({ "materials": [m] }) }).unwrap();
    s.project.validate().unwrap();
    s.apply(Command::DeleteNode { id: cut }).unwrap();
    let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
    assert!(c.materials.is_empty());
    s.project.validate().unwrap();
    s.undo().unwrap();
    let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
    assert_eq!(c.materials.len(), 1);
    s.project.validate().unwrap();
}

#[test]
fn a_material_pointing_at_a_missing_cut_is_refused() {
    let mut s = CoreSession::for_tests("t");
    let ch = s.ids.next();
    s.apply(Command::CreateNode { id: Some(ch.clone()), parent: None, index: None, name: "Hana".into(), data: NodeKind::Character(CharacterData::default()) }).unwrap();
    let m = CharMaterial { id: Id("m".into()), kind: MaterialKind::Other, name: "x".into(), content: MaterialContent::Sheet { cut: Id("nada".into()) }, notes: String::new(), palette: None };
    // a sessão valida o projeto depois de cada comando: a referência solta é recusada e o documento fica como estava
    let e = s.apply(Command::SetProps { id: ch.clone(), name: None, data: json!({ "materials": [m] }) });
    assert!(e.is_err());
    let NodeKind::Character(c) = &s.project.node(&ch).unwrap().data else { panic!() };
    assert!(c.materials.is_empty());
}
