// SPDX-License-Identifier: GPL-3.0-only
// The window: a sidebar of the settings app's pages, and the one picked.

import SwiftUI

/// The pages, in the settings app's order.
enum Destination: Hashable, CaseIterable {
    case models, library, speechToText, textToSpeech, shortcuts, assistants, settings, updates

    var title: String {
        switch self {
        case .models: t("models")
        case .library: t("library")
        case .speechToText: t("speech-to-text")
        case .textToSpeech: t("text-to-speech")
        case .shortcuts: t("shortcuts")
        case .assistants: t("assistants")
        case .settings: t("settings")
        case .updates: t("updates")
        }
    }

    var icon: Icon {
        switch self {
        case .models: .brain
        case .library: .books
        case .speechToText: .microphone
        case .textToSpeech: .speakerHigh
        case .shortcuts: .keyboard
        case .assistants: .robot
        case .settings: .gear
        case .updates: .arrowsClockwise
        }
    }
}

struct ContentView: View {
    @ObservedObject var notes: NotesModel
    @ObservedObject var header = HeaderModel.shared
    @State private var selection: Destination? = .speechToText

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                row(.models)
                row(.library)
                GroupDivider()
                row(.speechToText)
                row(.textToSpeech)
                GroupDivider()
                row(.shortcuts)
                row(.assistants)
                row(.settings)
                row(.updates)
            }
            // Low enough for the group lines, which are thin; a page's row is
            // still as tall as its icon and name.
            .environment(\.defaultMinListRowHeight, 8)
            .navigationSplitViewColumnWidth(min: 180, ideal: 200)
        } detail: {
            switch selection {
            case .speechToText:
                SpeechToTextView(notes: notes)
            case let other?:
                NotYetView(title: other.title, icon: other.icon)
            case nil:
                NotYetView(title: "Super Voice", icon: .microphone)
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                HeaderPills(model: header)
            }
            .withoutGlass()
        }
    }

    private func row(_ destination: Destination) -> some View {
        Label {
            Text(destination.title)
        } icon: {
            IconView(icon: destination.icon)
        }
        .tag(destination)
    }
}

/// The line between the sidebar's groups, as in the settings app: the models,
/// the two features, then the rest.
struct GroupDivider: View {
    var body: some View {
        Divider()
            .frame(height: 9)
            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
            .selectionDisabled()
    }
}

/// Speech to text's tabs, at the top of the page as in the settings app. Only
/// Notes is here so far.
struct SpeechToTextView: View {
    @ObservedObject var notes: NotesModel

    enum Tab: Hashable { case notes, contexts, behavior }
    @State private var tab: Tab = .notes

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Picker(t("speech-to-text"), selection: $tab) {
                Text(t("notes")).tag(Tab.notes)
                Text(t("contexts")).tag(Tab.contexts)
                Text(t("behavior")).tag(Tab.behavior)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            // In line with the grouped cards below it.
            .padding(.top)
            .padding(.horizontal, 28)
            switch tab {
            case .notes: NotesView(model: notes)
            case .contexts: NotYetView(title: t("contexts"), icon: .microphone)
            case .behavior: NotYetView(title: t("behavior"), icon: .microphone)
            }
        }
        .navigationTitle(t("speech-to-text"))
    }
}

extension ToolbarContent {
    /// Without the glass macOS 26 puts behind toolbar items, which would draw
    /// a second outline around the header's own pills.
    @ToolbarContentBuilder
    func withoutGlass() -> some ToolbarContent {
        if #available(macOS 26.0, *) {
            sharedBackgroundVisibility(.hidden)
        } else {
            self
        }
    }
}

/// A page this app doesn't have yet.
struct NotYetView: View {
    let title: String
    let icon: Icon

    var body: some View {
        ContentUnavailableView {
            Label {
                Text(title)
            } icon: {
                IconView(icon: icon, size: 40)
            }
        } description: {
            Text(t("page-not-in-this-app-yet"))
        }
        .navigationTitle(title)
    }
}
