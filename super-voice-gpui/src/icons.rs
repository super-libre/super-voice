// SPDX-License-Identifier: GPL-3.0-only
//! Every icon the app draws: Phosphor's (<https://phosphoricons.com>), Regular
//! weight, the real files from `@phosphor-icons/core` 2.1.1 (its
//! `assets/regular/`, and `assets/fill/` for the one icon that is filled).
//! They sit in `resources/icons/phosphor/` with Phosphor's MIT licence.
//!
//! The app names its own icons with [`Phosphor`]. The kit draws icons of its
//! own inside its components (a select's chevron, a checkbox's mark, a
//! toast's sign, the window buttons): it asks the asset source for Lucide
//! paths such as `icons/chevron-down.svg`, and [`KIT`] answers each of them
//! with a Phosphor icon instead. No Lucide icon is ever drawn.
//!
//! A new icon: take its file from the same package, unchanged, add a line
//! to [`phosphor!`]. Never draw one or edit its paths.

use gpui_kit::SharedString;
use gpui_kit::assets::IconNamed;

/// The app's icons, and the bytes of each, from one list of Phosphor names.
macro_rules! phosphor {
    ($($variant:ident => $file:literal,)*) => {
        /// A Phosphor icon, by its name on phosphoricons.com.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Phosphor {
            $($variant,)*
        }

        impl Phosphor {
            /// Every icon the app carries.
            #[cfg(test)]
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The path of every icon the app carries.
            pub const PATHS: &[&str] = &[$(concat!("phosphor/", $file, ".svg"),)*];

            /// The path the app's asset source serves it at.
            pub const fn path_str(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("phosphor/", $file, ".svg"),)*
                }
            }

            /// Its SVG, as Phosphor publishes it.
            pub const fn svg(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/resources/icons/phosphor/",
                        $file,
                        ".svg"
                    )),)*
                }
            }

            /// The icon served at `path`, if it's one of the app's.
            pub fn at(path: &str) -> Option<Self> {
                match path {
                    $(concat!("phosphor/", $file, ".svg") => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

phosphor! {
    AppWindow => "app-window",
    ArrowArcLeft => "arrow-arc-left",
    ArrowArcRight => "arrow-arc-right",
    ArrowClockwise => "arrow-clockwise",
    ArrowDown => "arrow-down",
    ArrowLeft => "arrow-left",
    ArrowRight => "arrow-right",
    ArrowSquareOut => "arrow-square-out",
    ArrowUUpLeft => "arrow-u-up-left",
    ArrowUUpRight => "arrow-u-up-right",
    ArrowUp => "arrow-up",
    ArrowsClockwise => "arrows-clockwise",
    ArrowsIn => "arrows-in",
    ArrowsOut => "arrows-out",
    ArrowsOutSimple => "arrows-out-simple",
    Asterisk => "asterisk",
    Backspace => "backspace",
    BatteryCharging => "battery-charging",
    BatteryEmpty => "battery-empty",
    BatteryFull => "battery-full",
    BatteryLow => "battery-low",
    BatteryMedium => "battery-medium",
    BatteryWarning => "battery-warning",
    Bell => "bell",
    BellSlash => "bell-slash",
    BookOpen => "book-open",
    Books => "books",
    Brain => "brain",
    Bug => "bug",
    Buildings => "buildings",
    Calendar => "calendar",
    CaretDown => "caret-down",
    CaretLeft => "caret-left",
    CaretRight => "caret-right",
    CaretUp => "caret-up",
    CaretUpDown => "caret-up-down",
    Cards => "cards",
    ChartPie => "chart-pie",
    ChatText => "chat-text",
    Check => "check",
    CheckCircle => "check-circle",
    CircleNotch => "circle-notch",
    Cloud => "cloud",
    Copy => "copy",
    CopySimple => "copy-simple",
    Cpu => "cpu",
    CursorClick => "cursor-click",
    DotsThree => "dots-three",
    DotsThreeVertical => "dots-three-vertical",
    DownloadSimple => "download-simple",
    Eye => "eye",
    EyeSlash => "eye-slash",
    File => "file",
    FileAudio => "file-audio",
    FileText => "file-text",
    Folder => "folder",
    FolderOpen => "folder-open",
    FrameCorners => "frame-corners",
    FunnelSimple => "funnel-simple",
    Gear => "gear",
    GithubLogo => "github-logo",
    Globe => "globe",
    GraphicsCard => "graphics-card",
    HardDrive => "hard-drive",
    Heart => "heart",
    HeartBreak => "heart-break",
    Info => "info",
    Key => "key",
    Keyboard => "keyboard",
    List => "list",
    ListChecks => "list-checks",
    MagicWand => "magic-wand",
    MagnifyingGlass => "magnifying-glass",
    MapTrifold => "map-trifold",
    Memory => "memory",
    Microphone => "microphone",
    Minus => "minus",
    Moon => "moon",
    Network => "network",
    NotePencil => "note-pencil",
    Package => "package",
    Palette => "palette",
    Pause => "pause",
    Play => "play",
    Plugs => "plugs",
    Plus => "plus",
    Prohibit => "prohibit",
    Robot => "robot",
    ShieldSlash => "shield-slash",
    SidebarSimple => "sidebar-simple",
    SlidersHorizontal => "sliders-horizontal",
    SortAscending => "sort-ascending",
    SortDescending => "sort-descending",
    SpeakerHifi => "speaker-hifi",
    SpeakerHigh => "speaker-high",
    Spinner => "spinner",
    Square => "square",
    SquareHalf => "square-half",
    SquareHalfBottom => "square-half-bottom",
    SquaresFour => "squares-four",
    Star => "star",
    // The one Fill icon: the kit's filled star means filled.
    StarFill => "star-fill",
    Stop => "stop",
    Sun => "sun",
    Swap => "swap",
    TerminalWindow => "terminal-window",
    TextAa => "text-aa",
    TextT => "text-t",
    ThumbsDown => "thumbs-down",
    ThumbsUp => "thumbs-up",
    Trash => "trash",
    Tray => "tray",
    UploadSimple => "upload-simple",
    User => "user",
    UserCircle => "user-circle",
    UserSound => "user-sound",
    Warning => "warning",
    WarningCircle => "warning-circle",
    Waveform => "waveform",
    X => "x",
    XCircle => "x-circle",
}

impl IconNamed for Phosphor {
    fn path(self) -> SharedString {
        self.path_str().into()
    }
}

/// Each icon the kit draws inside its components, by the Lucide path it asks
/// for, and the Phosphor icon drawn there instead. Every icon of the kit's
/// default set is here (a test holds it to that), so a kit update can't
/// bring in a Lucide icon unseen.
pub const KIT: &[(&str, Phosphor)] = &[
    ("icons/a-large-small.svg", Phosphor::TextAa),
    ("icons/arrow-down.svg", Phosphor::ArrowDown),
    ("icons/arrow-left.svg", Phosphor::ArrowLeft),
    ("icons/arrow-right.svg", Phosphor::ArrowRight),
    ("icons/arrow-up.svg", Phosphor::ArrowUp),
    ("icons/asterisk.svg", Phosphor::Asterisk),
    ("icons/ban.svg", Phosphor::Prohibit),
    ("icons/battery-charging.svg", Phosphor::BatteryCharging),
    ("icons/battery-full.svg", Phosphor::BatteryFull),
    ("icons/battery-low.svg", Phosphor::BatteryLow),
    ("icons/battery-medium.svg", Phosphor::BatteryMedium),
    ("icons/battery-warning.svg", Phosphor::BatteryWarning),
    ("icons/battery.svg", Phosphor::BatteryEmpty),
    ("icons/bell.svg", Phosphor::Bell),
    ("icons/book-open.svg", Phosphor::BookOpen),
    ("icons/bot.svg", Phosphor::Robot),
    ("icons/building-2.svg", Phosphor::Buildings),
    ("icons/calendar.svg", Phosphor::Calendar),
    ("icons/case-sensitive.svg", Phosphor::TextAa),
    ("icons/chart-pie.svg", Phosphor::ChartPie),
    ("icons/check.svg", Phosphor::Check),
    ("icons/chevron-down.svg", Phosphor::CaretDown),
    ("icons/chevron-left.svg", Phosphor::CaretLeft),
    ("icons/chevron-right.svg", Phosphor::CaretRight),
    ("icons/chevron-up.svg", Phosphor::CaretUp),
    ("icons/chevrons-up-down.svg", Phosphor::CaretUpDown),
    ("icons/circle-alert.svg", Phosphor::WarningCircle),
    ("icons/circle-check.svg", Phosphor::CheckCircle),
    ("icons/circle-user.svg", Phosphor::UserCircle),
    ("icons/circle-x.svg", Phosphor::XCircle),
    ("icons/close.svg", Phosphor::X),
    ("icons/copy.svg", Phosphor::Copy),
    ("icons/cpu.svg", Phosphor::Cpu),
    ("icons/dash.svg", Phosphor::Minus),
    // Lucide's `delete` is the backspace key.
    ("icons/delete.svg", Phosphor::Backspace),
    ("icons/ellipsis-vertical.svg", Phosphor::DotsThreeVertical),
    ("icons/ellipsis.svg", Phosphor::DotsThree),
    ("icons/external-link.svg", Phosphor::ArrowSquareOut),
    ("icons/eye-off.svg", Phosphor::EyeSlash),
    ("icons/eye.svg", Phosphor::Eye),
    ("icons/file-text.svg", Phosphor::FileText),
    ("icons/file.svg", Phosphor::File),
    ("icons/folder-closed.svg", Phosphor::Folder),
    ("icons/folder-open.svg", Phosphor::FolderOpen),
    ("icons/folder.svg", Phosphor::Folder),
    ("icons/frame.svg", Phosphor::FrameCorners),
    ("icons/gallery-vertical-end.svg", Phosphor::Cards),
    ("icons/github.svg", Phosphor::GithubLogo),
    ("icons/globe.svg", Phosphor::Globe),
    ("icons/hard-drive.svg", Phosphor::HardDrive),
    ("icons/heart-off.svg", Phosphor::HeartBreak),
    ("icons/heart.svg", Phosphor::Heart),
    ("icons/inbox.svg", Phosphor::Tray),
    ("icons/info.svg", Phosphor::Info),
    ("icons/inspector.svg", Phosphor::CursorClick),
    ("icons/layout-dashboard.svg", Phosphor::SquaresFour),
    ("icons/loader-circle.svg", Phosphor::CircleNotch),
    ("icons/loader.svg", Phosphor::Spinner),
    ("icons/map.svg", Phosphor::MapTrifold),
    ("icons/maximize.svg", Phosphor::ArrowsOut),
    ("icons/memory-stick.svg", Phosphor::Memory),
    ("icons/menu.svg", Phosphor::List),
    ("icons/mic.svg", Phosphor::Microphone),
    ("icons/minimize.svg", Phosphor::ArrowsIn),
    ("icons/minus.svg", Phosphor::Minus),
    ("icons/moon.svg", Phosphor::Moon),
    ("icons/network.svg", Phosphor::Network),
    ("icons/palette.svg", Phosphor::Palette),
    ("icons/panel-bottom-open.svg", Phosphor::SquareHalfBottom),
    ("icons/panel-bottom.svg", Phosphor::SquareHalfBottom),
    ("icons/panel-left-close.svg", Phosphor::SidebarSimple),
    ("icons/panel-left-open.svg", Phosphor::SidebarSimple),
    ("icons/panel-left.svg", Phosphor::SidebarSimple),
    ("icons/panel-right-close.svg", Phosphor::SquareHalf),
    ("icons/panel-right-open.svg", Phosphor::SquareHalf),
    ("icons/panel-right.svg", Phosphor::SquareHalf),
    ("icons/pause.svg", Phosphor::Pause),
    ("icons/play.svg", Phosphor::Play),
    ("icons/plus.svg", Phosphor::Plus),
    ("icons/redo-2.svg", Phosphor::ArrowUUpRight),
    ("icons/redo.svg", Phosphor::ArrowArcRight),
    ("icons/refresh-cw.svg", Phosphor::ArrowsClockwise),
    ("icons/replace.svg", Phosphor::Swap),
    ("icons/resize-corner.svg", Phosphor::ArrowsOutSimple),
    ("icons/rotate-cw.svg", Phosphor::ArrowClockwise),
    ("icons/search.svg", Phosphor::MagnifyingGlass),
    ("icons/settings-2.svg", Phosphor::SlidersHorizontal),
    ("icons/settings.svg", Phosphor::Gear),
    ("icons/sort-ascending.svg", Phosphor::SortAscending),
    ("icons/sort-descending.svg", Phosphor::SortDescending),
    ("icons/square-terminal.svg", Phosphor::TerminalWindow),
    // The kit's square is a stop button (speech button and indicator).
    ("icons/square.svg", Phosphor::Stop),
    ("icons/star-fill.svg", Phosphor::StarFill),
    // Phosphor has no struck-out star.
    ("icons/star-off.svg", Phosphor::Star),
    ("icons/star.svg", Phosphor::Star),
    ("icons/sun.svg", Phosphor::Sun),
    ("icons/thumbs-down.svg", Phosphor::ThumbsDown),
    ("icons/thumbs-up.svg", Phosphor::ThumbsUp),
    ("icons/triangle-alert.svg", Phosphor::Warning),
    ("icons/undo-2.svg", Phosphor::ArrowUUpLeft),
    ("icons/undo.svg", Phosphor::ArrowArcLeft),
    ("icons/user.svg", Phosphor::User),
    // The title bar's window buttons on Linux.
    ("icons/window-close.svg", Phosphor::X),
    ("icons/window-maximize.svg", Phosphor::Square),
    ("icons/window-minimize.svg", Phosphor::Minus),
    ("icons/window-restore.svg", Phosphor::CopySimple),
];

/// The SVG to draw for `path`: one of the app's icons, or the Phosphor icon
/// drawn in place of one of the kit's.
pub fn svg(path: &str) -> Option<&'static [u8]> {
    if let Some(icon) = Phosphor::at(path) {
        return Some(icon.svg());
    }
    KIT.iter()
        .find(|(kit, _)| *kit == path)
        .map(|(_, icon)| icon.svg())
}

/// Every path [`svg`] answers.
pub fn paths() -> impl Iterator<Item = &'static str> {
    KIT.iter()
        .map(|(kit, _)| *kit)
        .chain(Phosphor::PATHS.iter().copied())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use gpui_kit::AssetSource as _;

    use super::*;

    /// Every icon the kit can draw is answered with a Phosphor one.
    #[test]
    fn every_kit_icon_is_phosphor() {
        let kit = gpui_kit::assets::Assets.list("").expect("the kit's icons");
        assert!(!kit.is_empty());
        let mapped: BTreeSet<&str> = KIT.iter().map(|(path, _)| *path).collect();
        for path in &kit {
            assert!(
                mapped.contains(path.as_ref()),
                "the kit draws {path}, which has no Phosphor icon in KIT"
            );
            let drawn = svg(path).expect("drawn");
            assert!(Phosphor::ALL.iter().any(|icon| icon.svg() == drawn));
        }
        assert_eq!(mapped.len(), KIT.len(), "a kit path is in KIT twice");
    }

    /// Every icon the app names loads, and is an SVG.
    #[test]
    fn every_app_icon_loads() {
        for icon in Phosphor::ALL {
            let path = icon.path_str();
            assert_eq!(Phosphor::at(path), Some(*icon));
            let drawn = svg(path).expect("drawn");
            assert!(drawn.starts_with(b"<svg"), "{path} is not an SVG");
            assert!(
                std::str::from_utf8(drawn)
                    .is_ok_and(|text| text.contains("viewBox=\"0 0 256 256\"")),
                "{path} is not on Phosphor's 256 grid"
            );
        }
    }

    /// Every file in the Phosphor folder is one the app carries, and the
    /// licence sits beside them.
    #[test]
    fn the_folder_holds_only_icons_in_use() {
        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/icons/phosphor");
        let carried: BTreeSet<String> = Phosphor::ALL
            .iter()
            .map(|icon| icon.path_str().trim_start_matches("phosphor/").to_owned())
            .collect();
        let mut licence = false;
        for entry in std::fs::read_dir(folder).expect("the Phosphor folder") {
            let name = entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("UTF-8");
            if name == "LICENSE" {
                licence = true;
                continue;
            }
            assert!(
                carried.contains(&name),
                "{name} is in the folder but not in phosphor!"
            );
        }
        assert!(licence, "Phosphor's licence must sit beside its icons");
    }

    /// The app names icons only through [`Phosphor`]: no Lucide name in its
    /// source.
    #[test]
    fn the_app_names_no_lucide_icon() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("the source folder") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let text = std::fs::read_to_string(&path).expect("a source file");
                    for (line, words) in text.lines().enumerate() {
                        if words.contains("IconName") || words.contains("Lucide::") {
                            found.push(format!("{}:{}", path.display(), line + 1));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")),
            &mut found,
        );
        found.retain(|place| !place.contains("/src/icons.rs:"));
        assert!(found.is_empty(), "Lucide icons named at {found:?}");
    }
}
