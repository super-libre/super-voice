// SPDX-License-Identifier: GPL-3.0-only
// The Notes page's model: the core's store underneath, the page it last drew,
// and the open note's editor text.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class NotesModel: ObservableObject {
    /// The one model, which the app's delegate saves on quit.
    static let shared = NotesModel()

    /// What the store last said the page shows.
    @Published private(set) var page = NotesPage(
        loaded: false, notes: [], open: nil, take: nil,
        confirmingDelete: false, exported: nil, notice: nil, folder: notesFolder()
    )

    /// The open note's editor. The store gets each change, and replaces it
    /// only when it dictated words into the note.
    @Published var text = ""

    private var store: NotesStore!
    private var shown: (id: String, replaced: UInt64)?

    private init() {
        let relay = NotesRelay(
            onChange: { [weak self] page in Task { @MainActor in self?.show(page) } },
            onExport: { [weak self] name, text in
                Task { @MainActor in self?.askWhereToExport(name: name, text: text) }
            }
        )
        store = NotesStore(observer: relay)
    }

    func send(_ action: NotesAction) {
        store.send(action: action)
    }

    /// The editor's text changed.
    func edited(_ text: String) {
        store.setText(text: text)
    }

    func saveBeforeExit() {
        store.saveBeforeExit()
    }

    private func show(_ page: NotesPage) {
        self.page = page
        if let open = page.open {
            if shown?.id != open.id || shown?.replaced != open.replaced {
                shown = (open.id, open.replaced)
                text = open.text
            }
        } else {
            shown = nil
        }
        #if DEBUG
        Snapshot.shown(page, model: self)
        #endif
    }

    #if DEBUG
    /// Shows `page` as if the store had sent it: the take scenes' made-up
    /// take.
    func pretend(_ page: NotesPage) {
        self.page = page
    }
    #endif

    private func askWhereToExport(name: String, text: String) {
        let panel = NSSavePanel()
        panel.title = t("export-note")
        panel.nameFieldStringValue = name
        panel.directoryURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        let store = store!
        panel.begin { answer in
            let path = answer == .OK ? panel.url?.path : nil
            store.exportTo(path: path, text: text)
        }
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class NotesRelay: NotesObserver {
    let onChange: @Sendable (NotesPage) -> Void
    let onExport: @Sendable (String, String) -> Void

    init(
        onChange: @escaping @Sendable (NotesPage) -> Void,
        onExport: @escaping @Sendable (String, String) -> Void
    ) {
        self.onChange = onChange
        self.onExport = onExport
    }

    func changed(page: NotesPage) {
        onChange(page)
    }

    func exportRequested(name: String, text: String) {
        onExport(name, text)
    }
}
