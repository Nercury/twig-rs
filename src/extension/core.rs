use {
    Extension,
    Environment,
    Associativity,
    Operator
};

pub struct CoreExtension;

impl Extension for CoreExtension {
    fn apply(env: &mut Environment) {
        env.push_operators(&[
            Operator::new_unary("not", 50),
            Operator::new_unary("-", 500),
            Operator::new_unary("+", 500),
        ]);

        env.push_operators(&[
            Operator::new_binary_left("or"         , 10),
            Operator::new_binary_left("and"        , 15),
            Operator::new_binary_left("b-or"       , 16),
            Operator::new_binary_left("b-xor"      , 17),
            Operator::new_binary_left("b-and"      , 18),
            Operator::new_binary_left("=="         , 20),
            Operator::new_binary_left("!="         , 20),
            Operator::new_binary_left("<"          , 20),
            Operator::new_binary_left(">"          , 20),
            Operator::new_binary_left(">="         , 20),
            Operator::new_binary_left("<="         , 20),
            Operator::new_binary_left("not in"     , 20),
            Operator::new_binary_left("in"         , 20),
            Operator::new_binary_left("matches"    , 20),
            Operator::new_binary_left("starts with", 20),
            Operator::new_binary_left("ends with"  , 20),
            Operator::new_binary_left(".."         , 25),
            Operator::new_binary_left("+"          , 30),
            Operator::new_binary_left("-"          , 30),
            Operator::new_binary_left("~"          , 40),
            Operator::new_binary_left("*"          , 60),
            Operator::new_binary_left("/"          , 60),
            Operator::new_binary_left("//"         , 60),
            Operator::new_binary_left("%"          , 60),
            Operator::new_binary_left("is"         , 100),
            Operator::new_binary_left("is not"     , 100),
            Operator::new_binary_right("**"         , 200),
        ]);
    }
}
