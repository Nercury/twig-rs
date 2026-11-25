//! The parser: Twig's `Twig\Parser` (TWIG/src/Parser.php) with the expression parsers of
//! `Twig\ExpressionParser\*`.
//!
//! [`Parser::parse`] turns a [`TokenStream`] into a `ModuleNode` exactly as `Parser::parse()` does: the body is
//! parsed by [`Parser::subparse`] (text, print statements and tags dispatched to the registered
//! [`TokenParser`](crate::token_parser::TokenParser)s), the module is assembled with the blocks, macros, traits and
//! embedded templates registered on the way, and the registered node visitors run over it
//! ([`crate::visitor::NodeTraverser`]). Expressions are parsed by [`Parser::parse_expression`] over the
//! expression-parser registry, with Twig's precedence-change deprecations.
//!
//! [`ParseContext`] is the entry point that turns a [`Source`] into a visited module the way
//! `$env->parse($env->tokenize($source))` does.
//!
//! The token stream is the lexer's [`TokenStream`] (Twig's `Twig\TokenStream`), re-exported here.

pub mod callables;
mod context;
pub mod expression;

pub use crate::lexer::TokenStream;
pub use context::{ParseContext, compile_source_error};

use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::{
    ArgumentKey, Arguments, ArrayExpr, BinaryOp, ChildKey, ConstantValue, Expr, ExprKind, Lossy,
    Node, NodeKey, NodeKind, NodeMut, Nodes, UnaryOp,
};
use crate::deprecation::{Deprecation, DeprecationSink};
use crate::error::{Error, ErrorKind, Result};
use crate::extension::callable::{dummy_filter, dummy_function, dummy_test};
use crate::extension::{ExtensionSet, TwigFilter, TwigFunction, TwigTest};
use crate::lexer::{LexerOptions, Token, TokenType};
use crate::source::Source;
use crate::visitor::NodeTraverser;
use expression::prefix::{LiteralExpressionParser, token_bytes};
use expression::{
    ExpressionParserEntry, ExpressionParserType, ExpressionParsers, InfixExpressionParser,
    ParserId, PrefixExpressionParser,
};

/// The test that recognizes the end tag of a body passed to [`Parser::subparse`] (Twig's `$test` callable).
pub struct EndTagTest<'a> {
    /// The tag of the token parser whose method the test is (Twig's `[$tokenParser, 'decideXxx']`): unknown tags in
    /// the body then report `(expecting closing tag for the "<tag>" tag defined near line N).`; `None` for a
    /// closure.
    pub tag: Option<&'a str>,
    /// Whether the token (the tag name after `{%`) ends the body.
    pub test: &'a dyn Fn(&Token) -> bool,
}

/// A symbol imported with `import` or `from` (Twig's `['name' => ..., 'node' => ...]`).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSymbol {
    /// The macro name, for `function` symbols.
    pub name: Option<Vec<u8>>,
    /// The `AssignMacroVariable` of the import, for `function` symbols.
    pub node: Option<Expr>,
}

impl ImportedSymbol {
    /// The macro variable of the import (`$alias['node']->getNode('var')`).
    ///
    /// # Errors
    ///
    /// A `LogicException`-kind error when the symbol has no import node (a `template` symbol).
    pub fn macro_variable(&self) -> Result<Expr> {
        match self.node.as_ref().map(|node| &node.kind) {
            Some(ExprKind::AssignMacroVariable(assign)) => Ok(assign.var.clone()),
            _ => Err(Error::logic("Call to a member function getNode() on null")),
        }
    }
}

/// One scope of imported symbols, by type (`function`, `template`) and alias.
type ImportScope = HashMap<(String, Vec<u8>), ImportedSymbol>;

/// Where the documentation of a tag goes when it is not the node the token parser returned
/// (Twig's `setDocumentationTarget()`, which takes the target node object).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentationTarget {
    /// The `BlockNode` registered with [`Parser::set_block`] under this name.
    Block(Vec<u8>),
    /// The `MacroNode` registered with [`Parser::set_macro`] under this name.
    Macro(Vec<u8>),
    /// A node inside the node the token parser returns, by the keys of the children leading to it (as
    /// [`ChildKey`] displays them, e.g. `["target"]` or `["0", "expr"]`); an empty path is the returned node itself
    /// (Twig then leaves the documentation where it is).
    Child(Vec<Vec<u8>>),
}

/// The per-template state of the parser (what Twig's `parse()` saves and restores around nested parses).
#[derive(Default, Clone)]
struct ParseState {
    parent: Option<Expr>,
    blocks: Nodes,
    block_stack: Vec<Vec<u8>>,
    documentation_targets: Vec<Option<DocumentationTarget>>,
    macros: Nodes,
    imported_symbols: Vec<ImportScope>,
    traits: Vec<Node>,
    embedded_templates: Vec<Node>,
}

impl ParseState {
    fn new() -> Self {
        ParseState {
            imported_symbols: vec![ImportScope::new()],
            ..Default::default()
        }
    }
}

/// The parser. Twig: `Twig\Parser` (TWIG/src/Parser.php).
///
/// Token parsers receive the parser in [`TokenParser::parse`](crate::token_parser::TokenParser::parse) and use the same
/// API as Twig's token parsers: the stream, [`parse_expression`](Self::parse_expression),
/// [`subparse`](Self::subparse), the block stack, local scopes and imported symbols, blocks, macros, traits and
/// embedded templates.
pub struct Parser<'a> {
    env: &'a ExtensionSet,
    stream: TokenStream,
    deprecations: &'a mut dyn DeprecationSink,
    state: ParseState,
    stack: Vec<ParseState>,
    last_embed_index: i64,
    ignore_unknown_twig_callables: bool,
    embedded_template_parent: Option<Expr>,
    next_macro_variable_id: u32,
    nesting: usize,
    max_nesting: usize,
}

/// The default maximum nesting level of expressions and bodies, see [`Parser::set_max_nesting_level`].
///
/// One level of nesting (a body, an expression; `embed` takes two levels) costs up to about 2.7 KiB of stack in
/// optimized builds and 12 KiB in debug builds (measured on x86-64 with nested tags; expressions cost less), so the
/// default fits the 1 MiB main-thread stack of Windows in optimized builds and a 2 MiB thread stack in debug builds.
pub const DEFAULT_MAX_NESTING_LEVEL: usize = 128;

/// Lexes and parses a template: `$env->parse($env->tokenize($source))` with the lexer built from the registry's
/// operators.
///
/// # Errors
///
/// The lexer and parser errors.
pub fn parse_source(
    env: &ExtensionSet,
    source: &Source,
    options: LexerOptions,
    deprecations: &mut dyn DeprecationSink,
) -> Result<Node> {
    let stream = env.lexer(options).tokenize(source, deprecations)?;
    Parser::new(env, stream, deprecations).parse(None, false)
}

impl<'a> Parser<'a> {
    /// A parser for a token stream. Twig: `new Parser($env)` followed by `parse($stream)`.
    pub fn new(
        env: &'a ExtensionSet,
        stream: TokenStream,
        deprecations: &'a mut dyn DeprecationSink,
    ) -> Self {
        Parser {
            env,
            stream,
            deprecations,
            state: ParseState::new(),
            stack: Vec::new(),
            last_embed_index: 0,
            ignore_unknown_twig_callables: false,
            embedded_template_parent: None,
            next_macro_variable_id: 0,
            nesting: 0,
            max_nesting: DEFAULT_MAX_NESTING_LEVEL,
        }
    }

    /// The extensions. Twig: `getEnvironment()`.
    pub fn environment(&self) -> &'a ExtensionSet {
        self.env
    }

    /// The token stream. Twig: `getStream()`.
    pub fn stream(&self) -> &TokenStream {
        &self.stream
    }

    /// The token stream, mutably.
    pub fn stream_mut(&mut self) -> &mut TokenStream {
        &mut self.stream
    }

    /// Consumes the parser, returning the stream.
    pub fn into_stream(self) -> TokenStream {
        self.stream
    }

    /// The current token. Twig: `getCurrentToken()`.
    pub fn current_token(&self) -> &Token {
        self.stream.current()
    }

    /// The deprecation sink.
    pub fn deprecations(&mut self) -> &mut dyn DeprecationSink {
        &mut *self.deprecations
    }

    /// Reports a deprecation.
    pub fn report_deprecation(&mut self, deprecation: Deprecation) {
        self.deprecations.report(deprecation);
    }

    /// A new identity for an anonymous macro variable (see [`MacroVariableExpr`](crate::ast::MacroVariableExpr)).
    pub fn next_macro_variable_id(&mut self) -> u32 {
        let id = self.next_macro_variable_id;
        self.next_macro_variable_id += 1;
        id
    }

    fn source(&self) -> Source {
        self.stream.source_context().clone()
    }

    fn syntax_error(&self, message: impl Into<String>, line: i64) -> Error {
        Error::syntax(message)
            .with_line(line)
            .with_source(self.source())
    }

    /// Parses the whole stream (or, with `test`, up to the end tag) into a `ModuleNode` and runs the node visitors
    /// over it. Twig: `Parser::parse()`.
    ///
    /// # Errors
    ///
    /// The syntax errors of the template (with the stream's source when they have none, and the current line when
    /// their line is `0`), and the errors of the node visitors.
    pub fn parse(&mut self, test: Option<&EndTagTest<'_>>, drop_needle: bool) -> Result<Node> {
        let embedded_template_parent = self.embedded_template_parent.take();

        // reset on root parse() calls only, so the counter spans nested/reentrant parses
        if self.stack.is_empty() {
            self.last_embed_index = 0;
        }

        let saved = std::mem::replace(&mut self.state, ParseState::new());
        self.stack.push(saved);
        let result = self.parse_module(test, drop_needle, embedded_template_parent);
        // restore previous stack so previous parse() call can resume working
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
        result
    }

    /// Parses another token stream in the middle of the current parse (a reentrant parse): the stream and the whole
    /// per-template state are saved and restored afterwards. Twig: `Parser::parse()` called with a different stream.
    ///
    /// # Errors
    ///
    /// As [`parse`](Self::parse).
    pub fn parse_stream(
        &mut self,
        stream: TokenStream,
        test: Option<&EndTagTest<'_>>,
        drop_needle: bool,
    ) -> Result<Node> {
        let previous = std::mem::replace(&mut self.stream, stream);
        let result = self.parse(test, drop_needle);
        self.stream = previous;
        result
    }

    fn parse_module(
        &mut self,
        test: Option<&EndTagTest<'_>>,
        drop_needle: bool,
        embedded_template_parent: Option<Expr>,
    ) -> Result<Node> {
        let body = match self.subparse(test, drop_needle) {
            Ok(body) => body,
            Err(mut error) => {
                if error.kind() == ErrorKind::Syntax {
                    if error.source_context().is_none() {
                        error.set_source_context(Some(self.source()));
                    }
                    // Past the end of the stream (an error at line 0 from a failed `next()`, only possible after
                    // `{% line 0 %}`) PHP's `getCurrentToken()` fails with a TypeError; the last token's line is used.
                    if error.template_line() == 0 {
                        error.set_template_line(self.current_token().line());
                    }
                }
                return Err(error);
            }
        };

        if let Some(parent) = embedded_template_parent {
            self.state.parent = Some(parent);
        }

        let body = if self.state.parent.is_some() {
            cleanup_body_for_child_templates(body)
        } else {
            body
        };

        let state = std::mem::replace(&mut self.state, ParseState::new());
        let blocks = if state.blocks.is_empty() {
            Node::empty(0)
        } else {
            Node::nodes(state.blocks, 0)
        };
        let traits = if state.traits.is_empty() {
            Node::empty(0)
        } else {
            Node::nodes(Nodes::from_list(state.traits), 0)
        };
        let embedded_templates = if state.embedded_templates.is_empty() {
            Node::empty(0)
        } else {
            Node::nodes(Nodes::from_list(state.embedded_templates), 0)
        };
        let mut module = Node::module(
            Node::body(Nodes::from_list([body]), 0),
            state.parent,
            blocks,
            Node::new(NodeKind::Macros(state.macros), 0),
            traits,
            embedded_templates,
            self.source(),
        );

        let env = self.env;
        NodeTraverser::new(env, env.node_visitors().iter().cloned())
            .traverse(&mut module, &mut *self.deprecations)?;
        Ok(module)
    }

    /// Parses an embedded template (`embed`): a nested [`parse`](Self::parse) of the same stream up to the end
    /// tag, whose parent is `parent`. Twig: `Parser::parseEmbeddedTemplate()`.
    ///
    /// # Errors
    ///
    /// As [`parse`](Self::parse).
    pub fn parse_embedded_template(&mut self, parent: Expr, test: &EndTagTest<'_>) -> Result<Node> {
        let previous = self.embedded_template_parent.replace(parent);
        let result = self.parse(Some(test), true);
        self.embedded_template_parent = previous;
        result
    }

    /// Whether unknown functions, filters and tests are replaced by dummies (inside a discarded `guard` branch).
    /// Twig: `shouldIgnoreUnknownTwigCallables()`.
    pub fn should_ignore_unknown_twig_callables(&self) -> bool {
        self.ignore_unknown_twig_callables
    }

    /// Parses a discarded part of the template: what it registers (parent, blocks, macros, ...) is discarded as
    /// well. Twig: `Parser::subparseIgnoreUnknownTwigCallables()`.
    ///
    /// # Errors
    ///
    /// As [`subparse`](Self::subparse) (unknown tags still fail).
    pub fn subparse_ignore_unknown_twig_callables(
        &mut self,
        test: Option<&EndTagTest<'_>>,
        drop_needle: bool,
    ) -> Result<()> {
        let previous = self.ignore_unknown_twig_callables;
        let saved = (
            self.state.parent.clone(),
            self.state.blocks.clone(),
            self.state.documentation_targets.clone(),
            self.state.macros.clone(),
            self.state.imported_symbols.clone(),
            self.state.traits.clone(),
            self.state.embedded_templates.clone(),
        );
        self.ignore_unknown_twig_callables = true;
        let result = self.subparse(test, drop_needle);
        self.ignore_unknown_twig_callables = previous;
        (
            self.state.parent,
            self.state.blocks,
            self.state.documentation_targets,
            self.state.macros,
            self.state.imported_symbols,
            self.state.traits,
            self.state.embedded_templates,
        ) = saved;
        result.map(|_| ())
    }

    /// Parses a body: text, print statements and tags, up to the end tag recognized by `test` (consumed when
    /// `drop_needle`) or the end of the stream. A body of exactly one node is that node, otherwise a `Nodes` at the
    /// line of its first token. Twig: `Parser::subparse()`.
    ///
    /// # Errors
    ///
    /// The syntax errors of the body, including `A block must start with a tag name.`, `Unknown "%s" tag.` (with
    /// suggestions) and `Unexpected "%s" tag (expecting closing tag for the "%s" tag defined near line %s).`.
    pub fn subparse(&mut self, test: Option<&EndTagTest<'_>>, drop_needle: bool) -> Result<Node> {
        self.enter_nesting()?;
        let result = self.subparse_unguarded(test, drop_needle);
        self.nesting -= 1;
        result
    }

    // The branches of the loop are separate functions with small frames: bodies nest through `parse_tag()`, and a
    // deep template recurses through these frames (see `DEFAULT_MAX_NESTING_LEVEL`).
    fn subparse_unguarded(
        &mut self,
        test: Option<&EndTagTest<'_>>,
        drop_needle: bool,
    ) -> Result<Node> {
        let lineno = self.current_token().line();
        let mut rv: Vec<Node> = Vec::new();
        while !self.stream.is_eof() {
            let current = self.stream.current();
            if current.test(TokenType::Text) {
                rv.push(self.parse_text()?);
            } else if current.test(TokenType::VarStart) {
                rv.push(self.parse_print()?);
            } else if current.test(TokenType::BlockStart) {
                match self.parse_tag(test, drop_needle, lineno)? {
                    TagOutcome::Node(node) => rv.push(*node),
                    TagOutcome::Nothing => {}
                    TagOutcome::End => return Ok(single_or_nodes(rv, lineno)),
                }
            } else {
                return Err(self.unsupported_state_error());
            }
        }
        Ok(single_or_nodes(rv, lineno))
    }

    /// A text token: a `TextNode`.
    #[inline(never)]
    fn parse_text(&mut self) -> Result<Node> {
        let token = self.stream.next()?;
        Ok(Node::text(token_bytes(&token), token.line()))
    }

    /// `{{ expr }}`: a `PrintNode` with the documentation of its opening token.
    #[inline(never)]
    fn parse_print(&mut self) -> Result<Node> {
        let token = self.stream.next()?;
        let expr = self.parse_expression(0)?;
        self.stream.expect(TokenType::VarEnd, None, None)?;
        let mut node = Node::print(expr, token.line());
        add_node_documentation(&mut node, &[&token]);
        Ok(node)
    }

    /// `{% name ... %}`: the end of the body when `test` recognizes the tag, else the node of the tag's token parser
    /// (with its tag and the documentation of its opening token).
    #[inline(never)]
    fn parse_tag(
        &mut self,
        test: Option<&EndTagTest<'_>>,
        drop_needle: bool,
        lineno: i64,
    ) -> Result<TagOutcome> {
        let start_token = Box::new(self.stream.next()?);
        let token = Box::new(self.current_token().clone());

        if !token.test(TokenType::Name) {
            return Err(self.syntax_error("A block must start with a tag name.", token.line()));
        }

        if let Some(test) = test
            && (test.test)(&token)
        {
            if drop_needle {
                self.stream.next()?;
            }
            return Ok(TagOutcome::End);
        }

        let tag_name = token_bytes(&token);
        let Some(subparser) = self.env.get_token_parser(&tag_name)? else {
            return Err(self.unknown_tag_error(test, &tag_name, token.line(), lineno));
        };

        self.stream.next()?;

        let target_index = self.state.documentation_targets.len();
        self.state.documentation_targets.push(None);
        let result = subparser.parse(self, &token);
        let target = if self.state.documentation_targets.len() > target_index {
            let target = self.state.documentation_targets[target_index].take();
            self.state.documentation_targets.truncate(target_index);
            target
        } else {
            None
        };
        match result? {
            None => {
                self.null_node_deprecation(subparser.as_ref());
                Ok(TagOutcome::Nothing)
            }
            Some(mut node) => {
                node.set_node_tag(subparser.tag())?;
                add_node_documentation(&mut node, &[&start_token]);
                if let Some(target) = target {
                    self.move_documentation(&mut node, &target);
                }
                Ok(TagOutcome::Node(Box::new(node)))
            }
        }
    }

    /// `Unexpected "%s" tag (expecting closing tag for the "%s" tag defined near line %s).` inside a body, `Unknown
    /// "%s" tag.` with suggestions at the top level.
    #[cold]
    #[inline(never)]
    fn unknown_tag_error(
        &self,
        test: Option<&EndTagTest<'_>>,
        tag_name: &[u8],
        line: i64,
        lineno: i64,
    ) -> Error {
        if let Some(test) = test {
            let mut error =
                self.syntax_error(format!("Unexpected \"{}\" tag", Lossy(tag_name)), line);
            if let Some(tag) = test.tag {
                error.append_message(&format!(
                    " (expecting closing tag for the \"{tag}\" tag defined near line {lineno})."
                ));
            }
            error
        } else {
            let mut error =
                self.syntax_error(format!("Unknown \"{}\" tag.", Lossy(tag_name)), line);
            let tags: Vec<Vec<u8>> = self
                .env
                .token_parsers()
                .map(|(tag, _)| tag.to_vec())
                .collect();
            error.add_suggestions(tag_name, tags);
            error
        }
    }

    /// The 3.12 deprecation of a token parser returning `null`.
    #[cold]
    #[inline(never)]
    fn null_node_deprecation(&mut self, subparser: &dyn crate::token_parser::TokenParser) {
        self.report_deprecation(Deprecation::twig(
            "3.12",
            format!(
                "Returning \"null\" from \"{}\" is deprecated and forbidden by \"TokenParserInterface\".",
                subparser.class_name()
            ),
        ));
    }

    /// `The lexer or the parser ended up in an unsupported state.`
    #[cold]
    #[inline(never)]
    fn unsupported_state_error(&self) -> Error {
        self.syntax_error(
            "The lexer or the parser ended up in an unsupported state.",
            self.current_token().line(),
        )
    }

    /// Twig: `NodeDocumentation::move($node, $documentationTarget)` (when the target is not the node itself).
    fn move_documentation(&mut self, node: &mut Node, target: &DocumentationTarget) {
        if matches!(target, DocumentationTarget::Child(path) if path.is_empty()) {
            return;
        }
        let Some(documentation) = node.documentation.take() else {
            return;
        };
        let slot = match target {
            DocumentationTarget::Block(name) => self
                .state
                .blocks
                .get_mut(&NodeKey::Name(name.clone()))
                .and_then(|body| body.as_nodes_mut())
                .and_then(|nodes| nodes.entries_mut().next().map(|(_, block)| block))
                .map(|block| &mut block.documentation),
            DocumentationTarget::Macro(name) => self
                .state
                .macros
                .get_mut(&NodeKey::Name(name.clone()))
                .map(|node| &mut node.documentation),
            DocumentationTarget::Child(path) => documentation_slot(NodeMut::Node(node), path),
        };
        match slot {
            Some(slot) => {
                let mut prepended = documentation;
                if let Some(current) = slot.take() {
                    prepended.push(b'\n');
                    prepended.extend_from_slice(&current);
                }
                *slot = Some(prepended);
            }
            // The target is not in the tree any more (Twig would move the documentation to a detached node): keep
            // the documentation on the node.
            None => node.documentation = Some(documentation),
        }
    }

    /// The name of the innermost block being parsed. Twig: `peekBlockStack()`.
    pub fn peek_block_stack(&self) -> Option<&[u8]> {
        self.state.block_stack.last().map(Vec::as_slice)
    }

    /// Twig: `popBlockStack()`.
    pub fn pop_block_stack(&mut self) {
        self.state.block_stack.pop();
    }

    /// Twig: `pushBlockStack()`.
    pub fn push_block_stack(&mut self, name: Vec<u8>) {
        self.state.block_stack.push(name);
    }

    /// Registers a block (stored as `BodyNode([$block])` at the block's line). Twig: `Parser::setBlock()`.
    ///
    /// # Errors
    ///
    /// `The block '%s' has already been defined line %d.` at the current line.
    pub fn set_block(&mut self, name: Vec<u8>, block: Node) -> Result<()> {
        let key = NodeKey::Name(name.clone());
        if let Some(existing) = self.state.blocks.get(&key) {
            return Err(Error::syntax(format!(
                "The block '{}' has already been defined line {}.",
                Lossy(&name),
                existing.line
            ))
            .with_line(self.current_token().line()));
        }
        let line = block.line;
        self.state
            .blocks
            .set(key, Node::body(Nodes::from_list([block]), line));
        Ok(())
    }

    /// The registered `BlockNode` named `name`, to set its body after parsing it (Twig mutates the object).
    pub fn block_mut(&mut self, name: &[u8]) -> Option<&mut Node> {
        self.state
            .blocks
            .get_mut(&NodeKey::Name(name.to_vec()))?
            .as_nodes_mut()?
            .entries_mut()
            .next()
            .map(|(_, block)| block)
    }

    /// Sets where the documentation of the tag being parsed goes. Twig: `Parser::setDocumentationTarget()`.
    ///
    /// # Errors
    ///
    /// `A documentation target can only be set while parsing a tag.` and `The documentation target for a tag can
    /// only be set once.` (`LogicException`s).
    pub fn set_documentation_target(&mut self, target: DocumentationTarget) -> Result<()> {
        let Some(slot) = self.state.documentation_targets.last_mut() else {
            return Err(Error::logic(
                "A documentation target can only be set while parsing a tag.",
            ));
        };
        if slot.is_some() {
            return Err(Error::logic(
                "The documentation target for a tag can only be set once.",
            ));
        }
        *slot = Some(target);
        Ok(())
    }

    /// Registers a macro; a redefinition is deprecated (since 3.29) and replaces the previous one.
    /// Twig: `Parser::setMacro()`.
    pub fn set_macro(&mut self, name: Vec<u8>, node: Node) {
        let key = NodeKey::Name(name.clone());
        if let Some(previous) = self.state.macros.get(&key) {
            let message = format!(
                "Defining the macro \"{}\" more than once in \"{}\" is deprecated and will throw a SyntaxError in Twig 4.0 (previous definition at line {}, new definition at line {}). The last definition is used in Twig 3.",
                Lossy(&name),
                self.stream.source_context().name(),
                previous.line,
                node.line
            );
            self.report_deprecation(Deprecation::twig("3.29", message));
        }
        self.state.macros.set(key, node);
    }

    /// Twig: `Parser::addTrait()`.
    pub fn add_trait(&mut self, node: Node) {
        self.state.traits.push(node);
    }

    /// Registers an embedded template and gives it the next index. Twig: `Parser::embedTemplate()`.
    pub fn embed_template(&mut self, mut template: Node) -> i64 {
        self.last_embed_index += 1;
        let index = self.last_embed_index;
        if let Some(module) = template.as_module_mut() {
            module.set_index(index);
        }
        self.state.embedded_templates.push(template);
        index
    }

    /// Registers an imported symbol in the innermost scope. Twig: `Parser::addImportedSymbol()`.
    pub fn add_imported_symbol(
        &mut self,
        symbol_type: &str,
        alias: Vec<u8>,
        name: Option<Vec<u8>>,
        node: Option<Expr>,
    ) {
        if let Some(scope) = self.state.imported_symbols.first_mut() {
            scope.insert(
                (symbol_type.to_owned(), alias),
                ImportedSymbol { name, node },
            );
        }
    }

    /// An imported symbol: from the innermost scope, else from the main scope. Twig: `Parser::getImportedSymbol()`.
    pub fn get_imported_symbol(&self, symbol_type: &str, alias: &[u8]) -> Option<&ImportedSymbol> {
        let key = (symbol_type.to_owned(), alias.to_vec());
        self.state
            .imported_symbols
            .first()
            .and_then(|scope| scope.get(&key))
            .or_else(|| {
                self.state
                    .imported_symbols
                    .last()
                    .and_then(|scope| scope.get(&key))
            })
    }

    /// Twig: `Parser::isMainScope()`.
    pub fn is_main_scope(&self) -> bool {
        self.state.imported_symbols.len() == 1
    }

    /// Twig: `Parser::pushLocalScope()`.
    pub fn push_local_scope(&mut self) {
        self.state.imported_symbols.insert(0, ImportScope::new());
    }

    /// Twig: `Parser::popLocalScope()`.
    pub fn pop_local_scope(&mut self) {
        if !self.state.imported_symbols.is_empty() {
            self.state.imported_symbols.remove(0);
        }
    }

    /// Whether the template extends another one or uses traits. Twig: `Parser::hasInheritance()`.
    pub fn has_inheritance(&self) -> bool {
        self.state.parent.is_some() || !self.state.traits.is_empty()
    }

    /// Sets the parent template. Twig: `Parser::setParent()`.
    ///
    /// # Errors
    ///
    /// `Multiple extends tags are forbidden.` (at the new parent's line) when a parent is already set and
    /// `throw_on_multiple`; otherwise a second parent is ignored.
    pub fn set_parent(&mut self, parent: Expr, throw_on_multiple: bool) -> Result<()> {
        if self.state.parent.is_some() {
            if !throw_on_multiple {
                return Ok(());
            }
            return Err(
                Error::syntax("Multiple extends tags are forbidden.").with_line(parent.line)
            );
        }
        self.state.parent = Some(parent);
        Ok(())
    }

    /// The function named `name`; unknown ones are an error (with suggestions), or a dummy while ignoring unknown
    /// callables. Reports the deprecation of a deprecated function. Twig: `Parser::getFunction()`.
    ///
    /// # Errors
    ///
    /// `Unknown "%s" function.` with suggestions.
    pub fn get_function(&mut self, name: &[u8], line: i64) -> Result<TwigFunction> {
        let function = self.ignoring_callback_errors(self.env.get_function(name))?;
        let Some(function) = function else {
            if self.ignore_unknown_twig_callables {
                return Ok(dummy_function(name));
            }
            let mut error =
                self.syntax_error(format!("Unknown \"{}\" function.", Lossy(name)), line);
            let names: Vec<Vec<u8>> = self.env.functions().map(|(n, _)| n.to_vec()).collect();
            error.add_suggestions(name, names);
            return Err(error);
        };
        if function.is_deprecated() {
            let file = self.deprecation_file();
            function.trigger_deprecation(Some(&file), Some(line), &mut *self.deprecations);
        }
        Ok(function)
    }

    /// The filter named `name` (see [`get_function`](Self::get_function)). Twig: `Parser::getFilter()`.
    ///
    /// # Errors
    ///
    /// `Unknown "%s" filter.` with suggestions.
    pub fn get_filter(&mut self, name: &[u8], line: i64) -> Result<TwigFilter> {
        let filter = self.ignoring_callback_errors(self.env.get_filter(name))?;
        let Some(filter) = filter else {
            if self.ignore_unknown_twig_callables {
                return Ok(dummy_filter(name));
            }
            let mut error = self.syntax_error(format!("Unknown \"{}\" filter.", Lossy(name)), line);
            let names: Vec<Vec<u8>> = self.env.filters().map(|(n, _)| n.to_vec()).collect();
            error.add_suggestions(name, names);
            return Err(error);
        };
        if filter.is_deprecated() {
            let file = self.deprecation_file();
            filter.trigger_deprecation(Some(&file), Some(line), &mut *self.deprecations);
        }
        Ok(filter)
    }

    /// Reads a test name from the stream and returns the test: when the next token is also a name, only the
    /// two-word test `"<a> <b>"` is looked up (and the second token consumed). Twig: `Parser::getTest()`.
    ///
    /// # Errors
    ///
    /// The `expect()` error when there is no name, and `Unknown "%s" test.` with suggestions.
    pub fn get_test(&mut self, line: i64) -> Result<TwigTest> {
        let token = self.stream.expect(TokenType::Name, None, None)?;
        let mut name = token_bytes(&token);
        let test = if self.stream.test(TokenType::Name) {
            // try 2-words tests
            name.push(b' ');
            name.extend_from_slice(&token_bytes(self.current_token()));
            let test = self.ignoring_callback_errors(self.env.get_test(&name))?;
            self.stream.next()?;
            test
        } else {
            self.ignoring_callback_errors(self.env.get_test(&name))?
        };

        let Some(test) = test else {
            if self.ignore_unknown_twig_callables {
                return Ok(dummy_test(&name));
            }
            let mut error = self.syntax_error(format!("Unknown \"{}\" test.", Lossy(&name)), line);
            let names: Vec<Vec<u8>> = self.env.tests().map(|(n, _)| n.to_vec()).collect();
            error.add_suggestions(&name, names);
            return Err(error);
        };
        if test.is_deprecated() {
            let file = self.deprecation_file();
            let line = self.current_token().line();
            test.trigger_deprecation(Some(&file), Some(line), &mut *self.deprecations);
        }
        Ok(test)
    }

    /// The `try { ... } catch (SyntaxError $e) { if (!$this->shouldIgnoreUnknownTwigCallables()) throw $e; }` around
    /// the registry lookups: a syntax error of an undefined-callable callback means "unknown" while ignoring unknown
    /// callables.
    fn ignoring_callback_errors<T>(&self, lookup: Result<Option<T>>) -> Result<Option<T>> {
        match lookup {
            Err(error)
                if error.kind() == ErrorKind::Syntax && self.ignore_unknown_twig_callables =>
            {
                Ok(None)
            }
            lookup => lookup,
        }
    }

    /// `$src->getPath() ?: $src->getName()`.
    fn deprecation_file(&self) -> String {
        let source = self.stream.source_context();
        let path = source.path();
        if path.is_empty() || path == "0" {
            source.name().to_owned()
        } else {
            path.to_owned()
        }
    }

    /// Parses an expression whose operators have at least the given precedence: a prefix parser for the current
    /// operator token (or the literal parser), then infix parsers while their precedence allows.
    /// Twig: `Parser::parseExpression()`.
    ///
    /// # Errors
    ///
    /// The syntax errors of the expression, and `Maximum nesting level of %d reached.` beyond the
    /// [nesting limit](Self::set_max_nesting_level).
    pub fn parse_expression(&mut self, precedence: i64) -> Result<Expr> {
        self.enter_nesting()?;
        let result = self.parse_expression_unguarded(precedence);
        self.nesting -= 1;
        result
    }

    fn parse_expression_unguarded(&mut self, precedence: i64) -> Result<Expr> {
        let parsers = self.env.expression_parsers();
        let mut expr = self.parse_prefix_expression(parsers)?;
        while let Some((id, parser, token)) = self.next_infix_parser(parsers, precedence)? {
            expr = parser.parse(self, expr, &token)?;
            self.check_precedence_deprecations(id, &mut expr)?;
        }
        Ok(expr)
    }

    /// The prefix part of `parseExpression()`: the prefix parser of the current operator token, or the literal
    /// parser. (Separate functions keep the stack frames of the recursion small.)
    #[inline(never)]
    fn parse_prefix_expression(&mut self, parsers: &'a ExpressionParsers) -> Result<Expr> {
        let token = Box::new(self.current_token().clone());
        let prefix = if token.test(TokenType::Operator) {
            token
                .value()
                .as_bytes()
                .and_then(|value| parsers.prefix(value))
        } else {
            None
        };
        if let Some((id, parser)) = prefix {
            self.advance()?;
            let mut expr = parser.parse(self, &token)?;
            self.check_precedence_deprecations(id, &mut expr)?;
            return Ok(expr);
        }
        match parsers.by_class::<LiteralExpressionParser>() {
            Some((_, ExpressionParserEntry::Prefix(literal))) => literal.parse(self, &token),
            _ => LiteralExpressionParser.parse(self, &token),
        }
    }

    /// The infix parser of the current operator token when its precedence is at least `precedence`; the operator
    /// token is consumed and returned.
    #[inline(never)]
    #[allow(
        clippy::type_complexity,
        reason = "an internal step of parse_expression"
    )]
    fn next_infix_parser(
        &mut self,
        parsers: &'a ExpressionParsers,
        precedence: i64,
    ) -> Result<Option<(ParserId, &'a Arc<dyn InfixExpressionParser>, Box<Token>)>> {
        let token = self.current_token();
        if !token.test(TokenType::Operator) {
            return Ok(None);
        }
        let Some((id, parser)) = token
            .value()
            .as_bytes()
            .and_then(|value| parsers.infix(value))
        else {
            return Ok(None);
        };
        if parser.precedence() < precedence {
            return Ok(None);
        }
        let token = Box::new(token.clone());
        self.advance()?;
        Ok(Some((id, parser, token)))
    }

    /// `$stream->next()` when the token is not needed.
    #[inline(never)]
    pub(crate) fn advance(&mut self) -> Result<()> {
        self.stream.next().map(drop)
    }

    /// `$stream->expect($type, $value, $message)` when the token is not needed.
    #[inline(never)]
    pub(crate) fn expect_token(
        &mut self,
        ty: TokenType,
        value: Option<&str>,
        message: Option<&str>,
    ) -> Result<()> {
        self.stream.expect(ty, value, message).map(drop)
    }

    /// `$stream->nextIf($type, $value)` when only the outcome is needed.
    #[inline(never)]
    pub(crate) fn skip_if(&mut self, ty: TokenType, value: &str) -> Result<bool> {
        Ok(self.stream.next_if_value(ty, value)?.is_some())
    }

    /// `$stream->nextIf($type)` when only the outcome is needed.
    #[inline(never)]
    pub(crate) fn skip_if_type(&mut self, ty: TokenType) -> Result<bool> {
        Ok(self.stream.next_if(ty)?.is_some())
    }

    /// Sets the maximum nesting level of expressions and bodies (default [`DEFAULT_MAX_NESTING_LEVEL`]).
    ///
    /// Twig has no such limit (PHP's call stack lives on the heap); the recursive Rust parser needs one so that
    /// pathological templates fail with an error instead of overflowing the thread's stack.
    pub fn set_max_nesting_level(&mut self, level: usize) {
        self.max_nesting = level;
    }

    fn enter_nesting(&mut self) -> Result<()> {
        if self.nesting >= self.max_nesting {
            return Err(self.nesting_error());
        }
        self.nesting += 1;
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn nesting_error(&self) -> Error {
        self.syntax_error(
            format!("Maximum nesting level of {} reached.", self.max_nesting),
            self.current_token().line(),
        )
    }

    /// Twig: `Parser::checkPrecedenceDeprecations()`: records which parser produced `expr` and reports the
    /// operators whose precedence will change in Twig 4 and that are used without explicit parentheses.
    fn check_precedence_deprecations(
        &mut self,
        parser_id: ParserId,
        expr: &mut Expr,
    ) -> Result<()> {
        expr.parsed_by = Some(parser_id);
        let env = self.env;
        let parsers = env.expression_parsers();
        let changes = parsers.precedence_changes();
        // Check that the all nodes that are between the 2 precedences have explicit parentheses
        let Some(Some(changers)) = changes.get(parser_id as usize) else {
            return Ok(());
        };
        if expr.with_parentheses {
            return Ok(());
        }
        let Some(entry) = parsers.get(parser_id) else {
            return Ok(());
        };

        let mut deprecations: Vec<(ParserId, i64)> = Vec::new();
        if entry.parser_type() == ExpressionParserType::Prefix {
            let operand = node_child(expr).ok_or_else(|| {
                Error::logic(format!(
                    "Node \"node\" does not exist for Node \"{}\".",
                    expr.class_name()
                ))
            })?;
            for (key, list) in changes.iter().enumerate() {
                let Some(list) = list else {
                    continue;
                };
                if list.contains(&parser_id) && operand.parsed_by == Some(key as ParserId) {
                    deprecations.push((parser_id, operand.line));
                }
            }
        }

        for &changer in changers {
            for (_, child) in expr.children() {
                if let crate::ast::NodeRef::Expr(child) = child
                    && child.parsed_by == Some(changer)
                    && !child.with_parentheses
                {
                    deprecations.push((changer, child.line));
                }
            }
        }

        for (changer, line) in deprecations {
            let Some(changer_entry) = parsers.get(changer) else {
                continue;
            };
            let Some(change) = changer_entry.parser().precedence_change() else {
                continue;
            };
            let message = format!(
                "As the \"{}\" {} operator will change its precedence in the next major version, add explicit parentheses to avoid behavior change in \"{}\" at line {}.",
                changer_entry.parser().name(),
                changer_entry.parser_type().as_str(),
                self.stream.source_context().name(),
                line
            );
            self.report_deprecation(Deprecation::new(
                change.package().to_owned(),
                change.version().to_owned(),
                message,
            ));
        }
        Ok(())
    }

    /// Validates a constant regular expression on the right of `matches` like `MatchesBinary::__construct()`:
    /// `preg_match($regexp, '')` with every PHP diagnostic turned into
    /// `Regexp "%s" passed to "matches" is not valid: %s.`. A `null` pattern fails with PHP's deprecation of `null`
    /// arguments; other patterns are checked by the registry's [`RegexValidator`](crate::extension::RegexValidator)
    /// when one is set.
    ///
    /// # Errors
    ///
    /// The syntax error above (at `line`, without source until `parse()` adds it).
    pub fn check_matches_regexp(&self, right: &Expr, line: i64) -> Result<()> {
        let ExprKind::Constant(constant) = &right.kind else {
            return Ok(());
        };
        let pattern = constant.value.to_php_string();
        let warning = match &constant.value {
            ConstantValue::Null => Some(
                "Passing null to parameter #1 ($pattern) of type string is deprecated".to_owned(),
            ),
            ConstantValue::Array(_) => None,
            _ => self
                .env
                .regex_validator()
                .and_then(|validator| validator.validate(&pattern)),
        };
        if let Some(warning) = warning {
            return Err(Error::syntax(format!(
                "Regexp \"{}\" passed to \"matches\" is not valid: {warning}.",
                Lossy(&pattern)
            ))
            .with_line(line));
        }
        Ok(())
    }

    /// Parses call arguments: positional, named (`name = value` or `name: value`) and spread (`...value`, only
    /// after the other arguments). Twig: `ArgumentsTrait::parseNamedArguments()`.
    ///
    /// # Errors
    ///
    /// `A list of arguments must begin with an opening parenthesis`, `Arguments must be separated by a comma`,
    /// `Normal arguments must be placed before argument unpacking.`, `A parameter name must be a string, "%s"
    /// given.` and the errors of the argument expressions.
    pub fn parse_named_arguments(&mut self, parse_open_parenthesis: bool) -> Result<Arguments> {
        let mut args = Arguments::new();
        if parse_open_parenthesis {
            self.expect_token(
                TokenType::Operator,
                Some("("),
                Some("A list of arguments must begin with an opening parenthesis"),
            )?;
        }
        let mut has_spread = false;
        while !self.stream.test_value(TokenType::Punctuation, ")") {
            if !args.is_empty() {
                self.expect_token(
                    TokenType::Punctuation,
                    Some(","),
                    Some("Arguments must be separated by a comma"),
                )?;
                // if the comma above was a trailing comma, early exit the argument parse loop
                if self.stream.test_value(TokenType::Punctuation, ")") {
                    break;
                }
            }

            let mut value = if self.stream.test_value(TokenType::Operator, "...") {
                let line = self.current_token().line();
                self.advance()?;
                has_spread = true;
                Expr::unary(UnaryOp::Spread, self.parse_expression(0)?, line)
            } else {
                let value = self.parse_expression(0)?;
                if has_spread {
                    return Err(self.syntax_error(
                        "Normal arguments must be placed before argument unpacking.",
                        self.current_token().line(),
                    ));
                }
                value
            };

            let mut name = None;
            if matches!(&value.kind, ExprKind::Binary(binary) if binary.op == BinaryOp::Set) {
                if let ExprKind::Binary(binary) = value.kind {
                    let binary = *binary;
                    name = binary.left.context_variable_name().map(<[u8]>::to_vec);
                    value = binary.right;
                }
            } else if self.stream.test_value(TokenType::Operator, "=")
                || self.stream.test_value(TokenType::Punctuation, ":")
            {
                let line = self.current_token().line();
                self.advance()?;
                let Some(parameter) = value.context_variable_name().map(<[u8]>::to_vec) else {
                    return Err(parameter_name_error(self, &value, line));
                };
                name = Some(parameter);
                value = self.parse_expression(0)?;
            }

            match name {
                None => args.push(value),
                Some(name) => args.set_named(name, value),
            }
        }
        self.expect_token(
            TokenType::Punctuation,
            Some(")"),
            Some("A list of arguments must be closed by a parenthesis"),
        )?;
        Ok(args)
    }

    /// Parses call arguments into an `ArrayExpression` keyed by `LocalVariable`s (positions and names) or, with
    /// `preserve_names`, by `ConstantExpression`s for the names (macro calls).
    /// Twig: `ArgumentsTrait::parseCallableArguments()`.
    ///
    /// # Errors
    ///
    /// As [`parse_named_arguments`](Self::parse_named_arguments), and `You cannot assign a value to "%s".` for
    /// names like `True` used as keys.
    pub fn parse_callable_arguments(
        &mut self,
        line: i64,
        parse_open_parenthesis: bool,
        preserve_names: bool,
    ) -> Result<Expr> {
        let mut arguments = ArrayExpr::new();
        for (key, value) in self
            .parse_named_arguments(parse_open_parenthesis)?
            .into_entries()
        {
            let key = match key {
                ArgumentKey::Index(i) => Expr::local_variable(i, line)?,
                ArgumentKey::Name(name) if !preserve_names => Expr::local_variable(name, line)?,
                ArgumentKey::Name(name) => Expr::constant(ConstantValue::String(name), line),
            };
            arguments.add_element(value, Some(key));
        }
        Ok(arguments.into_expr(line))
    }
}

/// `A parameter name must be a string, "%s" given.`
#[cold]
#[inline(never)]
fn parameter_name_error(parser: &Parser<'_>, value: &Expr, line: i64) -> Error {
    parser.syntax_error(
        format!(
            "A parameter name must be a string, \"{}\" given.",
            value.class_name()
        ),
        line,
    )
}

/// The `node` child of a prefix operator's product (`$expr->getNode('node')`).
fn node_child(expr: &Expr) -> Option<&Expr> {
    match &expr.kind {
        ExprKind::Unary(unary) => Some(&unary.node),
        _ => expr
            .children()
            .into_iter()
            .find_map(|(key, child)| match (key, child) {
                (crate::ast::ChildKey::Name("node"), crate::ast::NodeRef::Expr(e)) => Some(e),
                _ => None,
            }),
    }
}

/// What a tag in a body gives (see [`Parser::subparse`]).
enum TagOutcome {
    /// The node of the tag.
    Node(Box<Node>),
    /// The token parser returned no node (deprecated).
    Nothing,
    /// The tag ends the body.
    End,
}

/// `count($rv) === 1 ? $rv[0] : new Nodes($rv, $lineno)`.
fn single_or_nodes(mut rv: Vec<Node>, line: i64) -> Node {
    if rv.len() == 1
        && let Some(node) = rv.pop()
    {
        return node;
    }
    Node::nodes(Nodes::from_list(rv), line)
}

/// Twig: `NodeDocumentation::add($node, ...$tokens)`: the documentation of the tokens, then the node's own, joined by
/// newlines.
pub(crate) fn add_node_documentation(node: &mut Node, tokens: &[&Token]) {
    add_documentation(&mut node.documentation, tokens);
}

/// Twig: `NodeDocumentation::add($expr, ...$tokens)` for an expression.
pub(crate) fn add_expr_documentation(expr: &mut Expr, tokens: &[&Token]) {
    add_documentation(&mut expr.documentation, tokens);
}

fn add_documentation(slot: &mut Option<Vec<u8>>, tokens: &[&Token]) {
    let mut documentation: Vec<&[u8]> = tokens.iter().filter_map(|t| t.documentation()).collect();
    let existing = slot.take();
    if let Some(existing) = &existing {
        documentation.push(existing);
    }
    if documentation.is_empty() {
        *slot = existing;
        return;
    }
    *slot = Some(documentation.join(&b'\n'));
}

/// The documentation of the node at `path` below `node` (see [`DocumentationTarget::Child`]).
fn documentation_slot<'a>(node: NodeMut<'a>, path: &[Vec<u8>]) -> Option<&'a mut Option<Vec<u8>>> {
    let Some((first, rest)) = path.split_first() else {
        return match node {
            NodeMut::Node(node) => Some(&mut node.documentation),
            NodeMut::Expr(expr) => Some(&mut expr.documentation),
            NodeMut::Arguments(_) => None,
        };
    };
    let children = match node {
        NodeMut::Node(node) => node.children_mut(),
        NodeMut::Expr(expr) => expr.children_mut(),
        NodeMut::Arguments(arguments) => arguments.children_mut(),
    };
    let child = children.into_iter().find_map(|(key, child)| {
        let matches = match key {
            ChildKey::Name(name) => name.as_bytes() == first.as_slice(),
            ChildKey::Index(index) => index.to_string().as_bytes() == first.as_slice(),
            ChildKey::Key(key) => key == first.as_slice(),
        };
        matches.then_some(child)
    })?;
    documentation_slot(child, rest)
}

/// Twig: `Parser::cleanupBodyForChildTemplates()`: block references and blank texts at the root of a child
/// template are removed (keys are kept).
fn cleanup_body_for_child_templates(mut body: Node) -> Node {
    match &body.kind {
        NodeKind::BlockReference(_) => return Node::empty(0),
        NodeKind::Text(text) if text.is_blank() => return Node::empty(0),
        _ => {}
    }
    if let Some(nodes) = body.as_nodes_mut() {
        nodes.retain(|_, node| match &node.kind {
            NodeKind::BlockReference(_) => false,
            NodeKind::Text(text) => !text.is_blank(),
            _ => true,
        });
    }
    body
}
