//! Iso backend benchmarks: a whole scene, single sprites and welded parts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use scene_core::{CompiledObject, SceneGraph};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn scene(file: &str) -> SceneGraph {
    let path = examples().join(file);
    let source = std::fs::read_to_string(&path).expect("example exists");
    let program = scene_syntax::parse(&source).expect("example parses");
    scene_core::compile_scene(&program, &examples(), &examples()).expect("example compiles")
}

/// A brick wall of `rows * columns` boxes welded into one solid.
fn brick_wall(rows: u32, columns: u32) -> CompiledObject {
    let source = format!(
        "version 1;\nobject wall ({columns}, 1) {{\n    merge {{\n        for row in 0..{rows} {{\n            for column in 0..{columns} {{\n                box at (column + (row % 2) * 0.5, 0, row * 0.5) size (1, 1, 0.5) material wood;\n            }}\n        }}\n    }}\n}}\n"
    );
    let program = scene_syntax::parse(&source).expect("wall parses");
    let model = scene_core::compile_model(&program, Path::new(".")).expect("wall compiles");
    (*model.objects["wall"]).clone()
}

fn benches(c: &mut Criterion) {
    let classroom = scene("classroom.scene");
    c.bench_function("render/classroom", |b| {
        b.iter(|| scene_backend_iso::render(&classroom).expect("renders"));
    });

    let materials = classroom.materials.clone();
    let chair = classroom
        .instances
        .iter()
        .find(|instance| instance.object.name == "chair")
        .expect("classroom has a chair")
        .object
        .clone();
    c.bench_function("sprite/chair@32", |b| {
        b.iter(|| {
            scene_backend_iso::render_object_sprite_at(&chair, 90, 0.0, 32, &materials)
                .expect("renders")
        });
    });
    c.bench_function("sprite/chair@128", |b| {
        b.iter(|| {
            scene_backend_iso::render_object_sprite_at(&chair, 90, 0.0, 128, &materials)
                .expect("renders")
        });
    });

    let wall = brick_wall(12, 16);
    c.bench_function("sprite/merged-wall-192@16", |b| {
        b.iter(|| {
            scene_backend_iso::render_object_sprite_at(&wall, 0, 0.0, 16, &BTreeMap::new())
                .expect("renders")
        });
    });
}

criterion_group! {
    name = group;
    config = Criterion::default().sample_size(20);
    targets = benches
}
criterion_main!(group);
