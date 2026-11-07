use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use scene_walls::{
    BIT_BOTTOM, BIT_LEFT, BIT_RIGHT, BIT_TOP, layer_data, parse, render_tiles, wangid,
};

fn layout(source: &str) -> scene_walls::WallLayout {
    parse(source, Path::new("."), Path::new("."))
        .unwrap_or_else(|error| panic!("{}: {}", error.code, error.message))
}

#[test]
fn isolated_wall_has_all_sides_open() {
    let walls = layout("version 1;\nheight 2;\nlayout {\n\".#.\"\n\"...\"\n\"...\"\n}\n");
    assert_eq!(walls.cells.len(), 1);
    assert_eq!(
        walls.cells[0].mask,
        BIT_TOP | BIT_RIGHT | BIT_BOTTOM | BIT_LEFT
    );
}

#[test]
fn straight_wall_has_two_open_sides() {
    let walls = layout("version 1;\nlayout {\n\"###\"\n}\n");
    let middle = walls.cells.iter().find(|cell| cell.x == 1).expect("middle");
    assert_eq!(middle.mask, BIT_TOP | BIT_BOTTOM);
}

#[test]
fn full_block_has_no_open_sides() {
    let walls = layout("version 1;\nlayout {\n\"###\"\n\"###\"\n\"###\"\n}\n");
    let center = walls
        .cells
        .iter()
        .find(|cell| cell.x == 1 && cell.y == 1)
        .expect("center");
    assert_eq!(center.mask, 0);
}

#[test]
fn door_and_window_are_gaps() {
    let walls = layout("version 1;\nlayout {\n\"#D#W#\"\n}\n");
    assert_eq!(walls.cells.len(), 3);
    assert!(walls.cells.iter().all(|cell| cell.x % 2 == 0));
}

#[test]
fn tiles_have_the_expected_size_and_sixteen_variants() {
    let walls = layout("version 1;\nheight 2;\nlayout {\n\"#\"\n}\n");
    let tiles = render_tiles(&walls, 16);
    assert_eq!(tiles.tile_width, 32);
    assert_eq!(tiles.tile_height, 48);
    assert_eq!(tiles.image.width, 32 * 16);
    assert_eq!(tiles.image.height, 48);
    assert!(
        tiles
            .image
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[3] == 255)
    );
}

#[test]
fn wang_ids_match_the_tiled_edge_order() {
    assert_eq!(wangid(0), [1, 0, 1, 0, 1, 0, 1, 0]);
    assert_eq!(wangid(15), [0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(wangid(BIT_TOP), [0, 0, 1, 0, 1, 0, 1, 0]);
}

#[test]
fn layer_data_uses_masks_and_first_gid() {
    let walls = layout("version 1;\nlayout {\n\"##\"\n\"##\"\n}\n");
    let data = layer_data(&walls, 2, 2, 0, 0, 10);
    assert_eq!(data, vec![19, 13, 22, 16]);
}

#[test]
fn material_colour_is_loaded() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let root = std::env::temp_dir().join(format!("iso-scene-walls-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create temp dir");
    fs::write(
        root.join("wall.toml"),
        "name = \"Wall\"\ncolor = \"#3366CC\"\noutline = \"#101820\"\n",
    )
    .expect("write material");
    let source = "version 1;\nmaterial wall = \"wall.toml\";\nlayout {\n\"#\"\n}\n";
    let walls = parse(source, &root, &root).expect("parse");
    assert_eq!(walls.palette.top, [0x33, 0x66, 0xCC]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn uneven_rows_are_rejected() {
    let error = parse(
        "version 1;\nlayout {\n\"##\"\n\"#\"\n}\n",
        Path::new("."),
        Path::new("."),
    )
    .expect_err("uneven");
    assert_eq!(error.code, "E0201");
}

#[test]
fn unknown_layout_character_is_rejected() {
    let error = parse(
        "version 1;\nlayout {\n\"#?\"\n}\n",
        Path::new("."),
        Path::new("."),
    )
    .expect_err("character");
    assert_eq!(error.code, "E0202");
}

#[test]
fn unclosed_layout_is_rejected() {
    let error = parse(
        "version 1;\nlayout {\n\"#\"\n",
        Path::new("."),
        Path::new("."),
    )
    .expect_err("unclosed");
    assert_eq!(error.code, "E0201");
}

fn front_walls(source: &str) -> Vec<(u32, u32)> {
    layout(source)
        .cells
        .iter()
        .filter(|cell| cell.front)
        .map(|cell| (cell.x, cell.y))
        .collect()
}

#[test]
fn near_walls_of_a_room_are_in_front() {
    // Outdoor floor around the room does not make the far walls front walls.
    let front =
        front_walls("version 1;\nlayout {\n\".....\"\n\".###.\"\n\".#..#\"\n\".####\"\n}\n");
    assert_eq!(front, [(4, 2), (2, 3), (3, 3)]);
}

#[test]
fn a_wall_right_behind_a_gap_stays_at_the_back() {
    // The door at (2, 3) stands in front of the wall at (1, 3), so that wall
    // must not be drawn over it; the wall at (3, 3) is in front of the door.
    let front =
        front_walls("version 1;\nlayout {\n\"#####\"\n\"#...#\"\n\"#...#\"\n\"##D##\"\n}\n");
    assert_eq!(front, [(4, 1), (4, 2), (3, 3)]);
}

#[test]
fn a_gap_closes_the_room() {
    // The floor behind a door gap is still indoors.
    let front = front_walls("version 1;\nlayout {\n\"###\"\n\"D.#\"\n\"###\"\n}\n");
    assert_eq!(front, [(2, 1), (1, 2)]);
}
