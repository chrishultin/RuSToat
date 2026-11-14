import SwiftUI

/// An emoji: Unicode as text, custom as its image (from the shared cache)
/// or its `:name:` until that loads.
struct EmojiView: View {
    let emoji: EmojiItem
    var size: CGFloat = 18

    var body: some View {
        switch emoji {
        case .unicode(let text):
            Text(text).font(.system(size: size))
        case .custom(_, let name, let url):
            if let url, let image = EmojiImages.shared.image(url, size: size) {
                Image(uiImage: image)
                    .accessibilityLabel(name)
            } else {
                Text(name).font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// A message's reactions: chips that wrap, each its emoji and count, tinted
/// when the user is one of them. Tap to join or leave; long-press to see who.
struct ReactionsView: View {
    let reactions: [ReactionItem]
    let toggle: (String) -> Void
    let add: () -> Void

    var body: some View {
        FlowLayout(spacing: 6) {
            ForEach(reactions, id: \.key) { reaction in
                Button { toggle(reaction.key) } label: {
                    HStack(spacing: 4) {
                        EmojiView(emoji: reaction.emoji, size: 16)
                        Text("\(reaction.count)")
                            .font(.caption.weight(.semibold))
                            .monospacedDigit()
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(reaction.mine ? AnyShapeStyle(.tint.opacity(0.2)) : AnyShapeStyle(.quaternary.opacity(0.6)),
                                in: .capsule)
                    .overlay {
                        if reaction.mine {
                            Capsule().strokeBorder(.tint, lineWidth: 1)
                        }
                    }
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Text(Self.who(reaction))
                }
                .accessibilityLabel("\(reaction.count) \(reaction.mine ? ", including you" : "")")
            }
            Button(action: add) {
                Image(systemName: "face.smiling")
                    .font(.caption)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 5)
                    .background(.quaternary.opacity(0.6), in: .capsule)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Add Reaction")
        }
    }

    /// "You, Ada and 3 others".
    static func who(_ reaction: ReactionItem) -> String {
        var names = reaction.mine ? ["You"] : []
        names += reaction.names
        let others = Int(reaction.count) - names.count
        if others > 0 {
            names.append(others == 1 ? "1 other" : "\(others) others")
        }
        switch names.count {
        case 0: return ""
        case 1: return names[0]
        default: return names.dropLast().joined(separator: ", ") + " and " + names.last!
        }
    }
}

/// Lays its children out in rows, wrapping to the next row when one is full.
struct FlowLayout: Layout {
    var spacing: CGFloat = 6

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = rows(for: subviews, width: proposal.width ?? .infinity)
        let height = rows.map(\.height).reduce(0, +) + spacing * CGFloat(max(rows.count - 1, 0))
        let width = rows.map(\.width).max() ?? 0
        return CGSize(width: width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in rows(for: subviews, width: bounds.width) {
            var x = bounds.minX
            for index in row.indices {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y + (row.height - size.height) / 2),
                                      proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    struct Row {
        var indices: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    func rows(for subviews: Subviews, width: CGFloat) -> [Row] {
        var rows: [Row] = []
        var row = Row()
        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)
            let needed = row.indices.isEmpty ? size.width : row.width + spacing + size.width
            if needed > width, !row.indices.isEmpty {
                rows.append(row)
                row = Row()
            }
            row.width = row.indices.isEmpty ? size.width : row.width + spacing + size.width
            row.height = max(row.height, size.height)
            row.indices.append(index)
        }
        if !row.indices.isEmpty { rows.append(row) }
        return rows
    }
}
