# Implementation Record: Message markup (milestone 10)

_Plan: docs/plans/message-markup.md_
_Started: 2026-11-06_

Run in continuous mode. The user asked for every chunk's manual check to be batched at the end rather than done per chunk, so each chunk's "Manual Validation" says "Deferred" until then.

Before chunk 1 the branch was fast-forwarded to local `main` (02460a0, one commit ahead of `origin/main`).

Pre-existing dirty files kept out of every commit: `.gitignore` (a duplicated Riptide entry) and the untracked `.shadow/`. `docs/plans/roadmap.md` (the link to this plan) and `docs/plans/message-markup.md` belong to this milestone and went in with chunk 1.

Baseline before any change: 611 tests passed, clippy clean, `src/ui/chat.rs` rustfmt drift 17.

## Chunk 1: Copy button on code blocks

### Changes Made
- `src/ui/markup.rs` (new): `code_block_actions(block, window, cx)`. It returns an `h_flex` with the block's language (muted, `text_xs`) when the fence names one, and a ghost `xsmall` `Button` with `IconName::Copy`. A click writes `block.code()` to the clipboard and sets a keyed `Entity<bool>` (`window.use_keyed_state(("code-copied", span.start))`, scoped by the `TextView`'s own element id). The icon becomes `IconName::Check` with a "Copied" tooltip, and a 2-second timer sets it back. The wrapper stops left mouse-down propagation, so a press doesn't start a text selection.
- `src/ui/mod.rs`: `pub mod markup;`
- `src/ui/chat.rs`: `.code_block_actions(code_block_actions)` on the message body, system message and embed description `TextView`s, and the import.
- `src/ui/profile_card.rs`: the same on the bio `TextView`.

### Divergences
- `CodeBlock` is imported from `gpui_kit::base::text`. gpui-component's `text` facade doesn't re-export it.
- **The button overlays the first code line rather than getting extra top padding.** With actions set, gpui-base gives the block only 14px of top padding, and its actions container has the code background, so a long first line runs under the badge. Extra padding would need an explicit `TextViewStyle` (`with_code_block`) at each call site. That takes `cx` in the row renderers, which don't have it, and it pins the style to the current theme. The plan's risk allows either. Left as an overlay for the manual check to judge.

### Verification Performed
- `cargo build`: clean.
- `cargo test`: 611 passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `rustfmt --edition 2024` on `markup.rs` and `profile_card.rs` (clean at HEAD). `chat.rs` drift is still 17.

### Manual Validation
Deferred to the end (see the plan's chunk 1 manual check).
