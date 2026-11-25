//! The code generator: lowers a visited `ModuleNode` into a [`Program`], mirroring each node's `compile()` method.
//!
//! The generator walks the tree in the order Twig's compiler writes PHP (`ModuleNode::compileTemplate()`: the
//! constructor with the traits, `doGetParent()`, `doDisplay()`, the blocks in definition order, the macros, then the
//! embedded templates), so that:
//!
//! - expressions are evaluated in PHP's order (left to right, keys before values in array literals, the right operand
//!   of `matches` first, the double evaluations of `?:`, `??` and `default`);
//! - the line markers of `Compiler::addDebugInfo()` are emitted at the same points with the same file-wide
//!   "only when the line changed" state, which is what makes `Error::guess()` reproducible from the line tables;
//! - the compile-time decisions that change runtime behaviour are taken from the same flags (`strict_variables`, the
//!   `SandboxExtension`, `always_defined`, `ignore_strict_check`, `optimizable`, `with_loop`, ...), including the
//!   attribute mutations Twig performs while compiling (the `ignore_strict_check` a `GetAttrExpression` sets on its
//!   object node, the short-circuit marks of null-safe chains shared by `??`, the single temporary of `apply`).

use std::collections::HashMap;
use std::sync::Arc;

use twig_value::{Array, ArrayKey, Diagnostics, Ignore, PhpString, Value, ops};

use super::instr::{
    BinaryOp, BlockOp, ConstId, GetAttrOp, Instr, Label, LocalId, NameMode, UnitId, YieldArgs,
};
use super::program::{
    BlockDef, FileId, LineTable, MacroDef, MacroParam, ParentDef, Program, SecurityLists, TraitDef,
    Unit, UnitKind,
};
use crate::ast::{
    self, ArgumentKey, Arguments, BinaryExpr, CallType, ConstantValue, Expr, ExprKind, FilterKind,
    ForNode, FunctionKind, GetAttrExpr, MacroName, Node, NodeKey, NodeKind, NodeRef, TempName,
    TestKind, UnaryOp,
};
use crate::deprecation::DeprecationSink;
use crate::error::{Error, Result};
use crate::extension::arguments::CallableArgumentsExtractor;
use crate::extension::{CallableKind, TwigCallable};
use crate::source::Source;

/// The environment facts compiled code depends on (Twig reads them from the environment while compiling).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileOptions {
    /// `strict_variables`: names compile to existence checks and `[]` access is not optimized.
    pub strict_variables: bool,
    /// Whether the `SandboxExtension` is registered (`$env->hasExtension(SandboxExtension::class)`).
    pub sandboxed: bool,
}

/// Compiles a visited `ModuleNode` (and its embedded templates) into a [`Program`].
///
/// Twig: `Compiler::compile($node)` with `ModuleNode::compile()` (TWIG/src/Compiler.php,
/// TWIG/src/Node/ModuleNode.php).
///
/// # Errors
///
/// The errors Twig raises while compiling: `SyntaxError`s with the template source (argument binding,
/// `Empty array elements are only allowed in destructuring assignments.`) and `LogicException`-kind errors
/// (callables that cannot be reflected, malformed trees, node classes defined by extensions). The caller wraps them
/// like `Environment::compileSource()` does.
pub fn compile(
    module: &Node,
    options: &CompileOptions,
    file: FileId,
    deprecations: &mut dyn DeprecationSink,
) -> Result<Arc<Program>> {
    let mut codegen = Codegen {
        options: *options,
        deprecations,
        file,
        last_line: 0,
        marker: None,
        program: None,
        units: Vec::new(),
        null_safe: HashMap::new(),
        apply_temps: Vec::new(),
    };
    codegen.module(module)
}

/// The short-circuit state of a null-safe `GetAttrExpression` (its `is_short_circuited` and `var_name` attributes).
#[derive(Debug, Clone, Copy)]
struct NullSafeState {
    short_circuited: bool,
    var: Option<LocalId>,
}

/// A constant-pool key for deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    Null,
    Bool(bool),
    Int(i64),
    Str(Vec<u8>),
}

struct ProgramBuilder {
    source: Source,
    index: Option<i64>,
    constants: Vec<Value>,
    const_index: HashMap<ConstKey, ConstId>,
    callables: Vec<TwigCallable>,
    units: Vec<Unit>,
    security: Option<SecurityLists>,
}

/// The `$macros` slot a `MacroVariable` names.
enum MacroSlot {
    /// `_self`: `$this->getMacroNamespace()`.
    This,
    /// `$macros[key]`.
    Key(Vec<u8>),
}

struct UnitBuilder {
    kind: UnitKind,
    code: Vec<Instr>,
    lines: LineTable,
    current: Option<i64>,
    locals: u16,
    named_locals: HashMap<TempName, LocalId>,
}

struct Codegen<'d> {
    options: CompileOptions,
    deprecations: &'d mut dyn DeprecationSink,
    file: FileId,
    /// Twig's `Compiler::$lastLine` (`null` compares like `0`).
    last_line: i64,
    /// The line of the last emitted marker, `None` before the first one.
    marker: Option<i64>,
    program: Option<ProgramBuilder>,
    units: Vec<UnitBuilder>,
    null_safe: HashMap<usize, NullSafeState>,
    apply_temps: Vec<LocalId>,
}

/// `Node "%s" does not exist for Node "%s".` (Twig's `Node::getNode()`).
fn missing_node(name: &str, class: &str) -> Error {
    Error::logic(format!(
        "Node \"{name}\" does not exist for Node \"{class}\"."
    ))
}

impl Codegen<'_> {
    // ---- emission ------------------------------------------------------------------------------------------------

    fn program(&mut self) -> &mut ProgramBuilder {
        self.program
            .as_mut()
            .expect("a program is being compiled while code is emitted")
    }

    fn unit(&mut self) -> &mut UnitBuilder {
        self.units
            .last_mut()
            .expect("a unit is being compiled while code is emitted")
    }

    /// Twig: `Compiler::addDebugInfo()`: a marker is emitted when the line differs from the last marker's.
    fn add_debug_info(&mut self, line: i64) {
        if line != self.last_line {
            self.last_line = line;
            self.marker = Some(line);
        }
    }

    /// Emits an instruction at the current marker.
    fn emit(&mut self, instr: Instr) -> Label {
        let line = self.marker;
        self.emit_at(instr, line)
    }

    /// Emits an instruction attributed to `line` (the marker in effect where the PHP construct starts: calls report
    /// the line of their beginning).
    fn emit_at(&mut self, instr: Instr, line: Option<i64>) -> Label {
        let unit = self.unit();
        let ip = unit.code.len() as Label;
        if line != unit.current {
            unit.lines.push(ip, line.unwrap_or(LineTable::NO_MARKER));
            unit.current = line;
        }
        unit.code.push(instr);
        ip
    }

    /// The label of the next instruction.
    fn here(&mut self) -> Label {
        self.unit().code.len() as Label
    }

    /// Points the jump at `at` to `target`.
    fn patch(&mut self, at: Label, target: Label) {
        let instr = &mut self.unit().code[at as usize];
        match instr {
            Instr::Jump(t) | Instr::JumpIfFalse(t) | Instr::JumpIfTrue(t) => *t = target,
            Instr::IterNext { exit, .. } => *exit = target,
            Instr::TryLoader { catch } => *catch = target,
            _ => {}
        }
    }

    /// Points the jump at `at` to the next instruction.
    fn patch_here(&mut self, at: Label) {
        let target = self.here();
        self.patch(at, target);
    }

    fn local(&mut self) -> LocalId {
        let unit = self.unit();
        let id = unit.locals;
        unit.locals = unit.locals.saturating_add(1);
        id
    }

    /// The local of a `LocalVariable`: the temporary of the enclosing `apply` for a name generated at compile time,
    /// otherwise one slot per name.
    fn named_local(&mut self, name: &TempName) -> LocalId {
        if *name == TempName::Null {
            if let Some(&slot) = self.apply_temps.last() {
                return slot;
            }
            return self.local();
        }
        if let Some(&slot) = self.unit().named_locals.get(name) {
            return slot;
        }
        let slot = self.local();
        self.unit().named_locals.insert(name.clone(), slot);
        slot
    }

    fn constant(&mut self, value: Value) -> ConstId {
        let key = match &value {
            Value::Null => Some(ConstKey::Null),
            Value::Bool(b) => Some(ConstKey::Bool(*b)),
            Value::Int(i) => Some(ConstKey::Int(*i)),
            Value::String(s) => Some(ConstKey::Str(s.as_bytes().to_vec())),
            _ => None,
        };
        let program = self.program();
        if let Some(key) = &key
            && let Some(&id) = program.const_index.get(key)
        {
            return id;
        }
        let id = program.constants.len() as ConstId;
        program.constants.push(value);
        if let Some(key) = key {
            program.const_index.insert(key, id);
        }
        id
    }

    fn name_constant(&mut self, name: &[u8]) -> ConstId {
        self.constant(Value::String(PhpString::from(name)))
    }

    fn emit_const(&mut self, value: Value) {
        let id = self.constant(value);
        self.emit(Instr::Const(id));
    }

    fn callable(&mut self, callable: &TwigCallable) -> u32 {
        let program = self.program();
        if let Some(i) = program.callables.iter().position(|c| c == callable) {
            return i as u32;
        }
        program.callables.push(callable.clone());
        (program.callables.len() - 1) as u32
    }

    fn source(&self) -> Source {
        self.program
            .as_ref()
            .map(|p| p.source.clone())
            .expect("a program is being compiled")
    }

    fn begin_unit(&mut self, kind: UnitKind) {
        self.units.push(UnitBuilder {
            kind,
            code: Vec::new(),
            lines: LineTable::default(),
            current: None,
            locals: 0,
            named_locals: HashMap::new(),
        });
    }

    fn finish_unit(&mut self) -> UnitId {
        let unit = self
            .units
            .pop()
            .expect("finish_unit() matches begin_unit()");
        let program = self.program();
        program.units.push(Unit {
            kind: unit.kind,
            code: unit.code,
            lines: unit.lines,
            locals: unit.locals,
        });
        (program.units.len() - 1) as UnitId
    }

    // ---- modules -------------------------------------------------------------------------------------------------

    /// Twig: `ModuleNode::compileTemplate()` and the subcompilation of the embedded templates.
    fn module(&mut self, node: &Node) -> Result<Arc<Program>> {
        let NodeKind::Module(module) = &node.kind else {
            return Err(Error::logic(format!(
                "Cannot compile a \"{}\" node as a template.",
                node.class_name()
            )));
        };
        let saved = self.program.replace(ProgramBuilder {
            source: module.source.clone(),
            index: module.index,
            constants: Vec::new(),
            const_index: HashMap::new(),
            callables: Vec::new(),
            units: Vec::new(),
            security: None,
        });
        let result = self.module_body(module);
        let builder = std::mem::replace(&mut self.program, saved)
            .expect("the program compiled by module_body()");
        let parts = result?;
        Ok(Arc::new(Program {
            file: self.file,
            source: builder.source,
            index: builder.index,
            constants: builder.constants,
            callables: builder.callables,
            units: builder.units,
            body: parts.body,
            blocks: parts.blocks,
            macros: parts.macros,
            parent: parts.parent,
            traits: parts.traits,
            embedded: parts.embedded,
            traitable: parts.traitable,
            strategy: module.strategy.clone(),
            escaper: module.escaper,
            security: builder.security,
        }))
    }

    fn module_body(&mut self, module: &ast::ModuleNode) -> Result<ModuleParts> {
        // compileConstructor(): constructor_start, the traits, constructor_end
        self.statement(&module.constructor_start)?;
        let mut traits = Vec::new();
        if let Some(nodes) = module.traits.as_nodes() {
            for trait_node in nodes.nodes() {
                traits.push(self.trait_def(trait_node)?);
            }
        }
        self.statement(&module.constructor_end)?;

        // compileGetParent()
        let parent = match &module.parent {
            None => None,
            Some(expr) => match &expr.kind {
                ExprKind::Constant(constant) => {
                    self.add_debug_info(expr.line);
                    Some(ParentDef::Constant {
                        name: const_value(&constant.value),
                        line: expr.line,
                    })
                }
                _ => {
                    // `$macros = $this->macros;` then `return $this->load(<parent>, line);`
                    self.begin_unit(UnitKind::Parent);
                    self.add_debug_info(expr.line);
                    let start = self.marker;
                    self.expr(expr)?;
                    self.emit_at(
                        Instr::LoadTemplate {
                            line: expr.line,
                            index: None,
                        },
                        start,
                    );
                    self.emit(Instr::Return);
                    let unit = self.finish_unit();
                    Some(ParentDef::Dynamic {
                        unit,
                        line: expr.line,
                    })
                }
            },
        };

        // compileDisplay()
        self.begin_unit(UnitKind::Body);
        self.statement(&module.display_start)?;
        self.statement(&module.body)?;
        if let Some(expr) = &module.parent {
            // `$this->parent = $this->load(<parent>, line);` (constant parent) or `$this->getParent($context)`, then
            // `yield from <parent>->unwrap()->yield($context, array_merge($this->blocks, $blocks));`
            self.add_debug_info(expr.line);
            match &expr.kind {
                ExprKind::Constant(constant) => {
                    self.emit_const(const_value(&constant.value));
                    self.emit(Instr::LoadTemplate {
                        line: expr.line,
                        index: None,
                    });
                    self.emit(Instr::SetParent);
                }
                _ => {
                    self.emit(Instr::GetParent { this: true });
                }
            }
            self.emit(Instr::YieldTemplate(YieldArgs::Parent));
        }
        self.statement(&module.display_end)?;
        self.emit(Instr::Return);
        let body = self.finish_unit();

        // the blocks
        let mut blocks = Vec::new();
        for (name, block) in module.blocks() {
            let NodeKind::Block(block_node) = &block.kind else {
                continue;
            };
            // BlockNode::compile()
            self.add_debug_info(block.line);
            self.begin_unit(UnitKind::Block(name.to_vec()));
            self.statement(&block_node.body)?;
            self.emit(Instr::Return);
            let unit = self.finish_unit();
            blocks.push(BlockDef {
                name: name.to_vec(),
                unit,
                line: block.line,
            });
        }

        // compileMacros()
        let mut macros = Vec::new();
        for (_, macro_node) in module.macros() {
            macros.push(self.macro_def(macro_node)?);
        }

        let traitable = is_traitable(module);

        // compileClassFooter()
        self.statement(&module.class_end)?;

        // the embedded templates
        let mut embedded = Vec::new();
        for template in module.embedded_templates() {
            embedded.push(self.module(template)?);
        }

        Ok(ModuleParts {
            body,
            blocks,
            macros,
            parent,
            traits,
            embedded,
            traitable,
        })
    }

    /// The trait part of `ModuleNode::compileConstructor()`.
    fn trait_def(&mut self, node: &Node) -> Result<TraitDef> {
        let nodes = node
            .as_nodes()
            .ok_or_else(|| missing_node("template", &node.class_name()))?;
        let template = nodes
            .get(&NodeKey::Name(b"template".to_vec()))
            .ok_or_else(|| missing_node("template", &node.class_name()))?;
        let expr = template
            .as_expr()
            .ok_or_else(|| missing_node("template", &node.class_name()))?;
        self.add_debug_info(expr.line);
        let mut targets = Vec::new();
        if let Some(targets_node) = nodes
            .get(&NodeKey::Name(b"targets".to_vec()))
            .and_then(Node::as_nodes)
        {
            for (key, value) in targets_node.entries() {
                let name = match key {
                    NodeKey::Name(name) => name.clone(),
                    NodeKey::Index(i) => i.to_string().into_bytes(),
                };
                let alias = value
                    .as_expr()
                    .and_then(Expr::as_constant)
                    .map_or(Value::Null, const_value);
                targets.push((name, alias));
            }
        }
        Ok(TraitDef {
            template: expr.as_constant().map_or(Value::Null, const_value),
            line: expr.line,
            targets,
        })
    }

    /// Twig: `MacroNode::compile()`.
    fn macro_def(&mut self, node: &Node) -> Result<MacroDef> {
        let NodeKind::Macro(macro_node) = &node.kind else {
            return Err(Error::logic(format!(
                "Cannot compile a \"{}\" node as a macro.",
                node.class_name()
            )));
        };
        let mut params = Vec::new();
        if let ExprKind::Array(arguments) = &macro_node.arguments.kind {
            for (key, value) in &arguments.elements {
                let name: Vec<u8> = match &key.kind {
                    ExprKind::LocalVariable(TempName::String(name)) => {
                        TempName::strip_reserved_prefix(name).to_vec()
                    }
                    ExprKind::LocalVariable(TempName::Int(i)) => i.to_string().into_bytes(),
                    _ => Vec::new(),
                };
                let explicit_default =
                    !matches!(&value.kind, ExprKind::Constant(c) if c.is_implicit);
                params.push(MacroParam {
                    name: PhpString::from(name),
                    default: const_eval(value),
                    explicit_default,
                });
            }
        }
        self.begin_unit(UnitKind::Macro(macro_node.name.clone()));
        self.add_debug_info(node.line);
        self.emit(Instr::CaptureStart);
        self.statement(&macro_node.body)?;
        self.emit(Instr::CaptureEnd { markup: true });
        self.emit(Instr::Return);
        let unit = self.finish_unit();
        Ok(MacroDef {
            name: macro_node.name.clone(),
            unit,
            params,
            variadic: macro_node.variadic_name.clone(),
            line: node.line,
        })
    }

    // ---- statements ----------------------------------------------------------------------------------------------

    fn statement(&mut self, node: &Node) -> Result<()> {
        match &node.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => {
                let apply = node.tag.as_deref() == Some("apply");
                if apply {
                    let slot = self.local();
                    self.apply_temps.push(slot);
                }
                let mut result = Ok(());
                for child in nodes.nodes() {
                    result = self.statement(child);
                    if result.is_err() {
                        break;
                    }
                }
                if apply {
                    self.apply_temps.pop();
                }
                result
            }
            NodeKind::Text(text) => {
                // TextNode::compile()
                self.add_debug_info(node.line);
                let id = self.constant(Value::String(PhpString::from(text.data.as_slice())));
                self.emit(Instr::Text(id));
                Ok(())
            }
            NodeKind::Print(print) => {
                // PrintNode::compile()
                self.add_debug_info(node.line);
                self.expr(&print.expr)?;
                self.emit(Instr::Print);
                Ok(())
            }
            NodeKind::If(if_node) => self.if_node(node, if_node),
            NodeKind::For(for_node) => self.for_node(node, for_node),
            NodeKind::Set(set) => self.set_node(node, set),
            NodeKind::Capture(capture) => {
                self.capture(capture)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            NodeKind::With(with) => {
                // WithNode::compile()
                self.add_debug_info(node.line);
                let saved = self.local();
                self.emit(Instr::LoadContext);
                self.emit(Instr::StoreLocal(saved));
                if let Some(variables) = &with.variables {
                    let local = self.local();
                    self.expr(variables)?;
                    self.emit(Instr::StoreLocal(local));
                    self.emit(Instr::WithVars {
                        local,
                        only: with.only,
                        line: variables.line,
                    });
                }
                self.statement(&with.body)?;
                self.emit(Instr::LoadLocal(saved));
                self.emit(Instr::SetContext);
                Ok(())
            }
            NodeKind::Do(do_node) => {
                // DoNode::compile()
                self.add_debug_info(node.line);
                self.expr(&do_node.expr)?;
                self.emit(Instr::Pop);
                Ok(())
            }
            NodeKind::Flush => {
                // FlushNode::compile()
                self.add_debug_info(node.line);
                self.emit(Instr::Flush);
                Ok(())
            }
            NodeKind::Deprecated(deprecated) => {
                // DeprecatedNode::compile(): package, version, then the message
                self.add_debug_info(node.line);
                let start = self.marker;
                if let Some(package) = &deprecated.package {
                    self.expr(package)?;
                }
                if let Some(version) = &deprecated.version {
                    self.expr(version)?;
                }
                self.expr(&deprecated.expr)?;
                self.emit_at(
                    Instr::Deprecated {
                        has_package: deprecated.package.is_some(),
                        has_version: deprecated.version.is_some(),
                        line: node.line,
                    },
                    start,
                );
                Ok(())
            }
            NodeKind::AutoEscape(auto_escape) => self.statement(&auto_escape.body),
            NodeKind::BlockReference(block) => {
                // BlockReferenceNode::compile(): `yield from $this->unwrap()->yieldBlock('NAME', $context, $blocks);`
                self.add_debug_info(node.line);
                self.emit_const(Value::String(PhpString::from(block.name.as_slice())));
                self.emit(Instr::Block {
                    op: BlockOp::Yield,
                    template: false,
                });
                Ok(())
            }
            NodeKind::Include(include) => self.include(
                node.line,
                IncludeTarget::Expr(&include.expr),
                include.variables.as_ref(),
                include.only,
                include.ignore_missing,
            ),
            NodeKind::Embed(embed) => self.include(
                node.line,
                IncludeTarget::Embedded {
                    name: &embed.name,
                    index: embed.index,
                },
                embed.variables.as_ref(),
                embed.only,
                embed.ignore_missing,
            ),
            NodeKind::Import(import) => self.import(node, import),
            NodeKind::Sandbox(sandbox) => {
                // SandboxNode::compile(): the sandbox is switched on around the body, in a try/finally
                self.add_debug_info(node.line);
                let local = self.local();
                self.emit(Instr::SandboxStart { local });
                self.statement(&sandbox.body)?;
                self.emit(Instr::SandboxEnd { local });
                Ok(())
            }
            NodeKind::Expression(expr) => match &expr.kind {
                ExprKind::BlockReference(b) if b.output && !b.defined_test => self.expr(expr),
                ExprKind::Parent(p) if p.output => self.expr(expr),
                _ => {
                    self.expr(expr)?;
                    self.emit(Instr::Pop);
                    Ok(())
                }
            },
            NodeKind::Empty
            | NodeKind::Macros(_)
            | NodeKind::Block(_)
            | NodeKind::Macro(_)
            | NodeKind::MacroDeclaration(_)
            | NodeKind::Config
            | NodeKind::Types(_)
            | NodeKind::Type(_)
            | NodeKind::ForLoop(_)
            | NodeKind::ForElse(_)
            | NodeKind::CheckSecurityCall => Ok(()),
            NodeKind::CheckSecurity(check) => {
                // CheckSecurityNode::compile(): `ensureSecurityChecked()` and `checkSecurity()` with the four lists
                self.program().security = Some(SecurityLists {
                    tags: check.used_tags.clone(),
                    filters: check.used_filters.clone(),
                    functions: check.used_functions.clone(),
                    tests: check.used_tests.clone(),
                });
                Ok(())
            }
            NodeKind::Module(_) => Err(Error::logic(
                "A \"Twig\\Node\\ModuleNode\" cannot be compiled inside a template body.",
            )),
            NodeKind::Custom(custom) => Err(Error::logic(format!(
                "The \"{}\" node class cannot be compiled by twig-rs.",
                custom.class_name()
            ))),
        }
    }

    /// Twig: `IfNode::compile()`.
    fn if_node(&mut self, node: &Node, if_node: &ast::IfNode) -> Result<()> {
        self.add_debug_info(node.line);
        let tests = if_node.tests.as_nodes();
        let count = tests.map_or(0, ast::Nodes::len) as i64;
        let mut end_jumps = Vec::new();
        let mut i = 0;
        while i < count {
            let tests = if_node.tests.as_nodes();
            let condition = tests
                .and_then(|t| t.get(&NodeKey::Index(i)))
                .ok_or_else(|| missing_node(&i.to_string(), "Twig\\Node\\Nodes"))?;
            match condition.as_expr() {
                Some(expr) => self.expr(expr)?,
                None => self.statement(condition)?,
            }
            let skip = self.emit(Instr::JumpIfFalse(0));
            if let Some(body) = tests.and_then(|t| t.get(&NodeKey::Index(i + 1))) {
                self.statement(body)?;
            }
            end_jumps.push(self.emit(Instr::Jump(0)));
            self.patch_here(skip);
            i += 2;
        }
        if let Some(else_node) = &if_node.else_ {
            self.statement(else_node)?;
        }
        for jump in end_jumps {
            self.patch_here(jump);
        }
        Ok(())
    }

    /// Twig: `ForNode::compile()` with `ForLoopNode::compile()` and `ForElseNode::compile()`.
    fn for_node(&mut self, node: &Node, for_node: &ForNode) -> Result<()> {
        self.add_debug_info(node.line);
        let key_name = for_node
            .key_target
            .context_variable_name()
            .unwrap_or(b"_key")
            .to_vec();
        let value_name = for_node
            .value_target
            .context_variable_name()
            .unwrap_or_default()
            .to_vec();
        let key = self.name_constant(&key_name);
        let value = self.name_constant(&value_name);
        let has_else = for_node.else_.is_some();
        let with_loop = for_node.with_loop;

        self.emit(Instr::ForParent);
        let start = self.marker;
        self.expr(&for_node.seq)?;
        self.emit_at(Instr::EnsureTraversable, start);
        let seq = self.name_constant(b"_seq");
        self.emit(Instr::StoreName(seq));
        if has_else {
            let iterated = self.name_constant(b"_iterated");
            self.emit_const(Value::Bool(false));
            self.emit(Instr::StoreName(iterated));
        }
        if with_loop {
            self.emit(Instr::LoopInit);
        }
        self.emit(Instr::IterStart);
        let head = self.emit(Instr::IterNext {
            exit: 0,
            key,
            value,
        });
        match &for_node.body.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => {
                for child in nodes.nodes() {
                    if let NodeKind::ForLoop(_) = child.kind {
                        self.for_loop_end(has_else, with_loop);
                    } else {
                        self.statement(child)?;
                    }
                }
            }
            _ => self.statement(&for_node.body)?,
        }
        self.emit(Instr::Jump(head));
        self.patch_here(head);
        self.emit(Instr::IterEnd);
        if let Some(else_node) = &for_node.else_ {
            // ForElseNode::compile(): `if (!$context['_iterated']) { ... }`
            self.add_debug_info(else_node.line);
            let iterated = self.name_constant(b"_iterated");
            self.emit(Instr::LoadName {
                name: iterated,
                mode: NameMode::AlwaysDefined,
            });
            let skip = self.emit(Instr::JumpIfTrue(0));
            match &else_node.kind {
                NodeKind::ForElse(for_else) => self.statement(&for_else.body)?,
                _ => self.statement(else_node)?,
            }
            self.patch_here(skip);
        }
        self.emit(Instr::ForRestore {
            key,
            value,
            has_else,
            with_loop,
        });
        Ok(())
    }

    /// Twig: `ForLoopNode::compile()` with the attributes `ForNode::compile()` copies to it.
    fn for_loop_end(&mut self, has_else: bool, with_loop: bool) {
        if has_else {
            let iterated = self.name_constant(b"_iterated");
            self.emit_const(Value::Bool(true));
            self.emit(Instr::StoreName(iterated));
        }
        if with_loop {
            self.emit(Instr::LoopUpdate);
        }
    }

    /// Twig: `SetNode::compile()`.
    fn set_node(&mut self, node: &Node, set: &ast::SetNode) -> Result<()> {
        self.add_debug_info(node.line);
        let names: Vec<&Node> = match &set.names.kind {
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => nodes.nodes().collect(),
            _ => vec![&set.names],
        };
        if names.len() > 1 {
            // [$context["a"], $context["b"]] = [<v1>, <v2>];
            if set.capture {
                self.statement_value(&set.values)?;
            } else {
                self.emit(Instr::NewArray);
                let values: Vec<&Node> = match &set.values.kind {
                    NodeKind::Nodes(nodes) | NodeKind::Body(nodes) => nodes.nodes().collect(),
                    _ => vec![&set.values],
                };
                for value in values {
                    self.statement_value(value)?;
                    self.emit(Instr::ArrayAppend);
                }
            }
            let mut targets = Array::new();
            for name in &names {
                let target = name
                    .as_expr()
                    .and_then(Expr::context_variable_name)
                    .unwrap_or_default();
                let _ = targets.push(Value::String(PhpString::from(target)));
            }
            let targets = self.constant(Value::Array(targets));
            self.emit(Instr::ListAssign(targets));
            self.emit(Instr::Pop);
            return Ok(());
        }

        if set.capture {
            self.statement_value(&set.values)?;
        } else if set.safe {
            match set.values.as_expr().map(|e| &e.kind) {
                Some(ExprKind::Constant(constant)) => {
                    if constant.value == ConstantValue::String(Vec::new()) {
                        self.emit_const(Value::String(PhpString::empty()));
                    } else {
                        self.emit_const(const_value(&constant.value));
                        self.emit(Instr::MakeMarkup);
                    }
                }
                _ => {
                    self.statement_value(&set.values)?;
                    self.emit(Instr::MarkupUnlessEmpty);
                }
            }
        } else {
            self.statement_value(&set.values)?;
        }
        match names.first() {
            Some(target) => self.store_target(target),
            None => {
                self.emit(Instr::Pop);
                Ok(())
            }
        }
    }

    /// A node that compiles to a value (an expression, a `CaptureNode`, or a `Nodes` container holding one).
    fn statement_value(&mut self, node: &Node) -> Result<()> {
        match &node.kind {
            NodeKind::Expression(expr) => self.expr(expr),
            NodeKind::Capture(capture) => self.capture(capture),
            NodeKind::Nodes(nodes) | NodeKind::Body(nodes) if nodes.len() == 1 => {
                match nodes.nodes().next() {
                    Some(child) => self.statement_value(child),
                    None => Ok(()),
                }
            }
            _ => Err(Error::logic(format!(
                "A \"{}\" node does not compile to a value.",
                node.class_name()
            ))),
        }
    }

    /// Stores the top of the stack into an assignment target (`AssignContextVariable` or `LocalVariable`).
    fn store_target(&mut self, target: &Node) -> Result<()> {
        let Some(expr) = target.as_expr() else {
            return Err(Error::logic(format!(
                "Cannot assign to a \"{}\" node.",
                target.class_name()
            )));
        };
        match &expr.kind {
            ExprKind::AssignContextVariable(name) | ExprKind::ContextVariable(name) => {
                let id = self.name_constant(&name.name);
                self.emit(Instr::StoreName(id));
                Ok(())
            }
            ExprKind::LocalVariable(name) => {
                let slot = self.named_local(name);
                self.emit(Instr::StoreLocal(slot));
                Ok(())
            }
            _ => Err(Error::logic(format!(
                "Cannot assign to a \"{}\" node.",
                expr.class_name()
            ))),
        }
    }

    /// Twig: `CaptureNode::compile()`.
    fn capture(&mut self, capture: &ast::CaptureNode) -> Result<()> {
        self.emit(Instr::CaptureStart);
        self.statement(&capture.body)?;
        self.emit(Instr::CaptureEnd {
            markup: !capture.raw,
        });
        Ok(())
    }

    // ---- expressions ---------------------------------------------------------------------------------------------

    fn expr(&mut self, expr: &Expr) -> Result<()> {
        self.expr_with(expr, false)
    }

    /// Compiles an expression; `ignore_strict` stands for the `ignore_strict_check` attribute a parent
    /// `GetAttrExpression` sets on its object node while compiling.
    fn expr_with(&mut self, expr: &Expr, ignore_strict: bool) -> Result<()> {
        match &expr.kind {
            ExprKind::Constant(constant) => {
                // ConstantExpression::compile()
                if constant.defined_test {
                    self.emit_const(Value::Bool(true));
                } else {
                    self.emit_const(const_value(&constant.value));
                }
                Ok(())
            }
            ExprKind::ContextVariable(name) => {
                self.name(expr, name, ignore_strict);
                Ok(())
            }
            ExprKind::AssignContextVariable(name) => {
                // AssignContextVariable::compile(): `$context["name"]`
                let id = self.name_constant(&name.name);
                self.emit(Instr::LoadName {
                    name: id,
                    mode: NameMode::AlwaysDefined,
                });
                Ok(())
            }
            ExprKind::LocalVariable(name) => {
                let slot = self.named_local(name);
                self.emit(Instr::LoadLocal(slot));
                Ok(())
            }
            ExprKind::MacroVariable(_) => {
                // MacroVariable::compile()
                match self.macro_slot(expr)? {
                    MacroSlot::This => self.emit(Instr::SelfMacroNamespace),
                    MacroSlot::Key(key) => {
                        let name = self.name_constant(&key);
                        self.emit(Instr::MacroNamespace {
                            name,
                            line: expr.line,
                        })
                    }
                };
                Ok(())
            }
            ExprKind::AssignMacroVariable(_) => Err(Error::logic(
                "A \"Twig\\Node\\Expression\\Variable\\AssignMacroVariable\" only compiles as the target of an import.",
            )),
            ExprKind::Array(array) => self.array(expr, array),
            ExprKind::ArrowFunction(arrow) => self.arrow(expr, arrow),
            ExprKind::List(_) => Err(Error::logic(
                "A \"Twig\\Node\\Expression\\ListExpression\" only compiles as arrow function parameters.",
            )),
            ExprKind::Unary(unary) => {
                // AbstractUnary::compile()
                match unary.op {
                    UnaryOp::Spread => self.expr(&unary.node),
                    op => {
                        self.expr(&unary.node)?;
                        self.emit(match op {
                            UnaryOp::Neg => Instr::Neg,
                            UnaryOp::Pos => Instr::Pos,
                            UnaryOp::Not => Instr::Not,
                            UnaryOp::StringCast | UnaryOp::Spread => Instr::StringCast,
                        });
                        Ok(())
                    }
                }
            }
            ExprKind::Binary(binary) => self.binary(expr, binary),
            ExprKind::ConditionalTernary(ternary) => {
                // ConditionalTernary::compile(): ((test) ? (left) : (right))
                self.expr(&ternary.test)?;
                let otherwise = self.emit(Instr::JumpIfFalse(0));
                self.expr(&ternary.left)?;
                let end = self.emit(Instr::Jump(0));
                self.patch_here(otherwise);
                self.expr(&ternary.right)?;
                self.patch_here(end);
                Ok(())
            }
            ExprKind::GetAttr(get_attr) => self.get_attr(get_attr, ignore_strict),
            ExprKind::MacroReference(reference) => {
                // MacroReferenceExpression::compile(): `<template>->call(<name>, <arguments>, $context, line,
                // $this->source)` or `<template>->has(<name>, $context)`
                let start = self.marker;
                self.expr(&reference.template)?;
                match &reference.name {
                    MacroName::Static(name) => {
                        self.emit_const(Value::String(PhpString::from(name.as_slice())));
                    }
                    MacroName::Dynamic(name) => {
                        // `(string) <name>`
                        self.expr(name)?;
                        self.emit(Instr::StringCast);
                    }
                }
                if reference.defined_test {
                    self.emit_at(Instr::MacroDefined, start);
                } else {
                    self.expr(&reference.arguments)?;
                    self.emit_at(Instr::MacroCall { line: expr.line }, start);
                }
                Ok(())
            }
            ExprKind::BlockReference(block) => {
                // BlockReferenceExpression::compile(): `[yield from ]<target>->unwrap()->yieldBlock|renderBlock|hasBlock(
                // <name>, $context[, $blocks])` where the target is `$this` or `$this->load(<template>, line)`
                if block.output && !block.defined_test {
                    self.add_debug_info(expr.line);
                }
                let mut start = self.marker;
                if let Some(template) = &block.template {
                    self.expr(template)?;
                    self.emit_at(
                        Instr::LoadTemplate {
                            line: expr.line,
                            index: None,
                        },
                        start,
                    );
                    // `->unwrap()->renderBlock(` follows the template expression: PHP attributes a method call to
                    // the line of its name
                    start = self.marker;
                }
                self.expr(&block.name)?;
                let op = if block.defined_test {
                    BlockOp::Has
                } else if block.output {
                    BlockOp::Yield
                } else {
                    BlockOp::Render
                };
                let block = Instr::Block {
                    op,
                    template: block.template.is_some(),
                };
                if op == BlockOp::Yield {
                    // the body of the yieldBlock() generator runs on `yield from`, at the end of the statement
                    self.emit(block);
                } else {
                    self.emit_at(block, start);
                }
                Ok(())
            }
            ExprKind::Parent(parent) => {
                // ParentExpression::compile(): `yield from $this->yieldParentBlock("name", $context, $blocks);` or
                // `$this->renderParentBlock("name", $context, $blocks)`
                if parent.output {
                    self.add_debug_info(expr.line);
                }
                let name = self.name_constant(&parent.name);
                self.emit(Instr::ParentBlock {
                    name,
                    render: !parent.output,
                });
                Ok(())
            }
            ExprKind::Filter(filter) => self.filter(expr, filter),
            ExprKind::Function(function) => self.function(expr, function),
            ExprKind::Test(test) => self.test(expr, test),
            ExprKind::Empty => {
                // EmptyExpression::compile() writes nothing; only destructuring patterns contain it
                self.emit_const(Value::Null);
                Ok(())
            }
            ExprKind::CheckToString(check) => {
                // CheckToStringNode::compile()
                let start = self.marker;
                self.expr(&check.expr)?;
                self.emit_at(
                    Instr::CheckToString {
                        spread: check.spread,
                        line: check.expr.line,
                    },
                    start,
                );
                Ok(())
            }
            ExprKind::Custom(custom) => Err(Error::logic(format!(
                "The \"{}\" node class cannot be compiled by twig-rs.",
                custom.class_name()
            ))),
        }
    }

    /// Twig: `NameExpression::compile()`.
    fn name(&mut self, expr: &Expr, name: &ast::NameExpr, ignore_strict: bool) {
        self.add_debug_info(expr.line);
        let special = matches!(name.name.as_slice(), b"_self" | b"_context" | b"_charset");
        if name.defined_test {
            if special || expr.always_defined {
                self.emit_const(Value::Bool(true));
            } else {
                let id = self.name_constant(&name.name);
                self.emit(Instr::LoadName {
                    name: id,
                    mode: NameMode::Defined,
                });
            }
            return;
        }
        if special {
            self.emit(match name.name.as_slice() {
                b"_self" => Instr::LoadTemplateName,
                b"_context" => Instr::LoadContext,
                _ => Instr::LoadCharset,
            });
            return;
        }
        let mode = if expr.always_defined {
            NameMode::AlwaysDefined
        } else if name.ignore_strict_check || ignore_strict || !self.options.strict_variables {
            NameMode::Lenient
        } else {
            NameMode::Strict { line: expr.line }
        };
        let id = self.name_constant(&name.name);
        self.emit(Instr::LoadName { name: id, mode });
    }

    /// Twig: `ArrayExpression::compile()` (and `VariadicExpression::compile()`, whose `...` the caller adds).
    fn array(&mut self, expr: &Expr, array: &ast::ArrayExpr) -> Result<()> {
        if array.defined_test {
            self.emit_const(Value::Bool(true));
            return Ok(());
        }
        for (_, value) in &array.elements {
            if matches!(value.kind, ExprKind::Empty) {
                return Err(Error::syntax(
                    "Empty array elements are only allowed in destructuring assignments.",
                )
                .with_line(value.line)
                .with_source(self.source()));
            }
        }
        let _ = expr;
        self.emit(Instr::NewArray);
        let mut is_sequence = true;
        for (i, (key, value)) in array.elements.iter().enumerate() {
            let key_value: Option<Value> = match &key.kind {
                ExprKind::LocalVariable(name)
                | ExprKind::MacroVariable(ast::MacroVariableExpr { name, .. }) => {
                    Some(match name {
                        TempName::Null => Value::Null,
                        TempName::Int(i) => Value::Int(*i),
                        TempName::String(s) => Value::String(PhpString::from(s.as_slice())),
                    })
                }
                ExprKind::Constant(constant) => Some(const_value(&constant.value)),
                _ => None,
            };
            // `$key !== $i`
            if !matches!(&key_value, Some(Value::Int(k)) if *k == i as i64) {
                is_sequence = false;
            }
            let spread = match &value.kind {
                ExprKind::Unary(unary) if unary.op == UnaryOp::Spread => Some(&unary.node),
                _ => None,
            };
            if let Some(spread) = spread {
                self.expr(spread)?;
                self.emit(Instr::ArraySpread);
                continue;
            }
            if is_sequence {
                self.expr(value)?;
                self.emit(Instr::ArrayAppend);
                continue;
            }
            match key_value {
                Some(key_value) => self.emit_const(key_value),
                None => {
                    // a dynamic key: StringCastUnary
                    self.expr(key)?;
                    self.emit(Instr::StringCast);
                }
            }
            self.expr(value)?;
            self.emit(Instr::ArrayInsert);
        }
        Ok(())
    }

    /// Twig: `ArrowFunctionExpression::compile()`.
    fn arrow(&mut self, expr: &Expr, arrow: &ast::ArrowFunctionExpr) -> Result<()> {
        self.add_debug_info(expr.line);
        let start = self.marker;
        let mut params = Vec::new();
        if let ExprKind::List(list) = &arrow.names.kind {
            for item in &list.items {
                if let Some(name) = item.context_variable_name() {
                    params.push(PhpString::from(name));
                }
            }
        }
        self.begin_unit(UnitKind::Arrow(params));
        let result = self.expr(&arrow.expr);
        if result.is_ok() {
            self.emit(Instr::Return);
        }
        let unit = self.finish_unit();
        result?;
        self.emit_at(Instr::MakeArrow(unit), start);
        Ok(())
    }

    fn binary(&mut self, expr: &Expr, binary: &BinaryExpr) -> Result<()> {
        use ast::BinaryOp as B;
        let simple = |op: B| -> Option<BinaryOp> {
            Some(match op {
                B::Add => BinaryOp::Add,
                B::Sub => BinaryOp::Sub,
                B::Mul => BinaryOp::Mul,
                B::Div => BinaryOp::Div,
                B::FloorDiv => BinaryOp::FloorDiv,
                B::Mod => BinaryOp::Mod,
                B::Power => BinaryOp::Pow,
                B::Concat => BinaryOp::Concat,
                B::BitwiseAnd => BinaryOp::BitAnd,
                B::BitwiseOr => BinaryOp::BitOr,
                B::BitwiseXor => BinaryOp::BitXor,
                B::Equal => BinaryOp::Equal,
                B::NotEqual => BinaryOp::NotEqual,
                B::Less => BinaryOp::Less,
                B::Greater => BinaryOp::Greater,
                B::LessEqual => BinaryOp::LessEqual,
                B::GreaterEqual => BinaryOp::GreaterEqual,
                B::Spaceship => BinaryOp::Spaceship,
                B::SameAs => BinaryOp::Identical,
                B::NotSameAs => BinaryOp::NotIdentical,
                _ => return None,
            })
        };
        if binary.op == B::Power {
            // AbstractBinary::compile() writes `( -left ** right)` for a unary left operand without parentheses,
            // which PHP parses as `-(left ** right)`: `**` binds tighter than unary `-`/`+`.
            let mut peeled = Vec::new();
            let mut left = &binary.left;
            while let ExprKind::Unary(unary) = &left.kind {
                if left.with_parentheses || !matches!(unary.op, UnaryOp::Neg | UnaryOp::Pos) {
                    break;
                }
                peeled.push(unary.op);
                left = &unary.node;
            }
            self.expr(left)?;
            self.expr(&binary.right)?;
            self.emit(Instr::Binary(BinaryOp::Pow));
            for op in peeled.into_iter().rev() {
                self.emit(if op == UnaryOp::Neg {
                    Instr::Neg
                } else {
                    Instr::Pos
                });
            }
            return Ok(());
        }
        if let Some(op) = simple(binary.op) {
            // AbstractBinary::compile(): (left OP right)
            self.expr(&binary.left)?;
            self.expr(&binary.right)?;
            self.emit(Instr::Binary(op));
            return Ok(());
        }
        match binary.op {
            B::And => {
                // (T(left) && T(right))
                self.expr(&binary.left)?;
                let short = self.emit(Instr::JumpIfFalse(0));
                self.expr(&binary.right)?;
                self.emit(Instr::ToBool);
                let end = self.emit(Instr::Jump(0));
                self.patch_here(short);
                self.emit_const(Value::Bool(false));
                self.patch_here(end);
            }
            B::Or => {
                // (T(left) || T(right))
                self.expr(&binary.left)?;
                let short = self.emit(Instr::JumpIfTrue(0));
                self.expr(&binary.right)?;
                self.emit(Instr::ToBool);
                let end = self.emit(Instr::Jump(0));
                self.patch_here(short);
                self.emit_const(Value::Bool(true));
                self.patch_here(end);
            }
            B::Xor => {
                // (T(left) xor T(right))
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit(Instr::Binary(BinaryOp::Xor));
            }
            B::In | B::NotIn => {
                // [!]CoreExtension::inFilter(left, right)
                let start = self.marker;
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit_at(
                    Instr::In {
                        negate: binary.op == B::NotIn,
                    },
                    start,
                );
            }
            B::Matches => {
                // CoreExtension::matches(right, left)
                let start = self.marker;
                self.expr(&binary.right)?;
                self.expr(&binary.left)?;
                self.emit_at(Instr::Matches, start);
            }
            B::StartsWith | B::EndsWith => {
                // (is_string($a = left) && is_string($b = right) && str_starts_with($a, $b))
                let a = self.local();
                let b = self.local();
                self.expr(&binary.left)?;
                self.emit(Instr::StoreLocal(a));
                self.emit(Instr::LoadLocal(a));
                self.emit(Instr::IsString);
                let fail_a = self.emit(Instr::JumpIfFalse(0));
                self.expr(&binary.right)?;
                self.emit(Instr::StoreLocal(b));
                self.emit(Instr::LoadLocal(b));
                self.emit(Instr::IsString);
                let fail_b = self.emit(Instr::JumpIfFalse(0));
                self.emit(Instr::LoadLocal(a));
                self.emit(Instr::LoadLocal(b));
                self.emit(if binary.op == B::StartsWith {
                    Instr::StartsWith
                } else {
                    Instr::EndsWith
                });
                let end = self.emit(Instr::Jump(0));
                self.patch_here(fail_a);
                self.patch_here(fail_b);
                self.emit_const(Value::Bool(false));
                self.patch_here(end);
            }
            B::HasSome | B::HasEvery => {
                // CoreExtension::arraySome($this->env, left, right, <isSandboxed>)
                let start = self.marker;
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit_at(
                    if binary.op == B::HasSome {
                        Instr::HasSome
                    } else {
                        Instr::HasEvery
                    },
                    start,
                );
            }
            B::Range => {
                // range(left, right)
                let start = self.marker;
                self.expr(&binary.left)?;
                self.expr(&binary.right)?;
                self.emit_at(Instr::Range, start);
            }
            B::Elvis => {
                // ((test) ? (left) : (right)), test = TrueTest::wrap(clone left)
                let test = binary
                    .test
                    .as_ref()
                    .ok_or_else(|| missing_node("test", &expr.class_name()))?;
                self.expr(test)?;
                let otherwise = self.emit(Instr::JumpIfFalse(0));
                self.expr(&binary.left)?;
                let end = self.emit(Instr::Jump(0));
                self.patch_here(otherwise);
                self.expr(&binary.right)?;
                self.patch_here(end);
            }
            B::NullCoalesce => {
                // ((test) ? (left) : (right)); the null test of `test` shares its operand with `left`
                let test = binary
                    .test
                    .as_ref()
                    .ok_or_else(|| missing_node("test", &expr.class_name()))?;
                self.expr(test)?;
                if let Some(shared) = null_test_operand(test) {
                    self.share_null_safe_state(shared, &binary.left);
                }
                let otherwise = self.emit(Instr::JumpIfFalse(0));
                self.expr(&binary.left)?;
                let end = self.emit(Instr::Jump(0));
                self.patch_here(otherwise);
                self.expr(&binary.right)?;
                self.patch_here(end);
            }
            B::Set => {
                // ($context["name"] = right)
                self.expr(&binary.right)?;
                self.emit(Instr::Dup);
                let name = binary.left.context_variable_name().unwrap_or_default();
                let id = self.name_constant(name);
                self.emit(Instr::StoreName(id));
            }
            B::SequenceDestructuringSet => {
                // SequenceDestructuringSetBinary::compile()
                self.add_debug_info(expr.line);
                let mut names = Array::new();
                if let ExprKind::Array(array) = &binary.left.kind {
                    for (_, value) in &array.elements {
                        let _ = names.push(match value.context_variable_name() {
                            Some(name) => Value::String(PhpString::from(name)),
                            None => Value::Null,
                        });
                    }
                }
                let names = self.constant(Value::Array(names));
                self.expr(&binary.right)?;
                self.emit(Instr::SeqDestructure(names));
            }
            B::ObjectDestructuringSet => {
                // ObjectDestructuringSetBinary::compile()
                self.add_debug_info(expr.line);
                let mut mappings: Vec<(Value, Vec<u8>)> = Vec::new();
                if let ExprKind::Array(array) = &binary.left.kind {
                    for (key, value) in &array.elements {
                        let property = key.as_constant().map_or(Value::Null, const_value);
                        let variable = value.context_variable_name().unwrap_or_default().to_vec();
                        mappings.push((property, variable));
                    }
                }
                let object = self.local();
                let empty_arguments = self.constant(Value::Array(Array::new()));
                self.emit(Instr::NewArray);
                for (i, (property, _)) in mappings.iter().enumerate() {
                    let start = self.marker;
                    if i == 0 {
                        self.expr(&binary.right)?;
                        self.emit(Instr::Dup);
                        self.emit(Instr::StoreLocal(object));
                    } else {
                        self.emit(Instr::LoadLocal(object));
                    }
                    self.emit_const(property.clone());
                    self.emit(Instr::Const(empty_arguments));
                    self.emit_at(
                        Instr::GetAttr(GetAttrOp {
                            call_type: CallType::Any,
                            defined_test: false,
                            ignore_strict_check: false,
                            sandboxed: self.options.sandboxed,
                            line: binary.right.line,
                        }),
                        start,
                    );
                    self.emit(Instr::ArrayAppend);
                }
                let mut names = Array::new();
                for (_, variable) in &mappings {
                    let _ = names.push(Value::String(PhpString::from(variable.as_slice())));
                }
                let names = self.constant(Value::Array(names));
                self.emit(Instr::ListAssign(names));
            }
            _ => unreachable!("every simple operator is handled above"),
        }
        Ok(())
    }

    /// Twig: `GetAttrExpression::compile()`.
    fn get_attr(&mut self, get_attr: &GetAttrExpr, ignore_strict: bool) -> Result<()> {
        // a parent GetAttrExpression may have set `ignore_strict_check` on this node before compiling it
        let ignore_strict_check = get_attr.ignore_strict_check || ignore_strict;
        let mut sandbox_end = None;

        // optimize array calls
        if get_attr.optimizable
            && (!self.options.strict_variables || ignore_strict_check)
            && !get_attr.defined_test
            && get_attr.call_type == CallType::Array
        {
            let var = self.local();
            self.expr(&get_attr.node)?;
            self.emit(Instr::StoreLocal(var));
            self.emit(Instr::LoadLocal(var));
            self.emit(Instr::ArrayLike {
                sandboxed: self.options.sandboxed,
            });
            let otherwise = self.emit(Instr::JumpIfFalse(0));
            self.emit(Instr::LoadLocal(var));
            match &get_attr.attribute.kind {
                ExprKind::Constant(constant) => self.emit_const(const_value(&constant.value)),
                _ => {
                    self.expr(&get_attr.attribute)?;
                    self.emit(Instr::StringableKey { container: var });
                }
            }
            self.emit(Instr::DimIsset);
            let end = self.emit(Instr::Jump(0));
            self.patch_here(otherwise);
            if !self.options.sandboxed {
                self.emit_const(Value::Null);
                self.patch_here(end);
                return Ok(());
            }
            sandbox_end = Some(end);
        }

        // the null-safe node: this one, or the nearest null-safe node of the object chain
        let mut null_safe_node: Option<&GetAttrExpr> = None;
        if get_attr.null_safe {
            null_safe_node = Some(get_attr);
        } else {
            let mut node = &get_attr.node;
            while let ExprKind::GetAttr(inner) = &node.kind {
                if inner.null_safe {
                    null_safe_node = Some(inner);
                    break;
                }
                node = &inner.node;
            }
        }
        let mut short_circuit_end = None;
        if let Some(ns) = null_safe_node
            && !self.null_safe_state(ns).short_circuited
        {
            // ((null === ($_vK = <ns.node>)) ? null : ...)
            let var = self.null_safe_var(ns);
            // the object node of `ns` was given `ignore_strict_check` when `ns` is this node
            let ns_ignore = std::ptr::eq(ns, get_attr) && ignore_strict_check;
            self.expr_with(&ns.node, ns_ignore || ns.ignore_strict_check)?;
            self.emit(Instr::StoreLocal(var));
            self.emit(Instr::LoadLocal(var));
            self.emit(Instr::IsNull);
            let go = self.emit(Instr::JumpIfFalse(0));
            self.emit_const(Value::Null);
            short_circuit_end = Some(self.emit(Instr::Jump(0)));
            self.patch_here(go);
            self.mark_short_circuited(ns);
        }

        let start = self.marker;
        if get_attr.null_safe {
            let var = self.null_safe_var(get_attr);
            self.emit(Instr::LoadLocal(var));
        } else {
            self.expr_with(&get_attr.node, ignore_strict_check)?;
        }
        self.expr(&get_attr.attribute)?;
        match &get_attr.arguments {
            Some(arguments) => self.expr(arguments)?,
            None => self.emit_const(Value::Array(Array::new())),
        }
        self.emit_at(
            Instr::GetAttr(GetAttrOp {
                call_type: get_attr.call_type,
                defined_test: get_attr.defined_test,
                ignore_strict_check,
                sandboxed: self.options.sandboxed,
                line: get_attr.node.line,
            }),
            start,
        );
        if let Some(end) = sandbox_end {
            self.patch_here(end);
        }
        if let Some(end) = short_circuit_end {
            self.patch_here(end);
        }
        Ok(())
    }

    fn null_safe_key(node: &GetAttrExpr) -> usize {
        std::ptr::from_ref(node) as usize
    }

    fn null_safe_state(&self, node: &GetAttrExpr) -> NullSafeState {
        self.null_safe
            .get(&Self::null_safe_key(node))
            .copied()
            .unwrap_or(NullSafeState {
                short_circuited: false,
                var: None,
            })
    }

    /// Twig: `GetAttrExpression::getVarName()`: the variable is allocated once per node.
    fn null_safe_var(&mut self, node: &GetAttrExpr) -> LocalId {
        let key = Self::null_safe_key(node);
        if let Some(var) = self.null_safe.get(&key).and_then(|s| s.var) {
            return var;
        }
        let var = self.local();
        let state = self.null_safe.entry(key).or_insert(NullSafeState {
            short_circuited: false,
            var: None,
        });
        state.var = Some(var);
        var
    }

    fn mark_short_circuited(&mut self, node: &GetAttrExpr) {
        self.null_safe
            .entry(Self::null_safe_key(node))
            .or_insert(NullSafeState {
                short_circuited: false,
                var: None,
            })
            .short_circuited = true;
    }

    /// Copies the compile-time state of the null-safe nodes of `from` to the corresponding nodes of `to` (a clone of
    /// the same tree): in Twig both are one PHP object (`NullCoalesceBinary` shares its left operand with its null
    /// test).
    fn share_null_safe_state(&mut self, from: &Expr, to: &Expr) {
        if let (ExprKind::GetAttr(a), ExprKind::GetAttr(b)) = (&from.kind, &to.kind)
            && let Some(state) = self.null_safe.get(&Self::null_safe_key(a)).copied()
        {
            self.null_safe.insert(Self::null_safe_key(b), state);
        }
        let from_children = from.children();
        let to_children = to.children();
        for ((_, f), (_, t)) in from_children.iter().zip(to_children.iter()) {
            match (f, t) {
                (NodeRef::Expr(f), NodeRef::Expr(t)) => self.share_null_safe_state(f, t),
                (NodeRef::Arguments(f), NodeRef::Arguments(t)) => {
                    for ((_, f), (_, t)) in f.entries().iter().zip(t.entries().iter()) {
                        self.share_null_safe_state(f, t);
                    }
                }
                _ => {}
            }
        }
    }

    /// Twig: `FilterExpression::compile()` and the special filter classes.
    fn filter(&mut self, expr: &Expr, filter: &ast::FilterExpr) -> Result<()> {
        match filter.kind {
            // DefaultFilter::compile() and RawFilter::compile(): the node only
            FilterKind::Default | FilterKind::Raw => self.expr(&filter.node),
            FilterKind::Filter if filter.filter.name() == b"raw" => {
                self.deprecations.report(crate::deprecation::Deprecation::twig(
                    "3.11",
                    "Creating the \"raw\" filter via \"FilterExpression\" is deprecated; use \"RawFilter\" instead.",
                ));
                self.expr(&filter.node)
            }
            FilterKind::Filter | FilterKind::Escape { .. } => self.call(
                &TwigCallable::Filter(filter.filter.clone()),
                Some(&filter.node),
                Some(&filter.arguments),
                expr.line,
            ),
        }
    }

    /// Twig: `FunctionExpression::compile()`.
    fn function(&mut self, expr: &Expr, function: &ast::FunctionExpr) -> Result<()> {
        if function.kind == FunctionKind::RenderSandboxed {
            return self.render_sandboxed(expr, function);
        }
        if function.function.name() == b"constant" && function.defined_test {
            let mut arguments = function.arguments.clone();
            arguments.set_named(b"checkDefined".to_vec(), Expr::constant(true, expr.line));
            return self.call(
                &TwigCallable::Function(function.function.clone()),
                None,
                Some(&arguments),
                expr.line,
            );
        }
        self.call(
            &TwigCallable::Function(function.function.clone()),
            None,
            Some(&function.arguments),
            expr.line,
        )
    }

    /// Twig: `TestExpression::compile()` and the special test classes.
    fn test(&mut self, expr: &Expr, test: &ast::TestExpr) -> Result<()> {
        let argument = |i: i64| -> Result<&Expr> {
            let Some(arguments) = &test.arguments else {
                return Err(missing_node("arguments", test.kind.class_name()));
            };
            arguments
                .get(&ast::ArgumentKey::Index(i))
                .ok_or_else(|| missing_node(&i.to_string(), arguments.class_name()))
        };
        match test.kind {
            TestKind::Test | TestKind::NonStringCoercingInput => self.call(
                &TwigCallable::Test(test.test.clone()),
                Some(&test.node),
                test.arguments.as_ref(),
                expr.line,
            ),
            TestKind::Defined => self.expr(&test.node),
            TestKind::Null => {
                self.expr(&test.node)?;
                self.emit(Instr::IsNull);
                Ok(())
            }
            TestKind::SameAs => {
                self.expr(&test.node)?;
                self.expr(argument(0)?)?;
                self.emit(Instr::Binary(BinaryOp::Identical));
                Ok(())
            }
            TestKind::Even | TestKind::Odd => {
                self.expr(&test.node)?;
                self.emit_const(Value::Int(2));
                self.emit(Instr::Binary(BinaryOp::Mod));
                self.emit_const(Value::Int(0));
                self.emit(Instr::Binary(if test.kind == TestKind::Even {
                    BinaryOp::Equal
                } else {
                    BinaryOp::NotEqual
                }));
                Ok(())
            }
            TestKind::DivisibleBy => {
                // (0 == node % arguments[0])
                self.emit_const(Value::Int(0));
                self.expr(&test.node)?;
                self.expr(argument(0)?)?;
                self.emit(Instr::Binary(BinaryOp::Mod));
                self.emit(Instr::Binary(BinaryOp::Equal));
                Ok(())
            }
            TestKind::Constant => {
                // (node === constant([get_class(arguments[1])."::".]arguments[0]))
                self.expr(&test.node)?;
                let start = self.marker;
                let with_object = test
                    .arguments
                    .as_ref()
                    .is_some_and(|a| a.get(&ast::ArgumentKey::Index(1)).is_some());
                if with_object {
                    self.expr(argument(1)?)?;
                }
                self.expr(argument(0)?)?;
                self.emit_at(Instr::PhpConstant { with_object }, start);
                self.emit(Instr::Binary(BinaryOp::Identical));
                Ok(())
            }
            TestKind::True => {
                self.expr(&test.node)?;
                self.emit(Instr::TrueTest);
                Ok(())
            }
        }
    }

    /// Twig: `RenderSandboxedFunction::compile()`: the literal `output_strategy` argument (position 2 or a name that
    /// normalises to `outputstrategy`) is checked and removed, the rest compiles as a plain function call.
    fn render_sandboxed(&mut self, expr: &Expr, function: &ast::FunctionExpr) -> Result<()> {
        let strategy_key = function
            .arguments
            .entries()
            .iter()
            .map(|(key, _)| key)
            .find(|key| match key {
                ArgumentKey::Index(i) => *i == 2,
                ArgumentKey::Name(name) => {
                    let normalized: Vec<u8> = name
                        .iter()
                        .filter(|&&b| b != b'_')
                        .map(u8::to_ascii_lowercase)
                        .collect();
                    normalized == b"outputstrategy"
                }
            })
            .cloned();
        let Some(strategy_key) = strategy_key else {
            return Err(Error::syntax(
                "Value for argument \"output_strategy\" is required for function \"render_sandboxed\".",
            )
            .with_line(expr.line)
            .with_source(self.source()));
        };
        let mut arguments = function.arguments.clone();
        let strategy = arguments
            .remove(&strategy_key)
            .ok_or_else(|| missing_node("output_strategy", "Twig\\Node\\Nodes"))?;
        let valid = matches!(
            &strategy.kind,
            ExprKind::Constant(constant)
                if matches!(&constant.value, ConstantValue::String(s) if !s.is_empty() && s.as_slice() != b"all")
        );
        if !valid {
            return Err(Error::syntax(
                "The \"output_strategy\" argument of the \"render_sandboxed\" function must be a non-empty literal string other than \"all\".",
            )
            .with_line(strategy.line)
            .with_source(self.source()));
        }
        self.call(
            &TwigCallable::Function(function.function.clone()),
            None,
            Some(&arguments),
            expr.line,
        )
    }

    /// The `$macros` slot of a `MacroVariable`: `_self`, its name, or the name Twig's compiler gives a nameless one
    /// (`{% from %}`) the first time it compiles it (`Compiler::getVarName()`); twig-rs derives that name from the
    /// variable's identity, so the import and the calls it enables agree.
    fn macro_slot(&mut self, var: &Expr) -> Result<MacroSlot> {
        let ExprKind::MacroVariable(variable) = &var.kind else {
            return Err(Error::logic(format!(
                "A \"{}\" node cannot name a macro namespace.",
                var.class_name()
            )));
        };
        Ok(match &variable.name {
            TempName::String(name) if name.as_slice() == b"_self" => MacroSlot::This,
            other => MacroSlot::Key(macro_key(other, variable.id)),
        })
    }

    /// Twig: `ImportNode::compile()`: `$macros["name"] = [$this->macros["name"] = ]` (AssignMacroVariable, with its
    /// marker), then `$this->getMacroNamespace()` for `_self` or `$this->load(<template>,
    /// line)->unwrap()->getMacroNamespace()`.
    fn import(&mut self, node: &Node, import: &ast::ImportNode) -> Result<()> {
        let ExprKind::AssignMacroVariable(assign) = &import.var.kind else {
            return Err(missing_node("var", "Twig\\Node\\ImportNode"));
        };
        let ExprKind::MacroVariable(variable) = &assign.var.kind else {
            return Err(missing_node(
                "var",
                "Twig\\Node\\Expression\\Variable\\AssignMacroVariable",
            ));
        };
        self.add_debug_info(import.var.line);
        let start = self.marker;
        if import.expr.context_variable_name() == Some(b"_self".as_slice()) {
            self.emit(Instr::SelfMacroNamespace);
        } else {
            self.expr(&import.expr)?;
            self.emit_at(
                Instr::LoadTemplate {
                    line: node.line,
                    index: None,
                },
                start,
            );
            self.emit(Instr::TemplateMacroNamespace);
        }
        // the assignment target keeps the variable's own name, `_self` included
        let name = self.name_constant(&macro_key(&variable.name, variable.id));
        self.emit(Instr::SetMacroNamespace {
            name,
            global: assign.global,
        });
        Ok(())
    }

    /// Twig: `IncludeNode::compile()` with `addGetTemplate()` (`EmbedNode::addGetTemplate()` for an embed) and
    /// `addTemplateArguments()`: the template is loaded, then the variables are evaluated, then the template is
    /// rendered; with `ignore missing`, a `LoaderError` while loading (an embed also resolves its parent there)
    /// skips the template.
    fn include(
        &mut self,
        line: i64,
        target: IncludeTarget<'_>,
        variables: Option<&Expr>,
        only: bool,
        ignore_missing: bool,
    ) -> Result<()> {
        self.add_debug_info(line);
        let start = self.marker;
        let var = ignore_missing.then(|| self.local());
        let try_at = ignore_missing.then(|| self.emit(Instr::TryLoader { catch: 0 }));
        match target {
            IncludeTarget::Expr(expr) => {
                self.expr(expr)?;
                self.emit_at(Instr::LoadTemplate { line, index: None }, start);
            }
            IncludeTarget::Embedded { name, index } => {
                self.emit_const(Value::from(name));
                self.emit_at(
                    Instr::LoadTemplate {
                        line,
                        index: Some(index),
                    },
                    start,
                );
                if ignore_missing {
                    // `$_vN->getParent($context);`
                    self.emit(Instr::Dup);
                    self.emit(Instr::GetParent { this: false });
                    self.emit(Instr::Pop);
                }
            }
        }
        let mut skip = None;
        // PHP attributes a method call to the line of its name: `->unwrap()->yield(` follows the template
        // expression (and, with `ignore missing`, starts a statement of its own after the try/catch)
        let mut call = self.marker;
        if let (Some(var), Some(try_at)) = (var, try_at) {
            // `} catch (LoaderError $e) { $_vN = null; } if ($_vN) { yield from $_vN->unwrap()->yield(...); }`
            self.emit(Instr::EndTry);
            self.patch_here(try_at);
            self.emit(Instr::StoreLocal(var));
            self.emit(Instr::LoadLocal(var));
            skip = Some(self.emit(Instr::JumpIfFalse(0)));
            self.emit(Instr::LoadLocal(var));
            call = self.marker;
        }
        let args = match variables {
            None if only => YieldArgs::Empty,
            None => YieldArgs::Context,
            Some(variables) => {
                // `CoreExtension::merge($context, <variables>)` or `CoreExtension::toArray(<variables>)`
                let before = self.marker;
                self.expr(variables)?;
                self.emit_at(Instr::IncludeContext { merge: !only }, before);
                if only {
                    self.emit_at(Instr::CheckContext, call);
                }
                YieldArgs::Given
            }
        };
        self.emit(Instr::YieldTemplate(args));
        if let Some(skip) = skip {
            self.patch_here(skip);
        }
        Ok(())
    }

    /// Twig: `CallExpression::compileCallable()` and `compileArguments()`.
    fn call(
        &mut self,
        callable: &TwigCallable,
        node: Option<&Expr>,
        arguments: Option<&Arguments>,
        line: i64,
    ) -> Result<()> {
        // compileCallable(): callables other than plain PHP functions are reflected first
        let plain_function = matches!(
            callable.callable().map(|c| c.kind()),
            Some(CallableKind::Function(name)) if !name.contains("::")
        );
        if !plain_function && callable.callable().is_none() {
            return Err(Error::logic(format!(
                "Callback for {} \"{}\" is not callable in the current scope.",
                callable.callable_type().as_str(),
                String::from_utf8_lossy(callable.name())
            )));
        }
        let start = self.marker;
        let index = self.callable(callable);
        self.emit(Instr::ArgsStart {
            with_context: callable.needs_context(),
        });
        if let Some(node) = node {
            self.expr(node)?;
            self.emit(Instr::Arg);
        }
        if let Some(arguments) = arguments {
            let source = self.source();
            let extractor =
                CallableArgumentsExtractor::new(callable, line, Some(&source), node.is_some())?;
            let bound = extractor.extract_arguments(arguments, &mut *self.deprecations)?;
            for argument in &bound {
                match &argument.kind {
                    ExprKind::Array(array) if array.variadic => {
                        self.expr(argument)?;
                        self.emit(Instr::ArgSpread);
                    }
                    ExprKind::Unary(unary) if unary.op == UnaryOp::Spread => {
                        self.expr(&unary.node)?;
                        self.emit(Instr::ArgSpread);
                    }
                    _ => {
                        self.expr(argument)?;
                        self.emit(Instr::Arg);
                    }
                }
            }
        }
        self.emit_at(Instr::Call(index), start);
        Ok(())
    }
}

/// What an `IncludeNode` loads.
enum IncludeTarget<'a> {
    /// `{% include <expr> %}`.
    Expr(&'a Expr),
    /// `{% embed %}`: the embedded template `index` of the template `name` (the template containing the embed).
    Embedded { name: &'a str, index: i64 },
}

/// The `$macros` key of a macro variable: its name, or for a nameless one (`{% from %}`) a name derived from its
/// identity (Twig's compiler names it `_vN`; the `"\0"` prefix keeps it apart from every import alias).
fn macro_key(name: &TempName, id: u32) -> Vec<u8> {
    match name {
        TempName::String(name) => name.clone(),
        TempName::Int(i) => i.to_string().into_bytes(),
        TempName::Null => format!("\0_v{id}").into_bytes(),
    }
}

/// The parts of a program [`Codegen::module_body`] produces.
struct ModuleParts {
    body: UnitId,
    blocks: Vec<BlockDef>,
    macros: Vec<MacroDef>,
    parent: Option<ParentDef>,
    traits: Vec<TraitDef>,
    embedded: Vec<Arc<Program>>,
    traitable: bool,
}

/// The operand of the null test inside a `NullCoalesceBinary` test (`defined(clone) && !(null === left)`).
fn null_test_operand(test: &Expr) -> Option<&Expr> {
    let ExprKind::Binary(and) = &test.kind else {
        return None;
    };
    if and.op != ast::BinaryOp::And {
        return None;
    }
    let ExprKind::Unary(not) = &and.right.kind else {
        return None;
    };
    if not.op != UnaryOp::Not {
        return None;
    }
    match &not.node.kind {
        ExprKind::Test(null_test) if null_test.kind == TestKind::Null => Some(&null_test.node),
        _ => None,
    }
}

/// Twig: `ModuleNode::compileIsTraitable()`: no parent, no macros, and every node of the body has no children (the
/// check counts child nodes, so text and block references are fine).
fn is_traitable(module: &ast::ModuleNode) -> bool {
    if module.parent.is_some() || module.macros.as_nodes().is_some_and(|m| !m.is_empty()) {
        return false;
    }
    let body = match &module.body.kind {
        NodeKind::Body(nodes) => match nodes.get(&NodeKey::Index(0)) {
            Some(first) => first,
            None => return true,
        },
        _ => &module.body,
    };
    let children = body.children();
    if children.is_empty() {
        // `new Nodes([$nodes])`: the node itself has no children
        return true;
    }
    children
        .iter()
        .all(|(_, child)| child.children().is_empty())
}

/// The PHP value of a constant expression's value (`Compiler::repr()`).
pub(crate) fn const_value(value: &ConstantValue) -> Value {
    match value {
        ConstantValue::Null => Value::Null,
        ConstantValue::Bool(b) => Value::Bool(*b),
        ConstantValue::Int(i) => Value::Int(*i),
        ConstantValue::Float(f) => Value::Float(*f),
        ConstantValue::String(s) => Value::String(PhpString::from(s.as_slice())),
        ConstantValue::Array(items) => {
            let mut array = Array::new();
            for (key, value) in items {
                let key = match key {
                    ast::ArrayKey::Int(i) => ArrayKey::Int(*i),
                    ast::ArrayKey::String(s) => {
                        ArrayKey::from_string(PhpString::from(s.as_slice()))
                    }
                };
                array.insert(key, const_value(value));
            }
            Value::Array(array)
        }
    }
}

/// Evaluates the default value of a macro argument (a constant, an array of constants, or `-`/`+` of a constant), as
/// PHP evaluates a parameter's constant default.
fn const_eval(expr: &Expr) -> Value {
    let mut diag = Ignore;
    let diag: &mut dyn Diagnostics = &mut diag;
    match &expr.kind {
        ExprKind::Constant(constant) => const_value(&constant.value),
        ExprKind::Unary(unary) if unary.op == UnaryOp::Neg => {
            ops::negate(&const_eval(&unary.node), diag).unwrap_or(Value::Null)
        }
        ExprKind::Unary(unary) if unary.op == UnaryOp::Pos => {
            ops::unary_plus(&const_eval(&unary.node), diag).unwrap_or(Value::Null)
        }
        ExprKind::Array(array) => {
            let mut out = Array::new();
            let mut is_sequence = true;
            for (i, (key, value)) in array.elements.iter().enumerate() {
                let key_value = key.as_constant().map(const_value);
                if !matches!(&key_value, Some(Value::Int(k)) if *k == i as i64) {
                    is_sequence = false;
                }
                let value = const_eval(value);
                if is_sequence {
                    let _ = out.push(value);
                } else if let Some(key) = key_value
                    && let Ok(key) = ArrayKey::from_value(&key, twig_value::KeyContext::Write, diag)
                {
                    out.insert(key, value);
                }
            }
            Value::Array(out)
        }
        _ => Value::Null,
    }
}
