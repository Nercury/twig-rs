//! Rendering untrusted templates: a `Sandbox` over an environment dedicated to them and a strict `SecurityPolicy`
//! that allow-lists tags, filters and the methods of a host object, as Twig 3.29+ recommends.
//!
//! ```bash
//! cargo run --example sandbox
//! ```

use std::sync::Arc;

use twig::loader::ArrayLoader;
use twig::runtime::{Sandbox, SecurityError, SecurityErrorKind, SecurityPolicy};
use twig::value::object::ObjectBuilder;
use twig::{Environment, Value, context};

/// Templates written by users, e.g. stored in a database.
const USER_TEMPLATES: [(&str, &str); 4] = [
    (
        "newsletter.twig",
        "{% for article in articles %}{{ loop.index }}. {{ article.title|upper }}\n{% endfor %}",
    ),
    ("body.twig", "{{ articles[0].body }}"),
    ("raw.twig", "{{ articles[0].title|raw }}"),
    ("include.twig", "{% include 'newsletter.twig' %}"),
];

/// A host object of the class `App\Article`, as templates and policies see it: the methods `getTitle()` and
/// `getBody()`, so that `article.title` calls `getTitle()` as in PHP.
fn article(title: &str, body: &str) -> Value {
    let (title, body) = (title.to_owned(), body.to_owned());
    ObjectBuilder::new("App\\Article")
        .method("getTitle", [], move |_, _, _| Ok(title.clone()))
        .method("getBody", [], move |_, _, _| Ok(body.clone()))
        .build()
        .into()
}

fn main() -> twig::Result<()> {
    // Everything the templates can reach: the loader defines which templates exist, the extensions and globals
    // which capabilities do. Never hand the application's own environment to a sandbox.
    let env = Environment::new(ArrayLoader::new(USER_TEMPLATES));

    let policy = SecurityPolicy::new();
    policy.set_allowed_tags(["for"]);
    // auto-escaping applies the `escape` filter to every printed expression
    policy.set_allowed_filters(["escape", "upper"]);
    policy.set_allowed_methods([("App\\Article", ["getTitle"])]);
    // Twig 4.0's rules: nothing is allowed implicitly (`Sandbox` requires it)
    policy.set_strict(true);
    let sandbox = Sandbox::new(env, Arc::new(policy))?;

    let context = || {
        context! {
            articles => vec![
                article("Sandboxing <templates>", "Allowed methods only."),
                article("Strict policies", "Twig 4.0 rules today."),
            ],
        }
    };
    print!("{}", sandbox.render("newsletter.twig", context())?);

    // Every violation is an error of its own class with Twig's message, template name and line.
    for name in ["body.twig", "raw.twig", "include.twig"] {
        let error = sandbox.render(name, context()).unwrap_err();
        println!("{}: {error}", error.class_name());
        // The names a violation carries. `article.body` is neither an allowed property nor resolves to an allowed
        // method (`getBody()`): Twig reports the property.
        match SecurityError::of(&error).map(SecurityError::kind) {
            Some(SecurityErrorKind::Property { class, property }) => {
                println!("  property {property} of {class}");
            }
            Some(SecurityErrorKind::Method { class, method }) => {
                println!("  method {method}() of {class}");
            }
            Some(SecurityErrorKind::Filter(filter)) => println!("  filter {filter}"),
            Some(SecurityErrorKind::Tag(tag)) => println!("  tag {tag}"),
            _ => {}
        }
    }

    // Templates made from strings are sandboxed too.
    let template = sandbox.create_template(b"{{ articles|length }} articles", Some("count"))?;
    if let Err(error) = template.render(context()) {
        println!("{}: {error}", error.class_name());
    }
    Ok(())
}
