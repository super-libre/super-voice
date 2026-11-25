// SPDX-License-Identifier: GPL-3.0-only
// The Clips page's model: the core's store underneath, the page it last drew,
// and the next clip's editor text.

import AppKit
import SuperVoiceCore
import SwiftUI
import UniformTypeIdentifiers

@MainActor
final class ClipsModel: ObservableObject {
    static let shared = ClipsModel()

    /// What the store last said the page shows.
    @Published private(set) var page = ClipsPage(
        loaded: false, clips: [], draft: "", draftWords: 0, draftReplaced: 0, making: false,
        playing: nil, playingPosition: nil, playingLength: nil, confirmingDelete: nil,
        exportedId: nil, exportedPath: nil, notice: nil
    )

    /// The next clip's editor. The store gets each change, and replaces it
    /// only to empty it once its clip is made.
    @Published var draft = ""

    /// When the clip being made was started, for the card's clock.
    @Published private(set) var makingSince: Date?

    private var store: ClipsStore!
    private var replaced: UInt64 = 0

    private init() {
        let relay = ClipsRelay(
            onChange: { [weak self] page in Task { @MainActor in self?.show(page) } },
            onExport: { [weak self] id, name in
                Task { @MainActor in self?.askWhereToExport(id: id, name: name) }
            }
        )
        store = ClipsStore(observer: relay)
    }

    func send(_ action: ClipsAction) {
        store.send(action: action)
    }

    /// The draft's text changed.
    func draftChanged(_ text: String) {
        store.setDraft(text: text)
    }

    private func show(_ page: ClipsPage) {
        if page.making, !self.page.making {
            makingSince = Date()
        } else if !page.making {
            makingSince = nil
        }
        self.page = page
        if page.draftReplaced != replaced {
            replaced = page.draftReplaced
            draft = page.draft
        }
        #if DEBUG
        Snapshot.clipsShown(page)
        #endif
    }

    private func askWhereToExport(id: String, name: String) {
        let panel = NSSavePanel()
        panel.title = t("export-clip")
        panel.nameFieldStringValue = name
        panel.allowedContentTypes = [.wav]
        panel.directoryURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        let store = store!
        panel.begin { answer in
            store.exportTo(id: id, path: answer == .OK ? panel.url?.path : nil)
        }
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ClipsRelay: ClipsObserver {
    let onChange: @Sendable (ClipsPage) -> Void
    let onExport: @Sendable (String, String) -> Void

    init(
        onChange: @escaping @Sendable (ClipsPage) -> Void,
        onExport: @escaping @Sendable (String, String) -> Void
    ) {
        self.onChange = onChange
        self.onExport = onExport
    }

    func changed(page: ClipsPage) {
        onChange(page)
    }

    func exportRequested(id: String, name: String) {
        onExport(id, name)
    }
}
