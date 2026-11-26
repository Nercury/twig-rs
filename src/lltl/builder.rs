//! The code builder of node classes defined by extensions: what Twig's `Compiler` is to `Node::compile()`
//! (TWIG/src/Compiler.php).
//!
//! A Twig node class writes PHP source; a twig-rs node class ([`CustomNode`], [`CustomExpr`]) lowers itself into the
//! instructions of the low-level template language through a [`Compiler`], which emits them into the unit being
//! compiled with the line markers of the enclosing code. The builder checks the code as it is written, so that no
//! node class can leave the virtual machine in a state the built-in nodes never produce: values are taken only from
//! what the node's own code put on the operand stack, a statement leaves the stack as it found it and an expression
//! leaves exactly one value, every jump lands where the stack has the height it had at the jump, and no jump enters
//! or leaves a capture or the argument list of a runtime call. A violation is a `LogicException`-kind error of the
//! compilation (`The "%s" node class ...`), which the environment wraps like any exception thrown by a node's
//! `compile()` (`An exception has been thrown during the compilation of a template ("...")`).
//!
//! [`CustomNode`]: crate::ast::CustomNode
//! [`CustomExpr`]: crate::ast::CustomExpr

use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use twig_value::Value;

use super::codegen::Codegen;
use super::instr::{BinaryOp, Instr, Label as Position, LocalId, NameMode};
use super::runtime::NodeRuntime;
use crate::ast::{Arguments, ChildKey, Expr, Node, NodeRef};
use crate::error::{Error, Result};
use crate::extension::TwigCallable;

/// The identity of the next builder: the handles of a builder are refused by every other one, also in other
/// compilations.
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// What the code of a node must leave on the operand stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Produces {
    /// A statement node: nothing.
    Nothing,
    /// An expression: its value.
    Value,
}

/// The code builder handed to [`CustomNode::compile`](crate::ast::CustomNode::compile) and
/// [`CustomExpr::compile`](crate::ast::CustomExpr::compile). Twig: `Twig\Compiler`.
///
/// The code works on an operand stack, like the code of the built-in nodes: [`subcompile_expr`](Self::subcompile_expr)
/// and [`constant`](Self::constant) push a value, [`print`](Self::print) pops one, [`binary`](Self::binary) pops two
/// and pushes the result. A statement node must leave the stack as it found it, an expression exactly one value more.
/// Control flow uses [`Jump`]s (forward, bound to their target with [`bind`](Self::bind)) and [`Mark`]s (backward,
/// for loops).
///
/// | Twig's `Compiler` | twig-rs |
/// |---|---|
/// | `addDebugInfo($node)` | [`add_debug_info`](Self::add_debug_info) |
/// | `subcompile($node)` | [`subcompile`](Self::subcompile) (statements), [`subcompile_expr`](Self::subcompile_expr) (expressions) |
/// | `repr($value)` | [`constant`](Self::constant) |
/// | `getVarName()` | [`local`](Self::local) |
/// | `yield "text";`, `yield $value;` | [`text`](Self::text), [`print`](Self::print) |
/// | `$context["name"]` | [`load_name`](Self::load_name), [`store_name`](Self::store_name) |
/// | `if (...) { ... }`, loops | [`jump_if_false`](Self::jump_if_false), [`jump`](Self::jump), [`bind`](Self::bind), [`mark`](Self::mark), [`jump_to`](Self::jump_to) |
/// | `$this->env->getRuntime(...)->method(...)` | [`call_runtime`](Self::call_runtime) |
/// | `FunctionExpression::compileCallable()` | [`call`](Self::call) |
///
/// The lifetime `'t` is the lifetime of the syntax tree: only nodes of the tree (the children of the compiled node)
/// can be compiled, as in Twig, where `compile()` subcompiles the node's own children.
///
/// # Errors
///
/// The methods that take values from the stack, bind jumps or use handles return a `LogicException`-kind error
/// ([`ErrorKind::Logic`](crate::ErrorKind::Logic)) when the code breaks one of the rules above.
pub struct Compiler<'t, 'c> {
    codegen: &'c mut Codegen,
    class: String,
    produces: Produces,
    /// Identifies the handles this builder gave out.
    owner: u64,
    /// The values the node's code has on the stack.
    depth: usize,
    /// The values below this height belong to code outside the current region: the enclosing code, or the code
    /// before a capture or the arguments of a runtime call.
    floor: usize,
    /// Whether the next instruction can be reached by falling through (not after an unconditional jump).
    reachable: bool,
    /// The open regions (the node's code, captures, argument lists), innermost last: their identity and the number
    /// of their jumps not bound yet.
    regions: Vec<(u32, usize)>,
    next_region: u32,
    tree: PhantomData<fn(&'t ()) -> &'t ()>,
}

/// A forward jump made by [`Compiler::jump`], [`Compiler::jump_if_false`] or [`Compiler::jump_if_true`], waiting for
/// its target: [`Compiler::bind`] makes the next instruction the target.
#[must_use = "a jump must be bound to its target with `Compiler::bind`"]
pub struct Jump {
    owner: u64,
    at: Position,
    depth: usize,
    region: u32,
}

impl fmt::Debug for Jump {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Jump")
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

/// A position the code can jump back to (the start of a loop), made by [`Compiler::mark`].
#[derive(Clone, Copy)]
pub struct Mark {
    owner: u64,
    at: Position,
    depth: usize,
    region: u32,
}

impl fmt::Debug for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mark")
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

/// A local variable of the compiled method (Twig's `$_vN`), made by [`Compiler::local`]. A local nothing stored into
/// reads as `null`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Local {
    owner: u64,
    slot: LocalId,
}

impl fmt::Debug for Local {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Local").field(&self.slot).finish()
    }
}

impl<'t, 'c> Compiler<'t, 'c> {
    /// A builder for the node class `class` writing into the unit being compiled.
    pub(super) fn new(codegen: &'c mut Codegen, class: &str, produces: Produces) -> Self {
        let owner = NEXT_OWNER.fetch_add(1, Ordering::Relaxed);
        // the node's code follows the code written so far: PHP's `CG(zend_lineno)` is the current marker
        codegen.leaf();
        Compiler {
            codegen,
            class: class.to_owned(),
            produces,
            owner,
            depth: 0,
            floor: 0,
            reachable: true,
            regions: vec![(0, 0)],
            next_region: 1,
            tree: PhantomData,
        }
    }

    /// Checks the end of the node's code.
    pub(super) fn finish(self) -> Result<()> {
        let unbound = self.regions.last().map_or(0, |region| region.1);
        if unbound > 0 {
            return Err(self.error(format_args!("does not bind {}", count(unbound, "jump"))));
        }
        let expected = match self.produces {
            Produces::Nothing => 0,
            Produces::Value => 1,
        };
        if self.reachable && self.depth != expected {
            return Err(self.error(format_args!(
                "leaves {} on the stack where {} must leave {}",
                count(self.depth, "value"),
                match self.produces {
                    Produces::Nothing => "a statement node",
                    Produces::Value => "an expression",
                },
                count(expected, "value"),
            )));
        }
        // the code after the node continues on the line of the node's last token
        self.codegen.leaf();
        Ok(())
    }

    // ---- errors and checks ---------------------------------------------------------------------------------------

    fn error(&self, message: fmt::Arguments<'_>) -> Error {
        Error::logic(format!("The \"{}\" node class {message}.", self.class))
    }

    /// Takes `n` values the node's code pushed in the current region.
    fn take(&mut self, n: usize) -> Result<()> {
        if self.depth < self.floor + n {
            return Err(self.error(format_args!(
                "takes a value from the stack that its code did not put there"
            )));
        }
        self.depth -= n;
        Ok(())
    }

    fn put(&mut self, n: usize) {
        self.depth += n;
    }

    fn check_owner(&self, owner: u64) -> Result<()> {
        if owner != self.owner {
            return Err(self.error(format_args!(
                "uses a jump, a mark or a local of another compilation"
            )));
        }
        Ok(())
    }

    fn region(&self) -> u32 {
        self.regions.last().map_or(0, |region| region.0)
    }

    fn check_region(&self, region: u32) -> Result<()> {
        if region != self.region() {
            return Err(self.error(format_args!(
                "jumps into or out of a capture or the arguments of a runtime call"
            )));
        }
        Ok(())
    }

    /// Checks that the stack has the height `depth` where code jumps to.
    fn check_landing(&self, depth: usize) -> Result<()> {
        if self.reachable && self.depth != depth {
            return Err(self.error(format_args!(
                "jumps with {} on the stack to a place where there {} {}",
                count(depth, "value"),
                if self.depth == 1 { "is" } else { "are" },
                self.depth,
            )));
        }
        Ok(())
    }

    /// Runs `code` in a region of its own (a capture body, the arguments of a call): it cannot take the values below
    /// it, and no jump enters or leaves it.
    fn region_scope(&mut self, code: impl FnOnce(&mut Self) -> Result<()>) -> Result<usize> {
        let base = self.depth;
        let floor = std::mem::replace(&mut self.floor, base);
        let id = self.next_region;
        self.next_region = self.next_region.wrapping_add(1);
        self.regions.push((id, 0));
        let result = code(self);
        let (_, unbound) = self.regions.pop().unwrap_or_default();
        self.floor = floor;
        result?;
        if unbound > 0 {
            return Err(self.error(format_args!("does not bind {}", count(unbound, "jump"))));
        }
        Ok(base)
    }

    fn unbound(&mut self) -> &mut usize {
        let last = self.regions.len() - 1;
        &mut self.regions[last].1
    }

    // ---- the environment -----------------------------------------------------------------------------------------

    /// The node class being compiled (its [`class_name`](crate::ast::CustomNode::class_name)).
    pub fn class_name(&self) -> &str {
        &self.class
    }

    /// Whether the environment has `strict_variables` (Twig: `$compiler->getEnvironment()->isStrictVariables()`).
    pub fn is_strict_variables(&self) -> bool {
        self.codegen.options.strict_variables
    }

    /// Whether the environment has `debug` (Twig: `$compiler->getEnvironment()->isDebug()`).
    pub fn is_debug(&self) -> bool {
        self.codegen.options.debug
    }

    /// Whether the `SandboxExtension` is registered (Twig: `$compiler->getEnvironment()->hasExtension(
    /// SandboxExtension::class)`), which the built-in nodes check when they compile attribute accesses.
    pub fn has_sandbox_extension(&self) -> bool {
        self.codegen.options.sandboxed
    }

    // ---- lines and children --------------------------------------------------------------------------------------

    /// Twig: `addDebugInfo($node)` for a node of line `line`: the code written next is attributed to `line`, and so
    /// are the errors raised there without a line of their own. Like Twig's compiler, it starts a new marker only
    /// when the line differs from the last marker's.
    pub fn add_debug_info(&mut self, line: i64) {
        self.codegen.add_debug_info(line);
    }

    /// Twig: `subcompile($node)` of a statement node (a body, a [`Node::print`], another tag's node): its code runs
    /// here. The stack is left as it is.
    ///
    /// # Errors
    ///
    /// The errors of the node's compilation.
    pub fn subcompile(&mut self, node: &'t Node) -> Result<()> {
        self.codegen.statement(node)
    }

    /// Twig: `subcompile($node)` of an expression: `[] -> [value]`.
    ///
    /// # Errors
    ///
    /// The errors of the expression's compilation.
    pub fn subcompile_expr(&mut self, expr: &'t Expr) -> Result<()> {
        self.codegen.expr(expr)?;
        self.put(1);
        Ok(())
    }

    /// Twig's `Node::compile()`, the code of a node class that does not compile itself: the children in order.
    pub(crate) fn subcompile_children(
        &mut self,
        children: Vec<(ChildKey<'t>, NodeRef<'t>)>,
    ) -> Result<()> {
        match self.produces {
            // statement children run in order; an expression child would be written where PHP expects a statement
            Produces::Nothing => {
                for (_, child) in &children {
                    match child {
                        NodeRef::Node(node) => self.subcompile(node)?,
                        NodeRef::Expr(_) | NodeRef::Arguments(_) => {
                            return Err(self.not_compilable());
                        }
                    }
                }
                Ok(())
            }
            // the PHP of an expression's only child is the expression's PHP
            Produces::Value => match children.as_slice() {
                [(_, NodeRef::Expr(expr))] => self.subcompile_expr(expr),
                _ => Err(self.not_compilable()),
            },
        }
    }

    fn not_compilable(&self) -> Error {
        Error::logic(format!(
            "The \"{}\" node class cannot be compiled by twig-rs.",
            self.class
        ))
    }

    // ---- values --------------------------------------------------------------------------------------------------

    /// Twig: `repr($value)`: `[] -> [value]`.
    pub fn constant(&mut self, value: impl Into<Value>) {
        self.codegen.emit_const(value.into());
        self.put(1);
    }

    /// Discards the top of the stack: `[value] -> []`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn pop(&mut self) -> Result<()> {
        self.take(1)?;
        self.codegen.emit(Instr::Pop);
        Ok(())
    }

    /// Duplicates the top of the stack: `[value] -> [value value]`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn dup(&mut self) -> Result<()> {
        self.take(1)?;
        self.codegen.emit(Instr::Dup);
        self.put(2);
        Ok(())
    }

    /// PHP's `!$value`: `[value] -> [bool]`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn not(&mut self) -> Result<()> {
        self.unary(Instr::Not)
    }

    /// PHP's `(bool) $value`: `[value] -> [bool]`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn to_bool(&mut self) -> Result<()> {
        self.unary(Instr::ToBool)
    }

    /// PHP's `(string) $value`, with PHP's conversion warnings and errors: `[value] -> [string]`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn string_cast(&mut self) -> Result<()> {
        self.unary(Instr::StringCast)
    }

    /// `new Markup($value, $this->env->getCharset())`, a string the escaper leaves alone: `[value] -> [markup]`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn markup(&mut self) -> Result<()> {
        self.unary(Instr::MakeMarkup)
    }

    fn unary(&mut self, instr: Instr) -> Result<()> {
        self.take(1)?;
        self.codegen.emit(instr);
        self.put(1);
        Ok(())
    }

    /// A PHP binary operator with PHP 8.5's semantics (`.`, `+`, `==`, `<=>`, ...): `[left right] -> [result]`.
    ///
    /// # Errors
    ///
    /// Fewer than two values of the node's code are on the stack.
    pub fn binary(&mut self, op: BinaryOp) -> Result<()> {
        self.take(2)?;
        self.codegen.emit(Instr::Binary(op));
        self.put(1);
        Ok(())
    }

    // ---- locals --------------------------------------------------------------------------------------------------

    /// Twig: `getVarName()`: a new local variable of the compiled method (the method's `$_vN`).
    pub fn local(&mut self) -> Local {
        let slot = self.codegen.local();
        self.codegen.twig_var();
        Local {
            owner: self.owner,
            slot,
        }
    }

    /// Stores the top of the stack into a local: `[value] -> []`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack, or the local was made by another compilation.
    pub fn store_local(&mut self, local: Local) -> Result<()> {
        self.check_owner(local.owner)?;
        self.take(1)?;
        self.codegen.emit(Instr::StoreLocal(local.slot));
        Ok(())
    }

    /// Pushes the value of a local: `[] -> [value]`.
    ///
    /// # Errors
    ///
    /// The local was made by another compilation.
    pub fn load_local(&mut self, local: Local) -> Result<()> {
        self.check_owner(local.owner)?;
        self.codegen.emit(Instr::LoadLocal(local.slot));
        self.put(1);
        Ok(())
    }

    // ---- the context ---------------------------------------------------------------------------------------------

    /// Reads `$context["name"]` the way `mode` says: `[] -> [value]` (`[] -> [bool]` for [`NameMode::Defined`]).
    /// [`NameMode::Strict`] fails like a variable of a template with `strict_variables` (`Variable "name" does not
    /// exist.` at its line); [`NameMode::Lenient`] reads a missing variable as `null`.
    ///
    /// The value of a variable as a template reads it (`_self`, `_context`, `_charset`, the environment's
    /// `strict_variables`, the optimizer's loop variables, the sandbox's checks) comes from a `ContextVariable` child
    /// ([`Expr::context_variable`]) compiled with [`subcompile_expr`](Self::subcompile_expr).
    pub fn load_name(&mut self, name: impl AsRef<[u8]>, mode: NameMode) {
        let name = self.codegen.name_constant(name.as_ref());
        self.codegen.emit(Instr::LoadName { name, mode });
        self.put(1);
    }

    /// `$context["name"] = $value`: `[value] -> []`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn store_name(&mut self, name: impl AsRef<[u8]>) -> Result<()> {
        self.take(1)?;
        let name = self.codegen.name_constant(name.as_ref());
        self.codegen.emit(Instr::StoreName(name));
        Ok(())
    }

    /// The whole context, `$context` (what `_context` gives templates): `[] -> [array]`.
    pub fn load_context(&mut self) {
        self.codegen.emit(Instr::LoadContext);
        self.put(1);
    }

    /// `$context = $value` (a value that is not an array is converted like PHP's `(array)`): `[value] -> []`.
    /// [`load_context`](Self::load_context) into a [`local`](Self::local) first saves a context to restore.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn set_context(&mut self) -> Result<()> {
        self.take(1)?;
        self.codegen.emit(Instr::SetContext);
        Ok(())
    }

    // ---- output --------------------------------------------------------------------------------------------------

    /// `yield "text";`: writes the bytes.
    pub fn text(&mut self, text: impl AsRef<[u8]>) {
        let text = self
            .codegen
            .constant(Value::String(twig_value::PhpString::from(text.as_ref())));
        self.codegen.emit(Instr::Text(text));
    }

    /// `yield $value;`: writes PHP's string conversion of the top of the stack, `[value] -> []`. The value is written
    /// as it is: the escaper escapes the expressions of `PrintNode`s ([`Node::print`]), so an expression to escape is
    /// printed through such a child.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn print(&mut self) -> Result<()> {
        self.take(1)?;
        self.codegen.emit(Instr::Print);
        Ok(())
    }

    /// `flush()` (`{% flush %}`): flushes the sink when the template is displayed.
    pub fn flush(&mut self) {
        self.codegen.emit(Instr::Flush);
    }

    /// Twig: `CaptureNode::compile()`: the output of `body` (which shares the context) becomes a value instead of being
    /// written: a string with `raw`, else `''` or a `Markup` (`('' === $tmp = ...) ? '' : new Markup($tmp, $charset)`).
    /// `[] -> [captured]`.
    ///
    /// # Errors
    ///
    /// The errors of `body`; `body` takes values from the stack it did not put there, leaves values on it, or jumps
    /// out of the capture.
    pub fn capture(&mut self, raw: bool, body: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        self.codegen.emit(Instr::CaptureStart);
        let base = self.region_scope(body)?;
        if self.reachable && self.depth != base {
            return Err(self.error(format_args!(
                "leaves {} on the stack at the end of a capture",
                count(self.depth - base, "value")
            )));
        }
        self.depth = base;
        self.codegen.emit(Instr::CaptureEnd { markup: !raw });
        self.put(1);
        Ok(())
    }

    // ---- calls ---------------------------------------------------------------------------------------------------

    /// Twig: `CallExpression::compileCallable()`: a call of the filter, function or test `callable` (found by the token
    /// parser, e.g. with [`Parser::get_function`](crate::parser::Parser::get_function), or kept from the node's
    /// [`callable`](crate::ast::CustomExpr::callable)) with the input `node` of a filter or test and the template
    /// `arguments`, which are bound to the callable's parameters while compiling, as for a call written in the
    /// template: `[] -> [result]`. The call is attributed to the line where it starts.
    ///
    /// # Errors
    ///
    /// Twig's errors of the binding (`Unknown argument "%s" for function "%s".`, ... at `line`), and `Callback for
    /// %s "%s" is not callable in the current scope.` for a callable without PHP callable.
    pub fn call(
        &mut self,
        callable: &TwigCallable,
        node: Option<&'t Expr>,
        arguments: Option<&'t Arguments>,
        line: i64,
    ) -> Result<()> {
        self.codegen.call(callable, node, arguments, line)?;
        self.put(1);
        Ok(())
    }

    /// Calls a runtime closure with the values `arguments` puts on the stack (in order): `[] -> [result]`. The call is
    /// attributed to the line where it starts (the marker in effect before the arguments), like a call of a runtime
    /// method in a compiled template: an error the closure returns without a line gets that line.
    ///
    /// # Errors
    ///
    /// The errors of `arguments`; `arguments` takes values from the stack it did not put there, or jumps out of the
    /// argument list.
    pub fn call_runtime(
        &mut self,
        runtime: &NodeRuntime,
        arguments: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        let start = self.codegen.marker();
        let base = self.region_scope(arguments)?;
        let argc = u32::try_from(self.depth - base)
            .map_err(|_| self.error(format_args!("passes too many arguments")))?;
        self.depth = base;
        let runtime = self.codegen.runtime(runtime);
        self.codegen
            .emit_at(Instr::CallRuntime { runtime, argc }, start);
        self.put(1);
        Ok(())
    }

    // ---- control flow --------------------------------------------------------------------------------------------

    /// An unconditional forward jump; the code after it is reached only through a [`bind`](Self::bind) or a
    /// [`mark`](Self::mark).
    pub fn jump(&mut self) -> Jump {
        let at = self.codegen.emit(Instr::Jump(0));
        self.forward(at)
    }

    /// Pops a value and jumps forward when it is falsy (PHP's truthiness): `if ($value) { ... }`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn jump_if_false(&mut self) -> Result<Jump> {
        self.take(1)?;
        let at = self.codegen.emit(Instr::JumpIfFalse(0));
        let jump = self.forward(at);
        self.reachable = true;
        Ok(jump)
    }

    /// Pops a value and jumps forward when it is truthy: `if (!$value) { ... }`.
    ///
    /// # Errors
    ///
    /// No value of the node's code is on the stack.
    pub fn jump_if_true(&mut self) -> Result<Jump> {
        self.take(1)?;
        let at = self.codegen.emit(Instr::JumpIfTrue(0));
        let jump = self.forward(at);
        self.reachable = true;
        Ok(jump)
    }

    fn forward(&mut self, at: Position) -> Jump {
        *self.unbound() += 1;
        let jump = Jump {
            owner: self.owner,
            at,
            depth: self.depth,
            region: self.region(),
        };
        self.reachable = false;
        jump
    }

    /// Makes the next instruction the target of `jump`. The stack must have the height it had at the jump.
    ///
    /// # Errors
    ///
    /// The heights differ, the jump was made in another capture or argument list, or by another compilation.
    pub fn bind(&mut self, jump: Jump) -> Result<()> {
        self.check_owner(jump.owner)?;
        self.check_region(jump.region)?;
        self.check_landing(jump.depth)?;
        self.depth = jump.depth;
        self.reachable = true;
        *self.unbound() -= 1;
        self.codegen.patch_here(jump.at);
        Ok(())
    }

    /// A position to jump back to with [`jump_to`](Self::jump_to) (the start of a loop). Code can loop forever, as
    /// PHP code can.
    pub fn mark(&mut self) -> Mark {
        self.reachable = true;
        Mark {
            owner: self.owner,
            at: self.codegen.here(),
            depth: self.depth,
            region: self.region(),
        }
    }

    /// An unconditional jump back to `mark`; the stack must have the height it had at the mark.
    ///
    /// # Errors
    ///
    /// The heights differ, the mark is in another capture or argument list, or of another compilation.
    pub fn jump_to(&mut self, mark: Mark) -> Result<()> {
        self.check_mark(mark)?;
        self.codegen.emit(Instr::Jump(mark.at));
        self.reachable = false;
        Ok(())
    }

    /// Pops a value and jumps back to `mark` when it is falsy.
    ///
    /// # Errors
    ///
    /// As [`jump_to`](Self::jump_to), and no value of the node's code is on the stack.
    pub fn jump_if_false_to(&mut self, mark: Mark) -> Result<()> {
        self.take(1)?;
        self.check_mark(mark)?;
        self.codegen.emit(Instr::JumpIfFalse(mark.at));
        Ok(())
    }

    /// Pops a value and jumps back to `mark` when it is truthy.
    ///
    /// # Errors
    ///
    /// As [`jump_to`](Self::jump_to), and no value of the node's code is on the stack.
    pub fn jump_if_true_to(&mut self, mark: Mark) -> Result<()> {
        self.take(1)?;
        self.check_mark(mark)?;
        self.codegen.emit(Instr::JumpIfTrue(mark.at));
        Ok(())
    }

    fn check_mark(&self, mark: Mark) -> Result<()> {
        self.check_owner(mark.owner)?;
        self.check_region(mark.region)?;
        self.check_landing(mark.depth)
    }
}

impl fmt::Debug for Compiler<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compiler")
            .field("class", &self.class)
            .field("depth", &self.depth)
            .finish_non_exhaustive()
    }
}

/// `1 value`, `2 values`.
fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}
