//! `$$…$$` formatting: the KaTeX subset read into styled runs.

use stoat_gpui::math::{Family, KATEX_SCALE, Run, interpret, parse_color, plain_text};

fn runs(tex: &str) -> Vec<Run> {
    interpret(tex).unwrap_or_else(|| panic!("{tex:?} should be supported"))
}

fn text(tex: &str) -> String {
    plain_text(&runs(tex))
}

fn texts(tex: &str) -> Vec<String> {
    runs(tex).into_iter().map(|run| run.text).collect()
}

const GRAY: u32 = 0x808080ff;
const THIN: char = '\u{2009}';
const MEDIUM: char = '\u{2005}';
const THICK: char = '\u{2004}';

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn small_grey_sans_caption() {
    // The review bot's step timings.
    let caption = runs(r"\color{gray}\scriptsize \textsf{17.7s}");
    assert_eq!(caption.len(), 1);
    let run = &caption[0];
    assert_eq!(run.text, "17.7s");
    assert_eq!(run.style.color, Some(GRAY));
    assert!(close(run.style.scale, KATEX_SCALE * 0.7));
    assert_eq!(run.style.family, Family::Sans);
    assert!(!run.style.italic && !run.style.bold);
}

#[test]
fn dot_separated_captions_keep_their_spacing() {
    // `\ ` is a space, and `\cdot` gets TeX's medium space either side.
    let tex = r"\color{gray}\scriptsize \textsf{5 matches}\ \cdot\ \textsf{0.1s}";
    assert_eq!(text(tex), format!("5 matches{MEDIUM} ⋅{MEDIUM} 0.1s"));
    assert!(runs(tex).iter().all(|run| run.style.color == Some(GRAY)));
}

#[test]
fn colours() {
    assert_eq!(parse_color("gray"), Some(GRAY));
    assert_eq!(parse_color("Red"), Some(0xff0000ff));
    assert_eq!(parse_color("#0f0"), Some(0x00ff00ff));
    assert_eq!(parse_color("#123456"), Some(0x123456ff));
    assert_eq!(parse_color("123456"), Some(0x123456ff)); // KaTeX allows no `#`
    assert_eq!(parse_color("notacolour"), None);
    assert_eq!(parse_color("rgb(1,2,3)"), None);
    assert_eq!(parse_color("#12"), None);

    let colored = runs(r"\textcolor{#f00}{red} \color{blue} blue");
    assert_eq!(colored[0].style.color, Some(0xff0000ff));
    assert_eq!(colored.last().unwrap().style.color, Some(0x0000ffff));
    // A switch only lasts to the end of its group.
    let grouped = runs(r"{\color{red} a} b");
    assert_eq!(grouped[0].style.color, Some(0xff0000ff));
    assert_eq!(grouped[1].style.color, None);
    // An unknown colour is an error in KaTeX: shown as typed instead.
    assert_eq!(interpret(r"\color{nope} x"), None);

    let boxed = runs(r"\colorbox{yellow}{note}");
    assert_eq!(boxed[0].style.background, Some(0xffff00ff));
    assert_eq!(boxed[0].text, "note");
}

#[test]
fn sizes_and_fonts() {
    assert!(close(runs(r"\Huge x")[0].style.scale, KATEX_SCALE * 2.488));
    assert!(close(runs(r"\tiny x")[0].style.scale, KATEX_SCALE * 0.5));
    assert!(close(runs("x")[0].style.scale, KATEX_SCALE));

    // Math letters are italic serif; text is upright.
    let letter = &runs("x")[0].style;
    assert!(letter.italic && letter.family == Family::Serif);
    assert!(!runs("2")[0].style.italic);
    assert!(!runs(r"\text{x}")[0].style.italic);
    assert!(!runs(r"\mathrm{x}")[0].style.italic);
    assert!(runs(r"\textbf{x}")[0].style.bold);
    assert!(runs(r"\textit{x}")[0].style.italic);
    assert_eq!(runs(r"\texttt{x}")[0].style.family, Family::Mono);
    assert_eq!(runs(r"\sf x")[0].style.family, Family::Sans);
    let bold = &runs(r"\mathbf{x}")[0].style;
    assert!(bold.bold && !bold.italic);
    assert!(runs(r"\underline{x}")[0].style.underline);
    assert!(runs(r"\sout{x}")[0].style.strikethrough);
}

#[test]
fn spaces_follow_tex() {
    // Spaces are ignored in math, kept (collapsed) in text.
    assert_eq!(text("a b"), "ab");
    assert_eq!(text(r"\text{a   b}"), "a b");
    assert_eq!(text(r"a\ b~c\quad d"), "a b\u{a0}c\u{2003}d");
    // Operators and relations are spaced; a leading minus isn't.
    assert_eq!(text("a+b"), format!("a{MEDIUM}+{MEDIUM}b"));
    assert_eq!(text("x=-1"), format!("x{THICK}={THICK}−1"));
    assert_eq!(text(r"\sin x"), format!("sin{THIN}x"));
    assert_eq!(text("f(x),y"), format!("f(x),{THIN}y"));
    // Text-mode dashes and quotes.
    assert_eq!(text(r"\text{a--b---c ``q''}"), "a–b—c “q”");
}

#[test]
fn symbols_and_scripts() {
    assert_eq!(text(r"\alpha\to\infty"), format!("α{THICK}→{THICK}∞"));
    assert!(runs(r"\alpha")[0].style.italic);
    assert!(!runs(r"\Omega")[0].style.italic);
    // `markdown::prepare` passes `<` through as `\<`.
    assert_eq!(text(r"a\<b"), format!("a{THICK}<{THICK}b"));
    assert_eq!(text(r"\{x\}"), "{x}");
    assert_eq!(text(r"\left( x \right.\big]"), "(x]");

    let squared = runs("x^2");
    assert_eq!(texts("x^2"), ["x", "2"]);
    assert!(squared[1].style.shift > 0. && squared[1].style.scale < squared[0].style.scale);
    let indexed = runs("x_{10}");
    assert_eq!(indexed[1].text, "10");
    assert!(indexed[1].style.shift < 0.);
    assert_eq!(text(r"\operatorname{avg} x"), format!("avg{THIN}x"));
}

#[test]
fn unsupported_formulas_are_left_as_typed() {
    for tex in [
        r"\frac{1}{2}",
        r"\sqrt{2}",
        r"a \\ b",
        r"\begin{matrix} a \end{matrix}",
        "a & b",
        r"\hat{x}",
        r"\textsf{open",
        "x}",
        "x^",
        r"\text{$x$}",
        r"\notacommand",
    ] {
        assert_eq!(interpret(tex), None, "{tex:?}");
    }
    // Deep nesting gives up rather than recursing forever.
    let deep = format!("{}x{}", "{".repeat(100), "}".repeat(100));
    assert_eq!(interpret(&deep), None);
}

#[test]
fn any_input_is_handled_without_panicking() {
    let tricky = r"\color{gray}\scriptsize ü\textsf{日本 🦦}\ \cdot^{é_\alpha}\text{--''}%c";
    let boundaries: Vec<usize> = tricky
        .char_indices()
        .map(|(i, _)| i)
        .chain([tricky.len()])
        .collect();
    for &start in &boundaries {
        for &end in boundaries.iter().filter(|&&e| e >= start) {
            let _ = interpret(&tricky[start..end]);
        }
    }
}
