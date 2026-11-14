import SwiftUI

/// A message's text: the Markdown the core prepares, as SwiftUI renders it.
/// Bold, italics, strikethrough, code spans and links work; block quotes,
/// code blocks, maths and spoilers show as plain text for now.
struct MessageText: View {
    let markdown: String
    var jumbo = false

    var body: some View {
        Text(Self.attributed(markdown))
            .font(jumbo ? .system(size: 40) : .body)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    /// `markdown` parsed inline, keeping its line breaks. Falls back to the
    /// plain text if it doesn't parse.
    nonisolated static func attributed(_ markdown: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .inlineOnlyPreservingWhitespace,
            failurePolicy: .returnPartiallyParsedIfPossible
        )
        return (try? AttributedString(markdown: markdown, options: options)) ?? AttributedString(markdown)
    }
}
