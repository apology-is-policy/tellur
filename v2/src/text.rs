//! The reader of the core text form.
//!
//! The form is the notation of `docs/v2/core.md`: guarded statements,
//! `selector ▷ command`, with the glyphs of Rails and no sugar. Every glyph
//! has a spelling in ASCII, the one the surface document gives it ([G8])
//! where it has one. `docs/v2/oracle.md` states the grammar.

use crate::int::Int;
use crate::syntax::{
    Alternative, Arm, Binary, BoundaryDecl, Command, Control, Declaration, Expr, Function,
    LoopParameter, OutcomeDecl, Parameter, Place, PlaceStep, Program, ResourceDecl, Selector,
    Statement, StructDecl, Unary, VariantConstructor, VariantDecl,
};
use crate::typed::{BorrowMode, Type};
use std::fmt;

#[derive(Clone, Debug)]
pub struct ParseError {
    pub line: u32,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Name(String),
    Number(String),
    String(String),
    Top,
    Guard,
    Arms,
    Delta,
    Exit,
    Leave,
    Loop,
    Again,
    Test,
    Oblige,
    When,
    Impossible,
    Assign,
    Bind,
    Bar,
    Colon,
    Comma,
    Dot,
    Tilde,
    Amp,
    Move,
    EmptyMap,
    MapArrow,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    LeftAngle,
    RightAngle,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    And,
    Or,
    Not,
    Control,
    Extern,
    Function,
    Pub,
    Uses,
    Faults,
    Fault,
    Assume,
    Struct,
    Variant,
    Outcome,
    Resource,
    Copy,
    Boundary,
    Contingency,
    Retains,
    Supervise,
    Terminate,
    Escalate,
    Recover,
    Capture,
    Contingent,
    Mod,
    True,
    False,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: u32,
    first_on_line: bool,
}

fn word(text: &str) -> Tok {
    match text {
        "top" => Tok::Top,
        "delta" => Tok::Delta,
        "exit" | "exits" => Tok::Exit,
        "leave" => Tok::Leave,
        "loop" => Tok::Loop,
        "again" => Tok::Again,
        "if" => Tok::Test,
        "where" => Tok::Oblige,
        "when" => Tok::When,
        "impossible" => Tok::Impossible,
        "mutable" => Tok::Tilde,
        "ref" => Tok::Amp,
        "move" => Tok::Move,
        "control" => Tok::Control,
        "extern" => Tok::Extern,
        "function" => Tok::Function,
        "pub" => Tok::Pub,
        "uses" => Tok::Uses,
        "faults" => Tok::Faults,
        "fault" => Tok::Fault,
        "assume" => Tok::Assume,
        "struct" => Tok::Struct,
        "variant" => Tok::Variant,
        "outcome" => Tok::Outcome,
        "resource" => Tok::Resource,
        "copy" => Tok::Copy,
        "boundary" => Tok::Boundary,
        "contingency" => Tok::Contingency,
        "retains" => Tok::Retains,
        "supervise" => Tok::Supervise,
        "terminate" => Tok::Terminate,
        "escalate" => Tok::Escalate,
        "recover" => Tok::Recover,
        "capture" => Tok::Capture,
        "contingent" => Tok::Contingent,
        "mod" => Tok::Mod,
        "true" => Tok::True,
        "false" => Tok::False,
        _ => Tok::Name(text.to_string()),
    }
}

fn lex(source: &str) -> Result<Vec<Token>, ParseError> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1u32;
    let mut first_on_line = true;
    while index < characters.len() {
        let character = characters[index];
        let following = characters.get(index + 1).copied();
        if character == '\n' {
            line += 1;
            first_on_line = true;
            index += 1;
            continue;
        }
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        if character == '┄' || (character == '/' && following == Some('/')) {
            while index < characters.len() && characters[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if character == '"' {
            let start_line = line;
            index += 1;
            let mut value = String::new();
            let mut closed = false;
            while index < characters.len() {
                let ch = characters[index];
                if ch == '"' {
                    index += 1;
                    closed = true;
                    break;
                }
                if ch == '\n' {
                    line += 1;
                }
                if ch == '\\' {
                    index += 1;
                    let Some(&esc) = characters.get(index) else {
                        return Err(ParseError {
                            line: start_line,
                            message: "unterminated string escape".to_string(),
                        });
                    };
                    match esc {
                        '"' => value.push('"'),
                        '\\' => value.push('\\'),
                        'n' => value.push('\n'),
                        'r' => value.push('\r'),
                        't' => value.push('\t'),
                        '{' => value.push('{'),
                        '}' => value.push('}'),
                        other => {
                            return Err(ParseError {
                                line,
                                message: format!("unknown escape `\\{other}` in string literal"),
                            });
                        }
                    }
                    index += 1;
                } else {
                    value.push(ch);
                    index += 1;
                }
            }
            if !closed {
                return Err(ParseError {
                    line: start_line,
                    message: "unterminated string literal".to_string(),
                });
            }
            tokens.push(Token {
                tok: Tok::String(value),
                line: start_line,
                first_on_line,
            });
            first_on_line = false;
            continue;
        }
        let start = index;
        let tok = if character.is_ascii_alphabetic() || character == '_' {
            while index < characters.len()
                && (characters[index].is_ascii_alphanumeric() || characters[index] == '_')
            {
                index += 1;
            }
            word(&characters[start..index].iter().collect::<String>())
        } else if character.is_ascii_digit() {
            while index < characters.len() && characters[index].is_ascii_digit() {
                index += 1;
            }
            Tok::Number(characters[start..index].iter().collect())
        } else {
            let pair = |second: char| following == Some(second);
            let (tok, length) = match character {
                '⊤' => (Tok::Top, 1),
                '▷' => (Tok::Guard, 1),
                '▸' => (Tok::Arms, 1),
                'Δ' => (Tok::Delta, 1),
                '■' => (Tok::Exit, 1),
                '□' => (Tok::Leave, 1),
                '↻' => (Tok::Loop, 1),
                '↑' => (Tok::Again, 1),
                '?' => (Tok::Test, 1),
                '⊢' => (Tok::Oblige, 1),
                '⇐' => (Tok::When, 1),
                '⊥' => (Tok::Impossible, 1),
                '←' => (Tok::Assign, 1),
                '│' => (Tok::Bar, 1),
                '≠' => (Tok::NotEqual, 1),
                '≤' => (Tok::LessEqual, 1),
                '≥' => (Tok::GreaterEqual, 1),
                '∧' => (Tok::And, 1),
                '∨' => (Tok::Or, 1),
                '¬' => (Tok::Not, 1),
                '÷' => (Tok::Slash, 1),
                '~' => (Tok::Tilde, 1),
                '▾' => (Tok::Move, 1),
                '∅' => (Tok::EmptyMap, 1),
                '⇀' => (Tok::MapArrow, 1),
                '⟨' => (Tok::LeftAngle, 1),
                '⟩' => (Tok::RightAngle, 1),
                ',' => (Tok::Comma, 1),
                '.' => (Tok::Dot, 1),
                '(' => (Tok::LeftParen, 1),
                ')' => (Tok::RightParen, 1),
                '{' => (Tok::LeftBrace, 1),
                '}' => (Tok::RightBrace, 1),
                '[' => (Tok::LeftBracket, 1),
                ']' => (Tok::RightBracket, 1),
                '+' => (Tok::Plus, 1),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                ':' if pair('=') => (Tok::Bind, 2),
                ':' => (Tok::Colon, 1),
                '|' if pair('>') => (Tok::Guard, 2),
                '|' if pair('|') => (Tok::Or, 2),
                '|' if pair('=') && characters.get(index + 2) == Some(&'>') => (Tok::MapArrow, 3),
                '|' => (Tok::Bar, 1),
                '&' if pair('&') => (Tok::And, 2),
                '&' => (Tok::Amp, 1),
                '>' if pair('>') => (Tok::Arms, 2),
                '>' if pair('=') => (Tok::GreaterEqual, 2),
                '>' => (Tok::Greater, 1),
                '<' if pair('-') => (Tok::Assign, 2),
                '<' if pair('=') => (Tok::LessEqual, 2),
                '<' => (Tok::Less, 1),
                '!' if pair('=') => (Tok::NotEqual, 2),
                '!' => (Tok::Not, 1),
                '=' if pair('=') => (Tok::Equal, 2),
                '=' => (Tok::Equal, 1),
                other => {
                    return Err(ParseError {
                        line,
                        message: format!("the character `{other}` is not part of the core text"),
                    });
                }
            };
            index += length;
            tok
        };
        tokens.push(Token {
            tok,
            line,
            first_on_line,
        });
        first_on_line = false;
    }
    tokens.push(Token {
        tok: Tok::End,
        line,
        first_on_line: true,
    });
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

type Parsed<T> = Result<T, ParseError>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.tokens[self.position].tok
    }

    fn peek_at(&self, offset: usize) -> &Tok {
        self.tokens
            .get(self.position + offset)
            .map(|t| &t.tok)
            .unwrap_or(&Tok::End)
    }

    fn line(&self) -> u32 {
        self.tokens[self.position].line
    }

    fn first_on_line(&self) -> bool {
        self.tokens[self.position].first_on_line
    }

    fn advance(&mut self) -> Tok {
        let tok = self.tokens[self.position].tok.clone();
        if self.position + 1 < self.tokens.len() {
            self.position += 1;
        }
        tok
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error<T>(&self, message: impl Into<String>) -> Parsed<T> {
        Err(ParseError {
            line: self.line(),
            message: message.into(),
        })
    }

    fn expect(&mut self, tok: Tok, what: &str) -> Parsed<()> {
        if self.eat(&tok) {
            Ok(())
        } else {
            self.error(format!("expected {what}, found {}", describe(self.peek())))
        }
    }

    fn name(&mut self, what: &str) -> Parsed<String> {
        match self.peek().clone() {
            Tok::Name(name) => {
                self.advance();
                Ok(name)
            }
            Tok::True if what == "an alternative" => {
                self.advance();
                Ok("true".to_string())
            }
            Tok::False if what == "an alternative" => {
                self.advance();
                Ok("false".to_string())
            }
            // Allow `outcome` as an operation name (`Δ outcome(x)`).
            Tok::Outcome if what == "an operation" => {
                self.advance();
                Ok("outcome".to_string())
            }
            other => self.error(format!("expected {what}, found {}", describe(&other))),
        }
    }

    fn effect_name(&mut self) -> Parsed<String> {
        let mut out = self.name("an effect")?;
        while self.eat(&Tok::Dot) {
            out.push('.');
            out.push_str(&self.name("an effect")?);
        }
        Ok(out)
    }

    fn eat_open_angle(&mut self) -> bool {
        self.eat(&Tok::LeftAngle) || self.eat(&Tok::Less)
    }

    fn expect_close_angle(&mut self) -> Parsed<()> {
        if self.eat(&Tok::RightAngle) || self.eat(&Tok::Greater) {
            Ok(())
        } else {
            self.error(format!(
                "expected `⟩` or `>`, found {}",
                describe(self.peek())
            ))
        }
    }

    fn ty(&mut self) -> Parsed<Type> {
        if self.eat(&Tok::Amp) {
            let mode = if self.eat(&Tok::Tilde) {
                BorrowMode::Exclusive
            } else {
                BorrowMode::Shared
            };
            let inner = self.ty()?;
            return Ok(Type::Ref(mode, Box::new(inner)));
        }
        let left = self.base_ty()?;
        if self.eat(&Tok::MapArrow) {
            let right = self.base_ty()?;
            return Ok(Type::Map(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn base_ty(&mut self) -> Parsed<Type> {
        if self.eat(&Tok::LeftBracket) {
            let elem = self.ty()?;
            self.expect(Tok::RightBracket, "`]`")?;
            return Ok(Type::Vector(Box::new(elem)));
        }
        if self.eat(&Tok::Outcome) {
            if !self.eat_open_angle() {
                return self.error("expected `⟨` or `<` after `outcome`");
            }
            let name = self.name("an outcome or control name")?;
            self.expect_close_angle()?;
            return Ok(Type::OutcomeNamed(name));
        }
        let name = self.name("a type")?;
        if name == "vector" && self.eat_open_angle() {
            let elem = self.ty()?;
            self.expect_close_angle()?;
            return Ok(Type::Vector(Box::new(elem)));
        }
        if name == "map" && self.eat_open_angle() {
            let k = self.ty()?;
            self.expect(Tok::Comma, "`,`")?;
            let v = self.ty()?;
            self.expect_close_angle()?;
            return Ok(Type::Map(Box::new(k), Box::new(v)));
        }
        Ok(Type::from_name(&name).unwrap_or(Type::Named(name)))
    }

    fn field_list(&mut self, open: Tok, close: Tok) -> Parsed<Vec<Parameter>> {
        let mut out = Vec::new();
        self.expect(open, "an opening delimiter")?;
        while !self.eat(&close) {
            if !out.is_empty() {
                self.expect(Tok::Comma, "`,`")?;
            }
            let name = self.name("a field or parameter name")?;
            self.expect(Tok::Colon, "`:`")?;
            out.push(Parameter {
                name,
                ty: self.ty()?,
            });
        }
        Ok(out)
    }

    fn parameters(&mut self) -> Parsed<Vec<Parameter>> {
        self.field_list(Tok::LeftParen, Tok::RightParen)
    }

    fn contract_alternatives(&mut self) -> Parsed<Vec<Alternative>> {
        self.expect(Tok::LeftBrace, "`{` to open the contract")?;
        let mut contract = Vec::new();
        loop {
            let name = self.name("an alternative")?;
            let fields = if self.peek() == &Tok::LeftBrace {
                self.field_list(Tok::LeftBrace, Tok::RightBrace)?
            } else {
                Vec::new()
            };
            let condition = if self.eat(&Tok::When) {
                Some(self.expr()?)
            } else {
                None
            };
            contract.push(Alternative {
                name,
                fields,
                condition,
            });
            if !self.eat(&Tok::Bar) {
                break;
            }
        }
        self.expect(Tok::RightBrace, "`}` to close the contract")?;
        Ok(contract)
    }

    fn is_declaration_start(&self) -> bool {
        matches!(
            self.peek(),
            Tok::End
                | Tok::Function
                | Tok::Pub
                | Tok::Extern
                | Tok::Control
                | Tok::Contingent
                | Tok::Copy
                | Tok::Struct
                | Tok::Variant
                | Tok::Outcome
                | Tok::Boundary
        )
    }

    fn program(&mut self) -> Parsed<Program> {
        let mut declarations = Vec::new();
        loop {
            let line = self.line();
            match self.peek() {
                Tok::End => return Ok(Program { declarations }),
                Tok::Function => {
                    self.advance();
                    declarations.push(Declaration::Function(self.function(line)?));
                }
                Tok::Boundary => {
                    self.advance();
                    declarations.push(Declaration::Boundary(self.boundary_decl(line)?));
                }
                Tok::Pub
                | Tok::Extern
                | Tok::Control
                | Tok::Contingent
                | Tok::Copy
                | Tok::Struct
                | Tok::Variant
                | Tok::Outcome => {
                    let public = self.eat(&Tok::Pub);
                    if self.eat(&Tok::Function) {
                        declarations.push(Declaration::Function(self.function(line)?));
                        continue;
                    }
                    if self.eat(&Tok::Outcome) {
                        declarations.push(Declaration::Outcome(self.outcome_decl(public, line)?));
                        continue;
                    }
                    let is_copy = self.eat(&Tok::Copy);
                    if self.eat(&Tok::Struct) {
                        declarations.push(Declaration::Struct(
                            self.struct_decl(public, is_copy, line)?,
                        ));
                        continue;
                    }
                    if self.eat(&Tok::Variant) {
                        declarations.push(Declaration::Variant(
                            self.variant_decl(public, is_copy, line)?,
                        ));
                        continue;
                    }
                    if is_copy {
                        return self.error("`copy` is only allowed before `struct` or `variant`");
                    }
                    let mut contingent = self.eat(&Tok::Contingent);
                    let external = self.eat(&Tok::Extern);
                    if !contingent {
                        contingent = self.eat(&Tok::Contingent);
                    }
                    if external && !public && !contingent && self.eat(&Tok::Resource) {
                        declarations.push(Declaration::Resource(self.resource_decl(line)?));
                        continue;
                    }
                    declarations.push(Declaration::Control(
                        self.control(public, external, contingent, line)?,
                    ));
                }
                other => {
                    return self
                        .error(format!("expected a declaration, found {}", describe(other)));
                }
            }
        }
    }

    fn struct_decl(&mut self, public: bool, is_copy: bool, line: u32) -> Parsed<StructDecl> {
        let name = self.name("a struct name")?;
        let fields = self.field_list(Tok::LeftBrace, Tok::RightBrace)?;
        Ok(StructDecl {
            name,
            public,
            is_copy,
            fields,
            line,
        })
    }

    fn variant_decl(&mut self, public: bool, is_copy: bool, line: u32) -> Parsed<VariantDecl> {
        let name = self.name("a variant name")?;
        self.expect(Tok::LeftBrace, "`{` to open the variant")?;
        let mut constructors = Vec::new();
        while !self.eat(&Tok::RightBrace) {
            if !constructors.is_empty() && !self.eat(&Tok::Bar) {
                self.expect(Tok::Comma, "`│` or `,` between variant constructors")?;
            }
            let ctor_name = self.name("a constructor name")?;
            let fields = if self.peek() == &Tok::LeftBrace {
                self.field_list(Tok::LeftBrace, Tok::RightBrace)?
            } else {
                Vec::new()
            };
            constructors.push(VariantConstructor {
                name: ctor_name,
                fields,
            });
        }
        Ok(VariantDecl {
            name,
            public,
            is_copy,
            constructors,
            line,
        })
    }

    fn outcome_decl(&mut self, public: bool, line: u32) -> Parsed<OutcomeDecl> {
        let name = self.name("an outcome name")?;
        let alternatives = self.contract_alternatives()?;
        Ok(OutcomeDecl {
            name,
            public,
            alternatives,
            line,
        })
    }

    fn resource_decl(&mut self, line: u32) -> Parsed<ResourceDecl> {
        let name = self.name("a resource name")?;
        let on_fault_release = if matches!(self.peek(), Tok::Name(w) if w == "on") {
            self.advance();
            self.expect(Tok::Fault, "`fault` after `on`")?;
            let rel = self.name("`release`")?;
            if rel != "release" {
                return self.error("expected `release` after `on fault`");
            }
            true
        } else {
            false
        };
        Ok(ResourceDecl {
            name,
            on_fault_release,
            line,
        })
    }

    fn boundary_decl(&mut self, line: u32) -> Parsed<BoundaryDecl> {
        let name = self.name("a boundary name")?;
        let parameters = self.parameters()?;
        let mut retains = Vec::new();
        if self.eat(&Tok::Retains) {
            loop {
                retains.push(self.name("a retained parameter name")?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::LeftBrace, "`{` to open the boundary body")?;
        let body = self.statements(true)?;
        self.expect(Tok::RightBrace, "`}` to close the boundary body")?;
        self.expect(Tok::Contingency, "`contingency`")?;
        self.expect(Tok::LeftParen, "`(`")?;
        let message = self.name("the fault message parameter")?;
        if self.eat(&Tok::Colon) {
            let ty = self.ty()?;
            if ty != Type::String {
                return self.error("the fault message parameter of a contingency must be `string`");
            }
        }
        self.expect(Tok::RightParen, "`)`")?;
        self.expect(Tok::LeftBrace, "`{` to open the contingency body")?;
        let contingency = self.statements(true)?;
        self.expect(Tok::RightBrace, "`}` to close the contingency body")?;
        Ok(BoundaryDecl {
            name,
            parameters,
            retains,
            body,
            message,
            contingency,
            line,
        })
    }

    fn function(&mut self, line: u32) -> Parsed<Function> {
        let name = self.name("a function name")?;
        let parameters = self.parameters()?;
        let precondition = if self.eat(&Tok::Oblige) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(Tok::Colon, "`:` and the result type")?;
        let result = self.ty()?;
        self.expect(Tok::Bind, "`:=`")?;
        let body = self.expr()?;
        Ok(Function {
            name,
            parameters,
            precondition,
            result,
            body,
            line,
        })
    }

    fn control(
        &mut self,
        public: bool,
        external: bool,
        contingent: bool,
        line: u32,
    ) -> Parsed<Control> {
        self.expect(Tok::Control, "`control`")?;
        let name = self.name("a control name")?;
        let parameters = self.parameters()?;
        let precondition = if self.eat(&Tok::Oblige) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(Tok::Exit, "`■` and the contract")?;
        let contract = self.contract_alternatives()?;
        let faults = self.eat(&Tok::Faults);
        let mut uses = Vec::new();
        if self.eat(&Tok::Uses) {
            loop {
                uses.push(self.effect_name()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.eat(&Tok::Bind);
        let body = if external {
            Vec::new()
        } else {
            self.statements(false)?
        };
        Ok(Control {
            name,
            public,
            external,
            contingent,
            faults,
            parameters,
            precondition,
            contract,
            uses,
            body,
            line,
        })
    }

    /// The statements of a body. A control body ends at the next declaration;
    /// a loop or boundary body ends at its closing brace.
    fn statements(&mut self, braced: bool) -> Parsed<Vec<Statement>> {
        let mut out = Vec::new();
        loop {
            if braced && self.peek() == &Tok::RightBrace {
                return Ok(out);
            }
            if !braced && self.is_declaration_start() {
                return Ok(out);
            }
            if matches!(self.peek(), Tok::Number(_)) && self.first_on_line() {
                self.advance();
            }
            let line = self.line();
            let selector = self.selector()?;
            self.expect(Tok::Guard, "`▷` after the selector")?;
            let command = self.command()?;
            out.push(Statement {
                selector,
                command,
                line,
            });
        }
    }

    fn selector(&mut self) -> Parsed<Selector> {
        let mut left = self.selector_and()?;
        while self.eat(&Tok::Or) {
            left = Selector::Or(Box::new(left), Box::new(self.selector_and()?));
        }
        Ok(left)
    }

    fn selector_and(&mut self) -> Parsed<Selector> {
        let mut left = self.selector_unary()?;
        while self.eat(&Tok::And) {
            left = Selector::And(Box::new(left), Box::new(self.selector_unary()?));
        }
        Ok(left)
    }

    fn selector_unary(&mut self) -> Parsed<Selector> {
        if self.eat(&Tok::Not) {
            return Ok(Selector::Not(Box::new(self.selector_unary()?)));
        }
        if self.eat(&Tok::Top) {
            return Ok(Selector::Top);
        }
        if self.eat(&Tok::LeftParen) {
            let inner = self.selector()?;
            self.expect(Tok::RightParen, "`)`")?;
            return Ok(inner);
        }
        let site = self.name("a selector")?;
        self.expect(Tok::Dot, "`.` and an alternative")?;
        let alternative = self.name("an alternative")?;
        Ok(Selector::Atom(site, alternative))
    }

    fn arms(&mut self) -> Parsed<Vec<Arm>> {
        let mut out = Vec::new();
        if !self.eat(&Tok::Arms) {
            return Ok(out);
        }
        loop {
            let alternative = self.name("an alternative")?;
            let mut names = Vec::new();
            if self.eat(&Tok::LeftBrace) {
                while !self.eat(&Tok::RightBrace) {
                    if !names.is_empty() {
                        self.expect(Tok::Comma, "`,`")?;
                    }
                    let mutable = self.eat(&Tok::Tilde);
                    let pat_name = self.name("a payload name")?;
                    if mutable {
                        names.push(format!("~{pat_name}"));
                    } else {
                        names.push(pat_name);
                    }
                }
            }
            out.push(Arm { alternative, names });
            if !self.eat(&Tok::Bar) {
                return Ok(out);
            }
        }
    }

    fn payload(&mut self) -> Parsed<Vec<Expr>> {
        let mut out = Vec::new();
        if self.eat(&Tok::LeftBrace) {
            while !self.eat(&Tok::RightBrace) {
                if !out.is_empty() {
                    self.expect(Tok::Comma, "`,`")?;
                }
                out.push(self.expr()?);
            }
        }
        Ok(out)
    }

    fn arguments(&mut self) -> Parsed<Vec<Expr>> {
        let mut out = Vec::new();
        self.expect(Tok::LeftParen, "`(`")?;
        while !self.eat(&Tok::RightParen) {
            if !out.is_empty() {
                self.expect(Tok::Comma, "`,`")?;
            }
            out.push(self.expr()?);
        }
        Ok(out)
    }

    fn command(&mut self) -> Parsed<Command> {
        match self.peek().clone() {
            Tok::Oblige => {
                self.advance();
                Ok(Command::Oblige(self.expr()?))
            }
            Tok::Impossible => {
                self.advance();
                Ok(Command::Impossible)
            }
            Tok::Fault => {
                self.advance();
                Ok(Command::Fault {
                    message: self.expr()?,
                    assumed: false,
                })
            }
            Tok::Assume => {
                self.advance();
                self.expect(Tok::Impossible, "`⊥` after `assume`")?;
                Ok(Command::Fault {
                    message: self.expr()?,
                    assumed: true,
                })
            }
            Tok::Supervise => {
                self.advance();
                let boundary = self.name("a boundary name")?;
                let arguments = self.arguments()?;
                Ok(Command::Supervise {
                    boundary,
                    arguments,
                })
            }
            Tok::Terminate => {
                self.advance();
                let status = if !self.first_on_line()
                    && !matches!(self.peek(), Tok::RightBrace | Tok::End)
                {
                    Some(self.expr()?)
                } else {
                    None
                };
                Ok(Command::Terminate(status))
            }
            Tok::Escalate => {
                self.advance();
                Ok(Command::Escalate)
            }
            Tok::Recover => {
                self.advance();
                self.expect(Tok::Again, "`↑` after `recover`")?;
                let loop_name = self.name("the name of a loop")?;
                Ok(Command::Recover { loop_name })
            }
            Tok::Exit => {
                self.advance();
                let alternative = self.name("an alternative")?;
                Ok(Command::Exit {
                    alternative,
                    payload: self.payload()?,
                })
            }
            Tok::Leave => {
                self.advance();
                let alternative = self.name("an alternative")?;
                Ok(Command::Leave {
                    alternative,
                    payload: self.payload()?,
                })
            }
            Tok::Again => {
                self.advance();
                let name = self.name("the name of a loop")?;
                let arguments = if self.peek() == &Tok::LeftParen {
                    self.arguments()?
                } else {
                    Vec::new()
                };
                Ok(Command::Again { name, arguments })
            }
            Tok::Tilde => {
                self.advance();
                let name = self.name("a name")?;
                self.bind_or_capture(true, name)
            }
            Tok::Name(name) => {
                self.advance();
                match self.peek() {
                    Tok::LeftBrace => {
                        self.advance();
                        let mut names = Vec::new();
                        while !self.eat(&Tok::RightBrace) {
                            if !names.is_empty() {
                                self.expect(Tok::Comma, "`,`")?;
                            }
                            let mutable = self.eat(&Tok::Tilde);
                            let field_var = self.name("a variable name")?;
                            if mutable {
                                names.push(format!("~{field_var}"));
                            } else {
                                names.push(field_var);
                            }
                        }
                        self.expect(Tok::Bind, "`:=` after struct pattern")?;
                        let value = self.expr()?;
                        Ok(Command::Unpack {
                            struct_name: name,
                            names,
                            value,
                        })
                    }
                    Tok::Dot | Tok::LeftBracket => {
                        let mut steps = Vec::new();
                        while matches!(self.peek(), Tok::Dot | Tok::LeftBracket) {
                            if self.eat(&Tok::Dot) {
                                steps.push(PlaceStep::Field(self.name("a field name")?));
                            } else if self.eat(&Tok::LeftBracket) {
                                let idx = self.expr()?;
                                self.expect(Tok::RightBracket, "`]`")?;
                                steps.push(PlaceStep::Index(idx));
                            }
                        }
                        self.expect(Tok::Assign, "`←` after place")?;
                        let value = self.expr()?;
                        Ok(Command::AssignPlace {
                            target: Place { root: name, steps },
                            value,
                        })
                    }
                    Tok::Assign => {
                        self.advance();
                        Ok(Command::Assign {
                            name,
                            value: self.expr()?,
                        })
                    }
                    Tok::Bind => self.bind_or_capture(false, name),
                    Tok::Colon => match self.peek_at(1) {
                        Tok::Test => {
                            self.position += 2;
                            Ok(Command::Test {
                                site: name,
                                condition: self.expr()?,
                            })
                        }
                        Tok::Delta => {
                            self.position += 2;
                            let operation = self.name("an operation")?;
                            let arguments = self.arguments()?;
                            Ok(Command::Decide {
                                site: name,
                                operation,
                                arguments,
                                arms: self.arms()?,
                            })
                        }
                        Tok::Loop => {
                            self.position += 2;
                            self.loop_statement(name)
                        }
                        Tok::Boundary => {
                            self.position += 2;
                            self.lexical_boundary(name)
                        }
                        _ => self.bind_or_capture(false, name),
                    },
                    other => self.error(format!(
                        "expected `:=`, `←` or `:` after `{name}`, found {}",
                        describe(other)
                    )),
                }
            }
            other => self.error(format!("expected a command, found {}", describe(&other))),
        }
    }

    fn bind_or_capture(&mut self, mutable: bool, name: String) -> Parsed<Command> {
        let ty = if self.eat(&Tok::Colon) {
            Some(self.ty()?)
        } else {
            None
        };
        self.expect(Tok::Bind, "`:=`")?;
        if self.eat(&Tok::Capture) {
            let operation = self.name("an operation")?;
            let arguments = self.arguments()?;
            return Ok(Command::Capture {
                mutable,
                name,
                ty,
                operation,
                arguments,
            });
        }
        Ok(Command::Bind {
            mutable,
            name,
            ty,
            value: self.expr()?,
        })
    }

    fn lexical_boundary(&mut self, site: String) -> Parsed<Command> {
        self.expect(Tok::LeftBrace, "`{` to open the boundary body")?;
        let body = self.statements(true)?;
        self.expect(Tok::RightBrace, "`}` to close the boundary body")?;
        self.expect(Tok::Contingency, "`contingency`")?;
        let mut captures = Vec::new();
        if self.eat(&Tok::LeftBracket) {
            while !self.eat(&Tok::RightBracket) {
                if !captures.is_empty() {
                    self.expect(Tok::Comma, "`,`")?;
                }
                captures.push(self.name("a captured variable name")?);
            }
        }
        let message = if self.eat(&Tok::LeftParen) {
            let msg = self.name("the fault message parameter")?;
            if self.eat(&Tok::Colon) {
                let ty = self.ty()?;
                if ty != Type::String {
                    return self.error("the fault message parameter must have type `string`");
                }
            }
            self.expect(Tok::RightParen, "`)`")?;
            Some(msg)
        } else {
            None
        };
        self.expect(Tok::LeftBrace, "`{` to open the contingency body")?;
        let contingency = self.statements(true)?;
        self.expect(Tok::RightBrace, "`}` to close the contingency body")?;
        Ok(Command::Boundary {
            site,
            body,
            captures,
            message,
            contingency,
        })
    }

    fn loop_statement(&mut self, site: String) -> Parsed<Command> {
        let name = self.name("the name of the loop")?;
        let mut parameters = Vec::new();
        if self.eat(&Tok::LeftParen) {
            while !self.eat(&Tok::RightParen) {
                if !parameters.is_empty() {
                    self.expect(Tok::Comma, "`,`")?;
                }
                let mutable = self.eat(&Tok::Tilde);
                let parameter = self.name("a loop parameter")?;
                self.expect(Tok::Colon, "`:`")?;
                let ty = self.ty()?;
                self.expect(Tok::Bind, "`:=`")?;
                parameters.push(LoopParameter {
                    mutable,
                    name: parameter,
                    ty,
                    initial: self.expr()?,
                });
            }
        }
        self.expect(Tok::LeftBrace, "`{` to open the loop body")?;
        let invariant = if self.eat(&Tok::Oblige) {
            Some(self.expr()?)
        } else {
            None
        };
        let body = self.statements(true)?;
        self.expect(Tok::RightBrace, "`}` to close the loop body")?;
        Ok(Command::Loop {
            site,
            name,
            parameters,
            invariant,
            body,
            arms: self.arms()?,
        })
    }

    fn expr(&mut self) -> Parsed<Expr> {
        self.binary(1)
    }

    fn binary_operator(&self) -> Option<Binary> {
        Some(match self.peek() {
            Tok::Or => Binary::Or,
            Tok::And => Binary::And,
            Tok::Equal => Binary::Equal,
            Tok::NotEqual => Binary::NotEqual,
            Tok::Less => Binary::Less,
            Tok::LessEqual => Binary::LessEqual,
            Tok::Greater => Binary::Greater,
            Tok::GreaterEqual => Binary::GreaterEqual,
            Tok::Plus => Binary::Add,
            Tok::Minus => Binary::Subtract,
            Tok::Star => Binary::Multiply,
            Tok::Slash => Binary::Divide,
            Tok::Mod => Binary::Remainder,
            _ => return None,
        })
    }

    fn binary(&mut self, level: u8) -> Parsed<Expr> {
        if level > 5 {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(operator) = self.binary_operator() {
            if operator.level() != level {
                break;
            }
            self.advance();
            let right = self.binary(level + 1)?;
            left = Expr::Binary(operator, Box::new(left), Box::new(right));
            if level == 3 {
                if self.binary_operator().is_some_and(|next| next.level() == 3) {
                    return self.error("comparisons do not chain; use `∧`");
                }
                break;
            }
        }
        Ok(left)
    }

    fn unary(&mut self) -> Parsed<Expr> {
        if self.eat(&Tok::Not) {
            return Ok(Expr::Unary(Unary::Not, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Minus) {
            return Ok(Expr::Unary(Unary::Negate, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Move) {
            return Ok(Expr::Move(Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Amp) {
            let mode = if self.eat(&Tok::Tilde) {
                BorrowMode::Exclusive
            } else {
                BorrowMode::Shared
            };
            return Ok(Expr::Borrow(mode, Box::new(self.unary()?)));
        }
        if self.eat(&Tok::Tilde) {
            return Ok(Expr::Borrow(BorrowMode::Exclusive, Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Parsed<Expr> {
        let mut left = self.primary()?;
        loop {
            if !self.first_on_line() && self.eat(&Tok::Dot) {
                let member = self.name("a field or constructor name")?;
                if !self.first_on_line()
                    && self.peek() == &Tok::LeftBrace
                    && let Expr::Name(type_name) = left
                {
                    let fields = self.payload()?;
                    left = Expr::Variant(type_name, member, fields);
                    continue;
                }
                if !self.first_on_line() && self.peek() == &Tok::LeftParen {
                    let args = self.arguments()?;
                    if member == "len" && args.is_empty() {
                        left = Expr::Len(Box::new(left));
                    } else if member == "clone" && args.is_empty() {
                        left = Expr::Clone(Box::new(left));
                    } else {
                        let mut all_args = Vec::with_capacity(args.len() + 1);
                        all_args.push(left);
                        all_args.extend(args);
                        left = Expr::Call(member, all_args);
                    }
                    continue;
                }
                left = Expr::Field(Box::new(left), member);
                continue;
            }
            if !self.first_on_line() && self.eat(&Tok::LeftBracket) {
                let index = self.expr()?;
                self.expect(Tok::RightBracket, "`]`")?;
                left = Expr::Index(Box::new(left), Box::new(index));
                continue;
            }
            break;
        }
        Ok(left)
    }

    fn primary(&mut self) -> Parsed<Expr> {
        match self.peek().clone() {
            Tok::Number(digits) => {
                self.advance();
                Ok(Expr::Integer(Int::parse(&digits).expect("digits")))
            }
            Tok::String(text) => {
                self.advance();
                Ok(Expr::String(text))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Boolean(true))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Boolean(false))
            }
            Tok::EmptyMap => {
                self.advance();
                Ok(Expr::EmptyMap)
            }
            Tok::LeftBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.eat(&Tok::RightBracket) {
                    if !items.is_empty() {
                        self.expect(Tok::Comma, "`,`")?;
                    }
                    items.push(self.expr()?);
                }
                Ok(Expr::Vector(items))
            }
            Tok::LeftParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(Tok::RightParen, "`)`")?;
                Ok(inner)
            }
            Tok::Name(name) => {
                self.advance();
                if name == "map"
                    && !self.first_on_line()
                    && self.peek() == &Tok::LeftBrace
                    && self.peek_at(1) == &Tok::RightBrace
                {
                    self.advance();
                    self.advance();
                    return Ok(Expr::EmptyMap);
                }
                if !self.first_on_line() && self.peek() == &Tok::LeftParen {
                    let mut args = self.arguments()?;
                    if name == "len" && args.len() == 1 {
                        return Ok(Expr::Len(Box::new(args.remove(0))));
                    }
                    if name == "clone" && args.len() == 1 {
                        return Ok(Expr::Clone(Box::new(args.remove(0))));
                    }
                    if name == "old" && args.len() == 1 {
                        if let Expr::Name(inner) = args.remove(0) {
                            return Ok(Expr::Old(inner));
                        }
                        return self.error("`old(…)` takes a parameter name");
                    }
                    return Ok(Expr::Call(name, args));
                }
                if !self.first_on_line() && self.peek() == &Tok::LeftBrace {
                    let fields = self.payload()?;
                    return Ok(Expr::Struct(name, fields));
                }
                Ok(Expr::Name(name))
            }
            other => self.error(format!(
                "expected an expression, found {}",
                describe(&other)
            )),
        }
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Name(name) => format!("`{name}`"),
        Tok::Number(digits) => format!("`{digits}`"),
        Tok::String(s) => format!("\"{s}\""),
        Tok::End => "the end of the text".to_string(),
        other => format!("{other:?}").to_lowercase(),
    }
}

/// Reads a program in the core text form.
pub fn parse(source: &str) -> Result<Program, ParseError> {
    let mut parser = Parser {
        tokens: lex(source)?,
        position: 0,
    };
    parser.program()
}
