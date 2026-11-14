import SwiftUI

/// A picture from a URL, or the name's first letters while it loads (or if
/// there isn't one).
struct AvatarView: View {
    let url: String?
    let name: String
    var size: CGFloat = 36
    /// Server icons are rounded squares; people and groups are circles.
    var rounded = false

    var body: some View {
        let shape = rounded
            ? AnyShape(RoundedRectangle(cornerRadius: size * 0.28, style: .continuous))
            : AnyShape(Circle())
        Group {
            if let url, let parsed = URL(string: url) {
                AsyncImage(url: parsed) { phase in
                    if let image = phase.image {
                        image.resizable().scaledToFill()
                    } else {
                        initials
                    }
                }
            } else {
                initials
            }
        }
        .frame(width: size, height: size)
        .clipShape(shape)
    }

    private var initials: some View {
        ZStack {
            Rectangle().fill(.tint.opacity(0.25))
            Text(Self.initials(of: name))
                .font(.system(size: size * 0.38, weight: .semibold))
                .foregroundStyle(.tint)
        }
    }

    static func initials(of name: String) -> String {
        let words = name.split(whereSeparator: { $0.isWhitespace })
        let letters = words.prefix(2).compactMap(\.first)
        return letters.isEmpty ? "?" : String(letters).uppercased()
    }
}

/// An unread dot, or a mention count, at the end of a row.
struct UnreadBadge: View {
    let unread: Bool
    let mentions: UInt32

    var body: some View {
        if mentions > 0 {
            Text("\(mentions)")
                .font(.caption2.bold())
                .monospacedDigit()
                .foregroundStyle(.white)
                .padding(.horizontal, 7)
                .padding(.vertical, 2)
                .background(.red, in: .capsule)
        } else if unread {
            Circle()
                .fill(.tint)
                .frame(width: 9, height: 9)
                .accessibilityLabel("Unread")
        }
    }
}
