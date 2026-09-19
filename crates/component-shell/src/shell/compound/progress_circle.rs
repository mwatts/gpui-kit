use gpui_component::{Sizable as _, Size, progress::ProgressCircle};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, ComponentArgument, ComponentDescriptor,
    ComponentMaterializer, ComponentPayload, ComponentRegistry, ConstructorDescriptor,
    MaterializeRequest, MethodDescriptor, RegistryError, anyhow,
    gpui::{self, IntoElement as _, ParentElement as _, Refineable as _, Styled as _},
};
use std::sync::Arc;

use super::common::{finite_f32, nonempty_id};

#[derive(Clone)]
struct ProgressCirclePayload(String);
#[derive(Clone)]
enum ProgressCircleOp {
    Value(f32),
    Loading(bool),
    Label(String),
    Size(Size),
}
struct ProgressCircleMaterializer;
impl ProgressCircleMaterializer {
    fn component<'a>(
        payload: &ComponentPayload,
        ops: impl IntoIterator<Item = &'a ProgressCircleOp>,
    ) -> anyhow::Result<ProgressCircle> {
        let id = &payload
            .downcast_ref::<ProgressCirclePayload>()
            .ok_or_else(|| anyhow::anyhow!("ProgressCircle received an incompatible payload"))?
            .0;
        Ok(ops
            .into_iter()
            .fold(ProgressCircle::new(id.clone()), |progress, op| match op {
                ProgressCircleOp::Value(value) => progress.value(*value),
                ProgressCircleOp::Loading(value) => progress.loading(*value),
                ProgressCircleOp::Label(value) => progress.accessibility_label(value.clone()),
                ProgressCircleOp::Size(value) => progress.with_size(*value),
            }))
    }
}
impl ComponentMaterializer for ProgressCircleMaterializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let ops = request
            .methods()
            .filter_map(|m| m.payload().downcast_ref::<ProgressCircleOp>());
        let mut component = Self::component(request.payload(), ops)?;
        component.style().refine(&request.take_style());
        component.extend(request.take_children()?);
        Ok(component.into_any_element())
    }
}
fn unary(
    name: &'static str,
    schema: ArgumentSchema,
    documentation: &'static str,
    factory: fn(&ComponentArgument) -> Result<ProgressCircleOp, String>,
) -> MethodDescriptor {
    MethodDescriptor::new(
        name,
        vec![ArgumentDescriptor::new(name, schema)],
        move |args| match args {
            [arg] => factory(arg).map(ComponentPayload::new),
            _ => Err(format!(
                "ProgressCircle.{name}({name}) expects one argument"
            )),
        },
    )
    .with_documentation(documentation)
}
pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register(
        ComponentDescriptor::new("ProgressCircle", Arc::new(ProgressCircleMaterializer))
            .with_constructors(vec![ConstructorDescriptor::new(
                "ProgressCircle",
                vec![ArgumentDescriptor::new("id", ArgumentSchema::String)],
                |args| match args {
                    [ComponentArgument::String(id)] => nonempty_id(id, "ProgressCircle")
                        .map(ProgressCirclePayload)
                        .map(ComponentPayload::new),
                    _ => Err("ProgressCircle(id) expects a string id".into()),
                },
            )])
            .with_methods(vec![
                unary(
                    "value",
                    ArgumentSchema::Number,
                    "Sets percentage progress; the component clamps it to 0–100.",
                    |arguments| match arguments {
                        ComponentArgument::Number(value) => {
                            finite_f32(*value, "ProgressCircle.value(value)").map(ProgressCircleOp::Value)
                        }
                        _ => Err(
                            "ProgressCircle.value(value) expects a finite number representable as f32"
                                .into(),
                        ),
                    },
                ),
                unary(
                    "loading",
                    ArgumentSchema::Boolean,
                    "Enables indeterminate loading animation.",
                    |arguments| match arguments {
                        ComponentArgument::Boolean(value) => Ok(ProgressCircleOp::Loading(*value)),
                        _ => Err("ProgressCircle.loading(loading) expects a boolean".into()),
                    },
                ),
                unary(
                    "accessibility_label",
                    ArgumentSchema::String,
                    "Sets the accessible name.",
                    |arguments| match arguments {
                        ComponentArgument::String(value) => Ok(ProgressCircleOp::Label(value.clone())),
                        _ => Err("ProgressCircle.accessibility_label(label) expects a string".into()),
                    },
                ),
                unary(
                    "size",
                    ArgumentSchema::Enum(&["xsmall", "small", "medium", "large"]),
                    "Sets the semantic size.",
                    |arguments| match arguments {
                        ComponentArgument::Enum(value) => match value.as_str() {
                            "xsmall" => Ok(ProgressCircleOp::Size(Size::XSmall)),
                            "small" => Ok(ProgressCircleOp::Size(Size::Small)),
                            "medium" => Ok(ProgressCircleOp::Size(Size::Medium)),
                            "large" => Ok(ProgressCircleOp::Size(Size::Large)),
                            _ => Err(format!("unsupported ProgressCircle size `{value}`")),
                        },
                        _ => Err("ProgressCircle.size(size) expects a size literal".into()),
                    },
                ),
            ])
            .with_documentation("A native circular determinate or indeterminate progress indicator with optional center content."),
    )?;
    Ok(())
}
