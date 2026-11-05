use std::fs;
use std::path::Path;

#[test]
fn every_example_lexes() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;
    for entry in fs::read_dir(&examples).expect("examples directory") {
        let path = entry.expect("directory entry").path();
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("model" | "scene")) {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read example");
        scene_syntax::lex(&source).unwrap_or_else(|error| {
            panic!(
                "{}: {} {} at {}..{}",
                path.display(),
                error.code,
                error.message,
                error.span.start,
                error.span.end
            )
        });
        checked += 1;
    }
    assert!(checked >= 3, "expected at least three examples");
}
