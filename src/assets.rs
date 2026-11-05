//! Asset source: gpui-component's default icons plus the extra Lucide icons
//! this app uses. Without a registered source, icons (the password eye,
//! button spinners, sidebar glyphs) render blank.

use std::borrow::Cow;

use gpui_kit::assets::Assets;
use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    pub ExtraIcons,
    [
        AtSign,
        Bookmark,
        CornerUpRight,
        Download,
        FileImage,
        FileMusic,
        FileVideoCamera,
        Hash,
        House,
        Image,
        Users,
        Volume2,
        WifiOff
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::AppAssets;
    use gpui_kit::AssetSource;
    use gpui_kit::assets::IconName;

    #[test]
    fn extra_and_default_icons_both_load() {
        for path in [
            IconName::Hash.path(),
            IconName::Volume2.path(),
            IconName::FileImage.path(),
            IconName::Download.path(),
        ] {
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
        // A default component icon (used by the button spinner).
        assert!(AppAssets.load("icons/loader.svg").unwrap().is_some());
    }
}
