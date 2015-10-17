use std::collections::HashMap;

pub trait Loader {
    fn get_source(&self, name: &str) -> Option<String>;
}

pub struct ArrayLoader {
    files: HashMap<String, String>,
}

impl Loader for ArrayLoader {
    fn get_source(&self, name: &str) -> Option<String> {
        match self.files.get(name) {
            Some(contents) => Some(contents.clone()),
            None => None,
        }
    }
}
