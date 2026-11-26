//! Render benchmarks: `Environment::render()` of a scenario's `index.twig` with its context, after a first render
//! has loaded and compiled the templates (Twig's `Environment::render()` with a warm template cache).
//!
//! With `TWIG_BENCH_DUMP=<dir>` each scenario's output is also written to `<dir>/<scenario>.html`, to compare it with
//! what `research/benchmarks/bench.php --dump=<dir>` writes for PHP Twig.

use std::fs;
use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

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

criterion_group!(benches, render);
criterion_main!(benches);
