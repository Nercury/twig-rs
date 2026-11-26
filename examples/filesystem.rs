//! Templates on disk: a `FilesystemLoader` with the application's templates in the main namespace and a small UI kit
//! in the `ui` namespace, used with `{% extends %}`, `include()` and `{% import %}`.
//!
//! ```bash
//! cargo run --example filesystem
//! ```
//!
//! The templates are in `examples/templates/`:
//!
//! ```text
//! app/base.html.twig     the layout             base.html.twig
//! app/login.html.twig    the page               login.html.twig
//! ui/alert.html.twig     a component            @ui/alert.html.twig
//! ui/forms.html.twig     a macro library        @ui/forms.html.twig
//! ```

use twig::loader::FilesystemLoader;
use twig::{Array, Environment, context};

fn main() -> twig::Result<()> {
    // Relative directories are resolved against the root path (by default the current directory).
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/templates");
    let loader = FilesystemLoader::with_root_path(["app"], root)?;
    // `@ui/...` names are looked up in this directory (a namespace can have several, searched in order).
    loader.add_path("ui", "ui")?;

    let env = Environment::new(loader);
    env.add_global("site", "Example")?;

    let page = env.render(
        "login.html.twig",
        context! { username => "fabien", error => "Invalid credentials." },
    )?;
    print!("{page}");

    // A name that no directory has: Twig's loader error, listing the directories that were searched.
    if let Err(error) = env.render("register.html.twig", Array::new()) {
        eprintln!("{}: {error}", error.class_name());
    }
    // Names cannot leave the configured directories.
    if let Err(error) = env.render("../hello.rs", Array::new()) {
        eprintln!("{}: {error}", error.class_name());
    }
    Ok(())
}
