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
                NotYetView(title: other.title, icon: other.icon)
            case nil:
                NotYetView(title: "Super Voice", icon: .microphone)
            }
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

/// Speech to text's tabs. Only Notes is here so far.
struct SpeechToTextView: View {
    @ObservedObject var notes: NotesModel

    enum Tab: Hashable { case notes, contexts, behavior }
    @State private var tab: Tab = .notes

    var body: some View {
        Group {
            switch tab {
            case .notes: NotesView(model: notes)
            case .contexts: NotYetView(title: "Contexts", icon: .microphone)
            case .behavior: NotYetView(title: "Behavior", icon: .microphone)
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
    let icon: Icon

    var body: some View {
        ContentUnavailableView {
            Label {
                Text(title)
            } icon: {
                IconView(icon: icon, size: 40)
            }
        } description: {
            Text("This page is in the Linux app, and not here yet.")
        }
        .navigationTitle(title)
    }
}
