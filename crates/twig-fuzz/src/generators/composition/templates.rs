//! The template-set generator behind [`super::case`].
//!
//! Templates are generated in rank order: a template only refers to templates generated before it
//! (`extends`, `use`, `embed`, `include`, `import`, `from`, `block(name, template)`), so a set never
//! recurses through templates except in the bounded recursion patterns (a macro calling itself with
//! a decreasing counter, a partial including itself with a depth guard). Block bodies only contain
//! or call blocks of a lower rank ([`BLOCKS`]), blocks of rank 0 never call macros (macros may call
//! `block('a')`), and a macro only calls the macros of its template defined before it: rendering
//! terminates.
//!
//! Knowledge about the generated templates (their blocks, macros and signatures, whether they can be
//! used as traits) steers later templates towards meaningful overrides and calls, with a small share
//! of deliberate mistakes (missing templates, blocks and macros, wrong argument lists, content
//! outside blocks, blocks under tags in child templates, ...).

use super::rng::Rng;
use crate::kit::json::Json;

/// Block names in rank order.
const BLOCKS: &[&str] = &["a", "b", "c", "d", "e", "content"];
/// Names no template has.
const MISSING: &[&str] = &["missing.twig", "nope.twig"];
const LAYOUT_NAMES: &[&str] = &["base.twig", "mid.twig", "top.twig"];
const PARTIAL_NAMES: &[&str] = &["inc.twig", "part.twig", "row.twig"];
const MACRO_LIB_NAMES: &[&str] = &["m.twig", "lib.twig"];
const TRAIT_NAMES: &[&str] = &["t.twig", "t2.twig"];
/// Unusual template names: directories, extensions the `name` escaping strategy reads, a numeric
/// name (an integer key of the loader's array in PHP), a space, upper case.
const ODD_NAMES: &[&str] = &[
    "sub/x.twig",
    "x.html",
    "x.js.twig",
    "x.txt",
    "0",
    "x y.twig",
    "X.twig",
    "x.css.twig",
];
const MACRO_NAMES: &[&str] = &["box", "row", "item", "wrap", "pair", "show", "tag", "cell"];
/// Macro names that are also function names (a `from` import shadows the function).
const FUNCTION_LIKE_MACROS: &[&str] = &["range", "include", "max", "source", "cycle"];
const PARAM_NAMES: &[&str] = &["x", "y", "label", "items", "a", "v"];
/// Parameter names PHP reserves in the compiled closure (Twig renames them there).
const RESERVED_PARAMS: &[&str] = &["context", "macros", "blocks", "this"];
const ALIASES: &[&str] = &["m", "lib", "forms", "h", "me"];
const TEXTS: &[&str] = &[
    "x", "-", " ", "\n", "<i>", "&amp;", "txt", "  \n  ", "|", "A", "\n\n", "é",
];

fn block_rank(name: &str) -> usize {
    BLOCKS
        .iter()
        .position(|b| *b == name)
        .unwrap_or(BLOCKS.len())
}

/// What a template is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// Blocks and placeholders; may extend a lower layout.
    Layout,
    /// Included content; may extend a layout.
    Partial,
    /// Macro definitions.
    Macros,
    /// Blocks only: used with `{% use %}`.
    Trait,
    /// The rendered template.
    Main,
}

/// A macro parameter.
#[derive(Clone, Debug)]
struct Param {
    name: String,
    default: Option<String>,
}

/// A macro signature.
#[derive(Clone, Debug)]
struct MacroSig {
    name: String,
    params: Vec<Param>,
    /// The explicit variadic argument (`...rest`); without it extra arguments go to `varargs`.
    variadic: Option<String>,
    /// `rec(n, sep)`: calls itself while `n > 0`.
    recursive: bool,
}

/// A generated template.
#[derive(Clone, Debug)]
struct Tpl {
    name: String,
    kind: Kind,
    source: String,
    /// The block names of its block table (own, nested, traits, parents).
    blocks: Vec<String>,
    macros: Vec<MacroSig>,
    /// Macros of its parent templates whose names it does not define itself (found by
    /// `MacroNamespace::call()` through `getParent($context)`).
    inherited_macros: Vec<MacroSig>,
    traitable: bool,
}

/// Where an import points.
#[derive(Clone, Debug)]
enum Target {
    /// A generated template (index into [`Gen::tpls`]).
    Tpl(usize),
    /// `_self`.
    Itself,
    /// `template_from_string()` of a template defining `hi(x)`.
    FromString,
    /// A missing template or a run-time choice: no known macros.
    Unknown,
}

#[derive(Clone, Debug)]
enum Import {
    /// `{% import X as alias %}`.
    Namespace { alias: String, target: Target },
    /// `{% from X import name as alias %}`.
    Function {
        alias: String,
        target: Target,
        name: String,
    },
}

/// A macro that can be called from the current scope.
#[derive(Clone, Debug)]
struct Callee {
    /// The call expression up to the argument list (`m.box`, `box`, `_self.box`).
    head: String,
    sig: MacroSig,
}

/// The template being written.
struct Doc {
    name: String,
    kind: Kind,
    out: String,
    /// Blocks of the parent templates: overridable, `parent()` works in them.
    parent_blocks: Vec<String>,
    /// Blocks of the used traits (after aliasing).
    trait_blocks: Vec<String>,
    /// Blocks defined in this template (or embed) so far.
    own_blocks: Vec<String>,
    /// Macros of this template, planned before the body is written.
    macros: Vec<MacroSig>,
    /// Macros already written.
    written_macros: usize,
    /// Macros of the parent templates (see [`Tpl::inherited_macros`]).
    parent_macros: Vec<MacroSig>,
    /// Imports of the main scope.
    main_imports: Vec<Import>,
    /// Statements left.
    budget: i32,
    traitable: bool,
    /// An embedded template (`{% embed %}` body).
    embedded: bool,
}

impl Doc {
    fn new(name: &str, kind: Kind, budget: i32) -> Doc {
        Doc {
            name: name.to_owned(),
            kind,
            out: String::new(),
            parent_blocks: Vec::new(),
            trait_blocks: Vec::new(),
            own_blocks: Vec::new(),
            macros: Vec::new(),
            written_macros: 0,
            parent_macros: Vec::new(),
            main_imports: Vec::new(),
            budget,
            traitable: false,
            embedded: false,
        }
    }

    fn inherits(&self, block: &str) -> bool {
        self.parent_blocks.iter().any(|b| b == block)
            || self.trait_blocks.iter().any(|b| b == block)
    }

    fn has_inheritance(&self) -> bool {
        !self.parent_blocks.is_empty() || !self.trait_blocks.is_empty()
    }

    /// The names of the block table.
    fn all_blocks(&self) -> Vec<String> {
        let mut all: Vec<String> = Vec::new();
        for b in self
            .own_blocks
            .iter()
            .chain(&self.trait_blocks)
            .chain(&self.parent_blocks)
        {
            if !all.contains(b) {
                all.push(b.clone());
            }
        }
        all
    }
}

/// What the generator knows about the place a statement is written to.
#[derive(Clone, Debug, Default)]
struct Scope {
    /// The enclosing block.
    block: Option<String>,
    /// The enclosing macro (index into [`Doc::macros`]).
    in_macro: Option<usize>,
    /// Imports of the local scope (a block or a macro); the root uses [`Doc::main_imports`].
    imports: Vec<Import>,
    /// Inside a block or a macro (imports are local there).
    local: bool,
    /// Variables known to be defined.
    vars: Vec<String>,
    /// Data variables are visible (not in macros, not after `only`).
    data: bool,
    depth: u32,
    in_loop: bool,
    /// The root of a child template or of an embed: blocks and configuration tags only.
    child_root: bool,
    /// The rank limit of `block(name, _self)`: `_self` names the enclosing template even inside
    /// an embed, so its own blocks keep the limit of the enclosing block (`None`: no limit).
    self_limit: Option<usize>,
}

impl Scope {
    /// The rank limit of `block(name, _self)` here.
    fn self_block_limit(&self) -> usize {
        let block = self.block.as_deref().map_or(BLOCKS.len(), block_rank);
        let block = if self.in_macro.is_some() { 1 } else { block };
        block.min(self.self_limit.unwrap_or(BLOCKS.len()))
    }
}

/// The result: templates, data and settings of one case.
pub(super) struct Plan {
    /// Templates in loading order.
    pub templates: Vec<(String, String)>,
    pub data: Vec<(String, Json)>,
    /// Data built by PHP code: `(key, PHP expression)` (`$twig` is in scope).
    pub php: Vec<(String, String)>,
    /// The templates the PHP code loads (`$twig->load()`), in order: the oracle's `preload`.
    pub preload: Vec<String>,
    pub globals: Vec<(String, Json)>,
    pub strict: bool,
    /// `autoescape` (`None`: Twig's default, `html`).
    pub autoescape: Option<Json>,
    pub optimizations: Option<i64>,
    pub debug: bool,
    pub use_yield: bool,
    pub string_loader: bool,
    pub sandbox: Option<Json>,
    pub load_all: bool,
    /// `"errors": "throw"`: PHP warnings become `ErrorException`s (with a template and a line).
    pub throw: bool,
}

/// Statements.
#[derive(Clone, Copy, Debug)]
enum St {
    Text,
    Print,
    Set,
    Capture,
    If,
    For,
    With,
    Apply,
    Autoescape,
    Include,
    IncludeFn,
    Embed,
    BlockDef,
    BlockCall,
    Parent,
    MacroCall,
    Import,
    ContextKeys,
    Source,
    Do,
    Wrapper,
    Sandbox,
    Throw,
    Deprecated,
    Special,
    Flush,
    Mistake,
}

/// Expressions.
#[derive(Clone, Copy, Debug)]
enum Ex {
    Var,
    Literal,
    SelfName,
    Block,
    Parent,
    Include,
    Macro,
    Source,
    Concat,
    Ternary,
    Default,
    Keys,
    Defined,
    Wrapper,
    FromString,
}

/// The generator of one case.
pub(super) struct Gen {
    rng: Rng,
    tpls: Vec<Tpl>,
    data: Vec<(String, Json)>,
    php: Vec<(String, String)>,
    /// The templates `php` loads, in order (each once).
    preload: Vec<String>,
    globals: Vec<(String, Json)>,
    string_loader: bool,
    sandbox: bool,
    /// Counter for generated variable names.
    counter: usize,
    /// Variables set anywhere so far (`set` targets), for "is it visible here" references.
    set_vars: Vec<String>,
    /// Template names used so far (unique).
    names: Vec<String>,
    /// Nesting limit of statements (larger in "big" cases).
    max_depth: u32,
    /// The `autoescape` option (chosen first: the `name` strategy makes odd names likelier).
    autoescape: Option<Json>,
}

impl Gen {
    /// A generator for the case of `seed`.
    pub(super) fn new(seed: u64) -> Gen {
        Gen {
            rng: Rng::new(seed),
            tpls: Vec::new(),
            data: Vec::new(),
            php: Vec::new(),
            preload: Vec::new(),
            globals: Vec::new(),
            string_loader: false,
            sandbox: false,
            counter: 0,
            set_vars: Vec::new(),
            names: Vec::new(),
            max_depth: 4,
            autoescape: None,
        }
    }

    /// Generates the case.
    pub(super) fn plan(mut self) -> Plan {
        self.string_loader = self.rng.chance(30);
        self.sandbox = self.rng.chance(10);
        self.autoescape = match self.rng.below(20) {
            0..=10 => None,
            11..=16 => Some(Json::Bool(false)),
            17 => Some(Json::str("js")),
            _ => Some(Json::str("name")),
        };
        let big = self.rng.chance(10);
        if big {
            self.max_depth = 6;
        }
        self.base_data();
        for (kind, name) in self.choose_templates() {
            let tpl = self.template(kind, &name);
            self.tpls.push(tpl);
            if self.tpls.len() == 1 && self.rng.chance(15) {
                let first = self.tpls[0].name.clone();
                self.globals.push(("gtpl".to_owned(), Json::str(first)));
            }
        }
        let mut templates: Vec<(String, String)> = Vec::new();
        for t in &self.tpls {
            let source = match self.rng.below(40) {
                0 => t.source.replace('\n', "\r\n"),
                1 => t.source.replace('\n', "\r"),
                _ => t.source.clone(),
            };
            templates.push((t.name.clone(), source));
        }
        // loading order: the main template first, last, or anywhere
        match self.rng.below(3) {
            0 => {
                let main = templates.pop().expect("the main template");
                templates.insert(0, main);
            }
            1 => self.rng.shuffle(&mut templates),
            _ => {}
        }
        let strict = self.rng.chance(35);
        let autoescape = self.autoescape.clone();
        let optimizations = self.rng.chance(15).then_some(0);
        let debug = self.rng.chance(10);
        let use_yield = self.rng.chance(15);
        let load_all = self.rng.chance(50);
        let throw = self.rng.chance(10);
        let sandbox = self.sandbox.then(|| self.sandbox_settings());
        Plan {
            templates,
            data: self.data,
            php: self.php,
            preload: self.preload,
            globals: self.globals,
            strict,
            autoescape,
            optimizations,
            debug,
            use_yield,
            string_loader: self.string_loader,
            sandbox,
            load_all,
            throw,
        }
    }

    // ---- data -----------------------------------------------------------------------------

    fn base_data(&mut self) {
        let strings = ["<b>S</b>", "plain", "a&b \"q\" 'r'", "", "0", "multi\nline"];
        let entries: Vec<(&str, Json)> = vec![
            ("s", Json::str(*self.rng.pick(&strings))),
            ("n", Json::int(*self.rng.pick(&[2, 0, 3, -1, 1]))),
            (
                "f",
                Json::Number((*self.rng.pick(&["1.5", "0.1", "-2.0", "1e3"])).to_owned()),
            ),
            ("yes", Json::Bool(true)),
            ("no", Json::Bool(false)),
            ("nul", Json::Null),
            (
                "list",
                Json::Array(vec![Json::int(1), Json::int(2), Json::str("three")]),
            ),
            (
                "map",
                Json::Object(vec![
                    ("a".to_owned(), Json::int(1)),
                    ("b".to_owned(), Json::str("B<")),
                ]),
            ),
            (
                "deep",
                Json::Object(vec![(
                    "k".to_owned(),
                    Json::Object(vec![("k2".to_owned(), Json::str("v"))]),
                )]),
            ),
            (
                "num",
                Json::Object(vec![
                    ("5".to_owned(), Json::str("five")),
                    ("x".to_owned(), Json::str("ex")),
                ]),
            ),
        ];
        for (key, value) in entries {
            self.data.push((key.to_owned(), value));
        }
        if self.rng.chance(70) {
            self.globals.push(("g".to_owned(), Json::str("G")));
            if self.rng.chance(30) {
                // shadowed by the data
                self.globals.push(("s".to_owned(), Json::str("global-s")));
            }
            if self.rng.chance(30) {
                self.globals.push((
                    "gl".to_owned(),
                    Json::Array(vec![Json::int(1), Json::str("<g>")]),
                ));
            }
            if self.rng.chance(15) {
                // a global that is also a macro argument name
                self.globals.push(("x".to_owned(), Json::str("gx")));
            }
            if self.rng.chance(8) {
                // a numeric name: an integer key of the globals array
                self.globals.push(("7".to_owned(), Json::str("g7")));
            }
        }
        if self.rng.chance(8) {
            // integer keys of the context (`-1` too; `01` stays a string)
            let key = *self.rng.pick(&["5", "0", "-1", "01"]);
            self.data
                .push((key.to_owned(), Json::str(format!("num-{key}"))));
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    /// A data variable holding `value`.
    fn data_var(&mut self, prefix: &str, value: Json) -> String {
        let name = self.fresh(prefix);
        self.data.push((name.clone(), value));
        name
    }

    /// A `Twig\Markup` holding `content`, built by the data code.
    fn markup_var(&mut self, content: &str) -> String {
        let var = self.fresh("mk");
        self.php.push((
            var.clone(),
            format!("new \\Twig\\Markup({}, 'UTF-8')", php_string(content)),
        ));
        var
    }

    /// A `Twig\TemplateWrapper` of `name` built by the data code (`$twig->load()`). The case
    /// preloads `name` (the oracle's `preload`): a template that fails to load then fails the load
    /// phase in both runners, instead of stopping the oracle's data phase without the typed data
    /// twig-rs needs.
    fn wrapper_var(&mut self, name: &str) -> String {
        let var = self.fresh("w");
        self.php
            .push((var.clone(), format!("$twig->load({})", php_string(name))));
        if !self.preload.iter().any(|known| known == name) {
            self.preload.push(name.to_owned());
        }
        var
    }

    /// Data variables that print (the objects of `data_php` are left out).
    fn data_keys(&self) -> Vec<String> {
        self.data.iter().map(|(k, _)| k.clone()).collect()
    }

    /// The sandbox: a policy allowing what the templates use, now and then minus one name (a
    /// security error where that name is used), globally or only for `{% sandbox %}` and
    /// `include(..., sandboxed = true)`.
    fn sandbox_settings(&mut self) -> Json {
        let sources: Vec<&str> = self.tpls.iter().map(|t| t.source.as_str()).collect();
        let (mut tags, mut filters, mut functions) = used_names(&sources);
        if self.rng.chance(40) {
            let lists: [&mut Vec<String>; 3] = [&mut tags, &mut filters, &mut functions];
            let which = self.rng.below(3);
            let list = lists.into_iter().nth(which).expect("three lists");
            if !list.is_empty() {
                let i = self.rng.below(list.len());
                list.remove(i);
            }
        }
        let names = |list: Vec<String>| Json::Array(list.into_iter().map(Json::str).collect());
        let mut policy = vec![
            ("tags".to_owned(), names(tags)),
            ("filters".to_owned(), names(filters)),
            ("functions".to_owned(), names(functions)),
        ];
        if self.rng.chance(60) {
            let methods = [
                "render",
                "renderBlock",
                "hasBlock",
                "getBlockNames",
                "getTemplateName",
                "getSourceContext",
            ];
            policy.push((
                "methods".to_owned(),
                Json::Object(vec![(
                    "Twig\\TemplateWrapper".to_owned(),
                    Json::Array(methods.iter().map(|m| Json::str(*m)).collect()),
                )]),
            ));
        }
        Json::Object(vec![
            ("global".to_owned(), Json::Bool(self.rng.chance(50))),
            ("policy".to_owned(), Json::Object(policy)),
        ])
    }

    // ---- the template set -----------------------------------------------------------------

    fn choose_templates(&mut self) -> Vec<(Kind, String)> {
        // (layouts, partials, macro libraries, traits)
        let counts = match self.rng.below(6) {
            0 => (
                self.rng.range(1, 3),
                self.rng.range(0, 1),
                self.rng.range(0, 1),
                self.rng.range(0, 1),
            ),
            1 => (
                self.rng.range(0, 2),
                self.rng.range(0, 1),
                0,
                self.rng.range(1, 2),
            ),
            2 => (
                self.rng.range(1, 2),
                self.rng.range(0, 2),
                self.rng.range(0, 1),
                0,
            ),
            3 => (
                self.rng.range(0, 1),
                self.rng.range(1, 3),
                self.rng.range(0, 1),
                0,
            ),
            4 => (0, self.rng.range(0, 1), self.rng.range(1, 2), 0),
            _ => (
                self.rng.range(0, 2),
                self.rng.range(0, 2),
                self.rng.range(0, 2),
                self.rng.range(0, 2),
            ),
        };
        let (layouts, partials, libs, traits) = counts;
        let mut middle: Vec<Kind> = Vec::new();
        middle.extend(std::iter::repeat_n(Kind::Partial, partials));
        middle.extend(std::iter::repeat_n(Kind::Macros, libs));
        middle.extend(std::iter::repeat_n(Kind::Layout, layouts));
        self.rng.shuffle(&mut middle);
        let mut kinds: Vec<Kind> = std::iter::repeat_n(Kind::Trait, traits).collect();
        kinds.extend(middle);
        let mut used = [0usize; 4];
        let mut out = Vec::new();
        for kind in kinds {
            let (pool, slot) = match kind {
                Kind::Layout => (LAYOUT_NAMES, 0),
                Kind::Partial => (PARTIAL_NAMES, 1),
                Kind::Macros => (MACRO_LIB_NAMES, 2),
                _ => (TRAIT_NAMES, 3),
            };
            let mut name = pool[used[slot].min(pool.len() - 1)].to_owned();
            used[slot] += 1;
            let odd = if self.autoescape == Some(Json::str("name")) {
                40
            } else {
                6
            };
            if self.rng.chance(odd) {
                name = (*self.rng.pick(ODD_NAMES)).to_owned();
            }
            if self.names.contains(&name) {
                name = format!("{}{}", self.fresh("t"), ".twig");
            }
            self.names.push(name.clone());
            out.push((kind, name));
        }
        out.push((Kind::Main, "index.twig".to_owned()));
        out
    }

    fn template(&mut self, kind: Kind, name: &str) -> Tpl {
        let budget = match kind {
            Kind::Main => self.rng.range(6, 16),
            Kind::Trait => self.rng.range(3, 7),
            _ => self.rng.range(3, 10),
        } as i32;
        let budget = if self.max_depth > 4 {
            budget * 2
        } else {
            budget
        };
        let mut d = Doc::new(name, kind, budget);
        d.macros = self.plan_macros(kind);
        let parents =
            self.lower(|t| matches!(t.kind, Kind::Layout | Kind::Partial) && !t.blocks.is_empty());
        match kind {
            Kind::Trait => self.trait_template(&mut d),
            Kind::Macros => self.macro_library(&mut d),
            _ => {
                let child = match kind {
                    Kind::Layout => 60,
                    Kind::Partial => 15,
                    _ => 60,
                };
                let can_extend = !parents.is_empty() || self.rng.chance(5);
                if can_extend && self.rng.chance(child) {
                    self.child_template(&mut d);
                } else {
                    self.standalone(&mut d);
                }
            }
        }
        Tpl {
            name: d.name.clone(),
            kind,
            source: d.out.clone(),
            blocks: d.all_blocks(),
            macros: d.macros.clone(),
            inherited_macros: d
                .parent_macros
                .iter()
                .filter(|m| !d.macros.iter().any(|o| o.name == m.name))
                .cloned()
                .collect(),
            traitable: d.traitable,
        }
    }

    /// The macros a namespace of the templates `targets` finds (own first, then inherited).
    fn namespace_macros(&self, targets: &[usize]) -> Vec<MacroSig> {
        let mut out: Vec<MacroSig> = Vec::new();
        for &t in targets {
            for m in self.tpls[t]
                .macros
                .iter()
                .chain(&self.tpls[t].inherited_macros)
            {
                if !out.iter().any(|o| o.name == m.name) {
                    out.push(m.clone());
                }
            }
        }
        out
    }

    /// Indexes of the generated templates matching `f`.
    fn lower(&self, f: impl Fn(&Tpl) -> bool) -> Vec<usize> {
        (0..self.tpls.len()).filter(|&i| f(&self.tpls[i])).collect()
    }

    fn plan_macros(&mut self, kind: Kind) -> Vec<MacroSig> {
        let count = match kind {
            Kind::Macros => self.rng.range(1, 4),
            Kind::Trait => 0,
            _ if self.rng.chance(20) => self.rng.range(1, 2),
            _ => 0,
        };
        let mut sigs: Vec<MacroSig> = Vec::new();
        for _ in 0..count {
            let recursive = self.rng.chance(12);
            let name = if recursive {
                "rec".to_owned()
            } else if self.rng.chance(5) {
                (*self.rng.pick(FUNCTION_LIKE_MACROS)).to_owned()
            } else {
                (*self.rng.pick(MACRO_NAMES)).to_owned()
            };
            if sigs.iter().any(|s| s.name == name) && !self.rng.chance(10) {
                // a redefinition (deprecated, the last one wins) only now and then
                continue;
            }
            let sig = if recursive {
                MacroSig {
                    name,
                    params: vec![
                        Param {
                            name: "n".to_owned(),
                            default: None,
                        },
                        Param {
                            name: "sep".to_owned(),
                            default: Some("'-'".to_owned()),
                        },
                    ],
                    variadic: None,
                    recursive: true,
                }
            } else {
                let mut params: Vec<Param> = Vec::new();
                for _ in 0..self.rng.range(0, 3) {
                    let pname = if self.rng.chance(4) {
                        (*self.rng.pick(RESERVED_PARAMS)).to_owned()
                    } else {
                        (*self.rng.pick(PARAM_NAMES)).to_owned()
                    };
                    if params.iter().any(|p| p.name == pname) {
                        continue;
                    }
                    let default = self.rng.chance(40).then(|| self.default_value());
                    params.push(Param {
                        name: pname,
                        default,
                    });
                }
                let variadic = self.rng.chance(20).then(|| "rest".to_owned());
                MacroSig {
                    name,
                    params,
                    variadic,
                    recursive: false,
                }
            };
            sigs.push(sig);
        }
        sigs
    }

    fn default_value(&mut self) -> String {
        let values = [
            "1",
            "'d'",
            "null",
            "true",
            "[1, 2]",
            "{k: 'v'}",
            "-1",
            "'<x>'",
            "1.5",
            "false",
            "[]",
            "+2",
            "none",
            "[[1], {a: [2]}]",
            "-1.5e3",
            "'%s'",
        ];
        if self.rng.below(300) == 0 {
            // not a constant: a syntax error
            return "'a' ~ 'b'".to_owned();
        }
        (*self.rng.pick(&values)).to_owned()
    }

    // ---- template kinds -------------------------------------------------------------------

    fn root_scope(&self) -> Scope {
        Scope {
            data: true,
            ..Scope::default()
        }
    }

    fn standalone(&mut self, d: &mut Doc) {
        let mut sc = self.root_scope();
        if !self.lower(|t| t.kind == Kind::Trait).is_empty() && self.rng.chance(15) {
            self.use_tag(d);
        }
        let n = self.rng.range(2, 6);
        for _ in 0..n {
            if d.budget <= 0 {
                break;
            }
            if d.written_macros < d.macros.len() && self.rng.chance(25) {
                self.macro_def(d, &sc);
            } else {
                self.stmt(d, &mut sc);
            }
            self.sep(d);
        }
        while d.written_macros < d.macros.len() {
            self.macro_def(d, &sc);
            self.sep(d);
        }
    }

    fn child_template(&mut self, d: &mut Doc) {
        let mut sc = self.root_scope();
        sc.child_root = true;
        let (parent, targets) = self.template_ref(d, &sc, Use::Extends);
        d.parent_blocks = self.blocks_of(&targets);
        d.parent_macros = self.namespace_macros(&targets);
        if self.rng.chance(12) {
            self.child_item(d, &mut sc);
            self.sep(d);
        }
        let extends = format!("{{% extends {parent} %}}");
        match self.rng.below(80) {
            // deprecated: not at the root (the parent is still set while parsing)
            0 => d
                .out
                .push_str(&format!("{{% if yes %}}{extends}{{% endif %}}")),
            // forbidden: two parents
            1 => {
                d.out.push_str(&extends);
                d.out.push('\n');
                let (other, _) = self.template_ref(d, &sc, Use::Extends);
                d.out.push_str(&format!("{{% extends {other} %}}"));
            }
            _ => d.out.push_str(&extends),
        }
        self.sep(d);
        let n = self.rng.range(1, 5);
        for _ in 0..n {
            self.child_item(d, &mut sc);
            self.sep(d);
        }
        while d.written_macros < d.macros.len() {
            self.macro_def(d, &sc);
            self.sep(d);
        }
    }

    /// One item at the root of a child template or an embed.
    fn child_item(&mut self, d: &mut Doc, sc: &mut Scope) {
        let traits = !self.lower(|t| t.kind == Kind::Trait).is_empty();
        let menu = [
            (60, 'b'),
            (8, 's'),
            (8, 'i'),
            (
                match (traits, d.embedded) {
                    (false, _) => 0,
                    (true, false) => 6,
                    (true, true) => 1,
                },
                'u',
            ),
            (
                if d.written_macros < d.macros.len() {
                    12
                } else {
                    0
                },
                'm',
            ),
            (3, 'd'),
            (3, 'f'),
            (3, 'w'),
            (1, 'x'),
        ];
        match self.rng.weighted(&menu) {
            'b' => self.block_def(d, sc),
            's' => self.set_stmt(d, sc),
            'i' => self.import_stmt(d, sc),
            'u' => self.use_tag(d),
            'm' => self.macro_def(d, sc),
            'd' => {
                let e = self.expr(d, sc, 1);
                d.out.push_str(&format!("{{% do {e} %}}"));
            }
            'f' => {
                let c = self.cond(d, sc);
                let v = self.fresh("v");
                let e = self.expr(d, sc, 1);
                d.out
                    .push_str(&format!("{{% if {c} %}}{{% set {v} = {e} %}}{{% endif %}}"));
            }
            'w' => d
                .out
                .push_str(self.rng.pick_str(&["\n", "  ", "{# c #}", "\n\t"])),
            _ => self.child_mistake(d, sc),
        }
    }

    /// Content a child template cannot have.
    fn child_mistake(&mut self, d: &mut Doc, sc: &mut Scope) {
        match self.rng.below(6) {
            0 => d.out.push_str("text"),
            1 => {
                let e = self.expr(d, sc, 1);
                d.out.push_str(&format!("{{{{ {e} }}}}"));
            }
            2 => {
                // a block under a tag
                let tag = self.rng.below(3);
                let (open, close) = match tag {
                    0 => ("{% if yes %}".to_owned(), "{% endif %}"),
                    1 => ("{% for i in [1] %}".to_owned(), "{% endfor %}"),
                    _ => ("{% with {} %}".to_owned(), "{% endwith %}"),
                };
                d.out.push_str(&open);
                self.block_def(d, sc);
                d.out.push_str(close);
            }
            3 => {
                let (t, _) = self.template_ref(d, sc, Use::Include);
                d.out.push_str(&format!("{{% include {t} %}}"));
            }
            4 => {
                // a block inside a capture is fine: it is defined and printed into the variable
                let v = self.fresh("v");
                d.out.push_str(&format!("{{% set {v} %}}"));
                self.block_def(d, sc);
                d.out.push_str("{% endset %}");
            }
            _ => {
                // extends inside a block
                let (t, _) = self.template_ref(d, sc, Use::Extends);
                let name = self.new_block_name(d, None);
                d.own_blocks.push(name.clone());
                d.out.push_str(&format!(
                    "{{% block {name} %}}{{% extends {t} %}}{{% endblock %}}"
                ));
            }
        }
    }

    fn trait_template(&mut self, d: &mut Doc) {
        d.traitable = true;
        let sc = self.root_scope();
        if !self.lower(|t| t.kind == Kind::Trait).is_empty() && self.rng.chance(40) {
            self.use_tag(d);
        }
        let n = self.rng.range(1, 3);
        for _ in 0..n {
            let mut sc = sc.clone();
            self.block_def(d, &mut sc);
            if self.rng.chance(30) {
                d.out.push_str(self.rng.pick_str(&["\n", "text", " "]));
            }
        }
        if self.rng.chance(5) {
            // output at the root: not traitable any more
            d.out.push_str("{{ s }}");
            d.traitable = false;
        }
    }

    fn macro_library(&mut self, d: &mut Doc) {
        let mut sc = self.root_scope();
        if self.rng.chance(20) {
            let alias = self.alias(&sc, d);
            d.out
                .push_str(&format!("{{% import _self as {alias} %}}\n"));
            d.main_imports.push(Import::Namespace {
                alias,
                target: Target::Itself,
            });
        }
        if self.rng.chance(20) {
            self.import_stmt(d, &mut sc);
            self.sep(d);
        }
        while d.written_macros < d.macros.len() {
            self.macro_def(d, &sc);
            self.sep(d);
            if self.rng.chance(15) {
                self.stmt(d, &mut sc);
            }
        }
        if self.rng.chance(25) {
            self.stmt(d, &mut sc);
        }
    }

    fn macro_def(&mut self, d: &mut Doc, outer: &Scope) {
        let index = d.written_macros;
        d.written_macros += 1;
        let sig = d.macros[index].clone();
        let mut params: Vec<String> = sig
            .params
            .iter()
            .map(|p| match &p.default {
                Some(v) => format!("{} = {v}", p.name),
                None => p.name.clone(),
            })
            .collect();
        if let Some(rest) = &sig.variadic {
            params.push(format!("...{rest}"));
        }
        if self.rng.below(200) == 0 {
            // reserved for the implicit variadic argument: a syntax error
            params.push("varargs".to_owned());
        }
        d.out.push_str(&format!(
            "{{% macro {}({}) %}}",
            sig.name,
            params.join(", ")
        ));
        let mut sc = Scope {
            in_macro: Some(index),
            local: true,
            data: false,
            depth: outer.depth + 1,
            ..Scope::default()
        };
        sc.vars = sig.params.iter().map(|p| p.name.clone()).collect();
        sc.vars
            .push(sig.variadic.clone().unwrap_or_else(|| "varargs".to_owned()));
        if sig.recursive {
            let call = if self.rng.chance(50) {
                format!("_self.{}(n - 1, sep)", sig.name)
            } else {
                let alias = "me";
                d.out.push_str(&format!("{{% import _self as {alias} %}}"));
                format!("{alias}.{}(n - 1, sep)", sig.name)
            };
            d.out.push_str(&format!(
                "{{{{ n }}}}{{% if n > 0 %}}{{{{ sep }}}}{{{{ {call} }}}}{{% endif %}}"
            ));
        } else {
            let n = self.rng.range(1, 4);
            for _ in 0..n {
                self.stmt(d, &mut sc);
                if self.rng.chance(20) {
                    d.out.push('\n');
                }
            }
        }
        if self.rng.chance(30) {
            d.out.push_str(&format!("{{% endmacro {} %}}", sig.name));
        } else {
            d.out.push_str("{% endmacro %}");
        }
    }

    // ---- statements -----------------------------------------------------------------------

    fn sep(&mut self, d: &mut Doc) {
        match self.rng.below(10) {
            0..=3 => d.out.push('\n'),
            4 => d.out.push_str("\n\n"),
            5 => d.out.push(' '),
            _ => {}
        }
    }

    fn body(&mut self, d: &mut Doc, sc: &Scope, max: usize) {
        let mut sc = sc.clone();
        let n = self.rng.range(1, max.max(1));
        for _ in 0..n {
            if d.budget <= 0 && self.rng.chance(70) {
                break;
            }
            self.stmt(d, &mut sc);
            if self.rng.chance(25) {
                d.out.push('\n');
            }
        }
    }

    fn stmt(&mut self, d: &mut Doc, sc: &mut Scope) {
        d.budget -= 1;
        let start = d.out.len();
        let nest = sc.depth < self.max_depth && d.budget > 0;
        let w = |cond: bool, weight: u32| if cond { weight } else { 0 };
        let blocks_ok = sc.in_macro.is_none() || self.rng.chance(10);
        let has_tpls = !self.tpls.is_empty();
        let has_block_tpls = self.tpls.iter().any(|t| !t.blocks.is_empty());
        let macros_ok = sc.block.as_deref() != Some(BLOCKS[0]);
        let callables = !d.macros.is_empty()
            || self.tpls.iter().any(|t| !t.macros.is_empty())
            || (self.string_loader && self.rng.chance(10));
        let menu = [
            (10, St::Text),
            (18, St::Print),
            (5, St::Set),
            (w(nest, 2), St::Capture),
            (w(nest, 5), St::If),
            (w(nest, 4), St::For),
            (w(nest, 2), St::With),
            (w(nest, 2), St::Apply),
            (w(nest, 1), St::Autoescape),
            (w(has_tpls, 9), St::Include),
            (w(has_tpls, 4), St::IncludeFn),
            (w(nest && has_block_tpls, 5), St::Embed),
            (w(blocks_ok && nest, 6), St::BlockDef),
            (6, St::BlockCall),
            (
                match &sc.block {
                    Some(b) if d.inherits(b) => 8,
                    Some(_) if d.has_inheritance() => 1,
                    _ => 0,
                },
                St::Parent,
            ),
            (w(macros_ok && callables, 9), St::MacroCall),
            (w(macros_ok && callables, 3), St::Import),
            (2, St::ContextKeys),
            (1, St::Source),
            (2, St::Do),
            (w(has_tpls, 2), St::Wrapper),
            (w(self.sandbox && nest, 3), St::Sandbox),
            (2, St::Throw),
            (2, St::Deprecated),
            (1, St::Special),
            (1, St::Flush),
            (1, St::Mistake),
        ];
        match self.rng.weighted(&menu) {
            St::Text => d.out.push_str(self.rng.pick_str(TEXTS)),
            St::Print => {
                let e = self.expr(d, sc, 0);
                let e = self.maybe_filter(e);
                let (open, close) = match self.rng.below(12) {
                    0 => ("{{-", "}}"),
                    1 => ("{{", "-}}"),
                    2 => ("{{~", "~}}"),
                    _ => ("{{", "}}"),
                };
                d.out.push_str(&format!("{open} {e} {close}"));
            }
            St::Set => self.set_stmt(d, sc),
            St::Capture => {
                let v = self.fresh("v");
                d.out.push_str(&format!("{{% set {v} %}}"));
                let inner = self.nested(sc);
                self.body(d, &inner, 3);
                d.out.push_str("{% endset %}");
                sc.vars.push(v.clone());
                if self.rng.chance(60) {
                    d.out.push_str(&format!("[{{{{ {v} }}}}]"));
                }
            }
            St::If => {
                let c = self.cond(d, sc);
                d.out.push_str(&format!("{{% if {c} %}}"));
                let inner = self.nested(sc);
                self.body(d, &inner, 3);
                if self.rng.chance(20) {
                    let c = self.cond(d, sc);
                    d.out.push_str(&format!("{{% elseif {c} %}}"));
                    self.body(d, &inner, 2);
                }
                if self.rng.chance(40) {
                    d.out.push_str("{% else %}");
                    self.body(d, &inner, 2);
                }
                d.out.push_str("{% endif %}");
            }
            St::For => {
                let seq = self.sequence(sc);
                let mut inner = self.nested(sc);
                inner.in_loop = true;
                let head = if self.rng.chance(30) {
                    inner.vars.push("k".to_owned());
                    inner.vars.push("i".to_owned());
                    format!("k, i in {seq}")
                } else {
                    inner.vars.push("i".to_owned());
                    format!("i in {seq}")
                };
                d.out.push_str(&format!("{{% for {head} %}}"));
                self.body(d, &inner, 3);
                if self.rng.chance(15) {
                    d.out.push_str("{% else %}");
                    let outer = self.nested(sc);
                    self.body(d, &outer, 1);
                }
                d.out.push_str("{% endfor %}");
            }
            St::With => {
                let only = self.rng.chance(40);
                let (vars, keys) = self.hash(d, sc);
                let mut inner = self.nested(sc);
                if only {
                    inner.vars = keys;
                    inner.data = false;
                } else {
                    inner.vars.extend(keys);
                }
                d.out.push_str(&format!(
                    "{{% with {vars}{} %}}",
                    if only { " only" } else { "" }
                ));
                self.body(d, &inner, 3);
                d.out.push_str("{% endwith %}");
            }
            St::Apply => {
                let f = *self.rng.pick(&[
                    "upper",
                    "lower|escape",
                    "escape",
                    "trim",
                    "spaceless",
                    "raw",
                ]);
                d.out.push_str(&format!("{{% apply {f} %}}"));
                let inner = self.nested(sc);
                self.body(d, &inner, 3);
                d.out.push_str("{% endapply %}");
            }
            St::Autoescape => {
                let s = *self.rng.pick(&["'html'", "'js'", "false", "true", "'url'"]);
                d.out.push_str(&format!("{{% autoescape {s} %}}"));
                let inner = self.nested(sc);
                self.body(d, &inner, 3);
                d.out.push_str("{% endautoescape %}");
            }
            St::Include => self.include_tag(d, sc),
            St::IncludeFn => {
                let e = self.include_call(d, sc);
                d.out.push_str(&format!("{{{{ {e} }}}}"));
            }
            St::Embed => self.embed(d, sc),
            St::BlockDef => self.block_def(d, sc),
            St::BlockCall => {
                let e = self.block_call(d, sc);
                d.out.push_str(&format!("{{{{ {e} }}}}"));
            }
            St::Parent => {
                if self.rng.chance(80) {
                    d.out.push_str("{{ parent() }}");
                } else {
                    let f = *self.rng.pick(&["upper", "length", "trim", "e"]);
                    d.out.push_str(&format!("{{{{ parent()|{f} }}}}"));
                }
            }
            St::MacroCall => self.macro_call_stmt(d, sc),
            St::Import => self.import_stmt(d, sc),
            St::ContextKeys => d.out.push_str("{{ _context|keys|join(',') }}"),
            St::Source => {
                let (t, _) = self.template_ref(d, sc, Use::Source);
                if self.rng.chance(30) {
                    d.out
                        .push_str(&format!("{{{{ source({t}, ignore_missing = true) }}}}"));
                } else {
                    d.out.push_str(&format!("{{{{ source({t})|length }}}}"));
                }
            }
            St::Do => {
                let e = self.expr(d, sc, 1);
                d.out.push_str(&format!("{{% do {e} %}}"));
            }
            St::Wrapper => {
                let e = self.wrapper_expr(d, sc);
                d.out.push_str(&format!("{{{{ {e} }}}}"));
            }
            St::Sandbox => {
                // deprecated tag; only includes are allowed inside
                d.out.push_str("{% sandbox %}");
                self.include_tag(d, sc);
                d.out.push_str("{% endsandbox %}");
            }
            St::Throw => {
                let e = match self.rng.below(7) {
                    0 => "1 // 0".to_owned(),
                    1 => "n % 0".to_owned(),
                    2 => "range(1, 2, 0)|length".to_owned(),
                    3 => "map.zz".to_owned(),
                    4 => "include('-', 5)".to_owned(),
                    5 => "[1]|join(', ', 'x', 'y')".to_owned(),
                    _ => "list|batch(-1)|length".to_owned(),
                };
                if self.rng.chance(30) {
                    d.out.push_str(&format!("{{{{\n  {e}\n}}}}"));
                } else {
                    d.out.push_str(&format!("{{{{ {e} }}}}"));
                }
            }
            St::Deprecated => {
                let message = *self
                    .rng
                    .pick(&["'old'", "'gone ' ~ _self", "\"in #{_self}\""]);
                if self.rng.chance(30) {
                    d.out.push_str(&format!(
                        "{{% deprecated {message} package='acme/pkg' version='1.2' %}}"
                    ));
                } else {
                    d.out.push_str(&format!("{{% deprecated {message} %}}"));
                }
            }
            St::Flush => {
                let tag = *self.rng.pick(&[
                    "{% flush %}",
                    "{% types {x: 'string', y?: 'int'} %}",
                    "{% types {} %}",
                ]);
                d.out.push_str(tag);
            }
            St::Special => {
                let special = match self.rng.below(7) {
                    0 => "{% set _self = 'S' %}{{ _self }}|{{ _context._self }}",
                    1 => "{% set _context = {a: 1} %}{{ _context|keys|join(',') }}",
                    2 => "{% set _charset = 'C' %}{{ _charset }}",
                    3 => "{% for _self in [1, 2] %}{{ _self }}{% endfor %}",
                    4 => {
                        "{% set _parent = 'P' %}{% for i in [1] %}{{ _parent|length }}{% endfor %}{{ _parent }}"
                    }
                    5 => "{% set _seq = 'Q' %}{% for i in [1] %}{% endfor %}{{ _seq }}",
                    _ => "{% set _key = 'K' %}{{ _key }}",
                };
                d.out.push_str(special);
            }
            St::Mistake => {
                if self.rng.chance(35) {
                    self.mistake(d, sc);
                } else {
                    d.out.push_str(self.rng.pick_str(TEXTS));
                }
            }
        }
        let calls = d.out[start..].contains("(");
        if self.rng.chance(if calls { 14 } else { 6 }) {
            spread_lines(&mut d.out, start);
        }
        if self.rng.chance(8) {
            let open = *self.rng.pick(&['-', '~', ' ']);
            let close = *self.rng.pick(&['-', '~', ' ']);
            whitespace_control(&mut d.out, start, open, close);
        }
    }

    fn nested(&self, sc: &Scope) -> Scope {
        let mut inner = sc.clone();
        inner.depth += 1;
        inner.child_root = false;
        inner
    }

    fn set_stmt(&mut self, d: &mut Doc, sc: &mut Scope) {
        let v = self.fresh("v");
        match self.rng.below(10) {
            0 => {
                let w = self.fresh("v");
                let a = self.expr(d, sc, 1);
                let b = self.expr(d, sc, 1);
                d.out.push_str(&format!("{{% set {v}, {w} = {a}, {b} %}}"));
                sc.vars.push(w);
            }
            1 => {
                // shadow a data variable or a global
                let target = *self.rng.pick(&["s", "g", "n", "x"]);
                let e = self.expr(d, sc, 1);
                d.out.push_str(&format!("{{% set {target} = {e} %}}"));
                sc.vars.push(target.to_owned());
                return;
            }
            _ => {
                let e = self.expr(d, sc, 0);
                d.out.push_str(&format!("{{% set {v} = {e} %}}"));
            }
        }
        self.set_vars.push(v.clone());
        sc.vars.push(v);
    }

    fn mistake(&mut self, d: &mut Doc, sc: &mut Scope) {
        match self.rng.below(9) {
            0 => d.out.push_str("{{ parent() }}"),
            1 => {
                let (t, _) = self.template_ref(d, sc, Use::Extends);
                d.out.push_str(&format!("{{% extends {t} %}}"));
            }
            2 => d.out.push_str("{% use tpl_name %}"),
            3 => d.out.push_str("{{ block('nope') }}"),
            4 => {
                let alias = *self.rng.pick(ALIASES);
                d.out.push_str(&format!("{{{{ {alias}.nope() }}}}"));
            }
            5 => {
                if let Some(b) = d.own_blocks.first().cloned() {
                    // defined twice
                    d.out
                        .push_str(&format!("{{% block {b} %}}twice{{% endblock %}}"));
                } else {
                    d.out.push_str("{{ undefined_var }}");
                }
            }
            6 => d.out.push_str("{% block %}x{% endblock %}"),
            7 => {
                let name = self.new_block_name(d, sc.block.as_deref());
                d.own_blocks.push(name.clone());
                d.out
                    .push_str(&format!("{{% block {name} %}}x{{% endblock wrong %}}"));
            }
            _ => d.out.push_str("{{ _self.nope() }}"),
        }
    }

    // ---- blocks ---------------------------------------------------------------------------

    /// A block name that is not defined yet (of a lower rank than `enclosing`).
    fn new_block_name(&mut self, d: &Doc, enclosing: Option<&str>) -> String {
        let limit = enclosing.map_or(BLOCKS.len(), block_rank);
        let free: Vec<&str> = BLOCKS[..limit.min(BLOCKS.len())]
            .iter()
            .copied()
            .filter(|b| !d.own_blocks.iter().any(|o| o == b))
            .collect();
        if free.is_empty() {
            // nothing left: a block defined twice or of a too high rank would recurse; reuse the
            // lowest rank name (a syntax error when it exists already)
            BLOCKS[0].to_owned()
        } else {
            (*self.rng.pick(&free)).to_owned()
        }
    }

    fn block_def(&mut self, d: &mut Doc, sc: &mut Scope) {
        let limit = sc.block.as_deref().map_or(BLOCKS.len(), block_rank);
        let enclosing_macro = sc.in_macro.is_some();
        // overriding an inherited block is the point of a child template
        let inherited: Vec<String> = d
            .parent_blocks
            .iter()
            .chain(&d.trait_blocks)
            .filter(|b| block_rank(b) < limit && !d.own_blocks.contains(b))
            .cloned()
            .collect();
        let name = if enclosing_macro {
            // macros only render blocks of rank 0 (no recursion through macros)
            BLOCKS[0].to_owned()
        } else if !inherited.is_empty() && self.rng.chance(75) {
            self.rng.pick(&inherited).clone()
        } else {
            self.new_block_name(d, sc.block.as_deref())
        };
        if block_rank(&name) >= limit {
            // no name of a lower rank is left: a definition or a call would recurse
            d.out.push_str(self.rng.pick_str(TEXTS));
            return;
        }
        if d.own_blocks.contains(&name) && !self.rng.chance(3) {
            // already defined: print it instead
            d.out.push_str(&format!("{{{{ block('{name}') }}}}"));
            return;
        }
        d.own_blocks.push(name.clone());
        let mut inner = self.nested(sc);
        inner.self_limit = Some(sc.self_block_limit().min(block_rank(&name)));
        inner.block = Some(name.clone());
        inner.imports.clear();
        inner.local = true;
        inner.in_macro = None;
        if self.rng.chance(12) {
            // the short form
            let e = self.expr(d, &inner, 1);
            d.out.push_str(&format!("{{% block {name} {e} %}}"));
            return;
        }
        d.out.push_str(&format!("{{% block {name} %}}"));
        if d.inherits(&name) && self.rng.chance(45) {
            d.out.push_str("{{ parent() }}");
        }
        self.body(d, &inner, 4);
        if self.rng.chance(25) {
            d.out.push_str(&format!("{{% endblock {name} %}}"));
        } else {
            d.out.push_str("{% endblock %}");
        }
    }

    /// `block(...)` in an expression.
    fn block_call(&mut self, d: &mut Doc, sc: &Scope) -> String {
        let limit = sc.block.as_deref().map_or(BLOCKS.len(), block_rank);
        let limit = if sc.in_macro.is_some() { 1 } else { limit };
        let mine: Vec<String> = d
            .all_blocks()
            .into_iter()
            .filter(|b| block_rank(b) < limit)
            .collect();
        if mine.is_empty() && self.rng.chance(85) {
            return format!(
                "(block('{}') is defined ? 'D' : 'U')",
                self.rng.pick(&["nope", BLOCKS[0], BLOCKS[1]])
            );
        }
        let pick_name = |rng: &mut Rng, names: &[String]| -> String {
            if names.is_empty() || rng.chance(3) {
                (*rng.pick(&["nope", BLOCKS[0]])).to_owned()
            } else {
                rng.pick(names).clone()
            }
        };
        match self.rng.below(10) {
            0..=4 => {
                let name = pick_name(&mut self.rng, &mine);
                if name != "nope" && block_rank(&name) >= limit {
                    return "(block('nope') is defined ? 'D' : 'U')".to_owned();
                }
                if self.rng.chance(4) {
                    // not a string: converted like an array key (and printed with %s)
                    let odd = if sc.data && self.rng.chance(15) {
                        self.markup_var(&name)
                    } else {
                        (*self.rng.pick(&[
                            "5", "null", "1.5", "true", "false", "[]", "n", "nul", "list",
                        ]))
                        .to_owned()
                    };
                    return if self.rng.chance(50) {
                        format!("(block({odd}) is defined ? 'D' : 'U')")
                    } else {
                        format!("block({odd})")
                    };
                }
                if self.rng.chance(25) {
                    format!("(block('{name}') is defined ? 'D' : 'U')")
                } else if self.rng.chance(10) {
                    let var = self.data_var("bn", Json::str(&name));
                    format!("block({var})")
                } else {
                    format!("block('{name}')")
                }
            }
            5..=8 => {
                // another template's own block table
                let (t, targets) = self.template_ref(d, sc, Use::BlockOf);
                // `_self` is this (enclosing) template: its limit applies
                let limit = if t == "_self" {
                    limit.min(sc.self_block_limit())
                } else {
                    limit
                };
                let names: Vec<String> = self
                    .blocks_of(&targets)
                    .into_iter()
                    .filter(|b| block_rank(b) < limit)
                    .collect();
                let name = pick_name(&mut self.rng, &names);
                if name != "nope" && block_rank(&name) >= limit {
                    return format!("(block('nope', {t}) is defined ? 'D' : 'U')");
                }
                if self.rng.chance(25) {
                    format!("(block('{name}', {t}) is defined ? 'D' : 'U')")
                } else {
                    format!("block('{name}', {t})")
                }
            }
            _ => {
                // this template by name: its own blocks, without the child's overrides
                let self_limit = sc.self_block_limit();
                let names: Vec<String> = mine
                    .into_iter()
                    .filter(|b| block_rank(b) < self_limit)
                    .collect();
                let name = pick_name(&mut self.rng, &names);
                if name != "nope" && block_rank(&name) >= self_limit {
                    return "'-'".to_owned();
                }
                format!("block('{name}', _self)")
            }
        }
    }

    fn blocks_of(&self, targets: &[usize]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for &t in targets {
            for b in &self.tpls[t].blocks {
                if !out.contains(b) {
                    out.push(b.clone());
                }
            }
        }
        out
    }

    // ---- traits ---------------------------------------------------------------------------

    fn use_tag(&mut self, d: &mut Doc) {
        // a trait that gained output at its root is not traitable any more: rarely used
        let broken_too = self.rng.chance(10);
        let traits = self.lower(|t| t.kind == Kind::Trait && (t.traitable || broken_too));
        let target = if traits.is_empty() || self.rng.chance(4) {
            // not a trait: a layout or partial (an error unless it is traitable by accident)
            let any = self.lower(|t| t.kind != Kind::Trait);
            if any.is_empty() || self.rng.chance(30) {
                d.out
                    .push_str(&format!("{{% use '{}' %}}", self.rng.pick(MISSING)));
                return;
            }
            *self.rng.pick(&any)
        } else {
            *self.rng.pick(&traits)
        };
        let tpl = self.tpls[target].clone();
        let mut blocks = tpl.blocks.clone();
        let mut targets: Vec<String> = Vec::new();
        if !blocks.is_empty() && self.rng.chance(40) {
            for _ in 0..self.rng.range(1, 2) {
                let from = if self.rng.chance(3) {
                    "nope".to_owned()
                } else {
                    self.rng.pick(&blocks).clone()
                };
                // an alias of a lower rank could make the block call itself
                let higher: Vec<&str> = BLOCKS
                    .iter()
                    .copied()
                    .filter(|b| block_rank(b) >= block_rank(&from))
                    .collect();
                let to = if higher.is_empty() {
                    from.clone()
                } else {
                    (*self.rng.pick(&higher)).to_owned()
                };
                if targets.iter().any(|t| t.starts_with(&format!("{from} "))) {
                    continue;
                }
                blocks.retain(|b| *b != from);
                if !blocks.contains(&to) {
                    blocks.push(to.clone());
                }
                targets.push(format!("{from} as {to}"));
            }
        }
        let with = if targets.is_empty() {
            String::new()
        } else {
            format!(" with {}", targets.join(", "))
        };
        d.out
            .push_str(&format!("{{% use {}{with} %}}", quote(&tpl.name)));
        for b in blocks {
            if !d.trait_blocks.contains(&b) {
                d.trait_blocks.push(b);
            }
        }
    }

    // ---- include, embed -------------------------------------------------------------------

    fn include_tag(&mut self, d: &mut Doc, sc: &Scope) {
        // at the root of a template without a parent nothing can drop `depth` from the context
        // between two levels (`only`, macros, a parent's blocks)
        let root = sc.depth == 0
            && sc.block.is_none()
            && sc.in_macro.is_none()
            && sc.data
            && !d.embedded
            && d.parent_blocks.is_empty()
            && d.kind != Kind::Trait;
        if root && self.rng.chance(15) {
            // a template including itself, with a depth guard
            d.out.push_str(
                "{% set depth = (depth ?? 0) + 1 %}{{ depth }}{% if depth < 3 %}{% include _self %}{% endif %}",
            );
            return;
        }
        let (t, _) = self.template_ref(d, sc, Use::Include);
        let mut tag = format!("{{% include {t}");
        if self.rng.chance(20) {
            tag.push_str(" ignore missing");
        }
        if self.rng.chance(40) {
            let (vars, _) = self.with_value(d, sc);
            tag.push_str(&format!(" with {vars}"));
        }
        if self.rng.chance(12) {
            tag.push_str(" only");
        }
        tag.push_str(" %}");
        d.out.push_str(&tag);
    }

    fn include_call(&mut self, d: &mut Doc, sc: &Scope) -> String {
        let (t, _) = self.template_ref(d, sc, Use::Include);
        if self.rng.chance(15) {
            let (vars, _) = self.with_value(d, sc);
            return format!("include_only({t}, {vars})");
        }
        let mut args = vec![t];
        let mut context_given = false;
        if self.rng.chance(40) {
            let (vars, _) = self.with_value(d, sc);
            args.push(vars);
            if self.rng.chance(30) {
                args.push((*self.rng.pick(&["false", "true"])).to_owned());
                context_given = true;
            }
        }
        if !context_given && self.rng.chance(15) {
            args.push("with_context = false".to_owned());
        }
        if self.rng.chance(15) {
            args.push("ignore_missing = true".to_owned());
        }
        if self.sandbox && self.rng.chance(20) {
            args.push("sandboxed = true".to_owned());
        }
        let call = format!("include({})", args.join(", "));
        match self.rng.below(10) {
            0 => format!("{call}|upper"),
            1 => format!("{call}|length"),
            2 => format!("{call} ~ '|'"),
            _ => call,
        }
    }

    /// The variables of `include ... with`, `include()` and `embed ... with`.
    fn with_value(&mut self, d: &mut Doc, sc: &Scope) -> (String, Vec<String>) {
        match self.rng.below(30) {
            0 | 1 => ("map".to_owned(), vec!["a".to_owned(), "b".to_owned()]),
            2 | 3 => ("list".to_owned(), Vec::new()),
            4 | 5 => ("_context".to_owned(), Vec::new()),
            6 => ((*self.rng.pick(&["s", "n", "nul"])).to_owned(), Vec::new()),
            7 | 8 => ("{5: 'five', 'x': 1}".to_owned(), vec!["x".to_owned()]),
            9 if !self.php.iter().any(|(k, _)| k == "iter") => {
                self.php.push((
                    "iter".to_owned(),
                    "new \\ArrayIterator(['it' => 1, 's' => 'from-iter'])".to_owned(),
                ));
                ("iter".to_owned(), vec!["it".to_owned()])
            }
            10 if !self.php.iter().any(|(k, _)| k == "ao") => {
                self.php.push((
                    "ao".to_owned(),
                    "new \\ArrayObject(['ao' => 'from-ao', 5 => 'five'])".to_owned(),
                ));
                ("ao".to_owned(), vec!["ao".to_owned()])
            }
            _ => self.hash(d, sc),
        }
    }

    /// A hash literal with expressions as values; returns its keys too.
    fn hash(&mut self, d: &mut Doc, sc: &Scope) -> (String, Vec<String>) {
        let mut keys: Vec<String> = Vec::new();
        let mut parts: Vec<String> = Vec::new();
        for _ in 0..self.rng.range(0, 3) {
            let key = match self.rng.below(7) {
                0 => "s".to_owned(),
                1 => "g".to_owned(),
                2 => "depth".to_owned(),
                3 => "x".to_owned(),
                4 if self.rng.chance(30) => (*self
                    .rng
                    .pick(&["_self", "_context", "_charset", "varargs", "loop"]))
                .to_owned(),
                _ => self.fresh("k"),
            };
            if keys.contains(&key) {
                continue;
            }
            let value = self.expr(d, sc, 2);
            parts.push(format!("{key}: {value}"));
            keys.push(key);
        }
        (format!("{{{}}}", parts.join(", ")), keys)
    }

    fn embed(&mut self, d: &mut Doc, sc: &Scope) {
        let (t, targets) = self.template_ref(d, sc, Use::Embed);
        let mut tag = format!("{{% embed {t}");
        if self.rng.chance(10) {
            tag.push_str(" ignore missing");
        }
        let mut inner = self.nested(sc);
        if self.rng.chance(35) {
            let (vars, keys) = self.with_value(d, sc);
            tag.push_str(&format!(" with {vars}"));
            inner.vars.extend(keys);
        }
        if self.rng.chance(12) {
            tag.push_str(" only");
            inner.vars.clear();
            inner.data = false;
        }
        tag.push_str(" %}");
        d.out.push_str(&tag);
        // the embedded template: a child of the target with its own blocks, imports and macros
        let mut e = Doc::new(&d.name, Kind::Main, d.budget.min(6));
        e.embedded = true;
        e.parent_blocks = self.blocks_of(&targets);
        e.parent_macros = self.namespace_macros(&targets);
        inner.child_root = true;
        // the embed's blocks form a table of their own, `_self` still names this template
        inner.self_limit = Some(sc.self_block_limit());
        inner.block = None;
        inner.in_macro = None;
        inner.imports.clear();
        inner.local = false;
        for _ in 0..self.rng.range(0, 3) {
            self.child_item(&mut e, &mut inner);
        }
        d.budget = d.budget.min(e.budget);
        d.out.push_str(&e.out);
        d.out.push_str("{% endembed %}");
    }

    // ---- macros ---------------------------------------------------------------------------

    fn alias(&mut self, sc: &Scope, d: &Doc) -> String {
        let taken = |a: &str| {
            sc.imports
                .iter()
                .chain(&d.main_imports)
                .any(|i| matches!(i, Import::Namespace { alias, .. } | Import::Function { alias, .. } if alias == a))
        };
        let free: Vec<&str> = ALIASES.iter().copied().filter(|a| !taken(a)).collect();
        if self.rng.chance(3) {
            // the name of a variable (a namespace alias wins over the variable in `alias.x()`)
            return (*self.rng.pick(&["s", "map", "list", "g"])).to_owned();
        }
        if free.is_empty() || self.rng.chance(5) {
            // re-import under the same alias
            (*self.rng.pick(ALIASES)).to_owned()
        } else {
            (*self.rng.pick(&free)).to_owned()
        }
    }

    fn import_stmt(&mut self, d: &mut Doc, sc: &mut Scope) {
        let libs = self.lower(|t| !t.macros.is_empty());
        let (expr, target) = if self.rng.chance(1) {
            (quote(self.rng.pick_str(MISSING)), Target::Unknown)
        } else if libs.is_empty() || self.rng.chance(15) {
            ("_self".to_owned(), Target::Itself)
        } else {
            let t = *self.rng.pick(&libs);
            let expr = match self.rng.below(10) {
                0 => {
                    let name = self.tpls[t].name.clone();
                    self.data_var("tn", Json::str(name))
                }
                1 if self.string_loader => {
                    // a template from a string has no macros
                    return self.import_from_string(d, sc);
                }
                _ => quote(&self.tpls[t].name),
            };
            (expr, Target::Tpl(t))
        };
        let import = if self.rng.chance(35) {
            let macros = self.macros_of(&target, d);
            let mut names: Vec<(String, String)> = Vec::new();
            for _ in 0..self.rng.range(1, 2) {
                let name = if macros.is_empty() || self.rng.chance(4) {
                    "nope".to_owned()
                } else {
                    self.rng.pick(&macros).name.clone()
                };
                // an alias named like a function turns every later call of that function in
                // scope into a macro call: only safe for macros of lower templates (a macro of
                // this template could reach itself through an `include()` of its own body)
                let own = matches!(target, Target::Itself);
                let shadowing = FUNCTION_LIKE_MACROS.contains(&name.as_str());
                let alias = if !own && self.rng.chance(4) {
                    // a function name: the imported macro shadows the function
                    (*self.rng.pick(FUNCTION_LIKE_MACROS)).to_owned()
                } else if (own && shadowing) || self.rng.chance(40) {
                    self.fresh("f")
                } else {
                    name.clone()
                };
                if names.iter().any(|(n, a)| *a == alias || *n == name) {
                    continue;
                }
                names.push((name, alias));
            }
            let list: Vec<String> = names
                .iter()
                .map(|(n, a)| {
                    if n == a {
                        n.clone()
                    } else {
                        format!("{n} as {a}")
                    }
                })
                .collect();
            d.out
                .push_str(&format!("{{% from {expr} import {} %}}", list.join(", ")));
            names
                .into_iter()
                .map(|(name, alias)| Import::Function {
                    alias,
                    target: target.clone(),
                    name,
                })
                .collect::<Vec<_>>()
        } else {
            let alias = self.alias(sc, d);
            d.out.push_str(&format!("{{% import {expr} as {alias} %}}"));
            vec![Import::Namespace { alias, target }]
        };
        if sc.local {
            sc.imports.extend(import);
        } else {
            d.main_imports.extend(import);
        }
    }

    fn import_from_string(&mut self, d: &mut Doc, sc: &mut Scope) {
        let alias = self.alias(sc, d);
        d.out.push_str(&format!(
            "{{% import template_from_string(\"{{% macro hi(x) %}}hi {{{{ x }}}}{{% endmacro %}}\") as {alias} %}}"
        ));
        let import = Import::Namespace {
            alias,
            target: Target::FromString,
        };
        if sc.local {
            sc.imports.push(import);
        } else {
            d.main_imports.push(import);
        }
    }

    fn macros_of(&self, target: &Target, d: &Doc) -> Vec<MacroSig> {
        match target {
            Target::Tpl(t) => self.namespace_macros(&[*t]),
            Target::Itself => d.macros.clone(),
            Target::FromString => vec![MacroSig {
                name: "hi".to_owned(),
                params: vec![Param {
                    name: "x".to_owned(),
                    default: None,
                }],
                variadic: None,
                recursive: false,
            }],
            Target::Unknown => Vec::new(),
        }
    }

    /// The macros the current scope can call.
    fn callees(&self, d: &Doc, sc: &Scope) -> Vec<Callee> {
        let mut out: Vec<Callee> = Vec::new();
        // in a macro of this template only the macros defined before it (no recursion); a name
        // defined twice calls the last definition
        let own_limit = sc.in_macro.unwrap_or(d.macros.len());
        let own = |i: usize| {
            let name = &d.macros[i].name;
            let effective = d.macros.iter().rposition(|s| s.name == *name).unwrap_or(i);
            effective < own_limit
        };
        let visible: Vec<&Import> = if sc.local {
            sc.imports.iter().chain(&d.main_imports).collect()
        } else {
            d.main_imports.iter().collect()
        };
        for import in visible {
            match import {
                Import::Namespace { alias, target } => {
                    for (i, sig) in self.macros_of(target, d).into_iter().enumerate() {
                        if matches!(target, Target::Itself) && !own(i) {
                            continue;
                        }
                        out.push(Callee {
                            head: format!("{alias}.{}", sig.name),
                            sig,
                        });
                    }
                }
                Import::Function {
                    alias,
                    target,
                    name,
                } => {
                    let macros = self.macros_of(target, d);
                    if let Some((i, sig)) = macros.iter().enumerate().find(|(_, s)| s.name == *name)
                    {
                        if matches!(target, Target::Itself) && !own(i) {
                            continue;
                        }
                        out.push(Callee {
                            head: alias.clone(),
                            sig: sig.clone(),
                        });
                    }
                }
            }
        }
        if !d.embedded {
            for (i, sig) in d.macros.iter().enumerate() {
                if own(i) {
                    out.push(Callee {
                        head: format!("_self.{}", sig.name),
                        sig: sig.clone(),
                    });
                }
            }
        }
        // through the parent chain: the parents' macros (an embed's parent is its target)
        for sig in &d.parent_macros {
            if !d.macros.iter().any(|m| m.name == sig.name) {
                out.push(Callee {
                    head: format!("_self.{}", sig.name),
                    sig: sig.clone(),
                });
            }
        }
        out
    }

    fn macro_call_stmt(&mut self, d: &mut Doc, sc: &mut Scope) {
        let mut callees = self.callees(d, sc);
        if callees.is_empty() || self.rng.chance(15) {
            self.import_stmt(d, sc);
            callees = self.callees(d, sc);
        }
        let Some(callee) = (!callees.is_empty()).then(|| self.rng.pick(&callees).clone()) else {
            return;
        };
        let call = self.macro_call(d, sc, &callee);
        match self.rng.below(10) {
            0 => d.out.push_str(&format!("{{% do {call} %}}")),
            1 => {
                let v = self.fresh("v");
                d.out.push_str(&format!(
                    "{{% set {v} = {call} %}}{{{{ {v} is same as('') ? 'empty' : {v} }}}}"
                ));
            }
            _ => d.out.push_str(&format!("{{{{ {call} }}}}")),
        }
    }

    /// A call of `callee` (sometimes misspelt or tested with `is defined`).
    fn macro_call(&mut self, d: &mut Doc, sc: &Scope, callee: &Callee) -> String {
        let head = &callee.head;
        match self.rng.below(40) {
            0 => {
                // another case: deprecated, found case-insensitively
                if let Some((prefix, name)) = head.rsplit_once('.') {
                    return format!("{prefix}.{}()", name.to_uppercase());
                }
            }
            1 => {
                if let Some((prefix, name)) = head.rsplit_once('.') {
                    return format!("{prefix}.macro_{name}()");
                }
            }
            2 => {
                if head.contains('.') {
                    // no parentheses: deprecated
                    return head.clone();
                }
            }
            3 | 4 => {
                if let Some((prefix, name)) = head.rsplit_once('.') {
                    let tested = if self.rng.chance(30) { "nope" } else { name };
                    return format!("({prefix}.{tested} is defined ? 'D' : 'U')");
                }
            }
            5 | 6 => {
                // a name computed at run time: `m.(expr)()`
                if let Some((prefix, name)) = head.rsplit_once('.') {
                    let (a, b) = name.split_at(name.len() / 2);
                    let dynamic = match self.rng.below(4) {
                        0 => format!("'{a}' ~ '{b}'"),
                        1 => format!("'{}'", name.to_uppercase()),
                        2 => "n".to_owned(),
                        _ => format!("'{name}'"),
                    };
                    let args = self.macro_args(d, sc, &callee.sig);
                    return if self.rng.chance(15) {
                        format!("{prefix}.({dynamic})")
                    } else {
                        format!("{prefix}.({dynamic})({args})")
                    };
                }
            }
            7 => {
                if let Some((prefix, name)) = head.rsplit_once('.') {
                    let args = self.macro_args(d, sc, &callee.sig);
                    return format!("{prefix}?.{name}({args})");
                }
            }
            _ => {}
        }
        let args = self.macro_args(d, sc, &callee.sig);
        format!("{head}({args})")
    }

    fn arg_value(&mut self, d: &mut Doc, sc: &Scope) -> String {
        if self.rng.below(40) == 0 {
            // throws; inside an argument list the line PHP reports depends on constant folding
            return (*self.rng.pick(&[
                "n // 0", "n % nul", "-list", "[] + nul", "map - 1", "list * 2",
            ]))
            .to_owned();
        }
        if self.rng.chance(50) {
            self.literal()
        } else {
            self.expr(d, sc, 2)
        }
    }

    fn macro_args(&mut self, d: &mut Doc, sc: &Scope, sig: &MacroSig) -> String {
        if sig.recursive {
            let n = self.rng.range(0, 3);
            return match self.rng.below(6) {
                0 => format!("n = {n}"),
                1 => format!("{n}, '+'"),
                2 => format!("sep: '/', n: {n}"),
                _ => format!("{n}"),
            };
        }
        let params = &sig.params;
        let mut args: Vec<String> = Vec::new();
        match self.rng.below(40) {
            // positional, as many as declared
            0..=6 | 20..=29 => {
                for _ in params {
                    args.push(self.arg_value(d, sc));
                }
            }
            // fewer than declared
            7 | 8 => {
                let k = self.rng.below(params.len().max(1));
                for _ in 0..k {
                    args.push(self.arg_value(d, sc));
                }
            }
            // named, in another order
            9 | 10 | 34..=36 => {
                let mut order: Vec<&Param> = params.iter().collect();
                self.rng.shuffle(&mut order);
                for p in order {
                    if self.rng.chance(80) {
                        let v = self.arg_value(d, sc);
                        let sep = if self.rng.chance(30) { ": " } else { " = " };
                        args.push(format!("{}{sep}{v}", p.name));
                    }
                }
            }
            // positional then named
            11 | 12 | 37 | 38 => {
                let k = self.rng.below(params.len() + 1);
                for _ in 0..k {
                    args.push(self.arg_value(d, sc));
                }
                for p in &params[k..] {
                    if self.rng.chance(70) {
                        let v = self.arg_value(d, sc);
                        args.push(format!("{} = {v}", p.name));
                    }
                }
            }
            // more than declared
            13 | 14 | 30 | 31 => {
                for _ in 0..params.len() + self.rng.range(1, 2) {
                    args.push(self.arg_value(d, sc));
                }
                if self.rng.chance(40) {
                    let v = self.arg_value(d, sc);
                    args.push(format!("extra = {v}"));
                }
            }
            // an unknown named argument
            15 => {
                let v = self.arg_value(d, sc);
                let name = *self.rng.pick(&["zz", "X", "Label", "varargs"]);
                args.push(format!("{name} = {v}"));
            }
            // positional after named, or a name given twice: run-time errors
            16 => {
                if let Some(p) = params.first() {
                    let v = self.arg_value(d, sc);
                    if self.rng.chance(50) {
                        args.push(format!("{} = {v}", p.name));
                        args.push(self.arg_value(d, sc));
                    } else {
                        args.push(v);
                        let w = self.arg_value(d, sc);
                        args.push(format!("{} = {w}", p.name));
                    }
                }
            }
            // spread
            17 | 32 | 33 => {
                let spread = *self.rng.pick(&[
                    "list",
                    "map",
                    "[1, 2]",
                    "{x: 1, y: 2}",
                    "{0: 'z'}",
                    "s",
                    "nul",
                    "{0: 'a', x: 1, 1: 'b'}",
                    "[[1], {k: 2}]",
                    "list, ...map",
                    "{context: 1, varargs: 2}",
                ]);
                if self.rng.chance(40) {
                    args.push(self.arg_value(d, sc));
                }
                args.push(format!("...{spread}"));
            }
            // none
            _ => {}
        }
        args.join(", ")
    }

    // ---- expressions ----------------------------------------------------------------------

    fn expr(&mut self, d: &mut Doc, sc: &Scope, depth: u32) -> String {
        let w = |cond: bool, weight: u32| if cond { weight } else { 0 };
        let deep = depth < 3;
        let has_tpls = !self.tpls.is_empty();
        let menu = [
            (26, Ex::Var),
            (8, Ex::Literal),
            (5, Ex::SelfName),
            (w(deep, 7), Ex::Block),
            (w(sc.block.is_some() && d.has_inheritance(), 5), Ex::Parent),
            (w(deep && has_tpls, 5), Ex::Include),
            (
                w(deep && sc.block.as_deref() != Some(BLOCKS[0]), 8),
                Ex::Macro,
            ),
            (w(has_tpls, 1), Ex::Source),
            (w(deep, 4), Ex::Concat),
            (w(deep, 4), Ex::Ternary),
            (w(deep, 3), Ex::Default),
            (2, Ex::Keys),
            (w(deep, 3), Ex::Defined),
            (w(has_tpls && deep, 2), Ex::Wrapper),
            (w(self.string_loader && deep, 2), Ex::FromString),
        ];
        match self.rng.weighted(&menu) {
            Ex::Var => self.var_ref(sc),
            Ex::Literal => self.literal(),
            Ex::SelfName => "_self".to_owned(),
            Ex::Block => self.block_call(d, sc),
            Ex::Parent => "parent()".to_owned(),
            Ex::Include => self.include_call(d, sc),
            Ex::Macro => {
                let callees = self.callees(d, sc);
                if callees.is_empty() {
                    self.var_ref(sc)
                } else {
                    let callee = self.rng.pick(&callees).clone();
                    self.macro_call(d, sc, &callee)
                }
            }
            Ex::Source => {
                let (t, _) = self.template_ref(d, sc, Use::Source);
                format!("source({t}, ignore_missing = true)|length")
            }
            Ex::Concat => {
                let a = self.expr(d, sc, depth + 1);
                let b = self.expr(d, sc, depth + 1);
                format!("{a} ~ {b}")
            }
            Ex::Ternary => {
                let c = self.cond(d, sc);
                let a = self.expr(d, sc, depth + 1);
                let b = self.expr(d, sc, depth + 1);
                format!("({c} ? {a} : {b})")
            }
            Ex::Default => {
                let a = self.var_ref(sc);
                let b = self.expr(d, sc, depth + 1);
                if self.rng.chance(50) {
                    format!("({} ?? {b})", simple_name(&a))
                } else {
                    format!("{a}|default({b})")
                }
            }
            Ex::Keys => "_context|keys|join(',')".to_owned(),
            Ex::Defined => {
                if !self.set_vars.is_empty() && self.rng.chance(50) {
                    // set somewhere else: visible here or not
                    let v = self.rng.pick(&self.set_vars).clone();
                    return if self.rng.chance(50) {
                        format!("({v} is defined ? 'D' : 'U')")
                    } else {
                        format!("({v} ?? 'U')")
                    };
                }
                let v = self.var_ref(sc);
                format!("({} is defined ? 'D' : 'U')", simple_name(&v))
            }
            Ex::Wrapper => self.wrapper_expr(d, sc),
            Ex::FromString => {
                let source = *self.rng.pick(&[
                    "{{ s }}",
                    "[{{ _self }}]",
                    "{% block a %}A{% endblock %}",
                    "{{ g|default('-') }}",
                    "{{ 1 // 0 }}",
                    "{{ undef_in_string }}",
                    "{% include 'missing.twig' %}",
                    "x{{ block('nope') }}",
                ]);
                if self.rng.chance(20) {
                    format!(
                        "include(template_from_string({}, {}))",
                        quote(source),
                        quote(self.rng.pick_str(&["named.twig", "x.js.twig", ""]))
                    )
                } else {
                    format!("include(template_from_string({}))", quote(source))
                }
            }
        }
    }

    fn maybe_filter(&mut self, e: String) -> String {
        if !self.rng.chance(20) {
            return e;
        }
        // string filters fail on arrays: only where the expression is likely a scalar
        let arrayish = e.starts_with('[')
            || e.starts_with('{')
            || [
                "list", "map", "deep", "num", "gl", "varargs", "rest", "items", "_context", "iter",
            ]
            .iter()
            .any(|v| e == *v);
        let f = if arrayish {
            *self
                .rng
                .pick(&["length", "json_encode", "default('-')", "keys|join(',')"])
        } else {
            *self.rng.pick(&[
                "upper",
                "e",
                "raw",
                "length",
                "trim",
                "lower",
                "escape('js')",
                "json_encode",
                "default('-')",
                "striptags",
            ])
        };
        let e = if e.contains(' ') && !e.starts_with('(') {
            format!("({e})")
        } else {
            e
        };
        format!("{e}|{f}")
    }

    fn var_ref(&mut self, sc: &Scope) -> String {
        let mut pool: Vec<String> = sc.vars.clone();
        if sc.data {
            pool.extend(self.data_keys());
        }
        pool.extend(self.globals.iter().map(|(k, _)| k.clone()));
        if sc.in_loop {
            pool.push("loop.index".to_owned());
            pool.push("loop.first ? 'F' : ''".to_owned());
        }
        if pool.is_empty() {
            return if self.rng.chance(20) {
                "undef".to_owned()
            } else {
                self.literal()
            };
        }
        if self.rng.chance(2) {
            return (*self.rng.pick(&["undef", "s", "varargs"])).to_owned();
        }
        let v = self.rng.pick(&pool).clone();
        if v.contains(' ') {
            return format!("({v})");
        }
        match (v.as_str(), self.rng.below(4)) {
            ("map", 0) => "map.a".to_owned(),
            ("map", 1) => "map['b']".to_owned(),
            ("deep", 0 | 1) => "deep.k.k2".to_owned(),
            ("list", 0) => "list[1]".to_owned(),
            ("list" | "gl" | "varargs" | "rest" | "items", 1) => format!("{v}|join(',')"),
            ("list" | "gl" | "varargs" | "rest" | "items" | "map", 2) => format!("{v}|length"),
            ("num", 0) => "num[5]".to_owned(),
            ("varargs" | "rest", 3) => format!("{v}|json_encode"),
            _ => v,
        }
    }

    fn literal(&mut self) -> String {
        (*self.rng.pick(&[
            "'lit'",
            "\"<q>\"",
            "1",
            "-2.5",
            "true",
            "null",
            "[1, 'two']",
            "{k: 'v'}",
            "0",
            "''",
            "'é'",
        ]))
        .to_owned()
    }

    fn cond(&mut self, d: &mut Doc, sc: &Scope) -> String {
        let choice = self.rng.below(12);
        if !sc.data && matches!(choice, 0..=2 | 5 | 8) {
            return match self.rng.below(4) {
                0 => "true".to_owned(),
                1 => "false".to_owned(),
                _ => {
                    let v = self.var_ref(sc);
                    format!("{} is defined", simple_name(&v))
                }
            };
        }
        match choice {
            0 => "yes".to_owned(),
            1 => "no".to_owned(),
            2 => "n > 1".to_owned(),
            3 => {
                let v = self.var_ref(sc);
                format!("{} is defined", simple_name(&v))
            }
            4 => {
                let names = d.all_blocks();
                let name = if names.is_empty() {
                    "nope".to_owned()
                } else {
                    self.rng.pick(&names).clone()
                };
                format!("block('{name}') is defined")
            }
            5 => "list|length > 2".to_owned(),
            6 => "true".to_owned(),
            7 => "false".to_owned(),
            8 => "s is not empty".to_owned(),
            9 => {
                let callees = self.callees(d, sc);
                match callees.iter().find(|c| c.head.contains('.')) {
                    Some(c) => format!("{} is defined", c.head),
                    None => "not no".to_owned(),
                }
            }
            _ => {
                let v = self.var_ref(sc);
                if v.contains(' ') { format!("({v})") } else { v }
            }
        }
    }

    fn sequence(&mut self, sc: &Scope) -> String {
        let mut options = vec!["[1, 2]", "range(1, 2)", "{a: 1, b: 2}", "[]"];
        if sc.data {
            options.extend(["list", "map"]);
        }
        (*self.rng.pick(&options)).to_owned()
    }

    fn wrapper_expr(&mut self, d: &mut Doc, sc: &Scope) -> String {
        let candidates = self.lower(|_| true);
        if candidates.is_empty() {
            return self.var_ref(sc);
        }
        let t = *self.rng.pick(&candidates);
        let name = self.tpls[t].name.clone();
        let blocks = self.tpls[t].blocks.clone();
        let w = if !sc.data {
            format!("include({})", quote(&name))
        } else if self.rng.chance(90) {
            self.wrapper_var(&name)
        } else {
            // `include()` returns Markup, not a wrapper: method calls fail
            format!("include({})", quote(&name))
        };
        let limit = sc.block.as_deref().map_or(BLOCKS.len(), block_rank);
        let limit = if sc.in_macro.is_some() { 1 } else { limit };
        let callable: Vec<String> = blocks
            .iter()
            .filter(|b| block_rank(b) < limit)
            .cloned()
            .collect();
        let block = if callable.is_empty() || self.rng.chance(10) {
            "nope".to_owned()
        } else {
            self.rng.pick(&callable).clone()
        };
        let _ = d;
        match self.rng.below(12) {
            0 => format!("{w}.render({{s: 'R'}})"),
            1 => format!("{w}.renderBlock('{block}')"),
            2 => format!("({w}.hasBlock('{block}') ? 'H' : 'h')"),
            3 => format!("{w}.blockNames|join(',')"),
            4 => format!("{w}.templateName"),
            5 => format!("{w}.render"),
            6 => format!("{w}.renderBlock('{block}', {{s: 'ctx', g: 'cg'}})"),
            7 => format!("({w}.hasBlock('{block}', {{}}) ? 'H' : 'h')"),
            8 => format!("{w}.render(_context)|length"),
            9 => format!("{w}.getBlockNames({{n: 3}})|join(',')"),
            10 => match self.rng.below(6) {
                0 => format!("{w}.sourceContext.name"),
                1 => format!("{w}.sourceContext.code|length"),
                2 => format!("{w}.defaultEscapeStrategy|json_encode"),
                3 => format!("{w}.render('x')"),
                4 => format!("{w}.renderBlock(5)"),
                _ => format!("{w}.sourceContext.path|json_encode"),
            },
            _ => format!("{w}.render({{}})|length"),
        }
    }

    /// A template expression for `purpose` and the generated templates it may name.
    fn template_ref(&mut self, d: &Doc, sc: &Scope, purpose: Use) -> (String, Vec<usize>) {
        let candidates: Vec<usize> = match purpose {
            Use::Extends | Use::Embed => {
                let with_blocks = self.lower(|t| {
                    matches!(t.kind, Kind::Layout | Kind::Partial) && !t.blocks.is_empty()
                });
                if with_blocks.is_empty() || self.rng.chance(5) {
                    self.lower(|_| true)
                } else {
                    with_blocks
                }
            }
            Use::BlockOf => {
                let with_blocks = self.lower(|t| !t.blocks.is_empty());
                if with_blocks.is_empty() {
                    self.lower(|_| true)
                } else {
                    with_blocks
                }
            }
            Use::Include | Use::Source => {
                let preferred = self.lower(|t| matches!(t.kind, Kind::Partial | Kind::Layout));
                if preferred.is_empty() || self.rng.chance(15) {
                    self.lower(|_| true)
                } else {
                    preferred
                }
            }
        };
        if candidates.is_empty() || self.rng.chance(1) {
            let missing = *self.rng.pick(MISSING);
            return match self.rng.below(8) {
                0 if purpose == Use::Source => (quote(&d.name), Vec::new()),
                1 => (
                    (*self.rng.pick(&["null", "false", "5", "''", "[]"])).to_owned(),
                    Vec::new(),
                ),
                _ => (quote(missing), Vec::new()),
            };
        }
        let t = *self.rng.pick(&candidates);
        let name = self.tpls[t].name.clone();
        if self.rng.below(150) == 0 {
            // evaluating the name throws (in getParent(), load(), ...)
            return (format!("(1 // 0 ? {} : 'x')", quote(&name)), vec![t]);
        }
        let mut form = self.rng.below(24);
        // data variables are not visible in macros and after `only`; source() takes one name
        if (!sc.data && matches!(form, 14 | 15 | 17 | 20))
            || (purpose == Use::Source && matches!(form, 16 | 17 | 20 | 22 | 23))
        {
            form = 0;
        }
        match form {
            0..=13 => (quote(&name), vec![t]),
            14 | 15 => {
                if t == 0 && self.globals.iter().any(|(k, _)| k == "gtpl") && self.rng.chance(50) {
                    // the global naming the first template
                    return ("gtpl".to_owned(), vec![0]);
                }
                let var = self.data_var("tn", Json::str(&name));
                (var, vec![t])
            }
            16 => (
                format!("[{}, {}]", quote(self.rng.pick_str(MISSING)), quote(&name)),
                vec![t],
            ),
            17 => {
                let missing = *self.rng.pick(MISSING);
                let var = self.data_var(
                    "tl",
                    Json::Array(vec![Json::str(missing), Json::str(&name)]),
                );
                (var, vec![t])
            }
            18 => {
                let other = *self.rng.pick(&candidates);
                let c = if sc.data {
                    *self.rng.pick(&["yes", "no", "n > 1", "s"])
                } else {
                    *self.rng.pick(&["true", "false"])
                };
                (
                    format!(
                        "({c} ? {} : {})",
                        quote(&name),
                        quote(&self.tpls[other].name)
                    ),
                    vec![t, other],
                )
            }
            19 if name.len() > 2 => {
                let (a, b) = name.split_at(name.len() / 2);
                (format!("{} ~ {}", quote(a), quote(b)), vec![t])
            }
            20 => {
                if self.rng.chance(25) {
                    (self.markup_var(&name), vec![t])
                } else {
                    (self.wrapper_var(&name), vec![t])
                }
            }
            21 if purpose == Use::BlockOf => ("_self".to_owned(), Vec::new()),
            22 if self.string_loader && purpose != Use::BlockOf => {
                let source = *self.rng.pick(&[
                    "{% block a %}[fs-a]{% endblock %}{% block b %}[fs-b]{% endblock %}",
                    "<{{ s }}>",
                    "{% block c %}{{ _self }}{% endblock %}",
                ]);
                if self.rng.chance(30) {
                    (
                        format!("template_from_string({}, 'named.twig')", quote(source)),
                        Vec::new(),
                    )
                } else {
                    (
                        format!("template_from_string({})", quote(source)),
                        Vec::new(),
                    )
                }
            }
            23 => {
                let odd = match self.rng.below(9) {
                    0 => format!("[5, {}]", quote(&name)),
                    1 => format!("[null, {}]", quote(&name)),
                    2 => format!("[true, {}]", quote(&name)),
                    3 => format!("[['x'], {}]", quote(&name)),
                    4 => format!("[{}]", quote(self.rng.pick_str(MISSING))),
                    5 => format!("[{}, {}]", quote(MISSING[0]), quote(MISSING[1])),
                    6 => format!("[{}, {}]", quote(&name), quote(MISSING[0])),
                    7 => format!("{{a: {}, b: {}}}", quote(MISSING[1]), quote(&name)),
                    _ => format!("[{}]", quote(&name)),
                };
                (odd, vec![t])
            }
            _ => (quote(&name), vec![t]),
        }
    }
}

/// What a template expression is used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    Extends,
    Embed,
    Include,
    Source,
    BlockOf,
}

/// The tags, filters and functions the sources use (by their syntax; good enough for a policy).
fn used_names(sources: &[&str]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut tags: Vec<String> = Vec::new();
    let mut filters: Vec<String> = Vec::new();
    let mut functions: Vec<String> = Vec::new();
    let add = |list: &mut Vec<String>, name: String| {
        if !name.is_empty() && !list.contains(&name) {
            list.push(name);
        }
    };
    let word = |s: &str| -> String {
        s.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect()
    };
    for source in sources {
        let bytes = source.as_bytes();
        for (i, c) in source.char_indices() {
            match c {
                '%' if i > 0 && bytes[i - 1] == b'{' => {
                    let rest = source[i + 1..].trim_start_matches(['-', '~']).trim_start();
                    let tag = word(rest);
                    // end tags and `else` belong to their opening tag
                    if !tag.starts_with("end") && tag != "else" && tag != "elseif" {
                        add(&mut tags, tag);
                    }
                }
                '|' => add(&mut filters, word(source[i + 1..].trim_start())),
                '(' => {
                    let before = &source[..i];
                    let name: String = before
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect::<Vec<char>>()
                        .into_iter()
                        .rev()
                        .collect();
                    let preceded = before[..before.len() - name.len()].chars().last();
                    if !name.is_empty()
                        && !name.starts_with(|c: char| c.is_ascii_digit())
                        && preceded != Some('.')
                    {
                        add(&mut functions, name);
                    }
                }
                _ => {}
            }
        }
    }
    (tags, filters, functions)
}

/// Spreads the tags written since `start` over several lines: a line break after the opening
/// delimiter of the first tag and after the commas of argument lists and hashes (string literals
/// are left alone).
fn spread_lines(out: &mut String, start: usize) {
    let written = out.split_off(start);
    let mut result = String::with_capacity(written.len() + 8);
    let mut quote: Option<char> = None;
    let mut first_tag = true;
    let mut chars = written.chars().peekable();
    while let Some(c) = chars.next() {
        result.push(c);
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                '{' if first_tag && matches!(chars.peek(), Some('{' | '%')) => {
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                    // whitespace control belongs to the delimiter
                    if let Some(&modifier @ ('-' | '~')) = chars.peek() {
                        chars.next();
                        result.push(modifier);
                    }
                    result.push('\n');
                    first_tag = false;
                }
                ',' if chars.peek() == Some(&' ') => {
                    chars.next();
                    result.push('\n');
                }
                _ => {}
            },
        }
    }
    out.push_str(&result);
}

/// Adds whitespace control (`-` or `~`; a space adds nothing) to the first opening and the last
/// closing delimiter written since `start`, unless they have one.
fn whitespace_control(out: &mut String, start: usize, open: char, close: char) {
    if close != ' ' {
        let last = out[start..]
            .rfind("%}")
            .max(out[start..].rfind("}}"))
            .map(|i| i + start);
        if let Some(i) = last
            && i > start
            && !out[..i].ends_with(['-', '~', '{'])
        {
            out.insert(i, close);
        }
    }
    if open != ' ' {
        let first = match (out[start..].find("{%"), out[start..].find("{{")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(i) = first.map(|i| i + start + 2)
            && !out[i..].starts_with(['-', '~', '}', '%'])
        {
            out.insert(i, open);
        }
    }
}

/// The variable part of a reference (`map.a|length` -> `map.a`) for `is defined`.
fn simple_name(reference: &str) -> String {
    let name: String = reference
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    if name.is_empty() || name.ends_with('.') {
        "undef".to_owned()
    } else {
        name
    }
}

/// A Twig string literal.
fn quote(s: &str) -> String {
    if !s.contains('\'') {
        format!("'{s}'")
    } else {
        format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace("#{", "\\#{")
        )
    }
}

/// A PHP single-quoted string literal.
pub(super) fn php_string(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}
