// SPDX-License-Identifier: GPL-3.0-only
// The Contexts page's model: the core's store underneath, the page it last
// drew, and the editor's own fields.

import SuperVoiceCore
import SwiftUI

@MainActor
final class ContextsModel: ObservableObject {
    static let shared = ContextsModel()

    /// What the store last said the page shows.
    @Published private(set) var page = ContextsPage(
        loaded: false, contexts: [], active: nil, confirmingDelete: nil, editor: nil,
        focusRow: nil, focusAsked: 0, notice: nil, reach: nil
    )

    /// The editor's fields. Taken from the store when the editor opens, and
    /// the rows again whenever the store adds, removes or splits one; typing
    /// goes the other way, to the store.
    @Published var name = ""
    @Published var prompt = ""
    @Published var terms: [TermRow] = []

    private var store: ContextsStore!
    /// The context the editor is open over, so a new opening is told from a
    /// change to the open one.
    private var editing: String?

    private init() {
        let relay = ContextsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = ContextsStore(observer: relay)
    }

    func send(_ action: ContextsAction) {
        store.send(action: action)
    }

    /// The prompt changed.
    func promptChanged(_ text: String) {
        store.setPrompt(text: text)
    }

    private func show(_ page: ContextsPage) {
        if let editor = page.editor {
            if editing != editor.id {
                editing = editor.id
                name = editor.name
                prompt = editor.prompt
                terms = editor.terms
            } else if editor.terms.map(\.id) != terms.map(\.id) {
                terms = editor.terms
            }
        } else {
            editing = nil
        }
        self.page = page
        #if DEBUG
        Snapshot.contextsShown(page)
        #endif
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ContextsRelay: ContextsObserver {
    let onChange: @Sendable (ContextsPage) -> Void

    init(onChange: @escaping @Sendable (ContextsPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: ContextsPage) {
        onChange(page)
    }
}
