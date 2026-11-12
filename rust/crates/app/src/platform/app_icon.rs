//! The Dock icon, its badge and attention requests (spec 16.2, 19.2).
//!
//! `macos-icon` selects one of the bundled variants, or a user image for `custom`.
//! The variant images ship as `Resources/icons/<name>.png`.

use gpui_kit::App;

use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Variant {
    Official,
    Named(&'static str),
    Custom(std::path::PathBuf),
}

const NAMED: &[&str] = &[
    "blueprint",
    "chalkboard",
    "egyptian",
    "glass",
    "holographic",
    "microchip",
    "paper",
    "retro",
    "xray",
    "attack-on-titan",
];

pub fn variant_for(config: &angelly_core::config::Config) -> Variant {
    let value = config.get("macos-icon").unwrap_or("official").trim();
    if let Some(name) = NAMED.iter().find(|name| **name == value) {
        return Variant::Named(name);
    }
    match value {
        "custom" => match config.get("macos-custom-icon") {
            Some(path) if !path.is_empty() => {
                let env = angelly_core::config::Env::from_process();
                Variant::Custom(env.expand_home(path))
            }
            _ => Variant::Official,
        },
        _ => Variant::Official,
    }
}

/// Applies the icon chosen by the configuration. Safe to call again after a reload.
pub fn apply(cx: &mut App) {
    let variant = variant_for(&cx.global::<Theme>().config);
    imp::set_icon(&variant);
}

/// The number shown on the Dock icon; zero clears it. Counts above 99 show "99+".
pub fn set_badge(count: usize) {
    imp::set_badge(&badge_label(count));
}

pub fn badge_label(count: usize) -> String {
    match count {
        0 => String::new(),
        1..=99 => count.to_string(),
        _ => "99+".to_owned(),
    }
}

/// Bounces the Dock icon once when the app is in the background.
pub fn request_attention() {
    imp::request_attention();
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_app_kit::{NSApplication, NSImage, NSRequestUserAttentionType};
    use objc2_foundation::{MainThreadMarker, NSString};

    use super::Variant;
    use crate::platform::resources;

    fn load(variant: &Variant) -> Option<Retained<NSImage>> {
        let path = match variant {
            Variant::Official => return None,
            Variant::Named(name) => resources::path(&format!("icons/{name}.png"))?,
            Variant::Custom(path) => path.clone(),
        };
        let path = NSString::from_str(path.to_str()?);
        NSImage::initWithContentsOfFile(NSImage::alloc(), &path)
    }

    pub fn set_icon(variant: &Variant) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        // A `None` image restores the icon from the bundle.
        unsafe { app.setApplicationIconImage(load(variant).as_deref()) };
    }

    pub fn set_badge(label: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let tile = NSApplication::sharedApplication(mtm).dockTile();
        let label = (!label.is_empty()).then(|| NSString::from_str(label));
        tile.setBadgeLabel(label.as_deref());
    }

    pub fn request_attention() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        app.requestUserAttention(NSRequestUserAttentionType::InformationalRequest);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Variant;

    pub fn set_icon(_: &Variant) {}

    pub fn set_badge(_: &str) {}

    pub fn request_attention() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badges_cap_at_ninety_nine() {
        assert_eq!(badge_label(0), "");
        assert_eq!(badge_label(7), "7");
        assert_eq!(badge_label(99), "99");
        assert_eq!(badge_label(100), "99+");
    }
}
