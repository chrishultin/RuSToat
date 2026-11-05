//! Stoat's `$$…$$` formatting: a small part of KaTeX, read into styled runs.
//!
//! Stoat renders `$$…$$` with KaTeX (a single `$` isn't math there), and it's
//! mostly used for what Markdown can't do, colour and size:
//! `$$\color{gray}\scriptsize \textsf{17.7s}$$`. `TextView` has no math
//! renderer, so [`interpret`] reads that subset into [`Run`]s of plain text
//! with a style each, and `ui::math` draws them as one inline element:
//!
//! - colour: `\color`, `\textcolor`, `\colorbox`, `\fcolorbox` (CSS names, `#rgb`, `#rrggbb`)
//! - size: `\tiny` … `\Huge`, `\scriptstyle`, `\scriptscriptstyle`
//! - fonts: `\text`, `\textsf`, `\texttt`, `\textbf`, `\textit`, `\emph`,
//!   `\mathrm`, `\mathbf`, `\mathsf`, `\mathtt`, `\boldsymbol`, `\rm`, `\bf`, `\sf`, …
//! - spacing: `\ `, `\,`, `\;`, `\quad`, `~`, …, and TeX's own space around
//!   operators (`a+b`, `x=1`, `\sin x`)
//! - `^` and `_`, `\underline`, `\sout`/`\cancel` (struck through),
//!   `\left`/`\right`/`\big…` (just the delimiter), `\operatorname`
//! - Greek letters, operators, relations, arrows and other common symbols
//!
//! Anything else (fractions, roots, matrices, accents, line breaks, unknown
//! commands) makes [`interpret`] return `None`, and the formula shows as typed.
//! KaTeX's layout is approximated: math letters are italic and spaces are
//! ignored as in TeX, but scripts are only raised or lowered, not stacked.

use std::str::FromStr;

/// KaTeX draws formulas at 1.21× the surrounding text (`.katex { font-size: 1.21em }`).
pub const KATEX_SCALE: f32 = 1.21;

/// Deepest `{…}` nesting followed before giving up.
const MAX_DEPTH: usize = 32;

/// TeX's thin, medium and thick math spaces (3/18, 4/18 and 5/18 em), as the
/// nearest Unicode spaces.
const THIN: &str = "\u{2009}";
const MEDIUM: &str = "\u{2005}";
const THICK: &str = "\u{2004}";
const QUAD: &str = "\u{2003}";
const NBSP: &str = "\u{a0}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// TeX's default (KaTeX_Main is a Computer Modern serif).
    Serif,
    /// `\textsf`: the UI font.
    Sans,
    /// `\texttt`: the code font.
    Mono,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// `0xRRGGBBAA`. `None` keeps the surrounding text's colour.
    pub color: Option<u32>,
    /// `0xRRGGBBAA`, from `\colorbox`.
    pub background: Option<u32>,
    /// Font size relative to the surrounding text.
    pub scale: f32,
    /// Baseline shift in ems of the surrounding text, up is positive.
    pub shift: f32,
    pub family: Family,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// Text drawn in one style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

/// Read a formula (the text between `$$` and `$$`) into runs, or `None` if
/// it uses anything outside the supported subset.
pub fn interpret(tex: &str) -> Option<Vec<Run>> {
    let mut parser = Parser {
        rest: tex,
        depth: 0,
    };
    let mut items = Vec::new();
    parser.group(&State::default(), false, &mut items)?;
    Some(layout(&items, false))
}

/// The runs' text, for copying and screen readers.
pub fn plain_text(runs: &[Run]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

/// TeX's atom classes, which decide the space between neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
}

#[derive(Debug, Clone)]
enum Item {
    /// Something TeX spaces against its neighbours, with any scripts attached.
    Atom { class: Class, runs: Vec<Run> },
    /// Explicit space (`\ `, `\quad`, a space in text), which TeX doesn't count.
    Glue(Run),
}

/// What applies at a point in the formula. Switches like `\color` change it
/// for the rest of the enclosing `{…}`.
#[derive(Debug, Clone)]
struct State {
    text_mode: bool,
    color: Option<u32>,
    background: Option<u32>,
    /// `\tiny` (0.5) … `\Huge` (2.488).
    size: f32,
    /// 0 normally, 1 in a script, 2 in a script's script.
    script: u8,
    shift: f32,
    family: Family,
    bold: bool,
    /// `None`: letters are italic in math, everything else upright.
    italic: Option<bool>,
    underline: bool,
    strikethrough: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            text_mode: false,
            color: None,
            background: None,
            size: 1.,
            script: 0,
            shift: 0.,
            family: Family::Serif,
            bold: false,
            italic: None,
            underline: false,
            strikethrough: false,
        }
    }
}

impl State {
    fn scale(&self) -> f32 {
        KATEX_SCALE * self.size * [1., 0.7, 0.5][usize::from(self.script.min(2))]
    }

    fn style(&self, letter: bool) -> Style {
        let math_letter = letter && !self.text_mode;
        Style {
            color: self.color,
            background: self.background,
            scale: self.scale(),
            shift: self.shift,
            family: self.family,
            bold: self.bold,
            italic: self.italic.unwrap_or(math_letter),
            underline: self.underline,
            strikethrough: self.strikethrough,
        }
    }

    /// Switch to text mode, where letters are upright unless asked otherwise.
    fn text(mut self) -> Self {
        self.text_mode = true;
        self.italic = Some(self.italic.unwrap_or(false));
        self
    }

    fn upright(mut self, family: Family) -> Self {
        self.family = family;
        self.italic = Some(false);
        self
    }

    fn italic(mut self, italic: bool) -> Self {
        self.italic = Some(italic);
        self
    }
}

struct Parser<'a> {
    rest: &'a str,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.rest = &self.rest[c.len_utf8()..];
        Some(c)
    }

    fn skip_whitespace(&mut self) {
        self.rest = self.rest.trim_start();
    }

    /// Items up to the matching `}` (consumed) when `closed`, else to the end.
    /// Switches inside stay inside.
    fn group(&mut self, state: &State, closed: bool, items: &mut Vec<Item>) -> Option<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let mut state = state.clone();
        loop {
            if state.text_mode {
                if self.peek().is_some_and(char::is_whitespace) {
                    self.skip_whitespace();
                    items.push(Item::Glue(run(" ", state.style(false))));
                    continue;
                }
            } else {
                self.skip_whitespace();
            }
            let Some(c) = self.peek() else {
                // A missing `}`.
                return (!closed).then_some(());
            };
            match c {
                '}' if closed => {
                    self.bump();
                    break;
                }
                '}' => return None,
                '{' => {
                    self.bump();
                    self.group(&state, true, items)?;
                }
                '\\' => self.command(&mut state, items)?,
                '^' | '_' if !state.text_mode => self.script(c, &state, items)?,
                '%' => {
                    // A comment, to the end of the line.
                    let end = self.rest.find('\n').unwrap_or(self.rest.len());
                    self.rest = &self.rest[end..];
                }
                // Alignment, macro parameters, nested math: not supported.
                '&' | '#' | '$' => return None,
                '~' => {
                    self.bump();
                    items.push(Item::Glue(run(NBSP, state.style(false))));
                }
                _ => {
                    self.bump();
                    self.character(c, &state, items);
                }
            }
        }
        self.depth -= 1;
        Some(())
    }

    /// A command's argument: a `{…}` group, or a single character or command.
    fn argument(&mut self, state: &State, items: &mut Vec<Item>) -> Option<()> {
        self.skip_whitespace();
        match self.peek()? {
            '{' => {
                self.bump();
                self.group(state, true, items)
            }
            '}' => None,
            '\\' => self.command(&mut state.clone(), items),
            c => {
                self.bump();
                self.character(c, state, items);
                Some(())
            }
        }
    }

    /// The text of a `{…}` argument, unparsed (a colour name).
    fn raw_argument(&mut self) -> Option<&'a str> {
        self.skip_whitespace();
        let inner = self.rest.strip_prefix('{')?;
        let end = inner.find('}')?;
        self.rest = &inner[end + 1..];
        Some(&inner[..end])
    }

    /// The name after a `\`: a run of letters (spaces after it are skipped,
    /// as in TeX) or a single other character.
    fn control_sequence(&mut self) -> Option<&'a str> {
        let after = self.rest.strip_prefix('\\')?;
        let letters = after.bytes().take_while(u8::is_ascii_alphabetic).count();
        let len = if letters > 0 {
            letters
        } else {
            after.chars().next()?.len_utf8()
        };
        let name = &after[..len];
        self.rest = &after[len..];
        if letters > 0 {
            self.skip_whitespace();
        }
        Some(name)
    }

    fn character(&mut self, c: char, state: &State, items: &mut Vec<Item>) {
        if state.text_mode {
            let text = match c {
                '-' if self.rest.starts_with("--") => {
                    self.rest = &self.rest[2..];
                    "—".to_string()
                }
                '-' if self.rest.starts_with('-') => {
                    self.rest = &self.rest[1..];
                    "–".to_string()
                }
                '`' if self.rest.starts_with('`') => {
                    self.rest = &self.rest[1..];
                    "“".to_string()
                }
                '\'' if self.rest.starts_with('\'') => {
                    self.rest = &self.rest[1..];
                    "”".to_string()
                }
                '`' => "‘".to_string(),
                '\'' => "’".to_string(),
                c => c.to_string(),
            };
            items.push(atom(&text, Class::Ord, state.style(true)));
            return;
        }
        let (text, class) = match c {
            '+' => ("+".to_string(), Class::Bin),
            '-' => ("−".to_string(), Class::Bin),
            '*' => ("∗".to_string(), Class::Bin),
            '\'' => ("′".to_string(), Class::Ord),
            '=' | '<' | '>' | ':' => (c.to_string(), Class::Rel),
            ',' | ';' => (c.to_string(), Class::Punct),
            '(' | '[' => (c.to_string(), Class::Open),
            ')' | ']' | '!' | '?' => (c.to_string(), Class::Close),
            _ => (c.to_string(), Class::Ord),
        };
        // Only Latin letters are italic: KaTeX draws other scripts upright.
        items.push(atom(&text, class, state.style(c.is_ascii_alphabetic())));
    }

    fn script(&mut self, kind: char, state: &State, items: &mut Vec<Item>) -> Option<()> {
        self.bump();
        let mut inner_state = state.clone();
        let rise = if kind == '^' { 0.41 } else { -0.2 };
        inner_state.shift += rise * state.scale();
        inner_state.script = (state.script + 1).min(2);
        let mut inner = Vec::new();
        self.argument(&inner_state, &mut inner)?;
        let runs = layout(&inner, true);
        match items.last_mut() {
            Some(Item::Atom { runs: base, .. }) => base.extend(runs),
            _ => items.push(Item::Atom {
                class: Class::Ord,
                runs,
            }),
        }
        Some(())
    }

    fn command(&mut self, state: &mut State, items: &mut Vec<Item>) -> Option<()> {
        let name = self.control_sequence()?;

        // Spaces.
        let space = match name {
            " " | "space" => Some(" "),
            "," | "thinspace" => Some(THIN),
            ":" | ">" | "medspace" => Some(MEDIUM),
            ";" | "thickspace" => Some(THICK),
            "enspace" => Some("\u{2002}"),
            "quad" => Some(QUAD),
            "qquad" => Some("\u{2003}\u{2003}"),
            "nobreakspace" => Some(NBSP),
            // Negative space can't be drawn; leave it out.
            "!" | "negthinspace" | "negmedspace" | "negthickspace" => Some(""),
            _ => None,
        };
        if let Some(space) = space {
            if !space.is_empty() {
                items.push(Item::Glue(run(space, state.style(false))));
            }
            return Some(());
        }

        // Switches, for the rest of the group.
        if let Some(size) = size_factor(name) {
            state.size = size;
            return Some(());
        }
        match name {
            "displaystyle" | "textstyle" => state.script = 0,
            "scriptstyle" => state.script = 1,
            "scriptscriptstyle" => state.script = 2,
            "color" => state.color = Some(parse_color(self.raw_argument()?)?),
            "rm" | "rmfamily" => *state = state.clone().upright(Family::Serif),
            "sf" | "sffamily" => *state = state.clone().upright(Family::Sans),
            "tt" | "ttfamily" => *state = state.clone().upright(Family::Mono),
            "bf" => {
                *state = state.clone().upright(state.family);
                state.bold = true;
            }
            "bfseries" => state.bold = true,
            "mdseries" => state.bold = false,
            "it" | "itshape" => state.italic = Some(true),
            "upshape" => state.italic = Some(false),
            "normalfont" => {
                *state = state.clone().upright(Family::Serif);
                state.bold = false;
            }
            _ => return self.command_with_argument(name, state, items),
        }
        Some(())
    }

    fn command_with_argument(
        &mut self,
        name: &str,
        state: &State,
        items: &mut Vec<Item>,
    ) -> Option<()> {
        let mut inner = state.clone();
        match name {
            "textcolor" => inner.color = Some(parse_color(self.raw_argument()?)?),
            "colorbox" => {
                inner.background = Some(parse_color(self.raw_argument()?)?);
                inner = inner.text();
            }
            "fcolorbox" => {
                parse_color(self.raw_argument()?)?; // the frame isn't drawn
                inner.background = Some(parse_color(self.raw_argument()?)?);
                inner = inner.text();
            }
            "text" | "mbox" | "hbox" => inner = inner.text(),
            "textrm" => inner = inner.text().upright(Family::Serif),
            "textnormal" => {
                inner = inner.text().upright(Family::Serif);
                inner.bold = false;
            }
            "textsf" => {
                inner = inner.text();
                inner.family = Family::Sans;
            }
            "texttt" => {
                inner = inner.text();
                inner.family = Family::Mono;
            }
            "textbf" => {
                inner = inner.text();
                inner.bold = true;
            }
            "textmd" => {
                inner = inner.text();
                inner.bold = false;
            }
            "textit" => inner = inner.text().italic(true),
            "textup" => inner = inner.text().italic(false),
            "emph" => {
                inner = inner.text();
                let italic = inner.italic != Some(true);
                inner = inner.italic(italic);
            }
            "mathrm" => inner = inner.upright(Family::Serif),
            "mathsf" => inner = inner.upright(Family::Sans),
            "mathtt" => inner = inner.upright(Family::Mono),
            "mathit" => inner = inner.italic(true),
            "mathbf" => {
                inner = inner.upright(Family::Serif);
                inner.bold = true;
            }
            "boldsymbol" | "bm" => inner.bold = true,
            "mathnormal" => {
                inner.family = Family::Serif;
                inner.italic = None;
            }
            "underline" => inner.underline = true,
            "sout" | "cancel" | "bcancel" | "xcancel" => inner.strikethrough = true,
            "operatorname" => {
                let mut name_items = Vec::new();
                self.argument(&inner.upright(Family::Serif), &mut name_items)?;
                items.push(Item::Atom {
                    class: Class::Op,
                    runs: layout(&name_items, true),
                });
                return Some(());
            }
            "left" | "right" | "middle" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl"
            | "biggl" | "Biggl" | "bigr" | "Bigr" | "biggr" | "Biggr" | "bigm" | "Bigm"
            | "biggm" | "Biggm" => {
                // Just the delimiter that follows, at its normal size. `.` is "none".
                self.skip_whitespace();
                if let Some(rest) = self.rest.strip_prefix('.') {
                    self.rest = rest;
                }
                return Some(());
            }
            _ => return self.symbol(name, state, items),
        }
        self.argument(&inner, items)
    }

    fn symbol(&mut self, name: &str, state: &State, items: &mut Vec<Item>) -> Option<()> {
        if let Some(op) = OPERATOR_NAMES.iter().find(|&&op| op == name) {
            let style = state.clone().upright(Family::Serif).style(false);
            items.push(atom(op, Class::Op, style));
            return Some(());
        }
        let &(_, text, class) = SYMBOLS.iter().find(|(symbol, ..)| *symbol == name)?;
        // Lowercase Greek is italic in math, like Latin letters.
        let letter = text
            .chars()
            .all(|c| ('α'..='ω').contains(&c) || "ϵϑϖϱϕ".contains(c));
        let class = if state.text_mode { Class::Ord } else { class };
        items.push(atom(text, class, state.style(letter)));
        Some(())
    }
}

fn run(text: &str, style: Style) -> Run {
    Run {
        text: text.to_string(),
        style,
    }
}

fn atom(text: &str, class: Class, style: Style) -> Item {
    Item::Atom {
        class,
        runs: vec![run(text, style)],
    }
}

/// Runs for a list of items, with TeX's spaces between atoms and neighbouring
/// runs of the same style merged.
fn layout(items: &[Item], script: bool) -> Vec<Run> {
    let mut classes: Vec<Class> = items
        .iter()
        .filter_map(|item| match item {
            Item::Atom { class, .. } => Some(*class),
            Item::Glue(_) => None,
        })
        .collect();
    // A binary operator with nothing to combine on one side is ordinary:
    // `-1`, `(+)`, `x+`.
    for i in 0..classes.len() {
        let after_operand = i > 0
            && !matches!(
                classes[i - 1],
                Class::Bin | Class::Op | Class::Rel | Class::Open | Class::Punct
            );
        if classes[i] == Class::Bin && !after_operand {
            classes[i] = Class::Ord;
        }
    }
    for i in 0..classes.len() {
        let before_operand = classes
            .get(i + 1)
            .is_some_and(|next| !matches!(next, Class::Rel | Class::Close | Class::Punct));
        if classes[i] == Class::Bin && !before_operand {
            classes[i] = Class::Ord;
        }
    }

    let mut out: Vec<Run> = Vec::new();
    let mut push = |text: &str, style: &Style| {
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(text),
            _ => out.push(run(text, style.clone())),
        }
    };
    let mut index = 0;
    for item in items {
        match item {
            Item::Glue(glue) => push(&glue.text, &glue.style),
            Item::Atom { runs, .. } => {
                for r in runs {
                    push(&r.text, &r.style);
                }
                if let (Some(next), Some(first)) = (classes.get(index + 1), runs.first())
                    && let Some(space) = spacing(classes[index], *next, script)
                {
                    push(space, &first.style);
                }
                index += 1;
            }
        }
    }
    out
}

/// The space TeX puts between two atoms. In scripts only the thin space
/// around operators like `\sin` is kept.
fn spacing(left: Class, right: Class, script: bool) -> Option<&'static str> {
    use Class::*;
    let (space, in_scripts) = match (left, right) {
        (Ord | Op | Close, Op) | (Op, Ord) => (THIN, true),
        (Ord | Close, Bin) | (Bin, Ord | Op | Open) => (MEDIUM, false),
        (Ord | Op | Close, Rel) | (Rel, Ord | Op | Open) => (THICK, false),
        (Punct, Ord | Op | Rel | Open | Close | Punct) => (THIN, false),
        _ => return None,
    };
    (in_scripts || !script).then_some(space)
}

/// KaTeX's size commands, relative to `\normalsize`.
fn size_factor(name: &str) -> Option<f32> {
    Some(match name {
        "tiny" => 0.5,
        "scriptsize" => 0.7,
        "footnotesize" => 0.8,
        "small" => 0.9,
        "normalsize" => 1.,
        "large" => 1.2,
        "Large" => 1.44,
        "LARGE" => 1.728,
        "huge" => 2.074,
        "Huge" => 2.488,
        _ => return None,
    })
}

/// A KaTeX colour: `#rgb`, `#rrggbb`, `rrggbb` or a CSS name, as `0xRRGGBBAA`.
pub fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim();
    let hex = |s: &str| s.bytes().all(|b| b.is_ascii_hexdigit());
    let css = match value.strip_prefix('#') {
        Some(digits) if matches!(digits.len(), 3 | 6) && hex(digits) => value.to_string(),
        Some(_) => return None,
        None if value.len() == 6 && hex(value) => format!("#{value}"),
        None if !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphabetic()) => {
            value.to_ascii_lowercase()
        }
        None => return None,
    };
    let color = svgtypes::Color::from_str(&css).ok()?;
    Some(u32::from_be_bytes([
        color.red,
        color.green,
        color.blue,
        color.alpha,
    ]))
}

/// `\sin`-style operator names, drawn upright.
const OPERATOR_NAMES: &[&str] = &[
    "arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "det", "dim",
    "exp", "gcd", "hom", "inf", "ker", "lg", "lim", "liminf", "limsup", "ln", "log", "max", "min",
    "Pr", "sec", "sin", "sinh", "sup", "tan", "tanh",
];

/// Commands that stand for a character, with its class in math.
const SYMBOLS: &[(&str, &str, Class)] = {
    use Class::*;
    &[
        // Escaped characters (`\<` is how `markdown::prepare` passes `<` through).
        ("{", "{", Open),
        ("}", "}", Close),
        ("$", "$", Ord),
        ("%", "%", Ord),
        ("&", "&", Ord),
        ("#", "#", Ord),
        ("_", "_", Ord),
        ("|", "‖", Ord),
        ("<", "<", Rel),
        // Greek.
        ("alpha", "α", Ord),
        ("beta", "β", Ord),
        ("gamma", "γ", Ord),
        ("delta", "δ", Ord),
        ("epsilon", "ϵ", Ord),
        ("varepsilon", "ε", Ord),
        ("zeta", "ζ", Ord),
        ("eta", "η", Ord),
        ("theta", "θ", Ord),
        ("vartheta", "ϑ", Ord),
        ("iota", "ι", Ord),
        ("kappa", "κ", Ord),
        ("lambda", "λ", Ord),
        ("mu", "μ", Ord),
        ("nu", "ν", Ord),
        ("xi", "ξ", Ord),
        ("omicron", "ο", Ord),
        ("pi", "π", Ord),
        ("varpi", "ϖ", Ord),
        ("rho", "ρ", Ord),
        ("varrho", "ϱ", Ord),
        ("sigma", "σ", Ord),
        ("varsigma", "ς", Ord),
        ("tau", "τ", Ord),
        ("upsilon", "υ", Ord),
        ("phi", "ϕ", Ord),
        ("varphi", "φ", Ord),
        ("chi", "χ", Ord),
        ("psi", "ψ", Ord),
        ("omega", "ω", Ord),
        ("Gamma", "Γ", Ord),
        ("Delta", "Δ", Ord),
        ("Theta", "Θ", Ord),
        ("Lambda", "Λ", Ord),
        ("Xi", "Ξ", Ord),
        ("Pi", "Π", Ord),
        ("Sigma", "Σ", Ord),
        ("Upsilon", "Υ", Ord),
        ("Phi", "Φ", Ord),
        ("Psi", "Ψ", Ord),
        ("Omega", "Ω", Ord),
        // Binary operators.
        ("pm", "±", Bin),
        ("mp", "∓", Bin),
        ("times", "×", Bin),
        ("div", "÷", Bin),
        ("cdot", "⋅", Bin),
        ("ast", "∗", Bin),
        ("star", "⋆", Bin),
        ("circ", "∘", Bin),
        ("bullet", "∙", Bin),
        ("oplus", "⊕", Bin),
        ("ominus", "⊖", Bin),
        ("otimes", "⊗", Bin),
        ("odot", "⊙", Bin),
        ("cup", "∪", Bin),
        ("cap", "∩", Bin),
        ("setminus", "∖", Bin),
        ("wedge", "∧", Bin),
        ("land", "∧", Bin),
        ("vee", "∨", Bin),
        ("lor", "∨", Bin),
        ("dagger", "†", Bin),
        ("ddagger", "‡", Bin),
        // Relations and arrows.
        ("le", "≤", Rel),
        ("leq", "≤", Rel),
        ("ge", "≥", Rel),
        ("geq", "≥", Rel),
        ("ne", "≠", Rel),
        ("neq", "≠", Rel),
        ("approx", "≈", Rel),
        ("equiv", "≡", Rel),
        ("sim", "∼", Rel),
        ("simeq", "≃", Rel),
        ("cong", "≅", Rel),
        ("propto", "∝", Rel),
        ("ll", "≪", Rel),
        ("gg", "≫", Rel),
        ("in", "∈", Rel),
        ("notin", "∉", Rel),
        ("ni", "∋", Rel),
        ("subset", "⊂", Rel),
        ("supset", "⊃", Rel),
        ("subseteq", "⊆", Rel),
        ("supseteq", "⊇", Rel),
        ("perp", "⊥", Rel),
        ("parallel", "∥", Rel),
        ("mid", "∣", Rel),
        ("models", "⊨", Rel),
        ("vdash", "⊢", Rel),
        ("to", "→", Rel),
        ("rightarrow", "→", Rel),
        ("gets", "←", Rel),
        ("leftarrow", "←", Rel),
        ("leftrightarrow", "↔", Rel),
        ("Rightarrow", "⇒", Rel),
        ("Leftarrow", "⇐", Rel),
        ("Leftrightarrow", "⇔", Rel),
        ("longrightarrow", "⟶", Rel),
        ("longleftarrow", "⟵", Rel),
        ("implies", "⟹", Rel),
        ("impliedby", "⟸", Rel),
        ("iff", "⟺", Rel),
        ("mapsto", "↦", Rel),
        ("uparrow", "↑", Rel),
        ("downarrow", "↓", Rel),
        ("updownarrow", "↕", Rel),
        ("Uparrow", "⇑", Rel),
        ("Downarrow", "⇓", Rel),
        ("nearrow", "↗", Rel),
        ("searrow", "↘", Rel),
        ("swarrow", "↙", Rel),
        ("nwarrow", "↖", Rel),
        ("hookrightarrow", "↪", Rel),
        ("hookleftarrow", "↩", Rel),
        ("rightleftharpoons", "⇌", Rel),
        // Big operators.
        ("sum", "∑", Op),
        ("prod", "∏", Op),
        ("coprod", "∐", Op),
        ("int", "∫", Op),
        ("iint", "∬", Op),
        ("iiint", "∭", Op),
        ("oint", "∮", Op),
        ("bigcup", "⋃", Op),
        ("bigcap", "⋂", Op),
        ("bigvee", "⋁", Op),
        ("bigwedge", "⋀", Op),
        ("bigoplus", "⨁", Op),
        ("bigotimes", "⨂", Op),
        // Delimiters.
        ("langle", "⟨", Open),
        ("rangle", "⟩", Close),
        ("lfloor", "⌊", Open),
        ("rfloor", "⌋", Close),
        ("lceil", "⌈", Open),
        ("rceil", "⌉", Close),
        ("lbrace", "{", Open),
        ("rbrace", "}", Close),
        ("lbrack", "[", Open),
        ("rbrack", "]", Close),
        ("vert", "|", Ord),
        ("Vert", "‖", Ord),
        ("lvert", "|", Open),
        ("rvert", "|", Close),
        ("lVert", "‖", Open),
        ("rVert", "‖", Close),
        // Everything else.
        ("infty", "∞", Ord),
        ("partial", "∂", Ord),
        ("nabla", "∇", Ord),
        ("forall", "∀", Ord),
        ("exists", "∃", Ord),
        ("nexists", "∄", Ord),
        ("neg", "¬", Ord),
        ("lnot", "¬", Ord),
        ("emptyset", "∅", Ord),
        ("varnothing", "∅", Ord),
        ("hbar", "ℏ", Ord),
        ("ell", "ℓ", Ord),
        ("Re", "ℜ", Ord),
        ("Im", "ℑ", Ord),
        ("aleph", "ℵ", Ord),
        ("wp", "℘", Ord),
        ("angle", "∠", Ord),
        ("triangle", "△", Ord),
        ("square", "□", Ord),
        ("Box", "□", Ord),
        ("blacksquare", "■", Ord),
        ("lozenge", "◊", Ord),
        ("Diamond", "◊", Ord),
        ("blacklozenge", "⧫", Ord),
        ("checkmark", "✓", Ord),
        ("heartsuit", "♡", Ord),
        ("diamondsuit", "♢", Ord),
        ("clubsuit", "♣", Ord),
        ("spadesuit", "♠", Ord),
        ("flat", "♭", Ord),
        ("natural", "♮", Ord),
        ("sharp", "♯", Ord),
        ("prime", "′", Ord),
        ("degree", "°", Ord),
        ("top", "⊤", Ord),
        ("bot", "⊥", Ord),
        ("surd", "√", Ord),
        ("backslash", "\\", Ord),
        ("dots", "…", Ord),
        ("ldots", "…", Ord),
        ("cdots", "⋯", Ord),
        ("vdots", "⋮", Ord),
        ("ddots", "⋱", Ord),
        ("dag", "†", Ord),
        ("ddag", "‡", Ord),
        ("S", "§", Ord),
        ("P", "¶", Ord),
        ("copyright", "©", Ord),
        ("circledR", "®", Ord),
        ("pounds", "£", Ord),
        ("yen", "¥", Ord),
        ("euro", "€", Ord),
        ("textbackslash", "\\", Ord),
        ("textasciitilde", "~", Ord),
        ("textasciicircum", "^", Ord),
        ("textbullet", "•", Ord),
        ("textendash", "–", Ord),
        ("textemdash", "—", Ord),
        ("textdegree", "°", Ord),
        ("textless", "<", Ord),
        ("textgreater", ">", Ord),
        ("textbar", "|", Ord),
    ]
};
