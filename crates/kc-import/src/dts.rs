//! A small reader for devicetree source, enough for keymap files: nodes,
//! labels and properties, with the original text of each node kept so that
//! anything not understood can be carried over verbatim.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub name: String,
    /// The text after `=`, without the closing `;`. `None` for a flag.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Labels placed before the node name, as in `label: name { ... }`.
    pub labels: Vec<String>,
    /// The node name, `/` for the root, or `&label` for a reference.
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
    /// The node as written, from its labels to its closing `};`.
    pub text: String,
    /// The `//` comment lines directly above the node, joined with spaces.
    pub comment: String,
}

impl Node {
    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.value.as_deref())
    }

    pub fn has(&self, name: &str) -> bool {
        self.props.iter().any(|p| p.name == name)
    }

    /// The first string in a property, as in `compatible = "zmk,keymap"`.
    pub fn string(&self, name: &str) -> Option<&str> {
        self.prop(name)?.split('"').nth(1)
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The label a binding would use to refer to this node.
    pub fn label(&self) -> Option<&str> {
        self.labels.first().map(String::as_str)
    }
}

/// What a keymap file holds once comments and preprocessor lines are out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    /// Every top-level node, in order: `/ { ... }` blocks and `&label`
    /// overrides.
    pub nodes: Vec<Node>,
    /// Object-like `#define`s: name to replacement text.
    pub defines: HashMap<String, String>,
    /// Function-like macros the file defines, by name. Uses of them are
    /// expanded before the nodes are read.
    pub macros: Vec<String>,
    /// `#include` lines, as written.
    pub includes: Vec<String>,
    /// Names the file tests with `#if`, `#ifdef` or `#ifndef` that neither
    /// it nor a ZMK header the app knows defines. They are taken as not
    /// defined, as a compiler would.
    pub unknown_tests: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DtsError {
    #[error("line {0}: a `{{` is never closed")]
    Unclosed(usize),
    #[error("line {0}: expected `;`, `=` or `{{` after `{1}`")]
    Unexpected(usize, String),
    #[error("line {0}: `#{1}` has no matching `#if`")]
    Unmatched(usize, String),
    #[error("line {0}: the arguments of `{1}(` are never closed")]
    UnclosedCall(usize, String),
    #[error("line {0}: `{1}` takes {2} argument(s), not {3}")]
    Arguments(usize, String, usize, usize),
}

/// Removes comments, keeping line breaks so line numbers stay right.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_string = !in_string;
                out.push(c);
            }
            '/' if !in_string && chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if !in_string && chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                    }
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    /// Own-line `//` comments, by line number.
    comments: &'a HashMap<usize, String>,
}

impl Parser<'_> {
    fn line(&self) -> usize {
        self.src[..self.pos].matches('\n').count() + 1
    }

    fn skip_space(&mut self) {
        let rest = &self.src[self.pos..];
        self.pos += rest.len() - rest.trim_start().len();
    }

    /// Reads up to the next `{`, `=` or `;` outside any bracket or string.
    fn head(&mut self) -> Option<(String, char)> {
        let start = self.pos;
        let mut in_string = false;
        for (offset, c) in self.src[start..].char_indices() {
            match c {
                '"' => in_string = !in_string,
                '{' | '=' | ';' | '}' if !in_string => {
                    self.pos = start + offset + 1;
                    return Some((self.src[start..start + offset].trim().to_string(), c));
                }
                _ => {}
            }
        }
        None
    }

    /// Reads a property value up to its closing `;`.
    fn value(&mut self) -> String {
        let start = self.pos;
        let (mut depth, mut in_string) = (0i32, false);
        for (offset, c) in self.src[start..].char_indices() {
            match c {
                '"' => in_string = !in_string,
                '<' | '(' | '[' if !in_string => depth += 1,
                '>' | ')' | ']' if !in_string => depth -= 1,
                ';' if !in_string && depth <= 0 => {
                    self.pos = start + offset + 1;
                    return self.src[start..start + offset].trim().to_string();
                }
                _ => {}
            }
        }
        self.pos = self.src.len();
        self.src[start..].trim().to_string()
    }

    /// Parses the items inside a node, or at the top level.
    fn items(&mut self, top: bool) -> Result<(Vec<Prop>, Vec<Node>), DtsError> {
        let (mut props, mut children) = (Vec::new(), Vec::new());
        loop {
            self.skip_space();
            let start = self.pos;
            let line = self.line();
            let Some((head, end)) = self.head() else {
                return if top {
                    Ok((props, children))
                } else {
                    Err(DtsError::Unclosed(line))
                };
            };
            match end {
                '}' if !top && head.is_empty() => {
                    // Consume the `;` that closes the node.
                    self.skip_space();
                    if self.src[self.pos..].starts_with(';') {
                        self.pos += 1;
                    }
                    return Ok((props, children));
                }
                '{' => {
                    let mut parts: Vec<&str> = head.split(':').map(str::trim).collect();
                    let name = parts.pop().unwrap_or_default().to_string();
                    let labels = parts.into_iter().map(str::to_string).collect();
                    // The comment lines that end just above the node.
                    let mut comment: Vec<&str> = Vec::new();
                    let mut above = line;
                    while let Some(text) = above.checked_sub(1).and_then(|l| self.comments.get(&l))
                    {
                        comment.push(text);
                        above -= 1;
                    }
                    comment.reverse();
                    let comment = comment.join(" ").trim().to_string();
                    let (node_props, node_children) = self.items(false)?;
                    children.push(Node {
                        comment,
                        labels,
                        name,
                        props: node_props,
                        children: node_children,
                        text: self.src[start..self.pos].to_string(),
                    });
                }
                '=' => {
                    let value = self.value();
                    props.push(Prop {
                        name: head,
                        value: Some(value),
                    });
                }
                // `/dts-v1/;`, `/delete-node/ x;` and the like are skipped.
                ';' if head.starts_with('/') => {}
                ';' if !head.is_empty() => props.push(Prop {
                    name: head,
                    value: None,
                }),
                ';' => {}
                _ => return Err(DtsError::Unexpected(line, head)),
            }
        }
    }
}

/// A function-like `#define`.
#[derive(Debug, Clone)]
struct Macro {
    params: Vec<String>,
    /// Whether the parameter list ends in `...`.
    variadic: bool,
    body: String,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// What the preprocessor knows while it reads a file.
#[derive(Default)]
struct Defines {
    /// The file's object-like defines.
    objects: HashMap<String, String>,
    /// The file's function-like macros.
    macros: HashMap<String, Macro>,
    /// Names the ZMK headers the file includes define, with their values.
    /// They decide `#if` tests and are otherwise left as names.
    headers: HashMap<String, String>,
}

impl Defines {
    fn defined(&self, name: &str) -> bool {
        self.objects.contains_key(name)
            || self.macros.contains_key(name)
            || self.headers.contains_key(name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.objects
            .get(name)
            .or_else(|| self.headers.get(name))
            .map(String::as_str)
    }

    /// Learns what a vendored header defines.
    fn include(&mut self, header: &str) {
        for line in strip_comments(header).lines() {
            let Some(rest) = line.trim_start().strip_prefix("#define ") else {
                continue;
            };
            let rest = rest.trim_start();
            let end = rest.find(|c| !is_word(c)).unwrap_or(rest.len());
            let (name, value) = rest.split_at(end);
            if !name.is_empty() {
                let value = if value.starts_with('(') { "" } else { value };
                self.headers
                    .insert(name.to_string(), value.trim().to_string());
            }
        }
    }
}

/// Evaluates the condition of an `#if` or `#elif`.
struct Condition<'a> {
    tokens: Vec<String>,
    pos: usize,
    defines: &'a Defines,
    /// Names tested with `defined` that nothing defines.
    undefined: &'a mut Vec<String>,
    depth: usize,
}

impl<'a> Condition<'a> {
    fn new(text: &str, defines: &'a Defines, undefined: &'a mut Vec<String>, depth: usize) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let (mut tokens, mut i) = (Vec::new(), 0);
        while i < chars.len() {
            let c = chars[i];
            if c.is_whitespace() {
                i += 1;
            } else if is_word(c) {
                let start = i;
                while i < chars.len() && is_word(chars[i]) {
                    i += 1;
                }
                tokens.push(chars[start..i].iter().collect());
            } else if c == '\'' && i + 2 < chars.len() && chars[i + 2] == '\'' {
                tokens.push((chars[i + 1] as u32).to_string());
                i += 3;
            } else {
                let pair: String = chars[i..chars.len().min(i + 2)].iter().collect();
                if ["&&", "||", "==", "!=", "<=", ">="].contains(&pair.as_str()) {
                    tokens.push(pair);
                    i += 2;
                } else {
                    tokens.push(c.to_string());
                    i += 1;
                }
            }
        }
        Self {
            tokens,
            pos: 0,
            defines,
            undefined,
            depth,
        }
    }

    fn peek(&self) -> &str {
        self.tokens.get(self.pos).map_or("", String::as_str)
    }

    fn next(&mut self) -> String {
        let token = self.peek().to_string();
        self.pos += 1;
        token
    }

    /// Reads operators of one precedence level, lowest first.
    fn binary(&mut self, level: usize) -> i64 {
        const LEVELS: [&[&str]; 6] = [
            &["||"],
            &["&&"],
            &["==", "!="],
            &["<", ">", "<=", ">="],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        let Some(operators) = LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1);
        while operators.contains(&self.peek()) {
            let operator = self.next();
            let right = self.binary(level + 1);
            left = match operator.as_str() {
                "||" => i64::from(left != 0 || right != 0),
                "&&" => i64::from(left != 0 && right != 0),
                "==" => i64::from(left == right),
                "!=" => i64::from(left != right),
                "<" => i64::from(left < right),
                ">" => i64::from(left > right),
                "<=" => i64::from(left <= right),
                ">=" => i64::from(left >= right),
                "+" => left.wrapping_add(right),
                "-" => left.wrapping_sub(right),
                "*" => left.wrapping_mul(right),
                "/" => left.checked_div(right).unwrap_or(0),
                _ => left.checked_rem(right).unwrap_or(0),
            };
        }
        left
    }

    fn unary(&mut self) -> i64 {
        match self.next().as_str() {
            "!" => i64::from(self.unary() == 0),
            "-" => self.unary().wrapping_neg(),
            "+" => self.unary(),
            "(" => {
                let value = self.binary(0);
                self.next();
                value
            }
            "defined" => {
                let bracketed = self.peek() == "(";
                if bracketed {
                    self.next();
                }
                let name = self.next();
                if bracketed {
                    self.next();
                }
                let defined = self.defines.defined(&name);
                if !defined {
                    self.undefined.push(name);
                }
                i64::from(defined)
            }
            token => {
                let digits = token.trim_end_matches(['u', 'U', 'l', 'L']);
                let number = match digits.strip_prefix("0x").or(digits.strip_prefix("0X")) {
                    Some(hex) => i64::from_str_radix(hex, 16).ok(),
                    None => digits.parse().ok(),
                };
                match (number, self.defines.value(token)) {
                    (Some(number), _) => number,
                    // A name stands for its value; an unknown name is zero.
                    (None, Some(value)) if self.depth < 8 => {
                        Condition::new(value, self.defines, self.undefined, self.depth + 1)
                            .binary(0)
                    }
                    _ => 0,
                }
            }
        }
    }
}

/// One `#if` ... `#endif` block being read.
struct Branch {
    /// Whether the text around the block is being read.
    parent: bool,
    /// Whether an earlier branch of the block was the one taken.
    taken: bool,
    active: bool,
}

/// Replaces a macro's parameters in its body, then expands the result.
fn substitute(
    name: &str,
    mac: &Macro,
    args: &[String],
    defines: &Defines,
    depth: usize,
    line: usize,
) -> Result<String, DtsError> {
    enum Token {
        Word(String),
        Space(String),
        Paste,
        Hash,
        Other(String),
    }
    let chars: Vec<char> = mac.body.chars().collect();
    let (mut tokens, mut i) = (Vec::new(), 0);
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if is_word(c) {
            while i < chars.len() && is_word(chars[i]) {
                i += 1;
            }
            tokens.push(Token::Word(chars[start..i].iter().collect()));
        } else if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            tokens.push(Token::Space(chars[start..i].iter().collect()));
        } else if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            i = (i + 1).min(chars.len());
            tokens.push(Token::Other(chars[start..i].iter().collect()));
        } else if c == '#' && chars.get(i + 1) == Some(&'#') {
            tokens.push(Token::Paste);
            i += 2;
        } else if c == '#' {
            tokens.push(Token::Hash);
            i += 1;
        } else {
            tokens.push(Token::Other(c.to_string()));
            i += 1;
        }
    }

    let rest = args.get(mac.params.len()..).unwrap_or_default().join(", ");
    let argument = |word: &str| -> Option<&str> {
        if mac.variadic && word == "__VA_ARGS__" {
            return Some(&rest);
        }
        let index = mac.params.iter().position(|p| p == word)?;
        args.get(index).map(String::as_str)
    };
    let mut out = String::new();
    let mut pasting = false;
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Paste => {
                out.truncate(out.trim_end().len());
                pasting = true;
            }
            Token::Space(_) if pasting => {}
            Token::Space(space) => out.push_str(space),
            Token::Hash => match tokens.get(index + 1) {
                Some(Token::Word(word)) if argument(word).is_some() => {
                    out.push_str(&format!("\"{}\"", argument(word).unwrap_or_default()));
                    index += 1;
                }
                _ => out.push('#'),
            },
            Token::Word(word) => {
                let pasted_next = tokens[index + 1..]
                    .iter()
                    .find(|t| !matches!(t, Token::Space(_)))
                    .is_some_and(|t| matches!(t, Token::Paste));
                match argument(word) {
                    // An argument joined to its neighbor is used as written.
                    Some(raw) if pasting || pasted_next => out.push_str(raw),
                    Some(raw) => out.push_str(&expand_all(raw, defines, depth + 1, line)?),
                    None => out.push_str(word),
                }
                pasting = false;
            }
            Token::Other(text) => {
                out.push_str(text);
                pasting = false;
            }
        }
        index += 1;
    }
    expand_all(&out, defines, depth + 1, line).map_err(|error| match error {
        // Name the outermost macro the user wrote.
        DtsError::UnclosedCall(line, _) if depth == 0 => {
            DtsError::UnclosedCall(line, name.to_string())
        }
        other => other,
    })
}

/// Expands everything in text that came out of a macro: object-like
/// defines and uses of function-like macros.
fn expand_all(
    text: &str,
    defines: &Defines,
    depth: usize,
    line: usize,
) -> Result<String, DtsError> {
    expand_calls(&expand(text, &defines.objects), defines, depth, line, false)
}

/// Expands every use of a function-like macro in `text`. Other text,
/// object-like defines included, is left as written. With `keep_lines`,
/// a use that spans lines leaves as many line breaks behind.
fn expand_calls(
    text: &str,
    defines: &Defines,
    depth: usize,
    first_line: usize,
    keep_lines: bool,
) -> Result<String, DtsError> {
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_string) = (0, false);
    while let Some(c) = text[i..].chars().next() {
        if c == '"' {
            in_string = !in_string;
        }
        if in_string || !is_word(c) {
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let start = i;
        i += text[i..].find(|c| !is_word(c)).unwrap_or(text.len() - i);
        let word = &text[start..i];
        let after = &text[i..];
        let gap = after.len() - after.trim_start().len();
        let mac = match defines.macros.get(word) {
            Some(mac) if after[gap..].starts_with('(') && depth < 32 => mac,
            _ => {
                out.push_str(word);
                continue;
            }
        };
        let line = if keep_lines {
            first_line + text[..start].matches('\n').count()
        } else {
            first_line
        };
        let args_start = i + gap + 1;
        let mut args = vec![String::new()];
        let (mut nesting, mut quoted, mut end) = (1, false, None);
        for (offset, c) in text[args_start..].char_indices() {
            match c {
                '"' => quoted = !quoted,
                '(' if !quoted => nesting += 1,
                ')' if !quoted => {
                    nesting -= 1;
                    if nesting == 0 {
                        end = Some(args_start + offset);
                        break;
                    }
                }
                ',' if !quoted && nesting == 1 => {
                    args.push(String::new());
                    continue;
                }
                _ => {}
            }
            if let Some(arg) = args.last_mut() {
                arg.push(c);
            }
        }
        let end = end.ok_or_else(|| DtsError::UnclosedCall(line, word.to_string()))?;
        let mut args: Vec<String> = args.iter().map(|a| a.trim().to_string()).collect();
        if mac.params.is_empty() && args == [""] {
            args.clear();
        }
        let wanted = mac.params.len();
        if args.len() < wanted || (args.len() > wanted && !mac.variadic) {
            return Err(DtsError::Arguments(
                line,
                word.to_string(),
                wanted,
                args.len(),
            ));
        }
        let expansion = substitute(word, mac, &args, defines, depth, line)?;
        out.push_str(&expansion.replace('\n', " "));
        if keep_lines {
            out.push_str(&"\n".repeat(text[start..end].matches('\n').count()));
        }
        i = end + 1;
    }
    Ok(out)
}

pub fn parse(src: &str) -> Result<Source, DtsError> {
    let mut source = Source::default();
    let mut defines = Defines::default();
    let mut branches: Vec<Branch> = Vec::new();
    // Names tested and found undefined, and every name the file defines
    // anywhere, which covers include guards and `#ifndef` defaults.
    let (mut undefined, mut ever_defined) = (Vec::new(), Vec::new());
    let mut body = String::new();

    // Own-line `//` comments, which describe the node below them.
    let comments: HashMap<usize, String> = src
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let text = line.trim().strip_prefix("//")?;
            Some((index + 1, text.trim_start_matches('/').trim().to_string()))
        })
        .collect();

    let stripped = strip_comments(src);
    let mut lines = stripped.lines().enumerate();
    while let Some((index, first)) = lines.next() {
        // A line ending in `\` continues on the next. The line breaks are
        // put back after it so that line numbers stay right.
        let mut line = first.to_string();
        let mut continued = 0;
        while line.ends_with('\\') {
            line.pop();
            line.push(' ');
            let Some((_, next)) = lines.next() else {
                break;
            };
            line.push_str(next);
            continued += 1;
        }
        let number = index + 1;
        let active = branches.last().is_none_or(|b| b.active);
        let trimmed = line.trim_start();
        let directive = trimmed.strip_prefix('#').map(str::trim_start);
        let word = directive
            .and_then(|d| d.split(|c: char| !c.is_ascii_alphabetic()).next())
            .unwrap_or_default();
        let rest = directive.map_or("", |d| d[word.len()..].trim());
        let mut keep = false;
        match word {
            "if" | "ifdef" | "ifndef" => {
                let holds = active
                    && match word {
                        "if" => Condition::new(rest, &defines, &mut undefined, 0).binary(0) != 0,
                        _ => {
                            let defined = defines.defined(rest);
                            if !defined {
                                undefined.push(rest.to_string());
                            }
                            defined == (word == "ifdef")
                        }
                    };
                branches.push(Branch {
                    parent: active,
                    taken: holds,
                    active: holds,
                });
            }
            "elif" | "else" => {
                let unmatched = DtsError::Unmatched(number, word.to_string());
                let (parent, taken) = branches
                    .last()
                    .map(|b| (b.parent, b.taken))
                    .ok_or(unmatched)?;
                let holds = parent
                    && !taken
                    && (word == "else"
                        || Condition::new(rest, &defines, &mut undefined, 0).binary(0) != 0);
                if let Some(branch) = branches.last_mut() {
                    branch.active = holds;
                    branch.taken |= holds;
                }
            }
            "endif" => {
                branches
                    .pop()
                    .ok_or_else(|| DtsError::Unmatched(number, word.to_string()))?;
            }
            "define" => {
                let end = rest.find(|c| !is_word(c)).unwrap_or(rest.len());
                let (name, value) = rest.split_at(end);
                ever_defined.push(name.to_string());
                if !active || name.is_empty() {
                } else if let Some(list) = value.strip_prefix('(') {
                    let (params, body) = list.split_once(')').unwrap_or((list, ""));
                    let mut params: Vec<String> = params
                        .split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect();
                    let variadic = params.last().is_some_and(|p| p == "...");
                    if variadic {
                        params.pop();
                    }
                    if !source.macros.iter().any(|m| m == name) {
                        source.macros.push(name.to_string());
                    }
                    defines.objects.remove(name);
                    defines.macros.insert(
                        name.to_string(),
                        Macro {
                            params,
                            variadic,
                            body: body.trim().to_string(),
                        },
                    );
                } else {
                    defines.macros.remove(name);
                    defines
                        .objects
                        .insert(name.to_string(), value.trim().to_string());
                }
            }
            "undef" if active => {
                defines.objects.remove(rest);
                defines.macros.remove(rest);
            }
            "include" if active => {
                source.includes.push(trimmed.to_string());
                let path = rest.trim_matches(|c| matches!(c, '<' | '>' | '"'));
                if let Some(header) = kc_zmk::headers::header(path) {
                    defines.include(header);
                }
            }
            "undef" | "include" | "pragma" | "error" | "warning" => {}
            // `#binding-cells` and friends are properties, not directives.
            _ => keep = active,
        }
        if keep {
            body.push_str(&line);
        }
        body.push_str(&"\n".repeat(continued + 1));
    }

    if !defines.macros.is_empty() {
        body = expand_calls(&body, &defines, 0, 1, true)?;
    }
    let mut parser = Parser {
        src: &body,
        pos: 0,
        comments: &comments,
    };
    let (_, nodes) = parser.items(true)?;
    source.nodes = nodes;
    source.defines = defines.objects;
    for name in undefined {
        if !ever_defined.contains(&name) && !source.unknown_tests.contains(&name) {
            source.unknown_tests.push(name);
        }
    }
    Ok(source)
}

/// Replaces every identifier that is an object-like `#define` with its
/// value, repeatedly, so that aliases of aliases resolve.
pub fn expand(text: &str, defines: &HashMap<String, String>) -> String {
    let mut current = text.to_string();
    for _ in 0..8 {
        let mut out = String::with_capacity(current.len());
        let mut word = String::new();
        let mut changed = false;
        for c in current.chars().chain([' ']) {
            if c.is_alphanumeric() || c == '_' {
                word.push(c);
                continue;
            }
            match defines.get(&word) {
                Some(value) if !word.is_empty() => {
                    out.push_str(value);
                    changed = true;
                }
                _ => out.push_str(&word),
            }
            word.clear();
            out.push(c);
        }
        out.pop();
        current = out;
        if !changed {
            break;
        }
    }
    current
}

/// The contents of each `<...>` group in a property value.
pub fn cell_groups(value: &str) -> Vec<String> {
    let mut groups = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (index, c) in value.char_indices() {
        match c {
            '<' => {
                if depth == 0 {
                    start = index + 1;
                }
                depth += 1;
            }
            '>' => {
                depth -= 1;
                if depth == 0 {
                    groups.push(value[start..index].trim().to_string());
                }
            }
            _ => {}
        }
    }
    groups
}

/// Whitespace-separated tokens, keeping `NAME(a, b)` and `(A | B)` whole.
pub fn tokens(cells: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut current, mut depth) = (String::new(), 0usize);
    for c in cells.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else if !c.is_whitespace() {
            current.push(c);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Every token in a property value's `<...>` groups.
pub fn tokens_in(value: &str) -> Vec<String> {
    cell_groups(value).iter().flat_map(|g| tokens(g)).collect()
}

/// Splits a flat run of cells into bindings, each starting at a `&`.
pub fn bindings(cells: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in tokens(cells) {
        match out.last_mut() {
            Some(last) if !token.starts_with('&') => {
                last.push(' ');
                last.push_str(&token);
            }
            _ => out.push(token),
        }
    }
    out
}

/// The numbers in a property such as `<40 43>`.
pub fn numbers(value: &str) -> Vec<i64> {
    cell_groups(value)
        .iter()
        .flat_map(|g| tokens(g))
        .filter_map(|t| {
            let t = t.trim_matches(|c| c == '(' || c == ')');
            match t.strip_prefix("0x") {
                Some(hex) => i64::from_str_radix(hex, 16).ok(),
                None => t.parse().ok(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
/* A keymap. */
#include <behaviors.dtsi>
#define LAYER_Nav 1
#define HYPER LC(LS(LG(LALT)))
#define ROW(a, b) a b
#ifdef SOMETHING
#endif

/ {
    chosen { zmk,physical-layout = &layout_a; };
    behaviors {
        hm: homerow_mods {
            compatible = "zmk,behavior-hold-tap";
            #binding-cells = <2>;
            flavor = "balanced"; // why not
            retro-tap;
            bindings = <&kp>, <&kp>;
        };
    };
    keymap {
        compatible = "zmk,keymap";
        base {
            display-name = "Base; with {braces}";
            bindings = <&kp A &mo LAYER_Nav
                        &kp HYPER &rgb_ug RGB_COLOR_HSB(1, 2, 3)>;
        };
    };
};

&listener {
    input-processors = <&zip_xy_scaler 1 3>, <&zip_xy_to_scroll_mapper>;
};
"#;

    #[test]
    fn reads_nodes_labels_properties_and_directives() {
        let source = parse(SRC).unwrap();
        assert_eq!(source.nodes.len(), 2);
        assert_eq!(source.defines["LAYER_Nav"], "1");
        assert_eq!(source.macros, ["ROW"]);
        assert_eq!(source.includes, ["#include <behaviors.dtsi>"]);
        assert_eq!(source.unknown_tests, ["SOMETHING"]);

        let root = &source.nodes[0];
        assert_eq!(root.name, "/");
        let hold_tap = &root.child("behaviors").unwrap().children[0];
        assert_eq!(
            (hold_tap.label(), hold_tap.name.as_str()),
            (Some("hm"), "homerow_mods")
        );
        assert_eq!(hold_tap.string("compatible"), Some("zmk,behavior-hold-tap"));
        assert_eq!(hold_tap.prop("#binding-cells"), Some("<2>"));
        assert!(hold_tap.has("retro-tap") && !hold_tap.has("lazy"));
        assert!(hold_tap.text.starts_with("hm: homerow_mods {") && hold_tap.text.ends_with("};"));

        let layer = &root.child("keymap").unwrap().children[0];
        assert_eq!(layer.string("display-name"), Some("Base; with {braces}"));
        let cells = expand(
            &cell_groups(layer.prop("bindings").unwrap())[0],
            &source.defines,
        );
        assert_eq!(
            bindings(&cells),
            [
                "&kp A",
                "&mo 1",
                "&kp LC(LS(LG(LALT)))",
                "&rgb_ug RGB_COLOR_HSB(1,2,3)"
            ]
        );

        let listener = &source.nodes[1];
        assert_eq!(listener.name, "&listener");
        assert_eq!(
            cell_groups(listener.prop("input-processors").unwrap()).len(),
            2
        );
        assert_eq!(
            root.child("chosen").unwrap().prop("zmk,physical-layout"),
            Some("&layout_a")
        );
    }

    #[test]
    fn macros_with_arguments_are_expanded() {
        let source = parse(
            r#"
#define CORE_td compatible = "zmk,behavior-tap-dance"; \
    #binding-cells = <0>
#define BEHAVIOR(name, type, ...) \
    name: name { \
        CORE_ ## type; \
        __VA_ARGS__ \
    };
#define TD(name, ...) BEHAVIOR(name, td, __VA_ARGS__)
#define TD_LAYER(name, layer) TD(name, \
    bindings = <&mo layer>, <&to layer>; )
#define NAV 2
#define STR(x) #x
#define PAIR(a, b) <a b>
/ {
    behaviors {
        // Hold, or tap twice to stay.
        TD_LAYER(nav_td,
                 NAV)
        after: after { label = STR(a b); cells = PAIR((1, 2), 3); plain = <PAIR>; };
    };
};
"#,
        )
        .unwrap();
        assert_eq!(source.macros, ["BEHAVIOR", "TD", "TD_LAYER", "STR", "PAIR"]);
        let nodes = &source.nodes[0].children[0].children;
        let td = &nodes[0];
        assert_eq!((td.label(), td.name.as_str()), (Some("nav_td"), "nav_td"));
        assert_eq!(td.string("compatible"), Some("zmk,behavior-tap-dance"));
        assert_eq!(td.prop("#binding-cells"), Some("<0>"));
        assert_eq!(td.prop("bindings"), Some("<&mo 2>, <&to 2>"));
        assert_eq!(td.comment, "Hold, or tap twice to stay.");
        let after = &nodes[1];
        assert_eq!(after.string("label"), Some("a b"));
        assert_eq!(after.prop("cells"), Some("<(1, 2) 3>"));
        // A macro's name without arguments is not a use of it.
        assert_eq!(after.prop("plain"), Some("<PAIR>"));

        // Problems are reported on the line the user wrote.
        assert_eq!(
            parse("#define ONE(a) a\n\n/ { x = <ONE(1, 2)>; };").unwrap_err(),
            DtsError::Arguments(3, "ONE".into(), 1, 2)
        );
        assert_eq!(
            parse("#define ONE(a) a\n/ { x = <ONE(1>; };").unwrap_err(),
            DtsError::UnclosedCall(2, "ONE".into())
        );
        // A macro that uses itself stops rather than looping.
        assert!(parse("#define LOOP(a) LOOP(a)\n/ { x = <LOOP(1)>; };").is_ok());
    }

    #[test]
    fn conditional_blocks_are_evaluated() {
        let source = parse(
            r#"
#include <dt-bindings/zmk/bt.h>
#define KB_GO 2
#define KB KB_GO
#define OS 'M'
#ifndef FALLBACK
#define FALLBACK 7
#endif
/ {
#ifdef BT_DISC_CMD
    from_header;
#else
    not_from_header;
#endif
#if KB == 1
    first;
#elif defined(KB) && (KB >= 2) && OS == 'M' && !defined UNKNOWN
    second;
    #ifdef NOPE
    nested_off;
    #else
    nested_on;
    #endif
#else
    third;
    #define NEVER 1
#endif
#if 0
    #include <dt-bindings/zmk/rgb.h>
    off {
#endif
};
"#,
        )
        .unwrap();
        let flags: Vec<&str> = source.nodes[0]
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(flags, ["from_header", "second", "nested_on"]);
        assert_eq!(source.defines["FALLBACK"], "7");
        assert!(!source.defines.contains_key("NEVER"));
        // Header names decide tests but are not put into bindings.
        assert!(!source.defines.contains_key("BT_DISC_CMD"));
        assert_eq!(source.includes, ["#include <dt-bindings/zmk/bt.h>"]);
        // `FALLBACK` is the file's own; the others nothing defines.
        assert_eq!(source.unknown_tests, ["UNKNOWN", "NOPE"]);

        assert_eq!(
            parse("/ { };\n#endif").unwrap_err(),
            DtsError::Unmatched(2, "endif".into())
        );
        assert_eq!(
            parse("#else").unwrap_err(),
            DtsError::Unmatched(1, "else".into())
        );
    }

    #[test]
    fn numbers_and_errors() {
        assert_eq!(numbers("<40 43>, <0x10>"), [40, 43, 16]);
        assert!(matches!(
            parse("/ { a { b = <1>; };"),
            Err(DtsError::Unclosed(_))
        ));
        assert!(parse("").unwrap().nodes.is_empty());
    }
}
