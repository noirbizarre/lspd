//! Generic merge primitives shared by every patch type in `config::*`.
//!
//! There are exactly three merge shapes used across the whole configuration
//! model (`docs/adr/0002-config-layering-and-merge-semantics.md`):
//!
//! 1. **Replace-if-present**: an `Option<T>` field where a later layer
//!    setting it at all discards the earlier value wholesale (`command`,
//!    `args`, `languages`, activation lists).
//! 2. **Merge-by-key**: a map where each key's *value* is itself merged
//!    (LSP definitions, watch rules — both keyed by stable name).
//! 3. **Deep JSON merge**: `settings`/`initializationOptions`/`capabilities`,
//!    where a later layer's object keys merge recursively into the earlier
//!    object rather than replacing it outright, so `[lsp.pyright.settings]`
//!    in one file and `[lsp.pyright.settings.python.analysis]` in another can
//!    both contribute without either erasing the other.

use std::collections::HashMap;
use std::hash::Hash;

/// Later-wins replacement for a plain `Option` field: `None` means "this
/// layer did not touch it", not "clear it" — there is deliberately no way to
/// unset a value set by a lower layer, only to override it.
pub fn replace<T>(base: Option<T>, patch: Option<T>) -> Option<T> {
    patch.or(base)
}

/// Deep-merges two `serde_json::Value`s: objects merge key-by-key
/// (recursively), arrays and scalars from `patch` replace `base` outright.
/// `null` in `patch` is treated the same as "not merged" would be for a
/// scalar (i.e. it wins and replaces, matching JSON Merge Patch semantics)
/// rather than clearing the key — the MVP has no need to *unset* a nested
/// setting, only to override it.
pub fn merge_json(base: serde_json::Value, patch: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match (base, patch) {
        (Value::Object(mut base_map), Value::Object(patch_map)) => {
            for (key, patch_value) in patch_map {
                let merged = match base_map.remove(&key) {
                    Some(base_value) => merge_json(base_value, patch_value),
                    None => patch_value,
                };
                base_map.insert(key, merged);
            }
            Value::Object(base_map)
        }
        // Any non-object pairing: the patch wins outright.
        (_, patch) => patch,
    }
}

/// Merges two optional JSON blobs: `None` on either side falls through to
/// the other; both present deep-merges via [`merge_json`].
pub fn merge_json_opt(
    base: Option<serde_json::Value>,
    patch: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match (base, patch) {
        (Some(base), Some(patch)) => Some(merge_json(base, patch)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// Merges two maps key-by-key, delegating to `merge_value` for keys present
/// in both layers and simply inserting keys present in only one. Used for
/// `[lsp.*]` and `[watch.*]`, which merge independently by name — overriding
/// one definition never touches another (spec §9).
pub fn merge_maps_by_key<K, V>(
    mut base: HashMap<K, V>,
    patch: HashMap<K, V>,
    mut merge_value: impl FnMut(V, V) -> V,
) -> HashMap<K, V>
where
    K: Eq + Hash,
{
    for (key, patch_value) in patch {
        match base.remove(&key) {
            Some(base_value) => {
                base.insert(key, merge_value(base_value, patch_value));
            }
            None => {
                base.insert(key, patch_value);
            }
        }
    }
    base
}

/// Merges two plain string maps (e.g. LSP `environment`) per-key, later
/// layer wins per key rather than replacing the whole map.
pub fn merge_env(
    base: HashMap<String, String>,
    patch: HashMap<String, String>,
) -> HashMap<String, String> {
    merge_maps_by_key(base, patch, |_old, new| new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replace_prefers_the_patch_when_it_is_present() {
        assert_eq!(replace(Some(1), Some(2)), Some(2));
    }

    #[test]
    fn replace_falls_back_to_the_base_when_the_patch_is_absent() {
        assert_eq!(replace(Some(1), None), Some(1));
    }

    #[test]
    fn merge_json_combines_disjoint_object_keys() {
        let base = json!({"a": 1});
        let patch = json!({"b": 2});
        assert_eq!(merge_json(base, patch), json!({"a": 1, "b": 2}));
    }

    #[test]
    fn merge_json_merges_nested_objects_recursively() {
        let base = json!({"python": {"analysis": {"typeCheckingMode": "basic"}}});
        let patch = json!({"python": {"analysis": {"extraPaths": ["/src"]}}});
        assert_eq!(
            merge_json(base, patch),
            json!({"python": {"analysis": {"typeCheckingMode": "basic", "extraPaths": ["/src"]}}})
        );
    }

    #[test]
    fn merge_json_replaces_arrays_wholesale_rather_than_concatenating() {
        let base = json!({"extraPaths": ["/a"]});
        let patch = json!({"extraPaths": ["/b"]});
        assert_eq!(merge_json(base, patch), json!({"extraPaths": ["/b"]}));
    }

    #[test]
    fn merge_json_replaces_a_scalar_with_an_object_when_the_patch_shape_changes() {
        let base = json!({"x": 1});
        let patch = json!({"x": {"y": 2}});
        assert_eq!(merge_json(base, patch), json!({"x": {"y": 2}}));
    }

    #[test]
    fn merge_maps_by_key_merges_shared_keys_and_keeps_disjoint_ones() {
        let base = HashMap::from([("a".to_string(), 1), ("b".to_string(), 2)]);
        let patch = HashMap::from([("b".to_string(), 20), ("c".to_string(), 3)]);
        let merged = merge_maps_by_key(base, patch, |old, new| old + new);
        assert_eq!(
            merged,
            HashMap::from([
                ("a".to_string(), 1),
                ("b".to_string(), 22),
                ("c".to_string(), 3)
            ])
        );
    }
}
