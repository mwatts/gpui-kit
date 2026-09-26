//! Host-composed custom block leaves.
//!
//! Built-in paint for [`block_markdown::BlockType::Custom`] is a type label plus
//! plain text. Hosts that own interactive leaves (spreadsheet, media embeds,
//! later block types) register a composer by custom name. The registry stays in
//! the kit so Ashlar and other apps can plug leaves without forking paint.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AnyElement, App, Global, Window};

use crate::BlockSnapshot;

/// Paint one custom block, or `None` to keep the default label+text leaf.
pub type CustomBlockComposer =
    Arc<dyn Fn(&BlockSnapshot, &mut Window, &mut App) -> Option<AnyElement> + Send + Sync>;

#[derive(Default)]
struct CustomBlockRegistry {
    composers: HashMap<String, CustomBlockComposer>,
}

impl Global for CustomBlockRegistry {}

fn registry(cx: &mut App) -> &mut CustomBlockRegistry {
    if !cx.has_global::<CustomBlockRegistry>() {
        cx.set_global(CustomBlockRegistry::default());
    }
    cx.global_mut::<CustomBlockRegistry>()
}

/// Register (or replace) the outside composer for `BlockType::Custom(name)`.
pub fn register_custom_block(
    cx: &mut App,
    name: impl Into<String>,
    composer: CustomBlockComposer,
) {
    registry(cx).composers.insert(name.into(), composer);
}

/// Remove a previously registered composer.
pub fn unregister_custom_block(cx: &mut App, name: &str) {
    if cx.has_global::<CustomBlockRegistry>() {
        cx.global_mut::<CustomBlockRegistry>()
            .composers
            .remove(name);
    }
}

/// Whether a host registered an outside composer for this custom name.
#[must_use]
pub fn is_custom_block_registered(cx: &App, name: &str) -> bool {
    cx.try_global::<CustomBlockRegistry>()
        .is_some_and(|registry| registry.composers.contains_key(name))
}

/// Invoke the registered composer, if any.
pub(crate) fn compose(
    name: &str,
    block: &BlockSnapshot,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let composer = cx
        .try_global::<CustomBlockRegistry>()?
        .composers
        .get(name)
        .cloned()?;
    composer(block, window, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn registered_composer_is_queryable(cx: &mut TestAppContext) {
        cx.update(|cx| {
            register_custom_block(
                cx,
                "spreadsheet",
                Arc::new(|_, _, _| None),
            );
            assert!(is_custom_block_registered(cx, "spreadsheet"));
            assert!(!is_custom_block_registered(cx, "callout"));
            unregister_custom_block(cx, "spreadsheet");
            assert!(!is_custom_block_registered(cx, "spreadsheet"));
        });
    }
}
