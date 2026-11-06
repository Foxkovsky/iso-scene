use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use scene_backend_voxel::{VoxelModel, voxelize, write_vox};
use scene_core::compile_scene;
use scene_syntax::parse;

fn compile(source: &str) -> scene_core::SceneGraph {
    let program = parse(source).unwrap_or_else(|error| panic!("{}: {}", error.code, error.message));
    let here = Path::new(".");
    compile_scene(&program, here, here)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

fn voxel_source(source: &str) -> VoxelModel {
    voxelize(&compile(source), 8)
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

#[test]
fn unit_box_is_eight_by_eight_by_eight() {
    let model = voxel_source(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    );
    assert_eq!(model.size, [8, 8, 8]);
    assert_eq!(model.voxels.len(), 512);
    assert_eq!(model.palette.len(), 1);
    assert!(model.voxels.iter().all(|voxel| voxel.color == 1));
}

#[test]
fn rotation_swaps_plan_extents() {
    let model = voxelize(
        &compile(
            "version 1; object slab (2, 1) { box at (0, 0, 0) size (2, 1, 1) material wood; } render iso; scale 8; place slab at (0, 0, 0) rot 90;",
        ),
        4,
    )
    .expect("voxelize");
    assert_eq!(model.size, [4, 8, 4]);
    assert_eq!(model.voxels.len(), 128);
}

#[test]
fn custom_material_colour_reaches_the_palette() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root = std::env::temp_dir().join(format!("iso-scene-voxel-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create temp dir");
    fs::write(
        root.join("paint.toml"),
        "name = \"Paint\"\ncolor = \"#3366CC\"\noutline = \"#101820\"\n",
    )
    .expect("write material");
    let source = r#"
        version 1;
        material paint = "paint.toml";
        object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material paint; }
        render iso;
        scale 8;
        place b at (0, 0, 0);
    "#;
    let program = parse(source).expect("parse");
    let scene = compile_scene(&program, &root, &root).expect("compile");
    let model = voxelize(&scene, 4).expect("voxelize");
    assert_eq!(model.palette, vec![[0x33, 0x66, 0xCC]]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn vox_encoding_has_the_expected_header_and_chunks() {
    let model = voxel_source(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let bytes = model.encode();
    assert_eq!(&bytes[..4], b"VOX ");
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 150);
    assert_eq!(&bytes[8..12], b"MAIN");
    let mut offset = 20;
    let mut tags = Vec::new();
    while offset + 12 <= bytes.len() {
        let tag = &bytes[offset..offset + 4];
        let length = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        tags.push(tag.to_vec());
        offset += 12 + length;
    }
    assert_eq!(tags, [b"SIZE".to_vec(), b"XYZI".to_vec(), b"RGBA".to_vec()]);
}

#[test]
fn vox_file_is_written() {
    let model = voxel_source(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path =
        std::env::temp_dir().join(format!("iso-scene-vox-{}-{nanos}.vox", std::process::id()));
    write_vox(&model, &path).expect("write vox");
    let bytes = fs::read(&path).expect("read vox");
    assert_eq!(&bytes[..4], b"VOX ");
    let _ = fs::remove_file(path);
}

#[test]
fn too_dense_grid_is_rejected() {
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 1) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let error = voxelize(&scene, 300).expect_err("grid is too large");
    assert_eq!(error.code, "E0602");
}

#[test]
fn an_oversized_grid_suggests_a_density_that_fits() {
    // 8 blocks tall at density 300 gives 2400 voxels on one axis. XYZI stores
    // coordinates as bytes, so at most 256 fit; density 32 is the largest one.
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 8) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let error = voxelize(&scene, 300).expect_err("grid is too large");
    assert_eq!(error.code, "E0602");
    assert!(
        error.message.contains("--density 32"),
        "message names the density: {}",
        error.message
    );
}

#[test]
fn a_model_without_a_valid_density_is_reported_as_too_large() {
    // 320 blocks cannot fit 256 voxels even at one voxel per block.
    let scene = compile(
        "version 1; object b (1, 1) { box at (0, 0, 0) size (1, 1, 320) material wood; } render iso; scale 8; place b at (0, 0, 0);",
    );
    let error = voxelize(&scene, 1).expect_err("grid is too tall");
    assert_eq!(error.code, "E0602");
    assert!(
        error.message.contains("scale it down"),
        "message explains that no density helps: {}",
        error.message
    );
}

#[test]
fn empty_scene_is_rejected() {
    let scene = compile("version 1; render iso; scale 8;");
    let error = voxelize(&scene, 8).expect_err("nothing to voxelize");
    assert_eq!(error.code, "E0602");
}

#[test]
fn slanted_shapes_are_voxelized() {
    let model = voxel_source(
        r"
        version 1;
        object shapes (4, 4) {
            slantbox at (0, 0, 0) size (1, 1, 1) offset (0.2, 0.1) material wood;
            slantcyl at (2, 0, 0) radius 0.5 height 1 offset (0.2, 0) material metal;
            ramp at (0, 2, 0) size (1, 1) rise (0, 0.8) material red;
            sphere at (2.5, 2.5, 0.5) radius 0.5 material light;
        }
        render iso;
        scale 8;
        place shapes at (0, 0, 0);
        ",
    );
    assert_ne!(model.voxels.len(), 0);
    assert!(model.voxels.len() > 1_000);
}
