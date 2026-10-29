use crate::{Diagnostic, Result, SourceRange, Span};

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub range: SourceRange,
}
impl Expr {
    pub(crate) fn checked<T>(
        &self,
        fallback: Span,
        check: impl FnOnce(Span) -> Result<T>,
    ) -> Result<T> {
        check(self.span(fallback)).map_err(|error| self.locate(error, fallback))
    }
    pub fn span(&self, fallback: Span) -> Span {
        if self.range.start.line == 0 {
            fallback
        } else {
            self.range.start
        }
    }
    /// Preserve a more precise child diagnostic; synthetic nodes use their caller's location.
    pub fn locate(&self, mut error: Diagnostic, fallback: Span) -> Diagnostic {
        if error.end.is_none()
            && self.range.start.line != 0
            && (error.span == fallback || error.span == self.range.start)
        {
            error = error.with_range(self.range);
        }
        error
    }
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Name(String),
    String(String),
    U32(u32),
    Bool(bool),
    Unit,
    Bytes(Vec<u8>),
    Call(String, Vec<Expr>),
    List(Vec<Expr>),
    Construct(String, Vec<Expr>),
    NamedRecord(String, Vec<(String, Expr)>),
    Borrow(Box<Expr>),
    BorrowMutable(Box<Expr>),
    Deref(Box<Expr>),
    Project(Box<Expr>, String),
    Unary(Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}
impl ExprKind {
    pub fn at(self, range: SourceRange) -> Expr {
        Expr { kind: self, range }
    }
    pub fn synthetic(self) -> Expr {
        self.at(SourceRange::default())
    }
}
#[derive(Clone, Debug)]
pub enum Mask {
    Name(String),
    And(Box<Mask>, Box<Mask>),
    Or(Box<Mask>, Box<Mask>),
}
#[derive(Clone, Debug)]
pub struct Field {
    /// Start of the source type, retained for editor inference information.
    pub type_span: Span,
    pub origins: Vec<OriginArgument>,
    /// Bracket mappings on a schema reference field with a separate storage arrow.
    pub referent_origins: Vec<OriginArgument>,
    pub name: String,
    pub ty: String,
}
/// Source-level origin names have a scope distinct from values and types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginArgument {
    Group(String),
    Input(String),
    Contained { input: String, group: String },
}
impl OriginArgument {
    pub fn display(&self) -> String {
        match self {
            Self::Group(n) => format!("'{n}"),
            Self::Input(n) => n.clone(),
            Self::Contained { input, group } => format!("{input}['{group}]"),
        }
    }
}
#[derive(Clone, Debug)]
pub struct LoopParameter {
    pub field: Field,
    pub initial: Expr,
}
#[derive(Clone, Debug)]
pub struct Outcome {
    pub name: String,
    pub fields: Vec<Field>,
}
#[derive(Clone, Debug)]
pub struct Arm {
    pub outcome: String,
    pub bindings: Vec<Field>,
    pub atom: String,
}
#[derive(Clone, Debug)]
pub struct InlineArm {
    pub span: Span,
    pub pattern: Arm,
    pub result: OutcomeExpr,
}
#[derive(Clone, Debug)]
pub enum OutcomeExpr {
    Forward(Span),
    Construct(Span, String, Vec<Expr>),
    Delta(Span, Expr, Vec<InlineArm>),
}
#[derive(Clone, Debug)]
pub enum Statement {
    Forward(Expr),
    Invoke(Expr),
    Bind(String, Option<String>, Expr, bool),
    Assign(Expr, Expr),
    Drop(Expr),
    Emit(Expr),
    Exit(String, Vec<Expr>),
    ExitOutcome(OutcomeExpr),
    BindOutcome(String, Option<String>, OutcomeExpr, bool),
    ExitName(String),
    Capture(String, Option<String>, Expr, bool),
    Again(String, Vec<Expr>),
    Delta(Option<String>, Expr, Vec<Arm>),
}
#[derive(Clone, Debug)]
pub enum Item {
    Alias(Span, String, Mask),
    Guard(Mask, Vec<(Span, Statement)>),
    /// Explicit selection extent, without introducing a value or lifetime scope.
    Group(Span, Mask, Vec<Item>),
    Over(Span, Mask, Vec<Item>),
    Loop(Span, String, Vec<LoopParameter>, Vec<Item>),
}
pub type TypeBounds = std::collections::BTreeMap<String, Vec<String>>;
#[derive(Clone, Debug)]
pub struct Region {
    /// Explicit owner anchors, keyed by outcome and payload position.
    pub return_origins: std::collections::BTreeMap<(String, usize), String>,
    /// Compiler-only abstract witness signature; never emitted in an executable program.
    pub abstract_signature: bool,
    pub type_parameters: Vec<String>,
    pub type_bounds: TypeBounds,
    /// Compiler-only lexical type bindings of a specialized control.
    pub type_bindings: std::collections::BTreeMap<String, String>,
    pub public: bool,
    pub span: Span,
    pub end: Span,
    pub name_span: Span,
    pub name: String,
    pub parameters: Vec<Field>,
    pub exits: Vec<Outcome>,
    pub infer_exits: bool,
    /// Declared labels whose nonempty payloads are inferred from the body.
    pub infer_payloads: std::collections::BTreeSet<String>,
    pub exit_contract: Option<String>,
    pub exit_origins: Vec<OriginArgument>,
    pub modern: bool,
    pub body: Vec<Item>,
}
#[derive(Clone, Debug)]
pub struct OutcomeContract {
    /// Host-boundary defaults; deliberately separate from alternative shapes.
    pub exit_codes: std::collections::BTreeMap<String, u8>,
    pub origin_parameters: Vec<String>,
    pub type_parameters: Vec<String>,
    pub public: bool,
    pub span: Span,
    pub name_span: Span,
    pub end: Span,
    pub name: String,
    pub outcomes: Vec<Outcome>,
}
#[derive(Clone, Debug)]
pub enum DataBody {
    Struct(Vec<Field>),
    Variant(Vec<Outcome>),
}
#[derive(Clone, Debug)]
pub struct DataDeclaration {
    pub origin_parameters: Vec<String>,
    pub type_parameters: Vec<String>,
    pub public: bool,
    pub public_fields: std::collections::BTreeSet<String>,
    pub span: Span,
    pub name_span: Span,
    pub end: Span,
    pub name: String,
    pub body: DataBody,
}
#[derive(Clone, Debug)]
pub struct Import {
    pub span: Span,
    pub path: String,
    pub alias: String,
}
#[derive(Clone, Debug)]
pub struct AssociatedType {
    pub bounds: Vec<String>,
    pub span: Span,
    pub name: String,
    pub value: Option<String>,
}
#[derive(Clone, Debug)]
pub struct TraitDeclaration {
    pub span: Span,
    pub name_span: Span,
    pub end: Span,
    pub name: String,
    pub public: bool,
    pub module: usize,
    pub associated: Vec<AssociatedType>,
    pub methods: Vec<Region>,
}
#[derive(Clone, Debug)]
pub struct Implementation {
    pub span: Span,
    pub end: Span,
    pub trait_name: String,
    pub target: String,
    pub module: usize,
    pub associated: Vec<AssociatedType>,
    pub methods: Vec<Region>,
}
#[derive(Clone, Debug)]
pub struct TraitCheck {
    pub trait_name: String,
    pub requirements: Vec<(String, Vec<String>, Span)>,
    pub span: Span,
    pub target: String,
    pub public: bool,
    pub associated: Vec<String>,
    pub expected: Vec<Region>,
}
#[derive(Clone, Debug)]
pub struct Unit {
    pub trait_types: std::collections::BTreeMap<String, String>,
    pub traits: Vec<TraitDeclaration>,
    pub implementations: Vec<Implementation>,
    pub trait_checks: Vec<TraitCheck>,
    pub imports: Vec<Import>,
    pub regions: Vec<Region>,
    pub contracts: Vec<OutcomeContract>,
    pub data: Vec<DataDeclaration>,
}
#[derive(Clone, Debug)]
enum Kind {
    Word(String),
    String(String),
    Number(String),
    Symbol(String),
    End,
}
#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    span: Span,
    end: Span,
}
impl Token {
    fn is(&self, text: &str) -> bool {
        matches!(&self.kind, Kind::Word(s) | Kind::Symbol(s) if s == text)
    }
}

fn lex(source: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = source.chars().collect();
    let (mut i, mut line, mut column) = (0, 1, 1);
    let mut tokens = vec![];
    while i < chars.len() {
        let c = chars[i];
        let span = Span { line, column };
        if c.is_whitespace() {
            i += 1;
            if c == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
                column += 1;
            }
            continue;
        }
        if c == '"' {
            i += 1;
            column += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                let c = chars[i];
                i += 1;
                column += 1;
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    let Some(&e) = chars.get(i) else {
                        break;
                    };
                    i += 1;
                    column += 1;
                    s.push(match e {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        '\\' => '\\',
                        '"' => '"',
                        _ => {
                            return Err(Diagnostic::new(
                                "E_LEX",
                                span,
                                "unsupported string escape",
                            ));
                        }
                    });
                } else {
                    if c == '\n' {
                        line += 1;
                        column = 1;
                    }
                    s.push(c);
                }
            }
            if !closed {
                return Err(Diagnostic::new("E_LEX", span, "unterminated string"));
            }
            tokens.push(Token {
                kind: Kind::String(s),
                span,
                end: Span { line, column },
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
                column += 1;
            }
            if chars[start..i]
                .iter()
                .collect::<String>()
                .starts_with("_path_poly_")
            {
                return Err(Diagnostic::new(
                    "E_NAME",
                    span,
                    "compiler type-parameter identities cannot be spelled in source",
                ));
            }
            tokens.push(Token {
                kind: Kind::Word(chars[start..i].iter().collect()),
                span,
                end: Span { line, column },
            });
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
                column += 1;
            }
            tokens.push(Token {
                kind: Kind::Number(chars[start..i].iter().collect()),
                span,
                end: Span { line, column },
            });
            continue;
        }
        let pair: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let symbol = if chars[i..].starts_with(&['.', '.', '.']) {
            i += 3;
            column += 3;
            "...".to_string()
        } else if [":=", "<-", "->", "=>", "&&", "||", "==", "!=", "<=", ">="]
            .contains(&pair.as_str())
        {
            i += 2;
            column += 2;
            pair
        } else if "(){}[],:;=&|!<>.*+Δ'@".contains(c) {
            i += 1;
            column += 1;
            c.to_string()
        } else {
            return Err(Diagnostic::new(
                "E_LEX",
                span,
                format!("unsupported character {c:?}"),
            ));
        };
        tokens.push(Token {
            kind: Kind::Symbol(symbol),
            span,
            end: Span { line, column },
        });
    }
    tokens.push(Token {
        kind: Kind::End,
        span: Span { line, column },
        end: Span { line, column },
    });
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    brace_expr: bool,
}
impl Parser {
    fn here(&self) -> Span {
        self.tokens[self.pos].span
    }
    fn at(&self, s: &str) -> bool {
        self.tokens[self.pos].is(s)
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.at(s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn error(&self, message: impl Into<String>) -> Diagnostic {
        Diagnostic::new("E_PARSE", self.here(), message).with_range(SourceRange {
            start: self.here(),
            end: self.tokens[self.pos].end,
        })
    }
    fn need(&mut self, s: &str) -> Result<()> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{s}`")))
        }
    }
    fn id(&mut self) -> Result<String> {
        if let Kind::Word(s) = self.tokens[self.pos].kind.clone() {
            self.pos += 1;
            Ok(s)
        } else {
            Err(self.error("expected identifier"))
        }
    }
    fn qualified(&mut self) -> Result<String> {
        let mut s = self.id()?;
        while self.eat(".") {
            s.push('.');
            s.push_str(&self.id()?);
        }
        Ok(s)
    }
    fn ty(&mut self) -> Result<String> {
        if self.eat("ref") || self.eat("&") {
            let mutable = self.eat("mutable");
            return Ok(format!(
                "&{}{}",
                if mutable { "mutable " } else { "" },
                self.ty()?
            ));
        }
        let name = self.qualified()?;
        if self.eat("<") {
            if name == "outcome" {
                let producer = self.ty()?;
                self.need(">")?;
                return Ok(format!("outcome<{producer}>"));
            }
            let mut args = vec![self.ty()?];
            while self.eat(",") {
                if let Kind::Number(n) = self.tokens[self.pos].kind.clone() {
                    self.pos += 1;
                    args.push(n);
                } else {
                    args.push(self.ty()?);
                }
            }
            self.need(">")?;
            let mut ty = format!("{name}<{}>", args.join(","));
            while self.eat(".") {
                ty.push('.');
                ty.push_str(&self.id()?);
            }
            Ok(crate::manifest::canonical_type(&ty))
        } else {
            Ok(crate::manifest::canonical_type(&name))
        }
    }
    fn input_origin(&mut self) -> Result<OriginArgument> {
        let input = self.id()?;
        if self.eat("[") {
            self.need("'")?;
            let group = self.id()?;
            self.need("]")?;
            Ok(OriginArgument::Contained { input, group })
        } else {
            Ok(OriginArgument::Input(input))
        }
    }
    fn origin_arguments(&mut self) -> Result<Vec<OriginArgument>> {
        if !self.eat("[") {
            return Ok(vec![]);
        }
        let mut args = vec![];
        loop {
            let arg = if self.eat("'") {
                OriginArgument::Group(self.id()?)
            } else {
                self.input_origin()?
            };
            args.push(arg);
            if !self.eat(",") {
                break;
            }
        }
        self.need("]")?;
        Ok(args)
    }
    fn field(&mut self) -> Result<Field> {
        let named = self.tokens.get(self.pos + 1).is_some_and(|t| t.is(":"));
        let name = if named {
            let n = self.id()?;
            self.need(":")?;
            Some(n)
        } else {
            None
        };
        let type_span = self.here();
        let ty = self.ty()?;
        let origins = self.origin_arguments()?;
        let name = if let Some(n) = name { n } else { self.id()? };
        Ok(Field {
            name,
            ty,
            origins,
            referent_origins: vec![],
            type_span,
        })
    }
    fn ordinary_field(&mut self) -> Result<Field> {
        let field = self.field()?;
        if !field.origins.is_empty() {
            return Err(
                self.error("origin bindings belong on struct fields or control result payloads")
            );
        }
        Ok(field)
    }
    fn control(&mut self, span: Span, public: bool, signature: bool) -> Result<Region> {
        let modern = self.eat("control");
        if !modern {
            self.need("region")?;
        }
        let name_span = self.here();
        let name = self.id()?;
        let (type_parameters, type_bounds) = self.control_type_parameters()?;
        if !modern && !type_parameters.is_empty() {
            return Err(Diagnostic::new(
                "E_GENERIC",
                name_span,
                "generic definitions require control syntax and its owning-value rules",
            ));
        }
        if !self.at("(") {
            return Err(self.error("control parameters use parentheses"));
        }
        let parameters = self.fields()?;
        self.need("exits")?;
        let infer_exits = self.eat("auto");
        let mut exits = vec![];
        let mut infer_payloads = std::collections::BTreeSet::new();
        let mut return_origins = std::collections::BTreeMap::new();
        let mut exit_contract = None;
        let mut exit_origins = vec![];
        if !infer_exits {
            let wrapped = self.eat("{");
            let mut bare = true;
            loop {
                let saved = self.pos;
                let mut name = self.qualified()?;
                if self.at("<") {
                    self.pos = saved;
                    name = self.ty()?;
                }
                let bindings = self.origin_arguments()?;
                if !bindings.is_empty() {
                    if wrapped || !exits.is_empty() {
                        return Err(
                            self.error("origin arguments belong on a single named exit contract")
                        );
                    }
                    exit_origins = bindings;
                }
                // A body may start with ordinary statements. Recognize a brace
                // payload only if a complete field list precedes the next contract
                // alternative, wrapped-contract end, or control body.
                let (fields, origins) = if self.eat("...") {
                    if !modern || signature {
                        return Err(self.error("payload inference requires a control body; use an explicit payload contract here"));
                    }
                    bare = false;
                    infer_payloads.insert(name.clone());
                    (vec![], vec![])
                } else if self.at("(") {
                    bare = false;
                    self.exit_fields()?
                } else if self.at("{") {
                    let saved = self.pos;
                    match self.exit_fields() {
                        Ok(fields)
                            if self.at(",")
                                || self.at("{")
                                || self.at(";")
                                || (wrapped && self.at("}")) =>
                        {
                            bare = false;
                            fields
                        }
                        _ => {
                            self.pos = saved;
                            (vec![], vec![])
                        }
                    }
                } else {
                    (vec![], vec![])
                };
                for (index, origin) in origins.into_iter().enumerate() {
                    if let Some(origin) = origin {
                        return_origins.insert((name.clone(), index), origin);
                    }
                }
                exits.push(Outcome { name, fields });
                if !self.eat(",") || (wrapped && self.at("}")) {
                    break;
                }
            }
            if wrapped {
                self.need("}")?;
            } else if bare && exits.len() == 1 && exits[0].fields.is_empty() {
                // Resolve this candidate against declared outcome types in checking.
                // `exits { label }` always means an inline nullary contract.
                exit_contract = Some(exits[0].name.clone());
            }
        }
        if !exit_origins.is_empty() && exit_contract.is_none() {
            return Err(self.error("origin arguments require a named exit contract"));
        }
        let body = if signature {
            self.need(";")?;
            vec![]
        } else {
            self.body()?
        };
        Ok(Region {
            return_origins,
            abstract_signature: false,
            type_parameters,
            type_bounds,
            type_bindings: Default::default(),
            public,
            span,
            end: self.tokens[self.pos - 1].span,
            name_span,
            name,
            parameters,
            exits,
            infer_exits,
            infer_payloads,
            exit_contract,
            exit_origins,
            modern,
            body,
        })
    }
    fn associated(&mut self, implementation: bool) -> Result<AssociatedType> {
        let span = self.here();
        let name = self.id()?;
        let value = if implementation {
            self.need("=")?;
            Some(self.ty()?)
        } else {
            None
        };
        let mut bounds = vec![];
        if self.eat(":") {
            if implementation {
                return Err(
                    self.error("declare associated bounds in the trait, not the implementation")
                );
            }
            loop {
                let at = self.here();
                let bound = self.id()?;
                if !["copy", "clone", "drop"].contains(&bound.as_str()) || bounds.contains(&bound) {
                    return Err(Diagnostic::new(
                        "E_BOUND",
                        at,
                        "associated bounds must be distinct sealed capabilities: copy, clone, drop",
                    ));
                }
                bounds.push(bound);
                if !self.eat("+") {
                    break;
                }
            }
        }
        self.need(";")?;
        Ok(AssociatedType {
            span,
            name,
            value,
            bounds,
        })
    }
    fn type_parameters(&mut self) -> Result<Vec<String>> {
        if !self.eat("<") {
            return Ok(vec![]);
        }
        let mut parameters = vec![self.id()?];
        while self.eat(",") {
            parameters.push(self.id()?);
        }
        if self.at(":") {
            return Err(Diagnostic::new(
                "E_UNSUPPORTED",
                self.here(),
                "generic capability bounds are a later checkpoint",
            ));
        }
        self.need(">")?;
        Ok(parameters)
    }
    fn control_type_parameters(&mut self) -> Result<(Vec<String>, TypeBounds)> {
        let mut parameters = vec![];
        let mut bounds = std::collections::BTreeMap::new();
        if self.eat("<") {
            loop {
                let parameter = self.id()?;
                let mut requirements = vec![];
                if self.eat(":") {
                    loop {
                        let at = self.here();
                        let bound = self.qualified()?;
                        if requirements.contains(&bound) {
                            return Err(Diagnostic::new(
                                "E_BOUND",
                                at,
                                format!("duplicate capability `{bound}`"),
                            ));
                        }
                        requirements.push(bound);
                        if !self.eat("+") {
                            break;
                        }
                    }
                }
                bounds.insert(parameter.clone(), requirements);
                parameters.push(parameter);
                if !self.eat(",") {
                    break;
                }
            }
            self.need(">")?;
        }
        Ok((parameters, bounds))
    }
    fn schema_fields(&mut self) -> Result<Vec<Field>> {
        self.need("{")?;
        let mut fields = vec![];
        while !self.at("}") {
            let mut field = self.field()?;
            if self.eat("<-") {
                if !field.origins.is_empty()
                    && crate::manifest::reference_inner(&field.ty).is_none()
                {
                    return Err(self.error("arrow origins require a reference field"));
                }
                field.referent_origins = std::mem::take(&mut field.origins);
                self.need("'")?;
                field.origins.push(OriginArgument::Group(self.id()?));
            }
            fields.push(field);
            if !self.eat(",") {
                break;
            }
        }
        self.need("}")?;
        Ok(fields)
    }
    fn fields(&mut self) -> Result<Vec<Field>> {
        Ok(self.field_list(false)?.0)
    }
    fn exit_fields(&mut self) -> Result<(Vec<Field>, Vec<Option<String>>)> {
        self.field_list(true)
    }
    fn field_list(&mut self, origins_allowed: bool) -> Result<(Vec<Field>, Vec<Option<String>>)> {
        let end = if self.eat("{") {
            "}"
        } else {
            self.need("(")?;
            ")"
        };
        let mut fields = vec![];
        let mut origins = vec![];
        if !self.at(end) {
            loop {
                let field = self.field()?;
                if !origins_allowed && !field.origins.is_empty() {
                    return Err(self.error("origin bindings currently belong on struct fields or control result payloads"));
                }
                fields.push(field);
                origins.push(if origins_allowed && self.eat("from") {
                    Some(self.input_origin()?.display())
                } else {
                    None
                });
                if !self.eat(",") || self.at(end) {
                    break;
                }
            }
        }
        self.need(end)?;
        Ok((fields, origins))
    }
    fn mask(&mut self, level: u8) -> Result<Mask> {
        if level == 2 {
            return if self.eat("(") {
                let m = self.mask(0)?;
                self.need(")")?;
                Ok(m)
            } else {
                Ok(Mask::Name(self.qualified()?))
            };
        }
        let mut left = self.mask(level + 1)?;
        let op = if level == 0 { "|" } else { "&" };
        while self.eat(op) {
            let right = self.mask(level + 1)?;
            left = if level == 0 {
                Mask::Or(Box::new(left), Box::new(right))
            } else {
                Mask::And(Box::new(left), Box::new(right))
            };
        }
        Ok(left)
    }
    fn args(&mut self) -> Result<Vec<Expr>> {
        let end = if self.eat("{") {
            "}"
        } else {
            self.need("(")?;
            ")"
        };
        let previous = self.brace_expr;
        self.brace_expr = true;
        let mut args = vec![];
        if !self.at(end) {
            loop {
                args.push(self.expr(0)?);
                if !self.eat(",") || self.at(end) {
                    break;
                }
            }
        }
        self.need(end)?;
        self.brace_expr = previous;
        Ok(args)
    }
    fn expr(&mut self, level: u8) -> Result<Expr> {
        let start = self.here();
        if level == 3 {
            if self.eat("borrow") || self.eat("&") {
                let mutable = self.eat("mutable");
                let value = Box::new(self.expr(3)?);
                return Ok((if mutable {
                    ExprKind::BorrowMutable(value)
                } else {
                    ExprKind::Borrow(value)
                })
                .at(self.range_from(start)));
            }
            if self.eat("deref") || self.eat("*") {
                let value = self.expr(3)?;
                return Ok(ExprKind::Deref(Box::new(value)).at(self.range_from(start)));
            }
            if self.eat("!") {
                let value = self.expr(3)?;
                return Ok(ExprKind::Unary(Box::new(value)).at(self.range_from(start)));
            }
            let mut e = self.primary()?;
            while self.eat(".") {
                e = ExprKind::Project(Box::new(e), self.id()?).at(self.range_from(start));
            }
            return Ok(e);
        }
        let mut left = self.expr(level + 1)?;
        let ops: &[&str] = match level {
            0 => &["||"],
            1 => &["&&"],
            _ => &["==", "!=", "<", "<=", ">", ">="],
        };
        let mut used = false;
        while let Some(op) = ops.iter().find(|op| self.at(op)) {
            if level == 2 && used {
                return Err(self.error("comparisons cannot chain"));
            }
            let op = op.to_string();
            self.pos += 1;
            let right = self.expr(level + 1)?;
            left = ExprKind::Binary(op, Box::new(left), Box::new(right)).at(self.range_from(start));
            used = true;
        }
        Ok(left)
    }
    fn primary(&mut self) -> Result<Expr> {
        let start = self.here();
        let kind = self.primary_kind()?;
        Ok(kind.at(self.range_from(start)))
    }
    fn range_from(&self, start: Span) -> SourceRange {
        SourceRange {
            start,
            end: self.tokens[self.pos.saturating_sub(1)].end,
        }
    }
    fn primary_kind(&mut self) -> Result<ExprKind> {
        if self.eat("[") {
            let previous = self.brace_expr;
            self.brace_expr = true;
            let mut values = vec![];
            if !self.at("]") {
                loop {
                    values.push(self.expr(0)?);
                    if !self.eat(",") || self.at("]") {
                        break;
                    }
                }
            }
            self.need("]")?;
            self.brace_expr = previous;
            return Ok(ExprKind::List(values));
        }
        if self.eat("(") {
            let previous = self.brace_expr;
            self.brace_expr = true;
            let e = self.expr(0)?;
            self.need(")")?;
            self.brace_expr = previous;
            return Ok(e.kind);
        }
        let token = self.tokens[self.pos].clone();
        match token.kind {
            Kind::String(s) => {
                self.pos += 1;
                Ok(ExprKind::String(s))
            }
            Kind::Number(n) => {
                self.pos += 1;
                let digits = n.strip_suffix("u32").unwrap_or(&n);
                let n = digits.parse().map_err(|_| {
                    Diagnostic::new(
                        "E_LITERAL",
                        token.span,
                        "invalid or overflowing U32 literal",
                    )
                })?;
                Ok(ExprKind::U32(n))
            }
            Kind::Word(_) => {
                let saved = self.pos;
                let mut name = self.qualified()?;
                // Generic constructors are recognized only before braces. Ordinary
                // comparisons retain their existing parse, including `a < b`.
                if self.at("<") {
                    self.pos = saved;
                    if let Ok(ty) = self.ty() {
                        let mut candidate = ty;
                        if self.eat(".") {
                            candidate.push('.');
                            candidate.push_str(&self.id()?);
                        }
                        if (self.brace_expr && self.at("{")) || self.at("(") {
                            name = candidate;
                        } else {
                            self.pos = saved;
                            name = self.qualified()?;
                        }
                    } else {
                        self.pos = saved;
                        name = self.qualified()?;
                    }
                }
                match name.as_str() {
                    "true" => Ok(ExprKind::Bool(true)),
                    "false" => Ok(ExprKind::Bool(false)),
                    "unit" => Ok(ExprKind::Unit),
                    "bytes" if matches!(self.tokens[self.pos].kind, Kind::String(_)) => {
                        let Kind::String(s) = self.tokens[self.pos].kind.clone() else {
                            unreachable!()
                        };
                        self.pos += 1;
                        if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
                            return Err(Diagnostic::new(
                                "E_LITERAL",
                                token.span,
                                "invalid hexadecimal byte literal",
                            ));
                        }
                        Ok(ExprKind::Bytes(
                            (0..s.len())
                                .step_by(2)
                                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                                .collect(),
                        ))
                    }
                    _ => {
                        if self.at("(") {
                            Ok(ExprKind::Call(name, self.args()?))
                        } else if self.brace_expr && self.at("{") {
                            if self.tokens.get(self.pos + 2).is_some_and(|t| t.is(":")) {
                                self.need("{")?;
                                let mut fields = vec![];
                                loop {
                                    let field = self.id()?;
                                    self.need(":")?;
                                    fields.push((field, self.expr(0)?));
                                    if !self.eat(",") || self.at("}") {
                                        break;
                                    }
                                }
                                self.need("}")?;
                                Ok(ExprKind::NamedRecord(name, fields))
                            } else {
                                Ok(ExprKind::Construct(name, self.args()?))
                            }
                        } else {
                            Ok(ExprKind::Name(name))
                        }
                    }
                }
            }
            _ => Err(self.error("expected expression")),
        }
    }
    fn outcome_expr(&mut self) -> Result<OutcomeExpr> {
        let span = self.here();
        if self.at("forward")
            && !self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|t| t.is("{") || t.is("("))
        {
            self.pos += 1;
            return Ok(OutcomeExpr::Forward(span));
        }
        if self.eat("delta") || self.eat("Δ") {
            self.brace_expr = false;
            let operand = self.expr(0)?;
            self.brace_expr = true;
            self.need("{")?;
            let mut arms = vec![];
            while !self.at("}") {
                let arm_span = self.here();
                let outcome = self.qualified()?;
                let bindings = if self.at("{") { self.fields()? } else { vec![] };
                self.need("=>")?;
                let atom = if self.eat("[") {
                    let name = self.id()?;
                    self.need("]")?;
                    self.need(":")?;
                    name
                } else {
                    outcome.rsplit('.').next().unwrap().into()
                };
                let result = self.outcome_expr()?;
                arms.push(InlineArm {
                    span: arm_span,
                    pattern: Arm {
                        outcome,
                        bindings,
                        atom,
                    },
                    result,
                });
                if !self.eat(",") {
                    break;
                }
            }
            self.need("}")?;
            Ok(OutcomeExpr::Delta(span, operand, arms))
        } else {
            let name = self.id()?;
            if !self.at("{") {
                return Err(self.error("outcome expressions use braces, including empty {}"));
            }
            Ok(OutcomeExpr::Construct(span, name, self.args()?))
        }
    }
    fn statement(&mut self) -> Result<Statement> {
        if [
            "async", "await", "spawn", "defer", "capture", "vary", "flux",
        ]
        .iter()
        .any(|word| self.at(word))
        {
            return Err(Diagnostic::new(
                "E_UNSUPPORTED",
                self.here(),
                "this spelling is reserved for a later language revision",
            ));
        }
        let stmt =
            if self.at("forward") && !self.tokens.get(self.pos + 1).is_some_and(|t| t.is("(")) {
                self.pos += 1;
                Statement::Forward(self.expr(0)?)
            } else if self.eat("drop") {
                if !self.at("(") {
                    return Err(self.error("drop arguments use parentheses"));
                }
                let args = self.args()?;
                if args.len() != 1 {
                    return Err(self.error("drop requires one owned value"));
                }
                Statement::Drop(args.into_iter().next().unwrap())
            } else if self.eat("emit") {
                if !self.at("(") {
                    return Err(self.error("emit arguments use parentheses"));
                }
                let args = self.args()?;
                if args.len() != 1 {
                    return Err(self.error("emit requires one expression"));
                }
                Statement::Emit(args.into_iter().next().unwrap())
            } else if self.eat("exit") {
                if self.at("delta") || self.at("Δ") {
                    let expression = self.outcome_expr()?;
                    self.need(";")?;
                    return Ok(Statement::ExitOutcome(expression));
                }
                let name = self.id()?;
                let args = if self.at("(") || self.at("{") {
                    self.args()?
                } else {
                    self.need(";")?;
                    return Ok(Statement::ExitName(name));
                };
                Statement::Exit(name, args)
            } else if self.eat("again") {
                let name = self.id()?;
                if !self.at("(") {
                    return Err(self.error("restart arguments use parentheses"));
                }
                Statement::Again(name, self.args()?)
            } else if !self.at("delta")
                && !self.at("Δ")
                && !self.at("borrow")
                && !self.at("deref")
                && matches!(&self.tokens[self.pos].kind, Kind::Word(_))
                && self.tokens.get(self.pos + 1).is_some_and(|t| t.is("("))
            {
                Statement::Invoke(self.expr(0)?)
            } else {
                let site = if self.at("Δ") || self.at("delta") {
                    None
                } else if self.tokens.get(self.pos + 1).is_some_and(|t| t.is(":=")) {
                    let name = self.id()?;
                    self.need(":=")?;
                    let e = self.expr(0)?;
                    self.need(";")?;
                    return Ok(Statement::Bind(name, None, e, false));
                } else if self.tokens.get(self.pos + 1).is_some_and(|t| t.is("="))
                    && self
                        .tokens
                        .get(self.pos + 2)
                        .is_some_and(|t| t.is("delta") || t.is("Δ"))
                {
                    let name = self.id()?;
                    self.need("=")?;
                    Some(name)
                } else {
                    let mutable = self.eat("mutable");
                    if !mutable {
                        let saved = self.pos;
                        if let Ok(target) = self.expr(3) {
                            if self.eat("=") {
                                let value = self.expr(0)?;
                                self.need(";")?;
                                return Ok(Statement::Assign(target, value));
                            }
                            if matches!(target.kind, ExprKind::Call(..)) && self.eat(";") {
                                return Ok(Statement::Invoke(target));
                            }
                        }
                        self.pos = saved;
                    }
                    let ty = self.ty()?;
                    let name = self.id()?;
                    self.need("=")?;
                    if self.at("delta") || self.at("Δ") {
                        let expression = self.outcome_expr()?;
                        self.need(";")?;
                        return Ok(Statement::BindOutcome(
                            name,
                            (ty != "auto").then_some(ty),
                            expression,
                            mutable,
                        ));
                    }
                    if self.eat("capture") {
                        let operation = self.expr(0)?;
                        self.need(";")?;
                        return Ok(Statement::Capture(
                            name,
                            (ty != "auto").then_some(ty),
                            operation,
                            mutable,
                        ));
                    }
                    let expression = self.expr(0)?;
                    self.need(";")?;
                    return Ok(Statement::Bind(
                        name,
                        (ty != "auto").then_some(ty),
                        expression,
                        mutable,
                    ));
                };
                if !self.eat("Δ") {
                    self.need("delta")?;
                }
                self.brace_expr = false;
                let operand = self.expr(0)?;
                self.brace_expr = true;
                let modern = !self.eat("->");
                let end = if modern {
                    self.need("{")?;
                    "}"
                } else {
                    self.need("[")?;
                    "]"
                };
                let mut arms = vec![];
                while !self.at(end) {
                    let outcome = self.qualified()?;
                    let mut bindings = vec![];
                    if self.at("{") {
                        bindings = self.fields()?;
                    } else if self.eat("(") {
                        while !self.at(")") {
                            // Compatibility patterns may omit the type. New brace patterns may not.
                            if self
                                .tokens
                                .get(self.pos + 1)
                                .is_some_and(|t| t.is(",") || t.is(")"))
                            {
                                bindings.push(Field {
                                    type_span: Span::default(),
                                    origins: vec![],
                                    referent_origins: vec![],
                                    name: self.id()?,
                                    ty: "auto".into(),
                                });
                            } else {
                                bindings.push(self.ordinary_field()?);
                            }
                            if !self.eat(",") {
                                break;
                            }
                        }
                        self.need(")")?;
                    }
                    self.need("=>")?;
                    arms.push(Arm {
                        outcome,
                        bindings,
                        atom: self.id()?,
                    });
                    if !self.eat(",") {
                        break;
                    }
                }
                self.need(end)?;
                if modern {
                    self.eat(";");
                    return Ok(Statement::Delta(site, operand, arms));
                }
                Statement::Delta(site, operand, arms)
            };
        self.need(";")?;
        Ok(stmt)
    }
    fn body(&mut self) -> Result<Vec<Item>> {
        self.need("{")?;
        let mut items = vec![];
        let mut initial = true;
        while !self.at("}") {
            let span = self.here();
            if self.eat("path") {
                let name = self.id()?;
                self.need("=")?;
                let mask = self.mask(0)?;
                self.need(";")?;
                items.push(Item::Alias(span, name, mask));
            } else if self.eat("loop") {
                let name = self.id()?;
                self.need("(")?;
                let mut parameters = vec![];
                if !self.at(")") {
                    loop {
                        let field = self.ordinary_field()?;
                        self.need("=")?;
                        parameters.push(LoopParameter {
                            field,
                            initial: self.expr(0)?,
                        });
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.need(")")?;
                items.push(Item::Loop(span, name, parameters, self.body()?));
            } else {
                let mask = if self.eat("[") {
                    initial = false;
                    let mask = self.mask(0)?;
                    self.need("]")?;
                    mask
                } else if initial {
                    let mut statements = vec![];
                    while !self.at("[") && !self.at("}") && !self.at("path") && !self.at("loop") {
                        let span = self.here();
                        statements.push((span, self.statement()?));
                    }
                    items.push(Item::Guard(Mask::Name("entry".into()), statements));
                    continue;
                } else {
                    return Err(self.error("expected a path selector; use [entry]: to resume the enclosing entry selection"));
                };
                if self.eat("over") {
                    self.need("&")?;
                    self.need(":")?;
                    items.push(Item::Over(span, mask, self.body()?));
                } else {
                    self.need(":")?;
                    if self.at("{") {
                        items.push(Item::Group(span, mask, self.body()?));
                        // The preceding flat label ended at this label. A closed
                        // group resumes this body's entry, not that earlier label.
                        initial = true;
                        continue;
                    }
                    let mut statements = vec![];
                    while !self.at("[") && !self.at("}") && !self.at("path") && !self.at("loop") {
                        let span = self.here();
                        statements.push((span, self.statement()?));
                    }
                    if statements.is_empty() {
                        return Err(self.error("empty guarded item"));
                    }
                    items.push(Item::Guard(mask, statements));
                }
            }
        }
        self.need("}")?;
        Ok(items)
    }
}
pub fn parse(source: &str) -> Result<Vec<Region>> {
    Ok(parse_unit(source)?.regions)
}
pub fn parse_unit(source: &str) -> Result<Unit> {
    let mut p = Parser {
        tokens: lex(source)?,
        pos: 0,
        brace_expr: true,
    };
    let mut regions = vec![];
    let mut contracts = vec![];
    let mut data = vec![];
    let mut imports = vec![];
    let mut traits = vec![];
    let mut implementations = vec![];
    while !matches!(p.tokens[p.pos].kind, Kind::End) {
        let span = p.here();
        if p.eat("import") {
            let Kind::String(path) = p.tokens[p.pos].kind.clone() else {
                return Err(p.error("import expects a quoted relative file path"));
            };
            p.pos += 1;
            p.need("as")?;
            let alias = p.id()?;
            p.need(";")?;
            imports.push(Import { span, path, alias });
            continue;
        }
        let public = p.eat("pub");
        if p.eat("trait") {
            let name_span = p.here();
            let name = p.id()?;
            p.need("{")?;
            let mut associated = vec![];
            let mut methods = vec![];
            while !p.at("}") {
                if p.eat("type") {
                    associated.push(p.associated(false)?);
                } else {
                    let at = p.here();
                    methods.push(p.control(at, false, true)?);
                }
            }
            p.need("}")?;
            traits.push(TraitDeclaration {
                span,
                name_span,
                end: p.tokens[p.pos - 1].span,
                name,
                public,
                module: 0,
                associated,
                methods,
            });
            continue;
        }
        if p.eat("implement") || p.eat("impl") {
            if public {
                return Err(p.error(
                    "implementations do not use pub; trait and type visibility govern access",
                ));
            }
            let trait_name = p.qualified()?;
            p.need("for")?;
            let target = p.ty()?;
            p.need("{")?;
            let mut associated = vec![];
            let mut methods = vec![];
            while !p.at("}") {
                if p.eat("type") {
                    associated.push(p.associated(true)?);
                } else {
                    let at = p.here();
                    methods.push(p.control(at, false, false)?);
                }
            }
            p.need("}")?;
            implementations.push(Implementation {
                span,
                end: p.tokens[p.pos - 1].span,
                trait_name,
                target,
                module: 0,
                associated,
                methods,
            });
            continue;
        }
        if p.at("struct") || p.at("variant") {
            let record = p.eat("struct");
            if !record {
                p.need("variant")?;
            }
            let name_span = p.here();
            let name = p.id()?;
            let type_parameters = p.type_parameters()?;
            let origin_parameters = p
                .origin_arguments()?
                .into_iter()
                .map(|a| match a {
                    OriginArgument::Group(n) => Ok(n),
                    _ => Err(p.error("origin parameters use apostrophe names, such as 'a")),
                })
                .collect::<Result<Vec<_>>>()?;
            p.need("{")?;
            let mut public_fields = std::collections::BTreeSet::new();
            let body = if record {
                let mut fields = vec![];
                while !p.at("}") {
                    let visible = p.eat("pub");
                    let mut field = p.field()?;
                    if p.eat("<-") {
                        if !field.origins.is_empty()
                            && crate::manifest::reference_inner(&field.ty).is_none()
                        {
                            return Err(p.error("arrow origins require a reference field"));
                        }
                        field.referent_origins = std::mem::take(&mut field.origins);
                        p.need("'")?;
                        field.origins.push(OriginArgument::Group(p.id()?));
                    }
                    if visible {
                        public_fields.insert(field.name.clone());
                    }
                    fields.push(field);
                    p.need(";")?;
                }
                DataBody::Struct(fields)
            } else {
                let mut alternatives = vec![];
                while !p.at("}") {
                    let name = p.id()?;
                    let fields = if p.at("{") {
                        p.schema_fields()?
                    } else {
                        vec![]
                    };
                    alternatives.push(Outcome { name, fields });
                    if !p.eat(",") {
                        break;
                    }
                }
                DataBody::Variant(alternatives)
            };
            p.need("}")?;
            data.push(DataDeclaration {
                origin_parameters,
                type_parameters,
                public,
                public_fields,
                span,
                name_span,
                end: p.tokens[p.pos - 1].span,
                name,
                body,
            });
            p.eat(";");
            continue;
        }
        if p.eat("outcome") {
            let name_span = p.here();
            let name = p.id()?;
            let type_parameters = p.type_parameters()?;
            let origin_parameters = p
                .origin_arguments()?
                .into_iter()
                .map(|a| match a {
                    OriginArgument::Group(n) => Ok(n),
                    _ => Err(p.error("origin parameters use apostrophe names, such as 'a")),
                })
                .collect::<Result<Vec<_>>>()?;
            p.need("{")?;
            let mut outcomes = vec![];
            let mut exit_codes = std::collections::BTreeMap::new();
            while !p.at("}") {
                let annotation = if p.at("@") {
                    let at = p.here();
                    p.need("@")?;
                    let invalid = || {
                        Diagnostic::new(
                            "E_EXIT_CODE",
                            at,
                            "an outcome alternative annotation must be `@exit N`, with a decimal status from 0 through 255",
                        )
                    };
                    if !p.eat("exit") {
                        return Err(invalid());
                    }
                    let Kind::Number(number) = &p.tokens[p.pos].kind else {
                        return Err(invalid());
                    };
                    let code = number.parse::<u8>().map_err(|_| invalid())?;
                    p.pos += 1;
                    if p.at("@") {
                        return Err(Diagnostic::new(
                            "E_EXIT_CODE",
                            p.here(),
                            "an outcome alternative can have only one @exit annotation",
                        ));
                    }
                    Some(code)
                } else {
                    None
                };
                let name = p.id()?;
                if let Some(code) = annotation {
                    exit_codes.insert(name.clone(), code);
                }
                let fields = if p.at("{") {
                    p.schema_fields()?
                } else {
                    vec![]
                };
                outcomes.push(Outcome { name, fields });
                if !p.eat(",") {
                    break;
                }
            }
            p.need("}")?;
            contracts.push(OutcomeContract {
                exit_codes,
                origin_parameters,
                type_parameters,
                public,
                span,
                name_span,
                end: p.tokens[p.pos - 1].span,
                name,
                outcomes,
            });
            p.eat(";");
            continue;
        }
        if p.at("async") {
            return Err(Diagnostic::new(
                "E_UNSUPPORTED",
                p.here(),
                "async controls are specified in the revision roadmap but not implemented yet",
            ));
        }
        regions.push(p.control(span, public, false)?);
    }
    if regions.is_empty()
        && contracts.is_empty()
        && data.is_empty()
        && imports.is_empty()
        && traits.is_empty()
        && implementations.is_empty()
    {
        return Err(p.error("expected at least one control"));
    }
    Ok(Unit {
        trait_types: Default::default(),
        traits,
        implementations,
        trait_checks: vec![],
        imports,
        regions,
        contracts,
        data,
    })
}

/// Find the actual inference keyword in a checked declaration's type. Lexing
/// keeps comments, strings and Unicode positions out of the matching logic.
pub(crate) fn auto_binding_spans(source: &str) -> Result<Vec<(Span, Span, String)>> {
    let tokens = lex(source)?;
    let mut result = vec![];
    for (start, token) in tokens.iter().enumerate() {
        let mut index = start;
        while tokens[index].is("mutable") || tokens[index].is("ref") || tokens[index].is("&") {
            index += 1;
        }
        if tokens[index].is("auto")
            && let Some(Token {
                kind: Kind::Word(name),
                ..
            }) = tokens.get(index + 1)
        {
            result.push((token.span, tokens[index].span, name.clone()));
        }
    }
    Ok(result)
}

pub(crate) fn auto_exit_spans(source: &str) -> Result<Vec<Span>> {
    Ok(lex(source)?
        .windows(2)
        .filter(|pair| pair[0].is("exits") && pair[1].is("auto"))
        .map(|pair| pair[1].span)
        .collect())
}
