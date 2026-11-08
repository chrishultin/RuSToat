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
        Clock,
        CornerUpRight,
        Crown,
        Download,
        FaceSlightlySmiling,
        FaceSlightlySmilingPlus,
        FileImage,
        FileMusic,
        FileVideoCamera,
        FolderMinus,
        FolderPlus,
        Hash,
        House,
        IdCard,
        Image,
        KeyRound,
        Keyboard,
        Link,
        LogOut,
        Mail,
        MessageCircle,
        Monitor,
        MonitorSmartphone,
        Paperclip,
        Pencil,
        Pin,
        PinOff,
        RefreshCw,
        Reply,
        SendHorizontal,
        ShieldCheck,
        Smartphone,
        TextQuote,
        Trash,
        UserCheck,
        UserMinus,
        UserPen,
        UserPlus,
        UserX,
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

    /// Every icon the app draws, so none renders blank. The person and
    /// message menus, profile card, Friends page and category headings use
    /// these.
    #[test]
    fn people_icons_load() {
        for icon in [
            IconName::Ban,
            IconName::ChevronDown,
            IconName::ChevronRight,
            IconName::CircleUser,
            IconName::Copy,
            IconName::Crown,
            IconName::Ellipsis,
            IconName::IdCard,
            IconName::Link,
            IconName::Mail,
            IconName::MessageCircle,
            IconName::ShieldCheck,
            IconName::TextQuote,
            IconName::UserCheck,
            IconName::UserMinus,
            IconName::UserPlus,
            IconName::UserX,
            IconName::Users,
        ] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
    }

    /// The blocked-messages row and the age gate.
    #[test]
    fn safety_icons_load() {
        for icon in [IconName::UserX, IconName::TriangleAlert] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
    }

    /// The rail's folders and the folder items in its menus.
    #[test]
    fn folder_icons_load() {
        for icon in [
            IconName::Folder,
            IconName::FolderOpen,
            IconName::FolderPlus,
            IconName::FolderMinus,
            IconName::Pencil,
            IconName::Trash,
        ] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
    }

    /// The slowmode badge by the message box.
    #[test]
    fn slowmode_icon_loads() {
        let path = IconName::Clock.path();
        assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
    }

    /// The status menu on the user's avatar.
    #[test]
    fn status_icons_load() {
        for icon in [
            IconName::FaceSlightlySmilingPlus,
            IconName::Pencil,
            IconName::Close,
            IconName::UserPen,
        ] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
    }

    /// The Sessions and Account pages.
    #[test]
    fn security_icons_load() {
        for icon in [
            IconName::Copy,
            IconName::KeyRound,
            IconName::LogOut,
            IconName::Monitor,
            IconName::MonitorSmartphone,
            IconName::RefreshCw,
            IconName::ShieldCheck,
            IconName::Smartphone,
        ] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
    }

    /// The settings window's sidebar icons.
    #[test]
    fn settings_icons_load() {
        for icon in [
            IconName::Bell,
            IconName::Keyboard,
            IconName::Palette,
            IconName::Play,
            IconName::Settings,
        ] {
            let path = icon.path();
            assert!(AppAssets.load(&path).unwrap().is_some(), "{path} missing");
        }
    }
}
