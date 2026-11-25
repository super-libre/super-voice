// SPDX-License-Identifier: GPL-3.0-only
// Drawing the window into a PNG, for `just macos-swift-shot`. Debug builds
// only.
//
// `SUPER_VOICE_SNAPSHOT` names the file, and `SUPER_VOICE_SCENE` the state to
// shoot: `notes`, the list (the default); `note`, the newest note open; or
// `edited`, the newest note open with a sentence added through its editor and
// saved. `edited` writes to the notes it reads, so point it at a copy.
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
        if !(NSApp.isActive && window.isKeyWindow),
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
        guard let png = rep.representation(using: .png, properties: [:]) else {
            fail("the window could not be encoded as a PNG")
        }
        do {
            try png.write(to: URL(fileURLWithPath: path))
        } catch {
            fail("\(path) could not be written: \(error)")
        }
        NSApp.terminate(nil)
    }

    private static func fail(_ why: String) -> Never {
        FileHandle.standardError.write(Data("snapshot: failed: \(why)\n".utf8))
        exit(1)
    }
}
#endif
