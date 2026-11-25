// SPDX-License-Identifier: GPL-3.0-only
// The Library page: the backends installed, each to update or uninstall, and
// the registry's catalog to install more from, with its search and filters.
// As in the settings app.

import SuperVoiceCore
import SwiftUI

@MainActor
final class LibraryModel: ObservableObject {
    static let shared = LibraryModel()

    @Published private(set) var page = LibraryPage(
        installed: [], installedTotal: 0, installedFeature: .all, installedRunsOn: .all,
        catalogLoaded: false, catalogError: nil, catalogStale: false, catalog: [], search: "",
        includeIncompatible: false, feature: .all, runsOn: .all, pendingUninstall: nil
    )

    /// The Browse tab's search field. The store gets each change.
    @Published var search = ""

    private var store: LibraryStore!

    private init() {
        let relay = LibraryRelay { [weak self] page in Task { @MainActor in self?.show(page) } }
        store = LibraryStore(observer: relay)
    }

    func send(_ action: LibraryAction) {
        store.send(action: action)
    }

    private func show(_ page: LibraryPage) {
        #if DEBUG
        if pretending { return }
        #endif
        self.page = page
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
        if Snapshot.scene == "library-browse" || Snapshot.scene == "library-sample-browse" {
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
            t("uninstall-backend-question", ["backend": page.pendingUninstall.flatMap { source in
                page.installed.first { $0.source == source }?.name
            } ?? ""]),
            isPresented: Binding(
                get: { page.pendingUninstall != nil },
                set: { shown in if !shown && page.pendingUninstall != nil { model.send(.cancelUninstall) } }
            )
        ) {
            if let source = page.pendingUninstall {
                Button(t("uninstall"), role: .destructive) { model.send(.uninstall(source: source)) }
            }
            Button(t("cancel"), role: .cancel) { model.send(.cancelUninstall) }
        } message: {
            Text(t("uninstall-backend-body"))
        }
    }
}

/// The two filters every tab has: the feature, and where it runs. `installed`
/// picks the Installed tab's, else the Browse tab's.
struct LibraryFilters: View {
    @ObservedObject var model: LibraryModel
    let installed: Bool

    var body: some View {
        let page = model.page
        let feature = installed ? page.installedFeature : page.feature
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
                LibraryFilters(model: model, installed: true)
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
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(backend.name).fontWeight(.semibold)
                    Text("v" + backend.version.trimmingCharacters(in: CharacterSet(charactersIn: "v")))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    StatePill(color: backend.online ? .systemBlue : .systemGreen,
                              word: backend.online ? t("cloud") : t("local"))
                }
                if !backend.description.isEmpty {
                    Text(backend.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(modelCount(backend.models))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let error = backend.uninstallError {
                    Text(t("uninstall-failed", ["err": error]))
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            Spacer()
            if let version = backend.updateTo {
                Button(t("update-to-version", ["version": version])) {
                    model.send(.update(source: backend.source))
                }
                .buttonStyle(.borderedProminent)
            } else if backend.installing {
                ProgressView().controlSize(.small)
            }
            Button(t("uninstall")) { model.send(.askUninstall(source: backend.source)) }
                .disabled(backend.installing)
        }
        .padding(.vertical, 2)
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
                    }
                } else if page.catalog.isEmpty {
                    Text(page.search.isEmpty
                         ? t("no-installable-backends-match-your-filters")
                         : t("no-backends-match-your-search-try-clearing"))
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(page.catalog, id: \.source) { entry in
                        CatalogRowView(model: model, entry: entry)
                    }
                }
            } header: {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        TextField("", text: $model.search, prompt: Text(t("search-backends-or-models-progress")))
                            .labelsHidden()
                            .textFieldStyle(.roundedBorder)
                            .onChange(of: model.search) { _, text in model.send(.search(text: text)) }
                        Button(t("refresh-catalog")) { model.send(.refreshCatalog) }
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

struct CatalogRowView: View {
    @ObservedObject var model: LibraryModel
    let entry: CatalogRow

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(entry.name).fontWeight(.semibold)
                    Text("v" + entry.version.trimmingCharacters(in: CharacterSet(charactersIn: "v")))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    StatePill(color: entry.online ? .systemBlue : .systemGreen,
                              word: entry.online ? t("cloud") : t("local"))
                }
                if let description = entry.description {
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(modelCount(entry.models))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let version = entry.needsVersion {
                    Text(t("needs-a-newer-super-voice") + " (\(version))")
                        .font(.caption)
                        .foregroundStyle(.orange)
                } else if !entry.compatible {
                    Text(entry.incompatibleReason ?? t("incompatible-hardware-or-os"))
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
                if case let .failed(message) = entry.install {
                    Text(t("failed-detail", ["err": message]))
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }
            Spacer()
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

/// "1 model", "4 models".
func modelCount(_ n: UInt32) -> String {
    n == 1 ? t("count-1-model") : t("models-count", ["model_count": "\(n)"])
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
