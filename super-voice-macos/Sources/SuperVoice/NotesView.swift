// SPDX-License-Identifier: GPL-3.0-only
// The notes the Notes tab keeps, newest first, as the core reads them.

import SuperVoiceCore
import SwiftUI

struct NotesView: View {
    @State private var notes: [NoteSummary] = []
    @State private var loaded = false
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            List(notes, id: \.id) { note in
                VStack(alignment: .leading, spacing: 2) {
                    Text(firstLine(note.text))
                        .lineLimit(1)
                    Text("\(note.created) · \(note.words) words")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 2)
            }
            .overlay {
                if let failure {
                    ContentUnavailableView(
                        "The notes could not be read",
                        systemImage: "exclamationmark.triangle",
                        description: Text(failure)
                    )
                } else if loaded && notes.isEmpty {
                    ContentUnavailableView(
                        "No notes yet",
                        systemImage: "note.text",
                        description: Text(notesFolder())
                    )
                }
            }
            .navigationTitle("Notes")
        }
        .task { await load() }
    }

    private func load() async {
        do {
            notes = try await listNotes()
        } catch {
            failure = "\(error)"
        }
        loaded = true
        #if DEBUG
        await Snapshot.pageReady()
        #endif
    }

    /// The note's first line, which the list shows as its title.
    private func firstLine(_ text: String) -> String {
        text.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? ""
    }
}
