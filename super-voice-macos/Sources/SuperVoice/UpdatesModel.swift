// SPDX-License-Identifier: GPL-3.0-only
// The Updates page's model: the core's store underneath, and the page it last
// drew.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class UpdatesModel: ObservableObject {
    static let shared = UpdatesModel()

    /// What the store last said the page shows.
    @Published private(set) var page = UpdatesPage(
        loaded: false, unsupported: false, latestVersion: nil, checkedAt: nil, lastCheckError: nil,
        check: CheckNow(enabled: false, checking: false), autoCheck: nil, beta: nil, panel: nil,
        notice: nil, headerBadge: false
    )

    /// A relaunch that didn't start: why.
    @Published private(set) var relaunchFailed = false

    private var store: UpdatesStore!

    private init() {
        let relay = UpdatesRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = UpdatesStore(observer: relay)
    }

    func send(_ action: UpdatesAction) {
        store.send(action: action)
    }

    /// Start a new copy of the app, then quit this one: the update replaced
    /// it on disk.
    func restart() {
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: Bundle.main.bundleURL, configuration: configuration) { _, error in
            Task { @MainActor in
                if error == nil {
                    NSApp.terminate(nil)
                } else {
                    self.relaunchFailed = true
                }
            }
        }
    }

    private func show(_ page: UpdatesPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
        #if DEBUG
        Snapshot.updatesShown(page)
        #endif
    }

    #if DEBUG
    private var pretending = false

    /// Shows `page` from now on, whatever the daemon says: a made-up scene's.
    func pretend(_ page: UpdatesPage) {
        pretending = true
        self.page = page
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class UpdatesRelay: UpdatesObserver {
    let onChange: @Sendable (UpdatesPage) -> Void

    init(onChange: @escaping @Sendable (UpdatesPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: UpdatesPage) {
        onChange(page)
    }
}
