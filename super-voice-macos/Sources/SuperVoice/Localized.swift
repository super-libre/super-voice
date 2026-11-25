// SPDX-License-Identifier: GPL-3.0-only
// The app's words, from the same Fluent catalogs as the settings app's, in
// super-voice-app-core/i18n.

import Foundation
import SuperVoiceCore

/// The message `key` in the app's language, with `args` filled in.
func t(_ key: String, _ args: [String: String] = [:]) -> String {
    args.isEmpty ? text(key: key) : textWith(key: key, args: args)
}

/// The locale dates and times are written in: the user's, or under
/// `SUPER_VOICE_LANGUAGE` that language's, so they match the words around
/// them.
@MainActor var appLocale = Locale.current

/// Use the user's languages, in their order of preference. A debug build takes
/// `SUPER_VOICE_LANGUAGE` instead, so a screenshot can be taken in any of them.
@MainActor func chooseLanguages() {
    var languages = Locale.preferredLanguages
    #if DEBUG
    if let chosen = ProcessInfo.processInfo.environment["SUPER_VOICE_LANGUAGE"] {
        languages = [chosen]
        appLocale = Locale(identifier: chosen)
    }
    #endif
    setLanguages(languages: languages)
}
