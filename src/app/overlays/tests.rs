use super::*;

fn settings() -> SettingsOverlay {
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
        true,
        Icons::Nerd,
    )
}

/// Put the cursor on a field by name rather than by number, so inserting a
/// row does not silently retarget every test that follows it.
fn focus(s: &mut SettingsOverlay, field: SettingField) {
    s.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == field)
        .expect("field is in SETTING_FIELDS");
    assert_eq!(s.field(), field);
}

#[test]
fn settings_cursor_wraps_both_ways() {
    // The last row is derived rather than named: this test is about the
    // wrap, and adding a settings row should not break it.
    let last = *SETTING_FIELDS.last().expect("at least one setting");
    let mut s = settings();
    assert_eq!(s.field(), SettingField::RefreshInterval);
    s.move_cursor(-1);
    assert_eq!(s.field(), last, "wraps to the end");
    s.move_cursor(1);
    assert_eq!(s.field(), SettingField::RefreshInterval);
}

#[test]
fn settings_interval_clamps_to_its_range() {
    let mut s = settings();
    for _ in 0..100 {
        s.step(-1);
    }
    assert_eq!(s.refresh_interval, 5, "never reaches zero");
    for _ in 0..2000 {
        s.step(1);
    }
    assert_eq!(s.refresh_interval, 3600);
}

#[test]
fn settings_view_cycles_through_auto() {
    let mut s = settings();
    assert_eq!(s.view, None);
    focus(&mut s, SettingField::View);
    s.step(1);
    assert_eq!(s.view, Some(View::Detail));
    s.step(1);
    assert_eq!(s.view, Some(View::Compact));
    s.step(1);
    assert_eq!(s.view, None, "wraps back to auto");
    s.step(-1);
    assert_eq!(s.view, Some(View::Compact), "and cycles backwards");
}

#[test]
fn settings_work_hours_stay_ordered() {
    // The pair must never invert or collapse: an empty work slot would
    // make the time-slot labels nonsense.
    let mut s = settings();
    focus(&mut s, SettingField::WorkStart);
    for _ in 0..40 {
        s.step(1);
    }
    assert!(s.work_start < s.work_end);
    assert_eq!(s.work_start, 17, "stops one below work_end");

    focus(&mut s, SettingField::WorkEnd);
    for _ in 0..40 {
        s.step(-1);
    }
    assert!(s.work_start < s.work_end);
    assert_eq!(s.work_end, 18, "stops one above work_start");
}

#[test]
fn settings_digits_type_into_numeric_fields_only() {
    let mut s = settings();
    s.push_digit(3);
    s.push_digit(0);
    assert_eq!(s.refresh_interval, 30, "'3' then '0' reads as 30");

    focus(&mut s, SettingField::View); // not numeric
    let before = s.view;
    s.push_digit(7);
    assert_eq!(
        s.view, before,
        "digits are ignored, not swallowed as a step"
    );
}

#[test]
fn settings_normalize_lifts_a_half_typed_value_to_its_floor() {
    // Typing "30" passes through "3", which is below INTERVAL_MIN. The
    // intermediate state is allowed; applying it is not.
    let mut s = settings();
    s.push_digit(3);
    assert_eq!(s.refresh_interval, 3);
    s.normalize();
    assert_eq!(s.refresh_interval, 5);
}

#[test]
fn settings_poll_interval_has_its_own_floor() {
    // Lower than the sweep floor on purpose: a poll is one GraphQL call,
    // a sweep is a fetch against every remote.
    let mut s = settings();
    focus(&mut s, SettingField::GithubPollInterval);
    for _ in 0..100 {
        s.step(-1);
    }
    assert_eq!(s.github_poll_interval, POLL_MIN);
    // And the sweep floor is untouched by it: two separate limits, because
    // the two operations cost very different things.
    assert_eq!(s.refresh_interval, 60);
}

#[test]
fn settings_watch_is_a_restart_field() {
    // `w` changes the live state; this row only sets what the next launch
    // starts with, and the overlay says so rather than looking inert.
    assert_eq!(SettingField::Watch.applies(), Applies::Restart);
    let mut s = settings();
    focus(&mut s, SettingField::Watch);
    let before = s.watch;
    s.step(1);
    assert_ne!(s.watch, before, "toggles");
}

#[test]
fn settings_write_every_field_back() {
    use crate::config::SettingValue;
    let mut s = settings();
    s.refresh_interval = 90;
    s.view = Some(View::Compact);
    s.persist_panes = true;

    let updates = s.config_updates();
    let by_key = |k: &str| {
        updates
            .iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(by_key("refresh_interval"), Some(SettingValue::Int(90)));
    assert_eq!(by_key("github_poll_interval"), Some(SettingValue::Int(30)));
    assert_eq!(by_key("watch"), Some(SettingValue::Bool(true)));
    assert_eq!(by_key("view"), Some(SettingValue::Str("compact")));
    assert_eq!(by_key("work_start"), Some(SettingValue::Int(9)));
    assert_eq!(by_key("persist_pane_sizes"), Some(SettingValue::Bool(true)));
}

#[test]
fn window_overlay_opens_on_number_field() {
    let o = TimeWindowOverlay::new(TimeWindow {
        n: 2,
        unit: WindowUnit::Weeks,
    });
    assert_eq!(o.field, WindowField::Number);
    assert_eq!(o.n, 2);
}

#[test]
fn window_overlay_slot_forces_unit_field() {
    let o = TimeWindowOverlay::new(TimeWindow::default());
    assert_eq!(o.field, WindowField::Unit);
    let mut o = o;
    o.toggle_field();
    assert_eq!(o.field, WindowField::Unit); // can't focus the dimmed number
}

#[test]
fn window_overlay_toggle_field_flips() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    assert_eq!(o.field, WindowField::Number);
    o.toggle_field();
    assert_eq!(o.field, WindowField::Unit);
    o.toggle_field();
    assert_eq!(o.field, WindowField::Number);
}

#[test]
fn window_overlay_number_wraps_both_ways() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 31,
        unit: WindowUnit::Days,
    });
    o.step_up();
    assert_eq!(o.n, 1);
    o.step_down();
    assert_eq!(o.n, 31);
}

#[test]
fn window_overlay_unit_cycles_forward_and_back() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    o.toggle_field(); // focus unit
    o.step_down(); // Days -> Weeks
    assert_eq!(o.unit, WindowUnit::Weeks);
    o.step_up(); // Weeks -> Days
    assert_eq!(o.unit, WindowUnit::Days);
}

#[test]
fn window_overlay_unit_wraps_and_slot_grabs_focus() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Years,
    });
    o.toggle_field(); // focus unit
    o.step_down(); // Years wraps to Slot
    assert_eq!(o.unit, WindowUnit::Slot);
    assert_eq!(o.field, WindowField::Unit);
}

#[test]
fn window_overlay_digit_entry_two_digit_and_clamp() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    o.push_digit(2);
    assert_eq!(o.n, 2);
    o.push_digit(9);
    assert_eq!(o.n, 29); // "2" then "9" = 29
    o.push_digit(5);
    assert_eq!(o.n, 5); // third digit starts a fresh entry
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Days,
    });
    o.push_digit(3);
    o.push_digit(5);
    assert_eq!(o.n, 31); // 35 clamps to max
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 5,
        unit: WindowUnit::Days,
    });
    o.push_digit(0);
    assert_eq!(o.n, 1); // 0 clamps to min
}

#[test]
fn window_overlay_digits_ignored_on_unit_field() {
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 4,
        unit: WindowUnit::Days,
    });
    o.toggle_field();
    o.push_digit(9);
    assert_eq!(o.n, 4);
}

#[test]
fn window_overlay_window_returns_clamped() {
    let o = TimeWindowOverlay::new(TimeWindow {
        n: 3,
        unit: WindowUnit::Months,
    });
    assert_eq!(
        o.window(),
        TimeWindow {
            n: 3,
            unit: WindowUnit::Months
        }
    );
}

#[test]
fn window_overlay_number_wraps_at_unit_max() {
    // Hours cap is 48, so 48 -> step_up wraps to 1.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 48,
        unit: WindowUnit::Hours,
    });
    o.step_up();
    assert_eq!(o.n, 1);
    o.step_down();
    assert_eq!(o.n, 48);
}

#[test]
fn window_overlay_digit_clamps_to_unit_max() {
    // Years cap is 10: typing 4 then 0 (=40) clamps to 10.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 1,
        unit: WindowUnit::Years,
    });
    o.push_digit(4);
    o.push_digit(0);
    assert_eq!(o.n, 10);
}

#[test]
fn window_overlay_unit_change_clamps_n_down() {
    // 52 weeks, switch forward to Years (max 10): n must clamp to 10.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 52,
        unit: WindowUnit::Weeks,
    });
    o.toggle_field(); // focus unit
    o.step_down(); // Weeks -> Months (max 24): 52 clamps to 24
    assert_eq!(o.unit, WindowUnit::Months);
    assert_eq!(o.n, 24);
    o.step_down(); // Months -> Years (max 10): 24 clamps to 10
    assert_eq!(o.unit, WindowUnit::Years);
    assert_eq!(o.n, 10);
}

#[test]
fn window_overlay_cycling_through_slot_preserves_count() {
    // Slot ignores the count, so passing through it must not squash n.
    let mut o = TimeWindowOverlay::new(TimeWindow {
        n: 14,
        unit: WindowUnit::Days,
    });
    o.toggle_field(); // focus unit
    o.step_up(); // Days -> Hours (max 48): 14 fits
    assert_eq!(o.unit, WindowUnit::Hours);
    assert_eq!(o.n, 14);
    o.step_up(); // Hours -> Slot: count must be preserved, not clamped to 1
    assert_eq!(o.unit, WindowUnit::Slot);
    assert_eq!(o.n, 14);
    o.step_up(); // Slot -> Years (max 10): NOW it clamps, to 10
    assert_eq!(o.unit, WindowUnit::Years);
    assert_eq!(o.n, 10);
}

/// The row cycles the three sizes both ways, starts from what the app
/// holds, and writes the key `Config` reads back. A row whose key did not
/// match would save happily and change nothing.
#[test]
fn the_cell_size_row_cycles_and_writes_its_key() {
    let mut overlay = SettingsOverlay::new(
        60,
        60,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        true,
        Icons::Nerd,
    );
    overlay.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == SettingField::ContribCellSize)
        .unwrap();
    assert_eq!(SettingField::ContribCellSize.applies(), Applies::Live);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "medium");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "large");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "small");
    overlay.step(-1);
    assert_eq!(overlay.value_label(SettingField::ContribCellSize), "large");

    let updates = overlay.config_updates();
    let written = updates
        .iter()
        .find(|(key, _)| *key == "contributions_cell_size")
        .map(|(_, v)| v.clone());
    assert_eq!(written, Some(crate::config::SettingValue::Str("large")));
    let cfg: crate::config::Config =
        toml::from_str(r#"contributions_cell_size = "large""#).unwrap();
    assert_eq!(
        CellSize::from_config(cfg.contributions_cell_size.as_deref().unwrap()),
        CellSize::Large
    );
}

#[test]
fn the_settings_row_cycles_the_three_modes() {
    let mut overlay = SettingsOverlay::new(
        60,
        60,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Off,
        CellSize::Medium,
        true,
        Icons::Nerd,
    );
    overlay.cursor = SETTING_FIELDS
        .iter()
        .position(|f| *f == SettingField::Contributions)
        .unwrap();
    assert_eq!(overlay.value_label(SettingField::Contributions), "off");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::Contributions), "github");
    overlay.step(1);
    assert_eq!(overlay.value_label(SettingField::Contributions), "local");
    overlay.step(1);
    assert_eq!(
        overlay.value_label(SettingField::Contributions),
        "off",
        "wraps"
    );
    overlay.step(-1);
    assert_eq!(
        overlay.value_label(SettingField::Contributions),
        "local",
        "wraps back"
    );
}

#[test]
fn the_settings_row_writes_the_contributions_key() {
    assert_eq!(SettingField::Contributions.config_key(), "contributions");
    assert_eq!(SettingField::Contributions.applies(), Applies::Live);
    let overlay = SettingsOverlay::new(
        60,
        60,
        true,
        None,
        9,
        18,
        false,
        ContributionsMode::Local,
        CellSize::Medium,
        true,
        Icons::Nerd,
    );
    let updates = overlay.config_updates();
    assert!(
        updates.iter().any(|(k, v)| *k == "contributions"
            && matches!(v, crate::config::SettingValue::Str("local")))
    );
}
