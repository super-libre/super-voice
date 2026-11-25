// SPDX-License-Identifier: GPL-3.0-only
// The settings app's own icons: Phosphor, regular weight, from
// super-voice-app/resources/icons/phosphor. `just macos-swift-app` copies that
// folder into the bundle's Resources/icons, so both apps draw the same files.

import AppKit
import SwiftUI

/// One of the app's icons, named as its file.
enum Icon: String {
    case arrowsClockwise = "arrows-clockwise"
    case books
    case brain
    case caretLeft = "caret-left"
    case caretRight = "caret-right"
    case check
    case cloud
    case cpu
    case gear
    case gitBranch = "git-branch"
    case graphicsCard = "graphics-card"
    case keyboard
    case microphone
    case play
    case robot
    case speakerHigh = "speaker-high"
    case stop
    case warning
    case x

    /// The icon's image, drawn only as a mask (see [`IconView`]). `nil`
    /// outside the bundle, as under `swift run`.
    @MainActor
    var image: NSImage? {
        if let cached = Self.cache[self] {
            return cached
        }
        guard let url = Bundle.main.url(forResource: rawValue, withExtension: "svg", subdirectory: "icons"),
              let image = NSImage(contentsOf: url)
        else {
            return nil
        }
        Self.cache[self] = image
        return image
    }

    @MainActor private static var cache: [Icon: NSImage] = [:]
}

/// An icon at `size` points, in the colour of the text beside it, as in the
/// settings app.
///
/// The colour is set, not inherited: the style SwiftUI hands an icon in a
/// sidebar is blended onto the window by the system, so it is not the text's,
/// and the app's own screenshots can't draw it.
struct IconView: View {
    let icon: Icon
    var size: CGFloat = 16
    /// The colour, when not the text's: `.tertiaryLabelColor` for a row's caret.
    var color: NSColor? = nil

    /// `.increased` on a selected row, whose text is white.
    @Environment(\.backgroundProminence) private var prominence

    var body: some View {
        Rectangle()
            .fill(fill)
            .frame(width: size, height: size)
            .mask {
                if let image = icon.image {
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                }
            }
    }

    private var fill: Color {
        if prominence == .increased {
            return .white
        }
        return Color(nsColor: color ?? .labelColor)
    }
}
