// SPDX-License-Identifier: GPL-3.0-only
// Drawing the window into a PNG, for `just macos-swift-shot`. Debug builds
// only.
//
// `SUPER_VOICE_SNAPSHOT` names the file. Once the page has loaded, the window
// is drawn into it at one size and the app quits. The app draws its own
// window rather than a screen capture taking it: that needs no Screen
// Recording permission, so it works from a shell.

#if DEBUG
import AppKit

@MainActor
enum Snapshot {
    /// Where this run's shot goes, or `nil` when it takes none.
    static let path = ProcessInfo.processInfo.environment["SUPER_VOICE_SNAPSHOT"]

    /// The size every shot is taken at, so two shots of a page frame the same.
    static let size = NSSize(width: 720, height: 480)

    /// The page is up: take the shot, if this run takes one, and quit.
    static func pageReady() async {
        guard let path else { return }
        guard let window = NSApp.windows.first(where: \.isVisible) else {
            fail("no window is open")
        }
        window.setContentSize(size)
        // A moment for the new size's layout, and the list's rows, to draw.
        try? await Task.sleep(for: .milliseconds(500))
        // The frame view, so the title bar is in the shot too.
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
