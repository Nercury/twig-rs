use std::collections::HashMap;
use error::{ Result, EngineError };

pub trait Loader {
    fn get_source(&self, name: &str) -> Result<String>;
}

#[derive(Debug)]
pub struct ArrayLoader {
    files: HashMap<String, String>,
}

impl ArrayLoader {
    pub fn new(sources: HashMap<String, String>) -> ArrayLoader {
        ArrayLoader {
            files: sources,
        }
    }
}

impl Loader for ArrayLoader {
    fn get_source(&self, name: &str) -> Result<String> {
        match self.files.get(name) {
            Some(contents) => Ok(contents.clone()),
            None => Err(EngineError::TemplateNotFound {
                name: name.into(),
                search_paths: Vec::new()
            }.into()),
        }
    }
}
