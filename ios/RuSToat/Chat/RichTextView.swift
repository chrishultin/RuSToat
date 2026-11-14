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

/// A run of spans as one `Text`, so it wraps and selects as a paragraph.
struct RichSpansText: View {
    let spans: [RichSpan]

    var body: some View {
        Text(Self.attributed(spans))
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// `spans` as styled text: bold, italics, strikethrough and code from
    /// their flags, links from their URLs.
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
            out.append(part)
        }
        return out
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
