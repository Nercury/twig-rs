//! Twig templates for Rust, rendered exactly like [Twig](https://twig.symfony.com/) 3.31.1 renders them on
//! PHP 8.5.13: the same output and the same error messages, so that templates work copy-pasted from PHP projects.
//!
//! ```
//! use twig::{Environment, context, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([(
//!     "index",
//!     "{% for user in users %}Hello {{ user.name|upper }}!{% endfor %}",
//! )]));
//! let users = vec![context! { name => "fabien" }];
//! assert_eq!(env.render("index", context! { users })?, "Hello FABIEN!");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! This page is the guide:
//!
//! - [Compatibility](#compatibility): what "exactly like Twig" covers;
//! - [Quick start](#quick-start): environments, options and rendering;
//! - [Template data](#template-data): contexts, [`context!`], conversions of Rust data, serde, globals;
//! - [Autoescaping](#autoescaping);
//! - [Extending Twig](#extending-twig): [filters, functions and tests](#filters-functions-and-tests),
//!   [extensions](#extensions), [tags](#tags);
//! - [Loaders](#loaders);
//! - [Sandbox](#sandbox);
//! - [Errors and diagnostics](#errors-and-diagnostics);
//! - [Thread safety](#thread-safety);
//! - [How it works](#how-it-works): the pipeline and the modules.
//!
//! The `examples/` directory of the repository has complete programs for most sections: `hello`, `filesystem`,
//! `custom_filter`, `extension`, `sandbox` and `serde_context` (`cargo run --example hello`).
//!
//! # Compatibility
//!
//! twig-rs targets one configuration: **Twig 3.31.1 on PHP 8.5.13** (64-bit, with PHP's built-in ini defaults such
//! as `precision=14`, `serialize_precision=-1` and `date.timezone=UTC`) and the libraries PHP bundles: PCRE2 10.44
//! for the `matches` operator, timelib 2022.17 with timezonedb 2026.6 for dates, and mbstring's Unicode 17.0 tables
//! for case mapping. For a template, "the same" means the same output bytes, the same error class, message,
//! template name and line, and the same PHP warnings and deprecations. PHP running Twig in Docker is the reference:
//! all runnable cases of Twig's integration fixtures pass, Twig's behaviour-level PHPUnit tests are ported, and
//! differential fuzzing compares the two on generated templates and data.
//!
//! What cannot or deliberately does not match is listed, with what twig-rs does instead, in
//! [`reference/deviations.md`](https://github.com/Nercury/twig-rs/blob/master/reference/deviations.md): messages
//! that would show PHP internals (generated class files, closure file names, object handle numbers), unseeded
//! randomness, recursion limits, the charsets twig-rs does not convert yet, and host APIs that have no PHP
//! equivalent. Any other difference is a bug.
//!
//! Charset conversions (the `convert_encoding` filter, escaping under a `charset` other than UTF-8, and `reverse`,
//! `shuffle` and `random` under such a charset) are PHP's `iconv()`, which an environment's
//! [`iconv_backend`](Environment::iconv_backend) implements. The default backend converts UTF-8, UTF-16, UTF-32,
//! UCS-2, UCS-4 (with their byte-order variants), ASCII and ISO-8859-1 exactly like PHP on glibc 2.41, and fails with
//! `iconv(): conversion of charset "X" is not supported by twig-rs` for the other charsets glibc knows. The
//! **`glibc-iconv`** cargo feature makes the backend of the `twig-iconv` crate the default instead: it converts nearly
//! every charset glibc does (single-byte codepages, the CJK charsets, the ISO-2022 family). That crate follows glibc's
//! sources and is licensed under the GNU LGPL 2.1 or later, unlike the rest of twig-rs, so the feature puts LGPL code
//! into the program. [`EnvironmentOptions::iconv_backend`] chooses a backend for one environment.
//!
//! There is no PHP interpreter: data is passed as PHP values made from Rust data, and filters, functions, tests,
//! extensions, loaders and security policies are Rust code. What needs PHP code is not available: PHP functions
//! named as callables (`'strtoupper'`), and node classes that compile themselves (a tag builds its node from the
//! core node types instead, see [Tags](#tags)).
//!
//! # Quick start
//!
//! An [`Environment`] holds the options, the extensions and globals, a [loader] that finds template sources by
//! name, and the templates it compiled. Create one per configuration and keep it: each template is lexed,
//! parsed and compiled once per environment (in memory: Twig's `cache` option has no equivalent), then rendered as
//! often as needed. [`EnvironmentOptions`] are Twig's options with Twig's defaults.
//!
//! ```
//! use twig::loader::ArrayLoader;
//! use twig::{Environment, EnvironmentOptions, context};
//!
//! let loader = ArrayLoader::new([
//!     ("base.html", "<title>{% block title %}{% endblock %}</title>{% block body %}{% endblock %}"),
//!     (
//!         "news.html",
//!         "{% extends 'base.html' %}{% block title %}{{ title }}{% endblock %}\
//!          {% block body %}{% for item in items %}<p>{{ item }}</p>{% endfor %}{% endblock %}",
//!     ),
//! ]);
//! let options = EnvironmentOptions { strict_variables: true, ..Default::default() };
//! let env = Environment::with_options(loader, options);
//!
//! let html = env.render("news.html", context! { title => "News", items => ["a", "b"] })?;
//! assert_eq!(html, "<title>News</title><p>a</p><p>b</p>");
//!
//! // A loaded template renders without asking the loader again, also one block at a time.
//! let news = env.load("news.html")?;
//! assert_eq!(news.render_block("title", context! { title => "Draft" })?, "Draft");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! [`render`](Environment::render) returns a `String`. Twig's output is a byte string, though:
//! [`render_bytes`](Environment::render_bytes) returns the bytes (a template may print bytes that are not UTF-8),
//! and [`display`](Environment::display) writes them to a [`Sink`](vm::Sink) as they are produced: a `Vec<u8>`, or
//! any [`std::io::Write`] wrapped in an [`IoSink`](vm::IoSink). [`create_template`](Environment::create_template)
//! compiles template source that no loader knows (Twig's `createTemplate()`).
//!
//! ```
//! use twig::{Array, vm::IoSink};
//! # let env = twig::Environment::new(twig::loader::ArrayLoader::new([("index", "")]));
//!
//! let template = env.create_template(b"{% for i in 1..3 %}{{ i }}{% endfor %}", None)?;
//! // a file, a socket, a locked stdout, ...
//! let mut writer = std::io::Cursor::new(Vec::new());
//! template.display(Array::new(), &mut IoSink(&mut writer))?;
//! assert_eq!(writer.into_inner(), b"123");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! # Template data
//!
//! A template's context is a PHP array: an [`Array`] (ordered, with PHP's key rules) of [`Value`]s (`null`, bool,
//! int, float, byte string, array, object). Rust data converts with `From`:
//!
//! - scalars and strings: `bool`, the integer types, `f32` and `f64`, `char`, `&str`, `String`, `Cow<str>`; byte
//!   strings: `&[u8]`, `Vec<u8>`, [`PhpString`](value::PhpString); `Option<T>`, where `None` is `null`;
//! - lists: `Vec<T>`, `&[T]`, `[T; N]`;
//! - arrays with keys: `HashMap`, `BTreeMap`, `IndexMap` and `[(K, V); N]`, with PHP's key normalisation (`"1"`
//!   becomes the integer key `1`).
//!
//! Integers beyond PHP's `int` range become floats, as `json_decode()` makes them; [`value::value`] has the whole
//! table. The [`context!`] macro builds an `Array` of `key => value` entries, and contexts nest:
//!
//! ```
//! use std::collections::BTreeMap;
//! use twig::{Environment, context, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([(
//!     "index",
//!     "{{ user.name }} ({{ user.roles|join(', ') }}): \
//!      {% for key, score in scores %}{{ key }}={{ score }} {% endfor %}{{ next ?? 'none' }}",
//! )]));
//! let scores = BTreeMap::from([("a", 1.5), ("b", 2.0)]);
//! let context = context! {
//!     user => context! { name => "Fabien", roles => ["admin", "dev"] },
//!     scores, // shorthand for `scores => scores`
//!     next => None::<String>,
//! };
//! assert_eq!(env.render("index", context)?, "Fabien (admin, dev): a=1.5 b=2 none");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! With the `serde` feature, `twig::to_context` makes a context of anything that implements serde's `Serialize` and
//! serializes to a struct or a map (its fields become the variables), and `twig::to_value` makes a single value:
//! structs and maps become arrays, sequences lists, `None` and unit `null`, and enums follow serde_json's external
//! tagging (the `serde` module of [`value`] has the mapping).
//!
//! ```
//! # #[cfg(feature = "serde")]
//! # fn main() -> twig::Result<()> {
//! #[derive(serde::Serialize)]
//! struct Page {
//!     title: &'static str,
//!     tags: Vec<&'static str>,
//!     author: Option<&'static str>,
//! }
//!
//! let env = twig::Environment::new(twig::loader::ArrayLoader::new([(
//!     "page",
//!     "{{ title }}: {{ tags|join(', ') }} by {{ author ?? 'nobody' }}",
//! )]));
//! let page = Page { title: "News", tags: vec!["a", "b"], author: None };
//! assert_eq!(env.render("page", twig::to_context(&page)?)?, "News: a, b by nobody");
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "serde"))]
//! # fn main() {}
//! ```
//!
//! Globals are variables every template sees: [`Environment::add_global`] adds one (until the first template is
//! loaded; afterwards only existing globals change), an extension's [`globals`](extension::Extension::globals)
//! several. HTML that must not be escaped again is passed as [`Markup`] (see [Autoescaping](#autoescaping)). Host
//! objects implement [`value::object::Object`]: a class with methods and properties as templates see them
//! (`article.title` calls `getTitle()`, as in PHP); `examples/sandbox.rs` has one.
//!
//! # Autoescaping
//!
//! As in Twig, printed expressions are escaped for HTML by default. The `autoescape` option
//! ([`EnvironmentOptions::autoescape`]) sets the default strategy: `"html"`, `"js"`, `"css"`, `"url"`,
//! `"html_attr"`, `"name"` (chosen by the template name: `.js.twig` and `.json.twig` are escaped for JavaScript,
//! `.css.twig` for CSS, `.txt.twig` not at all, everything else for HTML), a callback of the template name, or
//! [`DefaultStrategy::Disabled`](escaper::DefaultStrategy::Disabled). Templates switch strategies with
//! `{% autoescape 'js' %}`, escape explicitly with the `escape` filter (`|e('url')`) and opt out with `|raw`.
//!
//! What is safe already is not escaped again: [`Markup`] values (Twig's `Twig\Markup`), the output of filters and
//! functions declared safe for the strategy (their `is_safe` option, see
//! [Filters, functions and tests](#filters-functions-and-tests)), macro calls, `include()` and captured blocks.
//! Custom strategies are closures registered on the environment's
//! [`escaper_runtime`](Environment::escaper_runtime).
//!
//! ```
//! use twig::value::PhpString;
//! use twig::{Environment, Markup, context, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([(
//!     "index",
//!     "{{ text }} {{ text|raw }} {{ html }} <a href=\"?q={{ text|e('url') }}\"> \
//!      <script>let text = '{% autoescape 'js' %}{{ text }}{% endautoescape %}';</script>",
//! )]));
//! let context = context! {
//!     text => "<b>R&D</b>",
//!     html => Markup::new_value(PhpString::from("<i>safe</i>"), "UTF-8"),
//! };
//! assert_eq!(
//!     env.render("index", context)?,
//!     "&lt;b&gt;R&amp;D&lt;/b&gt; <b>R&D</b> <i>safe</i> <a href=\"?q=%3Cb%3ER%26D%3C%2Fb%3E\"> \
//!      <script>let text = '\\u003Cb\\u003ER\\u0026D\\u003C\\/b\\u003E';</script>",
//! );
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! # Extending Twig
//!
//! ## Filters, functions and tests
//!
//! [`TwigFilter::from_fn`], [`TwigFunction::from_fn`] and [`TwigTest::from_fn`] make a callable of a Rust closure
//! and the list of its template parameters. Twig's options are builder methods (`is_safe`, `pre_escape`,
//! `needs_context`, `needs_environment`, `is_variadic`, `deprecation_info`, ...), and `build()` makes the callable.
//! Register it with [`Environment::add_filter`], [`add_function`](Environment::add_function) or
//! [`add_test`](Environment::add_test), or from an extension, before the first template is loaded.
//!
//! The parameters behave like those of a PHP closure: templates pass arguments by position or by name, omitted
//! ones take their default, and unknown or missing ones fail at compile time with Twig's messages. The closure gets
//! the [`Call`](extension::Call) (PHP diagnostics, and the context, environment or charset a callable asks for) and
//! the arguments: for a filter or a test the input first, then one value per parameter. It returns anything that
//! converts into a [`Value`]; `?` turns PHP errors and Twig errors into the exception the template sees.
//!
//! ```
//! use twig::extension::{CallableParameter, TwigFilter, TwigFunction, TwigTest};
//! use twig::value::convert;
//! use twig::{Environment, context, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([(
//!     "index",
//!     "{{ name|shout(times: 3) }} {{ greet('Bob') }} {{ 7 is prime ? 'prime' }}",
//! )]));
//! env.add_filter(
//!     TwigFilter::from_fn("shout", [CallableParameter::optional("times", 1.into())], |call, args| {
//!         let [input, times] = args else { unreachable!("the input, then one value per parameter") };
//!         // PHP's conversions, with PHP's warnings and errors
//!         let input = convert::to_string(input, call.diagnostics())?.to_string_lossy().to_uppercase();
//!         let times = convert::to_long(times, call.diagnostics())?;
//!         Ok(input + &"!".repeat(times.try_into().unwrap_or(0)))
//!     })
//!     .build(),
//! )?;
//! env.add_function(
//!     TwigFunction::from_fn("greet", ["name".into()], |call, args| {
//!         Ok(format!("Hello {}", convert::to_string(&args[0], call.diagnostics())?))
//!     })
//!     .build(),
//! )?;
//! env.add_test(
//!     TwigTest::from_fn("prime", [], |call, args| {
//!         let n = convert::to_long(&args[0], call.diagnostics())?;
//!         Ok(n > 1 && (2..n).take_while(|d| d * d <= n).all(|d| n % d != 0))
//!     })
//!     .build(),
//! )?;
//! assert_eq!(env.render("index", context! { name => "hey" })?, "HEY!!! Hello Bob prime");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! [`TwigFilter::from_fn`] details the binding of arguments; `examples/custom_filter.rs` has filters with defaults,
//! HTML output and the template context. The lower-level [`extension::runtime`] API attaches an implementation to a
//! PHP callable described explicitly.
//!
//! ## Extensions
//!
//! An [`Extension`](extension::Extension) packages filters, functions, tests, tags, node visitors, operators and
//! globals, registered at once with [`Environment::add_extension`]. Every method but `as_any` has a default, so an
//! extension implements only what it contributes:
//!
//! ```
//! use std::any::Any;
//! use std::sync::Arc;
//! use twig::extension::{Extension, TwigFilter};
//! use twig::value::convert;
//! use twig::{Array, Environment, context, loader::ArrayLoader};
//!
//! struct SiteExtension;
//!
//! impl Extension for SiteExtension {
//!     fn filters(&self) -> Vec<TwigFilter> {
//!         let reverse_words = TwigFilter::from_fn("reverse_words", [], |call, args| {
//!             let text = convert::to_string(&args[0], call.diagnostics())?;
//!             Ok(text.to_string_lossy().split(' ').rev().collect::<Vec<_>>().join(" "))
//!         });
//!         vec![reverse_words.build()]
//!     }
//!
//!     fn globals(&self) -> Option<Array> {
//!         Some(context! { site => "Example" })
//!     }
//!
//!     fn as_any(&self) -> &dyn Any {
//!         self
//!     }
//! }
//!
//! let env = Environment::new(ArrayLoader::new([("index", "{{ site }}: {{ 'a b c'|reverse_words }}")]));
//! env.add_extension(Arc::new(SiteExtension))?;
//! assert_eq!(env.render("index", Array::new())?, "Example: c b a");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! Extensions are keyed by their [`class_name`](extension::Extension::class_name) (by default the Rust type name):
//! an extension is registered once, and [`Environment::runtime`] finds it again.
//!
//! ## Tags
//!
//! A tag is a [`TokenParser`](token_parser::TokenParser). When the parser meets `{% name`, it hands the token
//! stream, positioned after the name, to the token parser registered for `name`; the token parser consumes the rest
//! of the tag (and, for a tag with a body, everything up to its end tag) and returns a node. The node is made of the
//! core node types, built with the constructors of [`ast::Node`] and [`ast::Expr`] (`Node::print`, `Node::if_`,
//! `Node::set`, `Expr::binary`, `Expr::filter`, ...), so the node visitors (escaping, sandbox, optimizer) and the
//! compiler handle it like Twig's own tags; twig-rs cannot compile node types defined by extensions yet.
//!
//! ```
//! use std::sync::Arc;
//! use twig::ast::{BinaryOp, Expr, Node};
//! use twig::extension::TokenParser;
//! use twig::lexer::{Token, TokenType};
//! use twig::parser::Parser;
//! use twig::{Environment, context, loader::ArrayLoader};
//!
//! /// `{% greet expr %}` prints `{{ 'Hello ' ~ expr ~ '!' }}`.
//! struct GreetTokenParser;
//!
//! impl TokenParser for GreetTokenParser {
//!     fn tag(&self) -> &str {
//!         "greet"
//!     }
//!
//!     fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
//!         let line = token.line();
//!         let name = parser.parse_expression(0)?;
//!         parser.stream_mut().expect(TokenType::BlockEnd, None, None)?;
//!         let hello = Expr::binary(BinaryOp::Concat, Expr::constant("Hello ", line), name, line)?;
//!         let greeting = Expr::binary(BinaryOp::Concat, hello, Expr::constant("!", line), line)?;
//!         Ok(Some(Node::print(greeting, line)))
//!     }
//! }
//!
//! let env = Environment::new(ArrayLoader::new([("index", "{% greet user %}")]));
//! env.add_token_parser(Arc::new(GreetTokenParser))?;
//! // printed like `{{ }}`, so auto-escaped
//! assert_eq!(env.render("index", context! { user => "<you>" })?, "Hello &lt;you&gt;!");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! For a body, [`Parser::subparse`](parser::Parser::subparse) parses up to the end tag; `examples/extension.rs` has
//! an `{% unless %}...{% else %}...{% endunless %}` tag registered by an extension.
//!
//! # Loaders
//!
//! A [loader] turns template names into sources. [`ArrayLoader`](loader::ArrayLoader) holds templates in
//! memory; [`FilesystemLoader`](loader::FilesystemLoader) reads them from directories, with namespaces
//! (`@admin/index.html.twig`) and Twig's name normalisation and checks (no name gets out of its directories; see
//! `examples/filesystem.rs`); [`ChainLoader`](loader::ChainLoader) asks several loaders in turn. Other sources
//! implement [`Loader`](loader::Loader). The environment keeps a compiled template under the loader's cache key, so
//! a key that changes with the source makes it compile the new source.
//!
//! ```
//! use std::sync::Arc;
//! use twig::loader::{ArrayLoader, ChainLoader, Loader};
//! use twig::{Array, Environment, Error, Result, Source};
//!
//! /// Templates from a database table, say.
//! struct Database;
//!
//! impl Database {
//!     fn code(&self, name: &str) -> Result<&'static str> {
//!         match name {
//!             "db/greeting" => Ok("Hello from the database"),
//!             _ => Err(Error::loader(format!("Template \"{name}\" is not defined."))),
//!         }
//!     }
//! }
//!
//! impl Loader for Database {
//!     fn get_source_context(&self, name: &str) -> Result<Source> {
//!         Ok(Source::new(self.code(name)?, name))
//!     }
//!
//!     fn get_cache_key(&self, name: &str) -> Result<Vec<u8>> {
//!         Ok(format!("{name}:{}", self.code(name)?).into_bytes())
//!     }
//!
//!     fn is_fresh(&self, name: &str, _time: i64) -> Result<bool> {
//!         self.code(name).map(|_| true)
//!     }
//!
//!     fn exists(&self, name: &str) -> bool {
//!         self.code(name).is_ok()
//!     }
//! }
//!
//! let env = Environment::new(ChainLoader::new([
//!     Arc::new(ArrayLoader::new([("index", "{% include 'db/greeting' %}.")])) as Arc<dyn Loader>,
//!     Arc::new(Database),
//! ]));
//! assert_eq!(env.render("index", Array::new())?, "Hello from the database.");
//! let error = env.render("missing", Array::new()).unwrap_err();
//! assert_eq!(error.to_string(), "Template \"missing\" is not defined.");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! # Sandbox
//!
//! Templates written by untrusted authors are rendered through a [`Sandbox`](runtime::Sandbox) (Twig 3.29's
//! `Twig\Sandbox\Sandbox`): an environment dedicated to them and a strict [`SecurityPolicy`](runtime::SecurityPolicy)
//! that allow-lists the tags, filters, functions, tests, methods and properties they may use. Anything else fails
//! with a sandbox error of its own kind ([`ErrorKind::SecurityNotAllowedFilter`], ...), whose names
//! [`SecurityError::of`](runtime::SecurityError::of) gives back.
//!
//! ```
//! use std::sync::Arc;
//! use twig::runtime::{Sandbox, SecurityPolicy};
//! use twig::{Environment, ErrorKind, context, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([
//!     ("list", "{% for item in items %}{{ item|upper }} {% endfor %}"),
//!     ("dump", "{{ items|json_encode }}"),
//! ]));
//! let policy = SecurityPolicy::new();
//! policy.set_allowed_tags(["for"]);
//! // auto-escaping applies the `escape` filter to every printed expression
//! policy.set_allowed_filters(["escape", "upper"]);
//! // Twig 4.0's rules, which `Sandbox` requires: nothing is allowed implicitly
//! policy.set_strict(true);
//! let sandbox = Sandbox::new(env, Arc::new(policy))?;
//!
//! assert_eq!(sandbox.render("list", context! { items => ["a", "b"] })?, "A B ");
//! let error = sandbox.render("dump", context! { items => ["a", "b"] }).unwrap_err();
//! assert_eq!(error.kind(), ErrorKind::SecurityNotAllowedFilter);
//! assert_eq!(error.to_string(), "Filter \"json_encode\" is not allowed in \"dump\" at line 1.");
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! [Twig's sandbox documentation](https://twig.symfony.com/doc/3.x/sandbox.html) applies, including what the
//! sandbox does not protect against: the policy limits what templates can reach, not the time and memory they use.
//! [`runtime::sandbox`] also covers the `SandboxExtension` setup that Twig 3.29 deprecated (`{% sandbox %}`,
//! `include(..., sandboxed = true)`); `examples/sandbox.rs` adds a host object with allowed methods.
//!
//! # Errors and diagnostics
//!
//! Every failure is an [`Error`]. Its [`kind`](Error::kind) is the PHP exception class it stands for
//! ([`ErrorKind::Syntax`], [`ErrorKind::Loader`], [`ErrorKind::Runtime`], the sandbox's), and it displays exactly like
//! Twig's `getMessage()`: the message with the template name and line. [`raw_message`](Error::raw_message),
//! [`template_line`](Error::template_line) and [`source_context`](Error::source_context) give the parts.
//!
//! A PHP throwable that is not a Twig error (a `DivisionByZeroError`, a `ValueError` of a PHP function, an
//! exception of a host object) is [`ErrorKind::Php`], and [`Error::php_error`] gives its class and message. Thrown
//! inside a template, it is wrapped the way Twig wraps it: a runtime error `An exception has been thrown during the
//! rendering of a template ("...")` with the throwable as its [`previous`](Error::previous) error.
//!
//! ```
//! use twig::{Array, Environment, EnvironmentOptions, ErrorKind, loader::ArrayLoader};
//!
//! let loader = ArrayLoader::new([
//!     ("user", "{% if user.admin %}\n{{ user.name }}\n{% endif %}"),
//!     ("division", "{{ 1 / 0 }}"),
//!     ("syntax", "{{ 1 + }}"),
//! ]);
//! let env = Environment::with_options(loader, EnvironmentOptions { strict_variables: true, ..Default::default() });
//!
//! let error = env.render("user", Array::new()).unwrap_err();
//! assert_eq!(error.kind(), ErrorKind::Runtime);
//! assert_eq!(error.to_string(), "Variable \"user\" does not exist in \"user\" at line 1.");
//! assert_eq!((error.raw_message(), error.template_line()), ("Variable \"user\" does not exist.", 1));
//!
//! let error = env.render("division", Array::new()).unwrap_err();
//! assert_eq!(
//!     error.to_string(),
//!     "An exception has been thrown during the rendering of a template (\"Division by zero\") in \"division\" \
//!      at line 1.",
//! );
//! let previous = error.previous().map(ToString::to_string);
//! assert_eq!(previous.as_deref(), Some("DivisionByZeroError: Division by zero"));
//!
//! let error = env.render("syntax", Array::new()).unwrap_err();
//! assert_eq!(error.kind(), ErrorKind::Syntax);
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! PHP warnings, notices and deprecations (`Array to string conversion`, Twig's own deprecations, those of
//! deprecated callables) do not stop PHP. twig-rs never prints them: the environment's
//! [`DiagnosticsPolicy`](runtime::DiagnosticsPolicy) ignores them (the default), records them (`Collect`; read them
//! with [`take_notices`](Environment::take_notices)), or turns warnings and notices into `ErrorException`s (`Error`,
//! what PHPUnit does in Twig's own test suite).
//!
//! ```
//! use twig::runtime::DiagnosticsPolicy;
//! use twig::{Array, Environment, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([("index", "{{ [1, 2] ~ '' }} {{ 'x'|spaceless }}")]));
//! env.set_diagnostics_policy(DiagnosticsPolicy::Collect);
//! assert_eq!(env.render("index", Array::new())?, "Array x");
//! let notices: Vec<String> = env.take_notices().iter().map(ToString::to_string).collect();
//! assert_eq!(
//!     notices,
//!     [
//!         // reported when the template is compiled
//!         "Deprecated: Since twig/twig 3.12: Twig Filter \"spaceless\" is deprecated in index at line 1.",
//!         // reported when it renders
//!         "Warning: Array to string conversion",
//!     ],
//! );
//!
//! env.set_diagnostics_policy(DiagnosticsPolicy::Error);
//! assert_eq!(
//!     env.render("index", Array::new()).unwrap_err().to_string(),
//!     "An exception has been thrown during the rendering of a template (\"Array to string conversion\") in \
//!      \"index\" at line 1.",
//! );
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! # Thread safety
//!
//! [`Environment`] is a cheap handle (`Clone + Send + Sync`): clones share the configuration and the compiled
//! templates, so one environment serves every thread of a server. Configure it before the first template is loaded:
//! from then on, extensions, filters, functions, tests, tags and new globals are refused, as in Twig. Values are
//! `Send + Sync` as well (strings and arrays are reference counted and copied on write).
//!
//! ```
//! use twig::{Environment, context, loader::ArrayLoader};
//!
//! let env = Environment::new(ArrayLoader::new([("square", "{{ n * n }}")]));
//! let workers: Vec<_> = (1..=4)
//!     .map(|n| {
//!         let env = env.clone();
//!         std::thread::spawn(move || env.render("square", context! { n }))
//!     })
//!     .collect();
//! let squares = workers
//!     .into_iter()
//!     .map(|worker| worker.join().expect("no panic"))
//!     .collect::<twig::Result<Vec<_>>>()?;
//! assert_eq!(squares, ["1", "4", "9", "16"]);
//! # Ok::<(), twig::Error>(())
//! ```
//!
//! Where PHP has per-request state, twig-rs keeps it per environment or per thread: `random()` and `shuffle` draw
//! from a generator per thread, `{% sandbox %}` and `include(..., sandboxed = true)` sandbox the current thread
//! only, and the date settings (default timezone, clock) belong to the environment.
//!
//! # How it works
//!
//! The pipeline follows Twig's: the lexer ([`lexer`]) and the parser ([`parser`], [`token_parser`]) build a syntax
//! tree mirroring Twig's node classes ([`ast`]); node visitors check and transform it ([`visitor`]: correctness
//! checks, auto-escaping, the optimizer, the sandbox's checks); the code generator lowers it into a program of the
//! low-level template language ([`lltl`]) in Twig's compile order; the virtual machine ([`vm`]) runs it with PHP's
//! value semantics ([`value`], the `twig-value` crate). The [`runtime`] ties it together: the [`Environment`] owns
//! the extensions ([`extension`]: the core filters, functions, tests and operators, escaping ([`escaper`]), the
//! sandbox, debug and string-loader extensions, and the host's), the globals, the loader ([`loader`]) and the loaded
//! templates; template composition (inheritance, blocks, macros, `include`, `embed`, `use`) and the sandbox
//! ([`runtime::sandbox`]) run there. [`error`] holds the error model, [`source`] the template sources and
//! [`deprecation`] Twig's deprecation notices.

pub mod ast;
pub mod deprecation;
pub mod error;
pub mod escaper;
pub mod extension;
pub mod lexer;
pub mod lltl;
pub mod loader;
pub mod parser;
pub mod runtime;
pub mod source;
pub mod token_parser;
pub mod visitor;
pub mod vm;

pub use error::{Error, ErrorKind, Result};
pub use extension::{TwigFilter, TwigFunction, TwigTest};
pub use runtime::{Environment, EnvironmentOptions, Markup, TemplateWrapper};
pub use source::Source;
/// The PHP value model (the `twig-value` crate): template contexts are [`value::Array`]s of [`value::Value`]s.
pub use twig_value as value;
#[cfg(feature = "serde")]
pub use twig_value::serde::to_value;
pub use twig_value::{Array, Value};

/// Serializes `value` into a template context (feature `serde`): `value` must serialize to a map, a struct or a
/// sequence; the mapping of the serde data model onto PHP values is in [`value::serde`].
///
/// ```
/// #[derive(serde::Serialize)]
/// struct Page {
///     title: &'static str,
///     tags: Vec<&'static str>,
/// }
///
/// let env = twig::Environment::new(twig::loader::ArrayLoader::new([(
///     "page",
///     "{{ title }}: {{ tags|join(', ') }}",
/// )]));
/// let page = Page { title: "News", tags: vec!["a", "b"] };
/// assert_eq!(env.render("page", twig::to_context(&page)?)?, "News: a, b");
/// # Ok::<(), twig::Error>(())
/// ```
///
/// # Errors
///
/// The [`value::serde::Error`] of the serialization (see [`value::serde::to_array`]) as a `LogicException`
/// ([`ErrorKind::Logic`]).
#[cfg(feature = "serde")]
pub fn to_context<T: ?Sized + twig_value::serde::Serialize>(value: &T) -> Result<Array> {
    Ok(twig_value::serde::to_array(value)?)
}

#[cfg(feature = "serde")]
impl From<twig_value::serde::Error> for Error {
    /// A `LogicException` ([`ErrorKind::Logic`]) with the serialization error's message, which is also its previous
    /// error.
    fn from(error: twig_value::serde::Error) -> Self {
        Error::logic(error.to_string()).with_previous(error)
    }
}

/// Builds a template context: an [`Array`] of `key => value` entries.
///
/// ```
/// use twig::{Value, context};
///
/// let user = context! { name => "Fabien", roles => vec!["admin"] };
/// let title = "Users";
/// let context = context! {
///     title,                  // shorthand for `title => title`
///     users => vec![user],
///     "page-size" => 20,      // a string literal key
///     next => None::<i64>,
/// };
/// assert_eq!(context.len(), 4);
/// assert_eq!(context.get_str(b"page-size"), Some(&Value::Int(20)));
/// ```
///
/// A key is an identifier (also a keyword such as `type` written as the string literal `"type"`), a literal, or a
/// bare identifier that names both the key and the variable holding the value. A value is any expression that
/// converts into a [`Value`] with `From` (see [`value::value`]); a shorthand entry moves its variable. Keys follow
/// PHP's key normalisation (`"1"` becomes the int key `1`), and a repeated key keeps its first position and takes the
/// last value, as in a PHP array literal.
#[macro_export]
macro_rules! context {
    ($($key:tt $(=> $value:expr)?),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut context = $crate::value::Array::new();
        $(
            context.insert($crate::__context_key!($key), $crate::__context_value!($key $(, $value)?));
        )*
        context
    }};
}

/// The key of a [`context!`] entry.
#[doc(hidden)]
#[macro_export]
macro_rules! __context_key {
    ($key:ident) => {
        ::core::stringify!($key)
    };
    ($key:literal) => {
        $key
    };
}

/// The value of a [`context!`] entry: the expression, or the variable a shorthand entry names.
#[doc(hidden)]
#[macro_export]
macro_rules! __context_value {
    ($key:ident) => {
        $crate::value::Value::from($key)
    };
    ($key:tt, $value:expr) => {
        $crate::value::Value::from($value)
    };
}

/// The examples of `README.md`, compiled and run as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

#[cfg(test)]
mod tests {
    /// One environment (and its lexer) must be usable from many threads.
    #[test]
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<crate::Error>();
        assert_send_sync::<crate::Source>();
        assert_send_sync::<crate::lexer::Lexer>();
        assert_send_sync::<crate::lexer::TokenStream>();
        assert_send_sync::<crate::deprecation::Deprecation>();
        assert_send_sync::<crate::ast::Node>();
        assert_send_sync::<crate::ast::Expr>();
        assert_send_sync::<crate::extension::ExtensionSet>();
        assert_send_sync::<crate::parser::ParseContext>();
        assert_send_sync::<crate::runtime::Environment>();
        assert_send_sync::<crate::runtime::TemplateWrapper>();
        assert_send_sync::<crate::lltl::Program>();
        assert_send_sync::<crate::vm::Exception>();
        assert_send_sync::<crate::extension::runtime::CallableRuntime>();
        assert_send_sync::<crate::extension::FilterBuilder>();
        assert_send_sync::<crate::extension::FunctionBuilder>();
        assert_send_sync::<crate::extension::TestBuilder>();
    }
}
