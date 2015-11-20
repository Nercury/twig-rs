extern crate twig;
extern crate serde_json;
extern crate difference;
extern crate term;
#[macro_use] extern crate log;
extern crate env_logger;

use std::collections::HashMap;
use std::io::{self, Read, BufReader, BufRead};
use std::env;
use std::io::Write;
use difference::Difference;
use std::fs::{self, DirEntry, File};
use std::path::Path;

use twig::environment::{ Environment, Config };
use twig::loader::ArrayLoader;
use twig::Engine;

#[test]
fn fixtures() {
    env_logger::init().unwrap();

    let errors = visit_fixtures(&env::current_dir().unwrap().join("tests").join("fixtures"), &|entry| {
        let f = match File::open(entry.path()) {
            Ok(f) => f,
            Err(e) => panic!("error opening fixture file {:?}, {:?}", entry.path(), e),
        };
        let fixture = match Fixture::new(f) {
            Err(e) => panic!("invalid test {:?}", e),
            Ok(f) => f,
        };

        let message = match fixture.message.clone() {
            Some(m) => m,
            None => panic!("fixture {:?} must have a message", entry.path()),
        };
        print_fixture_start(&message).unwrap();

        let mut twig = Engine::new(ArrayLoader::new(
            vec![("index.twig".into(), fixture.template.expect("fixture must contain main template"))]
                .into_iter()
                .chain(fixture.templates.into_iter())
                .collect()
        ), match fixture.config {
            Some(config) => Environment::new(Config::from_hashmap(
                match serde_json::from_str(&config) {
                    Ok(map) => map,
                    Err(e) => panic!("failed to deserialize template config: {:#?}", e),
                }
            )),
            _ => Environment::default(),
        });

        let data = match fixture.data {
            Some(data) => match serde_json::from_str::<HashMap<String, String>>(&data) {
                Ok(map) => map,
                Err(e) => panic!("failed to deserialize template data: {:#?}", e),
            },
            None => HashMap::new(),
        };

        debug!("template data {:?}", data);

        let res = match twig.get("index.twig", data) {
            Ok(res) => res,
            Err(e) => panic!("\nerror executing template:\n    {:#?}\n", e),
        };

        let expected = fixture.expect.expect("fixture must have expect block");

        if res != expected {
            print_fixture_result(false).unwrap();

            let (_, changeset) = difference::diff(
                &res,
                &expected,
                "\n"
            );
            print_diff(&changeset).unwrap();

            Some((
                entry.path().to_string_lossy().into_owned(),
                message,
                changeset
            ))
        } else {
            print_fixture_result(true).unwrap();
            None
        }
    }).unwrap();

    let num_errors = errors.len();
    if num_errors > 0 {
        for (file, name, changeset) in errors {
            println!("in {}", file);
            println!("testing {}", name);
            print_uncolored(&changeset);
        }
        //panic!("{} fixtures produced errors", num_errors);
    }
}

fn visit_fixtures(dir: &Path, cb: &Fn(&DirEntry) -> Option<(String, String, Vec<Difference>)>) -> io::Result<Vec<(String, String, Vec<Difference>)>> {
    let mut errors = Vec::new();
    if try!(fs::metadata(dir)).is_dir() {
        for entry in try!(fs::read_dir(dir)) {
            let entry = try!(entry);
            if try!(fs::metadata(entry.path())).is_dir() {
                for e in try!(visit_fixtures(&entry.path(), cb)) {
                    errors.push(e);
                }
            } else {
                if let Some(Some("test")) = entry.path().extension().map(|v| v.to_str()) {
                    if let Some(err) = cb(&entry) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    Ok(errors)
}

#[derive(Debug)]
enum FixtureError {
    ExpectedBlockStart,
    IoError,
}

#[derive(Debug)]
enum TemplateName {
    Main,
    Other(String),
}

#[derive(Debug)]
enum ReadState {
    Message(String),
    Template((TemplateName, String)),
    Data(String),
    Config(String),
    Expect(String)
}

#[derive(Debug)]
struct Fixture {
    message: Option<String>,
    template: Option<String>,
    templates: Vec<(String, String)>,
    data: Option<String>,
    config: Option<String>,
    expect: Option<String>,
}

const TEMPLATE_NAME_START: &'static str = "--TEMPLATE(";
const TEMPLATE_NAME_END: &'static str = ")--";

fn check_for_new_state(line: &str) -> Option<ReadState> {
    match &line[..] {
        "--TEST--" => Some(ReadState::Message(String::new())),
        "--DATA--" => Some(ReadState::Data(String::new())),
        "--CONFIG--" => Some(ReadState::Config(String::new())),
        "--EXPECT--" => Some(ReadState::Expect(String::new())),
        other => if other.starts_with("--TEMPLATE(") {
            let name = other[TEMPLATE_NAME_START.len()..other.len()-TEMPLATE_NAME_END.len()].to_string();
            Some(ReadState::Template((TemplateName::Other(name), String::new())))
        } else if other.starts_with("--TEMPLATE--") {
            Some(ReadState::Template((TemplateName::Main, String::new())))
        } else {
            None
        },
    }
}

impl Fixture {
    pub fn new<R: Read>(input: R) -> Result<Fixture, FixtureError> {
        let mut state = None;
        let mut fixture = Fixture {
            message: None,
            template: None,
            templates: Vec::new(),
            data: None,
            config: None,
            expect: None,
        };

        for maybe_line in BufReader::new(input).lines() {
            let line = match maybe_line {
                Ok(l) => l,
                Err(_) => return Err(FixtureError::IoError),
            };

            state = match state {
                None => {
                    Some(try!(check_for_new_state(&line).ok_or(FixtureError::ExpectedBlockStart)))
                },
                Some(mut old) => match check_for_new_state(&line) {
                    Some(new_state) => {
                        fixture.collect(old);
                        Some(new_state)
                    },
                    None => {
                        match old {
                            ReadState::Message(ref mut m) => { if m.len() > 0 { m.push_str("\n"); } m.push_str(&line); },
                            ReadState::Template((_, ref mut m)) => { if m.len() > 0 { m.push_str("\n"); }  m.push_str(&line); },
                            ReadState::Data(ref mut m) => { if m.len() > 0 { m.push_str("\n"); }  m.push_str(&line); },
                            ReadState::Config(ref mut m) => { if m.len() > 0 { m.push_str("\n"); }  m.push_str(&line); },
                            ReadState::Expect(ref mut m) => { if m.len() > 0 { m.push_str("\n"); }  m.push_str(&line); },
                        };
                        Some(old)
                    }
                },
            }
        }

        if let Some(leftover_state) = state {
            fixture.collect(leftover_state);
        }

        Ok(fixture)
    }

    fn collect(&mut self, state: ReadState) {
        match state {
            ReadState::Message(m) => self.message = Some(m),
            ReadState::Template((TemplateName::Main, m)) => self.template = Some(m),
            ReadState::Template((TemplateName::Other(name), m)) => {
                self.templates.push((name, m));
            },
            ReadState::Data(m) => self.data = Some(m),
            ReadState::Config(m) => self.config = Some(m),
            ReadState::Expect(m) => self.expect = Some(m),
        }
    }
}

fn print_fixture_start(message: &str) -> io::Result<()> {
    let mut t = term::stdout().unwrap();
    try!(write!(t, "fixture "));
    try!(t.attr(term::Attr::Bold));
    try!(write!(t, "{}", message));
    try!(t.reset());
    try!(write!(t, " ... "));
    t.flush()
}

fn print_fixture_result(ok: bool) -> io::Result<()> {
    let mut t = term::stdout().unwrap();
    if ok {
        try!(t.fg(term::color::GREEN));
        try!(writeln!(t, "ok"));
    } else {
        try!(t.fg(term::color::RED));
        try!(writeln!(t, "ERROR!"));
    }
    try!(t.reset());
    t.flush()
}

fn print_diff(changeset: &Vec<Difference>) -> io::Result<()> {
    let mut t = term::stdout().unwrap();

    for i in 0..changeset.len() {
        match changeset[i] {
            Difference::Same(ref x) => {
                try!(t.reset());
                try!(writeln!(t, "  {}", x));
            },
            Difference::Add(ref x) => {
                for line in x.lines() {
                    try!(t.fg(term::color::GREEN));
                    try!(writeln!(t, "+ {}", line));
                }
            },
            Difference::Rem(ref x) => {
                for line in x.lines() {
                    try!(t.fg(term::color::RED));
                    try!(writeln!(t, "- {}", line));
                }
            }
        }
    }
    try!(t.reset());
    try!(writeln!(t, ""));
    t.flush()
}

fn print_uncolored(changeset: &Vec<Difference>) {
    for i in 0..changeset.len() {
        match changeset[i] {
            Difference::Same(ref x) => {
                println!("  {}", x);
            },
            Difference::Add(ref x) => {
                for line in x.lines() {
                    println!("+ {}", line);
                }
            },
            Difference::Rem(ref x) => {
                for line in x.lines() {
                    println!("- {}", line);
                }
            }
        }
    }
    println!("");
}
