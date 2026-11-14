import Foundation
import SwiftUI
import Testing
@testable import RuSToat

/// Message text through the core's parser (`richText`) and into the
/// `AttributedString`s paragraphs draw.
struct RichTextTests {
    private func spans(_ markdown: String) -> [RichSpan] {
        guard case .paragraph(let spans)? = richText(markdown: markdown).first else {
            Issue.record("no paragraph for \(markdown)")
            return []
        }
        return spans
    }

    private func runs(_ markdown: String) -> [(String, AttributeContainer)] {
        let text = RichSpansText.attributed(spans(markdown))
        return text.runs.map { (String(text[$0.range].characters), $0.attributes) }
    }

    @Test func boldItalicsAndStrikethrough() {
        let runs = runs("plain **bold** *it* ~~gone~~")
        #expect(runs.first { $0.0 == "bold" }?.1.inlinePresentationIntent == .stronglyEmphasized)
        #expect(runs.first { $0.0 == "it" }?.1.inlinePresentationIntent == .emphasized)
        #expect(runs.first { $0.0 == "gone" }?.1.inlinePresentationIntent == .strikethrough)
    }

    @Test func combinedStyles() {
        let both = runs("***both***").first { $0.0 == "both" }
        #expect(both?.1.inlinePresentationIntent == [.stronglyEmphasized, .emphasized])
    }

    @Test func codeSpans() {
        #expect(runs("run `cargo test` now").first { $0.0 == "cargo test" }?.1.inlinePresentationIntent == .code)
    }

    @Test func links() {
        let link = runs("see [the docs](https://stoat.chat/docs)").first { $0.0 == "the docs" }
        #expect(link?.1.link == URL(string: "https://stoat.chat/docs"))
        let mention = runs("hi [@Ada](stoat:user/u-ada)").first { $0.0 == "@Ada" }
        #expect(mention?.1.link?.scheme == "stoat")
    }

    @Test func lineBreaksAreKept() {
        let text = RichSpansText.attributed(spans("one  \ntwo"))
        #expect(String(text.characters) == "one\ntwo")
    }

    @Test func blocksComeThroughTheBridge() {
        let blocks = richText(markdown: "# Title\n\n> quoted\n\n```swift\nlet x = 1\n```\n\n- a\n- b")
        #expect(blocks.count == 4)
        if case .heading(let level, _) = blocks[0] { #expect(level == 1) } else { Issue.record("heading") }
        if case .quote = blocks[1] {} else { Issue.record("quote") }
        if case .code(let language, let code) = blocks[2] {
            #expect(language == "swift")
            #expect(code == "let x = 1")
        } else { Issue.record("code") }
        if case .list(let ordered, _, let items) = blocks[3] {
            #expect(!ordered)
            #expect(items.count == 2)
        } else { Issue.record("list") }
    }

    @Test func listMarkers() {
        #expect(RichBlockView.marker(ordered: true, number: 3, checked: nil) == "3.")
        #expect(RichBlockView.marker(ordered: false, number: 1, checked: nil) == "•")
        #expect(RichBlockView.marker(ordered: false, number: 1, checked: true) == "☑")
    }
}

/// Spoiler and guarded-link taps.
struct InternalLinkTapTests {
    @Test func spoilerWords() {
        #expect(InternalLinkTap(URL(string: "stoat:spoiler/2/0/3")!) == .spoiler(index: 2))
    }

    @Test func guardedLinks() {
        let url = URL(string: "stoat:link?url=https%3A%2F%2Fevil.example%2Fx&text=click+me")!
        #expect(InternalLinkTap(url) == .guarded(url: URL(string: "https://evil.example/x")!, text: "click me"))
    }

    @Test func mentionsDoNothingYet() {
        #expect(InternalLinkTap(URL(string: "stoat:user/u-ada")!) == .ignore)
    }

    @Test func roleColoursAreParsed() {
        #expect(RichSpansText.mentionTint(.role, "#ff0000") == Color(red: 1, green: 0, blue: 0))
        #expect(RichSpansText.mentionTint(.user, nil) == .accentColor)
    }
}

/// Reaction chips' "who reacted" text.
struct ReactionsTests {
    private func reaction(count: UInt32, mine: Bool, names: [String]) -> ReactionItem {
        ReactionItem(key: "👍", emoji: .unicode(text: "👍"), count: count, mine: mine, names: names)
    }

    @Test func listsYouFirstThenNamesThenOthers() {
        #expect(ReactionsView.who(reaction(count: 1, mine: true, names: [])) == "You")
        #expect(ReactionsView.who(reaction(count: 2, mine: true, names: ["Ada"])) == "You and Ada")
        #expect(ReactionsView.who(reaction(count: 5, mine: false, names: ["Ada", "Bob"])) == "Ada, Bob and 3 others")
        #expect(ReactionsView.who(reaction(count: 2, mine: false, names: ["Ada"])) == "Ada and 1 other")
    }
}
