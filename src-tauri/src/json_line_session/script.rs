use std::cell::RefCell;
use std::collections::VecDeque;
use std::process::ExitStatus;
use std::rc::Rc;

use serde_json::Value;

use super::{Failure, FailureKind, Peer, Session, Transport};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Exchange {
    Sent(Value),
    Received(Value),
}

#[derive(Clone)]
pub(crate) struct Script(Rc<RefCell<Exchanges>>);

struct Exchanges {
    replies: VecDeque<Value>,
    log: Vec<Exchange>,
}

impl Script {
    pub(crate) fn new(replies: impl IntoIterator<Item = Value>) -> Self {
        Self(Rc::new(RefCell::new(Exchanges {
            replies: replies.into_iter().collect(),
            log: Vec::new(),
        })))
    }

    pub(crate) fn session(&self, peer: Peer) -> Session {
        Session::over(peer, self.clone())
    }

    pub(crate) fn sent(&self) -> Vec<Value> {
        self.0
            .borrow()
            .log
            .iter()
            .filter_map(|exchange| match exchange {
                Exchange::Sent(message) => Some(message.clone()),
                Exchange::Received(_) => None,
            })
            .collect()
    }

    pub(crate) fn log(&self) -> Vec<Exchange> {
        self.0.borrow().log.clone()
    }
}

impl Transport for Script {
    fn send(&mut self, message: &Value) -> Result<(), Failure> {
        self.0
            .borrow_mut()
            .log
            .push(Exchange::Sent(message.clone()));
        Ok(())
    }

    fn receive(&mut self) -> Result<Value, Failure> {
        let mut exchanges = self.0.borrow_mut();
        let message = exchanges
            .replies
            .pop_front()
            .ok_or_else(|| Failure::new(FailureKind::Other, "the script has no more messages"))?;
        exchanges.log.push(Exchange::Received(message.clone()));
        Ok(message)
    }

    fn finish(&mut self) -> Option<ExitStatus> {
        None
    }
}
