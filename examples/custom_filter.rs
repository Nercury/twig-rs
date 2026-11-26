//! Filters made of Rust closures with `TwigFilter::from_fn`: parameters with defaults (and named arguments in
//! templates), a filter whose output is HTML, a filter that reads the template context, and Twig's errors for
//! wrong arguments.
//!
//! ```bash
//! cargo run --example custom_filter
//! ```

use std::collections::HashMap;

use twig::extension::{CallableParameter, TwigFilter};
use twig::loader::ArrayLoader;
use twig::value::convert;
use twig::{Environment, context};

const POST: &str = r#"<h1>{{ post.title|truncate(24) }}</h1>
<p class="lead">{{ post.title|truncate(ellipsis: " [...]", length: 16) }}</p>
{{ post.body|paragraphs }}
<footer>{{ "posted_by"|trans }} {{ post.author }}</footer>
"#;

fn main() -> twig::Result<()> {
    let env = Environment::new(ArrayLoader::new([
        ("post.html.twig", POST),
        ("typo.html.twig", "{{ post.title|truncate(lenght: 10) }}"),
    ]));

    // `{{ text|truncate }}`, `{{ text|truncate(10) }}`, `{{ text|truncate(ellipsis: "") }}`: the closure gets the
    // input, then one value per parameter (the argument, or the parameter's default).
    env.add_filter(
        TwigFilter::from_fn(
            "truncate",
            [
                CallableParameter::optional("length", 30.into()),
                CallableParameter::optional("ellipsis", "...".into()),
            ],
            |call, args| {
                let [text, length, ellipsis] = args else {
                    unreachable!("one value per parameter")
                };
                // PHP's conversions, with PHP's warnings and errors (`Array to string conversion`, ...)
                let text = convert::to_string(text, call.diagnostics())?;
                let length = convert::to_long(length, call.diagnostics())?;
                let ellipsis = convert::to_string(ellipsis, call.diagnostics())?;
                let text = text.to_string_lossy();
                let length = usize::try_from(length).unwrap_or(0);
                if text.chars().count() <= length {
                    return Ok(text.into_owned());
                }
                let mut truncated: String = text.chars().take(length).collect();
                truncated.push_str(&ellipsis.to_string_lossy());
                Ok(truncated)
            },
        )
        .build(),
    )?;

    // `{{ text|paragraphs }}` returns HTML: the input is escaped first (`pre_escape`) and the result is not escaped
    // again (`is_safe`).
    let mut paragraphs = TwigFilter::from_fn("paragraphs", [], |call, args| {
        let html = convert::to_string(&args[0], call.diagnostics())?;
        Ok(html
            .to_string_lossy()
            .split("\n\n")
            .map(|paragraph| format!("<p>{}</p>", paragraph.trim()))
            .collect::<Vec<_>>()
            .join("\n"))
    });
    paragraphs.pre_escape("html").is_safe(["html"]);
    env.add_filter(paragraphs.build())?;

    // `{{ "key"|trans }}` looks the key up for the `locale` of the template context (`needs_context`); the closure
    // owns its catalogue.
    let catalogue = HashMap::from([
        (("en", "posted_by"), "Posted by"),
        (("lt", "posted_by"), "Paskelbė"),
    ]);
    let mut trans = TwigFilter::from_fn("trans", [], move |call, args| {
        let key = convert::to_string(&args[0], call.diagnostics())?;
        let key = key.to_string_lossy();
        let context = call.context();
        let locale = match context.get_str(b"locale") {
            Some(locale) => convert::to_string(locale, call.diagnostics())?,
            None => "en".into(),
        };
        let locale = locale.to_string_lossy();
        Ok(catalogue
            .get(&(&*locale, &*key))
            .map_or_else(|| key.to_string(), |text| (*text).to_owned()))
    });
    trans.needs_context();
    env.add_filter(trans.build())?;

    let post = context! {
        title => "Twig templates in Rust, rendered like PHP renders them",
        body => "Filters are Rust closures.\n\nTheir <em>input</em> is escaped before they run.",
        author => "Fabien",
    };
    print!(
        "{}",
        env.render("post.html.twig", context! { locale => "lt", post })?
    );

    // Arguments are checked against the parameters at compile time, with Twig's messages.
    let post = context! { title => "Typo" };
    if let Err(error) = env.render("typo.html.twig", context! { post }) {
        eprintln!("{}: {error}", error.class_name());
    }
    Ok(())
}
