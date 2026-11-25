// SPDX-License-Identifier: GPL-3.0-only
// The Shortcuts page: each of Super Voice's keyboard shortcuts, where it
// stands on this computer, and a button to add or remove it. The daemon owns
// the shortcuts; the page shows what it answers. As in the settings app.

import SuperVoiceCore
import SwiftUI

@MainActor
final class ShortcutsModel: ObservableObject {
    static let shared = ShortcutsModel()

    @Published private(set) var page = ShortcutsPage(loaded: false, shortcuts: [], notice: nil)

    private var store: ShortcutsStore!

    private init() {
        let relay = ShortcutsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = ShortcutsStore(observer: relay)
    }

    func send(_ action: ShortcutsAction) {
        store.send(action: action)
    }

    private func show(_ page: ShortcutsPage) {
        self.page = page
        #if DEBUG
        Snapshot.shortcutsShown(page)
        #endif
    }
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ShortcutsRelay: ShortcutsObserver {
    let onChange: @Sendable (ShortcutsPage) -> Void

    init(onChange: @escaping @Sendable (ShortcutsPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: ShortcutsPage) {
        onChange(page)
    }
}

struct ShortcutsView: View {
    @ObservedObject var model: ShortcutsModel

    var body: some View {
        let page = model.page
        VStack(spacing: 0) {
            if let notice = page.notice {
                NoticeBanner(text: notice)
            }
            Form {
                if page.loaded {
                    Section {
                        ForEach(page.shortcuts, id: \.shortcut) { line in
                            ShortcutRowView(model: model, line: line)
                        }
                    } header: {
                        Text(t("keyboard-shortcuts"))
                    }
                }
            }
            .formStyle(.grouped)
        }
        .navigationTitle(t("shortcuts"))
        .onAppear { model.send(.refresh) }
    }
}

/// One shortcut: what it does and where it stands, with its keys once it is
/// set up and Remove where the daemon can remove it, or Add.
struct ShortcutRowView: View {
    @ObservedObject var model: ShortcutsModel
    let line: ShortcutLine

    var body: some View {
        let kind = line.shortcut
        switch line.row {
        case let .removable(keys, changedIn):
            LabeledContent {
                HStack(spacing: 12) {
                    Text(keys)
                    Button(t("remove")) { model.send(.remove(shortcut: kind)) }
                }
            } label: {
                Labelled(kind.name, kind.does + " " + changedIn.words)
            }
        case let .added(keys):
            LabeledContent {
                HStack(spacing: 12) {
                    Text(keys)
                    // Shown greyed out, as where it stands.
                    Button(t("added")) {}
                        .disabled(true)
                }
            } label: {
                Labelled(kind.name, kind.does + " " + t("shortcut-change-in-desktop"))
            }
        case let .missing(keys):
            LabeledContent {
                Button(t("add-shortcut")) { model.send(.add(shortcut: kind)) }
            } label: {
                Labelled(kind.name, kind.does + " " + t("shortcut-missing", ["keys": keys]))
            }
        case let .taken(keys, command):
            Labelled(kind.name, kind.does + " " + t("shortcut-taken", ["keys": keys, "command": command]))
        case let .elsewhere(command):
            Labelled(kind.name, kind.does + " " + t("shortcut-elsewhere", ["command": command]))
        }
    }
}

extension ShortcutKind {
    var name: String {
        switch self {
        case .dictate: t("shortcut-dictate")
        case .readAloud: t("shortcut-read-aloud")
        }
    }

    var does: String {
        switch self {
        case .dictate: t("shortcut-dictate-description")
        case .readAloud: t("shortcut-read-aloud-description")
        }
    }
}

extension ShortcutChangedIn {
    var words: String {
        switch self {
        case .cosmic: t("shortcut-set-up")
        case .gnome: t("shortcut-set-up-gnome")
        case .desktop: t("shortcut-set-up-desktop")
        }
    }
}
