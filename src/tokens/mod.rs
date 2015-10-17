/*!

All tools necessary to produce a token stream from source template.

# Summary

This module is capable of taking a Twig input template, for example, this one:

```twig
Hello
{% if world %}
    world
{% else %}
    {{ other }}
{% endif %}
```

And chopping it into tokens like these:

```text
Ok(TokenRef { value: Text("Hello\n"), line: 1 })
Ok(TokenRef { value: BlockStart, line: 2 })
Ok(TokenRef { value: Name("if"), line: 2 })
Ok(TokenRef { value: Name("world"), line: 2 })
Ok(TokenRef { value: BlockEnd, line: 2 })
Ok(TokenRef { value: Text("    world\n"), line: 3 })
Ok(TokenRef { value: BlockStart, line: 4 })
Ok(TokenRef { value: Name("else"), line: 4 })
Ok(TokenRef { value: BlockEnd, line: 4 })
Ok(TokenRef { value: Text("    "), line: 5 })
Ok(TokenRef { value: VarStart, line: 5 })
Ok(TokenRef { value: Name("other"), line: 5 })
Ok(TokenRef { value: VarEnd, line: 5 })
Ok(TokenRef { value: Text("\n"), line: 5 })
Ok(TokenRef { value: BlockStart, line: 6 })
Ok(TokenRef { value: Name("endif"), line: 6 })
Ok(TokenRef { value: BlockEnd, line: 6 })
```

Example code for this:

```rust
use twig::Environment;
use twig::tokens::Lexer;

let env = Environment::default().init_all();
let lexer = Lexer::default(&env.lexing);

# let source = r#"Hello
# {% if world %}
#     world
# {% else %}
#     {{ other }}
# {% endif %}"#;
for token in lexer.tokens(source) {
    println!("{:?}", token);
}
```

*/

mod token;
mod lexer;

pub use self::token::{ TokenRef, TokenValueRef, TokenValue };
pub use self::lexer::Lexer;
pub use self::lexer::iter::TokenIter;
pub use self::lexer::options::LexerOptions;
