// SPDX-License-Identifier: GPL-3.0-only
// The Contexts tab: what the user is dictating, so a model hears it. One row
// per context, one of them in force, and a sheet that edits one: its name, a
// prompt, and the words to listen for, one field per word. As in the settings
// app.

import SuperVoiceCore
import SwiftUI

struct ContextsView: View {
    @ObservedObject var model: ContextsModel

    var body: some View {
        let page = model.page
        VStack(spacing: 0) {
            if let notice = page.notice {
                NoticeBanner(text: notice.words)
            }
            Form {
                Section {
                    if !page.loaded {
                        Text(t("loading-progress"))
                            .foregroundStyle(.secondary)
                    } else if page.contexts.isEmpty {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(t("no-contexts-yet"))
                            Text(t("add-one-for-the-kind-of-thing"))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    ForEach(page.contexts, id: \.id) { context in
                        ContextRow(model: model, context: context)
                    }
                } header: {
                    Text(t("a-context-tells-a-model-what-you"))
                        .font(.body)
                        .foregroundStyle(.primary)
                        .textCase(nil)
                        .padding(.bottom, 6)
                }

                Section {
                    HStack {
                        // "None" isn't a context, so it isn't a row: it can't
                        // be edited or deleted.
                        if page.loaded && !page.contexts.isEmpty {
                            if page.active == nil {
                                Text(t("no-context-is-in-use"))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            } else {
                                Text(t("dictating-something-these-do-not-cover"))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                Button(t("use-none")) { model.send(.activate(id: nil)) }
                            }
                        }
                        Spacer()
                        Button(t("new-context")) { model.send(.create) }
                            .buttonStyle(.borderedProminent)
                    }
                }
            }
            .formStyle(.grouped)
        }
        .sheet(isPresented: editorOpen) {
            ContextEditorView(model: model)
        }
        .onAppear { model.send(.reload) }
    }

    /// The editor is open while the store has one; closing the sheet any
    /// other way drops what was typed, as Cancel does.
    private var editorOpen: Binding<Bool> {
        Binding(
            get: { model.page.editor != nil },
            set: { open in
                if !open, model.page.editor != nil {
                    model.send(.cancelEdit)
                }
            }
        )
    }
}

/// One context: its name, what it carries, its first words, and its buttons.
/// Delete asks again in the row.
struct ContextRow: View {
    @ObservedObject var model: ContextsModel
    let context: ContextSummary

    var body: some View {
        let active = model.page.active == context.id
        HStack(alignment: .top, spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(context.name)
                        .font(.headline)
                    if active {
                        Text(t("in-use"))
                            .font(.caption)
                            .foregroundStyle(Color.accentColor)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Color.accentColor))
                    }
                }
                Text(summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if !context.vocabulary.isEmpty {
                    Text(preview)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer()
            if !active {
                Button(t("use")) { model.send(.activate(id: context.id)) }
            }
            Button(t("edit")) { model.send(.edit(id: context.id)) }
            if model.page.confirmingDelete == context.id {
                Button(t("cancel")) { model.send(.deleteCancelled) }
                Button(t("delete")) { model.send(.deleteConfirmed(id: context.id)) }
                    .buttonStyle(.borderedProminent)
                    .tint(.red)
            } else {
                Button { model.send(.deleteRequested(id: context.id)) } label: {
                    IconView(icon: .x, size: 14)
                }
                .buttonStyle(.borderless)
                .help(t("delete"))
            }
        }
        .padding(.vertical, 2)
    }

    /// "12 terms · a prompt", or whichever halves it has.
    private var summary: String {
        var parts: [String] = []
        switch context.vocabulary.count {
        case 0: break
        case 1: parts.append(t("count-1-term"))
        case let n: parts.append(t("terms-count", ["n": "\(n)"]))
        }
        if context.hasPrompt {
            parts.append(t("a-prompt"))
        }
        return parts.isEmpty ? t("empty-nothing-is-sent-for-this-one") : parts.joined(separator: " · ")
    }

    /// The first few words, so a row is known without opening it.
    private var preview: String {
        let shown = 6
        let head = context.vocabulary.prefix(shown).joined(separator: ", ")
        let rest = context.vocabulary.count - shown
        return rest > 0 ? t("context-more-terms", ["head": head, "rest": "\(rest)"]) : head
    }
}

/// The editor sheet over one context.
struct ContextEditorView: View {
    @ObservedObject var model: ContextsModel
    @FocusState private var focused: UInt64?

    var body: some View {
        if let editor = model.page.editor {
            VStack(alignment: .leading, spacing: 0) {
                Text(title(editor))
                    .font(.title2.bold())
                    .padding([.top, .horizontal], 20)
                if let error = editor.error {
                    NoticeBanner(text: error)
                        .padding(.top, 10)
                }
                Form {
                    Section {
                        TextField(t("name"), text: $model.name, prompt: Text(t("coding")))
                            .onChange(of: model.name) { _, name in model.send(.nameChanged(name: name)) }
                    }
                    Section {
                        TextEditor(text: $model.prompt)
                            .font(.body)
                            .frame(height: 90)
                            .onChange(of: model.prompt) { _, text in model.promptChanged(text) }
                    } header: {
                        Text(t("prompt"))
                    } footer: {
                        Text(t("instructions-for-a-model-that-follows-them"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Section {
                        ForEach(Array(model.terms.enumerated()), id: \.element.id) { index, row in
                            TermField(model: model, index: index, row: row.id, focused: $focused, removable: model.terms.count > 1)
                                .id(row.id)
                        }
                    } header: {
                        HStack {
                            Text(t("words-to-listen-for"))
                            Spacer()
                            Text(wordsCount)
                                .foregroundStyle(.secondary)
                        }
                    } footer: {
                        Text(t("names-jargon-anything-the-model-keeps-mishearing"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .formStyle(.grouped)
                HStack {
                    Spacer()
                    Button(t("cancel")) { model.send(.cancelEdit) }
                        .keyboardShortcut(.cancelAction)
                    Button(editor.saving ? t("saving-progress") : t("save")) { model.send(.save) }
                        .buttonStyle(.borderedProminent)
                        .disabled(editor.saving)
                }
                .padding([.horizontal, .bottom], 20)
            }
            .frame(width: 520, height: 500)
            .onChange(of: model.page.focusAsked) { _, _ in
                focused = model.page.focusRow
            }
        }
    }

    /// What the user is doing: making one, or changing one.
    private func title(_ editor: ContextEditor) -> String {
        if editor.isNew {
            return t("new-context")
        }
        let name = model.name.trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? t("edit-context") : t("context-edit", ["name": name])
    }

    /// "0 words", "1 word", "12 words": the rows with something in them.
    private var wordsCount: String {
        let filled = model.terms.filter { !$0.text.trimmingCharacters(in: .whitespaces).isEmpty }.count
        return filled == 1 ? t("count-1-word") : t("words-count", ["n": "\(filled)"])
    }
}

/// One word's field: Enter for the next, and a clear button while there is
/// more than one row (the last empty one is where the next word goes).
struct TermField: View {
    @ObservedObject var model: ContextsModel
    let index: Int
    /// The row's id, which focus is asked by.
    let row: UInt64
    var focused: FocusState<UInt64?>.Binding
    let removable: Bool

    var body: some View {
        HStack {
            TextField("", text: text)
                .labelsHidden()
                .focused(focused, equals: row)
                .onSubmit { model.send(.termSubmitted(index: UInt32(index))) }
            if removable {
                Button { model.send(.termRemoved(index: UInt32(index))) } label: {
                    IconView(icon: .x, size: 12)
                }
                .buttonStyle(.borderless)
            }
        }
    }

    /// The row's text, sent to the store as it changes.
    private var text: Binding<String> {
        Binding(
            get: { index < model.terms.count ? model.terms[index].text : "" },
            set: { value in
                guard index < model.terms.count else { return }
                model.terms[index].text = value
                model.send(.termChanged(index: UInt32(index), value: value))
            }
        )
    }
}

extension ContextsNotice {
    /// What the page says about it.
    var words: String {
        switch self {
        case let .couldNotLoad(message): t("contexts-could-not-load", ["error": message])
        case let .couldNotSave(message): t("context-could-not-save", ["error": message])
        }
    }
}
