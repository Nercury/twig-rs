//! What PHP's compiler makes of the code Twig compiles a template to, as far as it changes what the template does.
//!
//! Twig turns a template into PHP source (each node's `compile()`), and php-src compiles that source into opcodes
//! (PHPSRC/Zend/zend_compile.c). Three of its compile-time decisions are observable from templates:
//!
//! - **Constant folding.** `zend_compile_expr()` replaces an operation between constants by its result when the
//!   operation cannot fail (`zend_try_ct_eval_binary_op()` with `zend_binary_op_produces_error()`,
//!   `zend_try_ct_eval_unary_pm()`, `zend_compile_short_circuiting()`), and an array literal of constants by a
//!   constant array (`zend_try_ct_eval_array()`). Inside an array literal, `zend_try_ct_eval_array()` first runs
//!   `zend_eval_const_expr()` over every key and value, which also folds conditionals, `??` and casts with constant
//!   operands ([`PhpModel::eval_const`]), and stamps every value it creates with the line of the array
//!   (`zend_ast_create_zval()`), which an opcode emitted after such a value then reports. A value the parser made
//!   (a literal, a concatenation of literals) keeps its own line, also as the branch a folded conditional chooses
//!   ([`PhpModel::keeps_parsed_line`]).
//! - **Operand kinds.** Each compiled operand is a constant, a temporary, a variable (function call results) or a
//!   compiled variable (`znode` op types `IS_CONST < IS_TMP_VAR < IS_VAR < IS_CV`). The commutative opcodes swap
//!   their operands when op1's kind is lower than op2's (`zend_vm_set_opcode_handler_ex()`, PHPSRC/Zend/zend_vm_execute.h),
//!   which decides which operand is converted (and reported) first ([`PhpModel::operand_kind`]).
//! - **Lines.** The line of an AST node is the line of its first token (`zend_ast_create_*()` take the line of their
//!   first child), which is where a debug marker of Twig's compiler may sit ([`first_line`]).
//! - **Errors.** PHP refuses some of the code Twig generates: a `ParseError` for a spread outside an argument list or
//!   array literal ([`generated_parse_error`]), an `E_COMPILE_ERROR` for positional arguments after a spread or for a
//!   constant literal that unpacks a constant that is not an array ([`PhpModel::array_eval_fatal`]). It parses the
//!   whole file first and compiles it in file order, skipping the code constant folding removed.
//!
//! The model works on the visited Twig expressions and mirrors the PHP each node class writes (`ArrayExpression`,
//! `AbstractBinary`, `AbstractUnary`, the test classes, `NameExpression`, ...); it never sees PHP source.

use twig_value::{Array, ArrayKey, Ignore, PhpString, Value, compare, convert, numeric, ops};

use crate::ast::{
    self, BinaryOp, ConstantValue, Expr, ExprKind, FilterKind, FunctionKind, TempName, TestKind,
    UnaryOp,
};

use super::codegen::const_value;

/// The kind of a compiled operand: PHP's `znode` op types (PHPSRC/Zend/zend_compile.h), in their order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OperandKind {
    /// `IS_CONST` (1): a literal or a folded constant.
    Const = 1,
    /// `IS_TMP_VAR` (2): the result of an operator, a cast, a conditional, a `$context[...]` read, a frameless call.
    Tmp = 2,
    /// `IS_VAR` (4): the result of a function or method call.
    Var = 4,
    /// `IS_CV` (8): a compiled variable (`$context`, `$_vN`).
    Cv = 8,
}

/// A compile-time fatal error of the generated PHP (`E_COMPILE_ERROR`): PHP stops before the template class exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompileFatal(pub(crate) String);

/// Which of PHP's two constant evaluators runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `zend_compile_expr()`: the operand is `IS_CONST`.
    Compile,
    /// `zend_eval_const_expr()`: the AST is replaced by a `ZEND_AST_ZVAL` (inside array literals).
    Eval,
}

/// The environment facts the PHP of a template depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PhpModel {
    /// `strict_variables`: decides whether `x['k']` compiles to the inline fast path or to `getAttribute()`.
    pub(crate) strict_variables: bool,
}

/// The PHP opcode of a binary operator that `zend_binary_op_produces_error()` knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhpOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    BwOr,
    BwAnd,
    BwXor,
    /// Comparisons and `xor`: never an error.
    Other,
}

impl PhpModel {
    /// `zend_compile_expr()` of the PHP of `expr`: the value when PHP compiles it to a constant operand (`IS_CONST`).
    pub(crate) fn compile_const(&self, expr: &Expr) -> Option<Value> {
        self.fold(expr, Mode::Compile)
    }

    /// `zend_eval_const_expr()` of the AST of `expr`: the value when PHP replaces it by a constant while folding the
    /// elements of an enclosing array literal (`zend_try_ct_eval_array()`).
    pub(crate) fn eval_const(&self, expr: &Expr) -> Option<Value> {
        self.fold(expr, Mode::Eval)
    }

    /// The value of the constant operand (`IS_CONST`) the PHP of `expr` compiles to. `folding`: `expr` is part of
    /// the elements of an array literal, outside the calls, assignments and closures they contain, so
    /// `zend_try_ct_eval_array()` ran `zend_eval_const_expr()` over its AST before `zend_compile_expr()` compiles
    /// it ([`eval_const`](Self::eval_const)); otherwise only `zend_compile_expr()` folds
    /// ([`compile_const`](Self::compile_const)).
    pub(crate) fn const_operand(&self, expr: &Expr, folding: bool) -> Option<Value> {
        self.fold(expr, if folding { Mode::Eval } else { Mode::Compile })
    }

    /// The branch `zend_eval_const_expr()` replaces the conditional `expr` by (`ZEND_AST_CONDITIONAL` with a test
    /// that folds, PHPSRC/Zend/zend_compile.c): `ConditionalTernary`, `ElvisBinary` and `NullCoalesceBinary` all
    /// compile to `((test) ? (left) : (right))`. `None` for anything else, or when the test does not fold.
    pub(crate) fn chosen_branch<'e>(&self, expr: &'e Expr) -> Option<&'e Expr> {
        let (test, left, right) = match &expr.kind {
            ExprKind::ConditionalTernary(ternary) => (&ternary.test, &ternary.left, &ternary.right),
            ExprKind::Binary(binary)
                if matches!(binary.op, BinaryOp::Elvis | BinaryOp::NullCoalesce) =>
            {
                (binary.test.as_ref()?, &binary.left, &binary.right)
            }
            _ => return None,
        };
        let test = self.eval_const(test)?;
        Some(if truthy(&test) { left } else { right })
    }

    /// Whether the value `zend_eval_const_expr()` leaves in place of `expr` (which folds inside an array literal) is
    /// one PHP's parser made ([`is_parsed_value`]), which keeps the line it is written on, rather than one
    /// `zend_eval_const_expr()` creates (`zend_ast_create_zval()`), which gets the line of the array literal. A
    /// conditional whose test folds is replaced by the branch it chooses itself (`ZEND_AST_CONDITIONAL`,
    /// PHPSRC/Zend/zend_compile.c), so it keeps the line of a chosen parsed value; `RawFilter` and `DefaultFilter`
    /// compile as their node.
    pub(crate) fn keeps_parsed_line(&self, expr: &Expr) -> bool {
        if is_parsed_value(expr) {
            return true;
        }
        match &expr.kind {
            ExprKind::Filter(filter)
                if matches!(filter.kind, FilterKind::Default | FilterKind::Raw) =>
            {
                self.keeps_parsed_line(&filter.node)
            }
            _ => self
                .chosen_branch(expr)
                .is_some_and(|chosen| self.keeps_parsed_line(chosen)),
        }
    }

    /// The `E_COMPILE_ERROR` PHP raises while folding the array literal `array` (a constant array literal that
    /// unpacks a constant that is not an array: `zend_try_ct_eval_array()`).
    pub(crate) fn array_fatal(&self, array: &ast::ArrayExpr) -> Option<CompileFatal> {
        self.fold_array(array).err()
    }

    fn fold(&self, expr: &Expr, mode: Mode) -> Option<Value> {
        match &expr.kind {
            ExprKind::Constant(constant) => Some(if constant.defined_test {
                Value::Bool(true)
            } else {
                const_value(&constant.value)
            }),
            // NameExpression::compile() in defined-test mode writes `true` for special and always-defined names
            ExprKind::ContextVariable(name) if name.defined_test => {
                let special = matches!(name.name.as_slice(), b"_self" | b"_context" | b"_charset");
                (special || expr.always_defined).then_some(Value::Bool(true))
            }
            ExprKind::Array(array) => {
                if array.defined_test {
                    return Some(Value::Bool(true));
                }
                self.fold_array(array).ok().flatten()
            }
            ExprKind::Unary(unary) => match unary.op {
                // zend_try_ct_eval_unary_pm(): `op * ±1` unless it produces an error
                UnaryOp::Neg | UnaryOp::Pos => {
                    let value = self.fold(&unary.node, mode)?;
                    let factor = Value::Int(if unary.op == UnaryOp::Neg { -1 } else { 1 });
                    fold_binary(PhpOp::Mul, &value, &factor)
                }
                // zend_try_ct_eval_unary_op(ZEND_BOOL_NOT): a float may be NAN, which warns
                UnaryOp::Not => match self.fold(&unary.node, mode)? {
                    Value::Float(_) => None,
                    value => Some(Value::Bool(!truthy(&value))),
                },
                // zend_compile_cast() never folds
                UnaryOp::StringCast => match mode {
                    Mode::Compile => None,
                    Mode::Eval => string_cast(&self.fold(&unary.node, mode)?),
                },
                UnaryOp::Spread => None,
            },
            ExprKind::Binary(binary) => self.fold_binary_expr(binary, mode),
            // ConditionalTernary::compile(): `((test) ? (left) : (right))`
            ExprKind::ConditionalTernary(ternary) => match mode {
                Mode::Compile => None,
                Mode::Eval => {
                    let test = self.fold(&ternary.test, mode)?;
                    let chosen = if truthy(&test) {
                        &ternary.left
                    } else {
                        &ternary.right
                    };
                    self.fold(chosen, mode)
                }
            },
            ExprKind::Filter(filter) => match filter.kind {
                // DefaultFilter::compile() and RawFilter::compile() write their node only
                FilterKind::Default | FilterKind::Raw => self.fold(&filter.node, mode),
                _ => None,
            },
            ExprKind::Test(test) => self.fold_test(test, mode),
            _ => None,
        }
    }

    fn fold_binary_expr(&self, binary: &ast::BinaryExpr, mode: Mode) -> Option<Value> {
        use BinaryOp as B;
        let php = match binary.op {
            B::Add => PhpOp::Add,
            B::Sub => PhpOp::Sub,
            B::Mul => PhpOp::Mul,
            B::Div => PhpOp::Div,
            B::Mod => PhpOp::Mod,
            B::Power => PhpOp::Pow,
            B::Concat => PhpOp::Concat,
            B::BitwiseAnd => PhpOp::BwAnd,
            B::BitwiseOr => PhpOp::BwOr,
            B::BitwiseXor => PhpOp::BwXor,
            B::Equal
            | B::NotEqual
            | B::Less
            | B::LessEqual
            | B::Spaceship
            | B::SameAs
            | B::NotSameAs
            | B::Xor => PhpOp::Other,
            // zend_ct_eval_greater(): `is_smaller(right, left)`, without an error check
            B::Greater | B::GreaterEqual => {
                let left = self.fold(&binary.left, mode)?;
                let right = self.fold(&binary.right, mode)?;
                let result = if binary.op == B::Greater {
                    compare::less(&right, &left, &mut Ignore)
                } else {
                    compare::less_or_equal(&right, &left, &mut Ignore)
                };
                return result.ok().map(Value::Bool);
            }
            // zend_compile_short_circuiting() / ZEND_AST_AND, ZEND_AST_OR
            B::And | B::Or => {
                let left = self.fold(&binary.left, mode)?;
                let left = truthy(&left);
                if left == (binary.op == B::Or) {
                    return Some(Value::Bool(left));
                }
                let right = self.fold(&binary.right, mode)?;
                return Some(Value::Bool(truthy(&right)));
            }
            // ElvisBinary/NullCoalesceBinary::compile(): `((test) ? (left) : (right))`
            B::Elvis | B::NullCoalesce => {
                if mode == Mode::Compile {
                    return None;
                }
                let test = self.fold(binary.test.as_ref()?, mode)?;
                let chosen = if truthy(&test) {
                    &binary.left
                } else {
                    &binary.right
                };
                return self.fold(chosen, mode);
            }
            // calls, casts of calls, assignments
            _ => return None,
        };
        let left = self.fold(&binary.left, mode)?;
        let right = self.fold(&binary.right, mode)?;
        match binary.op {
            B::Equal => compare::equals(&left, &right, &mut Ignore)
                .ok()
                .map(Value::Bool),
            B::NotEqual => compare::not_equals(&left, &right, &mut Ignore)
                .ok()
                .map(Value::Bool),
            B::Less => compare::less(&left, &right, &mut Ignore)
                .ok()
                .map(Value::Bool),
            B::LessEqual => compare::less_or_equal(&left, &right, &mut Ignore)
                .ok()
                .map(Value::Bool),
            B::Spaceship => compare::spaceship(&left, &right, &mut Ignore)
                .ok()
                .map(Value::Int),
            B::SameAs => Some(Value::Bool(compare::identical(&left, &right))),
            B::NotSameAs => Some(Value::Bool(!compare::identical(&left, &right))),
            B::Xor => Some(Value::Bool(truthy(&left) != truthy(&right))),
            _ => fold_binary(php, &left, &right),
        }
    }

    /// The test classes that compile inline (TWIG/src/Node/Expression/Test/).
    fn fold_test(&self, test: &ast::TestExpr, mode: Mode) -> Option<Value> {
        let argument = |i: i64| -> Option<&Expr> {
            let argument = test.arguments.as_ref()?.get(&ast::ArgumentKey::Index(i))?;
            (!matches!(&argument.kind, ExprKind::Unary(u) if u.op == UnaryOp::Spread))
                .then_some(argument)
        };
        match test.kind {
            // `(null === node)`
            TestKind::Null => {
                let value = self.fold(&test.node, mode)?;
                Some(Value::Bool(value.is_null()))
            }
            // `(node === arguments[0])`
            TestKind::SameAs => {
                let left = self.fold(&test.node, mode)?;
                let right = self.fold(argument(0)?, mode)?;
                Some(Value::Bool(compare::identical(&left, &right)))
            }
            // `(node % 2 == 0)`, `(node % 2 != 0)`
            TestKind::Even | TestKind::Odd => {
                let value = self.fold(&test.node, mode)?;
                let rest = fold_binary(PhpOp::Mod, &value, &Value::Int(2))?;
                let equal = compare::equals(&rest, &Value::Int(0), &mut Ignore).ok()?;
                Some(Value::Bool(equal == (test.kind == TestKind::Even)))
            }
            // `(0 == node % arguments[0])`
            TestKind::DivisibleBy => {
                let value = self.fold(&test.node, mode)?;
                let divisor = self.fold(argument(0)?, mode)?;
                let rest = fold_binary(PhpOp::Mod, &value, &divisor)?;
                compare::equals(&Value::Int(0), &rest, &mut Ignore)
                    .ok()
                    .map(Value::Bool)
            }
            // DefinedTest::compile(): the node in defined-test mode
            TestKind::Defined => self.fold(&test.node, mode),
            TestKind::Test
            | TestKind::NonStringCoercingInput
            | TestKind::Constant
            | TestKind::True => None,
        }
    }

    /// `zend_try_ct_eval_array()` of the PHP `ArrayExpression::compile()` writes: `[v0, v1, ...]` while the keys are
    /// `0, 1, ...`, then `key => value` (a `TempNameExpression` key by its name, a dynamic key cast with
    /// `(string)`), and `...value` for spreads. Every key and value must fold (`zend_eval_const_expr()`); then the
    /// array is built, which gives up on a key PHP converts at runtime (a float that is not an integer, `null`) and
    /// fails the compilation for an unpacked constant that is not an array.
    fn fold_array(&self, array: &ast::ArrayExpr) -> Result<Option<Value>, CompileFatal> {
        enum Element {
            Spread(Value),
            Keyed(Value, Value),
            Next(Value),
        }
        let mut elements = Vec::with_capacity(array.elements.len());
        let mut constant = true;
        let mut is_sequence = true;
        for (i, (key, value)) in array.elements.iter().enumerate() {
            let key_value = match &key.kind {
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
            if !matches!(&key_value, Some(Value::Int(k)) if *k == i as i64) {
                is_sequence = false;
            }
            if let ExprKind::Unary(unary) = &value.kind
                && unary.op == UnaryOp::Spread
            {
                match self.eval_const(&unary.node) {
                    Some(v) => elements.push(Element::Spread(v)),
                    None => constant = false,
                }
                continue;
            }
            let value = match self.eval_const(value) {
                Some(value) => value,
                None => {
                    constant = false;
                    continue;
                }
            };
            if is_sequence {
                elements.push(Element::Next(value));
                continue;
            }
            let key = match key_value {
                Some(key) => key,
                // ` (string)<key>` (StringCastUnary)
                None => match self.eval_const(key).as_ref().and_then(string_cast) {
                    Some(key) => key,
                    None => {
                        constant = false;
                        continue;
                    }
                },
            };
            elements.push(Element::Keyed(key, value));
        }
        if !constant {
            return Ok(None);
        }
        let mut result = Array::new();
        for element in elements {
            match element {
                Element::Spread(Value::Array(unpacked)) => {
                    for (key, value) in &unpacked {
                        match key {
                            ArrayKey::Str(_) => {
                                result.insert(key.clone(), value.clone());
                            }
                            ArrayKey::Int(_) => {
                                if result.push(value.clone()).is_err() {
                                    return Ok(None);
                                }
                            }
                        }
                    }
                }
                Element::Spread(other) => {
                    return Err(CompileFatal(format!(
                        "Only arrays and Traversables can be unpacked, {} given",
                        twig_value::types::zend_zval_value_name(&other)
                    )));
                }
                Element::Keyed(key, value) => {
                    let key = match key {
                        Value::Int(i) => ArrayKey::Int(i),
                        Value::String(s) => ArrayKey::from_string(s),
                        Value::Float(d) => {
                            let l = convert::dval_to_lval_silent(d);
                            if !convert::is_long_compatible(d, l) {
                                return Ok(None);
                            }
                            ArrayKey::Int(l)
                        }
                        Value::Bool(b) => ArrayKey::Int(i64::from(b)),
                        // a null key is converted (and reported) at runtime
                        Value::Null => return Ok(None),
                        Value::Array(_) | Value::Object(_) => {
                            return Err(CompileFatal("Illegal offset type".to_owned()));
                        }
                    };
                    result.insert(key, value);
                }
                Element::Next(value) => {
                    if result.push(value).is_err() {
                        return Ok(None);
                    }
                }
            }
        }
        Ok(Some(Value::Array(result)))
    }

    /// The kind of operand the PHP of `expr` compiles to (`znode.op_type`). `folding`: as for
    /// [`const_operand`](Self::const_operand). Inside an array literal, `zend_eval_const_expr()` has replaced the
    /// constant parts of the AST by values and a conditional whose test folds by its chosen branch (`zend_eval_const_expr()`
    /// on `ZEND_AST_CONDITIONAL`), so the operand is what that branch compiles to; elsewhere `zend_compile_conditional()`
    /// does not fold, and a conditional is a temporary (`JMPZ` + `QM_ASSIGN`).
    pub(crate) fn operand_kind(&self, expr: &Expr, folding: bool) -> OperandKind {
        if self.const_operand(expr, folding).is_some() {
            return OperandKind::Const;
        }
        if folding && let Some(chosen) = self.chosen_branch(expr) {
            return self.operand_kind(chosen, folding);
        }
        match &expr.kind {
            ExprKind::ContextVariable(name) if !name.defined_test => match name.name.as_slice() {
                b"_context" => OperandKind::Cv,
                // `$this->getTemplateName()`, `$this->env->getCharset()`
                b"_self" | b"_charset" => OperandKind::Var,
                _ => OperandKind::Tmp,
            },
            // `array_key_exists(...)` (ZEND_ARRAY_KEY_EXISTS)
            ExprKind::ContextVariable(_) => OperandKind::Tmp,
            ExprKind::LocalVariable(_) => OperandKind::Cv,
            ExprKind::Binary(binary) => match binary.op {
                // CoreExtension::inFilter(), matches(), arraySome()/arrayEvery(), range()
                BinaryOp::In
                | BinaryOp::Matches
                | BinaryOp::HasSome
                | BinaryOp::HasEvery
                | BinaryOp::Range => OperandKind::Var,
                _ => OperandKind::Tmp,
            },
            ExprKind::GetAttr(get_attr) => {
                // the inline `[]` access and the null-safe short circuit are conditionals
                let inline = get_attr.optimizable
                    && (!self.strict_variables || get_attr.ignore_strict_check)
                    && !get_attr.defined_test
                    && get_attr.call_type == ast::CallType::Array;
                if inline || has_null_safe(get_attr) {
                    OperandKind::Tmp
                } else {
                    OperandKind::Var
                }
            }
            ExprKind::MacroReference(_)
            | ExprKind::BlockReference(_)
            | ExprKind::Parent(_)
            | ExprKind::CheckToString(_) => OperandKind::Var,
            ExprKind::Filter(filter) => match filter.kind {
                FilterKind::Default | FilterKind::Raw => self.operand_kind(&filter.node, folding),
                _ => OperandKind::Var,
            },
            ExprKind::Function(function) => {
                if function.kind == FunctionKind::Function && is_frameless_call(function) {
                    OperandKind::Tmp
                } else {
                    OperandKind::Var
                }
            }
            ExprKind::Test(test) => match test.kind {
                TestKind::Test | TestKind::NonStringCoercingInput => OperandKind::Var,
                TestKind::Defined => self.operand_kind(&test.node, folding),
                _ => OperandKind::Tmp,
            },
            _ => OperandKind::Tmp,
        }
    }

    /// The first `E_COMPILE_ERROR` PHP raises while `zend_compile_array()` compiles the outermost array literal
    /// `array` (one that is not part of the folding of an enclosing literal): `zend_try_ct_eval_array()` first runs
    /// `zend_eval_const_expr()` over every element (the value before the key, PHPSRC/Zend/zend_compile.c), which
    /// builds every nested literal that folds, and then builds `array` itself when all its elements folded. Building
    /// a literal that unpacks a constant that is not an array fails the compilation ([`array_fatal`](Self::array_fatal)),
    /// so the nested literals fail first, in that order, before anything of the elements is compiled.
    pub(crate) fn array_eval_fatal(&self, array: &ast::ArrayExpr) -> Option<CompileFatal> {
        let mut is_sequence = true;
        for (i, (key, value)) in array.elements.iter().enumerate() {
            let static_key = match &key.kind {
                ExprKind::LocalVariable(name)
                | ExprKind::MacroVariable(ast::MacroVariableExpr { name, .. }) => {
                    Some(match name {
                        TempName::Int(i) => Some(*i),
                        _ => None,
                    })
                }
                ExprKind::Constant(constant) => Some(match &constant.value {
                    ConstantValue::Int(i) => Some(*i),
                    _ => None,
                }),
                _ => None,
            };
            if static_key != Some(Some(i as i64)) {
                is_sequence = false;
            }
            if let ExprKind::Unary(unary) = &value.kind
                && unary.op == UnaryOp::Spread
            {
                if let Some(fatal) = self.eval_fatal(&unary.node) {
                    return Some(fatal);
                }
                continue;
            }
            if let Some(fatal) = self.eval_fatal(value) {
                return Some(fatal);
            }
            // a dynamic key is written as ` (string) key` (StringCastUnary)
            if !is_sequence
                && static_key.is_none()
                && let Some(fatal) = self.eval_fatal(key)
            {
                return Some(fatal);
            }
        }
        self.array_fatal(array)
    }

    /// [`array_eval_fatal`](Self::array_eval_fatal) of the literal `[v0, v1, ...]` (the values of a `{% set %}`
    /// with several targets: `[$context["a"], $context["b"]] = [v0, v1];`).
    pub(crate) fn values_eval_fatal<'e>(
        &self,
        values: impl IntoIterator<Item = &'e Expr>,
    ) -> Option<CompileFatal> {
        values.into_iter().find_map(|value| self.eval_fatal(value))
    }

    /// The first `E_COMPILE_ERROR` of `zend_eval_const_expr()` over the PHP of `expr` (see
    /// [`array_eval_fatal`](Self::array_eval_fatal)): it descends into operators, casts, conditionals and array
    /// literals, children first, and not into calls, assignments, `isset()`, `instanceof` and closures. Both operands
    /// of `&&` and `||` are evaluated before the left one decides; a conditional whose test folds is replaced by the
    /// branch it chooses before the other branch is evaluated.
    fn eval_fatal(&self, expr: &Expr) -> Option<CompileFatal> {
        match &expr.kind {
            ExprKind::Array(array) => {
                if array.defined_test {
                    // `true`
                    return None;
                }
                self.array_eval_fatal(array)
            }
            ExprKind::ConditionalTernary(ternary) => {
                self.conditional_eval_fatal(&ternary.test, &ternary.left, &ternary.right)
            }
            ExprKind::Binary(binary) => match binary.op {
                BinaryOp::Elvis | BinaryOp::NullCoalesce => {
                    let test = binary.test.as_ref()?;
                    self.conditional_eval_fatal(test, &binary.left, &binary.right)
                }
                _ if folds_children(expr) => self
                    .eval_fatal(&binary.left)
                    .or_else(|| self.eval_fatal(&binary.right)),
                _ => None,
            },
            ExprKind::Unary(unary) => self.eval_fatal(&unary.node),
            ExprKind::Filter(filter) => match filter.kind {
                FilterKind::Default | FilterKind::Raw => self.eval_fatal(&filter.node),
                _ => None,
            },
            ExprKind::Test(test) => {
                let argument = || {
                    test.arguments
                        .as_ref()?
                        .get(&ast::ArgumentKey::Index(0))
                        .and_then(|argument| self.eval_fatal(argument))
                };
                match test.kind {
                    // `(null === node)`, `(node % 2 == 0)`, the node in defined-test mode, `(node === constant(...))`
                    TestKind::Null
                    | TestKind::Even
                    | TestKind::Odd
                    | TestKind::Defined
                    | TestKind::Constant => self.eval_fatal(&test.node),
                    // `(node === arguments[0])`, `(0 == node % arguments[0])`
                    TestKind::SameAs | TestKind::DivisibleBy => {
                        self.eval_fatal(&test.node).or_else(argument)
                    }
                    TestKind::Test | TestKind::NonStringCoercingInput | TestKind::True => None,
                }
            }
            _ => None,
        }
    }

    /// `zend_eval_const_expr()` of `((test) ? (left) : (right))`: the test, then the branch it chooses when it folds,
    /// else both branches.
    fn conditional_eval_fatal(
        &self,
        test: &Expr,
        left: &Expr,
        right: &Expr,
    ) -> Option<CompileFatal> {
        if let Some(fatal) = self.eval_fatal(test) {
            return Some(fatal);
        }
        match self.eval_const(test) {
            Some(test) => self.eval_fatal(if truthy(&test) { left } else { right }),
            None => self.eval_fatal(left).or_else(|| self.eval_fatal(right)),
        }
    }
}

/// Whether the object chain of `get_attr` has a null-safe node (`?.`).
fn has_null_safe(get_attr: &ast::GetAttrExpr) -> bool {
    let mut node = get_attr;
    loop {
        if node.null_safe {
            return true;
        }
        match &node.node.kind {
            ExprKind::GetAttr(inner) => node = inner,
            _ => return false,
        }
    }
}

/// A call PHP compiles to `ZEND_FRAMELESS_ICALL_*` (whose result is a temporary): an internal function declared
/// `@frameless-function` for the number of arguments given, called with positional arguments only
/// (`zend_compile_frameless_icall()`, PHPSRC/Zend/zend_compile.c; PHPSRC/ext/standard/basic_functions.stub.php).
/// Of Twig's callables only the functions `max` and `min` (PHP's `max()` and `min()`, frameless with 2 arguments)
/// can be.
fn is_frameless_call(function: &ast::FunctionExpr) -> bool {
    let name = function.function.name();
    if name != b"max" && name != b"min" {
        return false;
    }
    let entries = function.arguments.entries();
    entries.len() == 2
        && entries.iter().all(|(key, value)| {
            matches!(key, ast::ArgumentKey::Index(_))
                && !matches!(&value.kind, ExprKind::Unary(u) if u.op == UnaryOp::Spread)
        })
}

/// `zend_try_ct_eval_cast()` to `IS_STRING` (PHPSRC/Zend/zend_compile.c): floats (whose conversion depends on the
/// runtime `precision`) and arrays are left to runtime.
fn string_cast(value: &Value) -> Option<Value> {
    match value {
        Value::Float(_) | Value::Array(_) => None,
        value => convert::to_string(value, &mut Ignore)
            .ok()
            .map(Value::String),
    }
}

/// PHP truthiness of a compile-time constant (`zend_is_true()`).
pub(crate) fn truthy(value: &Value) -> bool {
    convert::to_bool(value, &mut Ignore).unwrap_or(true)
}

/// `zend_try_ct_eval_binary_op()`: the result of a numeric operator or `.` between two constants, unless
/// [`binary_op_produces_error()`].
fn fold_binary(op: PhpOp, left: &Value, right: &Value) -> Option<Value> {
    if binary_op_produces_error(op, left, right) {
        return None;
    }
    let op = match op {
        PhpOp::Add => ops::BinaryOp::Add,
        PhpOp::Sub => ops::BinaryOp::Sub,
        PhpOp::Mul => ops::BinaryOp::Mul,
        PhpOp::Div => ops::BinaryOp::Div,
        PhpOp::Mod => ops::BinaryOp::Mod,
        PhpOp::Pow => ops::BinaryOp::Pow,
        PhpOp::Concat => ops::BinaryOp::Concat,
        PhpOp::BwOr => ops::BinaryOp::BitOr,
        PhpOp::BwAnd => ops::BinaryOp::BitAnd,
        PhpOp::BwXor => ops::BinaryOp::BitXor,
        PhpOp::Other => return None,
    };
    ops::binary_op(op, left, right, &mut Ignore).ok()
}

/// Port of `zend_binary_op_produces_error()` (PHPSRC/Zend/zend_compile.c): whether evaluating the operator at
/// compile time could raise an error or a diagnostic, in which case PHP leaves it to run time.
fn binary_op_produces_error(op: PhpOp, op1: &Value, op2: &Value) -> bool {
    if op == PhpOp::Concat {
        // Array to string warning
        return is_array(op1) || is_array(op2);
    }
    if op == PhpOp::Other {
        // only the numeric operations throw errors
        return false;
    }
    if is_array(op1) || is_array(op2) {
        // adding two arrays is allowed; numeric operators throw for one array operand
        return !(op == PhpOp::Add && is_array(op1) && is_array(op2));
    }
    let bitwise = matches!(op, PhpOp::BwOr | PhpOp::BwAnd | PhpOp::BwXor);
    // bitwise operators produce no errors for two strings
    if bitwise && matches!(op1, Value::String(_)) && matches!(op2, Value::String(_)) {
        return false;
    }
    if let Value::String(s) = op1
        && numeric::is_numeric_string(s.as_bytes(), false).is_none()
    {
        return true;
    }
    if let Value::String(s) = op2
        && numeric::is_numeric_string(s.as_bytes(), false).is_none()
    {
        return true;
    }
    // operations casting floats and float strings to integers may produce incompatible-float errors
    if bitwise && (!is_op_long_compatible(op1) || !is_op_long_compatible(op2)) {
        return true;
    }
    if op == PhpOp::Div && get_double(op2) == 0.0 {
        // division by zero throws
        return true;
    }
    if op == PhpOp::Mod
        && (!is_op_long_compatible(op1) || !is_op_long_compatible(op2) || get_long(op2) == 0)
    {
        return true;
    }
    if op == PhpOp::Pow && get_double(op1) == 0.0 && get_double(op2) < 0.0 {
        // 0 ** (<0) throws a division by zero error
        return true;
    }
    false
}

fn is_array(value: &Value) -> bool {
    matches!(value, Value::Array(_))
}

/// Port of `zend_is_op_long_compatible()` (PHPSRC/Zend/zend_compile.c).
fn is_op_long_compatible(op: &Value) -> bool {
    match op {
        Value::Array(_) => false,
        Value::Float(d) => convert::is_long_compatible(*d, convert::dval_to_lval_silent(*d)),
        Value::String(s) => match numeric::is_numeric_str_function(s.as_bytes()) {
            None => false,
            Some(twig_value::Number::Float(d)) => {
                convert::is_long_compatible(d, convert::dval_to_lval_silent(d))
            }
            Some(twig_value::Number::Int(_)) => true,
        },
        _ => true,
    }
}

/// `zval_get_double()` of a constant.
fn get_double(op: &Value) -> f64 {
    convert::to_double(op, &mut Ignore).unwrap_or(0.0)
}

/// `zval_get_long()` of a constant.
fn get_long(op: &Value) -> i64 {
    convert::to_long(op, &mut Ignore).unwrap_or(0)
}

/// Where the line of the PHP node of an expression comes from (see [`first_line`]).
#[derive(Debug, Clone, Copy)]
pub(crate) enum FirstLine<'e> {
    /// A marker for this line is written right before the token that gives the node its line.
    Marker(i64),
    /// No marker of the expression comes before that token: it is written at the marker in effect where the
    /// expression starts.
    Start,
    /// That token follows the dynamic key `key` of an array literal, which `ArrayExpression::compile()` writes
    /// before the value (` (string) key => value`, TWIG/src/Node/Expression/ArrayExpression.php), and no marker comes
    /// between them: it is written at the marker in effect once the key is compiled.
    AfterKey(&'e Expr),
}

/// Where the line of the PHP node of `expr` comes from: PHP gives an AST node the line of its first child
/// (`zend_ast_create_*()`, PHPSRC/Zend/zend_ast.c), down to a token, so the line of a binary operator is the line of
/// its left operand, the line of an array literal the line of its first element (`zend_ast_create_list()`), whose
/// line is its value's (`ZEND_AST_ARRAY_ELEM` is created as `(value, key)`, PHPSRC/Zend/zend_language_parser.y), and
/// the line of a call the line of its name. `NameExpression::compile()` and `ArrowFunctionExpression::compile()`
/// call `addDebugInfo()` before writing anything, so a node whose first token comes from one of them gets that
/// marker's line ([`FirstLine::Marker`]).
pub(crate) fn first_line(expr: &Expr) -> FirstLine<'_> {
    match &expr.kind {
        // AssignNameExpression::compile() writes `$context["name"]` without a marker
        ExprKind::ContextVariable(_) | ExprKind::ArrowFunction(_) => FirstLine::Marker(expr.line),
        ExprKind::Array(array) => {
            let Some((key, value)) = array.elements.first().filter(|_| !array.defined_test) else {
                return FirstLine::Start;
            };
            match &value.kind {
                // `...value`: no key is written
                ExprKind::Unary(unary) if unary.op == UnaryOp::Spread => first_line(&unary.node),
                _ => match first_line(value) {
                    // a constant or temporary-name key is written as a literal (`ArrayExpression::compile()`)
                    FirstLine::Start
                        if !matches!(
                            key.kind,
                            ExprKind::Constant(_)
                                | ExprKind::LocalVariable(_)
                                | ExprKind::MacroVariable(_)
                        ) =>
                    {
                        FirstLine::AfterKey(key)
                    }
                    line => line,
                },
            }
        }
        ExprKind::Unary(unary) => first_line(&unary.node),
        ExprKind::Binary(binary) => match binary.op {
            // `(left OP right)` and the operators written like it
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Power
            | BinaryOp::Concat
            | BinaryOp::BitwiseAnd
            | BinaryOp::BitwiseOr
            | BinaryOp::BitwiseXor
            | BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::Greater
            | BinaryOp::LessEqual
            | BinaryOp::GreaterEqual
            | BinaryOp::Spaceship
            | BinaryOp::SameAs
            | BinaryOp::NotSameAs
            | BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Xor => first_line(&binary.left),
            // `((test) ? (left) : (right))`
            BinaryOp::Elvis | BinaryOp::NullCoalesce => {
                binary.test.as_ref().map_or(FirstLine::Start, first_line)
            }
            // calls (`CoreExtension::inFilter(`, `(int) floor(`, ...) and assignments
            _ => FirstLine::Start,
        },
        ExprKind::ConditionalTernary(ternary) => first_line(&ternary.test),
        ExprKind::Filter(filter) => match filter.kind {
            FilterKind::Default | FilterKind::Raw => first_line(&filter.node),
            _ => FirstLine::Start,
        },
        ExprKind::Test(test) => match test.kind {
            TestKind::SameAs
            | TestKind::Even
            | TestKind::Odd
            | TestKind::Constant
            | TestKind::Defined => first_line(&test.node),
            // `(null === `, `(0 == `, `(($tmp = `, calls
            _ => FirstLine::Start,
        },
        _ => FirstLine::Start,
    }
}

/// Whether PHP's AST of `expr` is already a value (`ZEND_AST_ZVAL`, from a number or string literal) that
/// `zend_eval_const_expr()` leaves alone, with its own line. `Compiler::repr()` writes a non-negative number or a
/// string as a literal; `null`, `true`, `false`, `INF`, `NAN`, negative numbers (`-1` is a unary minus) and arrays
/// are expressions PHP folds into a new value.
pub(crate) fn is_php_literal(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Constant(constant) if !constant.defined_test => match &constant.value {
            ConstantValue::Int(i) => *i >= 0,
            ConstantValue::Float(f) => f.is_finite() && f.is_sign_positive(),
            ConstantValue::String(_) => true,
            _ => false,
        },
        _ => false,
    }
}

/// Whether PHP's parser already turns the PHP of `expr` into a value (`ZEND_AST_ZVAL`) with the line it is written
/// on: a literal ([`is_php_literal`]), the concatenation of two such values, which `zend_ast_create_concat_op()`
/// (PHPSRC/Zend/zend_ast.c) folds while parsing `(a . b)` unless `zend_binary_op_produces_error()` (only arrays,
/// which are never literals), with the line of the `)` that follows (`CG(zend_lineno)`, no marker comes between), and
/// a `RawFilter` or `DefaultFilter`, which compile as their node. `-1 . "b"` is not folded by the parser (`-1` is a
/// unary minus).
pub(crate) fn is_parsed_value(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Binary(binary) if binary.op == BinaryOp::Concat => {
            is_parsed_value(&binary.left) && is_parsed_value(&binary.right)
        }
        ExprKind::Filter(filter)
            if matches!(filter.kind, FilterKind::Default | FilterKind::Raw) =>
        {
            is_parsed_value(&filter.node)
        }
        _ => is_php_literal(expr),
    }
}

/// Whether `zend_eval_const_expr()` descends from the PHP of `expr` into the PHP of its children: it does for
/// operators, conditionals, `??`, casts and array literals, and not for calls, assignments, `isset()`,
/// `instanceof` and closures (PHPSRC/Zend/zend_compile.c), so the children of those are compiled outside the
/// folding of an enclosing array literal.
pub(crate) fn folds_children(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Binary(binary) => !matches!(
            binary.op,
            // `(int) floor(...)`, calls, `is_string($_v = ...)`, assignments
            BinaryOp::FloorDiv
                | BinaryOp::In
                | BinaryOp::NotIn
                | BinaryOp::Matches
                | BinaryOp::HasSome
                | BinaryOp::HasEvery
                | BinaryOp::Range
                | BinaryOp::StartsWith
                | BinaryOp::EndsWith
                | BinaryOp::Set
                | BinaryOp::SequenceDestructuringSet
                | BinaryOp::ObjectDestructuringSet
        ),
        ExprKind::Test(test) => !matches!(
            test.kind,
            // `(($tmp = ...) && ...)`, calls
            TestKind::True | TestKind::Test | TestKind::NonStringCoercingInput
        ),
        ExprKind::Filter(filter) => matches!(filter.kind, FilterKind::Default | FilterKind::Raw),
        ExprKind::Constant(_)
        | ExprKind::ContextVariable(_)
        | ExprKind::AssignContextVariable(_)
        | ExprKind::LocalVariable(_)
        | ExprKind::Array(_)
        | ExprKind::Unary(_)
        | ExprKind::ConditionalTernary(_)
        | ExprKind::Empty => true,
        _ => false,
    }
}

/// Whether PHP fails to parse the code Twig generates for the module `node` (and its embedded templates, which are
/// classes of the same file): Twig compiles a `SpreadUnary` as ` ...expr` wherever its node lands, and PHP accepts
/// `...` only in an argument list or an array literal. A spread argument lands elsewhere through the node classes
/// that place their arguments themselves: `DefaultFilter` (the first argument is also the else branch of its
/// defined test, `x|default(...y)`), `SameasTest` (`(x === ...y)`), `DivisiblebyTest` (`(0 == x % ...y)`) and
/// `ConstantTest` with an object (`constant(get_class(o)."::". ...y)`). PHP then throws `ParseError: syntax error,
/// unexpected token "..."` when it loads the template. Node classes of extensions write no PHP: where their children
/// land is their own code's business, so a spread child of theirs is no error (the children of the children are
/// checked as anywhere else).
pub(crate) fn generated_parse_error(node: &crate::ast::Node) -> bool {
    use crate::ast::NodeRef;
    fn walk_node(node: &crate::ast::Node) -> bool {
        // an expression used as a statement (`{{ block(...) }}` in output mode) is the expression node itself
        if let crate::ast::NodeKind::Expression(expr) = &node.kind {
            return walk_expr(expr, false);
        }
        let custom = matches!(node.kind, crate::ast::NodeKind::Custom(_));
        node.children().into_iter().any(|(_, child)| match child {
            NodeRef::Node(child) => walk_node(child),
            NodeRef::Expr(expr) => walk_expr(expr, custom),
            NodeRef::Arguments(arguments) => arguments
                .entries()
                .iter()
                .any(|(_, value)| walk_expr(value, custom)),
        })
    }
    /// `allowed`: the PHP position of `expr` accepts `...`.
    fn walk_expr(expr: &Expr, allowed: bool) -> bool {
        if let ExprKind::Unary(unary) = &expr.kind
            && unary.op == UnaryOp::Spread
        {
            return !allowed || walk_expr(&unary.node, false);
        }
        let arguments = |arguments: &ast::Arguments, allowed: bool| {
            arguments
                .entries()
                .iter()
                .any(|(_, value)| walk_expr(value, allowed))
        };
        match &expr.kind {
            ExprKind::Array(array) => array
                .elements
                .iter()
                .any(|(key, value)| walk_expr(key, false) || walk_expr(value, true)),
            ExprKind::Filter(filter) => match filter.kind {
                // compiled as their node only
                FilterKind::Default | FilterKind::Raw => walk_expr(&filter.node, false),
                _ => walk_expr(&filter.node, false) || arguments(&filter.arguments, true),
            },
            ExprKind::Function(function) => arguments(&function.arguments, true),
            ExprKind::Test(test) => {
                let node = walk_expr(&test.node, false);
                let Some(args) = &test.arguments else {
                    return node;
                };
                node || match test.kind {
                    TestKind::Test | TestKind::NonStringCoercingInput => arguments(args, true),
                    TestKind::SameAs | TestKind::DivisibleBy => args
                        .get(&ast::ArgumentKey::Index(0))
                        .is_some_and(|value| walk_expr(value, false)),
                    TestKind::Constant => {
                        let object = args.get(&ast::ArgumentKey::Index(1));
                        args.get(&ast::ArgumentKey::Index(0))
                            .is_some_and(|value| walk_expr(value, object.is_none()))
                            || object.is_some_and(|value| walk_expr(value, true))
                    }
                    // the arguments are not compiled
                    TestKind::Null
                    | TestKind::Even
                    | TestKind::Odd
                    | TestKind::Defined
                    | TestKind::True => false,
                }
            }
            // call arguments (PHP refuses the positional arguments after them when it compiles the call)
            ExprKind::GetAttr(get_attr) => {
                walk_expr(&get_attr.node, true)
                    || walk_expr(&get_attr.attribute, true)
                    || get_attr
                        .arguments
                        .as_ref()
                        .is_some_and(|a| walk_expr(a, true))
            }
            ExprKind::BlockReference(block) => {
                walk_expr(&block.name, true)
                    || block.template.as_ref().is_some_and(|t| walk_expr(t, true))
            }
            _ => {
                let custom = matches!(expr.kind, ExprKind::Custom(_));
                expr.children().into_iter().any(|(_, child)| match child {
                    NodeRef::Expr(child) => walk_expr(child, custom),
                    NodeRef::Node(child) => walk_node(child),
                    NodeRef::Arguments(args) => arguments(args, custom),
                })
            }
        }
    }
    if walk_node(node) {
        return true;
    }
    match &node.kind {
        crate::ast::NodeKind::Module(module) => {
            module.embedded_templates().any(generated_parse_error)
        }
        _ => false,
    }
}
