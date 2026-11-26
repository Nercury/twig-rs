//! Compile benchmarks: lexing, parsing, the node visitors and the code generation of all templates of a scenario
//! (`Environment::compile_source()`, Twig's `Environment::compileSource()`).

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use twig::Source;

mod common;

fn compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");
    for scenario in common::scenarios() {
        let env = scenario.environment();
        let sources: Vec<Source> = scenario
            .templates
            .iter()
            .map(|(name, code)| Source::new(code.as_bytes(), name.as_str()))
            .collect();
        let bytes: usize = sources.iter().map(|source| source.code().len()).sum();
        group.throughput(Throughput::Bytes(bytes as u64));
        group.bench_function(scenario.name.as_str(), |b| {
            b.iter(|| {
                for source in &sources {
                    let program = env.compile_source(black_box(source));
                    black_box(program.expect("the scenario compiles"));
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, compile);
criterion_main!(benches);
