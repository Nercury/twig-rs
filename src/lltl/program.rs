//! Compiled templates: programs, units and line tables.

use std::fmt::{self, Write as _};
use std::sync::{Arc, OnceLock};

use twig_value::{PhpString, Value};

use super::instr::{ConstId, Instr, UnitId};
use crate::extension::TwigCallable;
use crate::source::Source;

/// The identity of a compiled template file. Twig compiles a template and its embedded templates (`{% embed %}`)
/// into one PHP file; `Error::guess()` compares files, so the programs of embedded templates share the [`FileId`]
/// of their root template.
pub type FileId = u64;

/// What a unit implements.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitKind {
    /// `doDisplay()`: the template body.
    Body,
    /// `block_NAME()`.
    Block(Vec<u8>),
    /// The closure of a macro (`loadDeclaredMacros()`).
    Macro(Vec<u8>),
    /// `doGetParent()` of a dynamic parent (`{% extends expr %}`).
    Parent,
    /// The closure of an arrow function; the parameters are the assigned context names, in order.
    Arrow(Vec<PhpString>),
}

/// The run-length table mapping instructions to template lines: the line of an instruction is the line of the last
/// `// line N` marker Twig's compiler emitted before the PHP code the instruction stands for (Twig's
/// `getDebugInfo()`), or none before the first marker. Runs start where `Compiler::addDebugInfo()` emits a marker; a
/// call instruction whose PHP opcode carries the line of the call's start (PHP reports the line where a call
/// begins) gets a run of its own when markers were emitted inside its arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineTable {
    runs: Vec<(u32, i64)>,
}

impl LineTable {
    /// The run value that stands for "no marker yet".
    pub(crate) const NO_MARKER: i64 = i64::MIN;

    /// The line of instruction `ip`: the line of the marker in effect, `None` when no marker precedes it (Twig's
    /// `guessTemplateInfo()` then looks at the next frame).
    pub fn line_at(&self, ip: u32) -> Option<i64> {
        match self.runs.partition_point(|&(start, _)| start <= ip) {
            0 => None,
            i => match self.runs[i - 1].1 {
                Self::NO_MARKER => None,
                line => Some(line),
            },
        }
    }

    /// Starts a run at instruction `ip` with `line` ([`Self::NO_MARKER`] for none); a run starting at the same
    /// instruction is replaced.
    pub(crate) fn push(&mut self, ip: u32, line: i64) {
        if let Some(last) = self.runs.last_mut()
            && last.0 == ip
        {
            last.1 = line;
            let n = self.runs.len();
            let redundant = if n >= 2 {
                self.runs[n - 2].1 == line
            } else {
                line == Self::NO_MARKER
            };
            if redundant {
                self.runs.pop();
            }
            return;
        }
        let current = self.runs.last().map_or(Self::NO_MARKER, |run| run.1);
        if current != line {
            self.runs.push((ip, line));
        }
    }

    /// The runs as `(first instruction, line)` pairs ([`i64::MIN`] for "no marker").
    pub fn runs(&self) -> &[(u32, i64)] {
        &self.runs
    }
}

/// A unit of code: the body, a block, a macro, a dynamic parent expression or an arrow function.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub(crate) kind: UnitKind,
    pub(crate) code: Vec<Instr>,
    pub(crate) lines: LineTable,
    pub(crate) locals: u16,
}

impl Unit {
    /// What the unit implements.
    pub fn kind(&self) -> &UnitKind {
        &self.kind
    }

    /// The instructions.
    pub fn code(&self) -> &[Instr] {
        &self.code
    }

    /// The line table.
    pub fn lines(&self) -> &LineTable {
        &self.lines
    }

    /// The number of local slots the unit uses.
    pub fn locals(&self) -> u16 {
        self.locals
    }
}

/// A block of the template (`$this->blocks`): its name, unit and line.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockDef {
    /// The block name.
    pub name: Vec<u8>,
    /// The unit implementing `block_NAME()`.
    pub unit: UnitId,
    /// The line of the `{% block %}` tag.
    pub line: i64,
}

/// A parameter of a macro: the Twig name, the default value and whether the default was written explicitly
/// (`$signature` of `TwigMacro`).
///
/// A default PHP folds while compiling (`zend_const_expr_to_zval()`) is its value. One it cannot fold without an
/// error stays a constant expression (`IS_CONSTANT_AST`), evaluated by `ZEND_RECV_INIT` when the argument is
/// missing: `default` is then a value standing for it, which the macro's code replaces (see [`Instr::RecvInit`]).
///
/// [`Instr::RecvInit`]: super::Instr::RecvInit
#[derive(Debug, Clone, PartialEq)]
pub struct MacroParam {
    /// The argument name (without the reserved-name prefix).
    pub name: PhpString,
    /// The default value (`null` for an argument declared without a default).
    pub default: Value,
    /// The default was written in the template.
    pub explicit_default: bool,
}

/// A macro of the template (`loadDeclaredMacros()`).
#[derive(Debug, Clone, PartialEq)]
pub struct MacroDef {
    /// The macro name.
    pub name: Vec<u8>,
    /// The unit of the macro body (a capture of the body returning `''` or a `Markup`).
    pub unit: UnitId,
    /// The declared parameters in order.
    pub params: Vec<MacroParam>,
    /// The name of an explicit variadic parameter (`...rest`); `None` means the implicit `varargs` bucket.
    pub variadic: Option<Vec<u8>>,
    /// The line of the `{% macro %}` tag.
    pub line: i64,
}

/// The parent of a template (`{% extends %}`, or the parent of an embedded template).
#[derive(Debug, Clone, PartialEq)]
pub enum ParentDef {
    /// A constant parent name: `$this->parent ??= $this->load("base.twig", line)`.
    Constant {
        /// The constant value of the expression.
        name: Value,
        /// The line of the expression.
        line: i64,
    },
    /// A dynamic parent: the unit evaluates the expression (`doGetParent()`).
    Dynamic {
        /// The unit.
        unit: UnitId,
        /// The line of the expression.
        line: i64,
    },
}

/// A trait of the template (`{% use %}`).
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDef {
    /// The template name (a constant string).
    pub template: Value,
    /// The line of the template expression.
    pub line: i64,
    /// The renamed blocks: original name and alias.
    pub targets: Vec<(Vec<u8>, Value)>,
}

/// What a template uses that the sandbox checks when it starts (`CheckSecurityNode`, added by the sandbox visitor):
/// the tags, filters, functions and tests with the line of their first use, in first-use order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecurityLists {
    /// The tags (`$usedTags`).
    pub tags: Vec<(Vec<u8>, i64)>,
    /// The filters (`$usedFilters`).
    pub filters: Vec<(Vec<u8>, i64)>,
    /// The functions (`$usedFunctions`).
    pub functions: Vec<(Vec<u8>, i64)>,
    /// The tests (`$usedTests`).
    pub tests: Vec<(Vec<u8>, i64)>,
}

/// A compiled template: the LLTL code of one Twig template class.
///
/// Twig compiles a template into a PHP class (`doDisplay()`, `block_*()`, `loadDeclaredMacros()`, `doGetParent()`)
/// plus one class per embedded template; a [`Program`] holds the units for these methods, the constant pool, the
/// callables the code calls, and the compile-time facts the runtime needs (blocks, macros, parent, traits, whether the
/// template can be used as a trait). Programs are immutable and shared between renders and threads.
#[derive(Debug, Clone)]
pub struct Program {
    pub(crate) file: FileId,
    pub(crate) source: Source,
    pub(crate) index: Option<i64>,
    pub(crate) constants: Vec<Value>,
    pub(crate) callables: Vec<TwigCallable>,
    pub(crate) units: Vec<Unit>,
    pub(crate) body: UnitId,
    pub(crate) blocks: Vec<BlockDef>,
    pub(crate) macros: Vec<MacroDef>,
    pub(crate) parent: Option<ParentDef>,
    pub(crate) traits: Vec<TraitDef>,
    pub(crate) embedded: Vec<Arc<Program>>,
    pub(crate) traitable: bool,
    pub(crate) strategy: Option<String>,
    pub(crate) escaper: bool,
    pub(crate) security: Option<SecurityLists>,
    pub(crate) debug: bool,
    /// The runtime cache of `ZEND_RECV_INIT`: the macro defaults evaluated at runtime that are kept for later
    /// calls (PHP keeps them per compiled closure; twig-rs per compiled program).
    pub(crate) recv_cache: Vec<OnceLock<Value>>,
}

impl Program {
    /// The template name (`getTemplateName()`, also `_self`).
    pub fn name(&self) -> &str {
        self.source.name()
    }

    /// The template source.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The compiled file the program belongs to.
    pub fn file(&self) -> FileId {
        self.file
    }

    /// The embedded-template index (`___N` classes), `None` for a root template.
    pub fn index(&self) -> Option<i64> {
        self.index
    }

    /// The constant pool.
    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    /// The constant `i`.
    pub fn constant(&self, i: ConstId) -> &Value {
        &self.constants[i as usize]
    }

    /// The filters, functions and tests the code calls.
    pub fn callables(&self) -> &[TwigCallable] {
        &self.callables
    }

    /// The units.
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// The unit `i`.
    pub fn unit(&self, i: UnitId) -> &Unit {
        &self.units[i as usize]
    }

    /// The body unit (`doDisplay()`).
    pub fn body(&self) -> UnitId {
        self.body
    }

    /// The blocks in definition order.
    pub fn blocks(&self) -> &[BlockDef] {
        &self.blocks
    }

    /// The macros in definition order.
    pub fn macros(&self) -> &[MacroDef] {
        &self.macros
    }

    /// The parent, if the template extends another one.
    pub fn parent(&self) -> Option<&ParentDef> {
        self.parent.as_ref()
    }

    /// The traits in `use` order.
    pub fn traits(&self) -> &[TraitDef] {
        &self.traits
    }

    /// The embedded templates; index `i` is the template with embed index `i + 1`.
    pub fn embedded(&self) -> &[Arc<Program>] {
        &self.embedded
    }

    /// The embedded template with the given embed index (`___N`), looked up in the whole tree of embedded templates
    /// of this program: Twig numbers the embedded templates of a file from 1 in the order their parse finishes, so a
    /// nested `{% embed %}` gets its index before the embed that contains it, and every class of the file is
    /// reachable from every other one.
    pub fn embedded_template(&self, index: i64) -> Option<&Arc<Program>> {
        self.embedded.iter().find_map(|program| {
            if program.index == Some(index) {
                Some(program)
            } else {
                program.embedded_template(index)
            }
        })
    }

    /// Twig: `isTraitable()`.
    pub fn is_traitable(&self) -> bool {
        self.traitable
    }

    /// The default escaping strategy the escaper visitor recorded (`getDefaultEscapeStrategy()`), `None` for `false`.
    pub fn default_escape_strategy(&self) -> Option<&str> {
        self.strategy.as_deref()
    }

    /// The tags, filters, functions and tests the sandbox checks when the template starts (`checkSecurity()`), when
    /// the sandbox visitor ran (the `SandboxExtension` is registered).
    pub fn security(&self) -> Option<&SecurityLists> {
        self.security.as_ref()
    }

    /// Whether the template was compiled with `debug` on: the compiled `getSourceContext()` then returns the code of
    /// the template (Twig: `ModuleNode::compileGetSourceContext()`), else an empty string.
    pub fn is_debug(&self) -> bool {
        self.debug
    }

    /// Whether escape filters of the template use the template's escaper (the module's `escaper` attribute, set by the
    /// escaper visitor): Twig's constructor then fetches the `EscaperRuntime` (`$this->escaper =
    /// $env->getRuntime(EscaperRuntime::class)`) when the template is loaded.
    pub fn uses_template_escaper(&self) -> bool {
        self.escaper
    }

    /// A human-readable listing of the program (units, instructions, lines), for debugging and tests.
    pub fn disassemble(&self) -> String {
        let mut out = String::new();
        let _ = self.write_disassembly(&mut out);
        out
    }

    fn write_disassembly(&self, out: &mut String) -> fmt::Result {
        match self.index {
            Some(index) => writeln!(out, "program {:?} (embedded {index})", self.name())?,
            None => writeln!(out, "program {:?}", self.name())?,
        }
        for (i, unit) in self.units.iter().enumerate() {
            writeln!(out, "unit {i}: {:?} (locals: {})", unit.kind, unit.locals)?;
            for (ip, instr) in unit.code.iter().enumerate() {
                match unit.lines.line_at(ip as u32) {
                    Some(line) => write!(out, "  {ip:4} [{line:3}] ")?,
                    None => write!(out, "  {ip:4} [  -] ")?,
                }
                self.write_instr(out, instr)?;
                out.push('\n');
            }
        }
        for program in &self.embedded {
            program.write_disassembly(out)?;
        }
        Ok(())
    }

    fn write_instr(&self, out: &mut String, instr: &Instr) -> fmt::Result {
        let constant = |i: ConstId| -> String {
            match self.constants.get(i as usize) {
                Some(Value::String(s)) => format!("{:?}", s.to_string_lossy()),
                Some(v) => format!("{v:?}"),
                None => format!("#{i}?"),
            }
        };
        match instr {
            Instr::Const(i) => write!(out, "Const {}", constant(*i)),
            Instr::Text(i) => write!(out, "Text {}", constant(*i)),
            Instr::LoadName { name, mode } => write!(out, "LoadName {} {mode:?}", constant(*name)),
            Instr::StoreName(i) => write!(out, "StoreName {}", constant(*i)),
            Instr::Call(i) => match self.callables.get(*i as usize) {
                Some(callable) => write!(out, "Call {callable}"),
                None => write!(out, "Call #{i}?"),
            },
            Instr::IterNext { exit, key, value } => write!(
                out,
                "IterNext exit={exit} key={} value={}",
                constant(*key),
                constant(*value)
            ),
            Instr::ForRestore {
                key,
                value,
                has_else,
                with_loop,
            } => write!(
                out,
                "ForRestore key={} value={} else={has_else} loop={with_loop}",
                constant(*key),
                constant(*value)
            ),
            Instr::SeqDestructure(i) => write!(out, "SeqDestructure {}", constant(*i)),
            Instr::ParentBlock { name, render } => {
                write!(out, "ParentBlock {} render={render}", constant(*name))
            }
            Instr::MacroNamespace { name, line } => {
                write!(out, "MacroNamespace {} line={line}", constant(*name))
            }
            Instr::SetMacroNamespace { name, global } => {
                write!(out, "SetMacroNamespace {} global={global}", constant(*name))
            }
            Instr::ListAssign(i) => write!(out, "ListAssign {}", constant(*i)),
            Instr::RecvInit {
                name, cache, skip, ..
            } => write!(
                out,
                "RecvInit {} cache={cache} skip={skip}",
                constant(*name)
            ),
            Instr::RecvStore { name, cache } => {
                write!(out, "RecvStore {} cache={cache}", constant(*name))
            }
            other => write!(out, "{other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_table_runs() {
        let mut table = LineTable::default();
        assert_eq!(table.line_at(0), None);
        table.push(0, LineTable::NO_MARKER);
        assert!(table.runs().is_empty());
        table.push(2, 1);
        table.push(3, 1);
        table.push(5, 4);
        table.push(5, 6);
        assert_eq!(table.runs(), &[(2, 1), (5, 6)]);
        assert_eq!(table.line_at(0), None);
        assert_eq!(table.line_at(2), Some(1));
        assert_eq!(table.line_at(4), Some(1));
        assert_eq!(table.line_at(5), Some(6));
        assert_eq!(table.line_at(100), Some(6));
        table.push(7, 1);
        table.push(8, 6);
        assert_eq!(table.line_at(7), Some(1));
        assert_eq!(table.line_at(8), Some(6));
        table.push(9, LineTable::NO_MARKER);
        assert_eq!(table.line_at(9), None);
        table.push(10, 0);
        assert_eq!(table.line_at(10), Some(0));
    }
}
