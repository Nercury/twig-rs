use Extension;
use environment::{ Environment, Associativity };

pub struct CoreExtension;

impl Extension for CoreExtension {
    fn apply(env: &mut Environment) {
        env.push_unary_operators(&[
            ("not", 50),
            ("-", 500),
            ("+", 500),
        ]);

        env.push_binary_operators(&[
            ("or"         , 10, Associativity::Left),
            ("and"        , 15, Associativity::Left),
            ("b-or"       , 16, Associativity::Left),
            ("b-xor"      , 17, Associativity::Left),
            ("b-and"      , 18, Associativity::Left),
            ("=="         , 20, Associativity::Left),
            ("!="         , 20, Associativity::Left),
            ("<"          , 20, Associativity::Left),
            (">"          , 20, Associativity::Left),
            (">="         , 20, Associativity::Left),
            ("<="         , 20, Associativity::Left),
            ("not in"     , 20, Associativity::Left),
            ("in"         , 20, Associativity::Left),
            ("matches"    , 20, Associativity::Left),
            ("starts with", 20, Associativity::Left),
            ("ends with"  , 20, Associativity::Left),
            (".."         , 25, Associativity::Left),
            ("+"          , 30, Associativity::Left),
            ("-"          , 30, Associativity::Left),
            ("~"          , 40, Associativity::Left),
            ("*"          , 60, Associativity::Left),
            ("/"          , 60, Associativity::Left),
            ("//"         , 60, Associativity::Left),
            ("%"          , 60, Associativity::Left),
            ("is"         , 100, Associativity::Left),
            ("is not"     , 100, Associativity::Left),
            ("**"         , 200, Associativity::Right),
        ]);
    }
}
