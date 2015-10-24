use environment::Environment;
use extension::Extension;
use operator::Operator;
use nodes::TokenParser;

pub mod token_parsers;
pub mod error;

pub struct CoreExtension;

impl Extension for CoreExtension {
    fn apply(env: &mut Environment) {
        env.push_operators(vec![
            Operator::new_unary("not", 50, |_| unimplemented!()),
            Operator::new_unary("-", 500, |_| unimplemented!()),
            Operator::new_unary("+", 500, |_| unimplemented!()),

            Operator::new_binary_left("or"         , 10, |_, _| unimplemented!()),
            Operator::new_binary_left("and"        , 15, |_, _| unimplemented!()),
            Operator::new_binary_left("b-or"       , 16, |_, _| unimplemented!()),
            Operator::new_binary_left("b-xor"      , 17, |_, _| unimplemented!()),
            Operator::new_binary_left("b-and"      , 18, |_, _| unimplemented!()),
            Operator::new_binary_left("=="         , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("!="         , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("<"          , 20, |_, _| unimplemented!()),
            Operator::new_binary_left(">"          , 20, |_, _| unimplemented!()),
            Operator::new_binary_left(">="         , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("<="         , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("not in"     , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("in"         , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("matches"    , 20, |_, _| unimplemented!()),
            Operator::new_binary_left("starts with", 20, |_, _| unimplemented!()),
            Operator::new_binary_left("ends with"  , 20, |_, _| unimplemented!()),
            Operator::new_binary_left(".."         , 25, |_, _| unimplemented!()),
            Operator::new_binary_left("+"          , 30, |_, _| unimplemented!()),
            Operator::new_binary_left("-"          , 30, |_, _| unimplemented!()),
            Operator::new_binary_left("~"          , 40, |_, _| unimplemented!()),
            Operator::new_binary_left("*"          , 60, |_, _| unimplemented!()),
            Operator::new_binary_left("/"          , 60, |_, _| unimplemented!()),
            Operator::new_binary_left("//"         , 60, |_, _| unimplemented!()),
            Operator::new_binary_left("%"          , 60, |_, _| unimplemented!()),
            Operator::new_binary_left("is"         , 100, |_, _| unimplemented!()),
            Operator::new_binary_left("is not"     , 100, |_, _| unimplemented!()),

            Operator::new_binary_right("**"         , 200, |_, _| unimplemented!()),
        ]);

        env.push_token_parsers(vec![
            TokenParser::new(token_parsers::For::new()),
            TokenParser::new(token_parsers::If::new()),
            TokenParser::new(token_parsers::Extends::new()),
            TokenParser::new(token_parsers::Include::new()),
            TokenParser::new(token_parsers::Block::new()),
            TokenParser::new(token_parsers::Use::new()),
            TokenParser::new(token_parsers::Filter::new()),
            TokenParser::new(token_parsers::Macro::new()),
            TokenParser::new(token_parsers::Import::new()),
            TokenParser::new(token_parsers::From::new()),
            TokenParser::new(token_parsers::Set::new()),
            TokenParser::new(token_parsers::Spaceless::new()),
            TokenParser::new(token_parsers::Flush::new()),
            TokenParser::new(token_parsers::Do::new()),
            TokenParser::new(token_parsers::Embed::new()),
        ]);
    }
}
