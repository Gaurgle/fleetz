use super::*;

fn overlay(icons: Icons) -> SettingsOverlay {
    let mut so = SettingsOverlay::new(
        60,
        30,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        true,
        icons,
    );
    so.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == SettingField::Icons)
        .expect("the icons row");
    so
}

fn saved_icons(so: &SettingsOverlay) -> crate::config::SettingValue {
    so.config_updates()
        .into_iter()
        .find(|(key, _)| *key == "icons")
        .map(|(_, value)| value)
        .expect("icons is not written to the file")
}

/// The row switches between the two sets both ways, says which is
/// selected, applies without a restart and writes the key the loader
/// reads. Fails if the row is inert, mislabelled or not saved.
#[test]
fn the_icons_row_toggles_and_saves_under_the_key_config_reads() {
    assert_eq!(SettingField::Icons.config_key(), "icons");
    assert_eq!(SettingField::Icons.applies(), Applies::Live);

    let mut so = overlay(Icons::Nerd);
    assert_eq!(so.value_label(SettingField::Icons), "nerd");
    assert_eq!(saved_icons(&so), crate::config::SettingValue::Str("nerd"));

    so.step(1);
    assert_eq!(so.icons, Icons::Plain, "the row did not toggle");
    assert_eq!(so.value_label(SettingField::Icons), "plain");
    assert_eq!(saved_icons(&so), crate::config::SettingValue::Str("plain"));

    so.step(-1);
    assert_eq!(so.icons, Icons::Nerd, "the row did not toggle back");
}

/// Typing a digit on the row changes nothing: it is not numeric.
#[test]
fn digits_leave_the_icons_row_alone() {
    let mut so = overlay(Icons::Plain);
    so.push_digit(1);
    assert_eq!(so.icons, Icons::Plain);
}

/// Both values survive a real save and load, read back the way launch
/// reads them (`Icons::from_config`). Fails if the written string is one
/// the loader does not recognise, which would fall back to Nerd.
#[test]
fn each_icon_set_survives_a_save_and_load() {
    let dir = std::env::temp_dir().join(format!("fleetz-icons-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("config.toml");

    for icons in [Icons::Plain, Icons::Nerd] {
        let so = overlay(icons);
        crate::config::save_settings(&path, &so.config_updates()).expect("save");
        let loaded = crate::config::Config::load_from(&path).expect("load");
        let read_back = loaded
            .icons
            .as_deref()
            .map(Icons::from_config)
            .unwrap_or_default();
        assert_eq!(read_back, icons, "the set did not survive the round trip");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
