//! Tiled export benchmarks: sprites, map layers and collisions, written to disk.

use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use scene_export_tiled::{WallOrder, export_with};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn benches(c: &mut Criterion) {
    let path = examples().join("classroom.scene");
    let source = std::fs::read_to_string(&path).expect("example exists");
    let program = scene_syntax::parse(&source).expect("example parses");
    let scene = scene_core::compile_scene(&program, &examples(), &examples()).expect("compiles");
    // A wall ring around the classroom, with a door in the near wall.
    let (columns, rows) = (36, 22);
    let mut layout = String::from("version 1;\nheight 2;\nlayout {\n");
    for y in 0..rows {
        let row: String = (0..columns)
            .map(|x| match (x, y) {
                (18, y) if y == rows - 1 => 'D',
                (0, _) | (_, 0) => '#',
                (x, y) if x == columns - 1 || y == rows - 1 => '#',
                _ => '.',
            })
            .collect();
        layout.push('"');
        layout.push_str(&row);
        layout.push_str("\"\n");
    }
    layout.push_str("}\n");
    let walls = scene_walls::parse(&layout, &examples(), &examples()).expect("walls parse");

    let out = std::env::temp_dir().join(format!("iso-scene-bench-tiled-{}", std::process::id()));
    let mut group = c.benchmark_group("tiled");
    group.sample_size(20);
    group.bench_function("classroom", |b| {
        b.iter(|| export_with(&scene, None, false, WallOrder::default(), &out).expect("export"));
    });
    group.bench_function("classroom+walls", |b| {
        b.iter(|| {
            export_with(&scene, Some(&walls), false, WallOrder::default(), &out).expect("export")
        });
    });
    group.finish();
    let _ = std::fs::remove_dir_all(out);
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
