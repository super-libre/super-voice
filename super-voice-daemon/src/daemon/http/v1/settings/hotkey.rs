// SPDX-License-Identifier: GPL-3.0-only
//! `/settings/hotkey` — the keys the macOS shortcut listener listens for.
//!
//! The listener (`voice hotkey`, the app bundle's second `LaunchAgent`) reads
//! this at start and again whenever `settings_changed` names `hotkey`, and
//! swaps the shortcut it holds. Linux has no listener: there the desktop
//! binds `voice record --write`, through `/shortcuts`.

use super::super::wire::HotkeyState;

settings_dispatch!(
    get_hotkey,
    "get_hotkey",
    get "/settings/hotkey",
    HotkeyState,
    "Read the macOS shortcut's keys",
    "Answers with the binding the macOS shortcut listener listens for: the one set, \
or `ctrl+alt+space` (⌃⌥Space) when none is."
);
settings_setter!(
    set_hotkey,
    SetHotkeyBody { hotkey: String },
    "set_hotkey",
    "hotkey",
    "/settings/hotkey",
    HotkeyState,
    "Set the macOS shortcut's keys",
    "Sets the binding the macOS shortcut listener listens for, which starts and stops \
dictation. The listener follows the change at once.\n\nA binding is modifiers and one key \
joined by `+`, modifiers first: `ctrl`, `alt`, `shift`, `cmd`, then a key by its \
`keyboard-types` `Code` name (`Space`, `KeyR`, `Digit1`, `F5`) or its short form (`r`, `1`). \
One the listener could not register is refused with `400 invalid_value` and the reason: a \
key a Mac has no key code for, a media key, or a key that types something held with no \
modifier (only an F key may go alone). The answer is the binding as stored, written the one \
way the daemon writes it (`shift+cmd+KeyR`).",
    "The binding, such as `ctrl+alt+space` or `cmd+shift+KeyR`.",
);
settings_dispatch!(
    clear_hotkey,
    "clear_hotkey",
    delete "/settings/hotkey",
    HotkeyState,
    "Reset the macOS shortcut's keys",
    "Goes back to the default binding, `ctrl+alt+space` (⌃⌥Space)."
);
