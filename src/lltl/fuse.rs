//! Instruction fusion: the pass that ends the compilation of every unit. It replaces frequent instruction sequences
//! with fused instructions that do the same work in fewer dispatches and stack moves:
//!
//! - a call (`ArgsStart` without context, its arguments each followed by `Arg`, `Call`) takes its arguments from the
//!   operand stack ([`Instr::CallStack`]); then its trailing constant arguments become a constant array of the
//!   instruction, and a `Print` of its result is fused into it;
//! - the elements of an array literal that are constants (`NewArray`, then `Const` and `ArrayAppend`, or two `Const`
//!   and `ArrayInsert`) are built once, into a constant array (what `zend_try_ct_eval_array()` does in PHP);
//! - `LoadName` (lenient or always defined) followed by a call of one stack argument ([`Instr::CallName`]);
//! - `LoadName` (lenient) or `Const` followed by `StoreName` ([`Instr::CopyName`], [`Instr::StoreConst`]);
//! - `Const`, `NewArray` and `GetAttr`: an attribute without arguments ([`Instr::GetAttrConst`]);
//! - `Const` followed by a binary operator ([`Instr::BinaryConst`]), then a binary operator followed by `StoreName`
//!   ([`Instr::BinaryStore`]) or a comparison followed by a conditional jump ([`Instr::JumpIfCompare`]), then
//!   `LoadName` (lenient) followed by a `BinaryStore` with a constant operand ([`Instr::NameBinaryStore`]);
//! - a `Jump` to the next instruction is removed.
//!
//! The rewriting keeps every effect where it was, so that a fused unit behaves exactly like the code it replaces:
//!
//! - the instructions a fusion removes can neither fail, report a diagnostic nor run other code (`ArgsStart` without
//!   context, `Arg`, `Const`, `NewArray`, a lenient `LoadName`); two instructions that can are fused only when they
//!   have the same line (a call and the `Print` of its result), so that an error raised in the middle of the fused
//!   instruction keeps its line;
//! - a fused instruction takes the line of the instruction it replaces that can fail or run other code (the `Call`,
//!   the `GetAttr`): errors keep their lines, and so do the frames of the template stack while the fused instruction
//!   runs other code (`Error::guess()` reads them); every other instruction keeps its line, and the line table is
//!   rebuilt from the lines of the remaining instructions, so markers stay where `Compiler::addDebugInfo()` put them;
//! - jumps are remapped; a jump to a removed instruction goes to the next remaining one, which is only done where that
//!   is the same (a removed `ArgsStart` or `Arg`, the first constant of a fused run); sequences with a jump target
//!   in their middle are not fused, nor calls whose argument code jumps out of the call or is jumped into from outside;
//! - the last instruction of a unit is never removed or replaced (its line is the last marker of the unit, which the
//!   sandbox reads).

use std::collections::HashMap;

use twig_value::{Array, ArrayKey, Value};

use super::instr::{ConstId, Instr, Label};
use super::program::LineTable;

/// The constants of the program being compiled, as the pass reads and extends them.
pub(super) trait Constants {
    /// The constant `id`.
    fn constant(&self, id: ConstId) -> &Value;
    /// The id of a constant array holding the constants `ids` in order (one per distinct list).
    fn array_constant(&mut self, ids: &[ConstId]) -> ConstId;
    /// Adds a constant (a new entry of the pool).
    fn add_constant(&mut self, value: Value) -> ConstId;
}

/// The constant arrays the pass added to a program, by their elements.
#[derive(Debug, Default)]
pub(super) struct ArrayConstants(HashMap<Vec<ConstId>, ConstId>);

impl ArrayConstants {
    /// The id of the constant array of `ids`, adding it to `constants` the first time.
    pub(super) fn get(&mut self, constants: &mut Vec<Value>, ids: &[ConstId]) -> ConstId {
        if let Some(&id) = self.0.get(ids) {
            return id;
        }
        let array = Array::from_values(ids.iter().map(|&id| constants[id as usize].clone()));
        let id = constants.len() as ConstId;
        constants.push(Value::Array(array));
        self.0.insert(ids.to_vec(), id);
        id
    }
}

/// Fuses the instructions of a unit (see the module documentation).
pub(super) fn fuse(code: &mut Vec<Instr>, lines: &mut LineTable, constants: &mut dyn Constants) {
    if code.len() < 2 {
        return;
    }
    let mut flow = Flow::new(code, lines);
    rewrite(code, lines, &mut flow, fuse_calls);
    // a nested literal is folded before the literal holding it
    while rewrite(code, lines, &mut flow, |code, flow| {
        fold_arrays(code, flow, constants)
    }) {}
    rewrite(code, lines, &mut flow, |code, flow| {
        fuse_call_constants(code, flow, constants)
    });
    rewrite(code, lines, &mut flow, fuse_pairs);
    rewrite(code, lines, &mut flow, fuse_operators);
    rewrite(code, lines, &mut flow, fuse_name_operands);
    thread_jumps(code);
}

/// A jump to a `Jump` goes to where that one goes (the end of an `if` at the end of a loop body jumps to the
/// loop's head directly).
fn thread_jumps(code: &mut [Instr]) {
    for ip in 0..code.len() {
        let Some(mut target) = label(&code[ip]) else {
            continue;
        };
        // a cycle of jumps (an infinite loop) is left as it is
        for _ in 0..8 {
            match code.get(target as usize) {
                Some(Instr::Jump(next)) if *next != target => target = *next,
                _ => break,
            }
        }
        if let Some(label) = label_mut(&mut code[ip]) {
            *label = target;
        }
    }
}

/// What becomes of an instruction of the unit being rewritten.
enum Edit {
    Keep,
    Delete,
    Replace(Instr),
}

/// The lines and jumps of a unit.
struct Flow {
    /// The lines of the instructions ([`LineTable::NO_MARKER`] for none).
    lines: Vec<i64>,
    /// Whether a jump lands on each instruction (and on the end of the unit).
    targets: Vec<bool>,
    /// For each instruction (and the end of the unit), the first and the last instruction that jump to it
    /// (`(u32::MAX, 0)` when none does), so that [`is_closed`](Self::is_closed) need not look at every jump.
    sources: Vec<(u32, u32)>,
}

impl Flow {
    fn new(code: &[Instr], table: &LineTable) -> Flow {
        let mut lines = Vec::with_capacity(code.len());
        let mut runs = table.runs().iter().peekable();
        let mut line = LineTable::NO_MARKER;
        for ip in 0..code.len() as u32 {
            while let Some(&&(start, run_line)) = runs.peek() {
                if start > ip {
                    break;
                }
                line = run_line;
                runs.next();
            }
            lines.push(line);
        }
        Flow::with_lines(code, lines)
    }

    /// The flow of `code`, whose instructions have the lines `lines`.
    fn with_lines(code: &[Instr], lines: Vec<i64>) -> Flow {
        let mut targets = vec![false; code.len() + 1];
        let mut sources = vec![(u32::MAX, 0); code.len() + 1];
        for (ip, instr) in code.iter().enumerate() {
            if let Some(target) = label(instr)
                && let Some(slot) = targets.get_mut(target as usize)
            {
                *slot = true;
                let (first, last) = &mut sources[target as usize];
                *first = (*first).min(ip as u32);
                *last = (*last).max(ip as u32);
            }
        }
        Flow {
            lines,
            targets,
            sources,
        }
    }

    /// Whether a jump lands on instruction `ip`.
    fn is_target(&self, ip: usize) -> bool {
        self.targets[ip]
    }

    /// Whether the code strictly between `start` and `end` jumps only within `start + 1 ..= end`, and nothing outside
    /// `start ..= end` jumps into `start + 1 ..= end`.
    fn is_closed(&self, code: &[Instr], start: usize, end: usize) -> bool {
        let inside = |ip: usize| ip > start && ip <= end;
        let jumps_out = code[start + 1..end]
            .iter()
            .filter_map(label)
            .any(|target| !inside(target as usize));
        // a source outside `start + 1 .. end` is at or before `start`, or at or after `end`
        let jumped_into = (start + 1..=end).any(|ip| {
            let (first, last) = self.sources[ip];
            self.targets[ip] && (first as usize <= start || last as usize >= end)
        });
        !jumps_out && !jumped_into
    }
}

/// The jump target of an instruction.
fn label(instr: &Instr) -> Option<Label> {
    match instr {
        Instr::Jump(target) | Instr::JumpIfFalse(target) | Instr::JumpIfTrue(target) => {
            Some(*target)
        }
        Instr::JumpIfCompare { target, .. } => Some(*target),
        Instr::IterNext { exit, .. } => Some(*exit),
        Instr::TryLoader { catch } => Some(*catch),
        Instr::RecvInit { skip, .. } => Some(*skip),
        _ => None,
    }
}

/// The jump target of an instruction, to remap it.
fn label_mut(instr: &mut Instr) -> Option<&mut Label> {
    match instr {
        Instr::Jump(target) | Instr::JumpIfFalse(target) | Instr::JumpIfTrue(target) => {
            Some(target)
        }
        Instr::JumpIfCompare { target, .. } => Some(target),
        Instr::IterNext { exit, .. } => Some(exit),
        Instr::TryLoader { catch } => Some(catch),
        Instr::RecvInit { skip, .. } => Some(skip),
        _ => None,
    }
}

/// Runs one fusion over the unit: `plan` returns an edit per instruction (or `None` when nothing changes), which is
/// applied with the jumps remapped and the line table rebuilt from the lines of the remaining instructions; `flow` is
/// the flow of the code before and after. Returns whether the code changed.
fn rewrite(
    code: &mut Vec<Instr>,
    table: &mut LineTable,
    flow: &mut Flow,
    plan: impl FnOnce(&[Instr], &Flow) -> Option<Vec<Edit>>,
) -> bool {
    let Some(edits) = plan(code, flow) else {
        return false;
    };
    debug_assert_eq!(edits.len(), code.len());
    debug_assert!(matches!(edits.last(), Some(Edit::Keep)));
    // `index[ip]`: the new position of instruction `ip`, or of the first remaining instruction after it
    let mut index = Vec::with_capacity(code.len() + 1);
    let mut next = 0u32;
    for edit in &edits {
        index.push(next);
        if !matches!(edit, Edit::Delete) {
            next += 1;
        }
    }
    index.push(next);
    let mut fused = Vec::with_capacity(next as usize);
    let mut table_lines = LineTable::default();
    let mut lines = Vec::with_capacity(next as usize);
    for (ip, (instr, edit)) in code.drain(..).zip(edits).enumerate() {
        let instr = match edit {
            Edit::Keep => instr,
            Edit::Delete => continue,
            Edit::Replace(instr) => instr,
        };
        table_lines.push(fused.len() as u32, flow.lines[ip]);
        lines.push(flow.lines[ip]);
        fused.push(instr);
    }
    for instr in &mut fused {
        if let Some(target) = label_mut(instr) {
            // a label past the end (which neither the code generator nor `lltl::Compiler` makes) keeps ending the
            // unit, as it did before the fusion
            *target = index.get(*target as usize).copied().unwrap_or(next);
        }
    }
    *flow = Flow::with_lines(&fused, lines);
    *code = fused;
    *table = table_lines;
    true
}

/// A call whose `ArgsStart` was seen: where it starts, the `Arg`s of its arguments, and whether it can be fused.
struct OpenCall {
    start: usize,
    args: Vec<usize>,
    fusable: bool,
}

/// `ArgsStart`, the arguments each followed by `Arg`, `Call(i)` → the arguments, `CallStack`.
///
/// `ArgsStart` without context only starts an empty argument list and `Arg` moves a value into it: neither can fail or
/// be observed. A call starting with a context snapshot or unpacking arguments (`ArgSpread`) stays as it is; so does a
/// call whose argument code jumps out of it or is jumped into from outside (not produced by the code generator).
fn fuse_calls(code: &[Instr], flow: &Flow) -> Option<Vec<Edit>> {
    let mut edits: Option<Vec<Edit>> = None;
    let mut open: Vec<OpenCall> = Vec::new();
    for (ip, instr) in code.iter().enumerate() {
        match instr {
            Instr::ArgsStart { with_context } => open.push(OpenCall {
                start: ip,
                args: Vec::new(),
                fusable: !with_context,
            }),
            Instr::Arg => {
                if let Some(call) = open.last_mut() {
                    call.args.push(ip);
                }
            }
            Instr::ArgSpread { .. } => {
                if let Some(call) = open.last_mut() {
                    call.fusable = false;
                }
            }
            Instr::Call(callable) => {
                let Some(call) = open.pop() else { continue };
                if !call.fusable || ip + 1 == code.len() || !flow.is_closed(code, call.start, ip) {
                    continue;
                }
                let edits = edits.get_or_insert_with(|| keep_all(code.len()));
                edits[call.start] = Edit::Delete;
                for &arg in &call.args {
                    edits[arg] = Edit::Delete;
                }
                edits[ip] = Edit::Replace(Instr::CallStack {
                    callable: *callable,
                    argc: call.args.len() as u32,
                    consts: None,
                    print: false,
                });
            }
            _ => {}
        }
    }
    edits
}

/// `NewArray`, then constant elements (`Const`, `ArrayAppend` or `Const`, `Const`, `ArrayInsert`) → `Const` of the
/// array they build. The array is built with the operations of the instructions (`Array::new()`, `push()`, the key
/// normalisation of `ArrayInsert`), so it is the array the code builds, and it is folded only when building it
/// cannot fail or report a diagnostic: values are `null`, booleans, ints, floats other than `NAN`, strings and
/// arrays of such values, keys are ints, strings or booleans, and no append meets an occupied next element. An array
/// literal evaluated twice is then the same array both times, which is only observable through the comparisons that
/// take a shortcut for the same array (`zend_is_identical()`, `zend_compare_symbol_tables()`), whose results differ
/// only for an array holding `NAN`. The elements after a constant prefix are added at runtime to a copy of it.
fn fold_arrays(code: &[Instr], flow: &Flow, constants: &mut dyn Constants) -> Option<Vec<Edit>> {
    let mut edits: Option<Vec<Edit>> = None;
    let mut ip = 0;
    while ip + 1 < code.len() {
        if !matches!(code[ip], Instr::NewArray) {
            ip += 1;
            continue;
        }
        let mut array = Array::new();
        let mut end = ip + 1;
        loop {
            // `Const v; ArrayAppend` or `Const k; Const v; ArrayInsert`, with no jump landing inside
            let element = match (code.get(end), code.get(end + 1), code.get(end + 2)) {
                (Some(Instr::Const(value)), Some(Instr::ArrayAppend), _)
                    if !flow.is_target(end) && !flow.is_target(end + 1) =>
                {
                    Some((None, *value, 2))
                }
                (Some(Instr::Const(key)), Some(Instr::Const(value)), Some(Instr::ArrayInsert))
                    if !flow.is_target(end)
                        && !flow.is_target(end + 1)
                        && !flow.is_target(end + 2) =>
                {
                    Some((Some(*key), *value, 3))
                }
                _ => None,
            };
            let Some((key, value, len)) = element else {
                break;
            };
            let value = constants.constant(value).clone();
            if !foldable_value(&value, 0) {
                break;
            }
            let added = match key.map(|key| constants.constant(key)) {
                None => array.push(value).is_ok(),
                Some(Value::Int(i)) => {
                    array.insert(*i, value);
                    true
                }
                Some(Value::Bool(b)) => {
                    array.insert(i64::from(*b), value);
                    true
                }
                Some(Value::String(name)) => {
                    array.insert(ArrayKey::from_string(name.clone()), value);
                    true
                }
                Some(_) => false,
            };
            if !added {
                break;
            }
            end += len;
        }
        // the last instruction of the unit stays as it is
        if end == ip + 1 || end >= code.len() {
            ip = end.max(ip + 1);
            continue;
        }
        let edits = edits.get_or_insert_with(|| keep_all(code.len()));
        edits[ip] = Edit::Replace(Instr::Const(constants.add_constant(Value::Array(array))));
        for edit in &mut edits[ip + 1..end] {
            *edit = Edit::Delete;
        }
        ip = end;
    }
    edits
}

/// Whether a constant can be an element of a folded array (see [`fold_arrays`]).
fn foldable_value(value: &Value, depth: usize) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::String(_) => true,
        Value::Float(f) => !f.is_nan(),
        Value::Array(array) => {
            depth < 64 && array.values().all(|value| foldable_value(value, depth + 1))
        }
        Value::Object(_) => false,
    }
}

/// `Const c1 ... Const ck`, `CallStack` → `CallStack` with the constants `[c1 ... ck]` as its last arguments; then a
/// `Print` of the result with the line of the call.
fn fuse_call_constants(
    code: &[Instr],
    flow: &Flow,
    constants: &mut dyn Constants,
) -> Option<Vec<Edit>> {
    let mut edits: Option<Vec<Edit>> = None;
    for (ip, instr) in code.iter().enumerate() {
        let Instr::CallStack {
            callable,
            argc,
            consts: None,
            print: false,
        } = *instr
        else {
            continue;
        };
        if ip + 1 == code.len() {
            continue;
        }
        // the constant run before the call: no jump lands after its first instruction
        let mut first = ip;
        while first > 0
            && ip - first < argc as usize
            && matches!(code[first - 1], Instr::Const(_))
            && !flow.is_target(first)
        {
            first -= 1;
        }
        let print = ip + 2 < code.len()
            && matches!(code[ip + 1], Instr::Print)
            && !flow.is_target(ip + 1)
            && flow.lines[ip + 1] == flow.lines[ip];
        if first == ip && !print {
            continue;
        }
        let edits = edits.get_or_insert_with(|| keep_all(code.len()));
        let ids: Vec<ConstId> = code[first..ip]
            .iter()
            .map(|instr| match instr {
                Instr::Const(id) => *id,
                _ => unreachable!("the run holds constants"),
            })
            .collect();
        for edit in &mut edits[first..ip] {
            *edit = Edit::Delete;
        }
        if print {
            edits[ip + 1] = Edit::Delete;
        }
        edits[ip] = Edit::Replace(Instr::CallStack {
            callable,
            argc: argc - ids.len() as u32,
            consts: (!ids.is_empty()).then(|| constants.array_constant(&ids)),
            print,
        });
    }
    edits
}

/// The pairs and triples of simple instructions: `LoadName` (lenient) or `Const`, then `StoreName`; `Const`,
/// `NewArray`, `GetAttr`; `LoadName` (lenient or always defined), then a `CallStack` of one stack argument, at the same
/// line (a missing name read as always defined warns: the warning keeps its line, which it has when the policy makes
/// it an exception).
fn fuse_pairs(code: &[Instr], flow: &Flow) -> Option<Vec<Edit>> {
    let mut edits: Option<Vec<Edit>> = None;
    let mut ip = 0;
    // the last instruction stays as it is
    while ip + 2 < code.len() {
        let fused = match (&code[ip], &code[ip + 1]) {
            (
                Instr::LoadName {
                    name,
                    mode: super::instr::NameMode::Lenient,
                },
                Instr::StoreName(to),
            ) if !flow.is_target(ip + 1) => Some((
                2,
                Instr::CopyName {
                    from: *name,
                    to: *to,
                },
            )),
            (
                Instr::LoadName { name, mode },
                Instr::CallStack {
                    callable,
                    argc: 1,
                    consts,
                    print,
                },
            ) if matches!(
                mode,
                super::instr::NameMode::Lenient | super::instr::NameMode::AlwaysDefined
            ) && !flow.is_target(ip + 1)
                && flow.lines[ip] == flow.lines[ip + 1] =>
            {
                Some((
                    2,
                    Instr::CallName {
                        name: *name,
                        always_defined: *mode == super::instr::NameMode::AlwaysDefined,
                        callable: *callable,
                        consts: *consts,
                        print: *print,
                    },
                ))
            }
            (Instr::Const(value), Instr::StoreName(name)) if !flow.is_target(ip + 1) => Some((
                2,
                Instr::StoreConst {
                    value: *value,
                    name: *name,
                },
            )),
            (Instr::Const(item), Instr::NewArray)
                if ip + 3 < code.len() && !flow.is_target(ip + 1) && !flow.is_target(ip + 2) =>
            {
                match code[ip + 2] {
                    Instr::GetAttr(op) => Some((3, Instr::GetAttrConst { op, item: *item })),
                    _ => None,
                }
            }
            _ => None,
        };
        // a jump to the next instruction (the end of an `if` without `else`) does nothing
        if let Instr::Jump(target) = code[ip]
            && target as usize == ip + 1
        {
            edits.get_or_insert_with(|| keep_all(code.len()))[ip] = Edit::Delete;
            ip += 1;
            continue;
        }
        match fused {
            Some((len, instr)) => {
                let edits = edits.get_or_insert_with(|| keep_all(code.len()));
                // the fused instruction stands where the last instruction of the sequence was, with its line
                for edit in &mut edits[ip..ip + len - 1] {
                    *edit = Edit::Delete;
                }
                edits[ip + len - 1] = Edit::Replace(instr);
                ip += len;
            }
            None => ip += 1,
        }
    }
    edits
}

/// Binary operators: `Const` then `Binary` or `CommutedBinary` → `BinaryConst`; then `Binary` or `BinaryConst`
/// followed by `StoreName` → `BinaryStore`, and a comparison followed by `JumpIfFalse` or `JumpIfTrue` →
/// `JumpIfCompare`. The fused instruction stands where the operator was, with its line: the constant cannot fail, nor
/// can storing into the context or the truthiness of the boolean a comparison gives.
fn fuse_operators(code: &[Instr], flow: &Flow) -> Option<Vec<Edit>> {
    use super::instr::BinaryOp;
    let comparison = |op: BinaryOp| {
        matches!(
            op,
            BinaryOp::Equal
                | BinaryOp::NotEqual
                | BinaryOp::Less
                | BinaryOp::Greater
                | BinaryOp::LessEqual
                | BinaryOp::GreaterEqual
                | BinaryOp::Identical
                | BinaryOp::NotIdentical
        )
    };
    let mut edits: Option<Vec<Edit>> = None;
    let mut ip = 0;
    // the last instruction stays as it is
    while ip + 2 < code.len() {
        // the operator at `at`, with its constant right operand at `at - 1`
        let (first, op, commuted, value, at) = match (&code[ip], &code[ip + 1]) {
            (Instr::Const(value), Instr::Binary(op)) if !flow.is_target(ip + 1) => {
                (ip, *op, false, Some(*value), ip + 1)
            }
            (Instr::Const(value), Instr::CommutedBinary(op)) if !flow.is_target(ip + 1) => {
                (ip, *op, true, Some(*value), ip + 1)
            }
            (Instr::Binary(op), _) => (ip, *op, false, None, ip),
            (Instr::CommutedBinary(op), _) => (ip, *op, true, None, ip),
            _ => {
                ip += 1;
                continue;
            }
        };
        let next = code
            .get(at + 1)
            .filter(|_| at + 2 < code.len() && !flow.is_target(at + 1));
        let fused = match next {
            Some(Instr::StoreName(name)) => Some((
                at + 1,
                Instr::BinaryStore {
                    op,
                    commuted,
                    value,
                    name: *name,
                },
            )),
            Some(Instr::JumpIfFalse(target) | Instr::JumpIfTrue(target))
                if comparison(op) && !commuted =>
            {
                Some((
                    at + 1,
                    Instr::JumpIfCompare {
                        op,
                        value,
                        jump_if: matches!(next, Some(Instr::JumpIfTrue(_))),
                        target: *target,
                    },
                ))
            }
            _ => value.map(|value| {
                (
                    at,
                    Instr::BinaryConst {
                        op,
                        commuted,
                        value,
                    },
                )
            }),
        };
        let Some((last, instr)) = fused else {
            ip += 1;
            continue;
        };
        let edits = edits.get_or_insert_with(|| keep_all(code.len()));
        // the fused instruction stands where the operator was (its line); the instructions around it go
        for (i, edit) in edits.iter_mut().enumerate().take(last + 1).skip(first) {
            *edit = if i == at {
                Edit::Replace(instr.clone())
            } else {
                Edit::Delete
            };
        }
        ip = last + 1;
    }
    edits
}

/// `LoadName` (lenient) then a `BinaryStore` with a constant operand → `NameBinaryStore`, where the `BinaryStore`
/// was (its line): a lenient read can neither fail nor report anything.
fn fuse_name_operands(code: &[Instr], flow: &Flow) -> Option<Vec<Edit>> {
    let mut edits: Option<Vec<Edit>> = None;
    let mut ip = 0;
    // the last instruction stays as it is
    while ip + 2 < code.len() {
        if let (
            Instr::LoadName {
                name: left,
                mode: super::instr::NameMode::Lenient,
            },
            Instr::BinaryStore {
                op,
                commuted,
                value: Some(value),
                name,
            },
        ) = (&code[ip], &code[ip + 1])
            && !flow.is_target(ip + 1)
        {
            let edits = edits.get_or_insert_with(|| keep_all(code.len()));
            edits[ip] = Edit::Delete;
            edits[ip + 1] = Edit::Replace(Instr::NameBinaryStore {
                op: *op,
                commuted: *commuted,
                left: *left,
                value: *value,
                name: *name,
            });
            ip += 2;
        } else {
            ip += 1;
        }
    }
    edits
}

fn keep_all(len: usize) -> Vec<Edit> {
    (0..len).map(|_| Edit::Keep).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lltl::instr::{BinaryOp, NameMode};

    /// Constants of a test unit.
    struct Pool(Vec<Value>, ArrayConstants);

    impl Constants for Pool {
        fn constant(&self, id: ConstId) -> &Value {
            &self.0[id as usize]
        }

        fn array_constant(&mut self, ids: &[ConstId]) -> ConstId {
            self.1.get(&mut self.0, ids)
        }

        fn add_constant(&mut self, value: Value) -> ConstId {
            self.0.push(value);
            (self.0.len() - 1) as ConstId
        }
    }

    fn pool() -> Pool {
        Pool(
            vec![
                Value::from("name"),
                Value::from("html"),
                Value::Null,
                Value::Bool(true),
                Value::Int(1),
            ],
            ArrayConstants::default(),
        )
    }

    fn table(lines: &[(u32, i64)]) -> LineTable {
        let mut table = LineTable::default();
        for &(ip, line) in lines {
            table.push(ip, line);
        }
        table
    }

    /// An auto-escaped print of a variable becomes one instruction with the call's line; the instructions after it
    /// keep theirs.
    #[test]
    fn escaped_prints_are_fused_with_the_line_of_the_call() {
        let mut code = vec![
            Instr::ArgsStart {
                with_context: false,
            },
            Instr::LoadName {
                name: 0,
                mode: NameMode::Lenient,
            },
            Instr::Arg,
            Instr::Const(1),
            Instr::Arg,
            Instr::Const(2),
            Instr::Arg,
            Instr::Const(3),
            Instr::Arg,
            Instr::Call(0),
            Instr::Print,
            Instr::Text(0),
            Instr::Return,
        ];
        let mut lines = table(&[(0, 4), (11, 5)]);
        let mut pool = pool();
        fuse(&mut code, &mut lines, &mut pool);
        assert!(matches!(
            code.as_slice(),
            [
                Instr::CallName {
                    name: 0,
                    always_defined: false,
                    callable: 0,
                    consts: Some(_),
                    print: true
                },
                Instr::Text(0),
                Instr::Return,
            ]
        ));
        assert_eq!(lines.runs(), &[(0, 4), (1, 5)]);
    }

    /// A call whose `Print` has another line keeps its `Print`; a jump into the middle of a sequence stops its fusion,
    /// and jumps are remapped.
    #[test]
    fn lines_and_jump_targets_limit_fusion() {
        let mut code = vec![
            Instr::ArgsStart {
                with_context: false,
            },
            Instr::LoadName {
                name: 0,
                mode: NameMode::AlwaysDefined,
            },
            Instr::Arg,
            Instr::Call(0),
            Instr::Print,
            Instr::Const(4),
            Instr::JumpIfTrue(8),
            Instr::Const(4),
            Instr::StoreName(0),
            Instr::Return,
        ];
        let mut lines = table(&[(0, 1), (4, 2)]);
        let mut pool = pool();
        fuse(&mut code, &mut lines, &mut pool);
        assert!(matches!(
            code.as_slice(),
            [
                Instr::CallName {
                    name: 0,
                    always_defined: true,
                    consts: None,
                    print: false,
                    ..
                },
                Instr::Print,
                Instr::Const(4),
                Instr::JumpIfTrue(5),
                Instr::Const(4),
                Instr::StoreName(0),
                Instr::Return,
            ]
        ));
        assert_eq!(lines.runs(), &[(0, 1), (1, 2)]);
    }

    /// `{% set x = a + 1 %}` becomes one instruction; a name read that can warn stays apart.
    #[test]
    fn operators_on_a_variable_and_a_constant_are_fused() {
        let mut code = vec![
            Instr::LoadName {
                name: 0,
                mode: NameMode::Lenient,
            },
            Instr::Const(4),
            Instr::Binary(BinaryOp::Add),
            Instr::StoreName(0),
            Instr::LoadName {
                name: 0,
                mode: NameMode::AlwaysDefined,
            },
            Instr::Const(4),
            Instr::Binary(BinaryOp::Sub),
            Instr::StoreName(0),
            Instr::Return,
        ];
        let mut lines = table(&[(0, 1), (4, 2)]);
        let mut pool = pool();
        fuse(&mut code, &mut lines, &mut pool);
        assert!(matches!(
            code.as_slice(),
            [
                Instr::NameBinaryStore {
                    op: BinaryOp::Add,
                    commuted: false,
                    left: 0,
                    value: 4,
                    name: 0
                },
                Instr::LoadName {
                    name: 0,
                    mode: NameMode::AlwaysDefined
                },
                Instr::BinaryStore {
                    op: BinaryOp::Sub,
                    commuted: false,
                    value: Some(4),
                    name: 0
                },
                Instr::Return,
            ]
        ));
        assert_eq!(lines.runs(), &[(0, 1), (1, 2)]);
    }

    /// The constant elements of an array literal are built once.
    #[test]
    fn constant_array_literals_become_constants() {
        let mut code = vec![
            Instr::NewArray,
            Instr::Const(4),
            Instr::ArrayAppend,
            Instr::Const(0),
            Instr::Const(1),
            Instr::ArrayInsert,
            Instr::Pop,
            Instr::Return,
        ];
        let mut lines = table(&[(0, 1)]);
        let mut pool = pool();
        fuse(&mut code, &mut lines, &mut pool);
        let [Instr::Const(id), Instr::Pop, Instr::Return] = code.as_slice() else {
            panic!("{code:?}");
        };
        let mut expected = Array::new();
        let _ = expected.push(1);
        expected.insert("name", "html");
        assert!(matches!(pool.constant(*id), Value::Array(array) if array.identical(&expected)));
    }
}
