// SPDX-License-Identifier: GPL-3.0-only
// Drawing the window into a PNG, for `just macos-swift-shot`. Debug builds
// only.
//
// `SUPER_VOICE_SNAPSHOT` names the file, and `SUPER_VOICE_SCENE` the state to
// shoot: `notes`, the list (the default); `note`, the newest note open;
// `edited`, the newest note open with a sentence added through its editor and
// saved, which writes to the notes it reads, so point it at a copy; or
// `clips`, the Clips page with its clips read; `clips-playing`, the newest
// clip partway through playing, out loud; `voices`, the Voices page with its
// library read; `voices-pending`, a WAV file imported there, waiting to be
// saved; `tts-behavior`, text to speech's Behavior tab; or
// `tts-behavior-spoken`, a sentence typed there and Speak pressed, shot once
// the daemon has answered (speaking, or why not); `contexts`, speech to
// text's Contexts tab; `contexts-edit`, a new context typed into its editor;
// `contexts-saved`, that context saved, which writes to the daemon the app is
// pointed at; `stt-behavior`, speech to text's Behavior tab with its settings
// read; `stt-behavior-recorded`, its recording test run once against the
// daemon and shot when the take is over (its transcript, or why not),
// scrolled to the test at the bottom of the tab; `models`, the Models page
// once the daemon has said what fills each stage; `models-running`, that page
// with made-up stages (running, asleep, not loaded) and its transcription
// card's model running, for the layout; `models-download`, that card
// downloading its model; `models-staged`, a model staged that may not fit;
// `model-language`, a model's language button alone, its picker open, and
// `model-voice`, the synthesis model's voice picker alone, both with made-up
// languages and voices; `settings`, the Settings page once its settings are
// read;
// `settings-languages`, that page with the language picker up; `assistants`,
// the Assistants page once the daemon has listed them; `assistants-key`, a
// key for another assistant made there, which writes to the daemon the app is
// pointed at; `assistants-connected`, a made-up connected Claude Code's own
// page with two sessions, for the layout; `updates`, the Updates page once
// its status is read; `shortcuts`, the Shortcuts page once the daemon has
// listed them; `library`, the Library page's Installed tab; `library-browse`,
// its Browse tab once the catalog is read (or why not); `library-sample` and
// `library-sample-browse`, both tabs with made-up backends, for the layout;
// `library-add`, the "Install manually" sheet over them, previewing a
// made-up backend; `configure`, a made-up API-key backend's Configure sheet
// over the Library page, for the layout;
// `notes-take`, the list with a made-up take recording a new note; or
// `note-take`, the newest note open with a made-up take adding to it. A take
// scene's take, level and words are made up in the app, for the layout: a
// real one needs a microphone and a model.
// Once the page is in that state, the window is drawn into the file at one
// size and the app quits. The app draws its own window rather than a screen
// capture taking it: that needs no Screen Recording permission, so it works
// from a shell.

#if DEBUG
import AppKit
import SuperVoiceCore

@MainActor
enum Snapshot {
    /// Where this run's shot goes, or `nil` when it takes none.
    static let path = ProcessInfo.processInfo.environment["SUPER_VOICE_SNAPSHOT"]

    /// The state to shoot.
    static let scene = ProcessInfo.processInfo.environment["SUPER_VOICE_SCENE"] ?? "notes"

    /// The size every shot is taken at, so two shots of a page frame the same.
    static let size = NSSize(width: 960, height: 640)

    private static var taken = false
    private static var opened = false
    private static var edited = false
    private static var pretended = false

    /// What the `edited` scene adds to the note.
    static let addition = "Added by the edited scene."

    /// The Notes page showed `page`: move it toward the scene, or shoot it.
    static func shown(_ page: NotesPage, model: NotesModel) {
        guard path != nil, !taken, page.loaded else { return }
        switch scene {
        case "note":
            guard page.open != nil else {
                if !opened, let newest = page.notes.first {
                    opened = true
                    model.send(.open(id: newest.id))
                }
                return
            }
        case "edited":
            guard let open = page.open else {
                if !opened, let newest = page.notes.first {
                    opened = true
                    model.send(.open(id: newest.id))
                }
                return
            }
            if !edited {
                edited = true
                // Through the editor's binding, as typing would, once the
                // editor is on screen: a change made before it appears is
                // its first value, which it reports to no one.
                // On a line of its own, after the note's last.
                let text = (open.text.hasSuffix("\n") ? open.text : open.text + "\n") + addition
                Task {
                    try? await Task.sleep(for: .milliseconds(500))
                    model.text = text
                }
                return
            }
            guard open.saved, open.text.hasSuffix(addition) else { return }
        case "notes":
            guard page.open == nil else { return }
        case "notes-take", "note-take":
            if scene == "note-take" && page.open == nil {
                if !opened, let newest = page.notes.first {
                    opened = true
                    model.send(.open(id: newest.id))
                }
                return
            }
            if !pretended {
                pretended = true
                var shown = page
                shown.take = TakePage(
                    into: scene == "notes-take" ? .newNote : .openNote,
                    started: Date().timeIntervalSince1970 - 12,
                    limitSeconds: 600,
                    stopping: false,
                    preview: "Pick up milk, eggs and the good bread on the way home",
                    previewSpans: .everything
                )
                model.pretend(shown)
                MicrophoneModel.shared.pretend(Microphone(recording: true, level: 0.55, speech: true))
            }
        case "models", "models-running", "models-download", "models-staged", "settings", "settings-languages", "assistants", "assistants-key",
             "assistants-connected", "updates", "shortcuts", "library", "library-browse",
             "library-sample", "library-sample-browse", "model-language", "model-voice",
             "library-add", "configure":
            // Shot from its own page; see `modelsShown`.
            return
        case _ where ["clips", "voices", "tts-behavior", "contexts", "stt-behavior"].contains(where: scene.hasPrefix):
            // Shot from their own pages; see `clipsShown`, `voicesShown` and
            // `speechShown`.
            return
        default:
            fail("no scene is called \(scene)")
        }
        taken = true
        Task { await take() }
    }

    /// The Clips page showed `page`: shoot it once its clips are read, for the
    /// `clips` scene, or once the newest is partway through playing, for
    /// `clips-playing`, which plays it out loud.
    static func clipsShown(_ page: ClipsPage) {
        guard path != nil, !taken, scene.hasPrefix("clips"), page.loaded else { return }
        if scene == "clips-playing" {
            if let notice = page.notice {
                fail("the clip didn't play: \(notice.words)")
            }
            guard let position = page.playingPosition, position > 0.5 else {
                if !opened, let newest = page.clips.first {
                    opened = true
                    ClipsModel.shared.send(.play(id: newest.id))
                    // A clip that never gets going says how far it got.
                    Task {
                        try? await Task.sleep(for: .seconds(10))
                        if !taken {
                            let page = ClipsModel.shared.page
                            fail("the clip didn't get going: playing \(page.playing ?? "nothing"), at \(page.playingPosition.map { "\($0)s" } ?? "no position") of \(page.playingLength.map { "\($0)s" } ?? "no length")")
                        }
                    }
                }
                return
            }
        }
        taken = true
        Task { await take() }
    }

    /// The Voices page showed `page`: shoot it once its library is read, for
    /// the `voices` scene, or once a WAV file is imported, for
    /// `voices-pending`. The file is `import/Grace_Hopper.wav` in the data
    /// folder.
    static func voicesShown(_ page: VoicesPage) {
        guard path != nil, !taken, scene.hasPrefix("voices"), page.loaded else { return }
        if let notice = page.notice {
            fail("the Voices page says: \(notice.words)")
        }
        if scene == "voices-pending" {
            guard page.pending != nil else {
                if !opened {
                    opened = true
                    let data = ProcessInfo.processInfo.environment["XDG_DATA_HOME"] ?? NSTemporaryDirectory()
                    VoicesModel.shared.store.filePicked(path: data + "/import/Grace_Hopper.wav")
                }
                return
            }
        } else if scene != "voices" {
            fail("no scene is called \(scene)")
        }
        taken = true
        Task { await take() }
    }

    /// The speech test showed `page`: shoot it at once, for `tts-behavior`, or
    /// once a sentence typed into it has been spoken or refused, for
    /// `tts-behavior-spoken`.
    static func speechShown(_ page: SpeechPage) {
        guard path != nil, !taken, scene.hasPrefix("tts-behavior") else { return }
        switch scene {
        case "tts-behavior":
            break
        case "tts-behavior-spoken":
            guard page.speaking || page.notice != nil else {
                if !opened {
                    opened = true
                    // Through the field's binding, as typing would, once the
                    // field is on screen.
                    Task {
                        try? await Task.sleep(for: .milliseconds(500))
                        SpeechModel.shared.text = "Hello from the Mac."
                        try? await Task.sleep(for: .milliseconds(300))
                        SpeechModel.shared.send(.speak)
                    }
                }
                return
            }
        default:
            fail("no scene is called \(scene)")
        }
        taken = true
        Task { await take() }
    }

    /// The Contexts page showed `page`: shoot it once its list is read, for
    /// `contexts`; with a new context typed into the editor, for
    /// `contexts-edit`; or once that context is saved, for `contexts-saved`.
    static func contextsShown(_ page: ContextsPage) {
        guard path != nil, !taken, scene.hasPrefix("contexts"), page.loaded else { return }
        let model = ContextsModel.shared
        switch scene {
        case "contexts":
            break
        case "contexts-edit", "contexts-saved":
            if !opened {
                opened = true
                model.send(.create)
                // As typing would, through the fields, once the sheet is up.
                Task {
                    try? await Task.sleep(for: .milliseconds(600))
                    model.name = "Coding"
                    model.prompt = "Be terse."
                    model.terms[0].text = "kubectl"
                    model.send(.termChanged(index: 0, value: "kubectl"))
                    model.send(.termSubmitted(index: 0))
                    try? await Task.sleep(for: .milliseconds(300))
                    model.terms[1].text = "rebase"
                    model.send(.termChanged(index: 1, value: "rebase"))
                    try? await Task.sleep(for: .milliseconds(300))
                    edited = true
                    if scene == "contexts-saved" {
                        model.send(.save)
                    } else {
                        taken = true
                        await take()
                    }
                }
                return
            }
            guard scene == "contexts-saved", edited, page.editor == nil,
                  page.contexts.contains(where: { $0.name == "Coding" })
            else {
                if let error = page.editor?.error {
                    fail("the context wasn't saved: \(error)")
                }
                return
            }
        default:
            fail("no scene is called \(scene)")
        }
        taken = true
        Task { await take() }
    }

    /// Speech to text's Behavior tab showed `page`: shoot it once its
    /// settings are read, for `stt-behavior`.
    static func sttBehaviorShown(_ page: BehaviorPage) {
        guard path != nil, !taken, scene.hasPrefix("stt-behavior"), page.loaded else { return }
        switch scene {
        case "stt-behavior":
            break
        case "stt-behavior-recorded":
            // Shot from the test; see `recordingTestShown`.
            if !opened {
                opened = true
                RecordingTestModel.shared.send(.start)
            }
            return
        default:
            fail("no scene is called \(scene)")
        }
        taken = true
        Task { await take() }
    }

    /// The Settings page showed `page`: shoot it once its settings are read,
    /// for `settings`, and with the language picker up, for
    /// `settings-languages`.
    static func settingsShown(_ page: SettingsPage) {
        guard path != nil, !taken, scene.hasPrefix("settings"), page.loaded else { return }
        taken = true
        Task { await take() }
    }

    /// The Assistants page showed `page`: shoot it once the daemon has listed
    /// them, for `assistants`, or once a key made there is shown, for
    /// `assistants-key`.
    static func assistantsShown(_ page: AssistantsPage) {
        guard path != nil, !taken, scene.hasPrefix("assistants"), page.loaded else { return }
        if scene == "assistants-connected" {
            var shown = page
            shown.open = "claude-code"
            shown.assistants = [AssistantRow(
                id: "claude-code", name: "Claude Code", standing: .connected, readsEveryReply: true,
                connectedHere: false, disconnecting: false,
                flags: [
                    FlagRow(flag: .showTalk, on: true),
                    FlagRow(flag: .autosendDictatedPrompts, on: false),
                    FlagRow(flag: .announceWhenNeeded, on: true),
                    FlagRow(flag: .conversationContext, on: true),
                ],
                sessions: [
                    SessionRow(name: "super-voice", folder: "~/super-libre/super-voice", agents: 2,
                               speaking: true, listening: false, ago: .now),
                    SessionRow(name: "notes", folder: "~/notes", agents: 0,
                               speaking: false, listening: false, ago: .minutes(n: 12)),
                ]
            )]
            AssistantsModel.shared.pretend(shown)
        } else if scene == "assistants-key" {
            guard case .made = page.key else {
                if !opened {
                    opened = true
                    let model = AssistantsModel.shared
                    model.send(.askKey)
                    // As typing would, through the field, once the sheet is up.
                    Task {
                        try? await Task.sleep(for: .milliseconds(600))
                        model.keyName = "My editor"
                        try? await Task.sleep(for: .milliseconds(300))
                        model.send(.createKey)
                    }
                }
                return
            }
        }
        taken = true
        Task { await take() }
    }

    /// The Updates page showed `page`: shoot it once its status is read, for
    /// `updates`.
    static func updatesShown(_ page: UpdatesPage) {
        guard path != nil, !taken, scene == "updates", page.loaded else { return }
        taken = true
        Task { await take() }
    }

    /// The Shortcuts page showed `page`: shoot it once the daemon has listed
    /// them, for `shortcuts`.
    static func shortcutsShown(_ page: ShortcutsPage) {
        guard path != nil, !taken, scene == "shortcuts", page.loaded else { return }
        taken = true
        Task { await take() }
    }

    /// The Library page showed `page`: shoot it once the catalog is read
    /// (or why not), for `library` and `library-browse`; with made-up
    /// backends for `library-sample` and `library-sample-browse`, and with the
    /// "Install manually" sheet up over them, previewing one, for
    /// `library-add`.
    static func libraryShown(_ page: LibraryPage) {
        guard path != nil, !taken, scene.hasPrefix("library") || scene == "configure", page.catalogLoaded
        else { return }
        if scene == "configure" {
            pretendConfigure(page)
            taken = true
            Task {
                // The sheet slides in.
                try? await Task.sleep(for: .milliseconds(600))
                await take()
            }
            return
        }
        if scene.hasPrefix("library-sample") || scene == "library-add" {
            var shown = page
            shown.catalogError = nil
            shown.catalogUpdated = "2026-11-20T09:30:00Z"
            shown.installedTotal = 2
            shown.installed = [
                InstalledRow(source: "github.com/super-libre/super-stt-whisper", name: "Whisper", version: "0.2.1",
                             description: "Local Whisper speech to text.", online: false,
                             capabilities: [.gpu, .cpu, .contexts],
                             modelGroups: [LibraryModelGroup(kind: .transcription, names: [
                                 "tiny", "base", "small", "medium", "large-v3", "large-v3-turbo",
                             ])],
                             repository: "https://github.com/super-libre/super-stt-whisper",
                             updateTo: nil, update: .running(phase: .downloading, fraction: 0.6),
                             uninstallError: nil),
                InstalledRow(source: "github.com/super-libre/super-tts-kokoro", name: "Kokoro", version: "0.1.4",
                             description: "Fast local text to speech.", online: false,
                             capabilities: [.cpu, .voiceCloning],
                             modelGroups: [LibraryModelGroup(kind: .synthesis, names: ["kokoro-82m"])],
                             repository: "https://github.com/super-libre/super-tts-kokoro",
                             updateTo: "0.1.5", update: .idle, uninstallError: nil),
            ]
            shown.catalog = [
                CatalogRow(source: "github.com/super-libre/super-stt-qwen-asr", name: "Qwen", version: "0.1.0",
                           description: "Local Qwen3-ASR speech to text with streamed previews.", online: false,
                           capabilities: [.gpu, .cpu, .contexts],
                           modelGroups: [
                               LibraryModelGroup(kind: .transcription, names: ["qwen3-asr-0.6b", "qwen3-asr-1.7b"]),
                               LibraryModelGroup(kind: .postProcessing, names: ["qwen3-0.6b-tidy"]),
                           ],
                           repository: "https://github.com/super-libre/super-stt-qwen-asr", needsKey: false,
                           compatible: true, incompatibleReason: nil, needsVersion: nil,
                           install: .running(phase: .downloading, fraction: 0.42)),
                CatalogRow(source: "github.com/super-libre/super-stt-deepgram", name: "Deepgram", version: "0.3.0",
                           description: "Deepgram's speech to text, online.", online: true,
                           capabilities: [.cloud(hosts: ["api.deepgram.com"], userUrl: false)],
                           modelGroups: [LibraryModelGroup(kind: .transcription, names: [
                               "nova-3", "nova-2", "enhanced",
                           ])],
                           repository: "https://github.com/super-libre/super-stt-deepgram", needsKey: true,
                           compatible: true, incompatibleReason: nil, needsVersion: nil, install: .idle),
                CatalogRow(source: "github.com/super-libre/super-stt-voxtral", name: "Voxtral", version: "0.4.0",
                           description: "Mistral's Voxtral, local.", online: false, capabilities: [.gpu],
                           modelGroups: [LibraryModelGroup(kind: .transcription, names: [
                               "voxtral-mini", "voxtral-small",
                           ])],
                           repository: "https://github.com/super-libre/super-stt-voxtral", needsKey: false,
                           compatible: true, incompatibleReason: nil, needsVersion: "v0.4.0", install: .idle),
            ]
            if scene == "library-add" {
                let url = "https://github.com/someone/super-stt-parakeet"
                shown.add = AddSheet(
                    open: true, source: .repository, url: url, folder: "",
                    preview: .ready(backend: AddPreviewBackend(
                        source: "github.com/someone/super-stt-parakeet", name: "Parakeet", version: "0.2.0",
                        description: "NVIDIA's Parakeet speech to text, local.", incompatible: nil,
                        capabilities: [.gpu, .cpu],
                        runsAs: .subprocess, needs: [], reaches: ["huggingface.co"],
                        builtFor: "aarch64-apple-darwin, metal", license: "Apache-2.0",
                        models: [
                            AddPreviewModel(name: "parakeet-tdt-0.6b-v2", kind: .transcription,
                                            gpu: true, cpu: true, online: false),
                            AddPreviewModel(name: "parakeet-tdt-0.6b-v3", kind: .transcription,
                                            gpu: true, cpu: true, online: false),
                        ]
                    )),
                    install: .idle, installable: "Parakeet"
                )
            }
            LibraryModel.shared.pretend(shown)
        }
        taken = true
        // A moment for the sheet to come up.
        Task {
            if scene == "library-add" {
                try? await Task.sleep(for: .milliseconds(500))
            }
            await take()
        }
    }

    /// The `configure` scene: an online backend with an API key not set yet,
    /// a switch, a pick from a list, a text option set away from its default,
    /// and the context picker, its sheet up over the Library page.
    private static func pretendConfigure(_ page: LibraryPage) {
        let source = "github.com/super-libre/super-stt-openai"
        var shown = page
        shown.catalogError = nil
        shown.installedTotal = 2
        shown.installed = [
            InstalledRow(source: source, name: "OpenAI", version: "0.3.0",
                         description: "OpenAI's speech to text, online.", online: true,
                         capabilities: [.cloud(hosts: ["api.openai.com"], userUrl: false)],
                         modelGroups: [LibraryModelGroup(kind: .transcription, names: ["whisper-1", "gpt-4o-transcribe"])],
                         repository: "https://\(source)",
                         updateTo: nil, update: .idle, uninstallError: nil),
            InstalledRow(source: "github.com/super-libre/super-stt-whisper", name: "Whisper", version: "0.2.1",
                         description: "Local Whisper speech to text.", online: false,
                         capabilities: [.gpu, .cpu],
                         modelGroups: [LibraryModelGroup(kind: .transcription, names: ["base", "small", "large-v3-turbo"])],
                         repository: "https://github.com/super-libre/super-stt-whisper",
                         updateTo: nil, update: .idle, uninstallError: nil),
        ]
        LibraryModel.shared.pretend(shown)
        ConfigureModel.shared.pretend(ConfigureSheet(
            source: source, name: "OpenAI", loaded: true,
            context: ContextPicker(
                options: [
                    ContextOption(choice: .followActive, name: t("follow-active-context", ["value0": "Coding"])),
                    ContextOption(choice: .nothing, name: t("no-context")),
                    ContextOption(choice: .pinned(id: "coding"), name: "Coding"),
                    ContextOption(choice: .pinned(id: "email"), name: "Email"),
                ],
                selected: .followActive,
                hint: t("sending-context", ["value0": "Coding"])
            ),
            rows: [
                ConfigureRow(name: "api_key", label: "API key", hint: "Your OpenAI API key.", required: true,
                             control: .secret(configured: false, input: "")),
                ConfigureRow(name: "stream", label: "Stream", hint: "Send audio as it is recorded.",
                             required: false, control: .bool(on: true)),
                ConfigureRow(name: "model_size", label: "Speed", hint: "Faster, or more accurate.",
                             required: false,
                             control: .choice(choices: ["fast", "balanced", "accurate"], selected: "balanced")),
                ConfigureRow(name: "base_url", label: "Base URL",
                             hint: t("setting-hint-default", ["hint": "Where requests go.",
                                                              "default": "https://api.openai.com/v1"]),
                             required: false,
                             control: .text(input: "https://gateway.example.com/v1",
                                            value: "https://gateway.example.com/v1", overridden: true)),
            ],
            unmet: ["API key"],
            notice: nil
        ))
    }

    /// The Models page showed `header`: shoot it once the daemon has said what
    /// fills each stage, for `models`.
    static func modelsShown(_ header: HeaderStatus) {
        guard path != nil, !taken, scene.hasPrefix("models") else { return }
        if scene != "models" {
            var shown = header
            shown.connected = true
            shown.speechToText = .ready
            shown.textToSpeech = .notLoaded
            shown.pipeline = [
                PipelineStep(
                    stage: .transcription, readiness: .ready, backend: "Whisper",
                    model: .running(name: "large-v3-turbo", device: "Metal")
                ),
                PipelineStep(
                    stage: .postProcessing, readiness: .idle, backend: "S1-mini",
                    model: .idle(name: "s1-mini-q4_k_m")
                ),
                PipelineStep(
                    stage: .synthesis, readiness: .notLoaded, backend: "Kokoro",
                    model: .notLoaded
                ),
            ]
            HeaderModel.shared.pretend(shown)
            ModelsModel.shared.pretend(ModelsPage(cards: [madeUpCard(scene)], notice: nil))
        } else if header.pipeline.isEmpty {
            return
        }
        taken = true
        Task { await take() }
    }

    /// A made-up transcription card for `scene`: `models-running`, its model
    /// up on Metal; `models-download`, a model downloading; `models-staged`, a
    /// model staged that may not fit on the GPU.
    private static func madeUpCard(_ scene: String) -> StageCard {
        let whisper = CardBackend(
            source: "github.com/super-libre/super-voice-whisper", name: "Whisper", version: "0.2.1",
            description: "Local Whisper speech to text.",
            repository: "https://github.com/super-libre/super-voice-whisper",
            chips: BackendChips(gpu: true, cpu: true, online: false), models: 3
        )
        let qwen = CardBackend(
            source: "github.com/super-libre/super-voice-qwen-asr", name: "Qwen ASR", version: "0.1.0",
            description: "", repository: "https://github.com/super-libre/super-voice-qwen-asr",
            chips: BackendChips(gpu: true, cpu: true, online: false), models: 1
        )
        let devices = [Choice(id: "metal", name: "Metal"), Choice(id: "cpu", name: "CPU")]
        let notes = CardNotes(memory: nil, fallback: nil, warning: nil, unmet: [])
        var card = StageCard(
            stage: .transcription, source: whisper.source, backends: [whisper, qwen], backend: whisper,
            model: .running(model: "large-v3-turbo", idle: false),
            models: ["base", "small", "large-v3-turbo"], devices: devices, device: "metal",
            notes: notes, progress: .none, canLoad: false, reloadable: false, busy: false
        )
        switch scene {
        case "models-download":
            card.model = .staging(model: "large-v3-turbo")
            card.progress = .download(
                line: t("provision-model-progress", [
                    "value0": t("downloading"), "value1": "large-v3-turbo",
                    "value2": "2", "value3": "3", "value4": "42.0",
                ]),
                fraction: 0.42,
                bytes: t("download-size", ["mb": "690.5", "total": "1624.0"])
            )
        case "models-staged":
            card.model = .staging(model: "large-v3-turbo")
            card.canLoad = true
            card.notes = CardNotes(
                memory: t("model-needs-memory", ["size": t("size-gb", ["size": "3.1"])]),
                fallback: nil,
                warning: t("model-memory-warning", [
                    "value0": t("memory-size", ["gib": "3.1"]), "value1": t("memory-size", ["gib": "2.0"]),
                ]),
                unmet: []
            )
        default:
            break
        }
        return card
    }

    /// A model's language button or voice picker is up, with made-up
    /// languages and voices: shoot it, for `model-language` (the picker open)
    /// and `model-voice`.
    static func modelDetailsShown() {
        guard path != nil, !taken, ["model-language", "model-voice"].contains(scene) else { return }
        taken = true
        Task { await take() }
    }

    /// The recording test showed `page`: shoot it once its take is over, for
    /// `stt-behavior-recorded`.
    static func recordingTestShown(_ page: RecordingTestPage) {
        guard path != nil, !taken, scene == "stt-behavior-recorded", opened,
              page.heard != nil, !page.recording
        else { return }
        taken = true
        Task { await take() }
    }

    private static func take() async {
        guard let path else { return }
        guard let window = NSApp.windows.first(where: \.isVisible) else {
            fail("no window is open")
        }
        // A run pointed at a daemon waits for it, so the header's pills are in
        // the shot.
        if let socket = ProcessInfo.processInfo.environment["SUPER_VOICE_HTTP_SOCKET"] {
            for _ in 0..<50 where !HeaderModel.shared.header.connected {
                try? await Task.sleep(for: .milliseconds(100))
            }
            if !HeaderModel.shared.header.connected {
                fail("the daemon at \(socket) didn't answer")
            }
        }
        window.setContentSize(size)
        // In front and key, as the app is in use: an app opened from a shell
        // starts behind, and macOS draws a window that isn't key without its
        // accent colour (grey window buttons, a grey selected row).
        NSApp.activate()
        window.makeKeyAndOrderFront(nil)
        // A moment for the new size's layout, and the list's rows, to draw.
        try? await Task.sleep(for: .milliseconds(700))
        // The recording test is at the bottom of its tab, below the window.
        if scene == "stt-behavior-recorded" {
            scrollToBottom(in: window.contentView)
            try? await Task.sleep(for: .milliseconds(300))
        }
        // The frame view, so the title bar is in the shot too.
        // Not in front, the window draws without its accent colours: a shot
        // of that is not what the app looks like, so it is refused, unless
        // the run asked for one anyway (`SUPER_VOICE_SNAPSHOT_INACTIVE=1`).
        // With a sheet up, the sheet is the key window.
        let key = window.isKeyWindow || window.attachedSheet?.isKeyWindow == true
        if !(NSApp.isActive && key),
           ProcessInfo.processInfo.environment["SUPER_VOICE_SNAPSHOT_INACTIVE"] != "1"
        {
            fail("the app isn't in front (a screen saver, a locked screen, or another app in the way), so the window would draw without its accent colours")
        }
        guard let view = window.contentView?.superview ?? window.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds)
        else {
            fail("the window has nothing to draw")
        }
        view.cacheDisplay(in: view.bounds, to: rep)
        // A sheet is a window of its own, over this one: drawn on top, where
        // it sits.
        if let sheet = window.attachedSheet,
           let sheetView = sheet.contentView?.superview ?? sheet.contentView,
           let sheetRep = sheetView.bitmapImageRepForCachingDisplay(in: sheetView.bounds)
        {
            sheetView.cacheDisplay(in: sheetView.bounds, to: sheetRep)
            let at = NSPoint(
                x: sheet.frame.minX - window.frame.minX,
                y: sheet.frame.minY - window.frame.minY
            )
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
            sheetRep.draw(in: NSRect(origin: at, size: sheet.frame.size))
            NSGraphicsContext.restoreGraphicsState()
        }
        guard let png = rep.representation(using: .png, properties: [:]) else {
            fail("the window could not be encoded as a PNG")
        }
        do {
            try png.write(to: URL(fileURLWithPath: path))
        } catch {
            fail("\(path) could not be written: \(error)")
        }
        NSApp.terminate(nil)
        // An open sheet holds termination off, and the shot is all this run
        // was for.
        try? await Task.sleep(for: .seconds(2))
        exit(0)
    }

    /// Scroll the tallest scroll view under `view` to its end.
    private static func scrollToBottom(in view: NSView?) {
        func scrollViews(_ view: NSView) -> [NSScrollView] {
            (view as? NSScrollView).map { [$0] } ?? [] + view.subviews.flatMap(scrollViews)
        }
        guard let view,
              let scroller = scrollViews(view).max(by: {
                  ($0.documentView?.frame.height ?? 0) < ($1.documentView?.frame.height ?? 0)
              }),
              let document = scroller.documentView
        else { fail("no scroll view to scroll to the test") }
        let clip = scroller.contentView
        let end = document.isFlipped ? max(0, document.frame.height - clip.bounds.height) : 0
        clip.scroll(to: NSPoint(x: 0, y: end))
        scroller.reflectScrolledClipView(clip)
    }

    private static func fail(_ why: String) -> Never {
        FileHandle.standardError.write(Data("snapshot: failed: \(why)\n".utf8))
        exit(1)
    }
}
#endif
