//! A node class: `{% cache key %}...{% endcache %}` renders its body once per key and keeps the output, through runtime
//! closures that hold the fragments between renders.
//!
//! The token parser returns a node of its own class ([`CustomNode`]); the class compiles itself with
//! [`twig::lltl::Compiler`], as a Twig node class writes PHP in its `compile()` method.
//!
//! ```bash
//! cargo run --example custom_node
//! ```

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use twig::ast::{ChildKey, CustomNode, Expr, Node, NodeMut, NodeRef};
use twig::extension::{Extension, TokenParser};
use twig::lexer::{Token, TokenType};
use twig::lltl::{BinaryOp, Compiler, NodeRuntime};
use twig::loader::ArrayLoader;
use twig::parser::{EndTagTest, Parser};
use twig::value::{PhpString, Value, convert};
use twig::{Environment, context};

const INDEX: &str = "<h1>{{ title }}</h1>
{% cache 'menu' %}<nav>{% for item in menu %}<a>{{ item }}</a>{% endfor %}</nav> (rendered for {{ title }}){% endcache %}

";

/// The rendered fragments, by key, shared by the runtimes of the extension.
type Fragments = Arc<Mutex<HashMap<Vec<u8>, PhpString>>>;

/// The node of `{% cache key %}...{% endcache %}`. Its code, in PHP terms:
///
/// ```php
/// $key = (string) <key>;
/// if (null !== $fragment = $cache->lookup($key)) { yield $fragment; }
/// else { yield $cache->store($key, <the captured output of the body>); }
/// ```
#[derive(Debug, Clone, PartialEq)]
struct CacheNode {
    key: Expr,
    body: Node,
    line: i64,
    lookup: NodeRuntime,
    store: NodeRuntime,
}

impl CustomNode for CacheNode {
    fn class_name(&self) -> &str {
        "App\\Node\\CacheNode"
    }

    /// The children the node visitors walk (the escaper escapes the prints of the body, the sandbox checks it).
    fn children(&self) -> Vec<(ChildKey<'_>, NodeRef<'_>)> {
        vec![
            (ChildKey::Name("key"), NodeRef::Expr(&self.key)),
            (ChildKey::Name("body"), NodeRef::Node(&self.body)),
        ]
    }

    fn children_mut(&mut self) -> Vec<(ChildKey<'_>, NodeMut<'_>)> {
        vec![
            (ChildKey::Name("key"), NodeMut::Expr(&mut self.key)),
            (ChildKey::Name("body"), NodeMut::Node(&mut self.body)),
        ]
    }

    /// The key is converted to a string as it is used: the sandbox checks the `__toString()` of an object key.
    fn string_coerced_child_names(&self) -> Vec<String> {
        vec!["key".to_owned()]
    }

    fn compile<'t>(&'t self, compiler: &mut Compiler<'t, '_>) -> twig::Result<()> {
        compiler.add_debug_info(self.line);
        // $key = (string) <key>
        let key = compiler.local();
        compiler.subcompile_expr(&self.key)?;
        compiler.string_cast()?;
        compiler.store_local(key)?;
        // the fragment kept for the key, or null
        compiler.call_runtime(&self.lookup, |c| c.load_local(key))?;
        compiler.dup()?;
        compiler.constant(Value::Null);
        compiler.binary(BinaryOp::Identical)?;
        let miss = compiler.jump_if_true()?;
        compiler.print()?;
        let end = compiler.jump();
        // render the body into a string and keep it
        compiler.bind(miss)?;
        compiler.pop()?;
        compiler.call_runtime(&self.store, |c| {
            c.load_local(key)?;
            c.capture(true, |c| c.subcompile(&self.body))
        })?;
        compiler.print()?;
        compiler.bind(end)
    }

    fn clone_box(&self) -> Box<dyn CustomNode> {
        Box::new(self.clone())
    }

    fn eq_dyn(&self, other: &dyn CustomNode) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// `{% cache key %}`: the key expression, then the body up to `{% endcache %}`.
struct CacheTokenParser {
    lookup: NodeRuntime,
    store: NodeRuntime,
}

impl TokenParser for CacheTokenParser {
    fn tag(&self) -> &str {
        "cache"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let key = parser.parse_expression(0)?;
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        let end = |token: &Token| token.test_value(TokenType::Name, "endcache");
        let end = EndTagTest {
            tag: Some(self.tag()),
            test: &end,
        };
        let body = parser.subparse(Some(&end), true)?;
        parser
            .stream_mut()
            .expect(TokenType::BlockEnd, None, None)?;
        let line = token.line();
        let node = CacheNode {
            key,
            body,
            line,
            lookup: self.lookup.clone(),
            store: self.store.clone(),
        };
        Ok(Some(Node::custom(node, line)))
    }
}

/// The extension: the tag, with runtimes over its fragments.
struct CacheExtension {
    fragments: Fragments,
}

impl Extension for CacheExtension {
    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        let fragments = Arc::clone(&self.fragments);
        // lookup($key): the fragment, or null
        let lookup = NodeRuntime::new(move |call| {
            let key = call.arg(0).cloned().unwrap_or_default();
            let key = convert::to_string(&key, call.diagnostics())?;
            let fragments = fragments.lock().expect("not poisoned");
            Ok(fragments
                .get(key.as_bytes())
                .map_or(Value::Null, |fragment| Value::String(fragment.clone())))
        });
        let fragments = Arc::clone(&self.fragments);
        // store($key, $fragment): keeps the fragment and returns it
        let store = NodeRuntime::new(move |call| {
            let [key, fragment] = <[Value; 2]>::try_from(call.take_args()).expect("two arguments");
            let key = convert::to_string(&key, call.diagnostics())?;
            let fragment = convert::to_string(&fragment, call.diagnostics())?;
            let mut fragments = fragments.lock().expect("not poisoned");
            fragments.insert(key.as_bytes().to_vec(), fragment.clone());
            Ok(Value::String(fragment))
        });
        vec![Arc::new(CacheTokenParser { lookup, store })]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn main() -> twig::Result<()> {
    let env = Environment::new(ArrayLoader::new([
        ("index.html.twig", INDEX),
        (
            "broken.html.twig",
            "{% cache 'a' %}\n{{ 1 / 0 }}\n{% endcache %}",
        ),
    ]));
    let fragments = Fragments::default();
    env.add_extension(Arc::new(CacheExtension {
        fragments: Arc::clone(&fragments),
    }))?;

    let menu = ["Home", "Docs & API"];
    // the first render renders the fragment and keeps it, escaped as any print is
    print!(
        "{}",
        env.render("index.html.twig", context! { title => "Home", menu })?
    );
    // the second render prints the kept fragment
    print!(
        "{}",
        env.render("index.html.twig", context! { title => "Docs", menu })?
    );
    println!(
        "{} fragment(s) kept",
        fragments.lock().expect("not poisoned").len()
    );

    // errors in the body keep their template lines
    if let Err(error) = env.render("broken.html.twig", context! {}) {
        eprintln!("{}: {error}", error.class_name());
    }
    Ok(())
}
