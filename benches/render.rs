//! Render benchmarks: `Environment::render()` of a scenario's `index.twig` with its context, after a first render
//! has loaded and compiled the templates (Twig's `Environment::render()` with a warm template cache).
//!
//! With `TWIG_BENCH_DUMP=<dir>` each scenario's output is also written to `<dir>/<scenario>.html`, to compare it with
//! what `research/benchmarks/bench.php --dump=<dir>` writes for PHP Twig.
//!
//! The `micro` group renders one-off templates that repeat a single construct (`research/benchmarks/bench.php` has
//! the same ones), to see what one construct costs.

use std::fs;
use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use twig::loader::ArrayLoader;
use twig::runtime::Environment;
use twig::value::{Array, Value};

mod common;

fn render(c: &mut Criterion) {
    let dump = std::env::var_os("TWIG_BENCH_DUMP").map(PathBuf::from);
    let mut group = c.benchmark_group("render");
    for scenario in common::scenarios() {
        let env = scenario.environment();
        let output = env
            .render("index.twig", scenario.context.clone())
            .expect("the scenario renders");
        if let Some(dir) = &dump {
            fs::create_dir_all(dir).expect("the dump directory");
            fs::write(dir.join(format!("{}.html", scenario.name)), &output).expect("the dump file");
        }
        group.throughput(Throughput::Bytes(output.len() as u64));
        group.bench_function(scenario.name.as_str(), |b| {
            b.iter(|| {
                let context = black_box(scenario.context.clone());
                env.render(black_box("index.twig"), context)
                    .expect("the scenario renders")
            });
        });
    }
    group.finish();
}

/// The templates of the `micro` group (keep them in sync with `bench.php`).
fn micro_templates() -> Vec<(String, String)> {
    let repeat = |text: &str| text.repeat(1000);
    vec![
        ("hello".to_owned(), "Hello {{ name }}!".to_owned()),
        ("text".to_owned(), repeat("x{# c #}")),
        ("print_int".to_owned(), repeat("{{ 1 }}")),
        ("print_raw".to_owned(), repeat("{{ a|raw }}")),
        ("print_escaped".to_owned(), repeat("{{ a }}")),
        ("set".to_owned(), repeat("{% set x = a %}")),
        ("set_add".to_owned(), repeat("{% set x = a + 1 %}")),
        (
            "for".to_owned(),
            "{% for i in items %}{{ i }}{% endfor %}".to_owned(),
        ),
        ("attribute".to_owned(), repeat("{{ user.name }}")),
        ("filter".to_owned(), repeat("{{ name|length }}")),
        (
            "empty_loop".to_owned(),
            repeat("{% for i in [1, 2] %}{% endfor %}"),
        ),
        (
            "macro".to_owned(),
            format!(
                "{{% macro m(v) %}}{{{{ v }}}}{{% endmacro %}}{}",
                repeat("{{ _self.m(a) }}")
            ),
        ),
    ]
}

/// The context of the `micro` group: `name` is `'World'`, `a` is `1`, `items` the integers 1 to 1000, `user` a
/// mapping with a `name`.
fn micro_context() -> Array {
    let mut context = Array::new();
    context.insert("name", "World");
    context.insert("a", 1);
    context.insert(
        "items",
        Value::Array(Array::from_values((1..=1000).map(Value::from))),
    );
    context.insert("user", Array::from([("name", "Ada")]));
    context
}

fn micro(c: &mut Criterion) {
    let templates = micro_templates();
    let env = Environment::new(ArrayLoader::new(templates.clone()));
    let context = micro_context();
    let mut group = c.benchmark_group("micro");
    for (name, _) in &templates {
        env.render(name, context.clone())
            .expect("the template renders");
        group.bench_function(name.as_str(), |b| {
            b.iter(|| {
                env.render(black_box(name), black_box(context.clone()))
                    .expect("the template renders")
            });
        });
    }
    group.finish();
}

criterion_group!(benches, render, micro);
criterion_main!(benches);
