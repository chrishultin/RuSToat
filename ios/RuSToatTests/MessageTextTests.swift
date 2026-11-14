import Foundation
import Testing
@testable import RuSToat

/// The Markdown the core prepares, as message text renders it.
struct MessageTextTests {
    private func runs(_ markdown: String) -> [(String, AttributeContainer)] {
        let text = MessageText.attributed(markdown)
        return text.runs.map { (String(text[$0.range].characters), $0.attributes) }
    }

    @Test func boldAndItalics() {
        let runs = runs("plain **bold** *it*")
        let bold = runs.first { $0.0 == "bold" }
        #expect(bold?.1.inlinePresentationIntent == .stronglyEmphasized)
        let italic = runs.first { $0.0 == "it" }
        #expect(italic?.1.inlinePresentationIntent == .emphasized)
    }

    @Test func codeSpans() {
        let code = runs("run `cargo test` now").first { $0.0 == "cargo test" }
        #expect(code?.1.inlinePresentationIntent == .code)
    }

    @Test func links() {
        let link = runs("see [the docs](https://stoat.chat/docs)").first { $0.0 == "the docs" }
        #expect(link?.1.link == URL(string: "https://stoat.chat/docs"))
        let mention = runs("hi [@Ada](stoat:user/u-ada)").first { $0.0 == "@Ada" }
        #expect(mention?.1.link?.scheme == "stoat")
    }

    @Test func lineBreaksAreKept() {
        let text = MessageText.attributed("one\ntwo")
        #expect(String(text.characters) == "one\ntwo")
    }

    @Test func unparseableTextFallsBackToPlain() {
        let text = MessageText.attributed("**unclosed")
        #expect(String(text.characters).contains("unclosed"))
    }
}
