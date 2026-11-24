// SPDX-License-Identifier: GPL-3.0-only
// The Notes tab: the notes the app keeps, newest first, and the one open.

import SuperVoiceCore
import SwiftUI

struct NotesView: View {
    @ObservedObject var model: NotesModel

    var body: some View {
        VStack(spacing: 0) {
            if let notice = model.page.notice {
                NoticeBanner(text: notice.words)
            }
            if let open = model.page.open {
                OpenNoteView(model: model, open: open)
            } else {
                NotesListView(model: model)
            }
        }
        .onAppear { model.send(.reload) }
        .onDisappear { model.send(.saveNow) }
    }
}

/// The list, with the card a new note is recorded from.
struct NotesListView: View {
    @ObservedObject var model: NotesModel

    var body: some View {
        List {
            Section {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("New note")
                        Text("Recording isn't in this app yet. Use the Linux app, or the shortcut.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Record") {}
                        .disabled(true)
                }
                .padding(.vertical, 4)
            } footer: {
                Text("Notes are saved on this computer, in the app's own folder.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Section {
                ForEach(model.page.notes, id: \.id) { note in
                    Button {
                        model.send(.open(id: note.id))
                    } label: {
                        NoteRow(note: note)
                    }
                    .buttonStyle(.plain)
                }
            } header: {
                HStack {
                    Text("Notes")
                    Spacer()
                    Text(count(model.page.notes.count, "note"))
                }
            }
        }
        .overlay {
            if model.page.loaded && model.page.notes.isEmpty {
                ContentUnavailableView(
                    "No notes yet",
                    systemImage: "note.text",
                    description: Text(model.page.folder)
                )
            }
        }
    }
}

struct NoteRow: View {
    let note: NoteSummary

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(firstLine(note.text))
                    .lineLimit(1)
                Text("\(when(note.created)) · \(count(Int(note.words), "word"))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Image(systemName: "chevron.right")
                .foregroundStyle(.tertiary)
        }
        .contentShape(Rectangle())
        .padding(.vertical, 4)
    }
}

/// The open note, in place of the list.
struct OpenNoteView: View {
    @ObservedObject var model: NotesModel
    let open: OpenNotePage

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(when(open.created))
                    .font(.title2.bold())
                Text("\(count(Int(open.words), "word")) · \(open.saved ? "Saved" : "Saving…")")
                    .foregroundStyle(.secondary)
            }
            TextEditor(text: $model.text)
                .font(.body)
                .scrollContentBackground(.hidden)
                .padding(8)
                .background(.background.secondary, in: RoundedRectangle(cornerRadius: 8))
                .onChange(of: model.text) { _, text in model.edited(text) }
            HStack {
                Button("Delete", role: .destructive) { model.send(.deleteRequested) }
                Spacer()
                if let exported = model.page.exported {
                    Text("Exported to \(exported)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Button("Export…") { model.send(.export) }
                Button("Dictate more") {}
                    .disabled(true)
            }
        }
        .padding()
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button {
                    model.send(.close)
                } label: {
                    Label("All notes", systemImage: "chevron.left")
                }
            }
        }
        .confirmationDialog(
            "Delete this note?",
            isPresented: Binding(
                get: { model.page.confirmingDelete },
                set: { shown in if !shown && model.page.confirmingDelete { model.send(.deleteCancelled) } }
            )
        ) {
            Button("Delete", role: .destructive) { model.send(.deleteConfirmed) }
            Button("Cancel", role: .cancel) { model.send(.deleteCancelled) }
        }
    }
}

struct NoticeBanner: View {
    let text: String

    var body: some View {
        Label(text, systemImage: "exclamationmark.triangle")
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(10)
            .background(.orange.opacity(0.15))
    }
}

extension NotesNotice {
    /// What the page says about it.
    var words: String {
        switch self {
        case let .couldNotRead(message): "The notes couldn't be read: \(message)"
        case let .couldNotSave(message): "The note couldn't be saved: \(message)"
        case let .couldNotDelete(message): "The note couldn't be deleted: \(message)"
        case let .couldNotExport(message): "The note couldn't be exported: \(message)"
        case .noSpeech: "No speech was heard."
        case .anotherRecording: "Another recording is running."
        case let .recordingFailed(message): "The recording failed: \(message)"
        }
    }
}

/// "Today, 08:02", "Yesterday, 16:40", or the date and time.
func when(_ seconds: Int64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(seconds))
    let calendar = Calendar.current
    let time = date.formatted(date: .omitted, time: .shortened)
    if calendar.isDateInToday(date) { return "Today, \(time)" }
    if calendar.isDateInYesterday(date) { return "Yesterday, \(time)" }
    return date.formatted(date: .abbreviated, time: .shortened)
}

/// "1 note", "3 notes".
func count(_ n: Int, _ noun: String) -> String {
    n == 1 ? "1 \(noun)" : "\(n) \(noun)s"
}

/// A note's first line, which the list shows as its title.
func firstLine(_ text: String) -> String {
    text.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? ""
}
