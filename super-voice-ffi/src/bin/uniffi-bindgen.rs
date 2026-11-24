// SPDX-License-Identifier: GPL-3.0-only
//! Generates the bindings for another language from the built library:
//! `uniffi-bindgen generate --library <libsuper_voice_ffi> --language swift`.

fn main() {
    uniffi::uniffi_bindgen_main();
}
