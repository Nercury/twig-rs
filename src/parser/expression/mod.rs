//! Expression parsers and their registry: `Twig\ExpressionParser\*` (TWIG/src/ExpressionParser/).
//!
//! Prefix parsers start an expression (unary operators, grouping, the literal fallback), infix parsers continue
//! one (binary operators, calls, attribute access, filters, tests, ternary, assignment, arrow functions).
//! [`Parser::parse_expression`](crate::parser::Parser::parse_expression) climbs precedences over the registry.

pub mod infix;
pub mod prefix;

use std::any::TypeId;
use std::sync::{Arc, OnceLock};

use crate::ast::Expr;
use crate::deprecation::Deprecation;
use crate::error::Result;
use crate::lexer::Token;
use crate::parser::Parser;

/// A precedence change planned for the next major version. Twig: `Twig\ExpressionParser\PrecedenceChange`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecedenceChange {
    package: String,
    version: String,
    new_precedence: i64,
}

impl PrecedenceChange {
    /// Twig: `new PrecedenceChange($package, $version, $newPrecedence)`.
    pub fn new(package: &str, version: &str, new_precedence: i64) -> Self {
        PrecedenceChange {
            package: package.to_owned(),
            version: version.to_owned(),
            new_precedence,
        }
    }

    /// Twig: `getPackage()`.
    pub fn package(&self) -> &str {
        &self.package
    }

    /// Twig: `getVersion()`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Twig: `getNewPrecedence()`.
    pub fn new_precedence(&self) -> i64 {
        self.new_precedence
    }
}

/// The associativity of an infix operator. Twig: `Twig\ExpressionParser\InfixAssociativity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfixAssociativity {
    /// Left associative: the right operand is parsed with a higher minimum precedence.
    Left,
    /// Right associative.
    Right,
}

/// Prefix or infix. Twig: `Twig\ExpressionParser\ExpressionParserType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpressionParserType {
    /// A prefix parser.
    Prefix,
    /// An infix parser.
    Infix,
}

impl ExpressionParserType {
    /// `prefix` or `infix`.
    pub fn as_str(self) -> &'static str {
        match self {
            ExpressionParserType::Prefix => "prefix",
            ExpressionParserType::Infix => "infix",
        }
    }
}

/// What every expression parser has. Twig: `Twig\ExpressionParser\ExpressionParserInterface` and
/// `AbstractExpressionParser`.
pub trait ExpressionParser: Send + Sync + 'static {
    /// Twig: `getName()`.
    fn name(&self) -> &str;

    /// Twig: `getPrecedence()`.
    fn precedence(&self) -> i64;

    /// Twig: `getPrecedenceChange()`.
    fn precedence_change(&self) -> Option<&PrecedenceChange> {
        None
    }

    /// Twig: `getAliases()`.
    fn aliases(&self) -> &[String] {
        &[]
    }

    /// The operator tokens the parser handles: what the lexer recognizes as operators and the registry keys.
    /// Twig: `getOperatorTokens()` (the name and the aliases by default).
    fn operator_tokens(&self) -> Vec<String> {
        let mut tokens = vec![self.name().to_owned()];
        tokens.extend(self.aliases().iter().cloned());
        tokens
    }

    /// Twig: `ExpressionParserDescriptionInterface::getDescription()`.
    fn description(&self) -> &str {
        ""
    }

    /// The identity of the parser's class (Twig keys parsers by class in `ExpressionParsers::$parsersByClass`).
    fn class_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// A parser that starts an expression. Twig: `Twig\ExpressionParser\PrefixExpressionParserInterface`.
pub trait PrefixExpressionParser: ExpressionParser {
    /// Parses the expression; `token` is the operator token, the stream is positioned after it.
    ///
    /// # Errors
    ///
    /// The syntax errors of the expression.
    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> Result<Expr>;
}

/// A parser that continues an expression. Twig: `Twig\ExpressionParser\InfixExpressionParserInterface`.
pub trait InfixExpressionParser: ExpressionParser {
    /// Parses the rest of the expression; `left` is what was parsed so far, `token` the operator token, the stream
    /// is positioned after it.
    ///
    /// # Errors
    ///
    /// The syntax errors of the expression.
    fn parse(&self, parser: &mut Parser<'_>, left: Expr, token: &Token) -> Result<Expr>;

    /// Twig: `getAssociativity()`.
    fn associativity(&self) -> InfixAssociativity;
}

/// A registered expression parser, prefix or infix.
#[derive(Clone)]
pub enum ExpressionParserEntry {
    /// A prefix parser.
    Prefix(Arc<dyn PrefixExpressionParser>),
    /// An infix parser.
    Infix(Arc<dyn InfixExpressionParser>),
}

impl ExpressionParserEntry {
    /// Wraps a prefix parser.
    pub fn prefix(parser: impl PrefixExpressionParser) -> Self {
        ExpressionParserEntry::Prefix(Arc::new(parser))
    }

    /// Wraps an infix parser.
    pub fn infix(parser: impl InfixExpressionParser) -> Self {
        ExpressionParserEntry::Infix(Arc::new(parser))
    }

    /// The parser's common interface.
    pub fn parser(&self) -> &dyn ExpressionParser {
        match self {
            ExpressionParserEntry::Prefix(p) => {
                p.as_ref() as &dyn PrefixExpressionParser as &dyn ExpressionParser
            }
            ExpressionParserEntry::Infix(p) => {
                p.as_ref() as &dyn InfixExpressionParser as &dyn ExpressionParser
            }
        }
    }

    /// Prefix or infix.
    pub fn parser_type(&self) -> ExpressionParserType {
        match self {
            ExpressionParserEntry::Prefix(_) => ExpressionParserType::Prefix,
            ExpressionParserEntry::Infix(_) => ExpressionParserType::Infix,
        }
    }

    fn same_object(&self, other: &ExpressionParserEntry) -> bool {
        match (self, other) {
            (ExpressionParserEntry::Prefix(a), ExpressionParserEntry::Prefix(b)) => {
                std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
            }
            (ExpressionParserEntry::Infix(a), ExpressionParserEntry::Infix(b)) => {
                std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
            }
            _ => false,
        }
    }

    /// `prefix(<name>)` or `infix(<name>)`. Twig: `AbstractExpressionParser::__toString()`.
    pub fn display(&self) -> String {
        format!("{}({})", self.parser_type().as_str(), self.parser().name())
    }
}

/// The identity of a registered parser within its registry (Twig compares parser objects with `===`).
pub type ParserId = u32;

/// The registry of expression parsers. Twig: `Twig\ExpressionParser\ExpressionParsers`
/// (TWIG/src/ExpressionParser/ExpressionParsers.php).
///
/// Parsers are found by operator token, separately for prefix and infix parsers; a later registration of a token
/// replaces the earlier parser at the earlier position (PHP array semantics), which also decides the iteration
/// order and thus the lexer's operator list.
#[derive(Default)]
pub struct ExpressionParsers {
    /// Every distinct parser object ever added, by id.
    parsers: Vec<ExpressionParserEntry>,
    /// `parsersByName`: per type (in the order the types were first added), token => parser.
    by_name: Vec<(ExpressionParserType, Vec<(String, ParserId)>)>,
    /// `parsersByClass`: class => last parser of that class.
    by_class: Vec<(TypeId, ParserId)>,
    precedence_changes: OnceLock<Vec<Option<Vec<ParserId>>>>,
    deprecations: Vec<Deprecation>,
}

impl std::fmt::Debug for ExpressionParsers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.iter().map(|(_, entry)| entry.display()))
            .finish()
    }
}

impl ExpressionParsers {
    /// An empty registry.
    pub fn new() -> Self {
        ExpressionParsers::default()
    }

    /// Registers parsers. Twig: `ExpressionParsers::add()`, including the 3.21 deprecation for precedences outside
    /// 0..512 (collected in [`deprecations`](Self::deprecations)).
    pub fn add(&mut self, parsers: impl IntoIterator<Item = ExpressionParserEntry>) {
        for entry in parsers {
            let parser = entry.parser();
            let precedence = parser.precedence();
            if !(0..=512).contains(&precedence) {
                self.deprecations.push(Deprecation::twig(
                    "3.21",
                    format!(
                        "Precedence for \"{}\" must be between 0 and 512, got {precedence}.",
                        parser.name()
                    ),
                ));
            }
            let id = match self.parsers.iter().position(|p| p.same_object(&entry)) {
                Some(i) => i as ParserId,
                None => {
                    self.parsers.push(entry.clone());
                    (self.parsers.len() - 1) as ParserId
                }
            };
            let class = parser.class_id();
            match self.by_class.iter_mut().find(|(c, _)| *c == class) {
                Some(slot) => slot.1 = id,
                None => self.by_class.push((class, id)),
            }
            let parser_type = entry.parser_type();
            let group_index = match self.by_name.iter().position(|(t, _)| *t == parser_type) {
                Some(i) => i,
                None => {
                    self.by_name.push((parser_type, Vec::new()));
                    self.by_name.len() - 1
                }
            };
            for token in parser.operator_tokens() {
                let group = &mut self.by_name[group_index].1;
                match group.iter_mut().find(|(t, _)| *t == token) {
                    Some(slot) => slot.1 = id,
                    None => group.push((token, id)),
                }
            }
        }
        self.precedence_changes = OnceLock::new();
    }

    /// The deprecations raised while adding parsers.
    pub fn deprecations(&self) -> &[Deprecation] {
        &self.deprecations
    }

    /// The parser with this id.
    pub fn get(&self, id: ParserId) -> Option<&ExpressionParserEntry> {
        self.parsers.get(id as usize)
    }

    fn by_name(&self, parser_type: ExpressionParserType, token: &[u8]) -> Option<ParserId> {
        let token = std::str::from_utf8(token).ok()?;
        self.by_name
            .iter()
            .find(|(t, _)| *t == parser_type)?
            .1
            .iter()
            .find(|(name, _)| name == token)
            .map(|&(_, id)| id)
    }

    /// The prefix parser for an operator token. Twig: `getByName(PrefixExpressionParserInterface::class, $name)`.
    pub fn prefix(&self, token: &[u8]) -> Option<(ParserId, &Arc<dyn PrefixExpressionParser>)> {
        let id = self.by_name(ExpressionParserType::Prefix, token)?;
        match self.get(id)? {
            ExpressionParserEntry::Prefix(p) => Some((id, p)),
            ExpressionParserEntry::Infix(_) => None,
        }
    }

    /// The infix parser for an operator token. Twig: `getByName(InfixExpressionParserInterface::class, $name)`.
    pub fn infix(&self, token: &[u8]) -> Option<(ParserId, &Arc<dyn InfixExpressionParser>)> {
        let id = self.by_name(ExpressionParserType::Infix, token)?;
        match self.get(id)? {
            ExpressionParserEntry::Infix(p) => Some((id, p)),
            ExpressionParserEntry::Prefix(_) => None,
        }
    }

    /// The last registered parser of class `T`. Twig: `getByClass(T::class)`.
    pub fn by_class<T: ExpressionParser>(&self) -> Option<(ParserId, &ExpressionParserEntry)> {
        let class = TypeId::of::<T>();
        let &(_, id) = self.by_class.iter().find(|(c, _)| *c == class)?;
        Some((id, self.get(id)?))
    }

    /// The parsers in Twig's iteration order (`getIterator()`): those found by name, by type then token
    /// position, then the remaining ones found by class; each parser once.
    pub fn iter(&self) -> impl Iterator<Item = (ParserId, &ExpressionParserEntry)> {
        let mut seen: Vec<ParserId> = Vec::new();
        let mut order: Vec<ParserId> = Vec::new();
        for (_, group) in &self.by_name {
            for &(_, id) in group {
                if !seen.contains(&id) {
                    seen.push(id);
                    order.push(id);
                }
            }
        }
        for &(_, id) in &self.by_class {
            if !seen.contains(&id) {
                seen.push(id);
                order.push(id);
            }
        }
        order
            .into_iter()
            .filter_map(move |id| self.get(id).map(|entry| (id, entry)))
    }

    /// The operator tokens in the order `Lexer::getOperatorRegex()` collects them (duplicates included).
    pub fn operator_tokens(&self) -> Vec<String> {
        self.iter()
            .flat_map(|(_, entry)| entry.parser().operator_tokens())
            .collect()
    }

    /// For every parser, the parsers whose precedence change crosses its precedence, in iteration order (`None`
    /// for parsers that are not affected). Twig: `ExpressionParsers::getPrecedenceChanges()`.
    pub fn precedence_changes(&self) -> &[Option<Vec<ParserId>>] {
        self.precedence_changes.get_or_init(|| {
            let mut changes: Vec<Option<Vec<ParserId>>> = vec![None; self.parsers.len()];
            let order: Vec<(ParserId, &ExpressionParserEntry)> = self.iter().collect();
            for &(changer_id, changer) in &order {
                let Some(change) = changer.parser().precedence_change() else {
                    continue;
                };
                let precedence = changer.parser().precedence();
                let min = change.new_precedence().min(precedence);
                let max = change.new_precedence().max(precedence);
                for &(id, entry) in &order {
                    let p = entry.parser().precedence();
                    if p > min && p < max {
                        changes[id as usize]
                            .get_or_insert_with(Vec::new)
                            .push(changer_id);
                    }
                }
            }
            changes
        })
    }
}
