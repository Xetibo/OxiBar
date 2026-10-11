use std::sync::Arc;

use oxibar::{
    messages::Message,
    settings::Action,
    test_support::{FakePluginOptions, TestHost},
};
use oxiced::theme::snapshot::{ThemeSnapshot, parse_color};
use toml::Value;

fn set(host: &mut TestHost, path: &str, value: Value) {
    host.drive(Message::Settings(Action::Set(path.into(), value)));
}

#[test]
fn live_snapshots_are_shared_immutable_and_legacy_plugins_keep_their_fallback() {
    let upgraded = FakePluginOptions {
        live_theme: true,
        ..FakePluginOptions::default()
    };
    let mut host = TestHost::with_plugins([
        ("Clock", upgraded),
        ("Audio", upgraded),
        ("Legacy", FakePluginOptions::default()),
    ]);
    let original = host.theme_snapshot();
    set(
        &mut host,
        "theme.palette.primary",
        Value::String("#12345680".into()),
    );
    let changed = host.theme_snapshot();
    assert_eq!(changed.tokens.primary, parse_color("12345680").unwrap());
    assert_eq!(original.tokens, ThemeSnapshot::legacy().tokens);
    assert!(Arc::ptr_eq(&changed, &host.plugin_theme("Clock").unwrap()));
    assert!(Arc::ptr_eq(&changed, &host.plugin_theme("Audio").unwrap()));
    assert!(host.plugin_theme("Legacy").is_none());
    set(&mut host, "theme.reduced_motion", Value::Boolean(true));
    set(&mut host, "theme.density", Value::Float(1.2));
    assert_eq!(host.theme_snapshot().duration(), std::time::Duration::ZERO);
    assert!(
        (host.theme_snapshot().tokens.padding_lg - original.tokens.padding_lg * 1.2).abs() < 0.001
    );
    assert!(host.restart_fields().is_empty());
    let valid = host.theme_snapshot();
    set(
        &mut host,
        "theme.palette.primary",
        Value::String("bad color".into()),
    );
    assert!(Arc::ptr_eq(&valid, &host.theme_snapshot()));
    assert!(host.settings_error().is_some());
    host.drive(Message::Settings(Action::Reset(
        "theme.palette.primary".into(),
    )));
    assert_eq!(
        host.theme_snapshot().tokens.primary,
        original.tokens.primary
    );
}

#[test]
fn stale_font_completion_cannot_overwrite_newer_family_palette_or_startup_font() {
    let mut host = TestHost::with_plugins([(
        "Clock",
        FakePluginOptions {
            live_theme: true,
            ..FakePluginOptions::default()
        },
    )]);
    let startup = host.theme_snapshot().font_family.clone();
    set(
        &mut host,
        "bar.font",
        Value::String("Restart-only font".into()),
    );
    set(&mut host, "theme.font_scale", Value::Float(1.1));
    assert_eq!(host.theme_snapshot().font_family, startup);
    assert_eq!(host.restart_fields(), ["bar.font"]);
    set(
        &mut host,
        "theme.font",
        Value::String("First family".into()),
    );
    set(
        &mut host,
        "theme.font",
        Value::String("Second family".into()),
    );
    set(
        &mut host,
        "theme.palette.text",
        Value::String("abcdef".into()),
    );
    host.drive(Message::ThemeFontReady(
        "First family".into(),
        Ok("Old canonical".into()),
    ));
    assert_eq!(host.theme_snapshot().font_family, "Second family");
    host.drive(Message::ThemeFontReady(
        "Second family".into(),
        Ok("New canonical".into()),
    ));
    assert_eq!(host.theme_snapshot().font_family, "New canonical");
    assert_eq!(
        host.theme_snapshot().tokens.text,
        parse_color("abcdef").unwrap()
    );
    assert!(Arc::ptr_eq(
        &host.theme_snapshot(),
        &host.plugin_theme("Clock").unwrap()
    ));
}
