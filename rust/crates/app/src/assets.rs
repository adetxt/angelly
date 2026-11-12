//! Application assets: everything under `assets/` plus the bundled Lucide icons.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
struct Embedded;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Embedded::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths: Vec<SharedString> =
            Embedded::iter().filter(|name| name.starts_with(path)).map(|name| name.to_string().into()).collect();
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(paths)
    }
}
