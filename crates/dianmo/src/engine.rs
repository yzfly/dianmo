//! The engine behind the controller: librime when it loads, otherwise [`BasicEngine`].

use dianmo_core::{Candidate, Engine, Schema, Snapshot};

use crate::basic::BasicEngine;

pub enum AnyEngine {
    #[cfg(all(windows, feature = "rime"))]
    Rime(dianmo_rime::RimeEngine),
    Basic(BasicEngine),
}

macro_rules! each {
    ($self:ident, $e:ident => $body:expr) => {
        match $self {
            #[cfg(all(windows, feature = "rime"))]
            AnyEngine::Rime($e) => $body,
            AnyEngine::Basic($e) => $body,
        }
    };
}

impl Engine for AnyEngine {
    fn schema(&self) -> Schema {
        each!(self, e => e.schema())
    }
    fn set_schema(&mut self, schema: Schema) {
        each!(self, e => e.set_schema(schema))
    }
    fn input(&mut self, c: char) -> Snapshot {
        each!(self, e => e.input(c))
    }
    fn backspace(&mut self) -> Snapshot {
        each!(self, e => e.backspace())
    }
    fn select(&mut self, index: usize) -> Snapshot {
        each!(self, e => e.select(index))
    }
    fn commit_raw(&mut self) -> Snapshot {
        each!(self, e => e.commit_raw())
    }
    fn clear(&mut self) -> Snapshot {
        each!(self, e => e.clear())
    }
    fn candidates(&mut self, start: usize, count: usize) -> Vec<Candidate> {
        each!(self, e => e.candidates(start, count))
    }
    fn t9_spellings(&mut self) -> Vec<String> {
        each!(self, e => e.t9_spellings())
    }
    fn pick_t9_spelling(&mut self, spelling: &str) -> Snapshot {
        each!(self, e => e.pick_t9_spelling(spelling))
    }
    fn snapshot(&mut self) -> Snapshot {
        each!(self, e => e.snapshot())
    }
}
/// librime is started on a background thread and moved to the UI thread (it has no thread
/// affinity; we only ever use it from one thread at a time).
#[cfg(all(windows, feature = "rime"))]
#[allow(dead_code)]
fn _rime_engine_is_send() {
    fn is_send<T: Send>() {}
    is_send::<dianmo_rime::RimeEngine>();
}
