use std::collections::{ HashMap, VecDeque };
use std::borrow::{ Borrow };
use little::*;
use value::Value;

pub struct Staging<'c, V> {
    pub globals: Basket<'c, Parameter>,
    pub locals: VecDeque<Basket<'c, Binding>>,
    pub template: Template<V>,
}

impl<'c, V> Staging<'c, V> {
    pub fn new<'r>() -> Staging<'r, V> {
        let mut st = Staging {
            globals: Basket::new(Parameter(0), |Parameter(p)| Parameter(p + 1)),
            locals: VecDeque::new(),
            template: Template::empty(),
        };

        st.locals.push_front(Basket::new(Binding(0), |Binding(p)| Binding(p + 1)));

        st
    }

    pub fn use_name(&mut self, name: &'c str) -> Mem {
        for basket in &self.locals {
            if let Some(ref binding) = basket.get(name) {
                return Mem::Binding(binding.clone());
            }
        }
        Mem::Param(self.globals.assign_space(name.borrow()))
    }

    pub fn instr(&mut self, instruction: Instruction) {
        trace!("instr {:?}", &instruction);
        self.template.push_instruction(instruction);
    }
}

impl<'a> Into<Template<Value>> for Staging<'a, Value> {
    fn into(self) -> Template<Value> {
        self.template
    }
}

pub struct Basket<'c, T>  {
    pub map: HashMap<&'c str, T>,
    next: Box<Fn(T) -> T>,
    current: T,
}

impl<'c, T> Basket<'c, T> where T: Eq + Clone {

    pub fn new<'r, N: Fn(T) -> T + 'static>(initial: T, next: N) -> Basket<'r, T> {
        Basket {
            map: HashMap::new(),
            next: Box::new(next),
            current: initial,
        }
    }

    pub fn assign_space(&mut self, name: &'c str) -> T {
        let len = self.map.len();
        let current = self.current.clone();
        let result = self.map.entry(&name).or_insert(current.clone()).clone();
        if self.map.len() > len {
            self.current = (self.next)(current);
        }
        result
    }

    pub fn get(&self, name: &str) -> Option<T> {
        self.map.get(name).cloned()
    }
}
