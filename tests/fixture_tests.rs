use std::io::{self, Read, BufReader, BufRead};
use std::env;
use std::fs::{self, DirEntry, File};
use std::path::Path;

#[test]
fn fixtures() {
    visit_fixtures(&env::current_dir().unwrap().join("tests").join("fixtures"), &|entry| {
        println!("fixture {:?}", entry.path());

        let f = File::open(entry.path()).ok().expect("error opening fixture file");
        let fixture = match Fixture::new(f) {
            Err(e) => panic!("invalid test {:?}", e),
            Ok(f) => f,
        };

        println!("{:#?}", fixture);
    }).unwrap();
}

fn visit_fixtures(dir: &Path, cb: &Fn(&DirEntry)) -> io::Result<()> {
    if try!(fs::metadata(dir)).is_dir() {
        for entry in try!(fs::read_dir(dir)) {
            let entry = try!(entry);
            if try!(fs::metadata(entry.path())).is_dir() {
                try!(visit_fixtures(&entry.path(), cb));
            } else {
                if let Some(Some("test")) = entry.path().extension().map(|v| v.to_str()) {
                    cb(&entry);
                }
            }
        }
    }
    Ok(())
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
