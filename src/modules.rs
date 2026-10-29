//! File modules: explicit imports, nominal identities, typed privacy checks and
//! source maps. Files are resolved relative to their importer, never by search path.
use crate::{
    CheckedProgram, Diagnostic, Span,
    manifest::Manifest,
    syntax::{self, DataBody, Expr, ExprKind, Item, OutcomeExpr, Statement},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};
const PREFIX: &str = "_path_m_";
#[derive(Clone, Debug, serde::Serialize)]
pub struct SourceFile {
    pub path: PathBuf,
    /// One-based start in the compiler's combined source-coordinate space.
    pub start_line: usize,
    pub line_count: usize,
    /// Internal declaration identity -> original source spelling.
    pub names: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct ModuleError {
    pub path: PathBuf,
    pub diagnostic: Diagnostic,
}
impl std::fmt::Display for ModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.path.display(), self.diagnostic)
    }
}
impl std::error::Error for ModuleError {}
type Result<T> = std::result::Result<T, ModuleError>;
fn error(path: &Path, span: Span, code: &'static str, message: impl Into<String>) -> ModuleError {
    ModuleError {
        path: path.into(),
        diagnostic: Diagnostic::new(code, span, message),
    }
}
#[derive(Clone, Default)]
pub(crate) struct Visibility {
    controls: BTreeMap<String, usize>,
    types: BTreeMap<String, usize>,
    fields: BTreeMap<String, BTreeSet<String>>,
    all_fields: BTreeMap<String, BTreeSet<String>>,
}
impl Visibility {
    pub(crate) fn owns(&self, module: usize, ty: &str) -> bool {
        let base = crate::collection_support::generic(ty).map_or(ty, |(base, _)| base);
        self.types.get(base) == Some(&module)
    }
    pub(crate) fn method(&mut self, name: String, module: usize) {
        self.controls.insert(name, module);
    }

    fn external(&self, caller: &str, ty: &str) -> bool {
        let caller = crate::collection_support::generic(caller).map_or(caller, |(base, _)| base);
        let ty = crate::collection_support::generic(ty).map_or(ty, |(base, _)| base);
        self.types
            .get(ty)
            .is_some_and(|owner| self.controls.get(caller) != Some(owner))
    }
    pub(crate) fn field(
        &self,
        caller: &str,
        ty: &str,
        name: &str,
        span: Span,
    ) -> crate::Result<()> {
        let ty = crate::collection_support::generic(ty).map_or(ty, |(base, _)| base);
        if self.external(caller, ty) && !self.fields.get(ty).is_some_and(|fs| fs.contains(name)) {
            return Err(Diagnostic::new(
                "E_VISIBILITY",
                span,
                format!("field `{name}` of `{ty}` is private to its module"),
            ));
        }
        Ok(())
    }
    pub(crate) fn construct(&self, caller: &str, ty: &str, span: Span) -> crate::Result<()> {
        let ty = crate::collection_support::generic(ty).map_or(ty, |(base, _)| base);
        if self.external(caller, ty)
            && self
                .all_fields
                .get(ty)
                .is_some_and(|fs| self.fields.get(ty) != Some(fs))
        {
            return Err(Diagnostic::new(
                "E_VISIBILITY",
                span,
                format!(
                    "cannot construct `{ty}` outside its module: it has private fields; use a public control"
                ),
            ));
        }
        Ok(())
    }
    pub(crate) fn constructor_visible(&self, caller: &str, ty: &str, name: &str) -> bool {
        !self.external(caller, ty) || name == ty || name.starts_with(&format!("{ty}."))
    }
}
#[derive(Clone)]
struct Symbol {
    global: String,
    public: bool,
    is_type: bool,
}
struct File {
    path: PathBuf,
    unit: syntax::Unit,
    lines: usize,
    imports: BTreeMap<String, usize>,
    symbols: BTreeMap<String, Symbol>,
}
struct Loader<F> {
    read: F,
    files: Vec<File>,
    ids: BTreeMap<PathBuf, usize>,
    active: BTreeSet<PathBuf>,
    prefixes: BTreeMap<String, PathBuf>,
    bytes: usize,
}
impl<F: FnMut(&Path) -> std::io::Result<String>> Loader<F> {
    fn load(&mut self, path: &Path, importer: Option<(&Path, Span)>) -> Result<usize> {
        let at = importer.map_or(Span { line: 1, column: 1 }, |(_, s)| s);
        let origin = importer.map_or(path, |(p, _)| p);
        let path = if importer.is_none() {
            source_identity(path)
        } else {
            std::fs::canonicalize(path)
        }
        .map_err(|e| {
            error(
                origin,
                at,
                "E_IMPORT",
                format!("cannot resolve {}: {e}", path.display()),
            )
        })?;
        if self.active.contains(&path) {
            return Err(error(
                origin,
                at,
                "E_IMPORT_CYCLE",
                format!("cyclic import of {}", path.display()),
            ));
        }
        if let Some(&id) = self.ids.get(&path) {
            return Ok(id);
        }
        if self.files.len() >= 256 {
            return Err(error(
                origin,
                at,
                "E_IMPORT_LIMIT",
                "module graph exceeds 256 files",
            ));
        }
        let source = (self.read)(&path).map_err(|e| {
            error(
                origin,
                at,
                "E_IMPORT",
                format!("cannot read {}: {e}", path.display()),
            )
        })?;
        self.bytes += source.len();
        if self.bytes > 8 * 1024 * 1024 {
            return Err(error(
                origin,
                at,
                "E_IMPORT_LIMIT",
                "module sources exceed 8 MiB",
            ));
        }
        let unit = syntax::parse_unit(&source).map_err(|diagnostic| ModuleError {
            path: path.clone(),
            diagnostic,
        })?;
        let id = self.files.len();
        let prefix = if id == 0 {
            String::new()
        } else {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            path.hash(&mut hash);
            let prefix = format!("{PREFIX}{:016x}_", hash.finish());
            if let Some(previous) = self.prefixes.insert(prefix.clone(), path.clone())
                && previous != path
            {
                return Err(error(&path, at, "E_IMPORT", "module identity collision"));
            }
            prefix
        };
        let mut symbols = BTreeMap::new();
        for (name, public, is_type, span) in unit
            .regions
            .iter()
            .map(|d| (&d.name, d.public, false, d.span))
            .chain(unit.data.iter().map(|d| (&d.name, d.public, true, d.span)))
            .chain(
                unit.traits
                    .iter()
                    .map(|d| (&d.name, d.public, true, d.span)),
            )
            .chain(
                unit.contracts
                    .iter()
                    .map(|d| (&d.name, d.public, true, d.span)),
            )
        {
            if name.starts_with(PREFIX) || symbols.contains_key(name) {
                return Err(error(
                    &path,
                    span,
                    "E_NAME",
                    format!("reserved or duplicate module declaration `{name}`"),
                ));
            }
            symbols.insert(
                name.clone(),
                Symbol {
                    global: format!("{prefix}{name}"),
                    public,
                    is_type,
                },
            );
        }
        let imports = unit.imports.clone();
        self.ids.insert(path.clone(), id);
        self.active.insert(path.clone());
        self.files.push(File {
            path: path.clone(),
            unit,
            lines: source.lines().count().max(1),
            imports: BTreeMap::new(),
            symbols,
        });
        for import in imports {
            if import.alias.starts_with(PREFIX)
                || self.files[id].imports.contains_key(&import.alias)
                || self.files[id].symbols.contains_key(&import.alias)
            {
                return Err(error(
                    &path,
                    import.span,
                    "E_NAME",
                    format!("reserved or duplicate import namespace `{}`", import.alias),
                ));
            }
            if Path::new(&import.path).is_absolute() || import.path.is_empty() {
                return Err(error(
                    &path,
                    import.span,
                    "E_IMPORT",
                    "import paths must be nonempty relative file paths",
                ));
            }
            let target = self.load(
                &path.parent().unwrap().join(&import.path),
                Some((&path, import.span)),
            )?;
            self.files[id].imports.insert(import.alias, target);
        }
        self.active.remove(&path);
        Ok(id)
    }
}
pub(crate) fn source_identity(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).or_else(|e| {
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            return std::fs::canonicalize(if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            })
            .map(|p| p.join(name));
        }
        Err(e)
    })
}
/// Compile an entry file and its explicitly imported dependency graph.
pub fn compile_file(path: impl AsRef<Path>, manifest: Manifest) -> Result<CheckedProgram> {
    compile_with_loader(path, manifest, |p| std::fs::read_to_string(p))
}
/// Check an entry file and its imports, collecting independent body errors.
pub fn check_file(
    path: impl AsRef<Path>,
    manifest: Manifest,
) -> crate::CheckResult<CheckedProgram, ModuleError> {
    check_with_loader(path, manifest, |p| std::fs::read_to_string(p))
}
/// Editor entry point: the loader may supply unsaved buffers for canonical paths.
/// Import targets must already exist so aliases and symlinks have one file identity.
pub fn compile_with_loader(
    path: impl AsRef<Path>,
    manifest: Manifest,
    read: impl FnMut(&Path) -> std::io::Result<String>,
) -> Result<CheckedProgram> {
    check_with_loader(path, manifest, read).map_err(crate::Diagnostics::first)
}
/// Diagnostic entry point with the same unsaved-buffer and file identity rules.
pub fn check_with_loader(
    path: impl AsRef<Path>,
    manifest: Manifest,
    read: impl FnMut(&Path) -> std::io::Result<String>,
) -> crate::CheckResult<CheckedProgram, ModuleError> {
    let mut loader = Loader {
        read,
        files: vec![],
        ids: BTreeMap::new(),
        active: BTreeSet::new(),
        prefixes: BTreeMap::new(),
        bytes: 0,
    };
    loader.load(path.as_ref(), None)?;
    resolve(loader.files, manifest)
}

struct Resolver<'a> {
    files: &'a [File],
    module: usize,
    locals: BTreeSet<String>,
    type_parameters: BTreeSet<String>,
    offset: usize,
}
impl Resolver<'_> {
    fn fail(&self, span: Span, code: &'static str, message: impl Into<String>) -> ModuleError {
        error(&self.files[self.module].path, span, code, message)
    }
    fn name(&self, name: &str, span: Span, kind: Option<bool>) -> Result<String> {
        if name.starts_with(PREFIX) {
            return Err(self.fail(
                span,
                "E_NAME",
                "compiler module identities cannot be spelled in source",
            ));
        }
        let (first, rest) = name.split_once('.').unwrap_or((name, ""));
        let current = &self.files[self.module];
        if let Some(&target) = current.imports.get(first) {
            if rest.is_empty() {
                return Err(self.fail(
                    span,
                    "E_NAME",
                    format!("`{first}` is a namespace; qualify its public member"),
                ));
            }
            let (member, suffix) = rest.split_once('.').unwrap_or((rest, ""));
            let symbol = self.files[target].symbols.get(member).ok_or_else(|| {
                self.fail(
                    span,
                    "E_NAME",
                    format!("namespace `{first}` has no declaration `{member}`"),
                )
            })?;
            if !symbol.public {
                return Err(self.fail(
                    span,
                    "E_VISIBILITY",
                    format!("`{first}.{member}` is private"),
                ));
            }
            if kind.is_some_and(|expected| expected != symbol.is_type) {
                return Err(self.fail(
                    span,
                    "E_TYPE",
                    format!("`{first}.{member}` is not the required kind of declaration"),
                ));
            }
            return Ok(if suffix.is_empty() {
                symbol.global.clone()
            } else {
                format!("{}.{suffix}", symbol.global)
            });
        }
        if let Some(symbol) = current.symbols.get(first) {
            if kind.is_some_and(|expected| expected != symbol.is_type) {
                return Err(self.fail(
                    span,
                    "E_TYPE",
                    format!("`{first}` is not the required kind of declaration"),
                ));
            }
            return Ok(if rest.is_empty() {
                symbol.global.clone()
            } else {
                format!("{}.{rest}", symbol.global)
            });
        }
        Ok(name.into())
    }
    fn ty(&self, ty: &str, span: Span) -> Result<String> {
        if self.type_parameters.contains(ty) {
            return Ok(ty.into());
        }
        if let Some((mutable, inner)) = crate::manifest::reference_inner(ty) {
            return Ok(format!(
                "&{}{}",
                if mutable { "mutable " } else { "" },
                self.ty(inner, span)?
            ));
        }
        if let Some(producer) = crate::manifest::outcome_inner(ty) {
            return Ok(format!("outcome<{}>", self.constructor(producer, span)?));
        }
        if let Some((application, member)) = crate::collection_support::qualified_member(ty) {
            return Ok(format!("{}.{member}", self.ty(application, span)?));
        }
        if let Some((base, args)) = crate::collection_support::generic(ty) {
            let args = args
                .iter()
                .map(|a| {
                    if a.bytes().all(|b| b.is_ascii_digit()) {
                        Ok(a.to_string())
                    } else {
                        self.ty(a, span)
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let base = if self.type_parameters.contains(base) {
                base.to_string()
            } else {
                self.name(base, span, Some(true))?
            };
            return Ok(format!("{base}<{}>", args.join(",")));
        }
        self.name(ty, span, Some(true))
    }
    fn fields(&self, fields: &mut [syntax::Field], span: Span) -> Result<()> {
        for f in fields {
            f.ty = self.ty(&f.ty, span)?;
            if f.type_span.line != 0 {
                f.type_span.line += self.offset;
            }
        }
        Ok(())
    }
    fn expr(&self, e: &mut Expr, span: Span) -> Result<()> {
        let span = e.span(span);
        let result = (|| {
            match &mut e.kind {
                ExprKind::Name(n) => {
                    if !self.locals.contains(n.split('.').next().unwrap())
                        && !self.type_parameters.contains(n.split('.').next().unwrap())
                    {
                        *n = self.name(n, span, None)?;
                    }
                }
                ExprKind::Call(n, args) | ExprKind::Construct(n, args) => {
                    *n = self.constructor(n, span)?;
                    for a in args {
                        self.expr(a, span)?;
                    }
                }
                ExprKind::NamedRecord(n, fields) => {
                    *n = self.ty(n, span)?;
                    for (_, value) in fields {
                        self.expr(value, span)?;
                    }
                }
                ExprKind::List(args) => {
                    for a in args {
                        self.expr(a, span)?;
                    }
                }
                ExprKind::Borrow(e)
                | ExprKind::BorrowMutable(e)
                | ExprKind::Deref(e)
                | ExprKind::Project(e, _)
                | ExprKind::Unary(e) => self.expr(e, span)?,
                ExprKind::Binary(_, a, b) => {
                    self.expr(a, span)?;
                    self.expr(b, span)?;
                }
                _ => (),
            }
            Ok(())
        })();
        result.map_err(|mut error: ModuleError| {
            error.diagnostic = e.locate(error.diagnostic, span);
            error
        })?;
        if e.range.start.line != 0 {
            e.range.start.line += self.offset;
            e.range.end.line += self.offset;
        }
        Ok(())
    }
    fn constructor(&self, name: &str, span: Span) -> Result<String> {
        if self.type_parameters.contains(name) {
            return Ok(name.into());
        }
        if let Some(end) = name.rfind('>') {
            let (base, args) = crate::collection_support::generic(&name[..=end])
                .ok_or_else(|| self.fail(span, "E_GENERIC", "invalid generic application"))?;
            let base = if self.type_parameters.contains(base) {
                base.to_string()
            } else {
                self.name(base, span, None)?
            };
            let args = args
                .iter()
                .map(|a| self.ty(a, span))
                .collect::<Result<Vec<_>>>()?;
            let ty = format!("{base}<{}>", args.join(","));
            return Ok(format!("{ty}{}", &name[end + 1..]));
        }
        self.name(name, span, None)
    }
    fn method(&mut self, region: &mut syntax::Region) -> Result<()> {
        let declared = locals(region);
        for n in &declared {
            if self.files[self.module].imports.contains_key(n) {
                return Err(self.fail(
                    region.span,
                    "E_NAME",
                    format!("local `{n}` conflicts with a namespace"),
                ));
            }
        }
        self.locals = region.parameters.iter().map(|p| p.name.clone()).collect();
        self.fields(&mut region.parameters, region.span)?;
        for o in &mut region.exits {
            self.fields(&mut o.fields, region.span)?;
        }
        if let Some(c) = &mut region.exit_contract
            && (self.files[self.module]
                .symbols
                .get(c)
                .is_some_and(|s| s.is_type)
                || c.contains('.')
                || c.contains('<')
                || self.type_parameters.contains(c))
        {
            *c = self.ty(c, region.span)?;
        }
        self.items(&mut region.body)?;
        region.span.line += self.offset;
        region.name_span.line += self.offset;
        region.end.line += self.offset;
        Ok(())
    }
    fn pattern(&self, arm: &mut syntax::Arm, span: Span) -> Result<()> {
        if arm.outcome.contains('.') {
            arm.outcome = self.name(&arm.outcome, span, None)?;
        }
        self.fields(&mut arm.bindings, span)
    }
    fn outcome(&mut self, e: &mut OutcomeExpr) -> Result<()> {
        match e {
            OutcomeExpr::Forward(at) => at.line += self.offset,
            OutcomeExpr::Construct(at, n, args) => {
                if n.contains('.') {
                    *n = self.name(n, *at, None)?;
                }
                for a in args {
                    self.expr(a, *at)?;
                }
                at.line += self.offset;
            }
            OutcomeExpr::Delta(at, e, arms) => {
                self.expr(e, *at)?;
                for arm in arms {
                    self.pattern(&mut arm.pattern, arm.span)?;
                    let outer = self.locals.clone();
                    self.locals
                        .extend(arm.pattern.bindings.iter().map(|f| f.name.clone()));
                    self.outcome(&mut arm.result)?;
                    self.locals = outer;
                    arm.span.line += self.offset;
                }
                at.line += self.offset;
            }
        }
        Ok(())
    }
    fn items(&mut self, body: &mut [Item]) -> Result<()> {
        for item in body {
            match item {
                Item::Guard(_, ss) => {
                    for (at, s) in ss {
                        match s {
                            Statement::Bind(name, ty, e, _)
                            | Statement::Capture(name, ty, e, _) => {
                                if let Some(t) = ty {
                                    *t = self.ty(t, *at)?;
                                }
                                self.expr(e, *at)?;
                                self.locals.insert(name.clone());
                            }
                            Statement::BindOutcome(name, ty, e, _) => {
                                if let Some(t) = ty {
                                    *t = self.ty(t, *at)?;
                                }
                                self.outcome(e)?;
                                self.locals.insert(name.clone());
                            }
                            Statement::ExitOutcome(e) => self.outcome(e)?,
                            Statement::Forward(e)
                            | Statement::Invoke(e)
                            | Statement::Drop(e)
                            | Statement::Emit(e) => self.expr(e, *at)?,
                            Statement::Assign(a, b) => {
                                self.expr(a, *at)?;
                                self.expr(b, *at)?;
                            }
                            Statement::Delta(_, e, arms) => {
                                self.expr(e, *at)?;
                                for arm in arms {
                                    self.pattern(arm, *at)?;
                                    self.locals
                                        .extend(arm.bindings.iter().map(|f| f.name.clone()));
                                }
                            }
                            Statement::Exit(_, args) | Statement::Again(_, args) => {
                                for a in args {
                                    self.expr(a, *at)?;
                                }
                            }
                            Statement::ExitName(_) => (),
                        }
                        at.line += self.offset;
                    }
                }
                Item::Loop(at, _, ps, body) => {
                    for p in ps.iter_mut() {
                        p.field.ty = self.ty(&p.field.ty, *at)?;
                        self.expr(&mut p.initial, *at)?;
                    }
                    let outer = self.locals.clone();
                    self.locals.extend(ps.iter().map(|p| p.field.name.clone()));
                    self.items(body)?;
                    self.locals = outer;
                    at.line += self.offset;
                }
                Item::Group(at, _, body) => {
                    self.items(body)?;
                    at.line += self.offset;
                }
                Item::Over(at, _, body) => {
                    let outer = self.locals.clone();
                    self.items(body)?;
                    self.locals = outer;
                    at.line += self.offset;
                }
                Item::Alias(at, ..) => at.line += self.offset,
            }
        }
        Ok(())
    }
}
fn locals(region: &syntax::Region) -> BTreeSet<String> {
    fn outcome(e: &OutcomeExpr, names: &mut BTreeSet<String>) {
        if let OutcomeExpr::Delta(_, _, arms) = e {
            for a in arms {
                names.extend(a.pattern.bindings.iter().map(|f| f.name.clone()));
                outcome(&a.result, names);
            }
        }
    }
    fn items(body: &[Item], names: &mut BTreeSet<String>) {
        for item in body {
            match item {
                Item::Guard(_, ss) => {
                    for (_, s) in ss {
                        match s {
                            Statement::Bind(n, ..) | Statement::Capture(n, ..) => {
                                names.insert(n.clone());
                            }
                            Statement::BindOutcome(n, _, e, _) => {
                                names.insert(n.clone());
                                outcome(e, names);
                            }
                            Statement::ExitOutcome(e) => outcome(e, names),
                            Statement::Delta(_, _, arms) => names.extend(
                                arms.iter()
                                    .flat_map(|a| a.bindings.iter().map(|f| f.name.clone())),
                            ),
                            _ => (),
                        }
                    }
                }
                Item::Loop(_, _, ps, body) => {
                    names.extend(ps.iter().map(|p| p.field.name.clone()));
                    items(body, names);
                }
                Item::Group(_, _, body) | Item::Over(_, _, body) => items(body, names),
                _ => (),
            }
        }
    }
    let mut names = region.parameters.iter().map(|f| f.name.clone()).collect();
    items(&region.body, &mut names);
    names
}
fn type_leaves(ty: &str) -> Vec<String> {
    if let Some((_, inner)) = crate::manifest::reference_inner(ty) {
        return type_leaves(inner);
    }
    if crate::manifest::outcome_inner(ty).is_some() {
        return vec![ty.into()];
    }
    if let Some((base, args)) = crate::collection_support::generic(ty) {
        return std::iter::once(base.into())
            .chain(args.iter().flat_map(|t| type_leaves(t)))
            .collect();
    }
    vec![ty.into()]
}
fn resolve(
    files: Vec<File>,
    manifest: Manifest,
) -> crate::CheckResult<CheckedProgram, ModuleError> {
    let mut combined = syntax::Unit {
        trait_types: Default::default(),
        traits: vec![],
        implementations: vec![],
        trait_checks: vec![],
        imports: vec![],
        regions: vec![],
        contracts: vec![],
        data: vec![],
    };
    let mut visibility = Visibility::default();
    let mut sources = vec![];
    let mut offset = 0;
    for (id, file) in files.iter().enumerate() {
        let mut unit = file.unit.clone();
        let mut r = Resolver {
            files: &files,
            module: id,
            locals: BTreeSet::new(),
            type_parameters: BTreeSet::new(),
            offset,
        };
        for name in file.symbols.keys() {
            if [
                "Bool", "U32", "String", "Bytes", "Unit", "Option", "bool", "u32", "string",
                "bytes", "unit", "option", "auto", "array", "vector", "map", "slice", "outcome",
            ]
            .contains(&name.as_str())
            {
                return Err((r.fail(
                    Span { line: 1, column: 1 },
                    "E_NAME",
                    format!("cannot redeclare builtin `{name}`"),
                ))
                .into());
            }
        }
        for data in &mut unit.data {
            r.type_parameters = data.type_parameters.iter().cloned().collect();
            data.name = file.symbols[&data.name].global.clone();
            visibility.types.insert(data.name.clone(), id);
            match &mut data.body {
                DataBody::Struct(fs) => {
                    visibility
                        .fields
                        .insert(data.name.clone(), data.public_fields.clone());
                    visibility.all_fields.insert(
                        data.name.clone(),
                        fs.iter().map(|f| f.name.clone()).collect(),
                    );
                    r.fields(fs, data.span)?;
                }
                DataBody::Variant(os) => {
                    for o in os {
                        r.fields(&mut o.fields, data.span)?;
                    }
                }
            }
            data.span.line += offset;
            data.name_span.line += offset;
            data.end.line += offset;
        }
        for contract in &mut unit.contracts {
            r.type_parameters = contract.type_parameters.iter().cloned().collect();
            contract.name = file.symbols[&contract.name].global.clone();
            visibility.types.insert(contract.name.clone(), id);
            for o in &mut contract.outcomes {
                r.fields(&mut o.fields, contract.span)?;
            }
            contract.span.line += offset;
            contract.name_span.line += offset;
            contract.end.line += offset;
        }
        for declaration in &mut unit.traits {
            if ["copy", "clone", "drop", "self"].contains(&declaration.name.as_str()) {
                return Err((r.fail(declaration.span, "E_TRAIT", "trait name is reserved")).into());
            }
            declaration.name = file.symbols[&declaration.name].global.clone();
            declaration.module = id;
            r.type_parameters = declaration
                .associated
                .iter()
                .map(|a| a.name.clone())
                .chain(std::iter::once("self".into()))
                .collect();
            for method in &mut declaration.methods {
                r.method(method)?;
            }
            for a in &mut declaration.associated {
                a.span.line += offset;
            }
            declaration.span.line += offset;
            declaration.name_span.line += offset;
            declaration.end.line += offset;
        }
        for implementation in &mut unit.implementations {
            r.type_parameters.clear();
            implementation.trait_name =
                r.name(&implementation.trait_name, implementation.span, Some(true))?;
            implementation.target = r.ty(&implementation.target, implementation.span)?;
            implementation.module = id;
            r.type_parameters = implementation
                .associated
                .iter()
                .map(|a| a.name.clone())
                .chain(std::iter::once("self".into()))
                .collect();
            for a in &mut implementation.associated {
                if let Some(ty) = &mut a.value {
                    *ty = r.ty(ty, a.span)?;
                }
                a.span.line += offset;
            }
            for method in &mut implementation.methods {
                r.method(method)?;
            }
            implementation.span.line += offset;
            implementation.end.line += offset;
        }
        for region in &mut unit.regions {
            r.type_parameters = region.type_parameters.iter().cloned().collect();
            for bounds in region.type_bounds.values_mut() {
                for bound in bounds {
                    if !["copy", "clone", "drop"].contains(&bound.as_str()) {
                        *bound = r.name(bound, region.span, Some(true))?;
                    }
                }
            }
            let declared_locals = locals(region);
            r.locals = region.parameters.iter().map(|p| p.name.clone()).collect();
            for n in &declared_locals {
                if n.starts_with(PREFIX) || file.imports.contains_key(n) {
                    return Err((r.fail(
                        region.span,
                        "E_NAME",
                        format!("local `{n}` conflicts with a namespace or reserved compiler name"),
                    ))
                    .into());
                }
            }
            region.name = file.symbols[&region.name].global.clone();
            visibility.controls.insert(region.name.clone(), id);
            r.fields(&mut region.parameters, region.span)?;
            for o in &mut region.exits {
                r.fields(&mut o.fields, region.span)?;
            }
            if let Some(contract) = &mut region.exit_contract {
                // A bare nullary exit label is not necessarily a type declaration.
                if file.symbols.get(contract).is_some_and(|s| s.is_type)
                    || contract.contains('.')
                    || contract.contains('<')
                {
                    *contract = r.ty(contract, region.span)?;
                }
            }
            r.items(&mut region.body)?;
            region.span.line += offset;
            region.name_span.line += offset;
            region.end.line += offset;
        }
        combined.traits.extend(unit.traits);
        combined.implementations.extend(unit.implementations);
        combined.data.extend(unit.data);
        combined.contracts.extend(unit.contracts);
        combined.regions.extend(unit.regions);
        sources.push(SourceFile {
            path: file.path.clone(),
            start_line: offset + 1,
            line_count: file.lines,
            names: file
                .symbols
                .iter()
                .map(|(n, s)| (s.global.clone(), n.clone()))
                .collect(),
        });
        offset += file.lines + 1;
    }
    if manifest.types.iter().any(|t| t.name().starts_with(PREFIX))
        || manifest
            .primitives
            .iter()
            .any(|p| p.name.starts_with(PREFIX))
    {
        return Err(error(
            &files[0].path,
            Span { line: 1, column: 1 },
            "E_NAME",
            "manifest uses a reserved module identity",
        )
        .into());
    }
    let mut program = crate::check::compile_unit(combined, manifest, &visibility)
        .map_err(|errors| errors.map(|diagnostic| locate(&sources, diagnostic)))?;
    program.source_files = sources;
    Ok(program)
}
fn locate(sources: &[SourceFile], mut diagnostic: Diagnostic) -> ModuleError {
    let file = sources
        .iter()
        .rev()
        .find(|s| s.start_line <= diagnostic.span.line)
        .unwrap_or(&sources[0]);
    diagnostic.span.line = diagnostic
        .span
        .line
        .saturating_sub(file.start_line - 1)
        .max(1);
    if let Some(end) = &mut diagnostic.end {
        end.line = end.line.saturating_sub(file.start_line - 1).max(1);
    }
    for note in &mut diagnostic.notes {
        let source = sources
            .iter()
            .rev()
            .find(|s| s.start_line <= note.span.line)
            .unwrap_or(&sources[0]);
        note.path = Some(source.path.clone());
        note.span.line = note.span.line.saturating_sub(source.start_line - 1).max(1);
    }
    for source in sources {
        for (global, local) in &source.names {
            if global != local {
                if let Some(crate::DiagnosticWitness::StructuralMayReach { control, .. }) =
                    diagnostic.witness.as_deref_mut()
                {
                    *control =
                        control.replace(global, &format!("{}::{local}", source.path.display()));
                }
                for note in &mut diagnostic.notes {
                    note.message = note
                        .message
                        .replace(global, &format!("{}::{local}", source.path.display()));
                }
                diagnostic.message = diagnostic
                    .message
                    .replace(global, &format!("{}::{local}", source.path.display()));
            }
        }
    }
    ModuleError {
        path: file.path.clone(),
        diagnostic,
    }
}

pub(crate) struct PublicInterfaces {
    private: BTreeSet<String>,
    controls: Vec<(String, Span)>,
    fields: Vec<(String, Span, Vec<String>)>,
}
impl PublicInterfaces {
    pub(crate) fn new(unit: &syntax::Unit) -> Self {
        let private = unit
            .data
            .iter()
            .filter(|d| !d.public)
            .map(|d| d.name.clone())
            .chain(
                unit.contracts
                    .iter()
                    .filter(|d| !d.public)
                    .map(|d| d.name.clone()),
            )
            .collect();
        let controls = unit
            .regions
            .iter()
            .filter(|r| r.public)
            .map(|r| (r.name.clone(), r.span))
            .collect();
        let mut fields = vec![];
        for check in &unit.trait_checks {
            if check.public {
                fields.extend(
                    check
                        .associated
                        .iter()
                        .map(|t| (t.clone(), check.span, vec![])),
                );
            }
        }
        for data in &unit.data {
            if data.public {
                match &data.body {
                    DataBody::Struct(fs) => fields.extend(
                        fs.iter()
                            .filter(|f| data.public_fields.contains(&f.name))
                            .map(|f| (f.ty.clone(), data.span, data.type_parameters.clone())),
                    ),
                    DataBody::Variant(os) => fields.extend(
                        os.iter()
                            .flat_map(|o| &o.fields)
                            .map(|f| (f.ty.clone(), data.span, data.type_parameters.clone())),
                    ),
                }
            }
        }
        for contract in &unit.contracts {
            if contract.public {
                fields.extend(contract.outcomes.iter().flat_map(|o| &o.fields).map(|f| {
                    (
                        f.ty.clone(),
                        contract.span,
                        contract.type_parameters.clone(),
                    )
                }));
            }
        }
        // A named completion contract is part of the public signature even
        // when all of its payload types are themselves public.
        for region in unit.regions.iter().filter(|r| r.public) {
            if let Some(contract) = &region.exit_contract {
                let base = crate::collection_support::generic(contract)
                    .map_or(contract.as_str(), |(base, _)| base);
                if unit.contracts.iter().any(|c| c.name == base) {
                    fields.push((
                        contract.clone(),
                        region.span,
                        region.type_parameters.clone(),
                    ));
                }
            }
        }
        Self {
            private,
            controls,
            fields,
        }
    }
    pub(crate) fn check(&self, program: &CheckedProgram) -> crate::Result<()> {
        let check = |ty: &str, span: Span, parameters: &[String]| -> crate::Result<()> {
            for leaf in type_leaves(ty) {
                if parameters.contains(&leaf) {
                    continue;
                }
                if self.private.contains(&leaf) || crate::manifest::outcome_inner(&leaf).is_some() {
                    return Err(Diagnostic::new(
                        "E_VISIBILITY",
                        span,
                        format!(
                            "public interface exposes private or producer-indexed type `{leaf}`; use a public named type or outcome contract"
                        ),
                    ));
                }
            }
            Ok(())
        };
        for (ty, span, parameters) in &self.fields {
            check(ty, *span, parameters)?;
        }
        for (name, span) in &self.controls {
            let region = program.region(name).unwrap();
            for p in &region.parameters {
                check(&p.ty, *span, &[])?;
            }
            for f in region.exits.iter().flat_map(|o| &o.fields) {
                check(&f.ty, *span, &[])?;
            }
        }
        Ok(())
    }
}
