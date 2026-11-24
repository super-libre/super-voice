// SPDX-License-Identifier: GPL-3.0-only
// The window: a sidebar of the settings app's pages, and the one picked.

import SwiftUI

/// The pages, in the settings app's order.
enum Destination: Hashable, CaseIterable {
    case models, library, speechToText, textToSpeech, shortcuts, assistants, settings, updates

    var title: String {
        switch self {
        case .models: "Models"
        case .library: "Library"
        case .speechToText: "Speech to text"
        case .textToSpeech: "Text to speech"
        case .shortcuts: "Shortcuts"
        case .assistants: "Assistants"
        case .settings: "Settings"
        case .updates: "Updates"
        }
    }

    var symbol: String {
        switch self {
        case .models: "cpu"
        case .library: "books.vertical"
        case .speechToText: "mic"
        case .textToSpeech: "speaker.wave.2"
        case .shortcuts: "keyboard"
        case .assistants: "sparkles"
        case .settings: "gearshape"
        case .updates: "arrow.triangle.2.circlepath"
        }
    }
}

struct ContentView: View {
    @ObservedObject var notes: NotesModel
    @State private var selection: Destination? = .speechToText

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Section {
                    row(.models)
                    row(.library)
                }
                Section {
                    row(.speechToText)
                    row(.textToSpeech)
                }
                Section {
                    row(.shortcuts)
                    row(.assistants)
                    row(.settings)
                    row(.updates)
                }
            }
            .navigationSplitViewColumnWidth(min: 180, ideal: 200)
        } detail: {
            switch selection {
            case .speechToText:
                SpeechToTextView(notes: notes)
            case let other?:
                NotYetView(title: other.title)
            case nil:
                NotYetView(title: "Super Voice")
            }
        }
    }

    private func row(_ destination: Destination) -> some View {
        Label(destination.title, systemImage: destination.symbol).tag(destination)
    }
}

/// Speech to text's tabs. Only Notes is here so far.
struct SpeechToTextView: View {
    @ObservedObject var notes: NotesModel

    enum Tab: Hashable { case notes, contexts, behavior }
    @State private var tab: Tab = .notes

    var body: some View {
        Group {
            switch tab {
            case .notes: NotesView(model: notes)
            case .contexts: NotYetView(title: "Contexts")
            case .behavior: NotYetView(title: "Behavior")
            }
        }
        .navigationTitle("Speech to text")
        .toolbar {
            ToolbarItem(placement: .principal) {
                Picker("Tab", selection: $tab) {
                    Text("Notes").tag(Tab.notes)
                    Text("Contexts").tag(Tab.contexts)
                    Text("Behavior").tag(Tab.behavior)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            }
        }
    }
}

/// A page this app doesn't have yet.
struct NotYetView: View {
    let title: String

    var body: some View {
        ContentUnavailableView(
            title,
            systemImage: "hammer",
            description: Text("This page is in the Linux app, and not here yet.")
        )
        .navigationTitle(title)
    }
}
