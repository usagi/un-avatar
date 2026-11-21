//! Native close is a request, not permission to discard a WebView's drafts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseIntent { Window, Quit }

#[derive(Default)]
pub struct CloseGate {
    sequence: u64,
    pending: Option<(u64, CloseIntent)>,
    closing: bool,
}

impl CloseGate {
    pub fn request(&mut self, intent: CloseIntent) -> Option<u64> {
        // A fresh explicit request can replace a lost/unanswered review, but
        // cannot interrupt a close that has already been approved.
        if self.closing { return None; }
        self.sequence = self.sequence.checked_add(1)?;
        self.pending = Some((self.sequence, intent));
        Some(self.sequence)
    }
    pub fn resolve(&mut self, id: u64, allow: bool) -> Result<Option<CloseIntent>, &'static str> {
        let Some((pending_id, intent)) = self.pending else { return Err("No pending close request"); };
        if pending_id != id { return Err("Stale close response"); }
        self.pending = None;
        self.closing = allow;
        Ok(allow.then_some(intent))
    }
    pub fn completion_failed(&mut self) { self.closing = false; }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refusal_and_stale_responses_cannot_stop_or_replace_intent() {
        let mut gate = CloseGate::default();
        let id = gate.request(CloseIntent::Window).unwrap();
        assert_eq!(gate.resolve(id, false), Ok(None));
        assert!(gate.resolve(id, true).is_err());
        let next = gate.request(CloseIntent::Quit).unwrap();
        assert!(gate.resolve(id, true).is_err());
        assert_eq!(gate.resolve(next, true), Ok(Some(CloseIntent::Quit)));
        assert!(gate.resolve(next, true).is_err());
        assert_eq!(gate.request(CloseIntent::Window), None);
    }
    #[test]
    fn window_approval_does_not_become_quit() {
        let mut gate=CloseGate::default();
        let id=gate.request(CloseIntent::Window).unwrap();
        assert_eq!(gate.resolve(id,true),Ok(Some(CloseIntent::Window)));
    }
    #[test]
    fn unanswered_review_can_be_replaced_without_accepting_old_approval() {
        let mut gate=CloseGate::default();
        let old=gate.request(CloseIntent::Quit).unwrap();
        let new=gate.request(CloseIntent::Window).unwrap();
        assert!(gate.resolve(old,true).is_err());
        assert!(gate.resolve(old,false).is_err());
        assert_eq!(gate.resolve(new,true),Ok(Some(CloseIntent::Window)));
        gate.completion_failed();
        assert!(gate.request(CloseIntent::Quit).is_some());
    }
}
