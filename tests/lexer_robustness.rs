//! The lexer must never panic or loop forever, whatever the input bytes and options. Random templates built from
//! the bytes that matter to the lexer go through lexers with default, custom and degenerate options.

use twig::Source;
use twig::deprecation::Deprecation;
use twig::lexer::{DEFAULT_OPERATORS, Lexer, LexerOptions, TokenType};

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const PIECES: &[&[u8]] = &[
    b"{{",
    b"}}",
    b"{%",
    b"%}",
    b"{#",
    b"#}",
    b"{##",
    b"##}",
    b"-",
    b"~",
    b" ",
    b"\n",
    b"\r",
    b"\r\n",
    b"\t",
    b"\x0c",
    b"\x0b",
    b"\0",
    b"'",
    b"\"",
    b"\\",
    b"#{",
    b"}",
    b"{",
    b"(",
    b")",
    b"[",
    b"]",
    b"|",
    b".",
    b"?",
    b":",
    b",",
    b"#",
    b"0",
    b"1_2",
    b"1e",
    b"1.5",
    b"_",
    b"a",
    b"x",
    b"not",
    b"in",
    b"is not",
    b"and",
    b"verbatim",
    b"endverbatim",
    b"line 3",
    b"$",
    b"\x85",
    b"\xc3\xa9",
    b"\xff",
    b"=>",
    b"...",
    b"??",
];

fn random_template(rng: &mut Rng) -> Vec<u8> {
    let mut template = Vec::new();
    for _ in 0..rng.below(40) {
        template.extend_from_slice(PIECES[rng.below(PIECES.len())]);
    }
    template
}

fn lexers() -> Vec<Lexer> {
    let empty_variable = LexerOptions {
        tag_variable: [Vec::new(), Vec::new()],
        tag_comment: [b"{#".to_vec(), Vec::new()],
        whitespace_trim: Vec::new(),
        ..LexerOptions::default()
    };
    let empty_interpolation = LexerOptions {
        interpolation: [Vec::new(), Vec::new()],
        tag_block: [b"{%".to_vec(), Vec::new()],
        whitespace_line_chars: Vec::new(),
        ..LexerOptions::default()
    };
    let half_empty_interpolation = LexerOptions {
        interpolation: [Vec::new(), b"}".to_vec()],
        ..LexerOptions::default()
    };
    let custom = LexerOptions {
        tag_comment: [b"[#".to_vec(), b"#]".to_vec()],
        tag_block: [b"{ %".to_vec(), b"% }".to_vec()],
        tag_variable: [b"{{".to_vec(), b"}}".to_vec()],
        whitespace_trim: b"#".to_vec(),
        whitespace_line_trim: b"\x85".to_vec(),
        interpolation: [b"${".to_vec(), b"}}".to_vec()],
        ..LexerOptions::default()
    };
    vec![
        Lexer::default(),
        Lexer::new(empty_variable, DEFAULT_OPERATORS),
        Lexer::new(empty_interpolation, DEFAULT_OPERATORS),
        Lexer::new(half_empty_interpolation, DEFAULT_OPERATORS),
        Lexer::new(custom, ["", " ", "x y", "\u{e9}", "a"]),
        Lexer::new(LexerOptions::default(), Vec::<&str>::new()),
    ]
}

#[test]
fn random_templates_never_panic() {
    let lexers = lexers();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let template = random_template(&mut rng);
        let source = Source::new(template.clone(), "index");
        for lexer in &lexers {
            let mut deprecations: Vec<Deprecation> = Vec::new();
            match lexer.tokenize(&source, &mut deprecations) {
                Ok(stream) => {
                    let tokens = stream.tokens();
                    assert_eq!(
                        tokens.last().map(|t| t.ty()),
                        Some(TokenType::Eof),
                        "{template:?}"
                    );
                    assert!(
                        tokens
                            .iter()
                            .all(|t| t.offset().is_some_and(|o| o <= template.len()))
                    );
                    assert!(
                        tokens.windows(2).all(|w| w[0].offset() <= w[1].offset()),
                        "{template:?}"
                    );
                }
                Err(error) => assert!(!error.raw_message().is_empty(), "{template:?}"),
            }
        }
    }
}
