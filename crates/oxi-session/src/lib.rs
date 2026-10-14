#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Choosing,
    Authenticating,
    Authenticated,
    Starting,
    Handoff,
}
pub struct Handoff {
    phase: Phase,
}
impl Default for Handoff {
    fn default() -> Self {
        Self {
            phase: Phase::Choosing,
        }
    }
}
impl Handoff {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn authenticating(&mut self) {
        self.phase = Phase::Authenticating;
    }
    pub fn authenticated(&mut self) -> bool {
        if self.phase != Phase::Authenticating {
            return false;
        }
        self.phase = Phase::Authenticated;
        true
    }
    pub fn start(&mut self) -> bool {
        if self.phase != Phase::Authenticated {
            return false;
        }
        self.phase = Phase::Starting;
        true
    }
    pub fn acknowledge(&mut self, accepted: bool) -> bool {
        if self.phase != Phase::Starting {
            return false;
        }
        self.phase = if accepted {
            Phase::Handoff
        } else {
            Phase::Authenticated
        };
        accepted
    }
    pub fn cancel(&mut self) {
        if self.phase != Phase::Handoff {
            self.phase = Phase::Choosing;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_acknowledged_authenticated_start_allows_kiosk_exit() {
        let mut handoff = Handoff::default();
        assert!(!handoff.start());
        assert!(!handoff.acknowledge(true));
        handoff.authenticating();
        assert!(handoff.authenticated());
        assert!(handoff.start());
        assert!(!handoff.acknowledge(false));
        assert_eq!(handoff.phase(), Phase::Authenticated);
        assert!(handoff.start());
        assert!(handoff.acknowledge(true));
        handoff.cancel();
        assert_eq!(handoff.phase(), Phase::Handoff);
    }
}

pub mod catalog;
#[cfg(test)]
mod flow_tests;
pub mod frontend;
pub mod ipc;
pub mod kiosk;
pub mod model;
mod users;
