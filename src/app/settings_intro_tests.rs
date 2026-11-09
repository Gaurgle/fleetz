use super::*;

fn overlay(intro: bool) -> SettingsOverlay {
    SettingsOverlay::new(
        60,
        30,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        intro,
        Icons::Nerd,
    )
}

/// The row exists, is a toggle, and writes the key the loader reads. A row
/// whose `config_key` did not match `Config`'s field would save happily and
/// change nothing.
#[test]
fn the_intro_row_toggles_and_saves_under_the_key_config_reads() {
    assert!(SETTING_FIELDS.contains(&SettingField::Intro));
    assert_eq!(SettingField::Intro.config_key(), "intro");

    let mut so = overlay(true);
    so.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == SettingField::Intro)
        .expect("the intro row");
    assert_eq!(so.value_label(SettingField::Intro), bool_label(true));
    so.step(1);
    assert!(!so.intro, "the row did not toggle");
    assert_eq!(so.value_label(SettingField::Intro), bool_label(false));

    let saved = so
        .config_updates()
        .into_iter()
        .find(|(key, _)| *key == "intro")
        .expect("intro is not written to the file");
    assert!(
        matches!(saved.1, crate::config::SettingValue::Bool(false)),
        "the saved value is not the toggled one"
    );
}

/// It is marked restart-only, and honestly so: the loading screen only
/// covers the first load of a launch, so by the time this row can be
/// reached it has already been and gone.
#[test]
fn the_intro_row_says_it_needs_a_restart() {
    assert_eq!(SettingField::Intro.applies(), Applies::Restart);
}

/// Every row's key round-trips through a real save and load, which is the
/// only thing that proves the overlay and the config agree about names.
#[test]
fn toggling_the_row_off_survives_a_save_and_load() {
    let dir = std::env::temp_dir().join(format!("fleetz-intro-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("config.toml");

    let mut so = overlay(true);
    so.intro = false;
    crate::config::save_settings(&path, &so.config_updates()).expect("save");
    let loaded = crate::config::Config::load_from(&path).expect("load");
    assert!(!loaded.intro, "the toggle did not survive the round trip");

    let _ = std::fs::remove_dir_all(&dir);
}
