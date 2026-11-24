//! Session-local HTTP exchanges. The UI holds cheap snapshots while the worker appends replies.
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Exchange {
    pub id: u64,
    pub target: Arc<str>,
    pub prompt: Arc<str>,
    pub request: Arc<str>,
    pub status: Arc<str>,
    pub response: Option<Arc<str>>,
    pub answer: Option<Arc<str>>,
}

#[derive(Clone, Default)]
pub struct Transcript(Arc<Mutex<VecDeque<Exchange>>>);
impl Transcript {
    pub fn snapshot(&self) -> Vec<Exchange> {
        self.0.lock().unwrap().iter().cloned().collect()
    }

    #[cfg(feature = "ai")]
    pub(super) fn begin(&self, target: String, prompt: String, request: String) -> Pending {
        let mut entries = self.0.lock().unwrap();
        let id = entries.back().map_or(1, |e| e.id + 1);
        if entries.len() == 100 {
            entries.pop_front();
        }
        entries.push_back(Exchange {
            id,
            target: target.into(),
            prompt: prompt.into(),
            request: request.into(),
            status: "Awaiting response".into(),
            response: None,
            answer: None,
        });
        Pending {
            transcript: self.clone(),
            id,
            finished: false,
        }
    }
}

#[cfg(feature = "ai")]
pub(super) struct Pending {
    transcript: Transcript,
    id: u64,
    finished: bool,
}
#[cfg(feature = "ai")]
impl Pending {
    pub fn finish(&mut self, status: &str, response: Option<String>, answer: Option<String>) {
        if let Some(e) = self
            .transcript
            .0
            .lock()
            .unwrap()
            .iter_mut()
            .find(|e| e.id == self.id)
        {
            e.status = status.into();
            e.response = response.map(Into::into);
            e.answer = answer.map(Into::into);
        }
        self.finished = true;
    }
}
#[cfg(feature = "ai")]
impl Drop for Pending {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("Cancelled", None, None);
        }
    }
}

#[cfg(all(test, feature = "ai"))]
mod tests {
    use super::*;
    #[test]
    fn in_flight_and_cancelled_calls_remain_in_bounded_history() {
        let transcript = Transcript::default();
        for _ in 0..100 {
            drop(transcript.begin("POST".into(), "prompt".into(), "{}".into()));
        }
        let pending = transcript.begin("POST".into(), "next".into(), "{}".into());
        let entries = transcript.snapshot();
        assert_eq!(entries.len(), 100);
        assert_eq!(entries[0].id, 2);
        assert_eq!(entries[0].status.as_ref(), "Cancelled");
        assert_eq!(entries[99].status.as_ref(), "Awaiting response");
        drop(pending);
        assert_eq!(transcript.snapshot()[99].status.as_ref(), "Cancelled");
    }
}
