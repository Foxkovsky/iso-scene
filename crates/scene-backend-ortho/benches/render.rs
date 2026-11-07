//! Orthographic backend benchmark: the top-down classroom.

use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn benches(c: &mut Criterion) {
    let path = examples().join("classroom_top.scene");
    let source = std::fs::read_to_string(&path).expect("example exists");
    let program = scene_syntax::parse(&source).expect("example parses");
    let scene =
        scene_core::compile_scene(&program, &examples(), &examples()).expect("example compiles");
    c.bench_function("render/classroom-top", |b| {
        b.iter(|| scene_backend_ortho::render(&scene).expect("renders"));
    });
}

criterion_group! {
    name = group;
    config = Criterion::default().sample_size(20);
    targets = benches
}
criterion_main!(group);
