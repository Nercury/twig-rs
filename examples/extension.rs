//! An extension: filters, a global and a tag (`{% unless %}`) packaged as one `Extension` and registered once.
//!
//! ```bash
//! cargo run --example extension
//! ```

use std::any::Any;
use std::sync::Arc;

use twig::ast::{Expr, Node, Nodes, UnaryOp};
use twig::extension::{Extension, TokenParser, TwigFilter};
use twig::lexer::{Token, TokenType};
use twig::loader::ArrayLoader;
use twig::parser::{EndTagTest, Parser};
use twig::value::convert;
use twig::{Array, Environment, context};

const INDEX: &str = r#"<h1>{{ blog_name }}</h1>
{% for post in posts %}
<article id="{{ post.title|slug }}">
  <h2>{{ post.title }}</h2>
{% unless post.published %}
  <p>Draft</p>
{% else %}
  <p>{{ post.body|reading_time }} min read</p>
{% endunless %}
</article>
{% endfor %}
"#;

/// The extension: what it registers is listed by the methods of [`Extension`]; the environment collects it when the
/// extensions are initialized (the first template load).
struct BlogExtension {
    name: String,
}

impl Extension for BlogExtension {
    fn filters(&self) -> Vec<TwigFilter> {
        vec![
            // `{{ "Hello, World!"|slug }}` is `hello-world`
            TwigFilter::from_fn("slug", [], |call, args| {
                let text = convert::to_string(&args[0], call.diagnostics())?;
                let slug = text
                    .to_string_lossy()
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|word| !word.is_empty())
                    .collect::<Vec<_>>()
                    .join("-");
                Ok(slug)
            })
            .build(),
            // `{{ text|reading_time }}`: minutes at 200 words per minute, at least 1
            TwigFilter::from_fn("reading_time", [], |call, args| {
                let text = convert::to_string(&args[0], call.diagnostics())?;
                let words = text.to_string_lossy().split_whitespace().count();
                Ok(words.div_ceil(200).max(1))
            })
            .build(),
        ]
    }

    fn token_parsers(&self) -> Vec<Arc<dyn TokenParser>> {
        vec![Arc::new(UnlessTokenParser)]
    }

    /// Twig's `GlobalsInterface::getGlobals()`: variables every template sees.
    fn globals(&self) -> Option<Array> {
        Some(context! { blog_name => self.name.as_str() })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `{% unless condition %}...{% else %}...{% endunless %}`: `{% if not (condition) %}` under another name.
///
/// A token parser reads the rest of its tag from the token stream and returns a node. This one builds the node from
/// the core node types (`Node::if_`, `Node::print`, `Node::set`, ...), which every visitor (escaping, sandbox,
/// optimizer) and the compiler know; a tag the core nodes cannot express gets a node class of its own
/// (`examples/custom_node.rs`).
struct UnlessTokenParser;

impl TokenParser for UnlessTokenParser {
    fn tag(&self) -> &str {
        "unless"
    }

    fn parse(&self, parser: &mut Parser<'_>, token: &Token) -> twig::Result<Option<Node>> {
        let line = token.line();
        // The stream is positioned after the tag name: `condition %}`.
        let condition = parser.parse_expression(0)?;
        expect_block_end(parser)?;

        // The body, up to `{% else %}` or `{% endunless %}`; `false` leaves that tag name in the stream.
        let fork = |token: &Token| token.test_value(TokenType::Name, &["else", "endunless"]);
        let fork = EndTagTest {
            tag: Some(self.tag()),
            test: &fork,
        };
        let body = parser.subparse(Some(&fork), false)?;
        let mut otherwise = None;
        let end_or_else = parser.stream_mut().next()?;
        if end_or_else.test_value(TokenType::Name, "else") {
            expect_block_end(parser)?;
            // `true` consumes the `endunless`.
            let end = |token: &Token| token.test_value(TokenType::Name, "endunless");
            let end = EndTagTest {
                tag: Some(self.tag()),
                test: &end,
            };
            otherwise = Some(parser.subparse(Some(&end), true)?);
        }
        expect_block_end(parser)?;

        let condition = Expr::unary(UnaryOp::Not, condition, line);
        let tests = Nodes::from_list([Node::expression(condition), body]);
        Ok(Some(Node::if_(tests, otherwise, line)))
    }
}

/// The `%}` that ends a tag (Twig: `$stream->expect(Token::BLOCK_END_TYPE)`).
fn expect_block_end(parser: &mut Parser<'_>) -> twig::Result<()> {
    parser
        .stream_mut()
        .expect(TokenType::BlockEnd, None, None)?;
    Ok(())
}

fn main() -> twig::Result<()> {
    let env = Environment::new(ArrayLoader::new([
        ("index.html.twig", INDEX),
        (
            "broken.html.twig",
            "{% unless ready %}\nwaiting\n{% endif %}",
        ),
    ]));
    env.add_extension(Arc::new(BlogExtension {
        name: "Engine notes".to_owned(),
    }))?;

    let posts = vec![
        context! {
            title => "Exact output, byte for byte",
            published => true,
            body => "word ".repeat(450),
        },
        context! { title => "What's next?", published => false, body => "" },
    ];
    print!("{}", env.render("index.html.twig", context! { posts })?);

    // An end tag that does not close the body: Twig's message names the tag of the `EndTagTest`.
    if let Err(error) = env.render("broken.html.twig", context! { ready => false }) {
        eprintln!("{}: {error}", error.class_name());
    }
    Ok(())
}
