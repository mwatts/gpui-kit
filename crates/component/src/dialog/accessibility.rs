use std::any::Any;

use gpui::{AnyElement, Element as _, IntoElement, SharedString};

/// Converts a title without narrowing the public title API, retaining direct
/// text as the modal's accessible name.
///
/// Composite title elements cannot be inspected safely after construction.
/// Their callers provide the name with the modal's `accessibility_label`
/// builder instead.
pub(crate) fn into_accessible_title(title: impl IntoElement) -> (AnyElement, Option<SharedString>) {
    let title = title.into_element();
    let value = (&title as &dyn Any)
        .downcast_ref::<SharedString>()
        .cloned()
        .or_else(|| {
            (&title as &dyn Any)
                .downcast_ref::<&'static str>()
                .map(|value| SharedString::from(*value))
        });

    (title.into_any(), value)
}
