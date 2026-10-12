#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Requested,
    Locked,
    Unlocking,
    Finished,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finish {
    DeniedDestroy,
    UnlockedDestroy,
    None,
}

pub struct Lifecycle {
    phase: Phase,
    was_locked: bool,
    ready: bool,
}
impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            phase: Phase::Requested,
            was_locked: false,
            ready: false,
        }
    }
}
impl Lifecycle {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn locked(&mut self) -> bool {
        if self.phase != Phase::Requested {
            return false;
        }
        self.phase = Phase::Locked;
        self.was_locked = true;
        self.ready = true;
        true
    }
    pub fn ready(&self) -> bool {
        self.ready && self.phase == Phase::Locked
    }
    pub fn authenticated_unlock(&mut self) -> bool {
        if self.phase != Phase::Locked {
            return false;
        }
        self.phase = Phase::Unlocking;
        self.ready = false;
        true
    }
    pub fn finished(&mut self) -> Finish {
        if self.phase == Phase::Finished {
            return Finish::None;
        }
        let finish = if self.was_locked {
            Finish::UnlockedDestroy
        } else {
            Finish::DeniedDestroy
        };
        self.phase = Phase::Finished;
        self.ready = false;
        finish
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_only_follows_locked_and_finish_uses_correct_destructor() {
        let mut denied = Lifecycle::default();
        assert!(!denied.ready());
        assert!(!denied.authenticated_unlock());
        assert_eq!(denied.finished(), Finish::DeniedDestroy);
        assert!(!denied.locked());
        assert_eq!(denied.finished(), Finish::None);
        let mut locked = Lifecycle::default();
        assert!(locked.locked());
        assert!(locked.ready());
        assert_eq!(locked.finished(), Finish::UnlockedDestroy);
        assert!(!locked.ready());
        let mut authenticated = Lifecycle::default();
        authenticated.locked();
        assert!(authenticated.authenticated_unlock());
        assert!(!authenticated.authenticated_unlock());
    }
}
