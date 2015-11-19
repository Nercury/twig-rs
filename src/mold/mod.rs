use std::collections::{ HashMap, VecDeque };
use little::*;
use value::Value;

pub struct Staging<'c, V: LittleValue> {
    next_constant: Constant,
    unique_constants: HashMap<Fingerprint, Constant>,
    constant_values: HashMap<Constant, V>,
    pub locals: VecDeque<Basket<'c, Binding>>,
    pub template: Template<V>,
}

impl<'c, V: LittleValue> Staging<'c, V> {
    pub fn new<'r>() -> Staging<'r, V> {
        let mut st = Staging {
            next_constant: Constant(0),
            unique_constants: HashMap::new(),
            constant_values: HashMap::new(),
            locals: VecDeque::new(),
            template: Template::empty(),
        };

        st.locals.push_front(Basket::new(Binding(0), |Binding(p)| Binding(p + 1)));

        st
    }

    pub fn include_const(&mut self, const_value: V) -> Mem {
        // next constant to insert.
        let mut next = self.next_constant;

        let constant = match const_value.identify_value() {
            Some(fingerprint) => { // constant is identifiable
                let mut added = false;
                // we can ensure that identifiable constant is kept only once
                let identifier = *self.unique_constants.entry(fingerprint).or_insert_with(|| {
                    // when we insert it, we increment identifier.
                    let identifier = next;
                    next = match next {
                        Constant(v) => Constant(v + 1),
                    };
                    added = true;
                    identifier
                });
                // and add it to constant list only once.
                if added {
                    self.constant_values.insert(identifier, const_value);
                }
                identifier
            },
            None => { // constant can not be identified
                // always add value to constant list for every constant.
                let identifier = next;
                next = match next {
                    Constant(v) => Constant(v + 1),
                };
                self.constant_values.insert(identifier, const_value);
                identifier
            },
        };

        // update next constant.
        self.next_constant = next;

        Mem::Const(constant)
    }

    pub fn use_name(&mut self, name: &'c str) -> Option<Mem> {
        for basket in &self.locals {
            if let Some(ref binding) = basket.get(name) {
                return Some(Mem::Binding(binding.clone()));
            }
        }
        None
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
