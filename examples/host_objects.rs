//! Rust types as PHP objects in templates: a type that implements `Object` by hand (`App\Article`) and objects
//! assembled with `ObjectBuilder` (`App\User`, `App\TagList`). Templates reach them like Twig reaches PHP objects:
//! `article.author` calls `getAuthor()`, `user.admin` calls `isAdmin()`, `{{ article }}` calls `__toString()`, and
//! `for` loops iterate an `IteratorAggregate`.
//!
//! ```bash
//! cargo run --example host_objects
//! ```

use std::sync::LazyLock;

use twig::loader::ArrayLoader;
use twig::value::object::{
    CallArgs, ClassBuilder, ClassRef, FunctionKind, MethodParams, Object, ObjectBuilder, ObjectRef,
    Param, PropertyStore, undefined_method,
};
use twig::value::{Diagnostics, FetchMode, PhpResult, convert};
use twig::{Environment, Value, context};

const PAGE: &str = r#"<h1>{{ article }}</h1>
<p class="byline">by {{ article.author }}{{ article.published ? '' : ' (draft)' }}</p>
<p>{{ article.excerpt(words: 6) }}</p>
<ul>
{% for tag in tags %}
  <li>{{ tag }}</li>
{% endfor %}
</ul>
<p>{{ tags|length }} tags, starting with {{ tags[0] }}.</p>
<footer>{{ user.greet('Welcome back') }}{% if user.admin %} <a href="/admin">admin</a>{% endif %}</footer>
"#;

// ---- implementing `Object` by hand ---------------------------------------------------------------------------------

/// The class, declared once and shared by every article: `public $title`, `getAuthor()`, `isPublished()`,
/// `excerpt($words = 3)` and `__toString()`.
static ARTICLE: LazyLock<ClassRef> = LazyLock::new(|| {
    ClassBuilder::class("App\\Article")
        .property("title", Value::Null)
        .method("getAuthor")
        .method("isPublished")
        .method("excerpt")
        .method("__toString")
        .build()
});

struct Article {
    /// The public properties: `title`.
    properties: PropertyStore,
    author: String,
    body: String,
    published: bool,
}

/// An article object: the title in its property, the rest in Rust fields.
fn article(title: &str, author: &str, body: &str, published: bool) -> ObjectRef {
    let properties = PropertyStore::new(&ARTICLE);
    properties.set(b"title", Value::from(title));
    ObjectRef::new(Article {
        properties,
        author: author.to_owned(),
        body: body.to_owned(),
        published,
    })
}

/// `excerpt($words = 3)`
fn excerpt_params() -> Vec<Param> {
    vec![Param::optional("words", 3)]
}

impl Object for Article {
    fn class(&self) -> &ClassRef {
        &ARTICLE
    }

    fn property_store(&self) -> Option<&PropertyStore> {
        Some(&self.properties)
    }

    /// The parameters of the methods (by lowercase name), so that named arguments bind as in PHP.
    fn method_params(&self, method: &str) -> Option<MethodParams> {
        Some(MethodParams::user(match method {
            "excerpt" => excerpt_params(),
            _ => Vec::new(),
        }))
    }

    /// The method bodies, by lowercase name.
    fn call(
        &self,
        this: &ObjectRef,
        method: &str,
        args: CallArgs,
        diag: &mut dyn Diagnostics,
    ) -> PhpResult<Value> {
        match method {
            "getauthor" => Ok(Value::from(self.author.as_str())),
            "ispublished" => Ok(Value::from(self.published)),
            "excerpt" => {
                let args = args.bind(
                    "App\\Article::excerpt",
                    &excerpt_params(),
                    FunctionKind::User,
                )?;
                let words = usize::try_from(convert::to_long(&args[0], diag)?).unwrap_or(0);
                let words: Vec<&str> = self.body.split(' ').take(words).collect();
                Ok(Value::from(words.join(" ") + "..."))
            }
            "__tostring" => this.read_property(b"title", FetchMode::Read, diag),
            _ => Err(undefined_method(self.class(), method)),
        }
    }
}

// ---- ObjectBuilder ---------------------------------------------------------------------------------------------------

/// `App\User`: the properties `name` and `roles`, `isAdmin()` and `greet($greeting, $end = '!')`.
fn user(name: &str, roles: &[&str]) -> ObjectRef {
    let admin = roles.contains(&"admin");
    ObjectBuilder::new("App\\User")
        .property("name", name)
        .property("roles", roles.to_vec())
        .method("isAdmin", [], move |_, _, _| Ok(admin))
        .method(
            "greet",
            [Param::required("greeting"), Param::optional("end", "!")],
            |this, args, diag| {
                let name = this.read_property(b"name", FetchMode::Read, diag)?;
                let [greeting, name, end] = [&args[0], &name, &args[1]].map(|part| {
                    convert::to_string(part, diag).map(|s| s.to_string_lossy().into_owned())
                });
                Ok(format!("{} {}{}", greeting?, name?, end?))
            },
        )
        .build()
}

/// `App\TagList`: `Countable`, `IteratorAggregate` and read-only `ArrayAccess` over a list of names.
fn tag_list(names: &'static [&'static str]) -> ObjectRef {
    ObjectBuilder::new("App\\TagList")
        .countable(move |_, _| Ok(names.len() as i64))
        .iterable(move |_, _| Ok(names))
        .array_access(move |_, offset, _| {
            Ok(match offset {
                Value::Int(i) => usize::try_from(*i)
                    .ok()
                    .and_then(|i| names.get(i))
                    .map(|name| Value::from(*name)),
                _ => None,
            })
        })
        .build()
}

fn main() -> twig::Result<()> {
    let env = Environment::new(ArrayLoader::new([
        ("page.html.twig", PAGE),
        ("typo.html.twig", "{{ article.excerpt(word: 2) }}"),
    ]));

    let context = context! {
        article => article(
            "Host objects",
            "Nerijus",
            "Templates call Rust methods exactly like they call PHP methods.",
            true,
        ),
        user => user("Fabien", &["admin", "dev"]),
        tags => tag_list(&["rust", "php", "twig"]),
    };
    print!("{}", env.render("page.html.twig", context.clone())?);

    // Arguments bind like PHP binds them to a method's parameters, with PHP's errors.
    if let Err(error) = env.render("typo.html.twig", context) {
        eprintln!("{}: {error}", error.class_name());
    }
    Ok(())
}
