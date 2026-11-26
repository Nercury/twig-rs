//! `Twig\BlockChain` (TWIG/src/BlockChain.php, Twig 3.31): the blocks of several templates and of their whole
//! lineages composed into one set, without their bodies or macros.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use twig_value::types::get_debug_type;
use twig_value::{Array, PhpError, PhpString, ThrowableClass, Value, ops};

use super::composition;
use super::environment::Environment;
use super::template::{BlockRef, Blocks, Template, TemplateWrapper};
use crate::error::{Error, Result};
use crate::vm::{Exception, Output, Sink, VmState};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// A template or a nested chain, as [`BlockChain::new`] takes them: a template name, a [`TemplateWrapper`], a
/// [`BlockChain`] or a PHP value (a string or a `Twig\TemplateWrapper` object). Twig:
/// `iterable<string|TemplateWrapper|self> $templates`.
#[derive(Clone, Debug)]
pub enum BlockChainTemplate {
    /// A template name, loaded by the chain's environment.
    Name(String),
    /// A loaded template.
    Wrapper(TemplateWrapper),
    /// A nested chain, which keeps its own default context and its place in the precedence order.
    Chain(BlockChain),
    /// A PHP value: a string is a name, a `Twig\TemplateWrapper` object a wrapper; anything else is a `TypeError`.
    Value(Value),
}

impl From<&str> for BlockChainTemplate {
    fn from(name: &str) -> Self {
        BlockChainTemplate::Name(name.to_owned())
    }
}

impl From<String> for BlockChainTemplate {
    fn from(name: String) -> Self {
        BlockChainTemplate::Name(name)
    }
}

impl From<&String> for BlockChainTemplate {
    fn from(name: &String) -> Self {
        BlockChainTemplate::Name(name.clone())
    }
}

impl From<TemplateWrapper> for BlockChainTemplate {
    fn from(wrapper: TemplateWrapper) -> Self {
        BlockChainTemplate::Wrapper(wrapper)
    }
}

impl From<&TemplateWrapper> for BlockChainTemplate {
    fn from(wrapper: &TemplateWrapper) -> Self {
        BlockChainTemplate::Wrapper(wrapper.clone())
    }
}

impl From<BlockChain> for BlockChainTemplate {
    fn from(chain: BlockChain) -> Self {
        BlockChainTemplate::Chain(chain)
    }
}

impl From<&BlockChain> for BlockChainTemplate {
    fn from(chain: &BlockChain) -> Self {
        BlockChainTemplate::Chain(chain.clone())
    }
}

impl From<Value> for BlockChainTemplate {
    fn from(value: Value) -> Self {
        BlockChainTemplate::Value(value)
    }
}

/// A member of a chain or of a lineage: `Template|self`.
#[derive(Clone)]
enum Member {
    Template(Arc<Template>),
    Chain(BlockChain),
}

impl Member {
    /// `===`: the same object.
    fn same(&self, other: &Member) -> bool {
        match (self, other) {
            (Member::Template(a), Member::Template(b)) => Arc::ptr_eq(a, b),
            (Member::Chain(a), Member::Chain(b)) => Arc::ptr_eq(&a.inner, &b.inner),
            _ => false,
        }
    }
}

/// What a chain resolved last: `$this->lineage`, `$this->blocks` and `$this->fixed`.
#[derive(Default)]
struct State {
    lineage: Vec<Member>,
    blocks: Blocks,
    /// Whether the lineage can no longer move, making every further resolution pointless.
    fixed: bool,
}

struct Inner {
    env: Environment,
    /// `$this->templates`: templates and nested chains, from the highest precedence to the lowest.
    templates: Vec<Member>,
    /// `$this->context`: the default variables of the dynamic parent expressions.
    context: Array,
    state: Mutex<State>,
}

/// The blocks of several templates and of their whole lineages (their parents, their traits) composed into one set,
/// without composing their bodies or their macros: what a form theme needs, for instance. Twig: `Twig\BlockChain`
/// (TWIG/src/BlockChain.php, Twig 3.31).
///
/// The templates are given from the highest precedence to the lowest; a block comes from the first template of the
/// precedence order that defines it, its own lineage included (a template, then its parent, ...). A rendered block
/// runs as a method of the template that defines it (its macros, its sandbox checks, its errors are that template's),
/// and the blocks it renders with `block()` (and `parent()`) are looked up in the chain's set. Dynamic parents
/// (`{% extends layout %}`) are resolved with the render context, over the chain's default context, over the globals;
/// once every lineage is fixed (constant parents only), the set is not computed again.
///
/// A chain is a handle: clones share the chain, and a chain given to another one is the same chain.
///
/// ```
/// use twig::runtime::BlockChain;
/// use twig::{Array, Environment, context, loader::ArrayLoader};
///
/// let env = Environment::new(ArrayLoader::new([
///     ("theme", "{% extends 'base' %}{% block label %}<b>{{ parent() }}</b>{% endblock %}"),
///     ("base", "{% block label %}{{ text }}{% endblock %}{% block row %}{{ block('label') }}!{% endblock %}"),
/// ]));
/// let chain = BlockChain::new(&env, ["theme"], Array::new())?;
/// assert_eq!(chain.block_names(Array::new())?, ["label", "row"]);
/// assert_eq!(chain.render_block("row", context! { text => "Name" })?, "<b>Name</b>!");
/// # Ok::<(), twig::Error>(())
/// ```
#[derive(Clone)]
pub struct BlockChain {
    inner: Arc<Inner>,
}

impl fmt::Debug for BlockChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockChain")
            .field("entry_template", &self.entry_template().name())
            .finish_non_exhaustive()
    }
}

impl BlockChain {
    /// `new BlockChain($env, $templates, $context)`: a chain of `templates` (from the highest precedence to the
    /// lowest; names are loaded at once) whose dynamic parents are resolved with `context` under the render context.
    ///
    /// # Errors
    ///
    /// The errors of loading a template name; the `LogicException`s `A block chain cannot contain a block chain from
    /// a different Twig environment.` and `A block chain cannot contain templates from different Twig environments.`;
    /// the `TypeError` `Block chain templates must be strings, "Twig\TemplateWrapper" or "Twig\BlockChain"
    /// instances, "%s" given.` for another PHP value; the `InvalidArgumentException` `A block chain requires at least
    /// one template.`
    pub fn new<T: Into<BlockChainTemplate>>(
        env: &Environment,
        templates: impl IntoIterator<Item = T>,
        context: Array,
    ) -> Result<BlockChain> {
        let mut members = Vec::new();
        for template in templates {
            let wrapper = match template.into() {
                BlockChainTemplate::Chain(chain) => {
                    if !chain.inner.env.same_as(env) {
                        return Err(Error::logic(
                            "A block chain cannot contain a block chain from a different Twig environment.",
                        ));
                    }
                    members.push(Member::Chain(chain));
                    continue;
                }
                BlockChainTemplate::Name(name) => env.load(&name)?,
                BlockChainTemplate::Wrapper(wrapper) => wrapper,
                BlockChainTemplate::Value(Value::String(name)) => {
                    env.load(&*name.to_string_lossy())?
                }
                BlockChainTemplate::Value(value) => match TemplateWrapper::from_value(&value) {
                    Some(wrapper) => wrapper,
                    None => {
                        return Err(Error::php(PhpError::type_error(format!(
                            "Block chain templates must be strings, \"Twig\\TemplateWrapper\" or \"Twig\\BlockChain\" instances, \"{}\" given.",
                            get_debug_type(&value)
                        ))));
                    }
                },
            };
            if !wrapper.is_owned_by(env) {
                return Err(Error::logic(
                    "A block chain cannot contain templates from different Twig environments.",
                ));
            }
            members.push(Member::Template(wrapper.unwrap(Some(env))?));
        }
        if members.is_empty() {
            return Err(Error::php(PhpError::new(
                ThrowableClass::InvalidArgumentException,
                "A block chain requires at least one template.",
            )));
        }
        Ok(BlockChain {
            inner: Arc::new(Inner {
                env: env.clone(),
                templates: members,
                context,
                state: Mutex::new(State::default()),
            }),
        })
    }

    /// The environment of the chain.
    pub fn environment(&self) -> &Environment {
        &self.inner.env
    }

    /// `$context + $this->context + $this->env->getGlobals()`.
    fn full_context(&self, context: Array) -> Array {
        let context = ops::array_union(&context, &self.inner.context);
        ops::array_union(&context, &self.inner.env.globals())
    }

    /// `$this->fixed ? $this->blocks : $this->resolveBlocks($context)`.
    fn blocks(
        &self,
        state: &mut VmState,
        context: &Array,
    ) -> std::result::Result<Blocks, Exception> {
        {
            let current = lock(&self.inner.state);
            if current.fixed {
                return Ok(current.blocks.clone());
            }
        }
        self.resolve_blocks(state, context)
    }

    /// Twig: `BlockChain::resolveBlocks()`: the lineage for `context`; nested chains that can still move resolve
    /// their own blocks again (with their default context under `context`); when the lineage changed, the set is
    /// composed again: a block comes from the first template of the lineage defining it, a nested chain adds the
    /// blocks it does not override (`$blocks += $chain->blocks`).
    fn resolve_blocks(
        &self,
        state: &mut VmState,
        context: &Array,
    ) -> std::result::Result<Blocks, Exception> {
        let (lineage, mut fixed) = self.resolve_lineage(state, context)?;
        let mut stale = {
            let current = lock(&self.inner.state);
            current.lineage.len() != lineage.len()
                || current
                    .lineage
                    .iter()
                    .zip(&lineage)
                    .any(|(a, b)| !a.same(b))
        };
        for member in &lineage {
            let Member::Chain(chain) = member else {
                continue;
            };
            if !lock(&chain.inner.state).fixed {
                // a nested chain that can still move may resolve to other blocks on every call
                chain.resolve_blocks(state, &ops::array_union(context, &chain.inner.context))?;
                stale = true;
            }
            fixed = fixed && lock(&chain.inner.state).fixed;
        }
        let mut current = lock(&self.inner.state);
        current.fixed = fixed;
        if !stale {
            return Ok(current.blocks.clone());
        }
        let mut blocks: Vec<(PhpString, BlockRef)> = Vec::new();
        let mut add = |name: &PhpString, block: &BlockRef| {
            if !blocks.iter().any(|(key, _)| key == name) {
                blocks.push((name.clone(), block.clone()));
            }
        };
        for member in &lineage {
            match member {
                Member::Chain(chain) => {
                    let nested = lock(&chain.inner.state).blocks.clone();
                    for (name, block) in nested.entries() {
                        add(name, block);
                    }
                }
                Member::Template(template) => {
                    for (name, block) in template.block_table().entries() {
                        add(name, block);
                    }
                }
            }
        }
        current.lineage = lineage;
        current.blocks = Blocks::from_entries(blocks);
        Ok(current.blocks.clone())
    }

    /// Twig: `BlockChain::resolveLineage()`: each template followed by its parents (resolved with `context`), each
    /// nested chain as it is, and whether no parent can move any more (`hasFixedParent()` of every template).
    fn resolve_lineage(
        &self,
        state: &mut VmState,
        context: &Array,
    ) -> std::result::Result<(Vec<Member>, bool), Exception> {
        let mut lineage = Vec::new();
        let mut fixed = true;
        for member in &self.inner.templates {
            let Member::Template(first) = member else {
                lineage.push(member.clone());
                continue;
            };
            let mut seen: Vec<Arc<Template>> = Vec::new();
            let mut template = Arc::clone(first);
            loop {
                if seen.iter().any(|seen| Arc::ptr_eq(seen, &template)) {
                    return Err(Error::logic(format!(
                        "Circular template inheritance detected while building a block chain from \"{}\".",
                        template.name()
                    ))
                    .into());
                }
                seen.push(Arc::clone(&template));
                lineage.push(Member::Template(Arc::clone(&template)));
                let parent = composition::get_parent(state, &template, context)?;
                fixed = fixed && template.resolved_parent().is_some();
                match parent {
                    Some(parent) => template = parent,
                    None => break,
                }
            }
        }
        Ok((lineage, fixed))
    }

    /// Twig: `BlockChain::entryTemplate()`: the first template, through nested chains.
    fn entry_template(&self) -> Arc<Template> {
        match &self.inner.templates[0] {
            Member::Template(template) => Arc::clone(template),
            Member::Chain(chain) => chain.entry_template(),
        }
    }

    /// Twig: `BlockChain::throwUnknownBlock()`.
    fn unknown_block(&self, name: &str) -> Exception {
        let entry = self.entry_template();
        Error::runtime(format!(
            "Block \"{name}\" on template \"{}\" does not exist.",
            entry.name()
        ))
        .with_source(entry.source().clone())
        .into()
    }

    fn state(&self) -> VmState {
        VmState::new(self.inner.env.clone())
    }

    /// Whether the chain has the block `name` for `context`. Twig: `BlockChain::hasBlock()`.
    ///
    /// # Errors
    ///
    /// The errors of resolving a lineage (a dynamic parent, a circular inheritance).
    pub fn has_block(&self, name: &str, context: impl Into<Array>) -> Result<bool> {
        let context = self.full_context(context.into());
        let mut state = self.state();
        let blocks = self
            .blocks(&mut state, &context)
            .map_err(Exception::into_error)?;
        Ok(blocks.get(name.as_bytes()).is_some())
    }

    /// The names of the blocks of the chain for `context`, in the order of the set (the precedence order). Twig:
    /// `BlockChain::getBlockNames()`.
    ///
    /// # Errors
    ///
    /// As [`has_block`](Self::has_block).
    pub fn block_names(&self, context: impl Into<Array>) -> Result<Vec<String>> {
        let context = self.full_context(context.into());
        let mut state = self.state();
        let blocks = self
            .blocks(&mut state, &context)
            .map_err(Exception::into_error)?;
        Ok(blocks
            .names()
            .map(|name| name.to_string_lossy().into_owned())
            .collect())
    }

    /// The blocks for `context`, `name` checked: `$blocks = ...; if (!isset($blocks[$name])) { throwUnknownBlock() }`.
    fn blocks_with(
        &self,
        state: &mut VmState,
        name: &str,
        context: &Array,
    ) -> std::result::Result<Blocks, Exception> {
        let blocks = self.blocks(state, context)?;
        if blocks.get(name.as_bytes()).is_none() {
            return Err(self.unknown_block(name));
        }
        Ok(blocks)
    }

    /// Renders the block `name` of the chain to bytes. Twig: `BlockChain::renderBlock()`.
    ///
    /// # Errors
    ///
    /// `Block "%s" on template "%s" does not exist.` (with the first template of the chain), the errors of resolving
    /// the lineage and those of the block.
    pub fn render_block_bytes(&self, name: &str, context: impl Into<Array>) -> Result<Vec<u8>> {
        let context = self.full_context(context.into());
        let mut state = self.state();
        let rendered = self
            .blocks_with(&mut state, name, &context)
            .and_then(|blocks| {
                composition::render_block(
                    &mut state,
                    &mut Output::detached(),
                    &self.entry_template(),
                    &Value::from(name),
                    context,
                    blocks,
                    true,
                )
            })
            .map_err(Exception::into_error)?;
        Ok(rendered.as_bytes().to_vec())
    }

    /// Renders the block `name` of the chain to a string. Twig: `BlockChain::renderBlock()`.
    ///
    /// # Errors
    ///
    /// As [`render_block_bytes`](Self::render_block_bytes), and output that is not valid UTF-8.
    pub fn render_block(&self, name: &str, context: impl Into<Array>) -> Result<String> {
        let bytes = self.render_block_bytes(name, context)?;
        String::from_utf8(bytes).map_err(|_| {
            Error::runtime(format!(
                "The output of template \"{}\" is not valid UTF-8 (render it to bytes instead).",
                self.entry_template().name()
            ))
        })
    }

    /// Displays the block `name` of the chain into `sink`. Twig: `BlockChain::displayBlock()`.
    ///
    /// # Errors
    ///
    /// As [`render_block_bytes`](Self::render_block_bytes), and the errors of the sink.
    pub fn display_block(
        &self,
        name: &str,
        context: impl Into<Array>,
        sink: &mut dyn Sink,
    ) -> Result<()> {
        let context = self.full_context(context.into());
        let mut state = self.state();
        let blocks = self
            .blocks_with(&mut state, name, &context)
            .map_err(Exception::into_error)?;
        let mut out = Output::new(sink, true);
        composition::yield_block(
            &mut state,
            &mut out,
            &self.entry_template(),
            &Value::from(name),
            context,
            blocks,
            true,
            None,
        )
        .map_err(Exception::into_error)
    }

    /// Streams the block `name` of the chain into `sink`, chunk by chunk as it is produced. Twig:
    /// `BlockChain::streamBlock()` (a generator in PHP).
    ///
    /// # Errors
    ///
    /// As [`display_block`](Self::display_block).
    pub fn stream_block(
        &self,
        name: &str,
        context: impl Into<Array>,
        sink: &mut dyn Sink,
    ) -> Result<()> {
        self.display_block(name, context, sink)
    }
}
