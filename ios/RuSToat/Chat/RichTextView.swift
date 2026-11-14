import SwiftUI
import UIKit

/// A message's text, block by block, as the core parsed it: paragraphs and
/// headings as text, then quotes, code, lists, rules, maths and tables as
/// their own views.
struct RichTextView: View {
    let blocks: [RichBlock]
    var jumbo = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                RichBlockView(block: block, jumbo: jumbo)
            }
        }
    }
}

/// One block. Nested blocks (in quotes and lists) go through `AnyView`,
/// since a view can't contain its own type directly.
struct RichBlockView: View {
    let block: RichBlock
    var jumbo = false

    var body: some View {
        switch block {
        case .paragraph(let spans):
            RichSpansText(spans: spans)
                .font(jumbo ? .system(size: 40) : .body)
        case .heading(let level, let spans):
            RichSpansText(spans: spans)
                .font(Self.headingFont(level))
        case .quote(let blocks):
            HStack(alignment: .top, spacing: 10) {
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(.secondary.opacity(0.5))
                    .frame(width: 3)
                AnyView(RichTextView(blocks: blocks))
                    .foregroundStyle(.secondary)
            }
            .fixedSize(horizontal: false, vertical: true)
        case .code(let language, let code):
            CodeBlockView(language: language, code: code)
        case .list(let ordered, let start, let items):
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(Self.marker(ordered: ordered, number: Int(start) + index, checked: item.checked))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                        AnyView(RichTextView(blocks: item.blocks))
                    }
                }
            }
        case .rule:
            Divider().padding(.vertical, 4)
        case .math(let source):
            CodeBlockView(language: "math", code: source)
        case .table(let header, let rows):
            ScrollView(.horizontal, showsIndicators: false) {
                Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 6) {
                    GridRow {
                        ForEach(Array(header.enumerated()), id: \.offset) { _, cell in
                            RichSpansText(spans: cell.spans).bold()
                        }
                    }
                    Divider()
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                        GridRow {
                            ForEach(Array(row.cells.enumerated()), id: \.offset) { _, cell in
                                RichSpansText(spans: cell.spans)
                            }
                        }
                    }
                }
                .padding(10)
            }
            .background(.quaternary.opacity(0.4), in: .rect(cornerRadius: 10))
        }
    }

    static func headingFont(_ level: UInt8) -> Font {
        switch level {
        case 1: .title2.bold()
        case 2: .title3.bold()
        default: .headline
        }
    }

    static func marker(ordered: Bool, number: Int, checked: Bool?) -> String {
        switch checked {
        case true?: "☑"
        case false?: "☐"
        case nil: ordered ? "\(number)." : "•"
        }
    }
}

/// A run of spans as one `Text`, so it wraps as a paragraph. Mentions and
/// timestamps get a pill behind them, and hidden spoiler words a bar over
/// them, drawn by `RichTextRenderer`. SwiftUI doesn't draw a custom
/// renderer on selectable text, so only paragraphs without those are
/// selectable (the message menu's Copy Text covers the rest).
struct RichSpansText: View {
    let spans: [RichSpan]

    var body: some View {
        if spans.contains(where: { $0.kind != .text && $0.kind != .break }) {
            Self.text(spans)
                .textRenderer(RichTextRenderer())
                .fixedSize(horizontal: false, vertical: true)
        } else {
            Text(Self.attributed(spans))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The spans as one `Text`, each marked for the renderer as needed.
    static func text(_ spans: [RichSpan]) -> Text {
        spans.reduce(Text(verbatim: "")) { result, span in
            var part = Text(attributed([span]))
            switch span.kind {
            case .mention(let kind, let colour):
                part = part.customAttribute(Pill(tint: mentionTint(kind, colour)))
            case .time:
                part = part.customAttribute(Pill(tint: .secondary))
            case .spoiler:
                part = part.customAttribute(SpoilerBar())
            case .text, .break:
                break
            }
            return Text("\(result)\(part)")
        }
    }

    /// A mention's colour: a role's own, else the accent colour.
    static func mentionTint(_ kind: MentionKind, _ colour: String?) -> Color {
        if kind == .role, let colour, let rgb = UInt32(colour.dropFirst(), radix: 16) {
            return Color(red: Double((rgb >> 16) & 0xff) / 255,
                         green: Double((rgb >> 8) & 0xff) / 255,
                         blue: Double(rgb & 0xff) / 255)
        }
        return .accentColor
    }

    /// `spans` as styled text: bold, italics, strikethrough and code from
    /// their flags, links from their URLs, and mentions in their colour.
    nonisolated static func attributed(_ spans: [RichSpan]) -> AttributedString {
        var out = AttributedString()
        for span in spans {
            var part = AttributedString(span.text)
            var intent: InlinePresentationIntent = []
            if span.bold { intent.insert(.stronglyEmphasized) }
            if span.italic { intent.insert(.emphasized) }
            if span.strike { intent.insert(.strikethrough) }
            if span.code { intent.insert(.code) }
            if !intent.isEmpty { part.inlinePresentationIntent = intent }
            if let link = span.link, let url = URL(string: link) {
                part.link = url
            }
            switch span.kind {
            case .mention:
                // Drawn in the pill's colour, not as a link's underline.
                part.underlineStyle = nil
                part.inlinePresentationIntent = (part.inlinePresentationIntent ?? []).union(.stronglyEmphasized)
            default:
                break
            }
            out.append(part)
        }
        return out
    }
}

/// Marks a mention or timestamp: drawn on a pill of `tint`.
struct Pill: TextAttribute {
    let tint: Color
}

/// Marks a hidden spoiler word: drawn as a bar until revealed.
struct SpoilerBar: TextAttribute {}

/// Draws message text, with pills behind mentions and timestamps and bars
/// in place of hidden spoiler words. The layout is untouched, so lines
/// don't move when a spoiler is revealed or a pill is drawn.
struct RichTextRenderer: TextRenderer {
    func draw(layout: Text.Layout, in context: inout GraphicsContext) {
        for line in layout {
            for run in line {
                let bounds = run.typographicBounds.rect
                if let pill = run[Pill.self] {
                    let shape = Path(roundedRect: bounds.insetBy(dx: -2, dy: -1), cornerRadius: 5)
                    context.fill(shape, with: .color(pill.tint.opacity(0.18)))
                    context.draw(run)
                } else if run[SpoilerBar.self] != nil {
                    let shape = Path(roundedRect: bounds.insetBy(dx: 0, dy: 1), cornerRadius: 4)
                    context.fill(shape, with: .color(.gray.opacity(0.55)))
                } else {
                    context.draw(run)
                }
            }
        }
    }
}

/// A code block: monospaced, scrolling sideways rather than wrapping, with
/// a copy button.
struct CodeBlockView: View {
    let language: String?
    let code: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                if let language {
                    Text(language)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc") {
                    UIPasteboard.general.string = code
                    copied = true
                    Task {
                        try? await Task.sleep(for: .seconds(1.5))
                        copied = false
                    }
                }
                .labelStyle(.iconOnly)
                .font(.caption)
                .buttonStyle(.borderless)
                .accessibilityLabel("Copy code")
            }
            ScrollView(.horizontal, showsIndicators: false) {
                Text(code)
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
                    .fixedSize(horizontal: true, vertical: true)
            }
        }
        .padding(10)
        .background(.quaternary.opacity(0.5), in: .rect(cornerRadius: 10))
    }
}
