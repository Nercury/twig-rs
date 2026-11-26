//! Template instances, their block tables, and `TemplateWrapper`.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use twig_value::object::{
    CallArgs, ClassBuilder, ClassRef, FunctionKind, Object, ObjectRef, Param,
};
use twig_value::param::{DeclaredType, TypeMask, UserArg, verify_user_arg};
use twig_value::{Array, ArrayKey, Diagnostics, PhpError, PhpResult, PhpString, Value};

use super::composition;
use super::environment::Environment;
use crate::error::{Error, Result};
use crate::lltl::{Program, UnitId};
use crate::source::Source;
use crate::vm::{Exception, Output, Sink, VmState};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// A block method of a template: Twig's `[$template, 'block_NAME']` pairs of `$this->blocks` and `$blocks`.
#[derive(Clone)]
pub(crate) struct BlockRef {
    /// The template owning the method (`$this` of the block).
    pub(crate) template: Arc<Template>,
    /// The unit implementing `block_NAME()`.
    pub(crate) unit: UnitId,
}

/// A table of blocks: Twig's `$blocks` arrays, block name => [`BlockRef`], in PHP array order. Block names are
/// never numeric (they are Twig names), so PHP's `array_merge()` of two tables is "keep the order of first
/// appearance, the right-hand value wins".
#[derive(Clone, Default)]
pub(crate) struct Blocks {
    entries: Arc<Vec<(PhpString, BlockRef)>>,
}

impl Blocks {
    fn from_entries(entries: Vec<(PhpString, BlockRef)>) -> Blocks {
        Blocks {
            entries: Arc::new(entries),
        }
    }

    /// The block named `name` (`$blocks[$name]`).
    pub(crate) fn get(&self, name: &[u8]) -> Option<&BlockRef> {
        self.entries
            .iter()
            .find(|(key, _)| key.as_bytes() == name)
            .map(|(_, block)| block)
    }

    /// The block names in order (`array_keys($blocks)`).
    pub(crate) fn names(&self) -> impl Iterator<Item = &PhpString> {
        self.entries.iter().map(|(name, _)| name)
    }

    /// Whether the table is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `array_merge($this, $other)`.
    pub(crate) fn merge(&self, other: &Blocks) -> Blocks {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other.clone();
        }
        let mut entries = (*self.entries).clone();
        for (name, block) in other.entries.iter() {
            match entries.iter_mut().find(|(key, _)| key == name) {
                Some(entry) => entry.1 = block.clone(),
                None => entries.push((name.clone(), block.clone())),
            }
        }
        Blocks::from_entries(entries)
    }
}

/// An entry of a template's own `$this->blocks`: one of its block methods or a block it got from a trait.
#[derive(Clone)]
enum BlockSlot {
    Own(UnitId),
    Trait(BlockRef),
}

/// `$this->parent`: `null` until a constant parent is resolved, `false` without a parent.
#[derive(Clone)]
enum ParentSlot {
    Unresolved,
    None,
    Some(Arc<Template>),
}

/// What the compiled constructor of a template computes (`ModuleNode::compileConstructor()`): the trait blocks, the
/// trait aliases and the block table.
pub(crate) struct Traits {
    /// `$this->traits`.
    pub(crate) blocks: Vec<(PhpString, BlockRef)>,
    /// `$this->traitAliases`.
    pub(crate) aliases: Vec<(PhpString, PhpString)>,
}

/// A loaded template: the compiled [`Program`] plus the per-instance state Twig keeps on a template object.
///
/// Twig instantiates one object per (environment, template class) and reuses it for every render
/// (`Environment::$loadedTemplates`); its observable state lives here behind interior mutability: the macro
/// namespaces imported in the main scope of the body (`$this->macros`), the resolved constant parent
/// (`$this->parent`), the blocks of its traits (`$this->traits`, resolved when the template is loaded). Embedded
/// templates (`{% embed %}`) are instances of their own, of the classes `___N` of the same file. Instances are shared
/// between threads.
pub struct Template {
    program: Arc<Program>,
    family: Arc<Program>,
    key: Vec<u8>,
    env_id: usize,
    macros: Mutex<Array>,
    parent: Mutex<ParentSlot>,
    traits: Vec<(PhpString, BlockRef)>,
    trait_aliases: Vec<(PhpString, PhpString)>,
    blocks: Vec<(PhpString, BlockSlot)>,
}

impl std::fmt::Debug for Template {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Template")
            .field("name", &self.program.name())
            .field("index", &self.program.index())
            .finish_non_exhaustive()
    }
}

impl Template {
    /// Twig: the compiled constructor once the traits are resolved: `$this->parent = false` without a parent,
    /// `$this->traits`, `$this->blocks = array_merge($this->traits, [own blocks])`.
    pub(crate) fn new(
        program: Arc<Program>,
        family: Arc<Program>,
        key: Vec<u8>,
        env_id: usize,
        traits: Traits,
    ) -> Self {
        let parent = if program.parent().is_some() {
            ParentSlot::Unresolved
        } else {
            ParentSlot::None
        };
        let mut blocks: Vec<(PhpString, BlockSlot)> = traits
            .blocks
            .iter()
            .map(|(name, block)| (name.clone(), BlockSlot::Trait(block.clone())))
            .collect();
        for block in program.blocks() {
            let name = PhpString::from(block.name.as_slice());
            match blocks.iter_mut().find(|(key, _)| *key == name) {
                Some(entry) => entry.1 = BlockSlot::Own(block.unit),
                None => blocks.push((name, BlockSlot::Own(block.unit))),
            }
        }
        Template {
            program,
            family,
            key,
            env_id,
            macros: Mutex::new(Array::new()),
            parent: Mutex::new(parent),
            traits: traits.blocks,
            trait_aliases: traits.aliases,
            blocks,
        }
    }

    /// The compiled program.
    pub fn program(&self) -> &Arc<Program> {
        &self.program
    }

    /// The program of the compiled file the template belongs to (the root template of its embedded templates).
    pub(crate) fn family(&self) -> &Arc<Program> {
        &self.family
    }

    /// The cache key of the template's class family (Twig's class name without `___N`).
    pub(crate) fn key(&self) -> &[u8] {
        &self.key
    }

    /// The identity of the environment that loaded the template (`Template::isOwnedBy()`).
    pub(crate) fn env_id(&self) -> usize {
        self.env_id
    }

    /// The template name. Twig: `getTemplateName()`.
    pub fn name(&self) -> &str {
        self.program.name()
    }

    /// The source of the compiled template: its name, its path and, only when it was compiled with `debug` on, its
    /// code (`''` otherwise). Twig: `getSourceContext()` (`ModuleNode::compileGetSourceContext()`).
    pub fn source(&self) -> &Source {
        self.program.source()
    }

    /// A snapshot of `$this->macros` (what a compiled method copies into its local `$macros`).
    pub(crate) fn macros(&self) -> Array {
        lock(&self.macros).clone()
    }

    /// `$this->macros[$name] = $namespace` (an import in the main scope of the template).
    pub(crate) fn set_macro(&self, name: ArrayKey, namespace: Value) {
        lock(&self.macros).insert(name, namespace);
    }

    /// `$this->parent` when it is not `null`: `Some(None)` for `false`, `Some(Some(parent))` for a resolved constant
    /// parent.
    pub(crate) fn resolved_parent(&self) -> Option<Option<Arc<Template>>> {
        match &*lock(&self.parent) {
            ParentSlot::Unresolved => None,
            ParentSlot::None => Some(None),
            ParentSlot::Some(parent) => Some(Some(Arc::clone(parent))),
        }
    }

    /// `$this->parent = $parent`.
    pub(crate) fn set_parent(&self, parent: Arc<Template>) {
        *lock(&self.parent) = ParentSlot::Some(parent);
    }

    /// The template's own `$this->blocks` entry for `name` (one of its block methods or a trait block).
    pub(crate) fn own_block(self: &Arc<Self>, name: &[u8]) -> Option<BlockRef> {
        self.blocks
            .iter()
            .find(|(key, _)| key.as_bytes() == name)
            .map(|(_, slot)| self.resolve_slot(slot))
    }

    fn resolve_slot(self: &Arc<Self>, slot: &BlockSlot) -> BlockRef {
        match slot {
            BlockSlot::Own(unit) => BlockRef {
                template: Arc::clone(self),
                unit: *unit,
            },
            BlockSlot::Trait(block) => block.clone(),
        }
    }

    /// `$this->blocks` (Twig: `getBlocks()`).
    pub(crate) fn block_table(self: &Arc<Self>) -> Blocks {
        Blocks::from_entries(
            self.blocks
                .iter()
                .map(|(name, slot)| (name.clone(), self.resolve_slot(slot)))
                .collect(),
        )
    }

    /// The names of `$this->blocks` in order.
    pub(crate) fn own_block_names(&self) -> impl Iterator<Item = &PhpString> {
        self.blocks.iter().map(|(name, _)| name)
    }

    /// `$this->traits[$name]`.
    pub(crate) fn trait_block(&self, name: &[u8]) -> Option<&BlockRef> {
        self.traits
            .iter()
            .find(|(key, _)| key.as_bytes() == name)
            .map(|(_, block)| block)
    }

    /// `$this->traitAliases[$name]`.
    pub(crate) fn trait_alias(&self, name: &[u8]) -> Option<&PhpString> {
        self.trait_aliases
            .iter()
            .find(|(key, _)| key.as_bytes() == name)
            .map(|(_, original)| original)
    }
}

static TEMPLATE_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("Twig\\Template")
        .abstract_class()
        .build()
});

/// A template object on the operand stack: what the compiled code's `$this->load()` returns (`$_vN` of an
/// `ignore missing` include, the target of `->unwrap()->yield()` and of block calls). It never reaches a template's
/// variables.
pub(crate) struct TemplateObject {
    template: Arc<Template>,
}

impl TemplateObject {
    /// The template as a value.
    pub(crate) fn value(template: Arc<Template>) -> Value {
        Value::Object(ObjectRef::new(TemplateObject { template }))
    }

    /// The template of a value made by [`TemplateObject::value`].
    pub(crate) fn template_of(value: &Value) -> Option<Arc<Template>> {
        match value {
            Value::Object(object) => object
                .downcast_ref::<TemplateObject>()
                .map(|object| Arc::clone(&object.template)),
            _ => None,
        }
    }
}

impl Object for TemplateObject {
    fn class(&self) -> &ClassRef {
        &TEMPLATE_CLASS
    }
}

/// Twig: `Template::yield()`, the entry point of a render (see [`composition::yield_template`]).
pub(crate) fn yield_template(
    state: &mut VmState,
    out: &mut Output<'_>,
    template: &Arc<Template>,
    context: Array,
) -> std::result::Result<(), Exception> {
    composition::yield_template(state, out, template, context, &Blocks::default())
}

/// A template ready to render: what `Environment::load()` returns. Twig: `Twig\TemplateWrapper`
/// (TWIG/src/TemplateWrapper.php).
///
/// The context is a PHP array ([`twig_value::Array`]); keys are template variable names.
///
/// ```
/// use twig::loader::ArrayLoader;
/// use twig::runtime::Environment;
/// use twig::value::{Array, Value};
///
/// let env = Environment::new(ArrayLoader::new([("index", "Hello {{ name }}!{% block title %}T{% endblock %}")]));
/// let template = env.load("index")?;
/// let mut context = Array::new();
/// context.insert("name", Value::from("Fabien"));
/// assert_eq!(template.render(context)?, "Hello Fabien!T");
/// assert_eq!(template.render_block("title", Array::new())?, "T");
/// assert!(template.has_block("title", Array::new())?);
/// # Ok::<(), twig::Error>(())
/// ```
#[derive(Clone)]
pub struct TemplateWrapper {
    env: Environment,
    template: Arc<Template>,
}

impl std::fmt::Debug for TemplateWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateWrapper")
            .field("name", &self.template.name())
            .finish_non_exhaustive()
    }
}

impl TemplateWrapper {
    pub(crate) fn new(env: Environment, template: Arc<Template>) -> Self {
        TemplateWrapper { env, template }
    }

    /// The environment the template belongs to.
    pub fn environment(&self) -> &Environment {
        &self.env
    }

    /// The template instance.
    pub fn template(&self) -> &Arc<Template> {
        &self.template
    }

    /// The template name. Twig: `TemplateWrapper::getTemplateName()`.
    pub fn template_name(&self) -> &str {
        self.template.name()
    }

    /// The source of the compiled template ([`Template::source`]: the code only with `debug`). Twig:
    /// `TemplateWrapper::getSourceContext()`.
    pub fn source_context(&self) -> &Source {
        self.template.source()
    }

    /// The escaping strategy the template was compiled with, `None` for `false`. Twig:
    /// `TemplateWrapper::getDefaultEscapeStrategy()`.
    pub fn default_escape_strategy(&self) -> Option<&str> {
        self.template.program().default_escape_strategy()
    }

    /// The wrapper as a PHP object (`Twig\TemplateWrapper`), to pass it to templates: `{% include tpl %}`,
    /// `{% extends tpl %}`, `include(tpl)`, `tpl.render()` work with it as in Twig, and only in the environment that
    /// loaded it.
    pub fn to_value(&self) -> Value {
        Value::Object(ObjectRef::new(WrapperObject {
            wrapper: self.clone(),
        }))
    }

    /// The wrapper of a `Twig\TemplateWrapper` object made by [`TemplateWrapper::to_value`].
    pub fn from_value(value: &Value) -> Option<TemplateWrapper> {
        match value {
            Value::Object(object) => object
                .downcast_ref::<WrapperObject>()
                .map(|object| object.wrapper.clone()),
            _ => None,
        }
    }

    /// Twig: `TemplateWrapper::unwrap($env)`: the template, when the wrapper belongs to `env`.
    ///
    /// # Errors
    ///
    /// `RuntimeError('A "Twig\TemplateWrapper" can only be used with the "Twig\Environment" that created it.')`.
    pub(crate) fn unwrap_for(&self, env: &Environment) -> Result<Arc<Template>> {
        if !self.env.same_as(env) || self.template.env_id() != env.id() {
            return Err(Error::runtime(
                "A \"Twig\\TemplateWrapper\" can only be used with the \"Twig\\Environment\" that created it.",
            ));
        }
        Ok(Arc::clone(&self.template))
    }

    fn state(&self) -> VmState {
        VmState::new(self.env.clone())
    }

    /// `$context + $this->env->getGlobals()` (the block methods of `TemplateWrapper`).
    fn with_globals(&self, context: Array) -> Array {
        twig_value::ops::array_union(&context, &self.env.globals())
    }

    /// Renders the template to bytes (PHP strings are bytes). Twig: `TemplateWrapper::render()`.
    ///
    /// # Errors
    ///
    /// The Twig error the rendering raised; what was produced so far is discarded.
    pub fn render_bytes(&self, context: Array) -> Result<Vec<u8>> {
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut out = Output::new(&mut buffer, false);
            let mut state = self.state();
            yield_template(&mut state, &mut out, &self.template, context)
                .map_err(Exception::into_error)?;
        }
        Ok(buffer)
    }

    /// Renders the template to a string. Twig: `TemplateWrapper::render()`.
    ///
    /// # Errors
    ///
    /// The Twig error the rendering raised, or a runtime error when the output is not valid UTF-8 (use
    /// [`render_bytes`](Self::render_bytes) for byte output).
    pub fn render(&self, context: Array) -> Result<String> {
        let bytes = self.render_bytes(context)?;
        self.utf8(bytes)
    }

    fn utf8(&self, bytes: Vec<u8>) -> Result<String> {
        String::from_utf8(bytes).map_err(|_| {
            Error::runtime(format!(
                "The output of template \"{}\" is not valid UTF-8 (render it to bytes instead).",
                self.template_name()
            ))
        })
    }

    /// Displays the template: writes its output to `sink` as it is produced (`{% flush %}` flushes the sink).
    /// Twig: `TemplateWrapper::display()`.
    ///
    /// # Errors
    ///
    /// The Twig error the rendering raised (the output written before it stays written), or an error of the sink.
    pub fn display(&self, context: Array, sink: &mut dyn Sink) -> Result<()> {
        let mut out = Output::new(sink, true);
        let mut state = self.state();
        yield_template(&mut state, &mut out, &self.template, context).map_err(Exception::into_error)
    }

    /// Streams the template into `sink`, chunk by chunk as it is produced. Twig: `TemplateWrapper::stream()` (a
    /// generator in PHP; here the sink receives the chunks).
    ///
    /// # Errors
    ///
    /// As [`display`](Self::display).
    pub fn stream(&self, context: Array, sink: &mut dyn Sink) -> Result<()> {
        self.display(context, sink)
    }

    /// Renders one block of the template to bytes, with the globals added to the context. Twig:
    /// `TemplateWrapper::renderBlock()`.
    ///
    /// # Errors
    ///
    /// `Block "%s" on template "%s" does not exist.` and the errors of the block.
    pub fn render_block_bytes(&self, name: &str, context: Array) -> Result<Vec<u8>> {
        let mut buffer: Vec<u8> = Vec::new();
        {
            let mut out = Output::new(&mut buffer, false);
            self.yield_block_into(name, context, &mut out)?;
        }
        Ok(buffer)
    }

    /// Renders one block of the template to a string. Twig: `TemplateWrapper::renderBlock()`.
    ///
    /// # Errors
    ///
    /// As [`render_block_bytes`](Self::render_block_bytes), and non-UTF-8 output.
    pub fn render_block(&self, name: &str, context: Array) -> Result<String> {
        let bytes = self.render_block_bytes(name, context)?;
        self.utf8(bytes)
    }

    /// Displays one block of the template into `sink`. Twig: `TemplateWrapper::displayBlock()`.
    ///
    /// # Errors
    ///
    /// As [`render_block_bytes`](Self::render_block_bytes), and the errors of the sink.
    pub fn display_block(&self, name: &str, context: Array, sink: &mut dyn Sink) -> Result<()> {
        let mut out = Output::new(sink, true);
        self.yield_block_into(name, context, &mut out)
    }

    /// Streams one block of the template into `sink`. Twig: `TemplateWrapper::streamBlock()`.
    ///
    /// # Errors
    ///
    /// As [`display_block`](Self::display_block).
    pub fn stream_block(&self, name: &str, context: Array, sink: &mut dyn Sink) -> Result<()> {
        self.display_block(name, context, sink)
    }

    fn yield_block_into(&self, name: &str, context: Array, out: &mut Output<'_>) -> Result<()> {
        let context = self.with_globals(context);
        let mut state = self.state();
        composition::yield_block(
            &mut state,
            out,
            &self.template,
            &Value::from(name),
            context,
            Blocks::default(),
            true,
            None,
        )
        .map_err(Exception::into_error)
    }

    /// Whether the template, its traits or its parents define a block. Twig: `TemplateWrapper::hasBlock()`.
    ///
    /// # Errors
    ///
    /// The errors of resolving a dynamic parent.
    pub fn has_block(&self, name: &str, context: Array) -> Result<bool> {
        let context = self.with_globals(context);
        let mut state = self.state();
        composition::has_block(
            &mut state,
            &self.template,
            &Value::from(name),
            &context,
            &Blocks::default(),
        )
        .map_err(Exception::into_error)
    }

    /// The names of the blocks of the template, its traits and its parents, each once, in Twig's order. Twig:
    /// `TemplateWrapper::getBlockNames()`.
    ///
    /// # Errors
    ///
    /// The errors of resolving a dynamic parent.
    pub fn block_names(&self, context: Array) -> Result<Vec<String>> {
        let context = self.with_globals(context);
        let mut state = self.state();
        let names =
            composition::block_names(&mut state, &self.template, &context, &Blocks::default())
                .map_err(Exception::into_error)?;
        Ok(names
            .into_iter()
            .map(|(_, name)| name.to_string_lossy().into_owned())
            .collect())
    }
}

static WRAPPER_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    // TWIG/src/TemplateWrapper.php: `final class TemplateWrapper` with private `$env` and `$template`
    ClassBuilder::class("Twig\\TemplateWrapper")
        .final_class()
        .method("__construct")
        .method("stream")
        .method("streamBlock")
        .method("render")
        .method("display")
        .method("hasBlock")
        .method("getBlockNames")
        .method("renderBlock")
        .method("displayBlock")
        .method("getSourceContext")
        .method("getTemplateName")
        .method("getDefaultEscapeStrategy")
        .method("isOwnedBy")
        .method("unwrap")
        .build()
});

/// `Twig\TemplateWrapper` as a PHP object.
struct WrapperObject {
    wrapper: TemplateWrapper,
}

static SOURCE_CLASS: LazyLock<ClassRef> = LazyLock::new(|| {
    // TWIG/src/Source.php: `final class Source` with private `$code`, `$name` and `$path`
    ClassBuilder::class("Twig\\Source")
        .final_class()
        .method("__construct")
        .method("getCode")
        .method("getName")
        .method("getPath")
        .build()
});

/// `Twig\Source` as a PHP object: what a compiled template's `getSourceContext()` returns to templates.
struct SourceObject {
    code: PhpString,
    name: PhpString,
    path: PhpString,
}

impl SourceObject {
    /// The compiled `getSourceContext()` of a template (Twig: `ModuleNode::compileGetSourceContext()`): `new
    /// Source(<code if the template was compiled with debug on, else ''>, <name>, <path>)`.
    fn of(template: &Template) -> Value {
        let source = template.source();
        let code = if template.program().is_debug() {
            PhpString::from(source.code())
        } else {
            PhpString::empty()
        };
        Value::Object(ObjectRef::new(SourceObject {
            code,
            name: PhpString::from(source.name()),
            path: PhpString::from(source.path()),
        }))
    }
}

impl Object for SourceObject {
    fn class(&self) -> &ClassRef {
        &SOURCE_CLASS
    }

    /// Twig: `Source::getCode()`, `getName()`, `getPath()` (extra arguments are ignored, as for any userland method).
    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        _args: CallArgs,
        _diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "getcode" => Ok(Value::String(self.code.clone())),
            "getname" => Ok(Value::String(self.name.clone())),
            "getpath" => Ok(Value::String(self.path.clone())),
            "__construct" => Err(PhpError::new(
                twig_value::ThrowableClass::LogicException,
                "Calling Twig\\Source::__construct() from a template is not supported by twig-rs.",
            )),
            _ => Err(twig_value::object::undefined_method(&SOURCE_CLASS, method)),
        }
    }
}

/// The arguments of a `TemplateWrapper` method bound to its parameters with PHP's rules (named arguments,
/// defaults), each checked against its declared type in coercive typing mode.
fn bind_args(
    method: &str,
    args: &CallArgs,
    params: &[(Param, TypeMask)],
    diag: &mut dyn Diagnostics,
) -> PhpResult<Vec<Value>> {
    let function = format!("Twig\\TemplateWrapper::{method}");
    let declared: Vec<Param> = params.iter().map(|(param, _)| param.clone()).collect();
    let values = args.bind(&function, &declared, FunctionKind::User)?;
    values
        .into_iter()
        .zip(params)
        .enumerate()
        .map(|(i, (value, (param, mask)))| {
            verify_user_arg(
                value,
                &DeclaredType::builtin(*mask),
                &UserArg::new(&function, i as u32 + 1, param.name),
                diag,
            )
        })
        .collect()
}

fn array_of(value: Option<Value>) -> Array {
    match value {
        Some(Value::Array(array)) => array,
        _ => Array::new(),
    }
}

fn string_of(value: Option<Value>) -> PhpString {
    match value {
        Some(Value::String(string)) => string,
        _ => PhpString::empty(),
    }
}

impl WrapperObject {
    /// Runs `f` on the running render's state when the wrapper belongs to its environment, else on a fresh one.
    fn with_state<R>(
        &self,
        diag: &mut dyn Diagnostics,
        f: impl FnOnce(&mut VmState) -> std::result::Result<R, Exception>,
    ) -> PhpResult<R> {
        let host = diag
            .host()
            .and_then(|host| host.downcast_mut::<VmState>())
            .filter(|state| state.env.same_as(&self.wrapper.env));
        match host {
            Some(state) => f(state).map_err(Exception::into_php_error),
            None => {
                let mut state = self.wrapper.state();
                f(&mut state).map_err(Exception::into_php_error)
            }
        }
    }
}

impl Object for WrapperObject {
    fn class(&self) -> &ClassRef {
        &WRAPPER_CLASS
    }

    fn call(
        &self,
        _this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        let wrapper = &self.wrapper;
        match method {
            "gettemplatename" => Ok(Value::from(wrapper.template_name())),
            "getdefaultescapestrategy" => Ok(match wrapper.default_escape_strategy() {
                Some(strategy) => Value::from(strategy),
                None => Value::Bool(false),
            }),
            "render" => {
                let mut values = bind_args(
                    "render",
                    &args,
                    &[(Param::optional("context", Array::new()), TypeMask::ARRAY)],
                    diag,
                )?
                .into_iter();
                let context = array_of(values.next());
                let template = Arc::clone(&wrapper.template);
                let bytes = self.with_state(diag, |state| {
                    let mut buffer: Vec<u8> = Vec::new();
                    {
                        let mut out = Output::new(&mut buffer, false);
                        yield_template(state, &mut out, &template, context)?;
                    }
                    Ok(buffer)
                })?;
                Ok(Value::String(PhpString::from(bytes)))
            }
            "renderblock" => {
                let mut values = bind_args(
                    "renderBlock",
                    &args,
                    &[
                        (Param::required("name"), TypeMask::STRING),
                        (Param::optional("context", Array::new()), TypeMask::ARRAY),
                    ],
                    diag,
                )?
                .into_iter();
                let name = string_of(values.next());
                let context = array_of(values.next());
                let context = wrapper.with_globals(context);
                let template = Arc::clone(&wrapper.template);
                let rendered = self.with_state(diag, |state| {
                    composition::render_block(
                        state,
                        &mut Output::detached(),
                        &template,
                        &Value::String(name),
                        context,
                        Blocks::default(),
                        true,
                    )
                })?;
                Ok(Value::String(rendered))
            }
            "hasblock" => {
                let mut values = bind_args(
                    "hasBlock",
                    &args,
                    &[
                        (Param::required("name"), TypeMask::STRING),
                        (Param::optional("context", Array::new()), TypeMask::ARRAY),
                    ],
                    diag,
                )?
                .into_iter();
                let name = string_of(values.next());
                let context = array_of(values.next());
                let context = wrapper.with_globals(context);
                let template = Arc::clone(&wrapper.template);
                let has = self.with_state(diag, |state| {
                    composition::has_block(
                        state,
                        &template,
                        &Value::String(name),
                        &context,
                        &Blocks::default(),
                    )
                })?;
                Ok(Value::Bool(has))
            }
            "getblocknames" => {
                let mut values = bind_args(
                    "getBlockNames",
                    &args,
                    &[(Param::optional("context", Array::new()), TypeMask::ARRAY)],
                    diag,
                )?
                .into_iter();
                let context = array_of(values.next());
                let context = wrapper.with_globals(context);
                let template = Arc::clone(&wrapper.template);
                let names = self.with_state(diag, |state| {
                    composition::block_names(state, &template, &context, &Blocks::default())
                })?;
                let mut array = Array::new();
                for (index, name) in names {
                    array.insert(ArrayKey::Int(index), Value::String(name));
                }
                Ok(Value::Array(array))
            }
            // `TemplateWrapper::getSourceContext()`: `$this->template->getSourceContext()`
            "getsourcecontext" => Ok(SourceObject::of(&wrapper.template)),
            "isownedby" | "unwrap" | "display" | "displayblock" | "stream" | "streamblock"
            | "__construct" => Err(PhpError::new(
                twig_value::ThrowableClass::LogicException,
                format!(
                    "Calling Twig\\TemplateWrapper::{}() from a template is not supported by twig-rs.",
                    WRAPPER_CLASS
                        .method(method)
                        .map_or(method, |m| m.name.as_ref())
                ),
            )),
            _ => Err(twig_value::object::undefined_method(&WRAPPER_CLASS, method)),
        }
    }
}
