// SPDX-License-Identifier: GPL-3.0-only
//! The keys the macOS shortcut listener (`voice hotkey`) listens for, as one
//! binding: modifiers and one key joined by `+`, such as `ctrl+alt+space` or
//! `cmd+shift+KeyR`.
//!
//! The listener reads a binding with `global-hotkey`'s parser
//! (`HotKey::from_str`). [`Binding::parse`] reads exactly the grammar that
//! parser does, so the daemon, the CLI and the apps can check a binding
//! without depending on `global-hotkey`, whose Linux half brings X11 with it.
//! The CLI holds the two to the same answers in a test
//! (`the_shared_binding_grammar_is_global_hotkeys`).
//!
//! The grammar, as `global-hotkey` 0.10 has it:
//!
//! 1. Parts are joined by `+`. A binding of one part is a key alone, read as
//!    it is; with more parts each is trimmed, and an empty one is refused.
//! 2. Modifiers come first, in any order and case: `ctrl`/`control`,
//!    `alt`/`option`, `shift`, `cmd`/`command`/`super`/`meta`, and
//!    `cmdorctrl` (and its spellings), which is `cmd` on a Mac.
//! 3. Then one key, in any case: a `keyboard-types` `Code` name (`Space`,
//!    `KeyR`, `Digit1`, `F5`, `ArrowUp`) or one of its short forms (`r`, `1`,
//!    `up`, `,`). Nothing may follow it.
//!
//! [`validate`] adds what the listener needs to register a binding on a Mac:
//! a key the Mac has a key code for, and a modifier unless the key is an F key.

use std::fmt;

/// The binding the listener uses until one is set: ⌃⌥Space. The obvious
/// candidates are taken: ⌘Space is Spotlight, ⌃Space switches input sources,
/// and ⌥Space is the `ChatGPT` app's default.
pub const DEFAULT_BINDING: &str = "ctrl+alt+space";

/// The setting's name: what `settings_changed` names when it changes, and its
/// path under `/settings`.
pub const SETTING: &str = "hotkey";

/// Which modifiers a binding holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    /// ⌃, `ctrl`.
    pub const CONTROL: Self = Self(1);
    /// ⌥, `alt`.
    pub const OPTION: Self = Self(1 << 1);
    /// ⇧, `shift`.
    pub const SHIFT: Self = Self(1 << 2);
    /// ⌘, `cmd`.
    pub const COMMAND: Self = Self(1 << 3);

    /// Each modifier, in the order a Mac shows them (⌃ ⌥ ⇧ ⌘), with its
    /// symbol and the name a binding spells it with.
    const ORDER: [(Self, &'static str, &'static str); 4] = [
        (Self::CONTROL, "⌃", "ctrl"),
        (Self::OPTION, "⌥", "alt"),
        (Self::SHIFT, "⇧", "shift"),
        (Self::COMMAND, "⌘", "cmd"),
    ];

    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The modifiers an `NSEvent`'s `modifierFlags` hold (its raw value).
    /// Caps Lock, the Fn key and the keypad's flag are not modifiers here.
    #[must_use]
    pub const fn from_mac_flags(flags: u64) -> Self {
        const SHIFT: u64 = 1 << 17;
        const CONTROL: u64 = 1 << 18;
        const OPTION: u64 = 1 << 19;
        const COMMAND: u64 = 1 << 20;
        let mut bits = 0;
        if flags & CONTROL != 0 {
            bits |= Self::CONTROL.0;
        }
        if flags & OPTION != 0 {
            bits |= Self::OPTION.0;
        }
        if flags & SHIFT != 0 {
            bits |= Self::SHIFT.0;
        }
        if flags & COMMAND != 0 {
            bits |= Self::COMMAND.0;
        }
        Self(bits)
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// How the Mac's listener registers a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Mac {
    /// Through Carbon's `RegisterEventHotKey`, by this virtual key code
    /// (`kVK_*`), as `global-hotkey`'s `key_to_scancode` maps it.
    Code(u16),
    /// A media key, which `global-hotkey` watches with an event tap. That
    /// needs Input Monitoring, which the listener is not granted.
    Media,
    /// Not at all: `global-hotkey` has no key code for it on a Mac.
    None,
}

/// One key the grammar names.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Key {
    /// Its `keyboard-types` `Code` name.
    code: &'static str,
    /// Every spelling `global-hotkey` reads as it, upper-cased, as its parser
    /// compares them.
    aliases: &'static [&'static str],
    mac: Mac,
}

/// Every key `global-hotkey`'s parser knows, in its parser's order (which
/// decides the name a Mac key code is read back as, where two share one).
/// Transcribed from `parse_key` and the macOS `key_to_scancode` of
/// `global-hotkey` 0.10.
#[rustfmt::skip]
static KEYS: &[Key] = &[
    Key { code: "Backquote", aliases: &["BACKQUOTE", "`"], mac: Mac::Code(0x32) },
    Key { code: "Backslash", aliases: &["BACKSLASH", "\\"], mac: Mac::Code(0x2a) },
    Key { code: "BracketLeft", aliases: &["BRACKETLEFT", "["], mac: Mac::Code(0x21) },
    Key { code: "BracketRight", aliases: &["BRACKETRIGHT", "]"], mac: Mac::Code(0x1e) },
    Key { code: "Pause", aliases: &["PAUSE", "PAUSEBREAK"], mac: Mac::None },
    Key { code: "Comma", aliases: &["COMMA", ","], mac: Mac::Code(0x2b) },
    Key { code: "Digit0", aliases: &["DIGIT0", "0"], mac: Mac::Code(0x1d) },
    Key { code: "Digit1", aliases: &["DIGIT1", "1"], mac: Mac::Code(0x12) },
    Key { code: "Digit2", aliases: &["DIGIT2", "2"], mac: Mac::Code(0x13) },
    Key { code: "Digit3", aliases: &["DIGIT3", "3"], mac: Mac::Code(0x14) },
    Key { code: "Digit4", aliases: &["DIGIT4", "4"], mac: Mac::Code(0x15) },
    Key { code: "Digit5", aliases: &["DIGIT5", "5"], mac: Mac::Code(0x17) },
    Key { code: "Digit6", aliases: &["DIGIT6", "6"], mac: Mac::Code(0x16) },
    Key { code: "Digit7", aliases: &["DIGIT7", "7"], mac: Mac::Code(0x1a) },
    Key { code: "Digit8", aliases: &["DIGIT8", "8"], mac: Mac::Code(0x1c) },
    Key { code: "Digit9", aliases: &["DIGIT9", "9"], mac: Mac::Code(0x19) },
    Key { code: "Equal", aliases: &["EQUAL", "="], mac: Mac::Code(0x18) },
    Key { code: "IntlBackslash", aliases: &["INTLBACKSLASH"], mac: Mac::Code(0x0a) },
    Key { code: "IntlRo", aliases: &["INTLRO"], mac: Mac::Code(0x5e) },
    Key { code: "IntlYen", aliases: &["INTLYEN"], mac: Mac::Code(0x5d) },
    Key { code: "KeyA", aliases: &["KEYA", "A"], mac: Mac::Code(0x00) },
    Key { code: "KeyB", aliases: &["KEYB", "B"], mac: Mac::Code(0x0b) },
    Key { code: "KeyC", aliases: &["KEYC", "C"], mac: Mac::Code(0x08) },
    Key { code: "KeyD", aliases: &["KEYD", "D"], mac: Mac::Code(0x02) },
    Key { code: "KeyE", aliases: &["KEYE", "E"], mac: Mac::Code(0x0e) },
    Key { code: "KeyF", aliases: &["KEYF", "F"], mac: Mac::Code(0x03) },
    Key { code: "KeyG", aliases: &["KEYG", "G"], mac: Mac::Code(0x05) },
    Key { code: "KeyH", aliases: &["KEYH", "H"], mac: Mac::Code(0x04) },
    Key { code: "KeyI", aliases: &["KEYI", "I"], mac: Mac::Code(0x22) },
    Key { code: "KeyJ", aliases: &["KEYJ", "J"], mac: Mac::Code(0x26) },
    Key { code: "KeyK", aliases: &["KEYK", "K"], mac: Mac::Code(0x28) },
    Key { code: "KeyL", aliases: &["KEYL", "L"], mac: Mac::Code(0x25) },
    Key { code: "KeyM", aliases: &["KEYM", "M"], mac: Mac::Code(0x2e) },
    Key { code: "KeyN", aliases: &["KEYN", "N"], mac: Mac::Code(0x2d) },
    Key { code: "KeyO", aliases: &["KEYO", "O"], mac: Mac::Code(0x1f) },
    Key { code: "KeyP", aliases: &["KEYP", "P"], mac: Mac::Code(0x23) },
    Key { code: "KeyQ", aliases: &["KEYQ", "Q"], mac: Mac::Code(0x0c) },
    Key { code: "KeyR", aliases: &["KEYR", "R"], mac: Mac::Code(0x0f) },
    Key { code: "KeyS", aliases: &["KEYS", "S"], mac: Mac::Code(0x01) },
    Key { code: "KeyT", aliases: &["KEYT", "T"], mac: Mac::Code(0x11) },
    Key { code: "KeyU", aliases: &["KEYU", "U"], mac: Mac::Code(0x20) },
    Key { code: "KeyV", aliases: &["KEYV", "V"], mac: Mac::Code(0x09) },
    Key { code: "KeyW", aliases: &["KEYW", "W"], mac: Mac::Code(0x0d) },
    Key { code: "KeyX", aliases: &["KEYX", "X"], mac: Mac::Code(0x07) },
    Key { code: "KeyY", aliases: &["KEYY", "Y"], mac: Mac::Code(0x10) },
    Key { code: "KeyZ", aliases: &["KEYZ", "Z"], mac: Mac::Code(0x06) },
    Key { code: "Minus", aliases: &["MINUS", "-"], mac: Mac::Code(0x1b) },
    Key { code: "Period", aliases: &["PERIOD", "."], mac: Mac::Code(0x2f) },
    Key { code: "Quote", aliases: &["QUOTE", "'"], mac: Mac::Code(0x27) },
    Key { code: "Semicolon", aliases: &["SEMICOLON", ";"], mac: Mac::Code(0x29) },
    Key { code: "Slash", aliases: &["SLASH", "/"], mac: Mac::Code(0x2c) },
    Key { code: "AltLeft", aliases: &["ALTLEFT"], mac: Mac::None },
    Key { code: "AltRight", aliases: &["ALTRIGHT"], mac: Mac::None },
    Key { code: "Backspace", aliases: &["BACKSPACE"], mac: Mac::Code(0x33) },
    Key { code: "CapsLock", aliases: &["CAPSLOCK"], mac: Mac::Code(0x39) },
    Key { code: "ContextMenu", aliases: &["CONTEXTMENU"], mac: Mac::None },
    Key { code: "ControlLeft", aliases: &["CONTROLLEFT"], mac: Mac::None },
    Key { code: "ControlRight", aliases: &["CONTROLRIGHT"], mac: Mac::None },
    Key { code: "Enter", aliases: &["ENTER"], mac: Mac::Code(0x24) },
    Key { code: "MetaLeft", aliases: &["METALEFT"], mac: Mac::None },
    Key { code: "MetaRight", aliases: &["METARIGHT"], mac: Mac::None },
    Key { code: "ShiftLeft", aliases: &["SHIFTLEFT"], mac: Mac::None },
    Key { code: "ShiftRight", aliases: &["SHIFTRIGHT"], mac: Mac::None },
    Key { code: "Space", aliases: &["SPACE"], mac: Mac::Code(0x31) },
    Key { code: "Tab", aliases: &["TAB"], mac: Mac::Code(0x30) },
    Key { code: "Convert", aliases: &["CONVERT"], mac: Mac::None },
    Key { code: "KanaMode", aliases: &["KANAMODE"], mac: Mac::None },
    Key { code: "Lang1", aliases: &["LANG1"], mac: Mac::Code(0x68) },
    Key { code: "Lang2", aliases: &["LANG2"], mac: Mac::Code(0x66) },
    Key { code: "Lang3", aliases: &["LANG3"], mac: Mac::None },
    Key { code: "Lang4", aliases: &["LANG4"], mac: Mac::None },
    Key { code: "Lang5", aliases: &["LANG5"], mac: Mac::None },
    Key { code: "NonConvert", aliases: &["NONCONVERT"], mac: Mac::None },
    Key { code: "Delete", aliases: &["DELETE"], mac: Mac::Code(0x75) },
    Key { code: "End", aliases: &["END"], mac: Mac::Code(0x77) },
    Key { code: "Help", aliases: &["HELP"], mac: Mac::Code(0x72) },
    Key { code: "Home", aliases: &["HOME"], mac: Mac::Code(0x73) },
    Key { code: "Insert", aliases: &["INSERT"], mac: Mac::Code(0x72) },
    Key { code: "PageDown", aliases: &["PAGEDOWN"], mac: Mac::Code(0x79) },
    Key { code: "PageUp", aliases: &["PAGEUP"], mac: Mac::Code(0x74) },
    Key { code: "PrintScreen", aliases: &["PRINTSCREEN"], mac: Mac::Code(0x46) },
    Key { code: "ScrollLock", aliases: &["SCROLLLOCK"], mac: Mac::None },
    Key { code: "ArrowDown", aliases: &["ARROWDOWN", "DOWN"], mac: Mac::Code(0x7d) },
    Key { code: "ArrowLeft", aliases: &["ARROWLEFT", "LEFT"], mac: Mac::Code(0x7b) },
    Key { code: "ArrowRight", aliases: &["ARROWRIGHT", "RIGHT"], mac: Mac::Code(0x7c) },
    Key { code: "ArrowUp", aliases: &["ARROWUP", "UP"], mac: Mac::Code(0x7e) },
    Key { code: "NumLock", aliases: &["NUMLOCK"], mac: Mac::Code(0x47) },
    Key { code: "Numpad0", aliases: &["NUMPAD0", "NUM0"], mac: Mac::Code(0x52) },
    Key { code: "Numpad1", aliases: &["NUMPAD1", "NUM1"], mac: Mac::Code(0x53) },
    Key { code: "Numpad2", aliases: &["NUMPAD2", "NUM2"], mac: Mac::Code(0x54) },
    Key { code: "Numpad3", aliases: &["NUMPAD3", "NUM3"], mac: Mac::Code(0x55) },
    Key { code: "Numpad4", aliases: &["NUMPAD4", "NUM4"], mac: Mac::Code(0x56) },
    Key { code: "Numpad5", aliases: &["NUMPAD5", "NUM5"], mac: Mac::Code(0x57) },
    Key { code: "Numpad6", aliases: &["NUMPAD6", "NUM6"], mac: Mac::Code(0x58) },
    Key { code: "Numpad7", aliases: &["NUMPAD7", "NUM7"], mac: Mac::Code(0x59) },
    Key { code: "Numpad8", aliases: &["NUMPAD8", "NUM8"], mac: Mac::Code(0x5b) },
    Key { code: "Numpad9", aliases: &["NUMPAD9", "NUM9"], mac: Mac::Code(0x5c) },
    Key { code: "NumpadAdd", aliases: &["NUMPADADD", "NUMADD", "NUMPADPLUS", "NUMPLUS"], mac: Mac::Code(0x45) },
    Key { code: "NumpadBackspace", aliases: &["NUMPADBACKSPACE"], mac: Mac::None },
    Key { code: "NumpadClear", aliases: &["NUMPADCLEAR", "NUMCLEAR"], mac: Mac::None },
    Key { code: "NumpadClearEntry", aliases: &["NUMPADCLEARENTRY"], mac: Mac::None },
    Key { code: "NumpadComma", aliases: &["NUMPADCOMMA"], mac: Mac::Code(0x5f) },
    Key { code: "NumpadDecimal", aliases: &["NUMPADDECIMAL", "NUMDECIMAL"], mac: Mac::Code(0x41) },
    Key { code: "NumpadDivide", aliases: &["NUMPADDIVIDE", "NUMDIVIDE"], mac: Mac::Code(0x4b) },
    Key { code: "NumpadEnter", aliases: &["NUMPADENTER", "NUMENTER"], mac: Mac::Code(0x4c) },
    Key { code: "NumpadEqual", aliases: &["NUMPADEQUAL", "NUMEQUAL"], mac: Mac::Code(0x51) },
    Key { code: "NumpadHash", aliases: &["NUMPADHASH"], mac: Mac::None },
    Key { code: "NumpadMemoryAdd", aliases: &["NUMPADMEMORYADD"], mac: Mac::None },
    Key { code: "NumpadMemoryClear", aliases: &["NUMPADMEMORYCLEAR"], mac: Mac::None },
    Key { code: "NumpadMemoryRecall", aliases: &["NUMPADMEMORYRECALL"], mac: Mac::None },
    Key { code: "NumpadMemoryStore", aliases: &["NUMPADMEMORYSTORE"], mac: Mac::None },
    Key { code: "NumpadMemorySubtract", aliases: &["NUMPADMEMORYSUBTRACT"], mac: Mac::None },
    Key { code: "NumpadMultiply", aliases: &["NUMPADMULTIPLY", "NUMMULTIPLY"], mac: Mac::Code(0x43) },
    Key { code: "NumpadParenLeft", aliases: &["NUMPADPARENLEFT"], mac: Mac::None },
    Key { code: "NumpadParenRight", aliases: &["NUMPADPARENRIGHT"], mac: Mac::None },
    Key { code: "NumpadStar", aliases: &["NUMPADSTAR"], mac: Mac::None },
    Key { code: "NumpadSubtract", aliases: &["NUMPADSUBTRACT", "NUMSUBTRACT"], mac: Mac::Code(0x4e) },
    Key { code: "Escape", aliases: &["ESCAPE", "ESC"], mac: Mac::Code(0x35) },
    Key { code: "Fn", aliases: &["FN"], mac: Mac::None },
    Key { code: "FnLock", aliases: &["FNLOCK"], mac: Mac::None },
    Key { code: "BrowserBack", aliases: &["BROWSERBACK"], mac: Mac::None },
    Key { code: "BrowserFavorites", aliases: &["BROWSERFAVORITES"], mac: Mac::None },
    Key { code: "BrowserForward", aliases: &["BROWSERFORWARD"], mac: Mac::None },
    Key { code: "BrowserHome", aliases: &["BROWSERHOME"], mac: Mac::None },
    Key { code: "BrowserRefresh", aliases: &["BROWSERREFRESH"], mac: Mac::None },
    Key { code: "BrowserSearch", aliases: &["BROWSERSEARCH"], mac: Mac::None },
    Key { code: "BrowserStop", aliases: &["BROWSERSTOP"], mac: Mac::None },
    Key { code: "Eject", aliases: &["EJECT"], mac: Mac::None },
    Key { code: "LaunchApp1", aliases: &["LAUNCHAPP1", "LAUNCHAPPLICATION1", "MYCOMPUTER"], mac: Mac::None },
    Key { code: "LaunchApp2", aliases: &["LAUNCHAPP2", "LAUNCHAPPLICATION2", "CALCULATOR"], mac: Mac::None },
    Key { code: "LaunchMail", aliases: &["LAUNCHMAIL"], mac: Mac::None },
    Key { code: "MediaSelect", aliases: &["MEDIASELECT", "LAUNCHMEDIAPLAYER"], mac: Mac::None },
    Key { code: "Power", aliases: &["POWER"], mac: Mac::None },
    Key { code: "Sleep", aliases: &["SLEEP"], mac: Mac::None },
    Key { code: "WakeUp", aliases: &["WAKEUP"], mac: Mac::None },
    Key { code: "Hyper", aliases: &["HYPER"], mac: Mac::None },
    Key { code: "Super", aliases: &["SUPER"], mac: Mac::None },
    Key { code: "Turbo", aliases: &["TURBO"], mac: Mac::None },
    Key { code: "Abort", aliases: &["ABORT"], mac: Mac::None },
    Key { code: "Resume", aliases: &["RESUME"], mac: Mac::None },
    Key { code: "Suspend", aliases: &["SUSPEND"], mac: Mac::None },
    Key { code: "Again", aliases: &["AGAIN"], mac: Mac::None },
    Key { code: "Copy", aliases: &["COPY"], mac: Mac::None },
    Key { code: "Cut", aliases: &["CUT"], mac: Mac::None },
    Key { code: "Find", aliases: &["FIND"], mac: Mac::None },
    Key { code: "Open", aliases: &["OPEN"], mac: Mac::None },
    Key { code: "Paste", aliases: &["PASTE"], mac: Mac::None },
    Key { code: "Props", aliases: &["PROPS"], mac: Mac::None },
    Key { code: "Select", aliases: &["SELECT"], mac: Mac::None },
    Key { code: "Undo", aliases: &["UNDO"], mac: Mac::None },
    Key { code: "Hiragana", aliases: &["HIRAGANA"], mac: Mac::None },
    Key { code: "Katakana", aliases: &["KATAKANA"], mac: Mac::None },
    Key { code: "Unidentified", aliases: &["UNIDENTIFIED"], mac: Mac::None },
    Key { code: "F1", aliases: &["F1"], mac: Mac::Code(0x7a) },
    Key { code: "F2", aliases: &["F2"], mac: Mac::Code(0x78) },
    Key { code: "F3", aliases: &["F3"], mac: Mac::Code(0x63) },
    Key { code: "F4", aliases: &["F4"], mac: Mac::Code(0x76) },
    Key { code: "F5", aliases: &["F5"], mac: Mac::Code(0x60) },
    Key { code: "F6", aliases: &["F6"], mac: Mac::Code(0x61) },
    Key { code: "F7", aliases: &["F7"], mac: Mac::Code(0x62) },
    Key { code: "F8", aliases: &["F8"], mac: Mac::Code(0x64) },
    Key { code: "F9", aliases: &["F9"], mac: Mac::Code(0x65) },
    Key { code: "F10", aliases: &["F10"], mac: Mac::Code(0x6d) },
    Key { code: "F11", aliases: &["F11"], mac: Mac::Code(0x67) },
    Key { code: "F12", aliases: &["F12"], mac: Mac::Code(0x6f) },
    Key { code: "AudioVolumeDown", aliases: &["AUDIOVOLUMEDOWN", "VOLUMEDOWN"], mac: Mac::Code(0x49) },
    Key { code: "AudioVolumeUp", aliases: &["AUDIOVOLUMEUP", "VOLUMEUP"], mac: Mac::Code(0x48) },
    Key { code: "AudioVolumeMute", aliases: &["AUDIOVOLUMEMUTE", "VOLUMEMUTE"], mac: Mac::Code(0x4a) },
    Key { code: "MediaPlay", aliases: &["MEDIAPLAY"], mac: Mac::None },
    Key { code: "MediaPause", aliases: &["MEDIAPAUSE"], mac: Mac::None },
    Key { code: "MediaPlayPause", aliases: &["MEDIAPLAYPAUSE"], mac: Mac::Media },
    Key { code: "MediaStop", aliases: &["MEDIASTOP"], mac: Mac::None },
    Key { code: "MediaTrackNext", aliases: &["MEDIATRACKNEXT"], mac: Mac::Media },
    Key { code: "MediaTrackPrevious", aliases: &["MEDIATRACKPREV", "MEDIATRACKPREVIOUS"], mac: Mac::Media },
    Key { code: "F13", aliases: &["F13"], mac: Mac::Code(0x69) },
    Key { code: "F14", aliases: &["F14"], mac: Mac::Code(0x6b) },
    Key { code: "F15", aliases: &["F15"], mac: Mac::Code(0x71) },
    Key { code: "F16", aliases: &["F16"], mac: Mac::Code(0x6a) },
    Key { code: "F17", aliases: &["F17"], mac: Mac::Code(0x40) },
    Key { code: "F18", aliases: &["F18"], mac: Mac::Code(0x4f) },
    Key { code: "F19", aliases: &["F19"], mac: Mac::Code(0x50) },
    Key { code: "F20", aliases: &["F20"], mac: Mac::Code(0x5a) },
    Key { code: "F21", aliases: &["F21"], mac: Mac::None },
    Key { code: "F22", aliases: &["F22"], mac: Mac::None },
    Key { code: "F23", aliases: &["F23"], mac: Mac::None },
    Key { code: "F24", aliases: &["F24"], mac: Mac::None },
    Key { code: "F25", aliases: &["F25"], mac: Mac::None },
    Key { code: "F26", aliases: &["F26"], mac: Mac::None },
    Key { code: "F27", aliases: &["F27"], mac: Mac::None },
    Key { code: "F28", aliases: &["F28"], mac: Mac::None },
    Key { code: "F29", aliases: &["F29"], mac: Mac::None },
    Key { code: "F30", aliases: &["F30"], mac: Mac::None },
    Key { code: "F31", aliases: &["F31"], mac: Mac::None },
    Key { code: "F32", aliases: &["F32"], mac: Mac::None },
    Key { code: "F33", aliases: &["F33"], mac: Mac::None },
    Key { code: "F34", aliases: &["F34"], mac: Mac::None },
    Key { code: "F35", aliases: &["F35"], mac: Mac::None },
    Key { code: "BrightnessDown", aliases: &["BRIGHTNESSDOWN"], mac: Mac::None },
    Key { code: "BrightnessUp", aliases: &["BRIGHTNESSUP"], mac: Mac::None },
    Key { code: "DisplayToggleIntExt", aliases: &["DISPLAYTOGGLEINTEXT"], mac: Mac::None },
    Key { code: "KeyboardLayoutSelect", aliases: &["KEYBOARDLAYOUTSELECT"], mac: Mac::None },
    Key { code: "LaunchAssistant", aliases: &["LAUNCHASSISTANT"], mac: Mac::None },
    Key { code: "LaunchControlPanel", aliases: &["LAUNCHCONTROLPANEL"], mac: Mac::None },
    Key { code: "LaunchScreenSaver", aliases: &["LAUNCHSCREENSAVER"], mac: Mac::None },
    Key { code: "MailForward", aliases: &["MAILFORWARD"], mac: Mac::None },
    Key { code: "MailReply", aliases: &["MAILREPLY"], mac: Mac::None },
    Key { code: "MailSend", aliases: &["MAILSEND"], mac: Mac::None },
    Key { code: "MediaFastForward", aliases: &["MEDIAFASTFORWARD"], mac: Mac::Media },
    Key { code: "MediaRecord", aliases: &["MEDIARECORD"], mac: Mac::None },
    Key { code: "MediaRewind", aliases: &["MEDIAREWIND"], mac: Mac::Media },
    Key { code: "MicrophoneMuteToggle", aliases: &["MICROPHONEMUTETOGGLE"], mac: Mac::None },
    Key { code: "PrivacyScreenToggle", aliases: &["PRIVACYSCREENTOGGLE"], mac: Mac::None },
    Key { code: "KeyboardBacklightToggle", aliases: &["KEYBOARDBACKLIGHTTOGGLE"], mac: Mac::None },
    Key { code: "SelectTask", aliases: &["SELECTTASK"], mac: Mac::None },
    Key { code: "ShowAllWindows", aliases: &["SHOWALLWINDOWS"], mac: Mac::None },
    Key { code: "ZoomToggle", aliases: &["ZOOMTOGGLE"], mac: Mac::None },
];

/// Why a binding was refused. Each says what is wrong in a sentence of its
/// own, with no colon in it: the daemon's error messages keep only what
/// comes before the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    /// A part between `+`s is empty: `ctrl++space`, `ctrl+`.
    EmptyPart,
    /// This part is neither a modifier nor a key the listener knows.
    UnknownKey(String),
    /// Something follows the key. The key goes last, after the modifiers.
    KeyNotLast,
    /// Only modifiers, no key.
    NoKey,
    /// A key that types something, with no modifier: it would stop typing it
    /// everywhere. Only an F key may go alone.
    NoModifier,
    /// A key the Mac has no key code for.
    NotOnMac(String),
    /// A media key, which the listener could only watch with Input Monitoring.
    MediaKey(String),
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPart => write!(f, "A part of the shortcut is empty"),
            Self::UnknownKey(part) => write!(f, "`{part}` is not a key the shortcut can use"),
            Self::KeyNotLast => write!(f, "The key goes last, after the modifiers"),
            Self::NoKey => write!(f, "The shortcut has no key, only modifiers"),
            Self::NoModifier => write!(
                f,
                "The shortcut needs a modifier (ctrl, alt, shift or cmd), unless its key is an F key"
            ),
            Self::NotOnMac(key) => write!(f, "A Mac shortcut cannot use the {key} key"),
            Self::MediaKey(key) => write!(
                f,
                "The {key} key needs Input Monitoring, which the shortcut listener does not have"
            ),
        }
    }
}

impl std::error::Error for BindingError {}

/// A binding: its modifiers and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Binding {
    modifiers: Modifiers,
    key: &'static Key,
}

impl Binding {
    /// Read `binding` as `global-hotkey`'s parser does: what it accepts, this
    /// accepts, with the same modifiers and key.
    ///
    /// # Errors
    /// When the parser would refuse it, saying why.
    pub fn parse(binding: &str) -> Result<Self, BindingError> {
        let parts: Vec<&str> = binding.split('+').collect();
        // One part is a key alone, read untrimmed, as the parser does.
        if let [only] = parts.as_slice() {
            return Ok(Self {
                modifiers: Modifiers::NONE,
                key: key_named(only)?,
            });
        }
        let mut modifiers = Modifiers::NONE;
        let mut key = None;
        for raw in parts {
            let part = raw.trim();
            if part.is_empty() {
                return Err(BindingError::EmptyPart);
            }
            if key.is_some() {
                return Err(BindingError::KeyNotLast);
            }
            match modifier_named(part) {
                Some(modifier) => modifiers |= modifier,
                None => key = Some(key_named(part)?),
            }
        }
        key.map(|key| Self { modifiers, key })
            .ok_or(BindingError::NoKey)
    }

    /// The binding the listener uses until one is set.
    ///
    /// # Panics
    /// Never: [`DEFAULT_BINDING`] parses, as a test holds it to.
    #[must_use]
    pub fn default_binding() -> Self {
        Self::parse(DEFAULT_BINDING).expect("the default binding parses")
    }

    #[must_use]
    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// The key, by its `keyboard-types` `Code` name: `Space`, `KeyR`.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        self.key.code
    }

    /// Whether this is [`DEFAULT_BINDING`].
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default_binding()
    }

    /// Every spelling of a key the grammar reads, upper-cased: `SPACE`,
    /// `KEYR`, `R`, `UP`, `,`. Modifiers are not keys and are not listed.
    pub fn key_spellings() -> impl Iterator<Item = &'static str> {
        KEYS.iter().flat_map(|key| key.aliases.iter().copied())
    }

    /// The binding a key press makes on a Mac: `key_code`, an `NSEvent`'s
    /// `keyCode`, with `modifiers`. `None` for a key the listener has no name
    /// for.
    #[must_use]
    pub fn from_mac_key(key_code: u16, modifiers: Modifiers) -> Option<Self> {
        KEYS.iter()
            .find(|key| key.mac == Mac::Code(key_code))
            .map(|key| Self { modifiers, key })
    }

    /// Whether the listener can register this binding on a Mac, and should:
    /// a key with a Mac key code, held with a modifier unless it is an F key.
    ///
    /// # Errors
    /// When it cannot or should not, saying why.
    pub fn check_usable(&self) -> Result<(), BindingError> {
        match self.key.mac {
            Mac::Code(_) => {}
            Mac::Media => return Err(BindingError::MediaKey(self.key.code.to_string())),
            Mac::None => return Err(BindingError::NotOnMac(self.key.code.to_string())),
        }
        if self.modifiers.is_empty() && !is_function_key(self.key.code) {
            return Err(BindingError::NoModifier);
        }
        Ok(())
    }

    /// The binding as a Mac shows it: its modifiers' symbols in the Mac's
    /// order, then its key: `⌃⌥Space`, `⌘⇧R`.
    #[must_use]
    pub fn symbols(&self) -> String {
        let mut out = String::new();
        for (modifier, symbol, _) in Modifiers::ORDER {
            if self.modifiers.contains(modifier) {
                out.push_str(symbol);
            }
        }
        out.push_str(key_symbol(self.key.code));
        out
    }
}

impl Default for Binding {
    fn default() -> Self {
        Self::default_binding()
    }
}

/// The binding written out the one way this crate writes it: modifiers in the
/// Mac's order, then the key's `Code` name: `ctrl+alt+Space`, `shift+cmd+KeyR`.
impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (modifier, _, name) in Modifiers::ORDER {
            if self.modifiers.contains(modifier) {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(self.key.code)
    }
}

/// Check `binding` as the daemon does before storing it: it parses
/// ([`Binding::parse`]) and the listener can use it ([`Binding::check_usable`]).
///
/// # Errors
/// When either says no, saying why.
pub fn validate(binding: &str) -> Result<Binding, BindingError> {
    let parsed = Binding::parse(binding)?;
    parsed.check_usable()?;
    Ok(parsed)
}

/// Whether a daemon event says the binding changed: a `daemon_status_changed`
/// whose `settings_changed` names [`SETTING`].
#[must_use]
pub fn changed_in(event_name: &str, payload: &serde_json::Value) -> bool {
    event_name == "daemon_status_changed"
        && payload.get("status").and_then(serde_json::Value::as_str) == Some("settings_changed")
        && payload.get("setting").and_then(serde_json::Value::as_str) == Some(SETTING)
}

fn modifier_named(part: &str) -> Option<Modifiers> {
    match part.to_uppercase().as_str() {
        "OPTION" | "ALT" => Some(Modifiers::OPTION),
        "CONTROL" | "CTRL" => Some(Modifiers::CONTROL),
        // `cmdorctrl` is `cmd` on a Mac, which is where the binding is used.
        "COMMAND" | "CMD" | "SUPER" | "META" | "COMMANDORCONTROL" | "COMMANDORCTRL"
        | "CMDORCTRL" | "CMDORCONTROL" => Some(Modifiers::COMMAND),
        "SHIFT" => Some(Modifiers::SHIFT),
        _ => None,
    }
}

fn key_named(part: &str) -> Result<&'static Key, BindingError> {
    let upper = part.to_uppercase();
    KEYS.iter()
        .find(|key| key.aliases.contains(&upper.as_str()))
        .ok_or_else(|| BindingError::UnknownKey(part.to_string()))
}

fn is_function_key(code: &str) -> bool {
    code.strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// How a Mac shows `code` in a shortcut. A key with no symbol of its own
/// shows its name.
fn key_symbol(code: &'static str) -> &'static str {
    if let Some(letter) = code.strip_prefix("Key") {
        return letter;
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return digit;
    }
    match code {
        "Enter" => "↩",
        "Tab" => "⇥",
        "Backspace" => "⌫",
        "Delete" => "⌦",
        "Escape" => "⎋",
        "CapsLock" => "⇪",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        "ArrowUp" => "↑",
        "ArrowDown" => "↓",
        "Home" => "↖",
        "End" => "↘",
        "PageUp" => "⇞",
        "PageDown" => "⇟",
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_control_option_space() {
        let binding = Binding::default_binding();
        assert_eq!(binding.modifiers(), Modifiers::CONTROL | Modifiers::OPTION);
        assert_eq!(binding.key(), "Space");
        assert_eq!(binding.symbols(), "⌃⌥Space");
        assert_eq!(binding.to_string(), "ctrl+alt+Space");
        assert!(binding.is_default());
        assert_eq!(validate(DEFAULT_BINDING), Ok(binding));
    }

    #[test]
    fn modifiers_show_in_the_macs_order() {
        let binding = Binding::parse("cmd+shift+KeyR").expect("parses");
        assert_eq!(binding.symbols(), "⇧⌘R");
        assert_eq!(binding.to_string(), "shift+cmd+KeyR");
        let binding = Binding::parse("META+Shift+Option+Control+digit1").expect("parses");
        assert_eq!(binding.symbols(), "⌃⌥⇧⌘1");
        assert_eq!(
            Binding::parse("ctrl+up").map(|b| b.symbols()),
            Ok("⌃↑".into())
        );
        assert_eq!(
            Binding::parse("alt+F5").map(|b| b.symbols()),
            Ok("⌥F5".into())
        );
        assert_eq!(
            Binding::parse("cmd+,").map(|b| b.symbols()),
            Ok("⌘,".into())
        );
    }

    #[test]
    fn the_written_form_reads_back_as_the_same_binding() {
        for text in [
            "ctrl+alt+space",
            "shift+cmd+r",
            "alt+F12",
            "ctrl+NumpadEnter",
            "cmd+\\",
        ] {
            let binding = Binding::parse(text).expect(text);
            assert_eq!(Binding::parse(&binding.to_string()), Ok(binding), "{text}");
        }
    }

    #[test]
    fn the_grammar_refuses_what_the_parser_refuses() {
        assert_eq!(
            Binding::parse(""),
            Err(BindingError::UnknownKey(String::new()))
        );
        assert_eq!(Binding::parse("ctrl++space"), Err(BindingError::EmptyPart));
        assert_eq!(Binding::parse("ctrl+"), Err(BindingError::EmptyPart));
        assert_eq!(Binding::parse("+"), Err(BindingError::EmptyPart));
        assert_eq!(
            Binding::parse("shift+KeyQ+alt"),
            Err(BindingError::KeyNotLast)
        );
        assert_eq!(Binding::parse("ctrl+a+b"), Err(BindingError::KeyNotLast));
        assert_eq!(Binding::parse("shift+ctrl"), Err(BindingError::NoKey));
        assert_eq!(
            Binding::parse("ctrl+banana"),
            Err(BindingError::UnknownKey("banana".into()))
        );
        // One part is read untrimmed; more are trimmed.
        assert!(Binding::parse(" space").is_err());
        assert!(Binding::parse(" ctrl + space ").is_ok());
        // Alone, `super` is a key; with others, a modifier.
        assert_eq!(Binding::parse("super").map(|b| b.key()), Ok("Super"));
        assert_eq!(
            Binding::parse("super+space").map(|b| b.modifiers()),
            Ok(Modifiers::COMMAND)
        );
    }

    #[test]
    fn the_listener_can_use_only_keys_the_mac_registers() {
        assert_eq!(validate("ctrl+r").map(|b| b.key()), Ok("KeyR"));
        assert_eq!(validate("F5").map(|b| b.key()), Ok("F5"));
        assert_eq!(validate("r"), Err(BindingError::NoModifier));
        assert_eq!(validate("space"), Err(BindingError::NoModifier));
        assert_eq!(
            validate("ctrl+BrowserBack"),
            Err(BindingError::NotOnMac("BrowserBack".into()))
        );
        assert_eq!(validate("F21"), Err(BindingError::NotOnMac("F21".into())));
        assert_eq!(
            validate("ctrl+MediaPlayPause"),
            Err(BindingError::MediaKey("MediaPlayPause".into()))
        );
    }

    #[test]
    fn a_key_press_makes_its_binding() {
        // kVK_ANSI_R with ⌘⇧ (NSEvent's raw flags, Caps Lock on as well).
        let flags = (1 << 20) | (1 << 17) | (1 << 16);
        let binding = Binding::from_mac_key(0x0f, Modifiers::from_mac_flags(flags)).expect("R");
        assert_eq!(binding.to_string(), "shift+cmd+KeyR");
        assert_eq!(
            Binding::from_mac_key(0x31, Modifiers::from_mac_flags((1 << 18) | (1 << 19))),
            Some(Binding::default_binding())
        );
        // Help and Insert share a key code; the Mac's key is Help.
        assert_eq!(
            Binding::from_mac_key(0x72, Modifiers::CONTROL).map(|b| b.key()),
            Some("Help")
        );
        assert_eq!(Binding::from_mac_key(0x3f, Modifiers::CONTROL), None);
    }

    #[test]
    fn every_mac_key_code_reads_back_as_its_key() {
        for key in KEYS {
            if let Mac::Code(code) = key.mac {
                let binding = Binding::from_mac_key(code, Modifiers::CONTROL).expect(key.code);
                assert_eq!(
                    validate(&binding.to_string()),
                    Ok(binding),
                    "{} ({code:#x})",
                    key.code
                );
            }
        }
    }

    #[test]
    fn only_a_hotkey_change_names_the_hotkey() {
        let changed = serde_json::json!({"status": "settings_changed", "setting": "hotkey"});
        assert!(changed_in("daemon_status_changed", &changed));
        assert!(!changed_in("download_progress", &changed));
        let other = serde_json::json!({"status": "settings_changed", "setting": "volume"});
        assert!(!changed_in("daemon_status_changed", &other));
    }
}
