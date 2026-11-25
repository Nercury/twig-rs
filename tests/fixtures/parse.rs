//! The `.test` fixture format: discovery and parsing exactly as Twig 3.31.1's test harness does it
//! (`IntegrationTestCase::assembleTests()` and `parseTemplates()`, TWIG/src/Test/IntegrationTestCase.php;
//! `reference/twig/test-suite.md` §1).
//!
//! Upstream parses a file with four PCRE patterns, verbatim:
//!
//! ```text
//! EXCEPTION  /--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)/sx
//! OUTPUT     /--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)--DATA--.*?--EXPECT--.*/s
//! OUTPUTS    /--DATA--(.*?)(?:--CONFIG--(.*?))?--EXPECT--(.*?)(?=\-\-DATA\-\-|$)/s
//! TEMPLATES  /--TEMPLATE(?:\((.*?)\))?--(.*?)(?=\-\-TEMPLATE|$)/s
//! ```
//!
//! `assembleTests()` tries EXCEPTION (`IntegrationTestCase.php:191`; its `x` flag is inert), then OUTPUT (`:198`)
//! with `preg_match_all(OUTPUTS)` over the whole file (`:204`); `parseTemplates()` runs `preg_match_all(TEMPLATES)`
//! over the templates group (`:390`). This module computes, by byte scanning, the match PCRE's backtracking returns
//! for those patterns on any input; the comments derive each step from the pattern. Two checks keep it honest:
//! `tests/fixtures/vectors/fixture_parser.json` (the real `assembleTests()` on edge cases and random inputs, see its
//! `.gen.php`) and the corpus (PHP's parse of all 488 fixtures). Inputs on which PCRE gives up (backtrack limit) are
//! not reproduced; no fixture comes close.
//!
//! Strings are bytes, as in PHP. `\s` is PCRE2's without the `u` flag: space, `\t`, `\n`, `\x0B`, `\x0C`, `\r`.
//! `$` (no `m`, no `D` modifier) matches at the end of the subject or before a final `"\n"`.

use std::io;
use std::path::{Path, PathBuf};

/// PCRE2's `\s` without the `u` flag.
pub const PCRE_WHITESPACE: &[u8] = b" \t\n\x0B\x0C\r";

const TEST: &[u8] = b"--TEST--";
const CONDITION: &[u8] = b"--CONDITION--";
const DEPRECATION: &[u8] = b"--DEPRECATION--";
const TEMPLATE: &[u8] = b"--TEMPLATE";
const DATA: &[u8] = b"--DATA--";
const CONFIG: &[u8] = b"--CONFIG--";
const EXPECT: &[u8] = b"--EXPECT--";
const EXCEPTION: &[u8] = b"--EXCEPTION--";

/// One `.test` file as `assembleTests()` returns it: `[$file, $message, $condition, $templates, $exception,
/// $outputs, $deprecation]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFixture {
    /// `$message`: the `--TEST--` description, without surrounding whitespace.
    pub message: Vec<u8>,
    /// `$condition`: the `--CONDITION--` PHP expression (`""` when absent). It keeps trailing whitespace and, when the
    /// file has several templates or a `--DEPRECATION--` section, swallows them (greedy `(.*)`).
    pub condition: Vec<u8>,
    /// `$deprecation`: the `--DEPRECATION--` section without surrounding whitespace (`""` when absent).
    pub deprecation: Vec<u8>,
    /// `$templates`: name => source in section order; a repeated name replaces the earlier source in place.
    pub templates: Vec<TemplateSection>,
    /// `$exception`: the raw `--EXCEPTION--` text for exception fixtures, `None` (PHP `false`) for output fixtures.
    pub exception: Option<Vec<u8>>,
    /// `$outputs`: the DATA/CONFIG/EXPECT sections; exception fixtures have exactly one (`[null, $data, null, '']`).
    pub outputs: Vec<OutputSection>,
}

/// One template of a fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSection {
    /// `$match[1] ?: 'index.twig'`: the name in `--TEMPLATE(name)--`, `index.twig` when it is absent, empty or `0`.
    pub name: Vec<u8>,
    /// The source. It starts right after the marker, so it keeps the newline that follows `--TEMPLATE--` and the
    /// first visible line of the template is line 2.
    pub source: Vec<u8>,
}

/// One DATA/CONFIG/EXPECT section (`[$full, $data, $config, $expect]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSection {
    /// `$data`: the PHP statements of `--DATA--` (`""` when an exception fixture has none).
    pub data: Vec<u8>,
    /// `$config`: the PHP statements of `--CONFIG--`; `Some("")` when an output section has none, `None` (PHP
    /// `null`) in exception fixtures.
    pub config: Option<Vec<u8>>,
    /// `$expect`: the raw `--EXPECT--` text (`""` in exception fixtures).
    pub expect: Vec<u8>,
}

impl TemplateSection {
    /// The name as text (template names are UTF-8 in every fixture).
    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }
}

impl ParsedFixture {
    /// The template sources of DATA/EXPECT case `case` as the harness builds them: before case `i`,
    /// `doIntegrationTest()` appends `i` spaces to every template of the already modified list
    /// (`IntegrationTestCase.php:255-258`, to force a recompilation), so case `i` has `i * (i + 1) / 2` spaces
    /// appended in total.
    pub fn case_templates(&self, case: usize) -> Vec<TemplateSection> {
        let spaces = case * (case + 1) / 2;
        self.templates
            .iter()
            .map(|t| {
                let mut source = t.source.clone();
                source.resize(source.len() + spaces, b' ');
                TemplateSection {
                    name: t.name.clone(),
                    source,
                }
            })
            .collect()
    }
}

/// A `.test` file found by [`discover`].
#[derive(Debug, Clone)]
pub struct FixtureFile {
    /// The test name: the path relative to the fixtures directory, with `/` separators (`filters/abs.test`).
    pub name: String,
    /// The file path.
    pub path: PathBuf,
    /// Whether upstream runs it through `testLegacyIntegration()` (`@group legacy`): its path contains
    /// `.legacy.test`.
    pub legacy: bool,
}

/// The file walk of `assembleTests()`: every file below `dir` (recursively) whose path matches `/\.test$/`, named by
/// its path relative to `dir`. Upstream iterates in filesystem order and splits the files between
/// `testIntegration()` and `testLegacyIntegration()`; here they are sorted by name, with [`FixtureFile::legacy`].
pub fn discover(dir: &Path) -> io::Result<Vec<FixtureFile>> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<FixtureFile>) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let name = format!("{prefix}{file_name}");
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(&path, &format!("{name}/"), out)?;
            } else if name.ends_with(".test") || name.ends_with(".test\n") {
                // `$` also matches before a final newline
                let legacy = path.to_string_lossy().contains(".legacy.test");
                out.push(FixtureFile { name, path, legacy });
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, "", &mut files)?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// Parses a `.test` file like `assembleTests()`: EXCEPTION first, then OUTPUT with OUTPUTS. `None` is upstream's
/// `InvalidArgumentException('Test "<name>" is not valid.')`.
pub fn parse_fixture(text: &[u8]) -> Option<ParsedFixture> {
    parse_exception_fixture(text).or_else(|| parse_output_fixture(text))
}

/// EXCEPTION: `$message = $match[1]`, `$condition = $match[2]`, `$deprecation = $match[3]`,
/// `$templates = parseTemplates($match[4])`, `$exception = $match[6]`, `$outputs = [[null, $match[5], null, '']]`.
fn parse_exception_fixture(text: &[u8]) -> Option<ParsedFixture> {
    let last_exception = rfind(text, EXCEPTION);
    let head = match_head(text, Tail::Exception { last_exception })?;
    // After the templates group: `\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)`. The group ends at the first
    // position where this matches (see `templates_group_end`), so one of the two branches holds here.
    let x = skip_ws(text, head.block.1);
    let (data, exception_marker) = match last_exception {
        // The optional DATA group is tried first. Its greedy `(.*)` gives back characters from the end of the
        // subject until `\s*--EXCEPTION--` matches: it ends at the last `--EXCEPTION--`, keeping trailing whitespace.
        Some(z) if starts_at(text, x, DATA) && z >= x + DATA.len() => {
            (&text[skip_ws(text, x + DATA.len())..z], z)
        }
        // Without DATA, `--EXCEPTION--` comes right after the whitespace.
        _ if starts_at(text, x, EXCEPTION) => (&b""[..], x),
        _ => return None,
    };
    Some(ParsedFixture {
        message: text[head.message.0..head.message.1].to_vec(),
        condition: head
            .condition
            .map_or_else(Vec::new, |(s, e)| text[s..e].to_vec()),
        deprecation: head
            .deprecation
            .map_or_else(Vec::new, |(s, e)| text[s..e].to_vec()),
        templates: split_templates(&text[head.block.0..head.block.1]),
        // `\s*(.*)`: everything after the marker and the whitespace, up to the end (no `$`, so the final newline stays)
        exception: Some(text[skip_ws(text, exception_marker + EXCEPTION.len())..].to_vec()),
        outputs: vec![OutputSection {
            data: data.to_vec(),
            config: None,
            expect: Vec::new(),
        }],
    })
}

/// OUTPUT (the same groups 1-4 as EXCEPTION) plus OUTPUTS over the whole file for `$outputs`.
fn parse_output_fixture(text: &[u8]) -> Option<ParsedFixture> {
    let head = match_head(
        text,
        Tail::Output {
            last_expect: rfind(text, EXPECT),
        },
    )?;
    Some(ParsedFixture {
        message: text[head.message.0..head.message.1].to_vec(),
        condition: head
            .condition
            .map_or_else(Vec::new, |(s, e)| text[s..e].to_vec()),
        deprecation: head
            .deprecation
            .map_or_else(Vec::new, |(s, e)| text[s..e].to_vec()),
        templates: split_templates(&text[head.block.0..head.block.1]),
        exception: None,
        outputs: output_sections(text),
    })
}

/// What a pattern requires after its templates group (`((?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+)`).
#[derive(Clone, Copy)]
enum Tail {
    /// EXCEPTION: `\s*(?:--DATA--\s*(.*))?\s*--EXCEPTION--\s*(.*)`.
    Exception { last_exception: Option<usize> },
    /// OUTPUT: `--DATA--.*?--EXPECT--.*`.
    Output { last_expect: Option<usize> },
}

impl Tail {
    /// Whether the tail matches at `x`.
    fn matches_at(self, text: &[u8], x: usize) -> bool {
        match self {
            Tail::Exception { last_exception } => {
                let x = skip_ws(text, x);
                // `(.*)` and the `\s*` around it can absorb anything up to a later `--EXCEPTION--`
                (starts_at(text, x, DATA) && last_exception.is_some_and(|z| z >= x + DATA.len()))
                    || starts_at(text, x, EXCEPTION)
            }
            Tail::Output { last_expect } => {
                starts_at(text, x, DATA) && last_expect.is_some_and(|q| q >= x + DATA.len())
            }
        }
    }

    /// Whether the tail matches at some position `>= from`.
    fn matches_from(self, text: &[u8], from: usize) -> bool {
        match self {
            // the position of the last `--EXCEPTION--` itself, if nothing earlier
            Tail::Exception { last_exception } => last_exception.is_some_and(|z| z >= from),
            // the first `--DATA--` from there, if a later `--EXPECT--` follows it (a later one cannot do better)
            Tail::Output { last_expect } => find(text, DATA, from)
                .is_some_and(|d| last_expect.is_some_and(|q| q >= d + DATA.len())),
        }
    }
}

/// The match of groups 1-4, shared by both `assembleTests()` patterns, as byte ranges.
struct Head {
    message: (usize, usize),
    condition: Option<(usize, usize)>,
    deprecation: Option<(usize, usize)>,
    block: (usize, usize),
}

/// `--TEST--\s*(.*?)\s*(?:--CONDITION--\s*(.*))?\s*(?:--DEPRECATION--\s*(.*?))?\s*((?:--TEMPLATE...)+)` + tail.
///
/// - The match starts at the first `--TEST--`: a match from a later `--TEST--` would also be a match from the first
///   one, with a longer message, and PCRE tries start positions from the left.
/// - `\s*` after `--TEST--` takes all the whitespace (giving some back cannot help: the message can absorb it).
/// - The message `(.*?)` is lazy: it ends at the first position `e` from which the rest matches. The rest skips
///   whitespace (so `e` is where the whitespace before the next element starts) and then needs, in this order of
///   preference: a `--CONDITION--` section followed by a viable template header, a `--DEPRECATION--` section followed
///   by one, or a viable template header (see [`viable_header`]).
/// - CONDITION: the greedy `(.*)` gives back characters from the end of the subject until the rest matches, which
///   first happens at the last viable template header: the condition keeps its trailing whitespace and swallows any
///   DEPRECATION section and every template but the last one; the DEPRECATION group does not participate.
/// - DEPRECATION: the lazy `(.*?)` ends at the whitespace before the first viable template header.
fn match_head(text: &[u8], tail: Tail) -> Option<Head> {
    let test = find(text, TEST, 0)?;
    let message_start = skip_ws(text, test + TEST.len());
    let mut e = message_start;
    loop {
        let p = skip_ws(text, e);
        if starts_at(text, p, CONDITION) {
            let condition_start = skip_ws(text, p + CONDITION.len());
            if let Some((h, header_end)) = last_viable_header(text, condition_start, tail) {
                return Some(Head {
                    message: (message_start, e),
                    condition: Some((condition_start, h)),
                    deprecation: None,
                    block: (h, templates_group_end(text, header_end, tail)?),
                });
            }
        } else if starts_at(text, p, DEPRECATION) {
            let deprecation_start = skip_ws(text, p + DEPRECATION.len());
            if let Some((h, header_end)) = first_viable_header(text, deprecation_start, tail) {
                let deprecation_end = ws_run_start(text, h).max(deprecation_start);
                return Some(Head {
                    message: (message_start, e),
                    condition: None,
                    deprecation: Some((deprecation_start, deprecation_end)),
                    block: (h, templates_group_end(text, header_end, tail)?),
                });
            }
        } else if let Some(header_end) = viable_header(text, p, tail) {
            return Some(Head {
                message: (message_start, e),
                condition: None,
                deprecation: None,
                block: (p, templates_group_end(text, header_end, tail)?),
            });
        }
        if p >= text.len() {
            return None;
        }
        // every `e` up to `p` skips to the same `p`
        e = p + 1;
    }
}

/// `--TEMPLATE(?:\((.*?)\))?--` at `at`: the byte range of the name (`None` when the optional group does not
/// participate) and the end of the header. The lazy name ends at the first `)--` after `(`; when there is none the
/// group fails, and so does the alternative without it (`--` cannot match at `(`). The rest of both patterns after
/// the header always matches once a header does, so later `)--` occurrences are never needed.
fn template_header(text: &[u8], at: usize) -> Option<(Option<(usize, usize)>, usize)> {
    if !starts_at(text, at, TEMPLATE) {
        return None;
    }
    let after = at + TEMPLATE.len();
    if text.get(after) == Some(&b'(') {
        let close = find(text, b")--", after + 1)?;
        Some((Some((after + 1, close)), close + 3))
    } else if starts_at(text, after, b"--") {
        Some((None, after + 2))
    } else {
        None
    }
}

/// The end of the header at `at` when the templates group can start there: the header is valid and the tail
/// matches somewhere after it (the lazy bodies can stretch over anything in between).
fn viable_header(text: &[u8], at: usize, tail: Tail) -> Option<usize> {
    let (_, header_end) = template_header(text, at)?;
    tail.matches_from(text, header_end).then_some(header_end)
}

/// The first viable header at or after `from`: `(start, header end)`.
fn first_viable_header(text: &[u8], from: usize, tail: Tail) -> Option<(usize, usize)> {
    let mut pos = from;
    while let Some(h) = find(text, TEMPLATE, pos) {
        if let Some(header_end) = viable_header(text, h, tail) {
            return Some((h, header_end));
        }
        pos = h + 1;
    }
    None
}

/// The last viable header at or after `from`: `(start, header end)`.
fn last_viable_header(text: &[u8], from: usize, tail: Tail) -> Option<(usize, usize)> {
    let mut last = None;
    let mut pos = from;
    while let Some(h) = find(text, TEMPLATE, pos) {
        if let Some(header_end) = viable_header(text, h, tail) {
            last = Some((h, header_end));
        }
        pos = h + 1;
    }
    last
}

/// The end of the templates group whose first header ends at `x`. In `(?:--TEMPLATE(?:\(.*?\))?--(?:.*?))+` the body
/// is lazy and the loop greedy, so at each position PCRE first tries another iteration (a header starting there),
/// then leaving the loop (the tail), then one more body character. An iteration that is entered succeeds exactly when
/// the tail matches somewhere after its header, so entering it is decided up front.
fn templates_group_end(text: &[u8], mut x: usize, tail: Tail) -> Option<usize> {
    loop {
        if let Some(header_end) = viable_header(text, x, tail) {
            x = header_end;
            continue;
        }
        if tail.matches_at(text, x) {
            return Some(x);
        }
        if x >= text.len() {
            return None;
        }
        x += 1;
    }
}

/// OUTPUTS with `PREG_SET_ORDER`: successive matches over the whole file, each search starting where the
/// previous match ended (the first `--DATA--` of the file may lie before the templates).
fn output_sections(text: &[u8]) -> Vec<OutputSection> {
    let mut sections = Vec::new();
    let mut pos = 0;
    while let Some(d) = find(text, DATA, pos) {
        match output_section_at(text, d) {
            Some((section, end)) => {
                sections.push(section);
                pos = end;
            }
            None => pos = d + 1,
        }
    }
    sections
}

/// One match of OUTPUTS at the `--DATA--` at `d`, and its end.
fn output_section_at(text: &[u8], d: usize) -> Option<(OutputSection, usize)> {
    let data_start = d + DATA.len();
    // `(.*?)` lazy: the first position where `(?:--CONFIG--(.*?))?--EXPECT--` matches. The optional CONFIG group is
    // tried first; its lazy body ends at the first `--EXPECT--` after the marker.
    let mut x = data_start;
    let (data_end, config, expect_start) = loop {
        if starts_at(text, x, CONFIG)
            && let Some(q) = find(text, EXPECT, x + CONFIG.len())
        {
            break (x, Some((x + CONFIG.len(), q)), q + EXPECT.len());
        }
        if starts_at(text, x, EXPECT) {
            break (x, None, x + EXPECT.len());
        }
        if x >= text.len() {
            return None;
        }
        x += 1;
    };
    // `(.*?)(?=--DATA--|$)`: up to the next `--DATA--`, else up to the end minus one final "\n"
    let expect_end = find(text, DATA, expect_start).unwrap_or_else(|| dollar(text, expect_start));
    Some((
        OutputSection {
            data: text[data_start..data_end].to_vec(),
            // an unmatched group before a matched one is "" in PHP
            config: Some(config.map_or_else(Vec::new, |(s, e)| text[s..e].to_vec())),
            expect: text[expect_start..expect_end].to_vec(),
        },
        expect_end,
    ))
}

/// `parseTemplates()`: TEMPLATES with `PREG_SET_ORDER` over the templates group, then
/// `$templates[$match[1] ?: 'index.twig'] = $match[2]`. A match attempt that fails (a `--TEMPLATE` that is not a
/// valid header) moves on to the next `--TEMPLATE`; the text it covered belongs to no template.
fn split_templates(block: &[u8]) -> Vec<TemplateSection> {
    let mut templates: Vec<TemplateSection> = Vec::new();
    let mut pos = 0;
    while let Some(start) = find(block, TEMPLATE, pos) {
        let Some((name, body_start)) = template_header(block, start) else {
            pos = start + 1;
            continue;
        };
        // `(.*?)(?=--TEMPLATE|$)`: up to the next `--TEMPLATE` (valid header or not), else up to the end minus one
        // final "\n"
        let body_end = find(block, TEMPLATE, body_start).map_or_else(
            || dollar(block, body_start),
            |n| n.min(dollar(block, body_start)),
        );
        let name = name.map_or(&b""[..], |(s, e)| &block[s..e]);
        // `?:`: "" and "0" are falsy in PHP
        let name = if name.is_empty() || name == b"0" {
            &b"index.twig"[..]
        } else {
            name
        };
        let source = block[body_start..body_end].to_vec();
        // PHP array assignment: an existing key keeps its position (numeric-string keys become integers, which
        // never makes two different names equal)
        match templates.iter_mut().find(|t| t.name == name) {
            Some(existing) => existing.source = source,
            None => templates.push(TemplateSection {
                name: name.to_vec(),
                source,
            }),
        }
        pos = body_end;
    }
    templates
}

/// The first position `>= from` where `$` matches: before a final `"\n"`, else at the end.
fn dollar(text: &[u8], from: usize) -> usize {
    if text.last() == Some(&b'\n') && from < text.len() {
        text.len() - 1
    } else {
        text.len()
    }
}

/// The first occurrence of `needle` at or after `from`.
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// The last occurrence of `needle`.
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn starts_at(hay: &[u8], at: usize, needle: &[u8]) -> bool {
    hay.get(at..).is_some_and(|rest| rest.starts_with(needle))
}

/// The first position `>= from` that is not `\s`.
fn skip_ws(hay: &[u8], from: usize) -> usize {
    let mut i = from.min(hay.len());
    while i < hay.len() && PCRE_WHITESPACE.contains(&hay[i]) {
        i += 1;
    }
    i
}

/// The start of the run of `\s` that ends at `at`.
fn ws_run_start(hay: &[u8], at: usize) -> usize {
    let mut i = at;
    while i > 0 && PCRE_WHITESPACE.contains(&hay[i - 1]) {
        i -= 1;
    }
    i
}
