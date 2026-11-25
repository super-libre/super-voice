// SPDX-License-Identifier: GPL-3.0-only
// The Settings page's model: the core's store underneath, and the page it
// last drew.

import SuperVoiceCore
import SwiftUI

@MainActor
final class SettingsModel: ObservableObject {
    static let shared = SettingsModel()

    /// What the store last said the page shows.
    @Published private(set) var page = SettingsPage(
        loaded: false, primaryLanguage: nil, primaryLanguageName: "", notificationMethods: [],
        notificationMethod: "", sleep: false, idleChoices: [], idleUnloadSecs: 0, notice: nil
    )

    private var store: SettingsStore!

    private init() {
        let relay = SettingsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = SettingsStore(observer: relay)
    }

    func send(_ action: SettingsAction) {
        store.send(action: action)
    }

    /// The language picker's rows for what the search holds.
    func languageRows(_ query: String) -> [Choice] {
        store.languageRows(query: query)
    }

    private func show(_ page: SettingsPage) {
        self.page = page
        #if DEBUG
        Snapshot.settingsShown(page)
        #endif
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class SettingsRelay: SettingsObserver {
    let onChange: @Sendable (SettingsPage) -> Void

    init(onChange: @escaping @Sendable (SettingsPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: SettingsPage) {
        onChange(page)
    }
}
