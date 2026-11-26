//! Template data from anything that implements serde's `Serialize` (feature `serde`): structs and maps become PHP
//! arrays, sequences become lists, `None` becomes `null`, enums use serde_json's external tagging.
//!
//! ```bash
//! cargo run --example serde_context --features serde
//! ```

#[cfg(feature = "serde")]
fn main() -> twig::Result<()> {
    use serde::Serialize;
    use twig::loader::ArrayLoader;
    use twig::{Environment, context};

    #[derive(Serialize)]
    struct Page {
        title: &'static str,
        author: Author,
        posts: Vec<Post>,
    }

    #[derive(Serialize)]
    struct Author {
        name: &'static str,
        email: Option<&'static str>,
    }

    #[derive(Serialize)]
    struct Post {
        title: &'static str,
        tags: Vec<&'static str>,
        state: State,
    }

    #[derive(Serialize)]
    enum State {
        // a unit variant is its name: `"Draft"`
        Draft,
        // other variants are `{"Published": {"on": "..."}}`
        Published { on: &'static str },
    }

    const INDEX: &str = r#"<h1>{{ title }}</h1>
<p>by {{ author.name }}{% if author.email %} &lt;{{ author.email }}&gt;{% endif %}</p>
{% for post in posts %}
<h2>{{ post.title }}</h2>
<p>{{ post.tags|join(", ") ?: "no tags" }}, {{ post.state.Published.on ?? post.state|lower }}</p>
{% endfor %}
"#;

    let env = Environment::new(ArrayLoader::new([
        ("index.html.twig", INDEX),
        (
            "stats.html.twig",
            "{{ stats.count }} posts, largest id {{ stats.max }}",
        ),
    ]));

    let page = Page {
        title: "Release notes",
        author: Author {
            name: "Fabien",
            email: None,
        },
        posts: vec![
            Post {
                title: "Serde support",
                tags: vec!["api", "serde"],
                state: State::Published { on: "2026-11-26" },
            },
            Post {
                title: "Host objects",
                tags: vec![],
                state: State::Draft,
            },
        ],
    };
    // A struct (or map) becomes the context itself: its fields are the template variables.
    print!(
        "{}",
        env.render("index.html.twig", twig::to_context(&page)?)?
    );

    // Any `Serialize` value becomes one variable with `to_value`, here JSON data: integers beyond PHP's `int` range
    // become floats, as with PHP's `json_decode()`.
    let stats: serde_json::Value =
        serde_json::from_str(r#"{"count": 3, "max": 18446744073709551615}"#).expect("valid JSON");
    let context = context! { stats => twig::to_value(&stats)? };
    println!("{}", env.render("stats.html.twig", context)?);
    Ok(())
}

#[cfg(not(feature = "serde"))]
fn main() {
    eprintln!(
        "this example needs the `serde` feature: cargo run --example serde_context --features serde"
    );
}
