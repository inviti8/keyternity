//! Keyternity's palette replaces SkelForm's purples, including in saved settings.

use keyternity_lib::shared::*;

#[test]
fn saved_purple_defaults_become_teal() {
    let mut saved = ColorConfig::default();
    saved.main = Color::new(32, 25, 46, 255);
    saved.light_accent = Color::new(65, 46, 105, 255);
    saved.bound_vert = Color::new(184, 110, 251, 255);
    saved.retire_skelform_purples();

    let new = ColorConfig::default();
    assert!(saved.main == new.main);
    assert!(saved.light_accent == new.light_accent);
    assert!(saved.bound_vert == new.bound_vert);
}

#[test]
fn colours_someone_changed_are_kept() {
    let mut saved = ColorConfig::default();
    let custom = Color::new(33, 25, 46, 255);
    saved.main = custom;
    saved.text = Color::new(32, 25, 46, 255); // the old main colour, but not on `main`
    saved.retire_skelform_purples();
    assert!(saved.main == custom);
    assert!(saved.text == Color::new(32, 25, 46, 255));
}
