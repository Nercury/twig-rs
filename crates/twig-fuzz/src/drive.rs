//! The one-shot driver: batch → oracle (Docker) + twig-rs → diff → clusters → minimisation →
//! report and regression vectors.
//!
//! Output directory:
//!
//! | file | contents |
//! |---|---|
//! | `report.md` | counts, then one section per mismatch cluster: signature, raw count, minimised reproduction, both results, differing fields |
//! | `report.json` | the same, machine readable (`clusters[]` with `signature`, `count`, `ids`, `minimized`, `oracle`, `twig_rs`, `fields`; `second_runs`: the cases twig-rs ran again after a timeout, [`Summary::second_runs`]) |
//! | `oracle.json`, `twig-rs.json` | the raw results of every case, in batch order |
//! | `vectors.json` | one regression vector per cluster (the minimised case and its oracle result), marked `known_mismatch` |

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::kit::case::{Case, CaseData, batch_cases};
use crate::kit::diff::{Mismatch, Verdict, compare, family};
use crate::kit::json::Json;
use crate::kit::result::CaseResult;
use crate::kit::run::Outcome;
use crate::kit::vectors::{KnownMismatch, Vector, VectorFile};
use crate::minimize::{self, SameSignature, oracle_request};
use crate::oracle::Oracle;
use crate::worker::{Executor, SECOND_RUN};

/// Driver settings.
#[derive(Clone, Debug)]
pub struct DriveOptions {
    /// Where the report, results and vectors go.
    pub out_dir: PathBuf,
    /// Minimise one representative per cluster (up to `max_minimize` clusters).
    pub minimize: bool,
    /// Clusters to minimise at most.
    pub max_minimize: usize,
    /// Representatives minimised per cluster (the smallest cases first).
    pub per_cluster: usize,
    /// Time limit of one twig-rs run.
    pub timeout: Duration,
    /// Reuse these oracle results (one per case, in order) instead of running the oracle.
    pub oracle_results: Option<PathBuf>,
    /// The description of the vector file.
    pub description: String,
    /// Mark written vectors `known_mismatch`.
    pub mark_known: bool,
    /// Minimise with the batch's own isolation mode instead of a PHP process per candidate
    /// (faster, but candidates then share compiled templates and declared classes: see
    /// [`minimize`]).
    pub shared_minimize: bool,
    /// Print progress to stderr.
    pub verbose: bool,
}

impl Default for DriveOptions {
    fn default() -> Self {
        DriveOptions {
            out_dir: PathBuf::from("fuzz-out"),
            minimize: true,
            max_minimize: 40,
            per_cluster: 1,
            timeout: Duration::from_secs(5),
            oracle_results: None,
            description: String::new(),
            mark_known: true,
            shared_minimize: false,
            verbose: true,
        }
    }
}

/// One compared case.
#[derive(Clone, Debug)]
pub struct Compared {
    /// The case object.
    pub case: Json,
    /// The oracle's result.
    pub oracle: Option<CaseResult>,
    /// twig-rs's result.
    pub twig_rs: Option<CaseResult>,
    /// The verdict, or why the case was not run.
    pub verdict: Result<Verdict, String>,
}

/// A cluster of mismatches with one signature.
#[derive(Clone, Debug)]
pub struct Cluster {
    /// The signature.
    pub signature: String,
    /// Indexes of the raw mismatches (into [`Summary::cases`]).
    pub members: Vec<usize>,
    /// Minimised reproductions: the case, its oracle and twig-rs results, the mismatch.
    pub minimized: Vec<Reproduction>,
}

/// A (minimised) reproduction.
#[derive(Clone, Debug)]
pub struct Reproduction {
    /// The case.
    pub case: Json,
    /// The oracle's result.
    pub oracle: CaseResult,
    /// twig-rs's result.
    pub twig_rs: CaseResult,
    /// The mismatch.
    pub mismatch: Mismatch,
    /// Oracle candidates the minimiser tested.
    pub tests: usize,
}

/// The outcome of a drive.
#[derive(Clone, Debug, Default)]
pub struct Summary {
    /// Every case.
    pub cases: Vec<Compared>,
    /// The mismatch clusters, largest first.
    pub clusters: Vec<Cluster>,
    /// Seconds spent.
    pub seconds: f64,
}

impl Summary {
    /// (identical, deviation, mismatch, skipped, unsupported) counts.
    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let mut counts = (0, 0, 0, 0, 0);
        for c in &self.cases {
            match &c.verdict {
                Ok(Verdict::Same) => counts.0 += 1,
                Ok(Verdict::Deviation(_)) => counts.1 += 1,
                Ok(Verdict::Mismatch(_)) => counts.2 += 1,
                Ok(Verdict::Skip(_)) => counts.3 += 1,
                Err(_) => counts.4 += 1,
            }
        }
        counts
    }

    /// The ids of the cases twig-rs ran a second time, with the PHP time limit, because they timed
    /// out while the oracle finished them ([`SECOND_RUN`]).
    pub fn second_runs(&self) -> Vec<String> {
        self.cases
            .iter()
            .filter(|c| {
                c.twig_rs
                    .as_ref()
                    .is_some_and(|r| r.extra(SECOND_RUN).is_some())
            })
            .map(|c| {
                let id = c.case.get("id").map(Json::to_compact).unwrap_or_default();
                id.trim_matches('"').to_owned()
            })
            .collect()
    }
}

/// Reads the cases of batch files; each case is paired with its file's isolation mode.
///
/// # Errors
///
/// A file that cannot be read.
pub fn load_batches(files: &[PathBuf]) -> Result<Vec<(Json, Option<String>)>, String> {
    let mut out = Vec::new();
    for file in files {
        let batch = Json::load(file)?;
        let (cases, isolation) = batch_cases(&batch);
        out.extend(cases.into_iter().map(|c| (c, isolation.clone())));
    }
    Ok(out)
}

fn log(options: &DriveOptions, message: &str) {
    if options.verbose {
        eprintln!("twig-fuzz: {message}");
    }
}

/// Runs the cases through the oracle (or reads `options.oracle_results`) and twig-rs, compares
/// them, clusters the mismatches and minimises representatives.
///
/// # Errors
///
/// The oracle failed as a whole.
pub fn drive(
    cases: &[(Json, Option<String>)],
    oracle: &mut Oracle,
    executor: &mut Executor,
    options: &DriveOptions,
) -> Result<Summary, String> {
    let started = Instant::now();
    // parse; unsupported cases never reach the oracle
    let mut compared: Vec<Compared> = Vec::with_capacity(cases.len());
    let mut runnable: Vec<usize> = Vec::new();
    for (json, _) in cases {
        let verdict = match Case::from_json(json) {
            Err(error) => Err(error),
            Ok(case) => match case.unsupported_reason() {
                Some(reason) => Err(reason),
                None => Ok(Verdict::Same),
            },
        };
        if verdict.is_ok() {
            runnable.push(compared.len());
        }
        compared.push(Compared {
            case: json.clone(),
            oracle: None,
            twig_rs: None,
            verdict,
        });
    }

    // the oracle, grouped by isolation mode
    let oracle_results: Vec<Json> = match &options.oracle_results {
        Some(path) => {
            let json = Json::load(path)?;
            let all = json.items().to_vec();
            if all.len() == cases.len() {
                runnable.iter().map(|&i| all[i].clone()).collect()
            } else if all.len() == runnable.len() {
                all
            } else {
                return Err(format!(
                    "{} has {} results for {} cases",
                    path.display(),
                    all.len(),
                    cases.len()
                ));
            }
        }
        None => {
            oracle.ensure_image()?;
            let mut results: Vec<Option<Json>> = vec![None; runnable.len()];
            let mut groups: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
            for (k, &i) in runnable.iter().enumerate() {
                groups.entry(cases[i].1.clone()).or_default().push(k);
            }
            for (isolation, members) in groups {
                let requests: Vec<Json> = members
                    .iter()
                    .map(|&k| oracle_request(&cases[runnable[k]].0))
                    .collect();
                log(
                    options,
                    &format!(
                        "oracle: {} cases (isolation {})",
                        requests.len(),
                        isolation.as_deref().unwrap_or("none")
                    ),
                );
                let answers = oracle.run(&requests, isolation.as_deref())?;
                for (&k, answer) in members.iter().zip(answers) {
                    results[k] = Some(answer);
                }
            }
            results
                .into_iter()
                .map(|r| r.unwrap_or(Json::Null))
                .collect()
        }
    };

    log(options, &format!("twig-rs: {} cases", runnable.len()));
    for (k, &i) in runnable.iter().enumerate() {
        let expected = CaseResult::from_json(&oracle_results[k]);
        let request = oracle_request(&compared[i].case);
        let case = Case::from_json(&request).expect("parsed above");
        let outcome = executor.run(&case, Some(&expected), options.timeout);
        if let Some(result) = outcome.result()
            && let Some(limit) = result.extra(SECOND_RUN)
        {
            log(
                options,
                &format!(
                    "twig-rs: {} took longer than {} ms, ran again within {} ms: {}",
                    case.id,
                    options.timeout.as_millis(),
                    limit.to_compact(),
                    result.phase.as_deref().unwrap_or("finished")
                ),
            );
        }
        compared[i].verdict = match &outcome {
            Outcome::Ran(actual) => Ok(compare(&expected, actual)),
            Outcome::Unsupported(reason) => Err(reason.clone()),
        };
        compared[i].twig_rs = outcome.result().cloned();
        compared[i].oracle = Some(expected);
    }

    // clusters
    let mut by_signature: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, c) in compared.iter().enumerate() {
        if let Ok(Verdict::Mismatch(m)) = &c.verdict {
            by_signature.entry(m.signature()).or_default().push(i);
        }
    }
    let mut clusters: Vec<Cluster> = by_signature
        .into_iter()
        .map(|(signature, members)| Cluster {
            signature,
            members,
            minimized: Vec::new(),
        })
        .collect();
    clusters.sort_by(|a, b| {
        b.members
            .len()
            .cmp(&a.members.len())
            .then(a.signature.cmp(&b.signature))
    });

    if options.minimize {
        let total = clusters.len().min(options.max_minimize);
        for (n, cluster) in clusters.iter_mut().take(options.max_minimize).enumerate() {
            let mut members = cluster.members.clone();
            members.sort_by_key(|&i| compared[i].case.to_compact().len());
            for &i in members.iter().take(options.per_cluster.max(1)) {
                log(
                    options,
                    &format!("minimising {}/{total}: {}", n + 1, cluster.signature),
                );
                if let Some(repro) = minimize_case(
                    &compared[i],
                    &cluster.signature,
                    oracle,
                    executor,
                    options,
                    cases[i].1.clone(),
                ) {
                    cluster.minimized.push(repro);
                }
            }
        }
    }

    let summary = Summary {
        cases: compared,
        clusters,
        seconds: started.elapsed().as_secs_f64(),
    };
    Ok(summary)
}

/// Minimises one mismatching case and records the oracle's result for the minimised case.
fn minimize_case(
    compared: &Compared,
    signature: &str,
    oracle: &mut Oracle,
    executor: &mut Executor,
    options: &DriveOptions,
    isolation: Option<String>,
) -> Option<Reproduction> {
    let isolation = if options.shared_minimize {
        isolation
    } else {
        Some(minimize::ISOLATION.to_owned())
    };
    let mut start = compared.case.clone();
    if let (Some(expected), Ok(case)) = (&compared.oracle, Case::from_json(&start))
        && matches!(case.data, CaseData::Php(_))
        && let Some(data) = expected.extra("data")
    {
        let keys: Vec<Json> = data
            .get("v")
            .map(Json::items)
            .unwrap_or_default()
            .iter()
            .filter_map(|entry| entry.items().first().cloned())
            .collect();
        start.set("_fuzz_data_keys", Json::Array(keys));
    }
    let (minimized, tests) = {
        let mut predicate = SameSignature::new(
            oracle,
            executor,
            signature.to_owned(),
            options.timeout,
            isolation.clone(),
        );
        let settings = minimize::Options {
            verbose: options.verbose,
            drop_test_classes: isolation.as_deref() == Some(minimize::ISOLATION),
            ..minimize::Options::default()
        };
        let result = minimize::minimize(&start, &mut predicate, settings);
        if !predicate.oracle_errors.is_empty() {
            log(
                options,
                &format!(
                    "oracle errors while minimising: {}",
                    predicate.oracle_errors[0]
                ),
            );
        }
        (result.case, result.tests)
    };
    // the minimised case keeps the original's cache directory (candidates had their own)
    let minimized = match minimize::cache_dir(&compared.case) {
        Some(dir) => minimize::with_cache_dir(&minimized, dir),
        None => minimized,
    };
    let request = oracle_request(&minimized);
    let expected = match oracle.run_served(std::slice::from_ref(&request), isolation.as_deref()) {
        Ok(mut results) => CaseResult::from_json(&results.remove(0)),
        Err(error) => {
            log(
                options,
                &format!("cannot re-run the minimised case: {error}"),
            );
            return None;
        }
    };
    let case = Case::from_json(&request).ok()?;
    let actual = executor
        .run(&case, Some(&expected), options.timeout)
        .result()
        .cloned()?;
    match compare(&expected, &actual) {
        Verdict::Mismatch(mismatch) => Some(Reproduction {
            case: request,
            oracle: expected,
            twig_rs: actual,
            mismatch,
            tests,
        }),
        _ => None,
    }
}

/// Writes the report, the raw results and the vectors into `options.out_dir`.
///
/// # Errors
///
/// A file cannot be written.
pub fn write_outputs(summary: &Summary, options: &DriveOptions) -> Result<(), String> {
    let dir = &options.out_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let write = |name: &str, text: String| -> Result<(), String> {
        let path = dir.join(name);
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    let raw = |pick: fn(&Compared) -> Option<&CaseResult>| -> String {
        Json::Array(
            summary
                .cases
                .iter()
                .map(|c| pick(c).map_or(Json::Null, CaseResult::to_json))
                .collect(),
        )
        .to_layout(1)
            + "\n"
    };
    write("oracle.json", raw(|c| c.oracle.as_ref()))?;
    write("twig-rs.json", raw(|c| c.twig_rs.as_ref()))?;
    write("report.md", markdown(summary))?;
    write("report.json", report_json(summary).to_layout(4) + "\n")?;
    let vectors = vectors_of(summary, options);
    if !vectors.vectors.is_empty() {
        vectors.save(&dir.join("vectors.json"))?;
    }
    Ok(())
}

/// The regression vectors of a drive: the first minimised reproduction of every cluster (or its
/// smallest raw case when it was not minimised).
pub fn vectors_of(summary: &Summary, options: &DriveOptions) -> VectorFile {
    let mut vectors = Vec::new();
    for (n, cluster) in summary.clusters.iter().enumerate() {
        let (case, oracle) = match cluster.minimized.first() {
            Some(repro) => (repro.case.clone(), repro.oracle.to_json()),
            None => {
                let Some(&i) = cluster
                    .members
                    .iter()
                    .min_by_key(|&&i| summary.cases[i].case.to_compact().len())
                else {
                    continue;
                };
                let c = &summary.cases[i];
                let Some(oracle) = &c.oracle else { continue };
                (oracle_request(&c.case), oracle.to_json())
            }
        };
        let mut case = case;
        case.remove("_fuzz_data_keys");
        let id = format!("cluster-{:03}", n + 1);
        case.set("id", Json::str(&id));
        let mut oracle = oracle;
        oracle.set("id", Json::str(&id));
        vectors.push(Vector {
            id,
            case,
            oracle,
            known_mismatch: options.mark_known.then(|| KnownMismatch {
                reason: format!(
                    "found by twig-fuzz ({} raw mismatches)",
                    cluster.members.len()
                ),
                signature: Some(cluster.signature.clone()),
            }),
            note: None,
        });
    }
    VectorFile {
        description: if options.description.is_empty() {
            "Mismatches found by twig-fuzz".to_owned()
        } else {
            options.description.clone()
        },
        vectors,
    }
}

fn templates_text(case: &Json) -> String {
    let mut out = String::new();
    if let Ok(parsed) = Case::from_json(case) {
        for (name, source) in &parsed.templates {
            let _ = writeln!(out, "--- {name}");
            out.push_str(&String::from_utf8_lossy(source));
            if !source.ends_with(b"\n") {
                out.push('\n');
            }
        }
    }
    out
}

fn settings_text(case: &Json) -> String {
    let mut fields: Vec<(String, Json)> = Vec::new();
    for (key, value) in case.entries() {
        // the oracle options the tools add are left out
        if !matches!(
            key.as_str(),
            "templates"
                | "templates_b64"
                | "id"
                | "typed_data"
                | "_fuzz_data_keys"
                | "memory_limit"
                | "time_limit"
                | "clock"
        ) {
            fields.push((key.clone(), value.clone()));
        }
    }
    Json::Object(fields).to_compact()
}

/// The Markdown report.
pub fn markdown(summary: &Summary) -> String {
    let (same, deviation, mismatch, skipped, unsupported) = summary.counts();
    let mut out = String::new();
    let _ = writeln!(out, "# twig-fuzz report\n");
    let _ = writeln!(
        out,
        "{} cases in {:.1} s: {same} identical, {deviation} identical up to known deviations, \
         {mismatch} mismatches in {} clusters, {skipped} not comparable, {unsupported} unsupported.\n",
        summary.cases.len(),
        summary.seconds,
        summary.clusters.len()
    );
    let second = summary.second_runs();
    if !second.is_empty() {
        let _ = writeln!(
            out,
            "Ran a second time in twig-rs, with the PHP time limit, after timing out: {}.\n",
            second.join(", ")
        );
    }
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for c in &summary.cases {
        match &c.verdict {
            Err(reason) => *reasons.entry(format!("unsupported: {reason}")).or_default() += 1,
            Ok(Verdict::Skip(reason)) => {
                *reasons
                    .entry(format!("not comparable: {reason}"))
                    .or_default() += 1;
            }
            Ok(Verdict::Deviation(rules)) => {
                *reasons
                    .entry(format!("deviation: {}", rules.join(", ")))
                    .or_default() += 1;
            }
            _ => {}
        }
    }
    let mut families: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for cluster in &summary.clusters {
        let entry = families.entry(family(&cluster.signature)).or_default();
        entry.0 += 1;
        entry.1 += cluster.members.len();
    }
    if !families.is_empty() {
        let mut rows: Vec<(&String, &(usize, usize))> = families.iter().collect();
        rows.sort_by(|a, b| b.1.1.cmp(&a.1.1).then(a.0.cmp(b.0)));
        let _ = writeln!(out, "| family | clusters | mismatches |\n|---|---|---|");
        for (name, (clusters, count)) in rows {
            let _ = writeln!(
                out,
                "| `{}` | {clusters} | {count} |",
                name.replace('|', "\\|")
            );
        }
        out.push('\n');
    }
    if !reasons.is_empty() {
        let _ = writeln!(out, "| other outcomes | cases |\n|---|---|");
        for (reason, count) in &reasons {
            let reason: String = reason.chars().take(200).collect();
            let _ = writeln!(
                out,
                "| {} | {count} |",
                reason.replace('|', "\\|").replace('\n', " ")
            );
        }
        out.push('\n');
    }
    for (n, cluster) in summary.clusters.iter().enumerate() {
        let _ = writeln!(
            out,
            "## Cluster {}: {} raw mismatches\n",
            n + 1,
            cluster.members.len()
        );
        let _ = writeln!(
            out,
            "Signature: `{}`\n",
            cluster.signature.replace('`', "'")
        );
        let ids: Vec<String> = cluster
            .members
            .iter()
            .take(12)
            .map(|&i| {
                let id = summary.cases[i]
                    .case
                    .get("id")
                    .map(Json::to_compact)
                    .unwrap_or_default();
                id.trim_matches('"').to_owned()
            })
            .collect();
        let _ = writeln!(
            out,
            "Cases: {}{}\n",
            ids.join(", "),
            if cluster.members.len() > 12 {
                ", ..."
            } else {
                ""
            }
        );
        let (case, oracle, twig_rs, mismatch) = match cluster.minimized.first() {
            Some(r) => (&r.case, &r.oracle, &r.twig_rs, r.mismatch.clone()),
            None => {
                let c = &summary.cases[cluster.members[0]];
                let Ok(Verdict::Mismatch(m)) = &c.verdict else {
                    continue;
                };
                let (Some(o), Some(t)) = (&c.oracle, &c.twig_rs) else {
                    continue;
                };
                (&c.case, o, t, m.clone())
            }
        };
        let label = if cluster.minimized.is_empty() {
            "Smallest raw case"
        } else {
            "Minimised"
        };
        let _ = writeln!(out, "{label}:\n\n```twig\n{}```\n", templates_text(case));
        let _ = writeln!(
            out,
            "Settings: `{}`\n",
            settings_text(case).replace('`', "'")
        );
        let _ = writeln!(out, "- PHP:     {}", oracle.summary());
        let _ = writeln!(out, "- twig-rs: {}\n", twig_rs.summary());
        let _ = writeln!(out, "```text\n{}```\n", mismatch.describe());
    }
    out
}

fn report_json(summary: &Summary) -> Json {
    let (same, deviation, mismatch, skipped, unsupported) = summary.counts();
    let clusters = summary
        .clusters
        .iter()
        .map(|cluster| {
            let ids: Vec<Json> = cluster
                .members
                .iter()
                .map(|&i| {
                    summary.cases[i]
                        .case
                        .get("id")
                        .cloned()
                        .unwrap_or(Json::Null)
                })
                .collect();
            let minimized: Vec<Json> = cluster
                .minimized
                .iter()
                .map(|r| {
                    Json::Object(vec![
                        ("case".to_owned(), r.case.clone()),
                        ("oracle".to_owned(), r.oracle.to_json()),
                        ("twig_rs".to_owned(), r.twig_rs.to_json()),
                        (
                            "fields".to_owned(),
                            Json::Array(
                                r.mismatch
                                    .fields
                                    .iter()
                                    .map(|f| {
                                        Json::Object(vec![
                                            ("field".to_owned(), Json::str(&f.field)),
                                            ("php".to_owned(), Json::str(&f.expected)),
                                            ("rs".to_owned(), Json::str(&f.actual)),
                                        ])
                                    })
                                    .collect(),
                            ),
                        ),
                        ("tests".to_owned(), Json::int(r.tests as i64)),
                    ])
                })
                .collect();
            Json::Object(vec![
                ("signature".to_owned(), Json::str(&cluster.signature)),
                ("count".to_owned(), Json::int(cluster.members.len() as i64)),
                ("ids".to_owned(), Json::Array(ids)),
                ("minimized".to_owned(), Json::Array(minimized)),
            ])
        })
        .collect();
    Json::Object(vec![
        ("cases".to_owned(), Json::int(summary.cases.len() as i64)),
        ("identical".to_owned(), Json::int(same as i64)),
        ("deviations".to_owned(), Json::int(deviation as i64)),
        ("mismatches".to_owned(), Json::int(mismatch as i64)),
        ("skipped".to_owned(), Json::int(skipped as i64)),
        ("unsupported".to_owned(), Json::int(unsupported as i64)),
        (
            "second_runs".to_owned(),
            Json::Array(summary.second_runs().iter().map(Json::str).collect()),
        ),
        ("clusters".to_owned(), Json::Array(clusters)),
    ])
}

/// Where a path is shown in messages (relative to the working directory when possible).
pub fn display_path(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}
