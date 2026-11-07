//! Sprite sheet benchmarks: rendering, packing and writing whole sheets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use scene_export_sprites::{Pivot, Projection, SheetObject, SheetOptions, export};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn options(name: &str, views: u32, scale: u32) -> SheetOptions {
    SheetOptions {
        name: name.into(),
        scale,
        projection: Projection::Iso,
        views: (0..views)
            .map(|step| f64::from(step) * 360.0 / f64::from(views))
            .collect(),
        pivot: Pivot::Front,
        shadow: true,
        padding: 0,
        debug_grid: false,
        atlas: false,
        max_rows: None,
        animations: Vec::new(),
        tile_width: None,
    }
}

fn benches(c: &mut Criterion) {
    let path = examples().join("classroom.scene");
    let source = std::fs::read_to_string(&path).expect("example exists");
    let program = scene_syntax::parse(&source).expect("example parses");
    let scene = scene_core::compile_scene(&program, &examples(), &examples()).expect("compiles");
    let mut furniture: Vec<SheetObject> = Vec::new();
    for instance in &scene.instances {
        if !furniture
            .iter()
            .any(|entry| entry.name == instance.object.name)
        {
            furniture.push(SheetObject {
                name: instance.object.name.clone(),
                object: instance.object.clone(),
                source: None,
            });
        }
    }
    let out = std::env::temp_dir().join(format!("iso-scene-bench-{}", std::process::id()));

    let furniture_16 = options("furniture", 16, 32);
    c.bench_function("sheet/furniture-x16@32", |b| {
        b.iter(|| export(&furniture, &scene.materials, &furniture_16, &out).expect("exports"));
    });

    let program = scene_syntax::parse(
        "version 1;\nobject crate (1, 1) {\n    box at (0, 0, 0) size (1, 1, 1) material wood;\n    box at (0.1, 0.1, 1) size (0.8, 0.8, 0.2) material wood;\n}\n",
    )
    .expect("crate parses");
    let model = scene_core::compile_model(&program, Path::new(".")).expect("crate compiles");
    let crate_object = model.objects["crate"].clone();
    let crates: Vec<SheetObject> = (0..128)
        .map(|index| SheetObject {
            name: format!("crate{index}"),
            object: Arc::clone(&crate_object),
            source: None,
        })
        .collect();
    let crates_8 = SheetOptions {
        max_rows: Some(32),
        ..options("crates", 8, 16)
    };
    c.bench_function("sheet/1024-cells@16", |b| {
        b.iter(|| export(&crates, &BTreeMap::new(), &crates_8, &out).expect("exports"));
    });
    let _ = std::fs::remove_dir_all(&out);
}

criterion_group! {
    name = group;
    config = Criterion::default().sample_size(10);
    targets = benches
}
criterion_main!(group);
