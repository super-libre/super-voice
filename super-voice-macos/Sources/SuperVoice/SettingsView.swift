// SPDX-License-Identifier: GPL-3.0-only
// The Settings page: what both features share. The primary language, how
// failures are reported, and how long an unused model stays in memory. As in
// the settings app.

import SuperVoiceCore
import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: SettingsModel
    @State private var choosingLanguage = Self.startsChoosing

    /// Whether the page opens with the language picker up: only for its shot.
    private static var startsChoosing: Bool {
        #if DEBUG
        return Snapshot.scene == "settings-languages"
        #else
        return false
        #endif
    }

    var body: some View {
        let page = model.page
        VStack(spacing: 0) {
            if let notice = page.notice {
                NoticeBanner(text: notice)
            }
            Form {
                Section {
                    LabeledContent {
                        Button(page.primaryLanguageName) { choosingLanguage = true }
                    } label: {
                        Labelled(t("primary-language"), t("default-language-for-models-that-support-it"))
                    }
                } header: {
                    Text(t("language"))
                }

                Section {
                    Picker(selection: Binding(
                        get: { page.notificationMethod },
                        set: { model.send(.notificationMethodPicked(id: $0)) }
                    )) {
                        ForEach(page.notificationMethods, id: \.id) { Text($0.name).tag($0.id) }
                    } label: {
                        Labelled(t("failure-notices"), t("how-recording-and-speech-failures-are-reported"))
                    }
                } header: {
                    Text(t("notifications"))
                }

                Section {
                    // The switch moves once the daemon has stored it.
                    Toggle(isOn: Binding(
                        get: { page.sleep },
                        set: { model.send(.sleepToggled(on: $0)) }
                    )) {
                        Labelled(t("sleep-when-idle"), t("sleep-when-idle-description"))
                    }
                    if page.sleep {
                        Picker(t("sleep-after"), selection: Binding(
                            get: { page.idleUnloadSecs },
                            set: { model.send(.idleUnloadPicked(afterSecs: $0)) }
                        )) {
                            ForEach(page.idleChoices, id: \.id) { choice in
                                Text(choice.name).tag(UInt64(choice.id) ?? 0)
                            }
                        }
                    }
                } header: {
                    Text(t("memory"))
                }
            }
            .formStyle(.grouped)
        }
        .navigationTitle(t("settings"))
        .onAppear { model.send(.reload) }
        .sheet(isPresented: $choosingLanguage) {
            LanguagePicker(
                title: t("primary-language"),
                rows: { model.languageRows($0) },
                selected: model.page.primaryLanguage,
                pick: { model.send(.primaryLanguagePicked(tag: $0)) },
                isPresented: $choosingLanguage
            )
        }
    }
}

/// A language picker: a search over the languages on offer, sorted by name,
/// under any rows pinned above them. Picking one closes it. The primary
/// language's, on this page, and a model's, on the Models page.
struct LanguagePicker: View {
    let title: String
    /// The rows for what the search holds.
    let rows: @MainActor (String) -> [Choice]
    /// The id of the row to mark.
    let selected: String?
    /// A row was picked, by its id.
    let pick: @MainActor (String) -> Void
    @Binding var isPresented: Bool
    @State private var query = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.title2.bold())
            TextField(t("search-languages-progress"), text: $query)
                .textFieldStyle(.roundedBorder)
            List(rows(query), id: \.id) { row in
                Button {
                    pick(row.id)
                    isPresented = false
                } label: {
                    HStack {
                        Text(row.name)
                        Spacer()
                        if row.id == selected {
                            IconView(icon: .check, size: 14, color: .controlAccentColor)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            HStack {
                Spacer()
                Button(t("cancel"), role: .cancel) { isPresented = false }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding()
        .frame(width: 420, height: 480)
    }
}
