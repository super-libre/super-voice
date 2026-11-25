// SPDX-License-Identifier: GPL-3.0-only
// A backend's Configure sheet's model: the core's store underneath, the sheet
// it last drew, the backend it is up over, and the sheet's own fields.

import SuperVoiceCore
import SwiftUI

@MainActor
final class ConfigureModel: ObservableObject {
    static let shared = ConfigureModel()

    /// What the store last said the sheet shows.
    @Published private(set) var sheet = ConfigureSheet(
        source: nil, name: "", loaded: false, context: nil, rows: [], unmet: [], notice: nil
    )

    /// The backend the sheet is up over, as the app asked. The store's sheet
    /// catches up once it has read the backend.
    @Published private(set) var presented: String?

    /// What each text and secret field holds, by `fieldKey`. Typing goes to
    /// the store; a field is filled from the store again only when what the
    /// daemon has changed (a secret stored or removed, an option saved, reset
    /// or changed by another app), so a keystroke still on its way is never
    /// undone.
    @Published private(set) var fields: [String: String] = [:]
    /// What the daemon had for each field when it was last filled.
    private var filledFrom: [String: String] = [:]

    private var store: ConfigureStore!

    private init() {
        let relay = ConfigureRelay { [weak self] sheet in Task { @MainActor in self?.show(sheet) } }
        store = ConfigureStore(observer: relay)
    }

    /// The sheet is up over `source`'s and showing what was read for it.
    var ready: Bool {
        sheet.loaded && sheet.source == presented
    }

    /// Put the sheet up over `source`, and read what it shows.
    func open(_ source: String) {
        fields = [:]
        filledFrom = [:]
        presented = source
        store.send(action: .open(source: source))
    }

    func close() {
        presented = nil
        store.send(action: .close)
    }

    func send(_ action: ConfigureAction) {
        store.send(action: action)
    }

    /// What a field holds.
    func field(_ row: ConfigureRow) -> String {
        fields[fieldKey(row)] ?? ""
    }

    /// The user typed `value` into a row's field.
    func typed(_ row: ConfigureRow, _ value: String) {
        fields[fieldKey(row)] = value
        switch row.control {
        case .secret: send(.secretTyped(name: row.name, value: value))
        case .text: send(.optionTyped(name: row.name, value: value))
        case .bool, .choice: break
        }
    }

    private func show(_ sheet: ConfigureSheet) {
        #if DEBUG
        if pretending { return }
        #endif
        fill(sheet)
        self.sheet = sheet
    }

    /// Fill each field whose daemon side changed from the store.
    private func fill(_ sheet: ConfigureSheet) {
        guard sheet.loaded, sheet.source == presented else { return }
        for row in sheet.rows {
            let input: String
            let had: String
            switch row.control {
            case let .secret(configured, typed):
                input = typed
                had = configured ? "configured" : ""
            case let .text(typed, value, _):
                input = typed
                had = value ?? ""
            case .bool, .choice:
                continue
            }
            let key = fieldKey(row)
            if filledFrom[key] != had {
                filledFrom[key] = had
                fields[key] = input
            }
        }
    }

    /// A secret and an option may share a name.
    private func fieldKey(_ row: ConfigureRow) -> String {
        if case .secret = row.control {
            return "secret:" + row.name
        }
        return "option:" + row.name
    }

    #if DEBUG
    private var pretending = false

    /// Shows `sheet` up from now on, whatever the store says: a made-up
    /// scene's.
    func pretend(_ sheet: ConfigureSheet) {
        pretending = true
        presented = sheet.source
        fields = [:]
        filledFrom = [:]
        fill(sheet)
        self.sheet = sheet
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class ConfigureRelay: ConfigureObserver {
    let onChange: @Sendable (ConfigureSheet) -> Void

    init(onChange: @escaping @Sendable (ConfigureSheet) -> Void) {
        self.onChange = onChange
    }

    func changed(sheet: ConfigureSheet) {
        onChange(sheet)
    }
}
