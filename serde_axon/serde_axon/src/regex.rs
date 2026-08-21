//! A small, dependency-free, linear-time regular-expression engine for the
//! AXON Schema `pattern` keyword.
//!
//! It exists to reproduce the reference validator's `_safe_regex_search`: a
//! pattern is first checked against a deliberately linear subset (the same one
//! the reference enforces via Python's `sre_parse`), and only then matched with
//! `re.search` semantics. Because the accepted subset forbids nested and
//! multiple variable-length quantifiers, matching is linear and needs no
//! backtracking guard.
//!
//! The engine mirrors the reference's accept/reject decisions, Unicode-aware
//! `re.search` results, and diagnostics for the governed pattern subset.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Why a pattern was rejected (never matched). Messages match the reference
/// validator for the governed pattern subset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexError {
    /// Human-readable reason, surfaced in the validator's `pattern` issue.
    pub message: String,
}

impl RegexError {
    fn new(message: impl Into<String>) -> Self {
        RegexError {
            message: message.into(),
        }
    }
}

/// Compile `pattern`, enforce the linear-subset safety rules, and return whether
/// it matches anywhere in `text` (`re.search` semantics). Mirrors the
/// reference `_safe_regex_search`.
pub fn safe_regex_search(pattern: &str, text: &str) -> Result<bool, RegexError> {
    if pattern.chars().count() > 1024 {
        return Err(RegexError::new("pattern exceeds 1024 characters"));
    }
    let ast = Parser::new(pattern).parse()?;
    check_safe(&ast)?;
    Ok(search(&ast, text))
}

// ---- AST ------------------------------------------------------------------ //

/// A parsed regular-expression node. Capturing vs non-capturing groups are not
/// distinguished because only match existence (a boolean) is needed.
#[derive(Clone, Debug, PartialEq)]
enum Ast {
    /// The empty pattern (matches the empty string).
    Empty,
    /// A single literal character.
    Literal(char),
    /// `.` -- any character except newline.
    Any,
    /// `[...]` character class; `negated` for `[^...]`.
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
    /// A zero-width anchor.
    Anchor(Anchor),
    /// A sequence of nodes.
    Concat(Vec<Ast>),
    /// `a|b|...` alternation.
    Alt(Vec<Ast>),
    /// A parenthesized group (capturing or `(?:...)`).
    Group(Box<Ast>),
    /// A scoped or global set of matching flags.
    Scoped { flags: Flags, node: Box<Ast> },
    /// An atomic group `(?>...)`.
    Atomic(Box<Ast>),
    /// A quantified node. `max = None` means unbounded.
    Repeat {
        min: u32,
        max: Option<u32>,
        mode: RepeatMode,
        node: Box<Ast>,
    },
    /// A back-reference `\1` (always rejected by the safety pass).
    Backref,
    /// A look-around assertion (always rejected by the safety pass).
    Look,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RepeatMode {
    #[default]
    Greedy,
    Lazy,
    Possessive,
}

#[derive(Clone, Copy)]
struct RepeatSpec {
    min: u32,
    max: Option<u32>,
    mode: RepeatMode,
}

/// One item inside a `[...]` class.
#[derive(Clone, Debug, PartialEq)]
enum ClassItem {
    /// A single character.
    Char(char),
    /// An inclusive character range `a-z`.
    Range(char, char),
    /// A shorthand class such as `\d`, with `negated` for `\D`.
    Shorthand { kind: Shorthand, negated: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shorthand {
    Digit,
    Word,
    Space,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    /// `^` -- start of string (no MULTILINE).
    Start,
    /// `$` -- end of string or just before a trailing newline (no MULTILINE).
    End,
    /// `\A` -- start of string.
    StartString,
    /// `\Z` -- end of string.
    EndString,
    /// `\b` -- word boundary.
    WordBoundary,
    /// `\B` -- not a word boundary.
    NotWordBoundary,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Flags {
    ascii: bool,
    ignore_case: bool,
    multiline: bool,
    dot_all: bool,
    verbose: bool,
}

// ---- parser --------------------------------------------------------------- //

/// Mirrors Python's `str.isidentifier`, which the reference parser applies to
/// `(?P<name>...)` group names: XID_Start or `_` first, XID_Continue after.
fn is_group_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !unicode_ident::is_xid_start(first) {
        return false;
    }
    chars.all(unicode_ident::is_xid_continue)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    flags: Flags,
    group_names: Vec<String>,
}

impl Parser {
    fn new(pattern: &str) -> Self {
        Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            flags: Flags::default(),
            group_names: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn parse(&mut self) -> Result<Ast, RegexError> {
        let ast = self.parse_alt()?;
        self.skip_ignored();
        if self.pos != self.chars.len() {
            // A stray `)` or other leftover.
            return Err(RegexError::new(alloc::format!(
                "unbalanced or unexpected character at position {}",
                self.pos
            )));
        }
        Ok(Ast::Scoped {
            flags: self.flags,
            node: Box::new(ast),
        })
    }

    fn parse_alt(&mut self) -> Result<Ast, RegexError> {
        let mut branches = alloc::vec![self.parse_concat()?];
        self.skip_ignored();
        while self.peek() == Some('|') {
            self.bump();
            branches.push(self.parse_concat()?);
            self.skip_ignored();
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(fold_branches(branches))
        }
    }

    fn parse_concat(&mut self) -> Result<Ast, RegexError> {
        let mut parts = Vec::new();
        loop {
            self.skip_ignored();
            let Some(c) = self.peek() else {
                break;
            };
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.parse_atom()?;
            let quantified = self.parse_quantifier(atom)?;
            if !matches!(quantified, Ast::Empty) {
                parts.push(quantified);
            }
        }
        Ok(match parts.len() {
            0 => Ast::Empty,
            1 => parts.pop().unwrap(),
            _ => Ast::Concat(parts),
        })
    }

    fn parse_quantifier(&mut self, atom: Ast) -> Result<Ast, RegexError> {
        self.skip_ignored();
        let (min, max) = match self.peek() {
            Some('*') => {
                self.bump();
                (0, None)
            }
            Some('+') => {
                self.bump();
                (1, None)
            }
            Some('?') => {
                self.bump();
                (0, Some(1))
            }
            Some('{') => match self.try_parse_bounds()? {
                Some(bounds) => bounds,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        if matches!(atom, Ast::Repeat { .. }) {
            return Err(RegexError::new(alloc::format!(
                "multiple repeat at position {}",
                self.pos
            )));
        }
        let mode = match self.peek() {
            Some('?') => {
                self.bump();
                RepeatMode::Lazy
            }
            Some('+') => {
                self.bump();
                RepeatMode::Possessive
            }
            _ => RepeatMode::Greedy,
        };
        Ok(Ast::Repeat {
            min,
            max,
            mode,
            node: alloc::boxed::Box::new(atom),
        })
    }

    fn skip_ignored(&mut self) {
        if !self.flags.verbose {
            return;
        }
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }
            if self.peek() != Some('#') {
                break;
            }
            while self.peek().is_some_and(|ch| ch != '\n') {
                self.bump();
            }
        }
    }

    /// Parse a `{m}`, `{m,}`, `{m,n}` or `{,n}` bound. Returns `None` (leaving a
    /// literal `{`) when the braces do not form a valid bound, matching Python.
    fn try_parse_bounds(&mut self) -> Result<Option<(u32, Option<u32>)>, RegexError> {
        let start = self.pos;
        self.bump(); // {
        let min_digits = self.take_digits();
        let mut max = None;
        let mut had_comma = false;
        if self.peek() == Some(',') {
            self.bump();
            had_comma = true;
            let max_digits = self.take_digits();
            if !max_digits.is_empty() {
                max = Some(max_digits);
            }
        }
        if self.peek() != Some('}') {
            // Not a quantifier; rewind and treat `{` as a literal.
            self.pos = start;
            return Ok(None);
        }
        if min_digits.is_empty() && !had_comma {
            self.pos = start;
            return Ok(None);
        }
        self.bump(); // }
        // An out-of-range repeat count is a pattern error, not a silent rewrite
        // to 0 / u32::MAX that changes the pattern's meaning (audit H10).
        let overflow = || {
            RegexError::new(alloc::format!(
                "repeat count out of range at position {start}"
            ))
        };
        let min = if min_digits.is_empty() {
            0
        } else {
            parse_u32(&min_digits).ok_or_else(overflow)?
        };
        let max = match max {
            Some(text) => Some(parse_u32(&text).ok_or_else(overflow)?),
            None if had_comma => None,
            None => Some(min),
        };
        if let Some(m) = max
            && m < min
        {
            return Err(RegexError::new(alloc::format!(
                "min repeat greater than max repeat at position {start}"
            )));
        }
        Ok(Some((min, max)))
    }

    fn take_digits(&mut self) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                out.push(c);
                self.bump();
            } else {
                break;
            }
        }
        out
    }

    fn parse_atom(&mut self) -> Result<Ast, RegexError> {
        let c = self
            .peek()
            .ok_or_else(|| RegexError::new("unexpected end of pattern"))?;
        match c {
            '(' => self.parse_group(),
            '[' => self.parse_class(),
            '.' => {
                self.bump();
                Ok(Ast::Any)
            }
            '^' => {
                self.bump();
                Ok(Ast::Anchor(Anchor::Start))
            }
            '$' => {
                self.bump();
                Ok(Ast::Anchor(Anchor::End))
            }
            '\\' => self.parse_escape(),
            '*' | '+' | '?' => Err(RegexError::new(alloc::format!(
                "nothing to repeat at position {}",
                self.pos
            ))),
            ')' => Err(RegexError::new(alloc::format!(
                "unbalanced parenthesis at position {}",
                self.pos
            ))),
            _ => {
                self.bump();
                Ok(Ast::Literal(c))
            }
        }
    }

    fn parse_group(&mut self) -> Result<Ast, RegexError> {
        let open = self.pos;
        let mut noncapturing = false;
        self.bump(); // (
        // Detect (?...) forms.
        if self.peek() == Some('?') {
            self.bump();
            match self.peek() {
                Some(':') => {
                    self.bump();
                    noncapturing = true;
                }
                Some('a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x' | '-') => {
                    let old_flags = self.flags;
                    let (flags, removed_any) = self.parse_flags(open)?;
                    match self.peek() {
                        Some(')') => {
                            if removed_any {
                                // Python only allows the bare `(?flags)` form for
                                // additions; removals require a scoped `:` group.
                                return Err(RegexError::new(alloc::format!(
                                    "missing : at position {open}"
                                )));
                            }
                            if open != 0 {
                                return Err(RegexError::new(alloc::format!(
                                    "global flags not at the start of the expression at position {open}"
                                )));
                            }
                            self.bump();
                            self.flags = flags;
                            return Ok(Ast::Empty);
                        }
                        Some(':') => {
                            self.bump();
                            self.flags = flags;
                            let inner = self.parse_alt()?;
                            self.expect_close(open)?;
                            self.flags = old_flags;
                            return Ok(Ast::Scoped {
                                flags,
                                node: Box::new(inner),
                            });
                        }
                        _ => {
                            return Err(RegexError::new(alloc::format!(
                                "missing : at position {open}"
                            )));
                        }
                    }
                }
                Some('P') => {
                    self.bump();
                    if self.bump() != Some('<') {
                        return Err(RegexError::new(alloc::format!(
                            "unknown extension at position {open}"
                        )));
                    }
                    let name_start = self.pos;
                    while self.peek().is_some_and(|ch| ch != '>') {
                        self.bump();
                    }
                    if self.peek() != Some('>') || self.pos == name_start {
                        return Err(RegexError::new(alloc::format!(
                            "missing group name at position {open}"
                        )));
                    }
                    let name: String = self.chars[name_start..self.pos].iter().collect();
                    if !is_group_name(&name) {
                        return Err(RegexError::new(alloc::format!(
                            "bad character in group name '{name}' at position {name_start}"
                        )));
                    }
                    if self.group_names.contains(&name) {
                        return Err(RegexError::new(alloc::format!(
                            "redefinition of group name '{name}' at position {name_start}"
                        )));
                    }
                    self.group_names.push(name);
                    self.bump();
                }
                Some('>') => {
                    self.bump();
                    let inner = self.parse_alt()?;
                    self.expect_close(open)?;
                    return Ok(Ast::Atomic(Box::new(inner)));
                }
                // Look-arounds -- parsed then rejected by the safety pass.
                Some('=') | Some('!') => {
                    self.bump();
                    let _ = self.parse_alt()?;
                    self.expect_close(open)?;
                    return Ok(Ast::Look);
                }
                Some('<') => {
                    self.bump();
                    match self.peek() {
                        Some('=') | Some('!') => {
                            self.bump();
                            let _ = self.parse_alt()?;
                            self.expect_close(open)?;
                            return Ok(Ast::Look);
                        }
                        _ => {
                            return Err(RegexError::new(alloc::format!(
                                "unknown extension at position {open}"
                            )));
                        }
                    }
                }
                Some('(') => {
                    // Conditional groups depend on capture state and are rejected by
                    // the reference safety pass. Parse their shape only far enough to
                    // classify them as unsafe rather than as a syntax extension.
                    self.bump();
                    while self.peek().is_some_and(|ch| ch != ')') {
                        self.bump();
                    }
                    if self.bump() != Some(')') {
                        return Err(RegexError::new(alloc::format!(
                            "unterminated conditional at position {open}"
                        )));
                    }
                    let _ = self.parse_alt()?;
                    self.expect_close(open)?;
                    return Ok(Ast::Look);
                }
                _ => {
                    return Err(RegexError::new(alloc::format!(
                        "unknown extension at position {open}"
                    )));
                }
            }
        }
        let inner = self.parse_alt()?;
        self.expect_close(open)?;
        if noncapturing {
            Ok(inner)
        } else {
            Ok(Ast::Group(alloc::boxed::Box::new(inner)))
        }
    }

    fn parse_flags(&mut self, open: usize) -> Result<(Flags, bool), RegexError> {
        let mut flags = self.flags;
        let mut removing = false;
        let mut saw_add = false;
        let mut saw_remove = false;
        // Bit masks over i/m/s/x so a flag both added and removed is caught,
        // and the last type flag ('a' or 'u') added, since they are exclusive.
        let mut added: u8 = 0;
        let mut removed_mask: u8 = 0;
        let mut type_flag: Option<char> = None;
        while let Some(ch) = self.peek() {
            if ch == '-' {
                if removing {
                    return Err(RegexError::new(alloc::format!(
                        "bad inline flags at position {open}"
                    )));
                }
                removing = true;
                self.bump();
                continue;
            }
            let bit: u8 = match ch {
                'i' => 1,
                'm' => 2,
                's' => 4,
                'x' => 8,
                // Unicode is the default; 'a' switches to ASCII mode. They are
                // mutually exclusive and may never be turned off, as in Python.
                'a' | 'u' => 0,
                // Locale mode is invalid for str patterns, matching Python's
                // parser rather than silently changing behavior.
                'L' => {
                    return Err(RegexError::new(alloc::format!(
                        "cannot use 'L' flag with a str pattern at position {open}"
                    )));
                }
                _ => break,
            };
            if removing {
                if matches!(ch, 'a' | 'u') {
                    return Err(RegexError::new(alloc::format!(
                        "bad inline flags: cannot turn off flags 'a', 'u' and 'L' at position {open}"
                    )));
                }
                removed_mask |= bit;
                saw_remove = true;
            } else {
                if matches!(ch, 'a' | 'u') {
                    if type_flag.is_some_and(|prev| prev != ch) {
                        return Err(RegexError::new(alloc::format!(
                            "bad inline flags: flags 'a', 'u' and 'L' are incompatible at position {open}"
                        )));
                    }
                    type_flag = Some(ch);
                }
                added |= bit;
                saw_add = true;
            }
            if added & removed_mask != 0 {
                return Err(RegexError::new(alloc::format!(
                    "bad inline flags: flag turned on and off at position {open}"
                )));
            }
            let slot = match ch {
                'a' => Some(&mut flags.ascii),
                'i' => Some(&mut flags.ignore_case),
                'm' => Some(&mut flags.multiline),
                's' => Some(&mut flags.dot_all),
                'x' => Some(&mut flags.verbose),
                _ => None,
            };
            if let Some(slot) = slot {
                *slot = !removing;
            }
            self.bump();
        }
        if !saw_add && !removing {
            return Err(RegexError::new(alloc::format!(
                "missing flag at position {open}"
            )));
        }
        if removing && !saw_remove {
            return Err(RegexError::new(alloc::format!(
                "missing flag at position {open}"
            )));
        }
        Ok((flags, removing))
    }

    fn expect_close(&mut self, open: usize) -> Result<(), RegexError> {
        if self.peek() == Some(')') {
            self.bump();
            Ok(())
        } else {
            Err(RegexError::new(alloc::format!(
                "missing ), unterminated subpattern at position {open}"
            )))
        }
    }

    fn parse_class(&mut self) -> Result<Ast, RegexError> {
        let open = self.pos;
        self.bump(); // [
        let negated = if self.peek() == Some('^') {
            self.bump();
            true
        } else {
            false
        };
        let mut items = Vec::new();
        // A `]` immediately after `[` or `[^` is a literal.
        if self.peek() == Some(']') {
            items.push(ClassItem::Char(']'));
            self.bump();
        }
        loop {
            match self.peek() {
                None => {
                    return Err(RegexError::new(alloc::format!(
                        "unterminated character set at position {open}"
                    )));
                }
                Some(']') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    let esc = self.parse_class_escape()?;
                    match esc {
                        ClassEscape::Shorthand { kind, negated } => {
                            items.push(ClassItem::Shorthand { kind, negated });
                        }
                        ClassEscape::Char(c) => {
                            self.maybe_range(&mut items, c)?;
                        }
                    }
                }
                Some(c) => {
                    self.bump();
                    self.maybe_range(&mut items, c)?;
                }
            }
        }
        Ok(Ast::Class { negated, items })
    }

    /// After reading a class member `lo`, look for `-hi` to form a range.
    fn maybe_range(&mut self, items: &mut Vec<ClassItem>, lo: char) -> Result<(), RegexError> {
        if self.peek() == Some('-') {
            // `-` before `]` is a literal dash.
            let after = self.chars.get(self.pos + 1).copied();
            if after == Some(']') || after.is_none() {
                items.push(ClassItem::Char(lo));
                return Ok(());
            }
            self.bump(); // -
            let hi = match self.peek() {
                Some('\\') => {
                    self.bump();
                    match self.parse_class_escape()? {
                        ClassEscape::Char(c) => c,
                        ClassEscape::Shorthand { .. } => {
                            return Err(RegexError::new(alloc::format!(
                                "bad character range at position {}",
                                self.pos
                            )));
                        }
                    }
                }
                Some(c) => {
                    self.bump();
                    c
                }
                None => {
                    return Err(RegexError::new("unterminated character set"));
                }
            };
            if (hi as u32) < (lo as u32) {
                return Err(RegexError::new(alloc::format!(
                    "bad character range {lo}-{hi} at position {}",
                    self.pos
                )));
            }
            items.push(ClassItem::Range(lo, hi));
        } else {
            items.push(ClassItem::Char(lo));
        }
        Ok(())
    }

    fn parse_escape(&mut self) -> Result<Ast, RegexError> {
        let start = self.pos;
        self.bump(); // backslash
        let c = self
            .peek()
            .ok_or_else(|| RegexError::new("trailing backslash"))?;
        if c.is_ascii_digit() && c != '0' {
            self.bump();
            // A back-reference; consume any further digits.
            while matches!(self.peek(), Some(d) if d.is_ascii_digit()) {
                self.bump();
            }
            return Ok(Ast::Backref);
        }
        self.bump();
        Ok(match c {
            'd' => Ast::Class {
                negated: false,
                items: alloc::vec![ClassItem::Shorthand {
                    kind: Shorthand::Digit,
                    negated: false
                }],
            },
            'D' => Ast::Class {
                negated: false,
                items: alloc::vec![ClassItem::Shorthand {
                    kind: Shorthand::Digit,
                    negated: true
                }],
            },
            'w' => Ast::Class {
                negated: false,
                items: alloc::vec![ClassItem::Shorthand {
                    kind: Shorthand::Word,
                    negated: false
                }],
            },
            'W' => Ast::Class {
                negated: false,
                items: alloc::vec![ClassItem::Shorthand {
                    kind: Shorthand::Word,
                    negated: true
                }],
            },
            's' => Ast::Class {
                negated: false,
                items: alloc::vec![ClassItem::Shorthand {
                    kind: Shorthand::Space,
                    negated: false
                }],
            },
            'S' => Ast::Class {
                negated: false,
                items: alloc::vec![ClassItem::Shorthand {
                    kind: Shorthand::Space,
                    negated: true
                }],
            },
            'b' => Ast::Anchor(Anchor::WordBoundary),
            'B' => Ast::Anchor(Anchor::NotWordBoundary),
            'A' => Ast::Anchor(Anchor::StartString),
            'Z' => Ast::Anchor(Anchor::EndString),
            'x' => Ast::Literal(self.parse_hex_escape(2, start)?),
            'u' => Ast::Literal(self.parse_hex_escape(4, start)?),
            'U' => Ast::Literal(self.parse_hex_escape(8, start)?),
            other if other.is_ascii_alphabetic() => {
                return Err(RegexError::new(alloc::format!(
                    "bad escape \\{other} at position {start}"
                )));
            }
            other => Ast::Literal(unescape(other)),
        })
    }

    fn parse_class_escape(&mut self) -> Result<ClassEscape, RegexError> {
        let start = self.pos.saturating_sub(1);
        let c = self
            .peek()
            .ok_or_else(|| RegexError::new("trailing backslash in character set"))?;
        self.bump();
        Ok(match c {
            'd' => ClassEscape::Shorthand {
                kind: Shorthand::Digit,
                negated: false,
            },
            'D' => ClassEscape::Shorthand {
                kind: Shorthand::Digit,
                negated: true,
            },
            'w' => ClassEscape::Shorthand {
                kind: Shorthand::Word,
                negated: false,
            },
            'W' => ClassEscape::Shorthand {
                kind: Shorthand::Word,
                negated: true,
            },
            's' => ClassEscape::Shorthand {
                kind: Shorthand::Space,
                negated: false,
            },
            'S' => ClassEscape::Shorthand {
                kind: Shorthand::Space,
                negated: true,
            },
            'b' => ClassEscape::Char('\u{08}'),
            'x' => ClassEscape::Char(self.parse_hex_escape(2, start)?),
            'u' => ClassEscape::Char(self.parse_hex_escape(4, start)?),
            'U' => ClassEscape::Char(self.parse_hex_escape(8, start)?),
            other if other.is_ascii_alphabetic() => {
                return Err(RegexError::new(alloc::format!(
                    "bad escape \\{other} at position {start}"
                )));
            }
            other => ClassEscape::Char(unescape(other)),
        })
    }

    fn parse_hex_escape(&mut self, width: usize, start: usize) -> Result<char, RegexError> {
        let mut value = 0u32;
        for _ in 0..width {
            let Some(ch) = self.bump() else {
                return Err(RegexError::new(alloc::format!(
                    "incomplete escape at position {start}"
                )));
            };
            let Some(digit) = ch.to_digit(16) else {
                return Err(RegexError::new(alloc::format!(
                    "incomplete escape at position {start}"
                )));
            };
            value = (value << 4) | digit;
        }
        char::from_u32(value)
            .ok_or_else(|| RegexError::new(alloc::format!("bad escape at position {start}")))
    }
}

enum ClassEscape {
    Char(char),
    Shorthand { kind: Shorthand, negated: bool },
}

fn unescape(c: char) -> char {
    match c {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'f' => '\u{0c}',
        'v' => '\u{0b}',
        'a' => '\u{07}',
        '0' => '\0',
        other => other,
    }
}

fn parse_u32(digits: &str) -> Option<u32> {
    digits.parse::<u32>().ok()
}

/// Fold an alternation of single-character branches into one character class,
/// matching Python's `sre_parse` optimization (so the safety pass sees a class,
/// not a branch, for e.g., `(a|b)+`).
fn fold_branches(branches: Vec<Ast>) -> Ast {
    let split: Vec<Vec<Ast>> = branches
        .into_iter()
        .map(|branch| match branch {
            Ast::Concat(parts) => parts,
            branch => alloc::vec![branch],
        })
        .collect();
    let shortest = split.iter().map(Vec::len).min().unwrap_or(0);
    let mut prefix = 0;
    while prefix < shortest
        && split
            .iter()
            .skip(1)
            .all(|branch| branch[prefix] == split[0][prefix])
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < shortest.saturating_sub(prefix)
        && split.iter().skip(1).all(|branch| {
            branch[branch.len() - 1 - suffix] == split[0][split[0].len() - 1 - suffix]
        })
    {
        suffix += 1;
    }
    let common_prefix = split[0][..prefix].to_vec();
    let common_suffix = if suffix == 0 {
        Vec::new()
    } else {
        split[0][split[0].len() - suffix..].to_vec()
    };
    let residual: Vec<Ast> = split
        .into_iter()
        .map(|branch| {
            let end = branch.len() - suffix;
            make_concat(branch[prefix..end].to_vec())
        })
        .collect();

    let mut items = Vec::new();
    for branch in &residual {
        match branch {
            Ast::Literal(c) => items.push(ClassItem::Char(*c)),
            Ast::Class {
                negated: false,
                items: class_items,
            } => items.extend(class_items.iter().cloned()),
            _ => {
                let middle = Ast::Alt(residual);
                let mut result = common_prefix;
                result.push(middle);
                result.extend(common_suffix);
                return make_concat(result);
            }
        }
    }
    let middle = Ast::Class {
        negated: false,
        items,
    };
    let mut result = common_prefix;
    result.push(middle);
    result.extend(common_suffix);
    make_concat(result)
}

fn make_concat(mut parts: Vec<Ast>) -> Ast {
    match parts.len() {
        0 => Ast::Empty,
        1 => parts.pop().expect("one part"),
        _ => Ast::Concat(parts),
    }
}

// ---- safety analysis ------------------------------------------------------ //

/// Enforce the linear subset: reject back-references, look-arounds, nested or
/// branch-bearing repeats, and more than one (or an unanchored) variable
/// quantifier. Mirrors the reference `safe`/`collect`/anchor logic; the error
/// messages match its `ValueError` text.
fn check_safe(ast: &Ast) -> Result<(), RegexError> {
    if !safe(ast, false) {
        return Err(RegexError::new(
            "pattern uses a potentially superlinear construct",
        ));
    }
    let mut variable = 0usize;
    collect_variable(ast, &mut variable);
    let anchored = starts_anchored(ast);
    if variable > 1 || (variable >= 1 && !anchored) {
        return Err(RegexError::new(
            "variable quantifiers require a start anchor and one repeat path",
        ));
    }
    if !anchored && fixed_match_work(ast) > 4096 {
        return Err(RegexError::new(
            "pattern uses a potentially superlinear construct",
        ));
    }
    Ok(())
}

fn fixed_match_work(ast: &Ast) -> usize {
    match ast {
        Ast::Repeat { min, max, node, .. } => {
            let child = fixed_match_work(node).max(1);
            if *max == Some(*min) {
                usize::try_from(*min)
                    .unwrap_or(usize::MAX)
                    .saturating_mul(child)
            } else {
                child
            }
        }
        Ast::Alt(branches) => branches.iter().fold(0usize, |work, branch| {
            work.saturating_add(fixed_match_work(branch))
        }),
        Ast::Concat(parts) => parts.iter().fold(0usize, |work, part| {
            work.saturating_add(fixed_match_work(part))
        }),
        Ast::Group(inner) | Ast::Scoped { node: inner, .. } | Ast::Atomic(inner) => {
            fixed_match_work(inner)
        }
        Ast::Empty => 0,
        _ => 1,
    }
}

fn safe(ast: &Ast, inside_repeat: bool) -> bool {
    match ast {
        Ast::Backref | Ast::Look => false,
        Ast::Repeat { node, .. } => {
            if inside_repeat {
                return false;
            }
            if contains_top_branch(node) {
                return false;
            }
            safe(node, true)
        }
        Ast::Alt(branches) => {
            if inside_repeat {
                return false;
            }
            branches.iter().all(|b| safe(b, inside_repeat))
        }
        Ast::Group(inner) | Ast::Scoped { node: inner, .. } => safe(inner, inside_repeat),
        Ast::Atomic(inner) => safe(inner, inside_repeat),
        Ast::Concat(parts) => parts.iter().all(|p| safe(p, inside_repeat)),
        _ => true,
    }
}

/// True when `node` (as the direct body of a repeat) is or immediately contains
/// an alternation -- mirrors the reference's "branch directly inside a repeat"
/// rejection before it descends further.
fn contains_top_branch(node: &Ast) -> bool {
    match node {
        Ast::Alt(_) => true,
        Ast::Concat(parts) => parts.iter().any(contains_top_branch),
        _ => false,
    }
}

fn collect_variable(ast: &Ast, count: &mut usize) {
    match ast {
        Ast::Repeat { min, max, node, .. } => {
            if max.map(u64::from) != Some(u64::from(*min)) {
                *count += 1;
            }
            collect_variable(node, count);
        }
        Ast::Alt(branches) => branches.iter().for_each(|b| collect_variable(b, count)),
        Ast::Group(inner) | Ast::Scoped { node: inner, .. } => collect_variable(inner, count),
        Ast::Atomic(inner) => collect_variable(inner, count),
        Ast::Concat(parts) => parts.iter().for_each(|p| collect_variable(p, count)),
        _ => {}
    }
}

fn starts_anchored(ast: &Ast) -> bool {
    let root = match ast {
        Ast::Scoped { node, .. } => node.as_ref(),
        _ => ast,
    };
    match root {
        Ast::Anchor(Anchor::Start | Anchor::StartString) => true,
        Ast::Concat(parts) => parts
            .first()
            .is_some_and(|part| matches!(part, Ast::Anchor(Anchor::Start | Anchor::StartString))),
        _ => false,
    }
}

/// Minimum number of input scalars an expression must consume. Saturation is
/// intentional: an enormous fixed repeat then cannot fit any ordinary input.
fn minimum_width(ast: &Ast) -> usize {
    match ast {
        Ast::Empty | Ast::Anchor(_) | Ast::Backref | Ast::Look => 0,
        Ast::Literal(_) | Ast::Any | Ast::Class { .. } => 1,
        Ast::Concat(parts) => parts.iter().fold(0usize, |width, part| {
            width.saturating_add(minimum_width(part))
        }),
        Ast::Alt(branches) => branches.iter().map(minimum_width).min().unwrap_or(0),
        Ast::Group(inner) | Ast::Scoped { node: inner, .. } | Ast::Atomic(inner) => {
            minimum_width(inner)
        }
        Ast::Repeat { min, node, .. } => usize::try_from(*min)
            .unwrap_or(usize::MAX)
            .saturating_mul(minimum_width(node)),
    }
}

// ---- matcher (re.search semantics) ---------------------------------------- //

/// True when `ast` matches anywhere in `text`.
fn search(ast: &Ast, text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut scratch = MatchScratch::new(chars.len() + 1);
    search_chars(ast, &chars, &mut scratch)
}

fn search_chars(ast: &Ast, chars: &[char], scratch: &mut MatchScratch) -> bool {
    let Some(last_start) = chars.len().checked_sub(minimum_width(ast)) else {
        return false;
    };
    for start in 0..=last_start {
        scratch.note_work(1);
        if !ends(ast, chars, start, Flags::default(), scratch).is_empty() {
            return true;
        }
    }
    false
}

/// Reusable endpoint-deduplication storage. Generational marks make clearing a
/// set O(1), so a repeat does not zero an input-sized bitmap for every consumed
/// character (and an unanchored search does not zero one for every start).
struct MatchScratch {
    marks: Vec<usize>,
    generation: usize,
    #[cfg(test)]
    work: usize,
}

impl MatchScratch {
    fn new(position_count: usize) -> Self {
        Self {
            marks: alloc::vec![0; position_count],
            generation: 0,
            #[cfg(test)]
            work: position_count,
        }
    }

    fn dedup(&mut self, positions: &mut Vec<usize>) {
        self.note_work(positions.len());
        self.generation = match self.generation.checked_add(1) {
            Some(next) => next,
            None => {
                self.marks.fill(0);
                1
            }
        };
        let generation = self.generation;
        positions.retain(|position| {
            let mark = &mut self.marks[*position];
            if *mark == generation {
                false
            } else {
                *mark = generation;
                true
            }
        });
    }

    fn note_work(&mut self, amount: usize) {
        #[cfg(test)]
        {
            self.work = self.work.saturating_add(amount);
        }
        #[cfg(not(test))]
        {
            let _ = amount;
        }
    }
}

/// Every position at which `ast` can finish, starting from `start`. A non-empty
/// result means a match exists from `start`. Bounded because the safe subset has
/// no nested or multiple variable quantifiers.
fn ends(
    ast: &Ast,
    chars: &[char],
    start: usize,
    flags: Flags,
    scratch: &mut MatchScratch,
) -> Vec<usize> {
    scratch.note_work(1);
    match ast {
        Ast::Empty => alloc::vec![start],
        Ast::Literal(c) => match chars.get(start) {
            Some(ch) if chars_equal(*ch, *c, flags) => alloc::vec![start + 1],
            _ => Vec::new(),
        },
        Ast::Any => match chars.get(start) {
            Some(ch) if flags.dot_all || *ch != '\n' => alloc::vec![start + 1],
            _ => Vec::new(),
        },
        Ast::Class { negated, items } => match chars.get(start) {
            Some(ch) => {
                let hit = items
                    .iter()
                    .any(|item| class_item_matches(item, *ch, flags));
                if hit != *negated {
                    alloc::vec![start + 1]
                } else {
                    Vec::new()
                }
            }
            None => Vec::new(),
        },
        Ast::Anchor(anchor) => {
            if anchor_holds(*anchor, chars, start, flags) {
                alloc::vec![start]
            } else {
                Vec::new()
            }
        }
        Ast::Group(inner) => ends(inner, chars, start, flags, scratch),
        Ast::Scoped {
            flags: scoped,
            node,
        } => ends(node, chars, start, *scoped, scratch),
        Ast::Atomic(inner) => ends(inner, chars, start, flags, scratch)
            .into_iter()
            .next()
            .into_iter()
            .collect(),
        Ast::Concat(parts) => {
            let mut positions = alloc::vec![start];
            for part in parts {
                let mut next = Vec::new();
                for &p in &positions {
                    next.extend(ends(part, chars, p, flags, scratch));
                }
                scratch.dedup(&mut next);
                if next.is_empty() {
                    return Vec::new();
                }
                positions = next;
            }
            positions
        }
        Ast::Alt(branches) => {
            let mut out = Vec::new();
            for branch in branches {
                out.extend(ends(branch, chars, start, flags, scratch));
            }
            scratch.dedup(&mut out);
            out
        }
        Ast::Repeat {
            min,
            max,
            mode,
            node,
        } => repeat_ends(
            node,
            RepeatSpec {
                min: *min,
                max: *max,
                mode: *mode,
            },
            chars,
            start,
            flags,
            scratch,
        ),
        Ast::Backref | Ast::Look => Vec::new(),
    }
}

fn repeat_ends(
    node: &Ast,
    repeat: RepeatSpec,
    chars: &[char],
    start: usize,
    flags: Flags,
    scratch: &mut MatchScratch,
) -> Vec<usize> {
    // Reachable positions after 0,1,2,... repetitions. `reached` grows until it
    // stops changing (or the max count is hit); positions with >= min reps are
    // valid ends.
    let mut out = Vec::new();
    let mut frontier = alloc::vec![start];
    let mut reps: u32 = 0;
    if repeat.min == 0 {
        out.push(start);
    }
    loop {
        if let Some(m) = repeat.max
            && reps >= m
        {
            break;
        }
        let mut next = Vec::new();
        let mut advanced = false;
        for &p in &frontier {
            for e in ends(node, chars, p, flags, scratch) {
                advanced |= e != p;
                next.push(e);
            }
        }
        scratch.dedup(&mut next);
        if next.is_empty() {
            break;
        }
        reps += 1;
        for &e in &next {
            if reps >= repeat.min {
                out.push(e);
            }
        }
        if !advanced {
            if reps < repeat.min {
                out.extend_from_slice(&next);
            }
            break;
        }
        frontier = next;
        // Safety valve: positions are bounded by input length, so this cannot
        // exceed `chars.len() + 1` distinct values.
        if reps as usize > chars.len() + 1 {
            break;
        }
    }
    scratch.dedup(&mut out);
    if repeat.mode != RepeatMode::Lazy {
        out.reverse();
    }
    if repeat.mode == RepeatMode::Possessive {
        out.truncate(1);
    }
    out
}

fn class_item_matches(item: &ClassItem, ch: char, flags: Flags) -> bool {
    match item {
        ClassItem::Char(c) => chars_equal(*c, ch, flags),
        ClassItem::Range(lo, hi) => range_matches(*lo, *hi, ch, flags),
        ClassItem::Shorthand { kind, negated } => shorthand_matches(*kind, ch, flags) != *negated,
    }
}

fn shorthand_matches(kind: Shorthand, ch: char, flags: Flags) -> bool {
    match kind {
        Shorthand::Digit if flags.ascii => ch.is_ascii_digit(),
        Shorthand::Word if flags.ascii => ch.is_ascii_alphanumeric() || ch == '_',
        Shorthand::Space if flags.ascii => {
            matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}')
        }
        Shorthand::Digit => is_python_decimal(ch),
        Shorthand::Word => ch.is_alphanumeric() || ch == '_',
        Shorthand::Space => is_python_whitespace(ch),
    }
}

fn is_word(ch: char, flags: Flags) -> bool {
    shorthand_matches(Shorthand::Word, ch, flags)
}

/// Python's Unicode-aware `\d` category (`str.isdecimal`, general category
/// Nd). The compact ranges are stable for the Unicode data bundled with the
/// AXON 2026 reference runtime.
fn is_python_decimal(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0030..=0x0039
            | 0x0660..=0x0669
            | 0x06F0..=0x06F9
            | 0x07C0..=0x07C9
            | 0x0966..=0x096F
            | 0x09E6..=0x09EF
            | 0x0A66..=0x0A6F
            | 0x0AE6..=0x0AEF
            | 0x0B66..=0x0B6F
            | 0x0BE6..=0x0BEF
            | 0x0C66..=0x0C6F
            | 0x0CE6..=0x0CEF
            | 0x0D66..=0x0D6F
            | 0x0DE6..=0x0DEF
            | 0x0E50..=0x0E59
            | 0x0ED0..=0x0ED9
            | 0x0F20..=0x0F29
            | 0x1040..=0x1049
            | 0x1090..=0x1099
            | 0x17E0..=0x17E9
            | 0x1810..=0x1819
            | 0x1946..=0x194F
            | 0x19D0..=0x19D9
            | 0x1A80..=0x1A89
            | 0x1A90..=0x1A99
            | 0x1B50..=0x1B59
            | 0x1BB0..=0x1BB9
            | 0x1C40..=0x1C49
            | 0x1C50..=0x1C59
            | 0xA620..=0xA629
            | 0xA8D0..=0xA8D9
            | 0xA900..=0xA909
            | 0xA9D0..=0xA9D9
            | 0xA9F0..=0xA9F9
            | 0xAA50..=0xAA59
            | 0xABF0..=0xABF9
            | 0xFF10..=0xFF19
            | 0x104A0..=0x104A9
            | 0x10D30..=0x10D39
            | 0x11066..=0x1106F
            | 0x110F0..=0x110F9
            | 0x11136..=0x1113F
            | 0x111D0..=0x111D9
            | 0x112F0..=0x112F9
            | 0x11450..=0x11459
            | 0x114D0..=0x114D9
            | 0x11650..=0x11659
            | 0x116C0..=0x116C9
            | 0x11730..=0x11739
            | 0x118E0..=0x118E9
            | 0x11950..=0x11959
            | 0x11C50..=0x11C59
            | 0x11D50..=0x11D59
            | 0x11DA0..=0x11DA9
            | 0x11F50..=0x11F59
            | 0x16A60..=0x16A69
            | 0x16AC0..=0x16AC9
            | 0x16B50..=0x16B59
            | 0x1D7CE..=0x1D7FF
            | 0x1E140..=0x1E149
            | 0x1E2F0..=0x1E2F9
            | 0x1E4F0..=0x1E4F9
            | 0x1E950..=0x1E959
            | 0x1FBF0..=0x1FBF9
    )
}

/// Python's `str.isspace` set, which is slightly broader than the Unicode
/// White_Space property because it includes the four information separators.
fn is_python_whitespace(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0009..=0x000D
            | 0x001C..=0x0020
            | 0x0085
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028..=0x2029
            | 0x202F
            | 0x205F
            | 0x3000
    )
}

fn chars_equal(left: char, right: char, flags: Flags) -> bool {
    if left == right {
        return true;
    }
    if !flags.ignore_case {
        return false;
    }
    if flags.ascii {
        return left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(&right);
    }
    let left = case_variants(left);
    let right = case_variants(right);
    left.iter().any(|candidate| right.contains(candidate))
}

fn range_matches(low: char, high: char, ch: char, flags: Flags) -> bool {
    let in_range = |candidate: char| (low as u32..=high as u32).contains(&(candidate as u32));
    if in_range(ch) {
        return true;
    }
    if !flags.ignore_case || (flags.ascii && !ch.is_ascii()) {
        return false;
    }
    case_variants(ch).into_iter().any(in_range)
}

fn case_variants(ch: char) -> Vec<char> {
    let mut variants = alloc::vec![ch];
    let mut cursor = 0;
    while cursor < variants.len() {
        let current = variants[cursor];
        cursor += 1;

        // Python's regex engine uses simple Unicode case mappings. Rust's
        // casing iterators expose full mappings as well (`ß` -> `SS`), which
        // must not make one input code point equal either `S`. Keep only
        // one-code-point transforms and close over them transitively.
        if let Some(candidate) = one_char(current.to_lowercase()) {
            push_case_variant(&mut variants, candidate);
        }
        if let Some(candidate) = one_char(current.to_uppercase()) {
            push_case_variant(&mut variants, candidate);
        }

        // These simple mappings are not all exposed by Rust's full casing
        // iterators but are part of CPython's IGNORECASE equivalence classes.
        match current {
            'ß' => push_case_variant(&mut variants, 'ẞ'),
            'ẞ' => push_case_variant(&mut variants, 'ß'),
            'i' | 'I' | 'İ' | 'ı' => {
                for candidate in ['i', 'I', 'İ', 'ı'] {
                    push_case_variant(&mut variants, candidate);
                }
            }
            _ => {}
        }
    }
    variants
}

fn one_char(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let only = chars.next()?;
    chars.next().is_none().then_some(only)
}

fn push_case_variant(variants: &mut Vec<char>, candidate: char) {
    if !variants.contains(&candidate) {
        variants.push(candidate);
    }
}

fn anchor_holds(anchor: Anchor, chars: &[char], pos: usize, flags: Flags) -> bool {
    let n = chars.len();
    match anchor {
        Anchor::Start if flags.multiline => {
            pos == 0 || chars.get(pos.wrapping_sub(1)) == Some(&'\n')
        }
        Anchor::Start | Anchor::StartString => pos == 0,
        Anchor::EndString => pos == n,
        Anchor::End if flags.multiline => pos == n || chars.get(pos) == Some(&'\n'),
        Anchor::End => pos == n || (pos + 1 == n && chars[pos] == '\n'),
        Anchor::WordBoundary => word_boundary(chars, pos, flags),
        Anchor::NotWordBoundary => !word_boundary(chars, pos, flags),
    }
}

fn word_boundary(chars: &[char], pos: usize, flags: Flags) -> bool {
    let before = pos > 0 && is_word(chars[pos - 1], flags);
    let after = pos < chars.len() && is_word(chars[pos], flags);
    before != after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_constructs_keep_python_order() {
        assert!(!safe_regex_search(r"^a++a$", "aa").unwrap());
        assert!(!safe_regex_search(r"^(?>a|ab)c$", "abc").unwrap());
        assert!(safe_regex_search(r"^(?>a+)b$", "aaab").unwrap());
    }

    #[test]
    fn ignore_case_uses_simple_unicode_mappings() {
        assert!(!safe_regex_search(r"(?i)^ß$", "S").unwrap());
        assert!(safe_regex_search(r"(?i)^ß$", "ẞ").unwrap());
        assert!(safe_regex_search(r"(?i)^İ$", "ı").unwrap());
    }

    #[test]
    fn atomic_groups_participate_in_safety_analysis() {
        let error = safe_regex_search(r"(?>a+)b", "aaab").unwrap_err();
        assert_eq!(
            error.message,
            "variable quantifiers require a start anchor and one repeat path"
        );
    }

    #[test]
    fn repeat_matching_work_scales_linearly() {
        let short = alloc::format!("{}b", "a".repeat(256));
        let long = alloc::format!("{}b", "a".repeat(512));
        let (short_match, short_work) = measured_search(r"^a+$", &short);
        let (long_match, long_work) = measured_search(r"^a+$", &long);

        assert!(!short_match);
        assert!(!long_match);
        assert!(
            long_work <= short_work.saturating_mul(3),
            "doubling input grew measured work from {short_work} to {long_work}"
        );

        for pattern in [r"a{4097}", r"a{500000}b"] {
            let error = safe_regex_search(pattern, "a").unwrap_err();
            assert_eq!(
                error.message,
                "pattern uses a potentially superlinear construct"
            );
        }
        assert!(!safe_regex_search(r"a{4096}", "a").unwrap());
        assert!(!safe_regex_search(r"a{2048}a{2048}", "a").unwrap());
        assert!(!safe_regex_search(r"^a{500000}b$", "a").unwrap());

        let cumulative = alloc::format!("{}b", "a{4096}".repeat(100));
        let error = safe_regex_search(&cumulative, "a").unwrap_err();
        assert_eq!(
            error.message,
            "pattern uses a potentially superlinear construct"
        );
    }

    fn measured_search(pattern: &str, text: &str) -> (bool, usize) {
        let ast = Parser::new(pattern).parse().unwrap();
        check_safe(&ast).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let mut scratch = MatchScratch::new(chars.len() + 1);
        let matched = search_chars(&ast, &chars, &mut scratch);
        (matched, scratch.work)
    }
}
