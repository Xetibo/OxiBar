use super::{
    snapshot::{ThemeSnapshot, parse_color},
    theme_impl::theme_from_toml,
};
use toml::Table;

#[test]
fn base24_bright_slots_accent_and_explicit_roles_have_consistent_precedence() {
    let config: Table = toml::from_str("[theme]\naccent='#bb88dd'\n[theme.base24]\nbase00='101820'\nbase10='172630'\nbase12='ff2233'\nbase14='55dd66'\nbase16='7799ff'\n[theme.palette]\ngood='33aa44'").unwrap();
    let snapshot = ThemeSnapshot::from_config(&config, 9).unwrap();
    assert_eq!(snapshot.tokens.mantle, parse_color("101820").unwrap());
    assert_eq!(snapshot.tokens.primary_bg, parse_color("172630").unwrap());
    assert_eq!(snapshot.tokens.bad_hover, parse_color("ff2233").unwrap());
    assert_ne!(snapshot.tokens.good_hover, parse_color("55dd66").unwrap());
    assert_eq!(snapshot.tokens.primary, parse_color("bb88dd").unwrap());
    assert_ne!(
        snapshot.tokens.primary_hover,
        parse_color("7799ff").unwrap()
    );
    assert_eq!(snapshot.base24.colors[0x0d], snapshot.tokens.primary);
    assert!(
        ThemeSnapshot::from_config(
            &toml::from_str("[theme.base24]\nbase18='123456'").unwrap(),
            0
        )
        .is_err()
    );
    assert!(
        ThemeSnapshot::from_config(
            &toml::from_str("[theme.base24]\nbase0D='123456'\nbase0d='ffffff'").unwrap(),
            0
        )
        .is_err()
    );
}

#[test]
fn standalone_themes_accept_flat_and_nested_base24_and_preserve_legacy_defaults() {
    let legacy = theme_from_toml("").unwrap();
    let flat =
        theme_from_toml("base00='102030'\nbase10='111927'\nbase12='ff4444'\naccent='aa88cc'")
            .unwrap();
    let nested = theme_from_toml(
        "accent='aa88cc'\n[base24]\nbase00='102030'\nbase10='111927'\nbase12='ff4444'",
    )
    .unwrap();
    assert_eq!(flat.tokens, nested.tokens);
    assert_eq!(flat.tokens.bad_hover, parse_color("ff4444").unwrap());
    assert_eq!(flat.tokens.primary, parse_color("aa88cc").unwrap());
    assert_eq!(
        theme_from_toml("mantle='102030'").unwrap().tokens.mantle,
        parse_color("102030").unwrap()
    );
    assert_eq!(legacy.tokens.primary, parse_color("89b4fa").unwrap());
    assert!(theme_from_toml("[base24]\nbase17='invalid'").is_err());
}

#[test]
fn catppuccin_dark_and_light_schemes_have_readable_semantic_text_in_all_states() {
    let schemes = [
        "[theme.base24]\nbase00='1e1e2e'\nbase01='181825'\nbase02='313244'\nbase03='45475a'\nbase04='585b70'\nbase05='cdd6f4'\nbase06='f5e0dc'\nbase07='b4befe'\nbase08='f38ba8'\nbase09='fab387'\nbase0a='f9e2af'\nbase0b='a6e3a1'\nbase0c='94e2d5'\nbase0d='89b4fa'\nbase0e='cba6f7'\nbase0f='f2cdcd'\nbase10='181825'\nbase11='11111b'\nbase12='eba0ac'\nbase13='f5e0dc'\nbase14='a6e3a1'\nbase15='89dceb'\nbase16='74c7ec'\nbase17='f5c2e7'",
        "[theme.base24]\nbase00='eff1f5'\nbase01='e6e9ef'\nbase02='ccd0da'\nbase03='bcc0cc'\nbase04='acb0be'\nbase05='4c4f69'\nbase06='dc8a78'\nbase07='7287fd'\nbase08='d20f39'\nbase09='fe640b'\nbase0a='df8e1d'\nbase0b='40a02b'\nbase0c='179299'\nbase0d='1e66f5'\nbase0e='8839ef'\nbase0f='dd7878'",
    ];
    for t in schemes
        .into_iter()
        .map(|text| {
            ThemeSnapshot::from_config(&toml::from_str::<Table>(text).unwrap(), 0)
                .unwrap()
                .tokens
        })
        .chain([theme_from_toml("").unwrap().tokens])
    {
        for background in [
            t.mantle,
            t.mantle_hover,
            t.mantle_active,
            t.primary_bg,
            t.primary_bg_hover,
            t.primary_bg_active,
            t.secondary_bg,
            t.secondary_bg_hover,
            t.secondary_bg_active,
            t.tertiary_bg,
            t.tertiary_bg_hover,
            t.tertiary_bg_active,
        ] {
            assert!(
                super::contrast::ratio(t.text, background) >= 4.5,
                "text {:?} on {:?}",
                t.text,
                background
            );
            assert!(
                super::contrast::ratio(t.text_muted, background) >= 4.5,
                "muted {:?} on {:?}",
                t.text_muted,
                background
            );
        }
        for (text, backgrounds) in [
            (
                t.primary_contrast,
                [t.primary, t.primary_hover, t.primary_active],
            ),
            (
                t.secondary_contrast,
                [t.secondary, t.secondary_hover, t.secondary_active],
            ),
            (t.good_contrast, [t.good, t.good_hover, t.good_active]),
            (t.bad_contrast, [t.bad, t.bad_hover, t.bad_active]),
            (t.info_contrast, [t.info, t.info_hover, t.info_active]),
            (
                t.warning_contrast,
                [t.warning, t.warning_hover, t.warning_active],
            ),
        ] {
            for background in backgrounds {
                assert!(
                    super::contrast::ratio(text, background) >= 4.5,
                    "filled text {:?} on {:?}",
                    text,
                    background
                );
            }
        }
        assert_ne!(t.tertiary_bg_hover, parse_color("fab387").unwrap());
    }
}

#[test]
fn base16_input_derives_extensions_and_explicit_semantic_roles_win() {
    let config: Table = toml::from_str("[theme.base24]\nbase00='202122'\nbase01='28292a'\nbase08='ab2345'\nbase0d='56abcd'\n[theme.palette]\ntext_muted='585b70'\nprimary_contrast='ffffff'").unwrap();
    let snapshot = ThemeSnapshot::from_config(&config, 0).unwrap();
    for (slot, source) in [(0x10, 0x01), (0x11, 0x00), (0x12, 0x08), (0x16, 0x0d)] {
        assert_eq!(snapshot.base24.colors[slot], snapshot.base24.colors[source]);
    }
    assert_eq!(snapshot.tokens.text_muted, parse_color("585b70").unwrap());
    assert_eq!(
        snapshot.tokens.primary_contrast,
        parse_color("ffffff").unwrap()
    );
}
