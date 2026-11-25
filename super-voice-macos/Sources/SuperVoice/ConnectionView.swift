// SPDX-License-Identifier: GPL-3.0-only
// The Connection page: whether the daemon answers, the socket the app reaches
// it on, and what to do when it doesn't. The window shows it in place of its
// pages while the daemon can't be reached, and View › Connection shows it at
// any time. As in the settings app.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class ConnectionModel: ObservableObject {
    static let shared = ConnectionModel()

    /// What the store last said the page shows.
    @Published private(set) var page = ConnectionPage(status: .disconnected, socketPath: "", fix: nil)

    /// Whether View › Connection's sheet is up.
    @Published var sheetShown = false

    private var store: ConnectionStore!

    private init() {
        let relay = ConnectionRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = ConnectionStore(observer: relay)
        page = store.page()
        #if DEBUG
        // A shot of another page is a shot of that page, whether or not a
        // daemon answers: the connection scenes are the ones that show this.
        if Snapshot.path != nil, !Snapshot.scene.hasPrefix("connection") {
            pretend(ConnectionPage(status: .connected, socketPath: page.socketPath, fix: nil))
        }
        #endif
    }

    /// Whether the daemon can't be reached: the window shows this page in
    /// place of its own. Not before the first answer, so a daemon that
    /// answers at once never shows it.
    var unreachable: Bool {
        switch page.status {
        case .connecting, .error, .denied: true
        case .disconnected, .connected: false
        }
    }

    func send(_ action: ConnectionAction) {
        store.send(action: action)
    }

    /// Put `text` on the clipboard.
    func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    private func show(_ page: ConnectionPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
    }

    #if DEBUG
    private var pretending = false

    /// Shows `page` from now on, whatever the daemon says: a made-up scene's.
    func pretend(_ page: ConnectionPage) {
        pretending = true
        self.page = page
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ConnectionRelay: ConnectionObserver {
    let onChange: @Sendable (ConnectionPage) -> Void

    init(onChange: @escaping @Sendable (ConnectionPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: ConnectionPage) {
        onChange(page)
    }
}

/// The page, filling the window while the daemon can't be reached.
struct ConnectionView: View {
    @ObservedObject var model: ConnectionModel

    var body: some View {
        Form {
            ConnectionRows(model: model)
        }
        .formStyle(.grouped)
        // Clear of the window's toolbar line, as the other pages' cards are.
        .contentMargins(.top, 24, for: .scrollContent)
        .navigationTitle(t("connection"))
    }
}

/// View › Connection: the same rows, in a sheet over the window.
struct ConnectionSheet: View {
    @ObservedObject var model: ConnectionModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(t("connection"))
                .font(.title2.bold())
                .padding([.top, .horizontal], 20)
            Form {
                ConnectionRows(model: model)
            }
            .formStyle(.grouped)
            HStack {
                Spacer()
                Button(t("done")) { model.sheetShown = false }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent)
            }
            .padding([.bottom, .horizontal], 20)
        }
        .frame(width: 560)
    }
}

/// The connection's rows, as the page and the sheet both show them.
struct ConnectionRows: View {
    @ObservedObject var model: ConnectionModel

    var body: some View {
        let page = model.page
        Section {
            LabeledContent(t("status")) {
                Text(page.status.words)
                    .textSelection(.enabled)
            }
            LabeledContent(t("socket-path")) {
                Text(page.socketPath)
                    .textSelection(.enabled)
            }
            switch page.fix {
            case nil:
                EmptyView()
            case let .start(command)?:
                LabeledContent(t("no-daemon")) {
                    Text(t("the-daemon-is-not-running"))
                }
                LabeledContent(t("start-it")) {
                    HStack {
                        Text(command)
                            .font(.body.monospaced())
                            .textSelection(.enabled)
                        Button(t("copy")) { model.copy(command) }
                    }
                }
            case .turnOn?:
                LabeledContent(t("no-daemon")) {
                    Text(t("super-voice-s-background-service-is-switched"))
                }
                LabeledContent(t("turn-it-on")) {
                    Button(t("open-login-items")) { model.send(.openLoginItems) }
                }
            case .register?:
                LabeledContent(t("no-daemon")) {
                    Text(t("super-voice-s-background-service-is-not"))
                }
                LabeledContent(t("register-it")) {
                    Button(t("register")) { model.send(.register) }
                }
            case let .reauthorize(restart)?:
                LabeledContent(t("action-required")) {
                    Text(t("authorization-denied-restart", ["command": restart]))
                        .textSelection(.enabled)
                }
                HStack {
                    Spacer()
                    Button(t("retry-authorization")) { model.send(.retryAuthorization) }
                }
            }
        }
    }
}

extension ConnectionStatus {
    /// What the Status row says.
    var words: String {
        switch self {
        case .connected: t("connected")
        case .connecting: t("connecting")
        case .disconnected: t("disconnected")
        case let .error(message): t("connection-error", ["err": message])
        case let .denied(reason): t("connection-denied", ["reason": reason])
        }
    }
}
