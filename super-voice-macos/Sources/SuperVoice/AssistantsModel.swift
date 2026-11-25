// SPDX-License-Identifier: GPL-3.0-only
// The Assistants page's model: the core's store underneath, the page it last
// drew, and the key's name field.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class AssistantsModel: ObservableObject {
    static let shared = AssistantsModel()

    /// What the store last said the page shows.
    @Published private(set) var page = AssistantsPage(
        loaded: false, assistants: [], keys: [], open: nil, connect: nil, key: nil, notice: nil
    )

    /// The key dialog's name field. The store gets each change.
    @Published var keyName = ""

    private var store: AssistantsStore!

    private init() {
        let relay = AssistantsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = AssistantsStore(observer: relay)
    }

    func send(_ action: AssistantsAction) {
        store.send(action: action)
    }

    /// Put `text` on the clipboard.
    func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private func show(_ page: AssistantsPage) {
        #if DEBUG
        if pretending { return }
        #endif
        // A dialog that opens starts with an empty name.
        if self.page.key == nil, page.key != nil {
            keyName = ""
        }
        self.page = page
        #if DEBUG
        Snapshot.assistantsShown(page)
        #endif
    }

    #if DEBUG
    private var pretending = false

    /// Shows `page` from now on, whatever the store says: a made-up scene's.
    func pretend(_ page: AssistantsPage) {
        pretending = true
        self.page = page
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class AssistantsRelay: AssistantsObserver {
    let onChange: @Sendable (AssistantsPage) -> Void

    init(onChange: @escaping @Sendable (AssistantsPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: AssistantsPage) {
        onChange(page)
    }
}
