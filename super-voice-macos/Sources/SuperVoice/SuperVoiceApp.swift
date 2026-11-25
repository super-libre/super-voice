// SPDX-License-Identifier: GPL-3.0-only
// The SwiftUI app for macOS: one window, laid out like the settings app on
// Linux, with its pages arriving one at a time.

import AppKit
import SuperVoiceCore
import SwiftUI
import UserNotifications

/// Where the process starts: a banner for the daemon (`--notify`), which is
/// posted without the app ever opening a window, or the app.
@main
enum Entry {
    @MainActor
    static func main() {
        if Notifier.requested {
            Notifier.run()
        }
        SuperVoiceApp.main()
    }
}

struct SuperVoiceApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        chooseLanguages()
    }

    var body: some Scene {
        WindowGroup("Super Voice") {
            ContentView(notes: NotesModel.shared)
                .frame(minWidth: 720, minHeight: 480)
        }
        .commands {
            CommandGroup(replacing: .appInfo) {
                Button(t("about-super-voice")) { showAbout() }
            }
            // The View menu, after the sidebar's own items.
            CommandGroup(after: .sidebar) {
                Button(t("connection")) { ConnectionModel.shared.sheetShown = true }
            }
        }
    }
}

/// The app opening and quitting.
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Opened from its bundle, the app is what installs the daemon: it
    /// registers the bundle's background services, on every launch. It asks to
    /// show notifications too, since the daemon's failure banners are posted
    /// as this app (`--notify`): asked now, the prompt comes up while the user
    /// is looking at Super Voice, not the first time a dictation fails. Once
    /// they have answered, macOS answers for them.
    func applicationDidFinishLaunching(_ notification: Notification) {
        registerLaunchAgents()
        // Outside a bundle Notification Center raises an exception rather
        // than answering.
        if Bundle.main.bundleIdentifier != nil {
            UNUserNotificationCenter.current().requestAuthorization(options: [.alert]) { _, _ in }
        }
        MainActor.assumeIsolated {
            // For "Update available" in the header, from the start.
            UpdatesModel.shared.send(.reload)
        }
    }

    /// The notes not yet written are written first.
    func applicationWillTerminate(_ notification: Notification) {
        MainActor.assumeIsolated {
            NotesModel.shared.saveBeforeExit()
        }
    }
}

/// The About window: the app's name, version and icon, as macOS shows them,
/// then the repository and the commit the app was built from, each a link, as
/// in the settings app.
@MainActor
func showAbout() {
    let info = about()
    let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
    let centered = NSMutableParagraphStyle()
    centered.alignment = .center
    func line(_ words: String, linking address: String) -> NSAttributedString {
        var attributes: [NSAttributedString.Key: Any] = [.font: font, .paragraphStyle: centered]
        if let url = URL(string: address) {
            attributes[.link] = url
        }
        return NSAttributedString(string: words, attributes: attributes)
    }
    let credits = NSMutableAttributedString()
    credits.append(line(info.repository, linking: info.repository))
    credits.append(NSAttributedString(string: "\n", attributes: [.font: font, .paragraphStyle: centered]))
    credits.append(line(
        t("git-commit", ["short_hash": info.shortCommit, "date": info.commitDate]),
        linking: info.commitUrl
    ))
    NSApp.orderFrontStandardAboutPanel(options: [
        .applicationName: t("app-title"),
        .credits: credits,
    ])
    NSApp.activate()
}

/// `SuperVoice --notify <title> <body>`: post one banner to Notification
/// Center as Super Voice, then exit. The daemon runs this; see
/// `super-voice-daemon/src/output/notification_center.rs`.
///
/// Notification Center serves only an app bundle's own executable, and
/// refuses the daemon beside it in the bundle. So the daemon hands each banner
/// to a short-lived process of this executable. That process presents itself
/// as an application, or Notification Center refuses it too, with no Dock
/// icon, no window and no event loop; posts; and exits: 0 once Notification
/// Center has the banner, 1 with the reason on stderr when it refused, 2 for
/// a malformed command line. As the settings app's `--notify`.
@MainActor
enum Notifier {
    /// The flag that selects this mode. The daemon passes the same string.
    static let flag = "--notify"

    /// The identifier every banner is posted under. Reusing one replaces the
    /// banner already showing instead of stacking a new one beside it.
    private static let requestID = "super-stt"

    /// How long to wait for each answer from Notification Center. The daemon
    /// waits on this process to learn whether to fall back to typing, so a
    /// permission request the user hasn't answered isn't waited out.
    private static let replyTimeout: DispatchTimeInterval = .seconds(5)

    /// Whether this process was started in notify mode.
    static var requested: Bool {
        CommandLine.arguments.count > 1 && CommandLine.arguments[1] == flag
    }

    static func run() -> Never {
        let args = Array(CommandLine.arguments.dropFirst(2))
        guard args.count == 2 else {
            FileHandle.standardError.write(Data("usage: SuperVoice \(flag) <title> <body>\n".utf8))
            exit(2)
        }
        let app = NSApplication.shared
        app.setActivationPolicy(.prohibited)
        app.finishLaunching()
        do {
            try post(title: args[0], body: args[1])
        } catch {
            FileHandle.standardError.write(Data("\(error)\n".utf8))
            exit(1)
        }
        exit(0)
    }

    private struct Refused: Error, CustomStringConvertible {
        let description: String
    }

    private static func post(title: String, body: String) throws {
        let center = UNUserNotificationCenter.current()
        let status: UNAuthorizationStatus = try wait("reading notification settings") { done in
            center.getNotificationSettings { settings in done(settings.authorizationStatus) }
        }
        switch status {
        case .denied:
            throw Refused(
                description: "notifications from Super Voice are turned off in System Settings › Notifications"
            )
        case .notDetermined:
            // The app asks when it opens, so this is reached only by a banner
            // posted before Super Voice was ever opened.
            let answer: Result<Bool, any Error> = try wait("asking to show notifications") { done in
                center.requestAuthorization(options: [.alert]) { granted, error in
                    if let error {
                        done(.failure(error))
                    } else {
                        done(.success(granted))
                    }
                }
            }
            if try !answer.get() {
                throw Refused(description: "notifications from Super Voice were not allowed")
            }
        default:
            break
        }
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        let request = UNNotificationRequest(identifier: requestID, content: content, trigger: nil)
        let posted: Result<Void, any Error> = try wait("posting the notification") { done in
            center.add(request) { error in
                if let error {
                    done(.failure(error))
                } else {
                    done(.success(()))
                }
            }
        }
        try posted.get()
    }

    /// The answer a completion handler gives, waited for up to
    /// `replyTimeout`. Notification Center answers on a queue of its own, so
    /// blocking this thread doesn't hold it up.
    private static func wait<T: Sendable>(
        _ doing: String,
        _ start: (@escaping @Sendable (T) -> Void) -> Void
    ) throws -> T {
        let answer = Answer<T>()
        start { value in answer.set(value) }
        guard answer.semaphore.wait(timeout: .now() + replyTimeout) == .success, let value = answer.value else {
            throw Refused(description: "Notification Center did not answer while \(doing)")
        }
        return value
    }
}

/// One answer from another thread, and the signal that it came.
private final class Answer<T: Sendable>: @unchecked Sendable {
    let semaphore = DispatchSemaphore(value: 0)
    private let lock = NSLock()
    private var stored: T?

    var value: T? {
        lock.withLock { stored }
    }

    func set(_ value: T) {
        lock.withLock { stored = value }
        semaphore.signal()
    }
}
