// SPDX-License-Identifier: GPL-3.0-only
// The Shortcuts page, on the Mac: the keys that start and stop dictation, a
// way to change them and to go back to ⌃⌥Space, and whether the shortcut's
// background service is set to run.
//
// A Mac's desktop binds no keys to a command, so the shortcut is the app
// bundle's listener (`voice hotkey`), which listens for the keys the daemon
// keeps. Changing them here changes the daemon's setting, and the listener
// follows at once. The settings app's rows on Linux (the desktop's shortcuts)
// have nothing to show here.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class ShortcutsModel: ObservableObject {
    static let shared = ShortcutsModel()

    @Published private(set) var page = ShortcutsPage(
        loaded: false, shortcuts: [], hotkey: nil, listener: nil, notice: nil
    )
    /// Whether the next key press is being recorded as the new keys.
    @Published private(set) var recording = false
    /// What the row says while recording: what to press, or why the last
    /// press was not taken.
    @Published private(set) var recordingHint = ""

    private var store: ShortcutsStore!
    private var monitor: Any?

    private init() {
        let relay = ShortcutsRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = ShortcutsStore(observer: relay)
    }

    func send(_ action: ShortcutsAction) {
        store.send(action: action)
    }

    /// Record the next key press as the new keys. Every key press comes here
    /// until one is taken, or Esc (or Cancel) stops it.
    func startRecording() {
        guard monitor == nil else { return }
        recording = true
        recordingHint = t("shortcut-press-keys")
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            let keyCode = event.keyCode
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            // Local monitors are called on the main thread.
            MainActor.assumeIsolated {
                self?.record(keyCode: keyCode, flags: flags)
            }
            // Swallowed: a key pressed to record is not typed anywhere.
            return nil
        }
    }

    func stopRecording() {
        if let monitor {
            NSEvent.removeMonitor(monitor)
        }
        monitor = nil
        recording = false
        recordingHint = ""
    }

    /// A key press while recording: `keyCode` is the key (`kVK_*`), `flags`
    /// the modifiers held.
    private func record(keyCode: UInt16, flags: NSEvent.ModifierFlags) {
        // Esc alone cancels; with a modifier it can be the shortcut.
        if keyCode == 53, flags.intersection([.command, .option, .control, .shift]).isEmpty {
            stopRecording()
            return
        }
        switch shortcutKeyPress(keyCode: keyCode, modifierFlags: UInt64(flags.rawValue)) {
        case let .keys(binding, _):
            stopRecording()
            send(.changeHotkey(binding: binding))
        case .needsModifier:
            recordingHint = t("shortcut-needs-modifier")
        case .unusable:
            recordingHint = t("shortcut-key-unusable")
        }
    }

    private func show(_ page: ShortcutsPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
        #if DEBUG
        Snapshot.shortcutsShown(page)
        #endif
    }

    #if DEBUG
    private var pretending = false

    /// Shows `page` from now on, whatever the store says: a made-up scene's.
    func pretend(_ page: ShortcutsPage) {
        pretending = true
        self.page = page
    }
    #endif
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
                if let hotkey = page.hotkey {
                    Section {
                        DictateRow(model: model, hotkey: hotkey)
                        if let listener = page.listener {
                            ListenerRow(model: model, listener: listener)
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
        .onDisappear { model.stopRecording() }
        // Back from System Settings, where the service may have been switched
        // on.
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            model.send(.refresh)
        }
    }
}

/// Dictate: its keys as a Mac shows them, Change, which records the next key
/// press, and Reset, back to ⌃⌥Space.
struct DictateRow: View {
    @ObservedObject var model: ShortcutsModel
    let hotkey: HotkeyKeys

    var body: some View {
        LabeledContent {
            HStack(spacing: 12) {
                if model.recording {
                    Text(model.recordingHint)
                        .foregroundStyle(.secondary)
                    Button(t("cancel")) { model.stopRecording() }
                } else {
                    KeyCaps(symbols: hotkey.symbols)
                    Button(t("shortcut-change")) { model.startRecording() }
                    Button(t("reset")) { model.send(.resetHotkey) }
                        .disabled(hotkey.isDefault)
                        .help(defaultHotkeySymbols())
                }
            }
        } label: {
            Labelled(t("shortcut-dictate"), t("shortcut-dictate-description"))
        }
    }
}

/// A shortcut's keys, as a Mac shows them in a menu: `⌃⌥Space`.
struct KeyCaps: View {
    let symbols: String

    var body: some View {
        Text(symbols)
            .font(.body.monospaced())
            .padding(.vertical, 3)
            .padding(.horizontal, 8)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: .quaternarySystemFill)))
            .accessibilityLabel(t("shortcut-dictate") + ", " + symbols)
    }
}

/// Whether the shortcut's background service is set to run, and what fixes
/// it when it isn't: a dot (green when it runs) and a sentence, with the one
/// button that fixes it.
struct ListenerRow: View {
    @ObservedObject var model: ShortcutsModel
    let listener: ShortcutListener

    var body: some View {
        switch listener {
        case .running:
            ListenerState(color: .systemGreen, words: t("shortcut-listener-running"))
        case .notRegistered:
            LabeledContent {
                Button(t("register")) { model.send(.registerListener) }
            } label: {
                ListenerState(color: .systemGray, words: t("shortcut-listener-not-registered"))
            }
        case .switchedOff:
            LabeledContent {
                Button(t("open-login-items")) { model.send(.openLoginItems) }
            } label: {
                ListenerState(color: .systemGray, words: t("shortcut-listener-switched-off"))
            }
        case .notInBundle:
            ListenerState(color: .systemGray, words: t("shortcut-listener-not-in-bundle"))
        }
    }
}

/// The service's state: a dot in its colour, and what it means.
struct ListenerState: View {
    let color: NSColor
    let words: String

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(Color(nsColor: color))
                .frame(width: 8, height: 8)
                .accessibilityHidden(true)
            Text(words)
                .foregroundStyle(.secondary)
        }
    }
}
