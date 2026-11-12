//! Icons and bitmaps the panels draw: a Lucide subset on top of the component
//! bundle, and the bundled PNGs.

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use gpui_kit::assets::icon_assets;
use gpui_kit::*;

icon_assets!(
    PanelIcons,
    [
        AppWindow,
        ArrowUpRight,
        Check,
        ChevronRight,
        ChevronsUpDown,
        CircleArrowDown,
        CircleQuestionMark,
        FileText,
        Folder,
        GitBranch,
        Info,
        Keyboard,
        MousePointer2,
        Package,
        Package2,
        Palette,
        Power,
        RefreshCw,
        Settings,
        SlidersHorizontal,
        SquareStack,
        TriangleAlert,
        Type,
    ]
);

/// The application's asset source; register with `Application::with_assets`.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = PanelIcons.load(path)? {
            return Ok(Some(bytes));
        }
        crate::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = crate::assets::Assets.list(path)?;
        paths.extend(PanelIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

macro_rules! bitmap {
    ($name:ident, $path:literal) => {
        pub fn $name() -> Arc<Image> {
            static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
            IMAGE
                .get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!($path).to_vec())))
                .clone()
        }
    };
}

bitmap!(codex_black, "../../assets/quota/codex-black.png");
bitmap!(codex_white, "../../assets/quota/codex-white.png");
bitmap!(claude_color, "../../assets/quota/claude-color.png");

/// Variants of the application icon, in the order the About window cycles
/// through them.
pub const ICON_VARIANTS: &[&str] = &[
    "official",
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

/// The bundled bitmap for a `macos-icon` value, if it is one of the built-in
/// variants.
pub fn app_icon(variant: &str) -> Option<Arc<Image>> {
    macro_rules! icons {
        ($($name:literal),*) => {
            match variant {
                $($name => {
                    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
                    IMAGE
                        .get_or_init(|| {
                            Arc::new(Image::from_bytes(
                                ImageFormat::Png,
                                include_bytes!(concat!("../../assets/icons/", $name, ".png")).to_vec(),
                            ))
                        })
                        .clone()
                })*
                _ => return None,
            }
        };
    }
    Some(icons!(
        "official",
        "blueprint",
        "chalkboard",
        "egyptian",
        "glass",
        "holographic",
        "microchip",
        "paper",
        "retro",
        "xray",
        "attack-on-titan"
    ))
}
