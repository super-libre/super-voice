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
// or `contexts-saved`, that context saved, which writes to the daemon the app
// is pointed at.
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
        case _ where ["clips", "voices", "tts-behavior", "contexts"].contains(where: scene.hasPrefix):
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

    private static func fail(_ why: String) -> Never {
        FileHandle.standardError.write(Data("snapshot: failed: \(why)\n".utf8))
        exit(1)
    }
}
#endif
