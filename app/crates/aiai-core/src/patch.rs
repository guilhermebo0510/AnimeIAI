//! JSON Merge Patch (RFC 7386) com geração do patch inverso. É a base do comando genérico `set_props`:
//! o agente (ou a UI) muda qualquer campo de qualquer nó com um objeto parcial, e o undo é o patch dos valores antigos.

use serde_json::{Map, Value};

/// Aplica `patch` sobre `target` (RFC 7386: `null` remove a chave; objeto mescla; o resto substitui).
pub fn merge(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(p) => {
            if !target.is_object() {
                *target = Value::Object(Map::new());
            }
            let t = target.as_object_mut().expect("acabou de virar objeto");
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        other => *target = other.clone(),
    }
}

/// Patch que, aplicado depois de `merge(old, patch)`, devolve `old`.
pub fn inverse(old: &Value, patch: &Value) -> Value {
    match patch {
        Value::Object(p) => {
            let mut inv = Map::new();
            let old_obj = old.as_object();
            for (k, v) in p {
                let old_v = old_obj.and_then(|o| o.get(k));
                let entry = match (old_v, v) {
                    (Some(ov @ Value::Object(_)), Value::Object(_)) => inverse(ov, v),
                    (Some(ov), _) => ov.clone(),
                    (None, _) => Value::Null,
                };
                inv.insert(k.clone(), entry);
            }
            Value::Object(inv)
        }
        _ => old.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_and_inverse_roundtrip() {
        let old = json!({"a": 1, "b": {"c": 2, "d": [1, 2]}, "e": "x"});
        let patch = json!({"a": 5, "b": {"c": null, "f": true}, "g": 3});
        let inv = inverse(&old, &patch);
        let mut v = old.clone();
        merge(&mut v, &patch);
        assert_eq!(v, json!({"a": 5, "b": {"d": [1, 2], "f": true}, "e": "x", "g": 3}));
        merge(&mut v, &inv);
        assert_eq!(v, old);
    }
}
