// SPDX-License-Identifier: GPL-3.0-only
// The Library page: the backends installed, each to update or uninstall, and
// the registry's catalog to install more from, with its search and filters,
// and a backend from outside the catalog installed by hand. As in the
// settings app.

import AppKit
import SuperVoiceCore
import SwiftUI

@MainActor
final class LibraryModel: ObservableObject {
    static let shared = LibraryModel()

    @Published private(set) var page = LibraryPage(
        installed: [], installedTotal: 0, installedFeature: .all, installedRunsOn: .all, installedKind: .all,
        catalogLoaded: false, catalogError: nil, catalogStale: false, catalogUpdated: nil, catalog: [],
        search: "", includeIncompatible: false, feature: .all, runsOn: .all, kind: .all, pendingUninstall: nil,
        add: AddSheet(open: false, source: .repository, url: "", folder: "", preview: .empty, install: .idle,
                      installable: nil)
    )

    /// The Browse tab's search field. The store gets each change.
    @Published var search = ""
    /// The "Install manually" sheet's fields. The store gets each change, and
    /// they take the store's back while the sheet is down: emptied once what
    /// it previewed is installed.
    @Published var addUrl = ""
    @Published var addFolder = ""

    private var store: LibraryStore!

    private init() {
        let relay = LibraryRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = LibraryStore(observer: relay)
    }

    func send(_ action: LibraryAction) {
        store.send(action: action)
    }

    /// Ask for the folder a backend is in; the one chosen is checked at once.
    func chooseFolder() {
        let panel = NSOpenPanel()
        panel.title = t("choose-backend-folder")
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.begin { [weak self] answer in
            guard answer == .OK, let path = panel.url?.path else { return }
            Task { @MainActor in
                self?.addFolder = path
                self?.send(.addFolderChosen(path: path))
            }
        }
    }

    private func show(_ page: LibraryPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
        if !page.add.open {
            addUrl = page.add.url
            addFolder = page.add.folder
        }
        #if DEBUG
        Snapshot.libraryShown(page)
        #endif
    }

    #if DEBUG
    private var pretending = false

    /// Shows `page` from now on, whatever the store says: a made-up scene's.
    func pretend(_ page: LibraryPage) {
        pretending = true
        self.page = page
        addUrl = page.add.url
        addFolder = page.add.folder
    }
    #endif
}

/// Hands the store's calls, which come on its own threads, to the model.
final class LibraryRelay: LibraryObserver {
    let onChange: @Sendable (LibraryPage) -> Void

    init(onChange: @escaping @Sendable (LibraryPage) -> Void) {
        self.onChange = onChange
    }

    func changed(page: LibraryPage) {
        onChange(page)
    }
}

struct LibraryView: View {
    @ObservedObject var model: LibraryModel
    enum Tab: Hashable { case installed, browse }
    @State private var tab: Tab = Self.firstTab

    private static var firstTab: Tab {
        #if DEBUG
        if ["library-browse", "library-sample-browse", "library-add"].contains(Snapshot.scene) {
            return .browse
        }
        #endif
        return .installed
    }

    var body: some View {
        let page = model.page
        VStack(alignment: .leading, spacing: 0) {
            Picker(t("library"), selection: $tab) {
                Text(t("installed")).tag(Tab.installed)
                Text(t("browse")).tag(Tab.browse)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .padding(.top)
            .padding(.horizontal, 28)
            switch tab {
            case .installed: InstalledTab(model: model)
            case .browse: BrowseTab(model: model)
            }
        }
        .navigationTitle(t("library"))
        .onAppear { model.send(.reload) }
        .confirmationDialog(
            t("uninstall-backend-question", ["backend": page.pendingUninstall?.name ?? ""]),
            isPresented: Binding(
                get: { page.pendingUninstall != nil },
                set: { shown in if !shown && page.pendingUninstall != nil { model.send(.cancelUninstall) } }
            )
        ) {
            if let question = page.pendingUninstall {
                Button(t("uninstall"), role: .destructive) { model.send(.uninstall(source: question.source)) }
            }
            Button(t("cancel"), role: .cancel) { model.send(.cancelUninstall) }
        } message: {
            Text(page.pendingUninstall?.inUse == true
                 ? t("uninstall-backend-body") + " " + t("uninstall-backend-in-use")
                 : t("uninstall-backend-body"))
        }
        .sheet(isPresented: Binding(
            get: { page.add.open },
            set: { shown in if !shown && page.add.open { model.send(.closeAdd) } }
        )) {
            AddBackendSheet(model: model)
        }
    }
}

/// The filters every tab has: the feature, under speech to text the kind,
/// and where it runs. `installed` picks the Installed tab's, else the Browse
/// tab's.
struct LibraryFilters: View {
    @ObservedObject var model: LibraryModel
    let installed: Bool

    var body: some View {
        let page = model.page
        let feature = installed ? page.installedFeature : page.feature
        let kind = installed ? page.installedKind : page.kind
        let runsOn = installed ? page.installedRunsOn : page.runsOn
        HStack(spacing: 16) {
            Picker(t("feature"), selection: Binding(
                get: { feature },
                set: { model.send(installed ? .installedFeature(choice: $0) : .feature(choice: $0)) }
            )) {
                Text(t("all")).tag(FeatureChoice.all)
                Text(t("speech-to-text")).tag(FeatureChoice.speechToText)
                Text(t("text-to-speech")).tag(FeatureChoice.textToSpeech)
            }
            .pickerStyle(.segmented)
            .fixedSize()
            // The kind splits speech to text's two stages, so it shows only
            // there.
            if feature == .speechToText {
                Picker(t("kind"), selection: Binding(
                    get: { kind },
                    set: { model.send(installed ? .installedKind(choice: $0) : .kind(choice: $0)) }
                )) {
                    Text(t("all")).tag(KindChoice.all)
                    Text(t("transcription")).tag(KindChoice.transcription)
                    Text(t("post-processing")).tag(KindChoice.postProcessing)
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
            Picker(t("runs-on"), selection: Binding(
                get: { runsOn },
                set: { model.send(installed ? .installedRunsOn(choice: $0) : .runsOn(choice: $0)) }
            )) {
                Text(t("all")).tag(RunsOnChoice.all)
                Text(t("local")).tag(RunsOnChoice.local)
                Text(t("cloud")).tag(RunsOnChoice.cloud)
            }
            .pickerStyle(.segmented)
            .fixedSize()
        }
    }
}

struct InstalledTab: View {
    @ObservedObject var model: LibraryModel

    var body: some View {
        let page = model.page
        Form {
            Section {
                ForEach(page.installed, id: \.source) { backend in
                    InstalledRowView(model: model, backend: backend)
                }
                if page.installed.isEmpty {
                    Text(page.installedTotal == 0
                         ? t("no-backends-installed-open-the-browse-tab")
                         : t("no-installed-backends-match-these-filters"))
                        .foregroundStyle(.secondary)
                }
            } header: {
                VStack(alignment: .leading, spacing: 8) {
                    LibraryFilters(model: model, installed: true)
                    Text(t("filtered-count", ["shown": "\(page.installed.count)",
                                              "total": "\(page.installedTotal)"]))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .textCase(nil)
                .padding(.bottom, 6)
            }
        }
        .formStyle(.grouped)
    }
}

struct InstalledRowView: View {
    @ObservedObject var model: LibraryModel
    let backend: InstalledRow

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 6) {
                BackendTitle(name: backend.name, version: backend.version)
                CapabilityChips(capabilities: backend.capabilities)
                if !backend.description.isEmpty {
                    Text(backend.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                ModelGroups(groups: backend.modelGroups)
                if let error = backend.uninstallError {
                    Text(t("uninstall-failed", ["err": error]))
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            Spacer()
            switch backend.update {
            case let .running(phase, fraction):
                UpdateProgress(phase: phase, fraction: fraction)
            case let .failed(message):
                Text(t("update-failed"))
                    .font(.caption)
                    .foregroundStyle(.red)
                    .help(message)
            case .idle:
                EmptyView()
            }
            if let version = backend.updateTo {
                Button(t("update-to-version", ["version": version])) {
                    model.send(.update(source: backend.source))
                }
                .buttonStyle(.borderedProminent)
            }
            RepositoryButton(url: backend.repository)
            Button(t("uninstall")) { model.send(.askUninstall(source: backend.source)) }
                .disabled(backend.isUpdating)
        }
        .padding(.vertical, 2)
    }
}

extension InstalledRow {
    /// An update of it is under way.
    var isUpdating: Bool {
        if case .running = update { return true }
        return false
    }
}

/// An update under way, where the Update button was: its step, then how far.
struct UpdateProgress: View {
    let phase: InstallStep
    let fraction: Float?

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            if let fraction {
                Text(t("update-backend-percent", ["value0": "\(Int(fraction * 100))"]))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                ProgressView(value: Double(fraction)).frame(width: 120)
            } else {
                Text(t("update-backend-progress", ["value0": phase.words]))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                ProgressView().controlSize(.small)
            }
        }
    }
}

struct BrowseTab: View {
    @ObservedObject var model: LibraryModel

    var body: some View {
        let page = model.page
        Form {
            Section {
                if !page.catalogLoaded {
                    Text(t("loading-registry-progress"))
                        .foregroundStyle(.secondary)
                } else if let error = page.catalogError {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(t("registry-unreachable", ["e": error]))
                        Button(t("retry")) { model.send(.refreshCatalog) }
                        CatalogUpdated(updated: page.catalogUpdated)
                    }
                } else if page.catalog.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(page.search.isEmpty
                             ? t("no-installable-backends-match-your-filters")
                             : t("no-backends-match-your-search-try-clearing"))
                            .foregroundStyle(.secondary)
                        CatalogUpdated(updated: page.catalogUpdated)
                    }
                } else {
                    ForEach(page.catalog, id: \.source) { entry in
                        CatalogRowView(model: model, entry: entry)
                    }
                }
            } header: {
                VStack(alignment: .leading, spacing: 8) {
                    if page.catalogLoaded && page.catalogError == nil {
                        Text(page.catalog.count == 1
                             ? t("count-1-backend-found")
                             : t("backends-count", ["shown": "\(page.catalog.count)"]))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    HStack {
                        TextField("", text: $model.search, prompt: Text(t("search-backends-or-models-progress")))
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .onChange(of: model.search) { _, text in model.send(.search(text: text)) }
                        Button(t("refresh-catalog")) { model.send(.refreshCatalog) }
                        Button(t("install-manually-progress")) { model.send(.openAdd) }
                    }
                    HStack {
                        LibraryFilters(model: model, installed: false)
                        Spacer()
                        Toggle(t("show-incompatible"), isOn: Binding(
                            get: { page.includeIncompatible },
                            set: { model.send(.includeIncompatible(on: $0)) }
                        ))
                    }
                    if page.catalogStale {
                        Text(t("catalog-refresh-failed"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .textCase(nil)
                .padding(.bottom, 6)
            }
        }
        .formStyle(.grouped)
    }
}

/// When the catalog shown was made, or that none has been read.
struct CatalogUpdated: View {
    let updated: String?

    var body: some View {
        Text(words)
            .font(.caption2)
            .foregroundStyle(.secondary)
    }

    private var words: String {
        guard let updated else { return t("catalog-not-loaded") }
        let when = (try? Date(updated, strategy: .iso8601))
            .map { $0.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened).locale(appLocale)) }
        return t("catalog-updated", ["t": when ?? updated])
    }
}

struct CatalogRowView: View {
    @ObservedObject var model: LibraryModel
    let entry: CatalogRow

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 6) {
                BackendTitle(name: entry.name, version: entry.version)
                CapabilityChips(capabilities: entry.capabilities)
                if let description = entry.description {
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                ModelGroups(groups: entry.modelGroups)
                if let version = entry.needsVersion {
                    Text(t("needs-a-newer-super-voice") + " (\(version))")
                        .font(.caption)
                        .foregroundStyle(.orange)
                } else if !entry.compatible {
                    Text(entry.incompatibleReason ?? t("incompatible-hardware-or-os"))
                        .font(.caption)
                        .foregroundStyle(.orange)
                } else if case let .failed(message) = entry.install {
                    Text(t("failed-detail", ["err": message]))
                        .font(.caption)
                        .foregroundStyle(.red)
                } else if entry.needsKey {
                    Text(t("api-key-required"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()
            RepositoryButton(url: entry.repository)
            switch entry.install {
            case let .running(phase, fraction):
                VStack(alignment: .trailing, spacing: 4) {
                    Text(phase.words)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    if let fraction {
                        ProgressView(value: Double(fraction)).frame(width: 120)
                    } else {
                        ProgressView().controlSize(.small)
                    }
                }
            case .idle, .failed:
                Button(t("install")) { model.send(.install(source: entry.source)) }
                    .buttonStyle(.borderedProminent)
                    .disabled(!entry.compatible || entry.needsVersion != nil)
            }
        }
        .padding(.vertical, 2)
    }
}

/// A card's name and the version it has, or would install.
struct BackendTitle: View {
    let name: String
    let version: String

    var body: some View {
        HStack(spacing: 8) {
            Text(name).fontWeight(.semibold)
            Text("v" + version.trimmingCharacters(in: CharacterSet(charactersIn: "v")))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}

/// Opens the backend's repository in the browser.
struct RepositoryButton: View {
    let url: String

    var body: some View {
        Button {
            if let url = URL(string: url) {
                NSWorkspace.shared.open(url)
            }
        } label: {
            IconView(icon: .gitBranch)
        }
        .buttonStyle(.borderless)
        .help(t("open-repository"))
        .accessibilityLabel(t("open-repository"))
    }
}

/// A backend's capability chips: what it runs on, where it sends audio,
/// whether it hears the dictation context or speaks in cloned voices.
struct CapabilityChips: View {
    let capabilities: [BackendCapability]

    var body: some View {
        if !capabilities.isEmpty {
            HStack(spacing: 6) {
                ForEach(Array(capabilities.enumerated()), id: \.offset) { _, capability in
                    CapabilityChip(capability: capability)
                }
            }
        }
    }
}

struct CapabilityChip: View {
    let capability: BackendCapability
    @State private var showingHosts = false

    var body: some View {
        switch capability {
        case .gpu:
            CapabilityPill(icon: .graphicsCard, word: "GPU").help(t("accelerated-on-gpu"))
        case .cpu:
            CapabilityPill(icon: .cpu, word: "CPU").help(t("runs-on-the-cpu"))
        case let .cloud(hosts, userUrl):
            if hosts.isEmpty && !userUrl {
                CapabilityPill(icon: .cloud, word: t("cloud"))
            } else {
                // The hosts it sends audio to, on a click: too long to hover.
                Button { showingHosts.toggle() } label: {
                    CapabilityPill(icon: .cloud, word: t("cloud"))
                }
                .buttonStyle(.plain)
                .popover(isPresented: $showingHosts, arrowEdge: .bottom) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(t("transmits-audio-to"))
                        ForEach(hosts, id: \.self) { host in
                            Text("• " + host).font(.body.monospaced())
                        }
                        if userUrl {
                            Text(t("another-url-you-set"))
                        }
                    }
                    .padding(12)
                }
            }
        case .contexts:
            CapabilityPill(icon: .chatText, word: t("contexts")).help(t("uses-your-dictation-context"))
        case .voiceCloning:
            CapabilityPill(icon: .userSound, word: t("voice-cloning")).help(t("speaks-in-voices-you-clone"))
        }
    }
}

/// A neutral pill: an icon and a word, a tag rather than a button.
struct CapabilityPill: View {
    let icon: Icon
    let word: String

    var body: some View {
        HStack(spacing: 4) {
            IconView(icon: icon, size: 13)
            Text(word).font(.caption)
        }
        .padding(.vertical, 2)
        .padding(.horizontal, 8)
        .background(Capsule().fill(Color(nsColor: .quaternarySystemFill)))
        .overlay(Capsule().strokeBorder(Color(nsColor: .separatorColor), lineWidth: 0.5))
    }
}

/// The models a backend ships, one line per kind: the kind, then the first
/// few names and how many more.
struct ModelGroups: View {
    let groups: [LibraryModelGroup]
    /// How many names a line lists before the rest are a count.
    private let shown = 3

    var body: some View {
        if !groups.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(groups, id: \.kind) { group in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(group.kind.words)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        HStack(spacing: 4) {
                            ForEach(group.names.prefix(shown), id: \.self) { name in
                                ModelTag(name: name)
                            }
                            if group.names.count > shown {
                                Text("+\(group.names.count - shown)")
                                    .font(.caption)
                                    .padding(.vertical, 1)
                                    .padding(.horizontal, 6)
                                    .background(Capsule().fill(Color(nsColor: .quaternarySystemFill)))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One model's name, as an outline tag.
struct ModelTag: View {
    let name: String

    var body: some View {
        Text(name)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
            .padding(.vertical, 1)
            .padding(.horizontal, 6)
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(Color(nsColor: .separatorColor)))
    }
}

/// "Install manually": a git repository's URL or a folder on this computer,
/// Check to read it, a preview of what it would install, then Install.
struct AddBackendSheet: View {
    @ObservedObject var model: LibraryModel

    var body: some View {
        let add = model.page.add
        let checking = add.preview.isChecking
        VStack(alignment: .leading, spacing: 12) {
            Text(t("install-manually"))
                .font(.title2.bold())
            Picker(t("source"), selection: Binding(
                get: { add.source },
                set: { model.send(.addSource(choice: $0)) }
            )) {
                Text(t("git-repository")).tag(AddSourceChoice.repository)
                Text(t("local-folder")).tag(AddSourceChoice.folder)
            }
            .fixedSize()
            HStack {
                switch add.source {
                case .repository:
                    TextField("", text: $model.addUrl, prompt: Text("https://github.com/owner/backend"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .onChange(of: model.addUrl) { _, text in model.send(.addUrl(text: text)) }
                        .onSubmit { check() }
                case .folder:
                    TextField("", text: $model.addFolder, prompt: Text("/Users/you/dev/my-backend"))
                        .labelsHidden()
                        .textFieldStyle(.roundedBorder)
                        .onChange(of: model.addFolder) { _, path in model.send(.addFolder(path: path)) }
                        .onSubmit { check() }
                    Button(t("choose-progress")) { model.chooseFolder() }
                }
                Button(checking ? t("checking-progress") : t("check")) { check() }
                    .disabled(checking || field.isEmpty)
            }
            ScrollView {
                AddPreviewView(preview: add.preview, source: add.source)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
            }
            .frame(minHeight: 120, maxHeight: 360)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: .quaternarySystemFill)))
            AddFooter(model: model, add: add)
        }
        .padding(20)
        .frame(width: 560)
    }

    private var field: String {
        let add = model.page.add
        return (add.source == .repository ? model.addUrl : model.addFolder)
            .trimmingCharacters(in: .whitespaces)
    }

    /// Check what the field names; a folder not named yet is chosen first.
    private func check() {
        if model.page.add.source == .folder && field.isEmpty {
            model.chooseFolder()
        } else if !field.isEmpty {
            model.send(.addCheck)
        }
    }
}

/// What the source would install, or why it couldn't be read.
struct AddPreviewView: View {
    let preview: AddPreview
    let source: AddSourceChoice

    var body: some View {
        switch preview {
        case .empty:
            VStack(alignment: .leading, spacing: 4) {
                Text(t("nothing-to-preview-yet"))
                Text(source == .repository
                     ? t("paste-a-repository-url-and-press-check")
                     : t("choose-a-folder-that-holds-a-backend"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        case let .checking(what):
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(t("reading-source", ["what": what]))
                    .foregroundStyle(.secondary)
            }
        case let .failed(message):
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                IconView(icon: .warning, size: 14, color: .systemRed)
                Text(message)
            }
        case let .ready(backend):
            PreviewedBackendView(backend: backend)
        }
    }
}

/// A backend a source would install: who it is, the facts a person agrees to
/// by installing it, and the models it serves.
struct PreviewedBackendView: View {
    let backend: AddPreviewBackend

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            // Said first: it's the most useful thing the preview can say.
            if let reason = backend.incompatible {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    IconView(icon: .warning, size: 14, color: .systemRed)
                    Text(reason.isEmpty ? t("this-backend-cannot-run-on-this-computer") : reason)
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(backend.name).font(.title3.bold())
                Text(backend.version).foregroundStyle(.secondary)
            }
            Text(backend.source)
                .font(.caption)
                .foregroundStyle(.secondary)
            if let description = backend.description {
                Text(description).foregroundStyle(.secondary)
            }
            CapabilityChips(capabilities: backend.capabilities)
            Divider()
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 12, verticalSpacing: 6) {
                fact(t("runs-as"), Text(backend.runsAs.words))
                if !backend.needs.isEmpty {
                    fact(t("needs"), Text(backend.needs.joined(separator: ", ")))
                }
                // Always, empty too: "nothing, runs offline" is what someone
                // installing a microphone reader looks for.
                if backend.reaches.isEmpty {
                    fact(t("reaches"), Text(t("nothing-runs-offline")).foregroundStyle(.green))
                } else {
                    fact(t("reaches"), Text(backend.reaches.joined(separator: ", ")).monospaced())
                }
                if let builtFor = backend.builtFor {
                    fact(t("built-for"), Text(builtFor).monospaced())
                }
                fact(t("license"), Text(backend.license))
            }
            .font(.caption)
            if !backend.models.isEmpty {
                Divider()
                Text(backend.models.count == 1 ? t("model") : t("models"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                ForEach(backend.models, id: \.name) { model in
                    HStack(spacing: 6) {
                        Text(model.name).font(.caption)
                        Spacer()
                        Text(model.kind.words)
                            .font(.caption)
                            .padding(.vertical, 1)
                            .padding(.horizontal, 6)
                            .background(Capsule().fill(Color(nsColor: .quaternarySystemFill)))
                        if model.gpu {
                            IconView(icon: .graphicsCard, size: 13).help(t("accelerated-on-gpu"))
                        }
                        if model.cpu {
                            IconView(icon: .cpu, size: 13).help(t("runs-on-the-cpu"))
                        }
                        if model.online {
                            IconView(icon: .cloud, size: 13).help(t("runs-in-the-cloud"))
                        }
                    }
                }
            }
        }
    }

    private func fact(_ label: String, _ value: Text) -> some View {
        GridRow {
            Text(label)
                .foregroundStyle(.secondary)
                .gridColumnAlignment(.leading)
            value
        }
    }
}

extension AddPreview {
    /// A source is being read.
    var isChecking: Bool {
        if case .checking = self { return true }
        return false
    }
}

/// The sheet's foot: what Install would do, or how the install it started
/// stands, then Close and Install.
struct AddFooter: View {
    @ObservedObject var model: LibraryModel
    let add: AddSheet

    var body: some View {
        HStack {
            note
                .font(.caption)
                .foregroundStyle(.secondary)
            Spacer()
            Button(t("cancel")) { model.send(.closeAdd) }
                .keyboardShortcut(.cancelAction)
            Button(label) { model.send(.addInstall) }
                .keyboardShortcut(.defaultAction)
                .buttonStyle(.borderedProminent)
                .disabled(add.installable == nil || running)
        }
    }

    private var running: Bool {
        if case .running = add.install { return true }
        return false
    }

    /// Named for what it installs, so the press and its result read as one.
    private var label: String {
        if case let .running(phase, _) = add.install {
            return t("install-backend-progress", ["value0": phase.words])
        }
        if let name = add.installable {
            return t("install-backend", ["value0": name])
        }
        if case .failed = add.install {
            return t("failed")
        }
        return t("install")
    }

    @ViewBuilder private var note: some View {
        switch add.install {
        case let .failed(message):
            Text(t("failed-detail", ["err": message])).foregroundStyle(.red)
        case let .running(_, fraction):
            if let fraction {
                Text("\(Int(fraction * 100))%")
            }
        case .idle:
            if case let .ready(backend) = add.preview, add.installable != nil {
                Text(t("backend-version-source", [
                    "value0": backend.version,
                    "where_from": add.source == .repository
                        ? t("from-its-latest-release") : t("copied-from-disk"),
                ]))
            }
        }
    }
}

extension LibraryModelKind {
    var words: String {
        switch self {
        case .transcription: t("speech-to-text")
        case .postProcessing: t("post-processing")
        case .synthesis: t("text-to-speech")
        }
    }
}

extension AddRunsAs {
    var words: String {
        switch self {
        case .wasm: t("wasm-component-sandboxed")
        case .subprocess: t("native-subprocess")
        case let .other(kind): kind
        }
    }
}

extension InstallStep {
    var words: String {
        switch self {
        case .resolving: t("registry-phase-resolving")
        case .downloading: t("registry-phase-downloading")
        case .verifying: t("registry-phase-verifying")
        case .extracting: t("registry-phase-extracting")
        case .installing: t("registry-phase-installing")
        case .rescanning: t("registry-phase-finishing")
        }
    }
}
