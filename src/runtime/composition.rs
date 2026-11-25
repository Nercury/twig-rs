//! Template composition at runtime: the methods of `Twig\Template` that compiled templates call to render a template,
//! its parent and its blocks, and to load other templates (TWIG/src/Template.php), plus the template lookups of
//! `Twig\Environment` they rely on (`load()`, `resolveTemplate()`, TWIG/src/Environment.php).
//!
//! Each function names the method it ports and keeps its evaluation order, its exception handlers (which template
//! handles an exception decides the template name and line of the error, `Template::handleException()`) and its
//! messages. PHP generators become direct calls writing to the render's [`Output`]; `render*()` methods capture.

use std::sync::{Arc, LazyLock};

use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Array, ArrayKey, Diagnostics, KeyContext, PhpString, Value, ops};

use super::sandbox::ensure_security_checked;
use super::template::{BlockRef, Blocks, Template, TemplateObject, TemplateWrapper};
use crate::error::{Error, ErrorKind};
use crate::lltl::ParentDef;
use crate::vm::{Exception, Frame, Output, VmState, guess, handle_exception, run_unit};

/// `string|TemplateWrapper|array`, the type of `Template::load()`'s `$template`.
static LOAD_TYPE: LazyLock<DeclaredType<'static>> = LazyLock::new(|| {
    DeclaredType::new(
        &["Twig\\TemplateWrapper"],
        TypeMask::ARRAY | TypeMask::STRING,
    )
});

/// Twig: `Template::yield()`: the globals are added under the context (`$context += $this->env->getGlobals()`), the
/// caller's blocks override the template's own (`array_merge($this->blocks, $blocks)`), the sandbox checks run, the
/// body runs (`doDisplay()`), and an exception is handled by this template.
pub(crate) fn yield_template(
    state: &mut VmState,
    out: &mut Output<'_>,
    template: &Arc<Template>,
    context: Array,
    blocks: &Blocks,
) -> Result<(), Exception> {
    let globals = state.env.globals();
    let context = ops::array_union(&context, &globals);
    let blocks = template.block_table().merge(blocks);
    let program = template.program();
    let result = ensure_security_checked(state, template).and_then(|()| {
        // doDisplay(): `$macros = $this->macros;`
        let mut frame = Frame::new(context, Arc::clone(template), template.macros(), blocks);
        run_unit(state, out, &mut frame, program, program.body()).map(|_| ())
    });
    result.map_err(|e| handle_exception(state, e, program))
}

/// Twig: `Template::getParent()`: `$this->parent` when it is not `null`; otherwise, after the sandbox checks,
/// `doGetParent($context)` (a constant parent is loaded once and kept in `$this->parent`, a dynamic one is evaluated
/// with the given context every time), whose exceptions this template handles. `None` stands for `false`.
pub(crate) fn get_parent(
    state: &mut VmState,
    this: &Arc<Template>,
    context: &Array,
) -> Result<Option<Arc<Template>>, Exception> {
    if let Some(parent) = this.resolved_parent() {
        return Ok(parent);
    }
    ensure_security_checked(state, this)?;
    let program = Arc::clone(this.program());
    let parent = match program.parent() {
        None => return Ok(None),
        Some(ParentDef::Constant { name, line }) => {
            // `return $this->parent ??= $this->load("name", line);`
            load(state, this, name.clone(), *line, None).inspect(|parent| {
                if this.resolved_parent().is_none() {
                    this.set_parent(Arc::clone(parent));
                }
            })
        }
        Some(ParentDef::Dynamic { unit, .. }) => {
            // `$macros = $this->macros; return $this->load(<parent>, line);`
            let mut frame = Frame::new(
                context.clone(),
                Arc::clone(this),
                this.macros(),
                Blocks::default(),
            );
            let mut out = Output::detached();
            run_unit(state, &mut out, &mut frame, &program, *unit).and_then(|value| {
                TemplateObject::template_of(&value).ok_or_else(|| {
                    Error::logic("The parent expression did not load a template.").into()
                })
            })
        }
    };
    parent
        .map(Some)
        .map_err(|e| handle_exception(state, e, &program))
}

/// `isset($blocks[$name])`: the PHP array key of the block name (with PHP's conversions and their diagnostics, done
/// at every `isset()` as in PHP) looked up in a block table. Block names are never numeric, so an integer key
/// matches nothing.
fn isset_block<'a>(
    blocks: &'a Blocks,
    name: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<Option<&'a BlockRef>, Exception> {
    Ok(match ArrayKey::from_value(name, KeyContext::Isset, diag)? {
        ArrayKey::Str(key) => blocks.get(key.as_bytes()),
        ArrayKey::Int(_) => None,
    })
}

/// `isset($this->blocks[$name])`.
fn isset_own_block(
    this: &Arc<Template>,
    name: &Value,
    diag: &mut dyn Diagnostics,
) -> Result<Option<BlockRef>, Exception> {
    Ok(match ArrayKey::from_value(name, KeyContext::Isset, diag)? {
        ArrayKey::Str(key) => this.own_block(key.as_bytes()),
        ArrayKey::Int(_) => None,
    })
}

/// `sprintf('%s', $name)`.
fn name_string(name: &Value, diag: &mut dyn Diagnostics) -> Result<String, Exception> {
    Ok(twig_value::convert::to_string(name, diag)?
        .to_string_lossy()
        .into_owned())
}

/// Twig: `Template::yieldBlock()`: the block from the caller's table (`$useBlocks`), else from this template's own
/// table (which then extends the caller's for nested `block()` calls), runs as a method of its owner, which handles
/// its exceptions; a block this template does not define is looked up in its parent, with `$templateContext` naming
/// the template the lookup started from in the error.
#[allow(clippy::too_many_arguments)]
pub(crate) fn yield_block(
    state: &mut VmState,
    out: &mut Output<'_>,
    this: &Arc<Template>,
    name: &Value,
    context: Array,
    blocks: Blocks,
    use_blocks: bool,
    template_context: Option<Arc<Template>>,
) -> Result<(), Exception> {
    let mut found = None;
    if use_blocks && let Some(block) = isset_block(&blocks, name, state)? {
        found = Some((block.clone(), blocks.clone()));
    }
    if found.is_none()
        && let Some(block) = isset_own_block(this, name, state)?
    {
        // expose this template's own blocks so nested block() calls resolve against them
        found = Some((block, this.block_table().merge(&blocks)));
    }
    if let Some((block, blocks)) = found {
        let owner = Arc::clone(&block.template);
        let program = Arc::clone(owner.program());
        let result = ensure_security_checked(state, &owner).and_then(|()| {
            // block_NAME(): `$macros = $this->macros;`
            let mut frame = Frame::new(context, Arc::clone(&owner), owner.macros(), blocks);
            run_unit(state, out, &mut frame, &program, block.unit).map(|_| ())
        });
        return result.map_err(|e| handle_exception(state, e, &program));
    }
    if let Some(parent) = get_parent(state, this, &context)? {
        let merged = this.block_table().merge(&blocks);
        let template_context = template_context.unwrap_or_else(|| Arc::clone(this));
        return yield_block(
            state,
            out,
            &parent,
            name,
            context,
            merged,
            false,
            Some(template_context),
        );
    }
    if let Some(block) = isset_block(&blocks, name, state)? {
        let owner = Arc::clone(&block.template);
        return Err(parent_block_missing(state, name, &owner, this));
    }
    Err(block_missing(
        state,
        name,
        this,
        template_context.as_ref().unwrap_or(this),
    ))
}

/// `Block "%s" should not call parent() in "%s" as the block does not exist in the parent template "%s".`
#[cold]
fn parent_block_missing(
    state: &mut VmState,
    name: &Value,
    owner: &Template,
    this: &Template,
) -> Exception {
    let name = match name_string(name, state) {
        Ok(name) => name,
        Err(error) => return error,
    };
    Error::runtime(format!(
        "Block \"{name}\" should not call parent() in \"{}\" as the block does not exist in the parent template \"{}\".",
        owner.name(),
        this.name()
    ))
    .with_source(owner.source().clone())
    .into()
}

/// `Block "%s" on template "%s" does not exist.`
#[cold]
fn block_missing(
    state: &mut VmState,
    name: &Value,
    this: &Template,
    template_context: &Template,
) -> Exception {
    let name = match name_string(name, state) {
        Ok(name) => name,
        Err(error) => return error,
    };
    Error::runtime(format!(
        "Block \"{name}\" on template \"{}\" does not exist.",
        this.name()
    ))
    .with_source(template_context.source().clone())
    .into()
}

/// Twig: `Template::yieldParentBlock()`: `parent()` inside the block `name` of this template (the owner of the running
/// block): the block of a trait (under its original name when it was aliased), else the block of the parent.
pub(crate) fn yield_parent_block(
    state: &mut VmState,
    out: &mut Output<'_>,
    this: &Arc<Template>,
    name: &[u8],
    context: Array,
    blocks: Blocks,
) -> Result<(), Exception> {
    if let Some(block) = this.trait_block(name) {
        let owner = Arc::clone(&block.template);
        let block_name = this
            .trait_alias(name)
            .cloned()
            .unwrap_or_else(|| PhpString::from(name));
        return yield_block(
            state,
            out,
            &owner,
            &Value::String(block_name),
            context,
            blocks,
            false,
            None,
        );
    }
    if let Some(parent) = get_parent(state, this, &context)? {
        return yield_block(
            state,
            out,
            &parent,
            &Value::String(PhpString::from(name)),
            context,
            blocks,
            false,
            None,
        );
    }
    Err(Error::runtime(format!(
        "The template has no parent and no traits defining the \"{}\" block.",
        String::from_utf8_lossy(name)
    ))
    .with_source(this.source().clone())
    .into())
}

/// Runs `f` in a capture of `out` (`ob_start()` ... `ob_get_clean()`), dropping the capture when it fails.
fn captured(
    out: &mut Output<'_>,
    f: impl FnOnce(&mut Output<'_>) -> Result<(), Exception>,
) -> Result<PhpString, Exception> {
    let depth = out.capture_depth();
    out.start_capture();
    let result = f(out);
    let captured = out.end_capture();
    out.truncate_captures(depth);
    result.map(|()| PhpString::from(captured))
}

/// Twig: `Template::renderBlock()`: [`yield_block`] captured into a string.
pub(crate) fn render_block(
    state: &mut VmState,
    out: &mut Output<'_>,
    this: &Arc<Template>,
    name: &Value,
    context: Array,
    blocks: Blocks,
    use_blocks: bool,
) -> Result<PhpString, Exception> {
    captured(out, |out| {
        yield_block(state, out, this, name, context, blocks, use_blocks, None)
    })
}

/// Twig: `Template::renderParentBlock()`: [`yield_parent_block`] captured into a string.
pub(crate) fn render_parent_block(
    state: &mut VmState,
    out: &mut Output<'_>,
    this: &Arc<Template>,
    name: &[u8],
    context: Array,
    blocks: Blocks,
) -> Result<PhpString, Exception> {
    captured(out, |out| {
        yield_parent_block(state, out, this, name, context, blocks)
    })
}

/// Twig: `Template::hasBlock()`: in the caller's table, in this template's own table, else in the parent (without
/// the caller's table).
pub(crate) fn has_block(
    state: &mut VmState,
    this: &Arc<Template>,
    name: &Value,
    context: &Array,
    blocks: &Blocks,
) -> Result<bool, Exception> {
    if isset_block(blocks, name, state)?.is_some() {
        // `$blocks[$name][0] instanceof self`
        return Ok(true);
    }
    if isset_own_block(this, name, state)?.is_some() {
        return Ok(true);
    }
    if let Some(parent) = get_parent(state, this, context)? {
        return has_block(state, &parent, name, context, &Blocks::default());
    }
    Ok(false)
}

/// Twig: `Template::getBlockNames()`: `array_unique(array_merge(array_keys($blocks), array_keys($this->blocks),
/// <parent names>))`, as (key, name) pairs: `array_unique()` keeps the key of the first occurrence.
pub(crate) fn block_names(
    state: &mut VmState,
    this: &Arc<Template>,
    context: &Array,
    blocks: &Blocks,
) -> Result<Vec<(i64, PhpString)>, Exception> {
    let mut names: Vec<PhpString> = blocks
        .names()
        .cloned()
        .chain(this.own_block_names().cloned())
        .collect();
    if let Some(parent) = get_parent(state, this, context)? {
        names.extend(
            block_names(state, &parent, context, &Blocks::default())?
                .into_iter()
                .map(|(_, name)| name),
        );
    }
    let mut unique: Vec<(i64, PhpString)> = Vec::new();
    for (i, name) in names.into_iter().enumerate() {
        if !unique.iter().any(|(_, seen)| *seen == name) {
            unique.push((i as i64, name));
        }
    }
    Ok(unique)
}

/// Twig: `Template::load()`: a template from a name, a `TemplateWrapper` of the same environment, or an array of names
/// (the first that exists, `Environment::resolveTemplate()`). A template naming itself loads its own class family
/// (`static::class` without `___N`, so the loader is not asked), with the embedded template `index` when given.
///
/// A Twig error raised while loading gets this template's source when it has none and, unless it has a line,
/// `line` (or a guessed line for `-1`): a missing included template is reported against the including template and
/// the line of the tag.
pub(crate) fn load(
    state: &mut VmState,
    this: &Arc<Template>,
    template: Value,
    line: i64,
    index: Option<i64>,
) -> Result<Arc<Template>, Exception> {
    let template = verify_user_arg(
        template,
        &LOAD_TYPE,
        &UserArg::new("Twig\\Template::load", 1, "template"),
        state,
    )?;
    let result = match &template {
        Value::Array(names) => resolve_template_array(state, names)
            .and_then(|wrapper| wrapper.unwrap_for(&state.env).map_err(Exception::from)),
        Value::String(name) if name.as_bytes() == this.name().as_bytes() => {
            state.env.load_family(this, index).map_err(Exception::from)
        }
        Value::String(name) => state
            .env
            .load_template_index(&name.to_string_lossy(), index)
            .map_err(Exception::from),
        other => match TemplateWrapper::from_value(other) {
            Some(wrapper) => wrapper.unwrap_for(&state.env).map_err(Exception::from),
            None => Err(UserArg::new("Twig\\Template::load", 1, "template")
                .type_error(&LOAD_TYPE, other)
                .into()),
        },
    };
    result.map_err(|mut exception| {
        if exception.is_twig_error() {
            let trace = state.trace();
            if let Some(error) = exception.kind_twig_mut() {
                if error.source_context().is_none() {
                    error.set_source_context(Some(this.source().clone()));
                }
                if error.template_line() <= 0 {
                    if line == -1 {
                        guess(error, state, this.program(), &[&trace]);
                    } else {
                        error.set_template_line(line);
                    }
                }
            }
        }
        exception
    })
}

/// A value converted for a userland `string` parameter (coercive typing mode).
fn string_param(
    value: &Value,
    function: &str,
    name: &str,
    diag: &mut dyn Diagnostics,
) -> Result<PhpString, Exception> {
    match verify_user_arg(
        value.clone(),
        &DeclaredType::builtin(TypeMask::STRING),
        &UserArg::new(function, 1, name),
        diag,
    )? {
        Value::String(string) => Ok(string),
        _ => Ok(PhpString::empty()),
    }
}

/// Twig: `Environment::load()`: a `TemplateWrapper` of this environment as it is, otherwise the template named by
/// the (string) value.
pub(crate) fn env_load(state: &mut VmState, name: &Value) -> Result<TemplateWrapper, Exception> {
    if let Some(wrapper) = TemplateWrapper::from_value(name) {
        wrapper.unwrap_for(&state.env)?;
        return Ok(wrapper);
    }
    let name = string_param(name, "Twig\\Environment::getTemplateClass", "name", state)?;
    let template = state
        .env
        .load_template_index(&name.to_string_lossy(), None)?;
    Ok(TemplateWrapper::new(state.env.clone(), template))
}

/// Twig: `Environment::resolveTemplate()`: a value that is not an array is loaded; in an array, the first
/// `TemplateWrapper`, or the first name the loader knows (a one-element array is loaded without asking).
pub(crate) fn resolve_template(
    state: &mut VmState,
    names: &Value,
) -> Result<TemplateWrapper, Exception> {
    match names {
        Value::Array(names) => resolve_template_array(state, names),
        other => env_load(state, other),
    }
}

fn resolve_template_array(
    state: &mut VmState,
    names: &Array,
) -> Result<TemplateWrapper, Exception> {
    let count = names.len();
    let loader = state.env.loader();
    for name in names.values() {
        if TemplateWrapper::from_value(name).is_some() {
            return env_load(state, name);
        }
        if count != 1 {
            let function = format!("{}::exists", loader.class_name());
            let string = string_param(name, &function, "name", state)?;
            if !loader.exists(&string.to_string_lossy()) {
                continue;
            }
        }
        return env_load(state, name);
    }
    let joined = twig_value::builtins::string::implode_frameless(
        &Value::from("\", \""),
        Some(&Value::Array(names.clone())),
        state,
    )?;
    Err(Error::loader(format!(
        "Unable to find one of the following templates: \"{}\".",
        joined.to_string_lossy()
    ))
    .into())
}

/// `CoreExtension::merge($context, $variables)` as `IncludeNode` compiles it for `with`: each argument must be
/// iterable; the result is `array_merge()` of their arrays (integer keys renumbered).
///
/// # Errors
///
/// `The "merge" filter expects a sequence or a mapping, got "%s" for argument %d.`
pub(crate) fn merge_include_variables(
    context: Array,
    variables: Value,
    diag: &mut dyn Diagnostics,
) -> Result<Array, Exception> {
    if !variables.is_iterable() {
        return Err(Error::runtime(format!(
            "The \"merge\" filter expects a sequence or a mapping, got \"{}\" for argument 2.",
            twig_value::types::get_debug_type(&variables)
        ))
        .into());
    }
    let variables = super::core::to_array(variables, true, diag)?;
    Ok(twig_value::builtins::array::array_merge(&[
        Value::Array(context),
        variables,
    ])?)
}

/// `Template::yield(array $context)` called with a value that is not an array (`with ... only` and a variable that
/// `CoreExtension::toArray()` leaves as it is).
pub(crate) fn yield_context_type_error(value: &Value) -> Exception {
    UserArg::new("Twig\\Template::yield", 1, "context")
        .type_error(&DeclaredType::builtin(TypeMask::ARRAY), value)
        .into()
}

/// Whether an error is a `LoaderError` (what `ignore missing` catches).
pub(crate) fn is_loader_error(exception: &Exception) -> bool {
    exception
        .twig_error()
        .is_some_and(|error| error.kind() == ErrorKind::Loader)
}
