//! Typed Tree binding for gpui-component.
//!
//! `list` / `uniform_list` and `DockArea` already bind in gpui-shell. Select,
//! Combobox, and DataTable live in their own component-shell families. This
//! module registers only Tree / TreeItem.

pub(super) use super::support::{bool_method, require_child};

pub(super) use super::typed_child::{Carrier, take};

mod tree;
use gpui_shell::{ComponentRegistry, RegistryError};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use tree::test_probe;
pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    tree::register(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_is_only_honest_tree_surface() {
        let mut r = ComponentRegistry::new(
            gpui_shell::COMPONENT_REGISTRY_API_VERSION,
            gpui_shell::DEFAULT_COMPONENT_MODULE,
        )
        .unwrap();
        register(&mut r).unwrap();
        assert_eq!(
            r.freeze()
                .unwrap()
                .descriptors()
                .map(|d| d.name())
                .collect::<Vec<_>>(),
            ["TreeItem", "Tree"]
        );
    }
}
