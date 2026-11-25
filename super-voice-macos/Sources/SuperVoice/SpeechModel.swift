// SPDX-License-Identifier: GPL-3.0-only
// The speech test's model: the core's store underneath, the test it last
// drew, and the text field.

import SuperVoiceCore
import SwiftUI

@MainActor
final class SpeechModel: ObservableObject {
    static let shared = SpeechModel()

    /// What the store last said the test shows.
    @Published private(set) var page = SpeechPage(
        speaking: false, playing: false, level: 0, canSpeak: false, notice: nil
    )

    /// The test field. The store gets each change and never replaces it.
    @Published var text = ""

    private var store: SpeechStore!

    private init() {
        let relay = SpeechRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = SpeechStore(observer: relay)
    }

    func send(_ action: SpeechAction) {
        store.send(action: action)
    }

    /// The test field changed.
    func textChanged(_ text: String) {
        store.setText(text: text)
    }

    private func show(_ page: SpeechPage) {
        self.page = page
        #if DEBUG
        Snapshot.speechShown(page)
        #endif
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class SpeechRelay: SpeechObserver {
    let onChange: @Sendable (SpeechPage) -> Void

    init(onChange: @escaping @Sendable (SpeechPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: SpeechPage) {
        onChange(page)
    }
}
