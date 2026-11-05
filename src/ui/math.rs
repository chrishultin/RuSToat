//! Draws Stoat's `$$…$$` formatting ([`crate::math`]) inside `TextView`.
//!
//! An inline `TextView` plugin. It claims the `$$…$$` math nodes that
//! [`math::interpret`] can read and draws their runs as one element. The
//! element doesn't wrap, which suits the short coloured or small labels this
//! is used for. Anything it doesn't claim, `TextView` shows as typed. That
//! includes `$…$`, which Stoat doesn't treat as math.

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
    markdown_ast,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::math::{self, Family, Run};

const NAME: &str = "stoat-math";

/// Stands in for KaTeX_Main, a Computer Modern serif.
const SERIF: &str = "Times New Roman";

/// KaTeX's line height (`.katex { line-height: 1.2 }`).
const LINE_HEIGHT: f32 = 1.2;

/// `\colorbox` padding either side (KaTeX's `\fboxsep`), in ems.
const BOX_PADDING: f32 = 0.3;

pub struct MathPlugin;

impl MarkdownPlugin for MathPlugin {
    fn name(&self) -> &str {
        NAME
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::InlineMath(formula) = node else {
            return None;
        };
        // `TextView` parses `$…$` as math too, but Stoat only treats `$$…$$`
        // as math ("$5 and $10" is prose).
        let source = cx.node_source(node)?;
        if !source.starts_with("$$") {
            return None;
        }
        let runs = math::interpret(&formula.value)?;
        let text = math::plain_text(&runs);
        Some(
            MarkdownNode::new(NAME, runs)
                .text(text)
                .markdown(source.to_string()),
        )
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let runs = node.data::<Vec<Run>>()?;
        Some(render_runs(runs, context, window, cx))
    }
}

struct Piece<'a> {
    run: &'a Run,
    font: Font,
    size: Pixels,
    line_height: Pixels,
    /// From the top of the piece's own line box.
    baseline: Pixels,
    shift: Pixels,
}

fn render_runs(
    runs: &[Run],
    context: &InlineRenderContext,
    window: &mut Window,
    cx: &App,
) -> InlineElement {
    let base = context.text_style();
    let pieces: Vec<Piece> = runs
        .iter()
        .map(|run| {
            let family: SharedString = match run.style.family {
                Family::Serif => SERIF.into(),
                Family::Sans => base.font_family.clone(),
                Family::Mono => cx.theme().mono_font_family.clone(),
            };
            let font = Font {
                family,
                features: base.font_features.clone(),
                fallbacks: base.font_fallbacks.clone(),
                weight: if run.style.bold {
                    FontWeight::BOLD
                } else {
                    base.font_weight
                },
                style: if run.style.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
            };
            let size = context.font_size() * run.style.scale;
            let line_height = size * LINE_HEIGHT;
            let text_system = window.text_system();
            let font_id = text_system.resolve_font(&font);
            let baseline = text_system.baseline_offset(font_id, size, line_height);
            Piece {
                run,
                font,
                size,
                line_height,
                baseline,
                shift: context.font_size() * run.style.shift,
            }
        })
        .collect();

    // Line every piece's baseline up (raised or lowered for scripts), and
    // report it so `TextView` lines the element up with the text around it.
    let baseline = pieces
        .iter()
        .map(|piece| piece.baseline + piece.shift)
        .fold(px(0.), |a, b| if b > a { b } else { a });
    let children = pieces.into_iter().map(|piece| {
        let style = &piece.run.style;
        let top = baseline - piece.shift - piece.baseline;
        div()
            .flex_none()
            .whitespace_nowrap()
            .pt(if top > px(0.) { top } else { px(0.) })
            .font(piece.font)
            .text_size(piece.size)
            .line_height(piece.line_height)
            .text_color(style.color.map_or(base.color, |color| rgba(color).into()))
            .when_some(style.background, |el, background| {
                el.bg(rgba(background)).px(piece.size * BOX_PADDING)
            })
            .when(style.underline, |el| el.underline())
            .when(style.strikethrough, |el| el.line_through())
            .child(piece.run.text.clone())
    });
    InlineElement::new(div().flex().items_start().children(children)).with_baseline(baseline)
}
