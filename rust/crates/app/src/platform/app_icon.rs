//! The application icon, its Dock badge and attention requests (spec 16.2, 19.2).
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
    use objc2_app_kit::{NSApplication, NSImage, NSRequestUserAttentionType, NSWorkspace, NSWorkspaceIconCreationOptions};
    use objc2_foundation::{MainThreadMarker, NSBundle, NSString};

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
        let image = load(variant);
        let path = NSBundle::mainBundle().bundlePath();
        if !set_bundle_icon(&path, image.as_deref()) {
            eprintln!("angelly: could not update the application icon at {path}");
        }
        // A `None` image restores the icon from the bundle.
        unsafe { app.setApplicationIconImage(image.as_deref()) };
    }

    fn set_bundle_icon(path: &NSString, image: Option<&NSImage>) -> bool {
        // Unbundled development/test executables must not change their enclosing folder's icon.
        if !path.hasSuffix(&NSString::from_str(".app")) {
            return true;
        }
        let workspace = NSWorkspace::sharedWorkspace();
        // Finder, launchers and Stage Manager read the bundle icon, independently of the Dock.
        if !workspace.setIcon_forFile_options(image, path, NSWorkspaceIconCreationOptions::empty()) {
            return false;
        }
        workspace.noteFileSystemChanged_(path);
        true
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

    #[cfg(test)]
    mod tests {
        use super::*;

        fn variant_image(name: &str) -> Retained<NSImage> {
            let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../../macos/Assets.xcassets/Alternate Icons/{name}Image.imageset/macOS-AppIcon-1024px.png"));
            NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(source.to_str().unwrap())).unwrap()
        }

        fn custom_icon_flag(path: &std::path::Path) -> bool {
            use std::os::unix::ffi::OsStrExt;

            let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            let mut info = [0u8; 32];
            let size = unsafe {
                libc::getxattr(path.as_ptr(), c"com.apple.FinderInfo".as_ptr(), info.as_mut_ptr().cast(), info.len(), 0, 0)
            };
            size == info.len() as isize && info[8] & 0x04 != 0
        }

        #[test]
        fn bundle_icon_is_set_replaced_and_restored() {
            let temp = tempfile::tempdir().unwrap();
            let bundle = temp.path().join("Icon Test.app");
            std::fs::create_dir_all(bundle.join("Contents")).unwrap();
            std::fs::write(bundle.join("Contents/Info.plist"),
                r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.adetxt.angelly.icon-test</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#,
            ).unwrap();
            let path = NSString::from_str(bundle.to_str().unwrap());
            let mut previous = None;
            for name in ["Blueprint", "Chalkboard"] {
                let image = variant_image(name);
                assert!(set_bundle_icon(&path, Some(&image)));
                assert!(custom_icon_flag(&bundle));
                let resource = std::fs::read(bundle.join("Icon\r/..namedfork/rsrc")).unwrap();
                assert!(!resource.is_empty());
                assert_ne!(previous.as_ref(), Some(&resource));
                previous = Some(resource);
            }
            assert!(set_bundle_icon(&path, None));
            assert!(!custom_icon_flag(&bundle));
        }

        #[test]
        fn unbundled_executable_directory_is_untouched() {
            let temp = tempfile::tempdir().unwrap();
            let path = NSString::from_str(temp.path().to_str().unwrap());
            assert!(set_bundle_icon(&path, Some(&variant_image("Blueprint"))));
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
            assert!(!custom_icon_flag(temp.path()));
        }
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
