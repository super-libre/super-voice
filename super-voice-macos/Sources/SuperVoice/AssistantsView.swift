// SPDX-License-Identifier: GPL-3.0-only
// The Assistants page: each AI assistant on this computer, connected to Super
// Voice or not, and keys for any other assistant that supports MCP. The daemon
// writes each assistant's config and holds the keys; the page shows what it
// answers. As in the settings app.

import SuperVoiceCore
import SwiftUI

struct AssistantsView: View {
    @ObservedObject var model: AssistantsModel

    var body: some View {
        let page = model.page
        let opened = page.assistants.first { $0.id == page.open && $0.standing == .connected }
        VStack(spacing: 0) {
            // While a dialog is up, its error is in it, beside what failed.
            if let notice = page.notice, page.connect == nil, page.key == nil {
                NoticeBanner(text: notice)
            }
            if let assistant = opened {
                AssistantPageView(model: model, assistant: assistant)
            } else {
                AssistantsListView(model: model)
            }
        }
        .navigationTitle(t("assistants"))
        .onAppear { model.send(.refresh) }
        .sheet(isPresented: Binding(
            get: { page.connect != nil },
            set: { shown in if !shown { model.send(.cancelConnect) } }
        )) {
            if let question = page.connect {
                ConnectSheet(model: model, question: question, notice: page.notice)
            }
        }
        .sheet(isPresented: Binding(
            get: { page.key != nil },
            set: { shown in if !shown { model.send(.closeKey) } }
        )) {
            if let key = page.key {
                KeySheet(model: model, key: key, notice: page.notice)
            }
        }
    }
}

/// The list: what the page is for, each assistant, then any other assistant's
/// keys.
struct AssistantsListView: View {
    @ObservedObject var model: AssistantsModel

    var body: some View {
        let page = model.page
        Form {
            Section {
                ForEach(page.assistants, id: \.id) { assistant in
                    AssistantListRow(model: model, assistant: assistant)
                }
            } header: {
                VStack(alignment: .leading, spacing: 4) {
                    Text(t("assistants-intro"))
                        .font(.body)
                        .foregroundStyle(.primary)
                    Text(t("assistants-intro-caption"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(t("on-this-computer"))
                        .padding(.top, 12)
                }
                .textCase(nil)
            }
            if page.loaded {
                Section {
                    LabeledContent {
                        Button(t("create-key")) { model.send(.askKey) }
                    } label: {
                        Labelled(t("any-mcp-assistant"), t("any-mcp-assistant-description"))
                    }
                    ForEach(page.keys, id: \.id) { key in
                        LabeledContent {
                            Button(t("remove")) { model.send(.removeKey(id: key.id)) }
                        } label: {
                            Labelled(key.name, key.created.map(when) ?? key.made)
                        }
                        ForEach(Array(key.sessions.enumerated()), id: \.offset) { _, session in
                            SessionRowView(session: session)
                                .padding(.leading, 24)
                        }
                    }
                } header: {
                    Text(t("another-assistant"))
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// One assistant on the list: Connect, nothing to do when it isn't on this
/// computer, or, connected, its sessions and the chevron that opens its page.
struct AssistantListRow: View {
    @ObservedObject var model: AssistantsModel
    let assistant: AssistantRow

    var body: some View {
        switch assistant.standing {
        case .notFound:
            Labelled(assistant.name, t("assistant-not-found"))
        case .found:
            LabeledContent {
                Button(t("connect")) { model.send(.askConnect(id: assistant.id)) }
                    .buttonStyle(.borderedProminent)
            } label: {
                Labelled(assistant.name, assistant.does)
            }
        case .connected:
            Button {
                model.send(.open(id: assistant.id))
            } label: {
                HStack {
                    // An assistant reads its config when it starts.
                    Labelled(
                        assistant.name,
                        assistant.connectedHere
                            ? t("assistant-restart-to-use", ["name": assistant.name])
                            : t("sessions-open", ["count": "\(assistant.sessions.count)"])
                    )
                    Spacer()
                    StatePill(color: .systemGreen, word: t("assistant-connected"))
                    IconView(icon: .caretRight, size: 14, color: .tertiaryLabelColor)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
    }
}

/// A connected assistant's own page, in the list's place: back to the list,
/// its switches with what each does, its open sessions, and Disconnect.
struct AssistantPageView: View {
    @ObservedObject var model: AssistantsModel
    let assistant: AssistantRow

    var body: some View {
        Form {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Button {
                        model.send(.close)
                    } label: {
                        HStack(spacing: 4) {
                            IconView(icon: .caretLeft, size: 14, color: .controlAccentColor)
                            Text(t("all-assistants"))
                                .foregroundStyle(Color.accentColor)
                        }
                    }
                    .buttonStyle(.plain)
                    HStack(spacing: 10) {
                        Text(assistant.name)
                            .font(.title2.bold())
                        StatePill(color: .systemGreen, word: t("assistant-connected"))
                    }
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .textCase(nil)
            }

            Section {
                ForEach(assistant.flags, id: \.flag) { row in
                    Toggle(isOn: Binding(
                        get: { row.on },
                        set: { model.send(.setFlag(id: assistant.id, flag: row.flag, on: $0)) }
                    )) {
                        Labelled(row.flag.title, row.flag.detail(assistant.name))
                    }
                }
            } header: {
                Text(t("settings"))
            }

            if !assistant.sessions.isEmpty {
                Section {
                    ForEach(Array(assistant.sessions.enumerated()), id: \.offset) { _, session in
                        SessionRowView(session: session)
                    }
                } header: {
                    Text(t("sessions"))
                }
            }

            Section {
                LabeledContent {
                    Button(t("disconnect")) { model.send(.disconnect(id: assistant.id)) }
                        .disabled(assistant.disconnecting)
                } label: {
                    Labelled(
                        t("disconnect-assistant", ["name": assistant.name]),
                        t("disconnect-assistant-description", ["name": assistant.name])
                    )
                }
            }
        }
        .formStyle(.grouped)
    }

    /// What it does, and that it needs a restart when connected here.
    private var description: String {
        assistant.connectedHere
            ? assistant.does + " " + t("assistant-restart-to-use", ["name": assistant.name])
            : assistant.does
    }
}

/// One open session: the folder it works in, whether it speaks or listens,
/// and when it last did.
struct SessionRowView: View {
    let session: SessionRow

    var body: some View {
        HStack {
            Labelled(session.name ?? t("session-unnamed"), about)
            Spacer()
            if session.speaking {
                StatePill(color: .systemGreen, word: t("session-speaking"))
            } else if session.listening {
                StatePill(color: .systemYellow, word: t("session-listening"))
            }
            if let ago = session.ago {
                Text(ago.words)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(width: 84, alignment: .trailing)
            }
        }
    }

    private var about: String {
        var parts: [String] = []
        if let folder = session.folder { parts.append(folder) }
        if session.agents > 0 { parts.append(t("session-agents", ["count": "\(session.agents)"])) }
        return parts.joined(separator: " · ")
    }
}

/// A state: a dot in its colour and the word, on a neutral pill, as the
/// header's states are drawn.
struct StatePill: View {
    let color: NSColor
    let word: String

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(Color(nsColor: color))
                .frame(width: 8, height: 8)
            Text(word)
                .font(.callout)
        }
        .padding(.vertical, 3)
        .padding(.horizontal, 9)
        .background(Capsule().fill(Color(nsColor: .quaternarySystemFill)))
    }
}

/// What the user lets an assistant do by connecting it, asked before the
/// daemon makes its key.
struct ConnectSheet: View {
    @ObservedObject var model: AssistantsModel
    let question: ConnectQuestion
    let notice: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(t("connect-assistant-question", ["name": question.name]))
                .font(.title2.bold())
            VStack(alignment: .leading, spacing: 4) {
                Text(t("assistant-will-be-able-to", ["name": question.name]))
                Text(t("assistant-can-speak"))
                Text(t("assistant-can-listen"))
            }
            Text(question.withPlugin
                 ? t("connect-plugin-caption", ["name": question.name])
                 : t("connect-codex-caption"))
                .font(.caption)
                .foregroundStyle(.secondary)
            if let notice {
                NoticeBanner(text: notice)
            }
            // Claude Code's plugin can take a minute to install: the button
            // says it is working, and neither answer can be given twice.
            HStack {
                Spacer()
                Button(t("cancel"), role: .cancel) { model.send(.cancelConnect) }
                    .keyboardShortcut(.cancelAction)
                    .disabled(question.connecting)
                Button(question.connecting ? t("connecting-assistant") : t("connect")) {
                    model.send(.connect)
                }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(question.connecting)
            }
        }
        .padding(20)
        .frame(width: 460)
        .interactiveDismissDisabled(question.connecting)
    }
}

/// A key for an assistant Super Voice does not connect itself: its name, then
/// the address and the key, shown this once.
struct KeySheet: View {
    @ObservedObject var model: AssistantsModel
    let key: KeyQuestion
    let notice: String?
    @FocusState private var naming: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(t("key-for-another-assistant"))
                .font(.title2.bold())
            VStack(alignment: .leading, spacing: 4) {
                Text(t("name"))
                switch key {
                case let .naming(_, creating, _):
                    TextField("", text: $model.keyName)
                        .textFieldStyle(.roundedBorder)
                        .focused($naming)
                        .disabled(creating)
                        .onChange(of: model.keyName) { _, name in model.send(.keyNameChanged(name: name)) }
                        .onSubmit { model.send(.createKey) }
                case let .made(name, _, _):
                    TextField("", text: .constant(name))
                        .textFieldStyle(.roundedBorder)
                        .disabled(true)
                }
                Text(t("key-name-caption"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if case let .made(_, secret, address) = key {
                VStack(spacing: 8) {
                    ValueRow(model: model, label: t("address"), value: address)
                    Divider()
                    ValueRow(model: model, label: t("key"), value: secret)
                }
                .padding(12)
                .background(Color(nsColor: .quaternarySystemFill), in: RoundedRectangle(cornerRadius: 8))
                Text(t("key-shown-only-now"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            if let notice {
                NoticeBanner(text: notice)
            }
            HStack {
                Spacer()
                switch key {
                case let .naming(_, creating, canCreate):
                    Button(t("cancel"), role: .cancel) { model.send(.closeKey) }
                        .keyboardShortcut(.cancelAction)
                        .disabled(creating)
                    Button(creating ? t("creating-key") : t("create")) { model.send(.createKey) }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent)
                        .disabled(!canCreate)
                case .made:
                    Button(t("done")) { model.send(.closeKey) }
                        .keyboardShortcut(.defaultAction)
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        .padding(20)
        .frame(width: 520)
        .onAppear { naming = true }
    }
}

/// One value the assistant needs, with Copy.
struct ValueRow: View {
    @ObservedObject var model: AssistantsModel
    let label: String
    let value: String

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .frame(width: 72, alignment: .leading)
            // A key has no spaces to wrap at, and wrapped as words it breaks
            // with hyphens that read as part of it. So it may break between
            // any two characters, with nothing added; Copy copies the value.
            Text(value.map(String.init).joined(separator: "\u{200B}"))
                .font(.body.monospaced())
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(t("copy")) { model.copy(value) }
        }
    }
}

extension AssistantRow {
    /// What it can do through Super Voice.
    var does: String {
        readsEveryReply ? t("assistant-reads-replies") : t("assistant-reads-dictated-replies")
    }
}

extension AssistantFlag {
    var title: String {
        switch self {
        case .showTalk: t("show-talk-above-prompt")
        case .autosendDictatedPrompts: t("autosend-dictated-prompts")
        case .announceWhenNeeded: t("announce-when-needed")
        case .conversationContext: t("use-conversation-as-context")
        }
    }

    func detail(_ name: String) -> String {
        switch self {
        case .showTalk: t("show-talk-above-prompt-description")
        case .autosendDictatedPrompts: t("autosend-dictated-prompts-description")
        case .announceWhenNeeded: t("announce-when-needed-description", ["name": name])
        case .conversationContext: t("use-conversation-as-context-description")
        }
    }
}

extension SessionAgo {
    var words: String {
        switch self {
        case .now: t("used-now")
        case let .minutes(n): t("used-minutes-ago", ["minutes": "\(n)"])
        case let .hours(n): t("used-hours-ago", ["hours": "\(n)"])
        }
    }
}
