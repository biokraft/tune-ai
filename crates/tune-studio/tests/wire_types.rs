//! `apps/desktop/src/client/generated.ts` is generated from the Rust wire
//! types. `UPDATE_GOLDEN=1` rewrites the file.

use tune_studio::wire_types_ts;

const GENERATED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../apps/desktop/src/client/generated.ts"
);

#[test]
fn generated_types_are_current() {
    let text = wire_types_ts();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(GENERATED, &text).expect("write generated.ts");
    }
    let on_disk = std::fs::read_to_string(GENERATED).unwrap_or_default();
    assert_eq!(
        on_disk, text,
        "generated.ts is stale; run `UPDATE_GOLDEN=1 cargo test -p tune-studio --test wire_types`"
    );
}
