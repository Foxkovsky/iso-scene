//! Previews of `.model` files, shared by the CLI and the MCP server.

/// Build a one-row scene from a `.model` file so it can be previewed.
/// # Errors
///
/// Returns a message for a model without objects.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn model_scene(model: &scene_core::CompiledModel) -> Result<scene_core::SceneGraph, String> {
    if model.objects.is_empty() {
        return Err("the model defines no objects".to_string());
    }
    let mut extent: f64 = 1.0;
    for object in model.objects.values() {
        let local = object.local_bounds();
        let size = local.size();
        extent = extent.max(size[0]).max(size[1]).max(size[2]);
    }
    let scale = (220.0 / extent).round().clamp(1.0, 64.0) as u32;

    let mut instances = Vec::new();
    let mut offset = 0.0_f64;
    for object in model.objects.values() {
        instances.push(scene_core::Instance {
            object: object.clone(),
            at: [offset, 0.0, 0.0],
            rotation: 0,
            angle: 0.0,
        });
        offset += object.width + object.width.mul_add(0.25, 1.0);
    }

    let mut bounds = scene_core::Bounds {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };
    for instance in &instances {
        let local = instance.world_bounds();
        for axis in 0..3 {
            bounds.min[axis] = bounds.min[axis].min(local.min[axis]);
            bounds.max[axis] = bounds.max[axis].max(local.max[axis]);
        }
    }

    Ok(scene_core::SceneGraph {
        render: scene_syntax::ast::RenderKind::Iso,
        scale,
        canvas: None,
        sun: [135.0, 45.0],
        ambient: 0.35,
        background: scene_syntax::ast::Background::Transparent,
        materials: model.materials.clone(),
        instances,
        bounds,
        animations: Vec::new(),
    })
}
