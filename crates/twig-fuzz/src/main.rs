//! `twig-fuzz`: differential testing of twig-rs against the PHP oracle. See the crate documentation
//! (`cargo doc -p twig-fuzz`) or `twig-fuzz help`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use twig_fuzz::drive::{self, DriveOptions};
use twig_fuzz::kit::case::{Case, batch_cases};
use twig_fuzz::kit::diff::{Verdict, compare};
use twig_fuzz::kit::json::Json;
use twig_fuzz::kit::result::CaseResult;
use twig_fuzz::kit::run::Outcome;
use twig_fuzz::kit::vectors::{
    KnownMismatch, ReplayStatus, Vector, VectorFile, refresh_file, replay_files, update_file,
    vector_files,
};
use twig_fuzz::minimize::{self, SameSignature, oracle_request};
use twig_fuzz::oracle::Oracle;
use twig_fuzz::selftest;
use twig_fuzz::worker::{Executor, serve_worker};

const HELP: &str = "\
twig-fuzz: differential testing of twig-rs against the PHP oracle (Twig 3.31.1 on PHP 8.5.13)

USAGE:
  twig-fuzz drive <batch.json>... --out <dir> [--oracle-results <file>] [--no-minimize]
                  [--max-minimize N] [--per-cluster N] [--timeout-ms N] [--description TEXT] [--unmarked]
                  [--shared-process] [--chunk N] [--jobs N]
      batch -> oracle (Docker) + twig-rs -> diff -> clusters -> minimised reproductions;
      writes <dir>/report.md, report.json, oracle.json, twig-rs.json, vectors.json
  twig-fuzz oracle <batch.json> [--out <file>] [--chunk N] [--jobs N]
      runs the batch through the oracle (data_php cases get \"typed_data\": true)
  twig-fuzz run <batch.json> [--oracle <results.json>] [--out <file>] [--timeout-ms N]
      runs the batch through twig-rs, printing results in the oracle's schema (data_php cases need
      the oracle's results for their typed data)
  twig-fuzz diff <batch.json> --oracle <results.json> [--twig-rs <results.json>]
      compares oracle results with twig-rs results (computed when --twig-rs is not given)
  twig-fuzz minimize <batch.json> --id <id> [--out <file>] [--timeout-ms N] [--shared-process]
      minimises one mismatching case (needs Docker); every candidate runs in a PHP process of its
      own unless --shared-process (faster, but candidates then share compiled templates, declared
      classes and their compile-time deprecations)
  twig-fuzz replay <vectors.json or dir>... [--update [--id ID]...]
      replays regression vector files (what tests/fuzz_regressions.rs does); --update also removes
      the known_mismatch marker of every marked vector that now matches the oracle (only the
      vectors named by --id when given), editing nothing else in the file; a vector that still
      mismatches keeps its marker, also when its signature changed (reported, left for a person)
  twig-fuzz refresh <vectors.json or dir>... (--id ID... | --all)
      runs the cases of the named vectors through the oracle again (each in a PHP process of its
      own) and writes the new results in place of the recorded ones, changing nothing else: how a
      vector gets what the oracle records today (replay --update afterwards flips it if it matches)
  twig-fuzz record <batch.json> --out <vectors.json> [--description TEXT] [--oracle <results.json>]
      records every case of the batch as a regression vector with the oracle's result; cases that
      mismatch are marked known_mismatch (with their signature), unsupported ones are left out
  twig-fuzz generate <family> [--seed N] [--count N] [--out <batch.json>]
      writes a random batch of one construct family: expressions, callables, control, composition,
      escaping, errors, formats, sandbox, whitespace, unicode, dynamic, structure (default seed 1,
      count 1000)
  twig-fuzz selftest-fixtures --out <dir>
      writes the plain-environment equivalents of the fixture corpus (fixtures.json,
      fixtures-isolated.json)
  twig-fuzz selftest-engine --out <dir>
      writes the engine vector batches as one batch (engine.json) with their recorded results
      (engine-results.json)

  twig-rs runs in a child process (`twig-fuzz worker`, internal) that is restarted after a timeout
  or a crash; --in-process runs it on threads of this process instead. --timeout-ms (default 5000)
  bounds one twig-rs run. Cases sent to the oracle get \"memory_limit\": \"512M\", \"time_limit\": 30
  and \"clock\": true unless they set them. Isolated oracle batches run --jobs cases at a time
  (default 8). A chunk whose container dies (PHP's hard memory limit, a crash) or stops making
  progress is run again case by case in PHP processes of their own. drive, diff and replay exit
  with 1 when something mismatches or fails.

ENVIRONMENT:
  TWIG_FUZZ_ORACLE       oracle command (default: docker run --rm -i $TWIG_ORACLE_IMAGE php runner.php)
  TWIG_ORACLE_IMAGE      oracle image (default twig-oracle:3.31.1)
  TWIG_FUZZ_ORACLE_JOBS  default of --jobs
";

struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: &[String]) -> Args {
        const VALUED: &[&str] = &[
            "--out",
            "--oracle",
            "--oracle-results",
            "--twig-rs",
            "--id",
            "--chunk",
            "--timeout-ms",
            "--max-minimize",
            "--per-cluster",
            "--description",
            "--seed",
            "--count",
            "--jobs",
        ];
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let arg = &raw[i];
            if let Some((name, value)) = arg.split_once('=').filter(|_| arg.starts_with("--")) {
                flags.push((name.to_owned(), Some(value.to_owned())));
            } else if VALUED.contains(&arg.as_str()) {
                flags.push((arg.clone(), raw.get(i + 1).cloned()));
                i += 1;
            } else if arg.starts_with("--") {
                flags.push((arg.clone(), None));
            } else {
                positional.push(arg.clone());
            }
            i += 1;
        }
        Args { positional, flags }
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }

    /// Every value of a repeatable flag; each value may list several, separated by commas.
    fn values(&self, name: &str) -> Vec<String> {
        self.flags
            .iter()
            .filter(|(n, _)| n == name)
            .filter_map(|(_, v)| v.as_deref())
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }

    fn number(&self, name: &str, default: usize) -> Result<usize, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| format!("{name} needs a number, got {v:?}")),
        }
    }

    fn timeout(&self) -> Result<Duration, String> {
        Ok(Duration::from_millis(
            self.number("--timeout-ms", 5_000)? as u64
        ))
    }

    fn executor(&self) -> Executor {
        match std::env::current_exe() {
            Ok(program) if !self.has("--in-process") => Executor::worker(program),
            _ => Executor::InProcess,
        }
    }
}

fn main() -> ExitCode {
    // panics of guarded runs become results; keep their default report off the terminal
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let guarded = std::thread::current()
            .name()
            .is_some_and(|n| n.starts_with("twig-fuzz "));
        if !guarded || std::env::var_os("TWIG_FUZZ_PANICS").is_some() {
            default_hook(info);
        }
    }));
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = raw.first().cloned() else {
        print!("{HELP}");
        return ExitCode::from(2);
    };
    let args = Args::parse(&raw[1..]);
    let result = match command.as_str() {
        "drive" => cmd_drive(&args),
        "oracle" => cmd_oracle(&args),
        "run" => cmd_run(&args),
        "diff" => cmd_diff(&args),
        "minimize" | "minimise" => cmd_minimize(&args),
        "replay" => cmd_replay(&args),
        "refresh" => cmd_refresh(&args),
        "record" => cmd_record(&args),
        "generate" => cmd_generate(&args),
        "selftest-fixtures" => cmd_selftest_fixtures(&args),
        "selftest-engine" => cmd_selftest_engine(&args),
        "worker" => {
            serve_worker();
            Ok(ExitCode::SUCCESS)
        }
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command {other:?} (try `twig-fuzz help`)")),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("twig-fuzz: {error}");
            ExitCode::from(2)
        }
    }
}

fn batch_arg(args: &Args) -> Result<(Vec<Json>, Option<String>), String> {
    let path = args.positional.first().ok_or("missing batch file")?;
    let batch = Json::load(&PathBuf::from(path))?;
    Ok(batch_cases(&batch))
}

fn write_or_print(args: &Args, json: &Json) -> Result<(), String> {
    let text = json.to_layout(1) + "\n";
    match args.value("--out") {
        Some(path) => std::fs::write(path, text).map_err(|e| format!("cannot write {path}: {e}")),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

fn load_results(path: &str) -> Result<Vec<CaseResult>, String> {
    Ok(Json::load(&PathBuf::from(path))?
        .items()
        .iter()
        .map(CaseResult::from_json)
        .collect())
}

fn cmd_drive(args: &Args) -> Result<ExitCode, String> {
    if args.positional.is_empty() {
        return Err("drive needs at least one batch file".to_owned());
    }
    let files: Vec<PathBuf> = args.positional.iter().map(PathBuf::from).collect();
    let cases = drive::load_batches(&files)?;
    let options = DriveOptions {
        out_dir: PathBuf::from(args.value("--out").ok_or("drive needs --out <dir>")?),
        minimize: !args.has("--no-minimize"),
        max_minimize: args.number("--max-minimize", 40)?,
        per_cluster: args.number("--per-cluster", 1)?,
        timeout: args.timeout()?,
        oracle_results: args.value("--oracle-results").map(PathBuf::from),
        description: args.value("--description").unwrap_or_default().to_owned(),
        mark_known: !args.has("--unmarked"),
        shared_minimize: args.has("--shared-process"),
        verbose: true,
    };
    let mut oracle = oracle_from_args(args)?;
    let mut executor = args.executor();
    let summary = drive::drive(&cases, &mut oracle, &mut executor, &options)?;
    drive::write_outputs(&summary, &options)?;
    let (same, deviation, mismatch, skipped, unsupported) = summary.counts();
    println!(
        "{} cases: {same} identical, {deviation} deviations, {mismatch} mismatches in {} clusters, \
         {skipped} not comparable, {unsupported} unsupported ({:.1} s, {} oracle containers)",
        summary.cases.len(),
        summary.clusters.len(),
        summary.seconds,
        oracle.containers
    );
    println!(
        "report: {}",
        drive::display_path(&options.out_dir.join("report.md"))
    );
    Ok(if mismatch == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// The oracle client with the `--chunk` and `--jobs` settings, reporting incidents on stderr.
fn oracle_from_args(args: &Args) -> Result<Oracle, String> {
    let mut oracle = Oracle::from_env();
    oracle.chunk = args.number("--chunk", 2000)?;
    oracle.jobs = args.number("--jobs", oracle.jobs)?.max(1);
    oracle.verbose = true;
    Ok(oracle)
}

fn cmd_oracle(args: &Args) -> Result<ExitCode, String> {
    let (cases, isolation) = batch_arg(args)?;
    let mut oracle = oracle_from_args(args)?;
    oracle.ensure_image()?;
    let requests: Vec<Json> = cases.iter().map(oracle_request).collect();
    let results = oracle.run(&requests, isolation.as_deref())?;
    write_or_print(args, &Json::Array(results))?;
    Ok(ExitCode::SUCCESS)
}

fn twig_rs_results(
    cases: &[Json],
    oracle: Option<&[CaseResult]>,
    timeout: Duration,
    executor: &mut Executor,
) -> Vec<(Json, Result<CaseResult, String>)> {
    cases
        .iter()
        .enumerate()
        .map(|(i, json)| {
            let outcome = match Case::from_json(&oracle_request(json)) {
                Err(error) => Err(error),
                Ok(case) => match executor.run(&case, oracle.and_then(|o| o.get(i)), timeout) {
                    Outcome::Ran(result) => Ok(*result),
                    Outcome::Unsupported(reason) => Err(reason),
                },
            };
            (json.clone(), outcome)
        })
        .collect()
}

fn cmd_run(args: &Args) -> Result<ExitCode, String> {
    let (cases, _) = batch_arg(args)?;
    let oracle = args.value("--oracle").map(load_results).transpose()?;
    let results = twig_rs_results(
        &cases,
        oracle.as_deref(),
        args.timeout()?,
        &mut args.executor(),
    );
    let out: Vec<Json> = results
        .into_iter()
        .map(|(case, r)| match r {
            Ok(result) => result.to_json(),
            Err(reason) => Json::Object(vec![
                (
                    "id".to_owned(),
                    case.get("id").cloned().unwrap_or(Json::Null),
                ),
                ("unsupported".to_owned(), Json::str(reason)),
            ]),
        })
        .collect();
    write_or_print(args, &Json::Array(out))?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_diff(args: &Args) -> Result<ExitCode, String> {
    let (cases, _) = batch_arg(args)?;
    let oracle = load_results(
        args.value("--oracle")
            .ok_or("diff needs --oracle <results>")?,
    )?;
    if oracle.len() != cases.len() {
        return Err(format!(
            "{} oracle results for {} cases",
            oracle.len(),
            cases.len()
        ));
    }
    let actual: Vec<Result<CaseResult, String>> = match args.value("--twig-rs") {
        Some(path) => load_results(path)?.into_iter().map(Ok).collect(),
        None => twig_rs_results(&cases, Some(&oracle), args.timeout()?, &mut args.executor())
            .into_iter()
            .map(|(_, r)| r)
            .collect(),
    };
    let mut clusters: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let (mut same, mut deviation, mut skipped, mut unsupported) = (0, 0, 0, 0);
    for ((case, expected), actual) in cases.iter().zip(&oracle).zip(&actual) {
        let id = case.get("id").map(Json::to_compact).unwrap_or_default();
        match actual {
            Err(reason) => {
                unsupported += 1;
                eprintln!("{id}: unsupported: {reason}");
            }
            Ok(actual) => match compare(expected, actual) {
                Verdict::Same => same += 1,
                Verdict::Deviation(_) => deviation += 1,
                Verdict::Skip(_) => skipped += 1,
                Verdict::Mismatch(m) => {
                    println!("{id}: {}\n{}", m.signature(), m.describe());
                    clusters.entry(m.signature()).or_default().push(id);
                }
            },
        }
    }
    let mismatches: usize = clusters.values().map(Vec::len).sum();
    println!(
        "{} cases: {same} identical, {deviation} deviations, {mismatches} mismatches in {} clusters, \
         {skipped} not comparable, {unsupported} unsupported",
        cases.len(),
        clusters.len()
    );
    for (signature, ids) in &clusters {
        println!("  {:>5}  {signature}", ids.len());
    }
    Ok(if mismatches == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn cmd_minimize(args: &Args) -> Result<ExitCode, String> {
    let (cases, isolation) = batch_arg(args)?;
    let id = args.value("--id").ok_or("minimize needs --id <id>")?;
    let case = cases
        .iter()
        .find(|c| {
            c.get("id")
                .map(|v| v.as_str().map_or_else(|| v.to_compact(), str::to_owned))
                .as_deref()
                == Some(id)
        })
        .ok_or_else(|| format!("no case with id {id:?}"))?
        .clone();
    let timeout = args.timeout()?;
    let mut oracle = oracle_from_args(args)?;
    let mut executor = args.executor();
    oracle.ensure_image()?;
    // a PHP process per candidate unless --shared-process (see the minimize module)
    let isolation = if args.has("--shared-process") {
        isolation
    } else {
        Some(minimize::ISOLATION.to_owned())
    };
    let request = oracle_request(&case);
    let expected = CaseResult::from_json(
        &oracle.run_served(std::slice::from_ref(&request), isolation.as_deref())?[0],
    );
    let parsed = Case::from_json(&request)?;
    let actual = match executor.run(&parsed, Some(&expected), timeout) {
        Outcome::Ran(result) => *result,
        Outcome::Unsupported(reason) => return Err(format!("unsupported: {reason}")),
    };
    let Verdict::Mismatch(mismatch) = compare(&expected, &actual) else {
        println!("the case does not mismatch");
        return Ok(ExitCode::SUCCESS);
    };
    let signature = mismatch.signature();
    eprintln!("signature: {signature}");
    let mut start = case.clone();
    if let Some(data) = expected.extra("data") {
        let keys: Vec<Json> = data
            .get("v")
            .map(Json::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|e| e.items().first().cloned())
            .collect();
        start.set("_fuzz_data_keys", Json::Array(keys));
    }
    let drop_test_classes = isolation.as_deref() == Some(minimize::ISOLATION);
    let minimized = {
        let mut predicate =
            SameSignature::new(&mut oracle, &mut executor, signature, timeout, isolation);
        let settings = minimize::Options {
            verbose: true,
            drop_test_classes,
            ..minimize::Options::default()
        };
        minimize::minimize(&start, &mut predicate, settings)
    };
    eprintln!("{} candidates tested", minimized.tests);
    let mut result = match minimize::cache_dir(&case) {
        Some(dir) => minimize::with_cache_dir(&minimized.case, dir),
        None => minimized.case,
    };
    result.remove("_fuzz_data_keys");
    write_or_print(args, &result)?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_record(args: &Args) -> Result<ExitCode, String> {
    let (cases, isolation) = batch_arg(args)?;
    let out = PathBuf::from(
        args.value("--out")
            .ok_or("record needs --out <vectors.json>")?,
    );
    let requests: Vec<Json> = cases.iter().map(oracle_request).collect();
    let results: Vec<Json> = match args.value("--oracle") {
        Some(path) => Json::load(&PathBuf::from(path))?.items().to_vec(),
        None => {
            let mut oracle = oracle_from_args(args)?;
            oracle.ensure_image()?;
            oracle.run(&requests, isolation.as_deref())?
        }
    };
    if results.len() != requests.len() {
        return Err(format!(
            "{} oracle results for {} cases",
            results.len(),
            requests.len()
        ));
    }
    let timeout = args.timeout()?;
    let mut executor = args.executor();
    let mut vectors = Vec::new();
    let (mut same, mut known, mut left_out) = (0, 0, 0);
    for (n, (request, result)) in requests.iter().zip(&results).enumerate() {
        let expected = CaseResult::from_json(result);
        let id = match request.get("id") {
            Some(Json::String(s)) => s.clone(),
            Some(other) => other.to_compact(),
            None => format!("case-{n}"),
        };
        let verdict = match Case::from_json(request) {
            Err(error) => Err(error),
            Ok(case) => match executor.run(&case, Some(&expected), timeout) {
                Outcome::Ran(actual) => Ok(compare(&expected, &actual)),
                Outcome::Unsupported(reason) => Err(reason),
            },
        };
        let known_mismatch = match verdict {
            Ok(Verdict::Same | Verdict::Deviation(_)) => {
                same += 1;
                None
            }
            Ok(Verdict::Mismatch(mismatch)) => {
                known += 1;
                Some(KnownMismatch {
                    reason: "recorded by twig-fuzz record: twig-rs differs (diagnose and replace this text)"
                        .to_owned(),
                    signature: Some(mismatch.signature()),
                })
            }
            Ok(Verdict::Skip(reason)) | Err(reason) => {
                left_out += 1;
                eprintln!("{id}: left out: {reason}");
                continue;
            }
        };
        vectors.push(Vector {
            id,
            case: request.clone(),
            oracle: result.clone(),
            known_mismatch,
            note: None,
        });
    }
    let file = VectorFile {
        description: args
            .value("--description")
            .unwrap_or("Recorded by twig-fuzz record")
            .to_owned(),
        vectors,
    };
    file.save(&out)?;
    println!(
        "{} vectors written to {} ({same} identical, {known} known mismatches, {left_out} left out)",
        file.vectors.len(),
        out.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// The vector files named on the command line (directories are searched recursively).
fn vector_file_args(args: &Args) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for arg in &args.positional {
        let path = PathBuf::from(arg);
        if path.is_dir() {
            files.extend(vector_files(&path));
        } else {
            files.push(path);
        }
    }
    files
}

fn cmd_refresh(args: &Args) -> Result<ExitCode, String> {
    let ids = args.values("--id");
    if ids.is_empty() && !args.has("--all") {
        return Err("refresh needs --id <id> (repeatable) or --all".to_owned());
    }
    let only = (!ids.is_empty()).then_some(ids.as_slice());
    let files = vector_file_args(args);
    let mut oracle = oracle_from_args(args)?;
    oracle.ensure_image()?;
    let mut total = 0;
    for file in &files {
        // each case alone in a fresh PHP process, as a recording of its own would run it
        let mut run = |cases: &[Json]| -> Result<Vec<Json>, String> {
            let requests: Vec<Json> = cases.iter().map(oracle_request).collect();
            oracle.run(&requests, Some("process"))
        };
        for id in refresh_file(file, only, &mut run)? {
            total += 1;
            println!("{} {id}: oracle result recorded again", file.display());
        }
    }
    println!("{total} vectors refreshed");
    Ok(ExitCode::SUCCESS)
}

fn cmd_replay(args: &Args) -> Result<ExitCode, String> {
    let files = vector_file_args(args);
    if args.has("--update") {
        return update_vector_files(&files, args);
    }
    let replayed = replay_files(&files, args.timeout()?);
    let mut failures = 0;
    for r in &replayed {
        match &r.status {
            ReplayStatus::Pass => {}
            ReplayStatus::PassWithDeviation(rules) => {
                println!(
                    "{} {}: identical up to {}",
                    r.file.display(),
                    r.id,
                    rules.join(", ")
                );
            }
            ReplayStatus::PassSignatureChanged { recorded, now } => println!(
                "{} {}: still mismatches, signature changed\n    was: {recorded}\n    now: {now}",
                r.file.display(),
                r.id
            ),
            ReplayStatus::Fail(reason) => {
                failures += 1;
                println!("{} {}: FAIL {reason}", r.file.display(), r.id);
            }
        }
    }
    println!("{} vectors, {failures} failures", replayed.len());
    Ok(if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// `replay --update`: [`update_file`] on every file, then what replays now.
fn update_vector_files(files: &[PathBuf], args: &Args) -> Result<ExitCode, String> {
    let ids = args.values("--id");
    let only = (!ids.is_empty()).then_some(ids.as_slice());
    let timeout = args.timeout()?;
    let (mut total, mut dropped, mut refused, mut failures) = (0, 0, 0, 0);
    for file in files {
        let updated = match update_file(file, timeout, only) {
            Ok(updated) => updated,
            Err(error) => {
                failures += 1;
                println!("{}: FAIL {error}", file.display());
                continue;
            }
        };
        for u in &updated {
            total += 1;
            if u.dropped {
                dropped += 1;
                println!(
                    "{} {}: matches the oracle now, known_mismatch removed",
                    file.display(),
                    u.id
                );
                continue;
            }
            match &u.status {
                ReplayStatus::Pass | ReplayStatus::PassWithDeviation(_) => {}
                ReplayStatus::PassSignatureChanged { recorded, now } => {
                    refused += 1;
                    println!(
                        "{} {}: still mismatches with another signature, left as it is\n    was: {recorded}\n    now: {now}",
                        file.display(),
                        u.id
                    );
                }
                ReplayStatus::Fail(reason) => {
                    failures += 1;
                    println!("{} {}: FAIL {reason}", file.display(), u.id);
                }
            }
        }
    }
    println!(
        "{total} vectors: {dropped} known_mismatch markers removed, {refused} left with a changed \
         signature, {failures} failures"
    );
    Ok(if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn cmd_selftest_fixtures(args: &Args) -> Result<ExitCode, String> {
    let out = PathBuf::from(
        args.value("--out")
            .ok_or("selftest-fixtures needs --out <dir>")?,
    );
    std::fs::create_dir_all(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let corpus = selftest::repo_root().join("tests/twig/oracle-corpus.json");
    let (shared, isolated, left_out) = selftest::fixture_batch(&corpus)?;
    let write = |name: &str, json: Json| -> Result<(), String> {
        let path = out.join(name);
        std::fs::write(&path, json.to_layout(1) + "\n")
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    write("fixtures.json", Json::Array(shared.clone()))?;
    write(
        "fixtures-isolated.json",
        Json::Object(vec![
            ("cases".to_owned(), Json::Array(isolated.clone())),
            ("isolation".to_owned(), Json::str("process")),
        ]),
    )?;
    println!(
        "{} shared + {} isolated cases ({left_out} fixture cases left out)",
        shared.len(),
        isolated.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_selftest_engine(args: &Args) -> Result<ExitCode, String> {
    let out = PathBuf::from(
        args.value("--out")
            .ok_or("selftest-engine needs --out <dir>")?,
    );
    std::fs::create_dir_all(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let (cases, results) =
        selftest::engine_batch(&selftest::repo_root().join("tests/vectors/engine"))?;
    let write = |name: &str, json: Json| -> Result<(), String> {
        let path = out.join(name);
        std::fs::write(&path, json.to_layout(1) + "\n")
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    write(
        "engine.json",
        Json::Object(vec![
            ("cases".to_owned(), Json::Array(cases.clone())),
            ("isolation".to_owned(), Json::str("process")),
        ]),
    )?;
    write("engine-results.json", Json::Array(results))?;
    println!("{} cases", cases.len());
    Ok(ExitCode::SUCCESS)
}

fn cmd_generate(args: &Args) -> Result<ExitCode, String> {
    use twig_fuzz::generators as g;
    let family = args
        .positional
        .first()
        .ok_or("missing family (try `twig-fuzz help`)")?;
    let number = |flag: &str, default: u64| -> Result<u64, String> {
        args.value(flag)
            .map(|v| {
                v.parse::<u64>()
                    .map_err(|e| format!("bad {flag} {v:?}: {e}"))
            })
            .transpose()
            .map(|v| v.unwrap_or(default))
    };
    let seed = number("--seed", 1)?;
    let count =
        usize::try_from(number("--count", 1000)?).map_err(|e| format!("bad --count: {e}"))?;
    let batch = match family.as_str() {
        "expressions" => Json::Array(g::expressions::generate(seed, count)),
        "callables" => Json::Array(g::callables::generate(seed, count)),
        "control" => Json::Array(g::control::generate(seed, count)),
        "composition" => g::composition::batch(&g::composition::Settings {
            seed,
            count,
            ..Default::default()
        }),
        "escaping" => Json::Array(g::escaping::generate(seed, count)),
        "errors" => Json::Array(g::errors::generate(seed, count)),
        "formats" => Json::Array(g::formats::generate(seed, count)),
        "sandbox" => g::sandbox::batch(seed, count),
        "whitespace" => g::whitespace::batch(seed, count),
        "unicode" => g::unicode::batch(seed, count),
        "dynamic" => g::dynamic::batch(seed, count),
        "structure" => g::structure::batch(seed, count),
        other => return Err(format!("unknown family {other:?} (try `twig-fuzz help`)")),
    };
    write_or_print(args, &batch)?;
    Ok(ExitCode::SUCCESS)
}
