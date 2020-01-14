#![warn(
    clippy::all,
    clippy::restriction,
    clippy::pedantic,
    clippy::nursery,
    // clippy::cargo
)]
#![deny(missing_debug_implementations)]
#![allow(
    dead_code,
    clippy::pub_enum_variant_names,
    clippy::implicit_return,
    clippy::use_self,
    clippy::missing_docs_in_private_items,
    clippy::todo,
    clippy::dbg_macro,
    clippy::unreachable,
    clippy::wildcard_enum_match_arm,
    clippy::option_expect_used,
    clippy::panic,
    clippy::unused_self,
    clippy::too_many_lines,
    clippy::integer_arithmetic,
    clippy::missing_errors_doc,
    clippy::let_underscore_must_use,
    clippy::module_name_repetitions
)]
// todo! handle erroring on styles at the toplevel
use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::iter::{Iterator, Peekable};
use std::path::Path;

use crate::common::{AtRule, Keyword, Op, Pos, Scope, Symbol, Whitespace};
use crate::css::Css;
use crate::error::SassError;
use crate::format::PrettyPrinter;
use crate::lexer::Lexer;
use crate::mixin::{CallArgs, FuncArgs, Mixin};
use crate::selector::{Attribute, Selector};
use crate::style::Style;
use crate::units::Unit;
use crate::utils::{devour_whitespace, IsWhitespace};

mod color;
mod common;
mod css;
mod error;
mod format;
mod imports;
mod lexer;
mod mixin;
mod selector;
mod style;
mod units;
mod utils;

type SassResult<T> = Result<T, SassError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pos: Pos,
    pub kind: TokenKind,
}

impl IsWhitespace for Token {
    fn is_whitespace(&self) -> bool {
        if let TokenKind::Whitespace(_) = self.kind {
            return true;
        }
        false
    }
}

impl IsWhitespace for &Token {
    fn is_whitespace(&self) -> bool {
        if let TokenKind::Whitespace(_) = self.kind {
            return true;
        }
        false
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Symbol(Symbol),
    AtRule(AtRule),
    Keyword(Keyword),
    Number(String),
    Unit(Unit),
    Whitespace(Whitespace),
    Variable(String),
    Attribute(Attribute),
    Op(Op),
    MultilineComment(String),
    Interpolation,
}

impl Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Ident(s) | TokenKind::Number(s) => write!(f, "{}", s),
            TokenKind::Symbol(s) => write!(f, "{}", s),
            TokenKind::AtRule(s) => write!(f, "{}", s),
            TokenKind::Op(s) => write!(f, "{}", s),
            TokenKind::Unit(s) => write!(f, "{}", s),
            TokenKind::Whitespace(s) => write!(f, "{}", s),
            TokenKind::Attribute(s) => write!(f, "{}", s),
            TokenKind::Keyword(kw) => write!(f, "{}", kw),
            TokenKind::MultilineComment(s) => write!(f, "/*{}*/", s),
            TokenKind::Variable(s) => write!(f, "${}", s),
            TokenKind::Interpolation => {
                panic!("we don't want to format TokenKind::Interpolation using Display")
            }
        }
    }
}

/// Represents a parsed SASS stylesheet with nesting
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StyleSheet {
    rules: Vec<Stmt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Stmt {
    /// A [`Style`](/grass/style/struct.Style)
    Style(Style),
    /// A  [`RuleSet`](/grass/struct.RuleSet.html)
    RuleSet(RuleSet),
    /// A multiline comment: `/* foo bar */`
    MultilineComment(String),
}

/// Represents a single rule set. Rule sets can contain other rule sets
///
/// ```scss
/// a {
///   color: blue;
///   b {
///     color: red;
///   }
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleSet {
    selector: Selector,
    rules: Vec<Stmt>,
    // potential optimization: we don't *need* to own the selector
    super_selector: Selector,
}

/// An intermediate representation of what are essentially single lines
/// todo! rename this
#[derive(Clone, Debug, Eq, PartialEq)]
enum Expr {
    /// A style: `color: red`
    Style(Style),
    /// A collection of styles, from a mixin or function
    Styles(Vec<Style>),
    /// A full selector `a > h1`
    Selector(Selector),
    /// A variable declaration `$var: 1px`
    VariableDecl(String, Vec<Token>),
    /// A multiline comment: `/* foobar */`
    MultilineComment(String),
    // /// Function call: `calc(10vw - 1px)`
    // FuncCall(String, Vec<Token>),
}

impl StyleSheet {
    pub fn new(input: &str) -> SassResult<StyleSheet> {
        StyleSheetParser {
            global_scope: Scope::new(),
            lexer: Lexer::new(input).peekable(),
            rules: Vec::new(),
            scope: 0,
            file: String::from("stdin"),
        }
        .parse_toplevel()
    }

    pub fn from_path<P: AsRef<Path> + Into<String>>(p: P) -> SassResult<StyleSheet> {
        StyleSheetParser {
            global_scope: Scope::new(),
            lexer: Lexer::new(&fs::read_to_string(p.as_ref())?).peekable(),
            rules: Vec::new(),
            scope: 0,
            file: p.into(),
        }
        .parse_toplevel()
    }

    /// Print the internal representation of a parsed stylesheet
    ///
    /// Very closely resembles the origin SASS, but contains only things translatable
    /// to pure CSS
    ///
    /// Used mainly in debugging, but can at times be useful
    pub fn pretty_print<W: std::io::Write>(&self, buf: W) -> io::Result<()> {
        PrettyPrinter::new(buf).pretty_print(self)
    }

    fn pretty_print_selectors<W: std::io::Write>(&self, buf: W) -> io::Result<()> {
        PrettyPrinter::new(buf).pretty_print_preserve_super_selectors(self)
    }

    /// Write the internal representation as CSS to `buf`
    pub fn print_as_css<W: std::io::Write>(self, buf: &mut W) -> io::Result<()> {
        Css::from_stylesheet(self).pretty_print(buf)
    }
}

#[derive(Debug, Clone)]
struct StyleSheetParser<'a> {
    global_scope: Scope,
    lexer: Peekable<Lexer<'a>>,
    rules: Vec<Stmt>,
    scope: u32,
    file: String,
}

impl<'a> StyleSheetParser<'a> {
    fn parse_toplevel(mut self) -> SassResult<StyleSheet> {
        let mut rules = Vec::new();
        while let Some(Token { kind, .. }) = self.lexer.peek() {
            match kind.clone() {
                TokenKind::Ident(_)
                | TokenKind::Attribute(_)
                | TokenKind::Interpolation
                | TokenKind::Symbol(Symbol::Hash)
                | TokenKind::Symbol(Symbol::Colon)
                | TokenKind::Symbol(Symbol::Mul)
                | TokenKind::Symbol(Symbol::Period) => rules
                    .extend(self.eat_rules(&Selector(Vec::new()), &mut self.global_scope.clone())),
                TokenKind::Whitespace(_) => {
                    self.lexer.next();
                    continue;
                }
                TokenKind::Variable(name) => {
                    let Token { pos, .. } = self
                        .lexer
                        .next()
                        .expect("this cannot occur as we have already peeked");
                    devour_whitespace(&mut self.lexer);
                    if self
                        .lexer
                        .next()
                        .unwrap_or_else(|| self.error(pos, "expected value after variable"))
                        .kind
                        != TokenKind::Symbol(Symbol::Colon)
                    {
                        self.error(pos, "unexpected variable use at toplevel");
                    }
                    let val = self.eat_variable_value();
                    self.global_scope.vars.insert(name, val);
                }
                TokenKind::MultilineComment(comment) => {
                    self.lexer.next();
                    rules.push(Stmt::MultilineComment(comment));
                }
                TokenKind::AtRule(_) => {
                    self.eat_at_rule();
                }
                _ => {
                    if let Some(Token { pos, .. }) = self.lexer.next() {
                        self.error(pos.clone(), "unexpected toplevel token")
                    } else {
                        unsafe { std::hint::unreachable_unchecked() }
                    }
                }
            };
        }
        Ok(StyleSheet { rules })
    }

    fn eat_mixin(&mut self) {
        let Token { pos, .. } = self.lexer.next().unwrap();
        devour_whitespace(&mut self.lexer);
        let name = if let Some(Token {
            kind: TokenKind::Ident(s),
            ..
        }) = self.lexer.next()
        {
            s
        } else {
            self.error(pos, "expected identifier after mixin declaration")
        };
        devour_whitespace(&mut self.lexer);
        let args = match self.lexer.next() {
            Some(Token {
                kind: TokenKind::Symbol(Symbol::OpenParen),
                ..
            }) => self.eat_func_args(),
            Some(Token {
                kind: TokenKind::Symbol(Symbol::OpenCurlyBrace),
                ..
            }) => FuncArgs::new(),
            _ => self.error(pos, "expected `(` or `{`"),
        };

        let body = self
            .lexer
            .by_ref()
            .take_while(|x| x.kind != TokenKind::Symbol(Symbol::CloseCurlyBrace))
            .collect();

        self.global_scope
            .mixins
            .insert(name, Mixin::new(self.global_scope.clone(), args, body));
    }

    fn eat_func_args(&mut self) -> FuncArgs {
        todo!()
    }

    fn eat_at_rule(&mut self) -> Option<Expr> {
        if let Some(Token {
            kind: TokenKind::AtRule(ref rule),
            pos,
        }) = self.lexer.next()
        {
            match rule {
                AtRule::Error => {
                    devour_whitespace(&mut self.lexer);
                    let message = self
                        .lexer
                        .by_ref()
                        .take_while(|x| x.kind != TokenKind::Symbol(Symbol::SemiColon))
                        .map(|x| x.kind.to_string())
                        .collect::<String>();
                    self.error(pos, &message);
                }
                AtRule::Warn => {
                    devour_whitespace(&mut self.lexer);
                    let message = self
                        .lexer
                        .by_ref()
                        .take_while(|x| x.kind != TokenKind::Symbol(Symbol::SemiColon))
                        .map(|x| x.kind.to_string())
                        .collect::<String>();
                    self.warn(pos, &message);
                }
                AtRule::Debug => {
                    devour_whitespace(&mut self.lexer);
                    let message = self
                        .lexer
                        .by_ref()
                        .take_while(|x| x.kind != TokenKind::Symbol(Symbol::SemiColon))
                        .map(|x| x.kind.to_string())
                        .collect::<String>();
                    self.debug(pos, &message);
                }
                AtRule::Mixin => self.eat_mixin(),
                AtRule::Include => return Some(self.eat_include()),
                _ => todo!("encountered unimplemented at rule"),
            }
        }
        None
    }

    fn eat_include(&mut self) -> Expr {
        devour_whitespace(&mut self.lexer);
        let Token { kind, pos } = self.lexer.next().unwrap();
        let name = if let TokenKind::Ident(s) = kind {
            s
        } else {
            self.error(pos, "expected identifier")
        };

        devour_whitespace(&mut self.lexer);

        match self.lexer.next() {
            Some(Token {
                kind: TokenKind::Symbol(Symbol::SemiColon),
                ..
            }) => {}
            Some(Token {
                kind: TokenKind::Symbol(Symbol::OpenParen),
                ..
            }) => {}
            Some(Token { pos, .. }) => self.error(pos, "expected `(` or `;`"),
            None => self.error(pos, "unexpected EOF"),
        }

        let mut mixin = if let Some(m) = self.global_scope.mixins.get(&name) {
            m.clone()
        } else {
            self.error(pos, "expected identifier")
        };
        let styles = mixin.eval();
        devour_whitespace(&mut self.lexer);
        Expr::Styles(styles)
    }

    fn eat_variable_value(&mut self) -> Vec<Token> {
        devour_whitespace(&mut self.lexer);
        let iter1 = self
            .lexer
            .by_ref()
            .take_while(|x| x.kind != TokenKind::Symbol(Symbol::SemiColon))
            .collect::<Vec<Token>>();
        let mut iter2 = Vec::with_capacity(iter1.len());
        for tok in iter1 {
            if let Token {
                kind: TokenKind::Variable(ref name),
                pos,
            } = tok
            {
                iter2.extend(
                    self.global_scope
                        .vars
                        .get(name)
                        .unwrap_or_else(|| self.error(pos, "Undefined variable"))
                        .clone(),
                );
            } else {
                iter2.push(tok);
            }
        }
        iter2
    }

    fn eat_func_call(&mut self) {}

    fn eat_rules(&mut self, super_selector: &Selector, scope: &mut Scope) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        while let Ok(tok) = self.eat_expr(scope, super_selector) {
            match tok {
                Expr::Style(s) => stmts.push(Stmt::Style(s)),
                Expr::Styles(s) => stmts.extend(s.iter().map(|s| Stmt::Style(s.clone()))),
                Expr::Selector(s) => {
                    self.scope += 1;
                    let rules = self.eat_rules(&super_selector.clone().zip(s.clone()), scope);
                    stmts.push(Stmt::RuleSet(RuleSet {
                        super_selector: super_selector.clone(),
                        selector: s,
                        rules,
                    }));
                    self.scope -= 1;
                    if self.scope == 0 {
                        return stmts;
                    }
                }
                Expr::VariableDecl(name, val) => {
                    if self.scope == 0 {
                        scope.vars.insert(name.clone(), val.clone());
                        self.global_scope.vars.insert(name, val);
                    } else {
                        scope.vars.insert(name, val);
                    }
                }
                Expr::MultilineComment(s) => stmts.push(Stmt::MultilineComment(s)),
            }
        }
        stmts
    }

    fn eat_expr(&mut self, scope: &Scope, super_selector: &Selector) -> Result<Expr, ()> {
        let mut values = Vec::with_capacity(5);
        while let Some(tok) = self.lexer.peek() {
            match &tok.kind {
                TokenKind::Symbol(Symbol::SemiColon)
                | TokenKind::Symbol(Symbol::CloseCurlyBrace) => {
                    self.lexer.next();
                    devour_whitespace(&mut self.lexer);
                    return Ok(Expr::Style(Style::from_tokens(&values, scope)?));
                }
                TokenKind::Symbol(Symbol::OpenCurlyBrace) => {
                    self.lexer.next();
                    devour_whitespace(&mut self.lexer);
                    return Ok(Expr::Selector(Selector::from_tokens(
                        &mut values.iter().peekable(),
                        super_selector,
                        scope,
                    )));
                }
                TokenKind::Variable(_) => {
                    let tok = self.lexer.next().unwrap();
                    let name = if let TokenKind::Variable(n) = tok.kind {
                        n
                    } else {
                        unsafe { std::hint::unreachable_unchecked() }
                    };
                    if let TokenKind::Symbol(Symbol::Colon) = self
                        .lexer
                        .peek()
                        .expect("expected something after variable")
                        .kind
                    {
                        self.lexer.next();
                        devour_whitespace(&mut self.lexer);
                        return Ok(Expr::VariableDecl(name, self.eat_variable_value()));
                    } else {
                        values.push(Token {
                            kind: TokenKind::Variable(name),
                            pos: tok.pos,
                        });
                    }
                }
                TokenKind::MultilineComment(_) => {
                    let tok = self.lexer.next().unwrap();
                    let s = if let TokenKind::MultilineComment(s) = &tok.kind {
                        s
                    } else {
                        unsafe { std::hint::unreachable_unchecked() }
                    };
                    devour_whitespace(&mut self.lexer);
                    if values.is_empty() {
                        return Ok(Expr::MultilineComment(s.clone()));
                    } else {
                        values.push(tok.clone())
                    }
                }
                TokenKind::AtRule(_) => {
                    if let Some(a) = self.eat_at_rule() {
                        return Ok(a);
                    }
                }
                TokenKind::Interpolation => {
                    while let Some(tok) = self.lexer.next() {
                        if tok.kind == TokenKind::Symbol(Symbol::CloseCurlyBrace) {
                            values.push(tok);
                            break;
                        }
                        values.push(tok);
                    }
                }
                _ => {
                    if let Some(tok) = self.lexer.next() {
                        values.push(tok.clone())
                    } else {
                        unsafe { std::hint::unreachable_unchecked() }
                    }
                }
            };
        }
        Err(())
    }
}

/// Functions that print to stdout or stderr
impl<'a> StyleSheetParser<'a> {
    fn debug(&self, pos: Pos, message: &str) {
        println!("{}:{} Debug: {}", self.file, pos.line(), message);
    }

    fn warn(&self, pos: Pos, message: &str) {
        eprintln!(
            "Warning: {}\n\t{} {}:{} todo!(scope)",
            message,
            self.file,
            pos.line(),
            pos.column()
        );
    }

    fn error(&self, pos: Pos, message: &str) -> ! {
        eprintln!("Error: {}", message);
        eprintln!(
            "{} {}:{} todo!(scope) on line {} at column {}",
            self.file,
            pos.line(),
            pos.column(),
            pos.line(),
            pos.column()
        );
        let padding = vec![' '; format!("{}", pos.line()).len() + 1]
            .iter()
            .collect::<String>();
        eprintln!("{}|", padding);
        eprint!("{} | ", pos.line());
        eprintln!("todo! get line to print as error");
        eprintln!(
            "{}| {}^",
            padding,
            vec![' '; pos.column() as usize].iter().collect::<String>()
        );
        eprintln!("{}|", padding);
        std::process::exit(1);
    }
}

fn main() -> SassResult<()> {
    let mut stdout = std::io::BufWriter::new(std::io::stdout());
    let s = StyleSheet::from_path("input.scss")?;
    // dbg!(s);
    s.pretty_print(&mut stdout)?;
    // s.pretty_print_selectors(&mut stdout)?;
    // s.print_as_css(&mut stdout)?;
    // dbg!(Css::from_stylesheet(s));
    // println!("{}", s);
    // drop(input);
    Ok(())
}

#[cfg(test)]
mod test_css {
    use super::StyleSheet;
    macro_rules! test {
        ($func:ident, $input:literal) => {
            #[test]
            fn $func() {
                let mut buf = Vec::new();
                StyleSheet::new($input)
                    .expect(concat!("failed to parse on ", $input))
                    .print_as_css(&mut buf)
                    .expect(concat!("failed to pretty print on ", $input));
                assert_eq!(
                    String::from($input),
                    String::from_utf8(buf).expect("produced invalid utf8")
                );
            }
        };
        ($func:ident, $input:literal, $output:literal) => {
            #[test]
            fn $func() {
                let mut buf = Vec::new();
                StyleSheet::new($input)
                    .expect(concat!("failed to parse on ", $input))
                    .print_as_css(&mut buf)
                    .expect(concat!("failed to pretty print on ", $input));
                assert_eq!(
                    String::from($output),
                    String::from_utf8(buf).expect("produced invalid utf8")
                );
            }
        };
    }

    test!(
        selector_nesting_el_mul_el,
        "a, b {\n  a, b {\n  color: red\n}\n}\n",
        "a a, a b, b a, b b {\n  color: red;\n}\n"
    );
    test!(selector_element, "a {\n  color: red;\n}\n");
    test!(selector_id, "#id {\n  color: red;\n}\n");
    test!(selector_class, ".class {\n  color: red;\n}\n");
    test!(selector_el_descendant, "a a {\n  color: red;\n}\n");
    test!(selector_universal, "* {\n  color: red;\n}\n");
    test!(selector_el_class_and, "a.class {\n  color: red;\n}\n");
    test!(selector_el_id_and, "a#class {\n  color: red;\n}\n");
    test!(
        selector_el_class_descendant,
        "a .class {\n  color: red;\n}\n"
    );
    test!(selector_el_id_descendant, "a #class {\n  color: red;\n}\n");
    test!(
        selector_el_universal_descendant,
        "a * {\n  color: red;\n}\n"
    );
    test!(
        selector_universal_el_descendant,
        "* a {\n  color: red;\n}\n"
    );

    test!(selector_attribute_any, "[attr] {\n  color: red;\n}\n");
    test!(
        selector_attribute_equals,
        "[attr=val] {\n  color: red;\n}\n"
    );
    test!(
        selector_attribute_single_quotes,
        "[attr='val'] {\n  color: red;\n}\n"
    );
    test!(
        selector_attribute_double_quotes,
        "[attr=\"val\"] {\n  color: red;\n}\n"
    );
    test!(selector_attribute_in, "[attr~=val] {\n  color: red;\n}\n");
    test!(
        selector_attribute_begins_hyphen_or_exact,
        "[attr|=val] {\n  color: red;\n}\n"
    );
    test!(
        selector_attribute_starts_with,
        "[attr^=val] {\n  color: red;\n}\n"
    );
    test!(
        selector_attribute_ends_with,
        "[attr$=val] {\n  color: red;\n}\n"
    );
    test!(
        selector_attribute_contains,
        "[attr*=val] {\n  color: red;\n}\n"
    );
    test!(selector_el_attribute_and, "a[attr] {\n  color: red;\n}\n");
    test!(
        selector_el_attribute_descendant,
        "a [attr] {\n  color: red;\n}\n"
    );
    test!(selector_el_mul_el, "a, b {\n  color: red;\n}\n");
    test!(
        selector_el_immediate_child_el,
        "a > b {\n  color: red;\n}\n"
    );
    test!(selector_el_following_el, "a + b {\n  color: red;\n}\n");
    test!(selector_el_preceding_el, "a ~ b {\n  color: red;\n}\n");
    test!(selector_pseudo, ":pseudo {\n  color: red;\n}\n");
    test!(selector_el_and_pseudo, "a:pseudo {\n  color: red;\n}\n");
    test!(
        selector_el_pseudo_descendant,
        "a :pseudo {\n  color: red;\n}\n"
    );
    test!(
        selector_pseudo_el_descendant,
        ":pseudo a {\n  color: red;\n}\n"
    );
    test!(
        selector_pseudo_paren_comma,
        ":pseudo(a, b, c) {\n  color: red;\n}\n"
    );
    test!(
        selector_pseudo_paren_space,
        ":pseudo(a b c) {\n  color: red;\n}\n"
    );
    test!(
        selector_el_pseudo_paren_and,
        "a:pseudo(a, b, c) {\n  color: red;\n}\n"
    );
    test!(
        selector_el_pseudo_paren_descendant,
        "a :pseudo(a, b, c) {\n  color: red;\n}\n"
    );
    test!(
        selector_pseudo_paren_el_descendant,
        ":pseudo(a, b, c) a {\n  color: red;\n}\n"
    );
    test!(
        selector_pseudo_paren_el_nested,
        "a {\n  :pseudo(a, b, c) {\n  color: red;\n  }\n}\n",
        "a :pseudo(a, b, c) {\n  color: red;\n}\n"
    );

    test!(basic_style, "a {\n  color: red;\n}\n");
    test!(two_styles, "a {\n  color: red;\n  color: blue;\n}\n");
    test!(
        two_inner_rulesets,
        "a {\n  b {\n  color: red;\n}\n  c {\n  color: white;\n}\n}\n",
        "a b {\n  color: red;\n}\na c {\n  color: white;\n}\n"
    );
    test!(
        two_rulesets,
        "a {\n  color: red;\n}\nc {\n  color: white;\n}\n"
    );
    test!(
        two_inner_outer_rulesets,
        "a {\n  b {\n  color: red;\n}\n  c {\n  color: white;\n}\n}\na {\n  b {\n  color: red;\n}\n  c {\n  color: white;\n}\n}\n",
        "a b {\n  color: red;\n}\na c {\n  color: white;\n}\na b {\n  color: red;\n}\na c {\n  color: white;\n}\n"
    );
    test!(selector_mul, "a, b {\n  color: red;\n}\n");
    test!(
        removes_empty_outer_styles,
        "a {\n  b {\n    color: red;\n  }\n",
        "a b {\n  color: red;\n}\n"
    );
    test!(removes_empty_styles, "a {}\n", "");
    test!(
        doesnt_eat_style_after_ruleset,
        "a {\n  b {\n  color: red;\n}\n  color: blue;\n}\n",
        "a {\n  color: blue;\n}\na b {\n  color: red;\n}\n"
    );

    test!(
        removes_inner_comments,
        "a {\n  color: red/* hi */;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        removes_inner_comments_whitespace,
        "a {\n  color: red    /* hi */;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        preserves_outer_comments_before,
        "a {\n  /* hi */\n  color: red;\n}\n"
    );
    test!(
        preserves_outer_comments_after,
        "a {\n  color: red;\n  /* hi */\n}\n"
    );
    test!(
        preserves_outer_comments_two,
        "a {\n  /* foo */\n  /* bar */\n  color: red;\n}\n"
    );
    test!(
        preserves_toplevel_comment_before,
        "/* foo */\na {\n  color: red;\n}\n"
    );
    test!(
        preserves_toplevel_comment_after,
        "a {\n  color: red;\n}\n/* foo */\n"
    );
    test!(
        removes_single_line_comment,
        "// a { color: red }\na {\n  height: 1 1px;\n}\n",
        "a {\n  height: 1 1px;\n}\n"
    );

    test!(
        outer_ampersand,
        "a, b {\n& c {\n  color: red;\n}\n}\n",
        "a c, b c {\n  color: red;\n}\n"
    );
    test!(
        inner_ampersand,
        "a, b {\na & c {\n  color: red;\n}\n}\n",
        "a a c, a b c {\n  color: red;\n}\n"
    );
    test!(
        ampersand_multiple_whitespace,
        " a  ,  b   {\n&c {\n  color: red;\n}\n}\n",
        "ac, bc {\n  color: red;\n}\n"
    );
    test!(
        ampersand_alone,
        "a, b {\n& {\n  color: red;\n}\n}\n",
        "a, b {\n  color: red;\n}\n"
    );
    test!(
        bem_dash_dash_selector,
        "a {\n&--b {\n  color: red;\n}\n}\n",
        "a--b {\n  color: red;\n}\n"
    );
    // test!(
    //     bem_underscore_selector,
    //     "a {\n&__b {\n  color: red;\n}\n}\n",
    //     "a__b {\n  color: red;\n}\n"
    // );
    test!(
        selector_interpolation_start,
        "#{a}bc {\n  color: red;\n}\n",
        "abc {\n  color: red;\n}\n"
    );
    test!(
        selector_interpolation_middle,
        "a#{b}c {\n  color: red;\n}\n",
        "abc {\n  color: red;\n}\n"
    );
    test!(
        selector_interpolation_end,
        "ab#{c} {\n  color: red;\n}\n",
        "abc {\n  color: red;\n}\n"
    );
    test!(
        selector_interpolation_variable,
        "$a: foo;\nab#{$a} {\n  color: red;\n}\n",
        "abfoo {\n  color: red;\n}\n"
    );
    test!(
        style_interpolation_start,
        "a {\n  #{c}olor: red;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        style_interpolation_middle,
        "a {\n  co#{l}or: red;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        style_interpolation_end,
        "a {\n  colo#{r}: red;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        style_interpolation_variable,
        "$a: foo;\na {\n  co#{$a}lor: red;\n}\n",
        "a {\n  cofoolor: red;\n}\n"
    );

    test!(
        style_val_interpolation_start,
        "a {\n  color: #{r}ed;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        style_val_interpolation_middle,
        "a {\n  color: r#{e}d;\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        style_val_interpolation_end,
        "a {\n  color: re#{d};\n}\n",
        "a {\n  color: red;\n}\n"
    );
    test!(
        style_val_interpolation_variable,
        "$a: foo;\na {\n  color: r#{$a}ed;\n}\n",
        "a {\n  color: rfooed;\n}\n"
    );
    test!(
        style_whitespace,
        "a {\n     color      :       red    ;    \n}\n",
        "a {\n  color: red;\n}\n"
    );
}
