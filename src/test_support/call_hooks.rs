use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type CallHook = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub struct CallHooks {
    hooks: Mutex<HashMap<&'static str, CallHook>>,
}

impl CallHooks {
    pub fn add(&self, operation: &'static str, hook: impl Fn() + Send + Sync + 'static) {
        self.hooks.lock().unwrap().insert(operation, Arc::new(hook));
    }

    pub fn run(&self, operation: &'static str) {
        let hook = self.hooks.lock().unwrap().get(operation).cloned();
        if let Some(hook) = hook {
            hook();
        }
    }
}
