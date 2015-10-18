mod template;

pub enum Error {
    /// Error lexing or parsing the template source file.
    Template(template::ErrorAt),
    /// Error reading files, compiling templates or writing cache.
    Engine,
    /// Error executing template.
    Runtime,
}

pub use self::template::{ Result, ErrorAt, TemplateError, Received, CustomErrorAt };
