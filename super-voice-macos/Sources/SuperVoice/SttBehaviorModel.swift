// SPDX-License-Identifier: GPL-3.0-only
// Speech to text's Behavior tab's model: the core's store underneath, the tab
// it last drew, and the typing test's field.

import SuperVoiceCore
import SwiftUI

@MainActor
final class SttBehaviorModel: ObservableObject {
    static let shared = SttBehaviorModel()

    /// What the store last said the tab shows.
    @Published private(set) var page = BehaviorPage(
        loaded: false, stopModes: [], stopMode: "", previewTyping: false, writeMethods: [],
        writeMethod: "", typedWith: nil, typingTestText: "", typingTestReplaced: 0,
        typingCountdown: nil, soundsOn: false, themes: [], theme: "", volume: 100, notice: nil
    )

    /// The typing test's field. The store gets each change, and empties it
    /// when a test starts; the daemon types into it.
    @Published var typingText = ""

    /// The volume slider while it is dragged, as the store has it otherwise.
    @Published var volume: Double = 100

    /// Asked for each time the store wants the cursor in the typing field.
    @Published private(set) var focusAsked = 0

    private var store: BehaviorStore!
    private var replaced: UInt64 = 0

    private init() {
        let relay = BehaviorRelay(
            onChange: { [weak self] page in Task { @MainActor in self?.show(page) } },
            onFocus: { [weak self] in Task { @MainActor in self?.focusAsked += 1 } }
        )
        store = BehaviorStore(observer: relay)
    }

    func send(_ action: BehaviorAction) {
        store.send(action: action)
    }

    private func show(_ page: BehaviorPage) {
        if page.volume != self.page.volume || !self.page.loaded {
            volume = Double(page.volume)
        }
        self.page = page
        if page.typingTestReplaced != replaced {
            replaced = page.typingTestReplaced
            typingText = page.typingTestText
        }
        #if DEBUG
        Snapshot.sttBehaviorShown(page)
        #endif
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class BehaviorRelay: BehaviorObserver {
    let onChange: @Sendable (BehaviorPage) -> Void
    let onFocus: @Sendable () -> Void

    init(onChange: @escaping @Sendable (BehaviorPage) -> Void, onFocus: @escaping @Sendable () -> Void) {
        self.onChange = onChange
        self.onFocus = onFocus
    }

    func changed(page: BehaviorPage) {
        onChange(page)
    }

    func focusTypingField() {
        onFocus()
    }
}
