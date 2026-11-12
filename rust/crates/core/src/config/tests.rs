use super::*;
use std::fs;

fn env(platform: Platform, dir: &Path) -> Env {
    Env {
        platform,
        home: Some(dir.join("home")),
        xdg_config_home: Some(dir.join("xdg")),
        resources_dir: None,
    }
}

fn mac(dir: &Path) -> Env {
    env(Platform::MacOs, dir)
}

fn parse(text: &str) -> Config {
    let dir = tempfile::tempdir().unwrap();
    Config::from_str(text, &mac(dir.path()), ColorScheme::Dark)
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn messages(config: &Config) -> Vec<String> {
    config.diagnostics().iter().map(|d| d.message.clone()).collect()
}

#[test]
fn line_parsing() {
    assert_eq!(parse_line("# comment"), None);
    assert_eq!(parse_line("   "), None);
    assert_eq!(parse_line("font-size = 14"), Some(("font-size".into(), Some("14".into()))));
    assert_eq!(parse_line("  title =  \"hello world\"  \r"), Some(("title".into(), Some("hello world".into()))));
    assert_eq!(parse_line("title = \"unterminated"), Some(("title".into(), Some("\"unterminated".into()))));
    assert_eq!(parse_line("title = \""), Some(("title".into(), Some("\"".into()))));
    assert_eq!(parse_line("font-thicken"), Some(("font-thicken".into(), None)));
    assert_eq!(parse_line("font-family ="), Some(("font-family".into(), Some(String::new()))));
    assert_eq!(parse_line("keybind = a=b=c"), Some(("keybind".into(), Some("a=b=c".into()))));
}

#[test]
fn comments_only_count_at_line_start() {
    let c = parse("title = a # b\n# title = c\n");
    assert_eq!(c.title(), Some("a # b"));
}

#[test]
fn last_value_wins_and_empty_resets() {
    let c = parse("font-size = 10\nfont-size = 20\n");
    assert_eq!(c.font_size(), 20.0);
    let c = parse("font-size = 10\nfont-size =\n");
    assert_eq!(c.font_size(), 13.0);
}

#[test]
fn repeatable_keys_accumulate_and_reset() {
    let c = parse("font-family = A\nfont-family = B\n");
    assert_eq!(c.font_family(), vec!["A", "B"]);
    let c = parse("font-family = A\nfont-family =\nfont-family = C\n");
    assert_eq!(c.font_family(), vec!["C"]);
}

#[test]
fn bare_boolean_keys_mean_true() {
    assert!(parse("font-thicken").font_thicken());
    assert!(!parse("font-thicken = false").font_thicken());
    assert!(!parse("font-thicken = true\nfont-thicken =").font_thicken());
}

#[test]
fn invalid_values_are_diagnosed_and_ignored() {
    let c = parse("font-size = 12\nfont-size = big\ncursor-style = wavy\nbackground = nope\nunknown-key = 1\n");
    assert_eq!(c.font_size(), 12.0);
    assert_eq!(c.cursor_style(), CursorStyle::Block);
    assert_eq!(
        messages(&c),
        vec![
            "font-size: invalid value",
            "cursor-style: invalid value",
            "background: invalid value",
            "unknown-key: unknown field",
        ]
    );
}

#[test]
fn diagnostics_carry_file_locations() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let path = dir.path().join("config");
    write(&path, "# header\n\nfont-size = nope\n");
    let c = Config::load_files(&env, std::slice::from_ref(&path), ColorScheme::Dark);
    assert_eq!(c.diagnostics()[0].location, Some(Location { path, line: 3 }));
}

#[test]
fn renamed_keys_are_rewritten() {
    let c = parse("scrollback-limit = 1000\nbackground-blur-radius = 5\ncopy-on-select = true\n");
    assert_eq!(c.scrollback_limit_bytes(), 1000);
    assert_eq!(c.background_blur(), BackgroundBlur::Radius(5));
    assert_eq!(c.copy_on_select(), CopyOnSelect::Clipboard);
    assert!(c.diagnostics().is_empty());

    let dir = tempfile::tempdir().unwrap();
    let linux = Config::from_str("copy-on-select = true", &env(Platform::Linux, dir.path()), ColorScheme::Dark);
    assert_eq!(linux.copy_on_select(), CopyOnSelect::Primary);
    assert_eq!(parse("copy-on-select = false").copy_on_select(), CopyOnSelect::None);
}

#[test]
fn legacy_inverted_color_switches_expand() {
    let c = parse("cursor-invert-fg-bg = true\nselection-invert-fg-bg\nbold-is-bright");
    assert_eq!(c.cursor_color(), Some(TerminalColor::CellForeground));
    assert_eq!(c.cursor_text(), Some(TerminalColor::CellBackground));
    assert_eq!(c.selection_foreground(), Some(TerminalColor::CellBackground));
    assert_eq!(c.selection_background(), Some(TerminalColor::CellForeground));
    assert_eq!(c.get("bold-color"), Some("bright"));
}

#[test]
fn defaults_match_ghostty_per_platform() {
    let dir = tempfile::tempdir().unwrap();
    let mac = Config::from_str("", &self::mac(dir.path()), ColorScheme::Dark);
    let linux = Config::from_str("", &env(Platform::Linux, dir.path()), ColorScheme::Dark);
    assert_eq!(mac.font_size(), 13.0);
    assert_eq!(linux.font_size(), 12.0);
    assert_eq!(mac.copy_on_select(), CopyOnSelect::None);
    assert_eq!(linux.copy_on_select(), CopyOnSelect::Primary);
    assert!(!mac.quit_after_last_window_closed());
    assert!(linux.quit_after_last_window_closed());
    assert_eq!(mac.scrollback_limit_bytes(), 50_000_000);
    assert_eq!(mac.window_padding_x(), WindowPadding { top_left: 2, bottom_right: 2 });
    assert_eq!(mac.macos_titlebar_style(), MacTitlebarStyle::Transparent);
    assert_eq!(mac.confirm_close_surface(), ConfirmCloseSurface::True);
    assert_eq!(mac.clipboard_read(), ClipboardAccess::Ask);
    assert_eq!(mac.shell_integration(), ShellIntegration::Detect);
    assert_eq!(mac.auto_update(), Some(AutoUpdate::Check));
    assert_eq!(mac.undo_timeout(), Duration::from_secs(5));
    assert_eq!(mac.term(), "xterm-ghostty");
    assert_eq!(mac.macos_option_as_alt(), None);
    assert_eq!(mac.background_opacity(), 1.0);
    assert_eq!(mac.unfocused_split_opacity(), 0.7);
}

#[test]
fn clamps_follow_ghostty() {
    let c = parse("background-opacity = 3\nunfocused-split-opacity = 0\nminimum-contrast = 99\nwindow-width = 3\nwindow-height = 1");
    assert_eq!(c.background_opacity(), 1.0);
    assert_eq!(c.unfocused_split_opacity(), 0.15);
    assert_eq!(c.minimum_contrast(), 21.0);
    assert_eq!(c.window_width(), 10);
    assert_eq!(c.window_height(), 4);
    assert_eq!(parse("window-width = 0").window_width(), 0);
}

#[test]
fn typed_accessors() {
    let c = parse(
        "scrollback-limit-bytes = unlimited\nwindow-padding-x = 4,6\nmacos-option-as-alt = left\n\
         window-decoration = false\nauto-update = off\nundo-timeout = 1m 30s\nbackground-blur = macos-glass-clear\n\
         env = A=1\nenv = B=2\nenv = A=3\nenv = B\ncursor-style = bar\ncursor-style-blink = false\n",
    );
    assert_eq!(c.scrollback_limit_bytes(), usize::MAX);
    assert_eq!(c.window_padding_x(), WindowPadding { top_left: 4, bottom_right: 6 });
    assert_eq!(c.macos_option_as_alt(), Some(OptionAsAlt::Left));
    assert_eq!(c.window_decoration(), WindowDecoration::None);
    assert_eq!(c.auto_update(), Some(AutoUpdate::Off));
    assert_eq!(c.undo_timeout(), Duration::from_secs(90));
    assert_eq!(c.background_blur(), BackgroundBlur::MacosGlassClear);
    assert_eq!(c.env(), vec![("A".to_string(), "3".to_string())]);
    assert_eq!(c.cursor_style(), CursorStyle::Bar);
    assert_eq!(c.cursor_style_blink(), Some(false));
}

#[test]
fn palette_entries_layer_over_the_default() {
    let c = parse("palette = 1=#112233\npalette = 0x10=#445566\n");
    assert_eq!(c.palette()[1], Rgb { r: 0x11, g: 0x22, b: 0x33 });
    assert_eq!(c.palette()[16], Rgb { r: 0x44, g: 0x55, b: 0x66 });
    let reset = parse("palette = 1=#112233\npalette =\n");
    assert_eq!(reset.palette()[1], default_palette()[1]);
    assert!(!messages(&parse("palette = 300=#112233")).is_empty());
}

#[test]
fn keybinds_apply_in_order_over_defaults() {
    let c = parse("keybind = cmd+shift+x=new_tab\nkeybind = bogus-key=new_tab\n");
    assert!(c.keybinds().trigger_for_action("new_tab").is_some());
    assert_eq!(c.diagnostics().len(), 1);
    assert!(c.diagnostics()[0].message.starts_with("keybind:"));

    let cleared = parse("keybind = clear\n");
    assert!(cleared.keybinds().set.is_empty());
    let restored = parse("keybind = clear\nkeybind =\n");
    assert_eq!(restored.keybinds().set.len(), Keybinds::defaults(true).set.len());
}

#[test]
fn default_theme_is_angelly_dark_for_both_schemes() {
    let dir = tempfile::tempdir().unwrap();
    for scheme in [ColorScheme::Light, ColorScheme::Dark] {
        let c = Config::from_str("", &mac(dir.path()), scheme);
        let theme = c.theme().unwrap();
        assert_eq!(theme.name, "Angelly Dark");
        assert_eq!(theme.source, ThemeSource::Embedded);
        assert_eq!(c.background(), Rgb::parse("#211922").unwrap());
        assert_eq!(c.foreground(), Rgb::parse("#f0e2eb").unwrap());
        assert_eq!(c.palette()[1], Rgb::parse("#d66b91").unwrap());
        assert_eq!(c.cursor_color(), Some(TerminalColor::Color(Rgb::parse("#f09abb").unwrap())));
    }
}

#[test]
fn light_dark_theme_pair_picks_by_scheme() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let text = "theme = light:Angelly Light,dark:Angelly Dark";
    let light = Config::from_str(text, &env, ColorScheme::Light);
    let dark = Config::from_str(text, &env, ColorScheme::Dark);
    assert_eq!(light.theme().unwrap().name, "Angelly Light");
    assert_eq!(dark.theme().unwrap().name, "Angelly Dark");
    assert_ne!(light.background(), dark.background());
}

#[test]
fn user_values_override_theme_values() {
    let c = parse("theme = Angelly Dark\nbackground = #000000\npalette = 1=#ffffff\n");
    assert_eq!(c.background(), Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(c.palette()[1], Rgb { r: 255, g: 255, b: 255 });
    assert_eq!(c.palette()[2], Rgb::parse("#83b394").unwrap());
    assert_eq!(c.foreground(), Rgb::parse("#f0e2eb").unwrap());
}

#[test]
fn empty_theme_value_disables_the_theme() {
    let c = parse("theme =");
    assert!(c.theme().is_none());
    assert_eq!(c.background(), Rgb { r: 0x28, g: 0x2C, b: 0x34 });
}

#[test]
fn user_theme_files_shadow_embedded_themes_and_load_from_the_themes_dir() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let themes = env.user_themes_dir().unwrap();
    write(&themes.join("Mine"), "background = #010203\nforeground = #040506\ntheme = ignored\n");
    write(&themes.join("Angelly Dark"), "background = #0a0b0c\n");

    let mine = Config::from_str("theme = Mine", &env, ColorScheme::Dark);
    assert_eq!(mine.background(), Rgb { r: 1, g: 2, b: 3 });
    assert_eq!(mine.theme().unwrap().source, ThemeSource::File(themes.join("Mine")));
    assert!(mine.get("theme").is_some_and(|t| t == "Mine"));

    let shadowed = Config::from_str("", &env, ColorScheme::Dark);
    assert_eq!(shadowed.background(), Rgb { r: 10, g: 11, b: 12 });
}

#[test]
fn resource_themes_are_searched_after_user_themes() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = mac(dir.path());
    env.resources_dir = Some(dir.path().join("resources"));
    write(&dir.path().join("resources/themes/Res"), "background = #111111\n");
    write(&env.user_themes_dir().unwrap().join("Both"), "background = #222222\n");
    write(&dir.path().join("resources/themes/Both"), "background = #333333\n");
    assert_eq!(Config::from_str("theme = Res", &env, ColorScheme::Dark).background(), Rgb { r: 0x11, g: 0x11, b: 0x11 });
    assert_eq!(Config::from_str("theme = Both", &env, ColorScheme::Dark).background(), Rgb { r: 0x22, g: 0x22, b: 0x22 });
}

#[test]
fn absolute_theme_paths_load_and_relative_paths_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let theme = dir.path().join("custom-theme");
    write(&theme, "background = #abcdef\n");
    let c = Config::from_str(&format!("theme = {}", theme.display()), &env, ColorScheme::Dark);
    assert_eq!(c.background(), Rgb { r: 0xab, g: 0xcd, b: 0xef });

    let c = Config::from_str("theme = sub/dir", &env, ColorScheme::Dark);
    assert!(c.theme().is_none());
    assert_eq!(messages(&c), vec!["theme \"sub/dir\" cannot include path separators unless it is an absolute path"]);

    let c = Config::from_str("theme = /nope/missing", &env, ColorScheme::Dark);
    assert_eq!(messages(&c), vec!["failed to load theme from the path \"/nope/missing\""]);
}

#[test]
fn missing_theme_reports_every_searched_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = mac(dir.path());
    env.resources_dir = Some(dir.path().join("res"));
    let c = Config::from_str("theme = Nope", &env, ColorScheme::Dark);
    assert!(c.theme().is_none());
    assert_eq!(c.diagnostics().len(), 2);
    assert!(c.diagnostics()[0].message.contains("theme \"Nope\" not found, tried path"));
}

#[test]
fn theme_pair_without_both_halves_is_invalid() {
    let c = parse("theme = light:Only");
    assert_eq!(messages(&c), vec!["theme: invalid value"]);
    assert_eq!(c.theme().unwrap().name, "Angelly Dark");
}

#[test]
fn default_files_load_in_order_with_app_support_last() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let xdg = env.xdg_ghostty_dir().unwrap();
    let support = env.app_support_dir().unwrap();
    write(&xdg.join("config"), "font-size = 10\ntitle = legacy\n");
    write(&xdg.join("config.ghostty"), "font-size = 11\n");
    write(&support.join("config.ghostty"), "font-size = 15\n");
    let c = Config::load(&env, ColorScheme::Dark);
    assert_eq!(c.font_size(), 15.0);
    assert_eq!(c.title(), Some("legacy"));

    let linux = Config::load(&Env { platform: Platform::Linux, ..env.clone() }, ColorScheme::Dark);
    assert_eq!(linux.font_size(), 11.0);
}

#[test]
fn missing_default_files_are_not_errors() {
    let dir = tempfile::tempdir().unwrap();
    let c = Config::load(&mac(dir.path()), ColorScheme::Dark);
    assert!(c.diagnostics().is_empty());
}

#[test]
fn includes_resolve_relative_to_the_declaring_file_and_apply_after_the_main_files() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let main = dir.path().join("main/config");
    write(&main, "config-file = extra/a\nfont-size = 10\ntitle = main\n");
    write(&dir.path().join("main/extra/a"), "font-size = 20\nconfig-file = b\n");
    write(&dir.path().join("main/extra/b"), "title = from-b\n");
    let c = Config::load_files(&env, &[main], ColorScheme::Dark);
    assert_eq!(c.font_size(), 20.0);
    assert_eq!(c.title(), Some("from-b"));
    assert!(c.diagnostics().is_empty(), "{:?}", c.diagnostics());
}

#[test]
fn includes_load_breadth_first_in_declaration_order() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let main = dir.path().join("config");
    write(&main, "config-file = a\nconfig-file = b\n");
    write(&dir.path().join("a"), "config-file = c\ntitle = a\n");
    write(&dir.path().join("b"), "title = b\n");
    write(&dir.path().join("c"), "title = c\n");
    let c = Config::load_files(&env, &[main], ColorScheme::Dark);
    assert_eq!(c.title(), Some("c"));
    let order: Vec<_> = c.get_all("title");
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[test]
fn optional_and_missing_includes() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let main = dir.path().join("config");
    write(&main, "config-file = ?missing-optional\nconfig-file = \"?quoted-optional\"\nconfig-file = missing-required\n");
    let c = Config::load_files(&env, &[main], ColorScheme::Dark);
    let msgs = messages(&c);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].starts_with("error opening config-file"));
    assert!(msgs[0].contains("missing-required"));
}

#[test]
fn include_cycles_are_reported_once_and_do_not_loop() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let main = dir.path().join("config");
    write(&main, "config-file = a\n");
    write(&dir.path().join("a"), "config-file = b\ntitle = a\n");
    write(&dir.path().join("b"), "config-file = a\ntitle = b\n");
    let c = Config::load_files(&env, &[main], ColorScheme::Dark);
    assert_eq!(c.title(), Some("b"));
    let cycles: Vec<_> = messages(&c).into_iter().filter(|m| m.contains("cycle detected")).collect();
    assert_eq!(cycles.len(), 1);
}

#[test]
fn home_relative_includes_expand() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    write(&dir.path().join("home/extra"), "title = home\n");
    let c = Config::from_str("config-file = ~/extra", &env, ColorScheme::Dark);
    assert_eq!(c.title(), Some("home"));
}

#[test]
fn relative_include_without_a_base_is_reported() {
    let c = parse("config-file = relative");
    assert_eq!(c.diagnostics().len(), 1);
    assert!(c.diagnostics()[0].message.contains("relative path"));
}

#[test]
fn include_of_a_directory_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let env = mac(dir.path());
    let c = Config::from_str(&format!("config-file = {}", dir.path().display()), &env, ColorScheme::Dark);
    assert_eq!(c.diagnostics().len(), 1);
    assert!(c.diagnostics()[0].message.contains("not a regular file"));
}

#[test]
fn embedded_themes_match_the_bundled_resources() {
    for name in ["angelly-dark", "angelly-light"] {
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources/themes").join(match name {
            "angelly-dark" => "Angelly Dark",
            _ => "Angelly Light",
        });
        if bundled.exists() {
            let embedded = if name == "angelly-dark" { ANGELLY_DARK } else { ANGELLY_LIGHT };
            assert_eq!(fs::read_to_string(bundled).unwrap(), embedded);
        }
    }
}

#[test]
fn every_known_key_is_unique_and_sorted() {
    let keys = known_keys::KNOWN_KEYS;
    assert!(keys.windows(2).all(|w| w[0] < w[1]));
}
