//! The reader of the core text form.
//!
//! The form is the notation of `docs/v2/core.md`: guarded statements,
//! `selector ▷ command`, with the glyphs of Rails and no sugar. Every glyph
//! has a spelling in ASCII, the one the surface document gives it ([G8])
//! where it has one. `docs/v2/oracle.md` states the grammar.
//!
//! The text is free of layout with one exception: a `(` that is the first
//! token on its line never continues the expression before it. A statement
//! may therefore begin with a selector in parentheses.

use crate::int::Int;
use crate::syntax::{
    Alternative, Arm, Binary, Command, Control, Declaration, Expr, Function, LoopParameter,
    Parameter, Program, Selector, Statement, Unary,
};
use crate::typed::Type;
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
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
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
        "control" => Tok::Control,
        "extern" => Tok::Extern,
        "function" => Tok::Function,
        "pub" => Tok::Pub,
        "uses" => Tok::Uses,
        "faults" => Tok::Faults,
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
                ',' => (Tok::Comma, 1),
                '.' => (Tok::Dot, 1),
                '(' => (Tok::LeftParen, 1),
                ')' => (Tok::RightParen, 1),
                '{' => (Tok::LeftBrace, 1),
                '}' => (Tok::RightBrace, 1),
                '+' => (Tok::Plus, 1),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                ':' if pair('=') => (Tok::Bind, 2),
                ':' => (Tok::Colon, 1),
                '|' if pair('>') => (Tok::Guard, 2),
                '|' if pair('|') => (Tok::Or, 2),
                '|' => (Tok::Bar, 1),
                '&' if pair('&') => (Tok::And, 2),
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

    fn line(&self) -> u32 {
        self.tokens[self.position].line
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
            // A test's alternatives are spelled like the Boolean literals.
            Tok::True if what == "an alternative" => {
                self.advance();
                Ok("true".to_string())
            }
            Tok::False if what == "an alternative" => {
                self.advance();
                Ok("false".to_string())
            }
            other => self.error(format!("expected {what}, found {}", describe(&other))),
        }
    }

    fn ty(&mut self) -> Parsed<Type> {
        let line = self.line();
        let name = self.name("a type")?;
        Type::from_name(&name).ok_or(ParseError {
            line,
            message: format!("`{name}` is not a type of this slice (bool, uN, iN, usize)"),
        })
    }

    fn parameters(&mut self) -> Parsed<Vec<Parameter>> {
        let mut out = Vec::new();
        self.expect(Tok::LeftParen, "`(`")?;
        while !self.eat(&Tok::RightParen) {
            if !out.is_empty() {
                self.expect(Tok::Comma, "`,`")?;
            }
            let name = self.name("a parameter name")?;
            self.expect(Tok::Colon, "`:`")?;
            out.push(Parameter {
                name,
                ty: self.ty()?,
            });
        }
        Ok(out)
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
                Tok::Pub | Tok::Extern | Tok::Control => {
                    declarations.push(Declaration::Control(self.control(line)?));
                }
                other => {
                    return self
                        .error(format!("expected a declaration, found {}", describe(other)));
                }
            }
        }
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

    fn control(&mut self, line: u32) -> Parsed<Control> {
        let public = self.eat(&Tok::Pub);
        let external = self.eat(&Tok::Extern);
        self.expect(Tok::Control, "`control`")?;
        let name = self.name("a control name")?;
        let parameters = self.parameters()?;
        let precondition = if self.eat(&Tok::Oblige) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(Tok::Exit, "`■` and the contract")?;
        self.expect(Tok::LeftBrace, "`{` to open the contract")?;
        let mut contract = Vec::new();
        loop {
            let name = self.name("an alternative")?;
            let mut fields = Vec::new();
            if self.eat(&Tok::LeftBrace) {
                while !self.eat(&Tok::RightBrace) {
                    if !fields.is_empty() {
                        self.expect(Tok::Comma, "`,`")?;
                    }
                    let field = self.name("a field name")?;
                    self.expect(Tok::Colon, "`:`")?;
                    fields.push(Parameter {
                        name: field,
                        ty: self.ty()?,
                    });
                }
            }
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
        if self.peek() == &Tok::Faults {
            return self.error("`faults` belongs to slice 2 and is not read yet");
        }
        let mut uses = Vec::new();
        if self.eat(&Tok::Uses) {
            loop {
                uses.push(self.name("an effect")?);
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
            parameters,
            precondition,
            contract,
            uses,
            body,
            line,
        })
    }

    /// The statements of a body. A control body ends at the next declaration;
    /// a loop body ends at its closing brace.
    fn statements(&mut self, braced: bool) -> Parsed<Vec<Statement>> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                Tok::RightBrace if braced => return Ok(out),
                Tok::End | Tok::Function | Tok::Pub | Tok::Extern | Tok::Control if !braced => {
                    return Ok(out);
                }
                _ => {}
            }
            // The documents number the statements of a body; the numbers mean
            // nothing.
            if matches!(self.peek(), Tok::Number(_)) && self.tokens[self.position].first_on_line {
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
                    names.push(self.name("a payload name")?);
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
                self.bind(true, name)
            }
            Tok::Name(name) => {
                self.advance();
                match self.peek() {
                    Tok::Assign => {
                        self.advance();
                        Ok(Command::Assign {
                            name,
                            value: self.expr()?,
                        })
                    }
                    Tok::Bind => self.bind(false, name),
                    Tok::Colon => match self.tokens[self.position + 1].tok {
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
                        _ => self.bind(false, name),
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

    fn bind(&mut self, mutable: bool, name: String) -> Parsed<Command> {
        let ty = if self.eat(&Tok::Colon) {
            Some(self.ty()?)
        } else {
            None
        };
        self.expect(Tok::Bind, "`:=`")?;
        Ok(Command::Bind {
            mutable,
            name,
            ty,
            value: self.expr()?,
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
        match self.peek().clone() {
            Tok::Number(digits) => {
                self.advance();
                Ok(Expr::Integer(Int::parse(&digits).expect("digits")))
            }
            Tok::True => {
                self.advance();
                Ok(Expr::Boolean(true))
            }
            Tok::False => {
                self.advance();
                Ok(Expr::Boolean(false))
            }
            Tok::LeftParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(Tok::RightParen, "`)`")?;
                Ok(inner)
            }
            Tok::Name(name) => {
                self.advance();
                let next = &self.tokens[self.position];
                if next.tok == Tok::LeftParen && !next.first_on_line {
                    return Ok(Expr::Call(name, self.arguments()?));
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
