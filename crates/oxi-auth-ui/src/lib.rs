use zeroize::{Zeroize, Zeroizing};

mod response;
pub mod view;
pub mod visuals;

pub const MAX_RESPONSE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Secret,
    Visible,
    Info,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub text: String,
}

pub struct Secret(Zeroizing<String>);

impl Default for Secret {
    fn default() -> Self {
        Self(Zeroizing::new(String::with_capacity(MAX_RESPONSE_BYTES)))
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}
impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn push(&mut self, value: &str) {
        for character in value.chars().filter(|c| !c.is_control()) {
            if self.0.len() + character.len_utf8() <= MAX_RESPONSE_BYTES {
                self.0.push(character);
            }
        }
    }
    pub fn clear(&mut self) {
        self.0.zeroize();
    }
    pub fn pop(&mut self) {
        if let Some((index, _)) = self.0.char_indices().next_back() {
            let retained = Zeroizing::new(self.0[..index].to_owned());
            self.clear();
            self.0.push_str(&retained);
        }
    }
    pub fn masked(&self) -> String {
        "•".repeat(self.0.chars().count())
    }
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Waiting,
    Prompt,
    Authenticated,
}

pub struct Interaction {
    epoch: u64,
    pub attempt: Option<Attempt>,
    pub phase: Phase,
    pub prompt: Option<Prompt>,
    pub input: Secret,
    pub notice: String,
}
impl Default for Interaction {
    fn default() -> Self {
        Self {
            epoch: 0,
            attempt: None,
            phase: Phase::Idle,
            prompt: None,
            input: Secret::default(),
            notice: String::new(),
        }
    }
}
impl Interaction {
    pub fn begin(&mut self) -> Option<Attempt> {
        if self.attempt.is_some() {
            return None;
        }
        self.epoch = self
            .epoch
            .checked_add(1)
            .expect("authentication attempt counter exhausted");
        let attempt = Attempt(self.epoch);
        self.attempt = Some(attempt);
        self.phase = Phase::Waiting;
        self.prompt = None;
        self.input.clear();
        self.notice.clear();
        Some(attempt)
    }
    pub fn receive(&mut self, attempt: Attempt, prompt: Prompt) -> bool {
        if self.attempt != Some(attempt) || self.phase == Phase::Authenticated {
            return false;
        }
        self.input.clear();
        match prompt.kind {
            PromptKind::Secret | PromptKind::Visible => {
                self.prompt = Some(prompt);
                self.phase = Phase::Prompt;
            }
            PromptKind::Info | PromptKind::Error => {
                self.notice = prompt.text;
            }
        }
        true
    }
    pub fn respond(&mut self) -> Option<Secret> {
        if self.phase != Phase::Prompt {
            return None;
        }
        self.phase = Phase::Waiting;
        Some(self.input.take())
    }
    pub fn complete(&mut self, attempt: Attempt, success: bool) -> bool {
        if self.attempt != Some(attempt) || self.phase == Phase::Authenticated {
            return false;
        }
        self.input.clear();
        self.prompt = None;
        if success {
            self.phase = Phase::Authenticated;
        } else {
            self.attempt = None;
            self.phase = Phase::Idle;
            self.notice = "Authentication failed. Try again.".into();
        }
        success
    }
    pub fn cancel(&mut self) -> Option<Attempt> {
        let attempt = self.attempt.take();
        self.phase = Phase::Idle;
        self.prompt = None;
        self.input.clear();
        self.notice.clear();
        attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_transaction_handles_multiple_prompts_and_rejects_cancelled_results() {
        let mut interaction = Interaction::default();
        let first = interaction.begin().unwrap();
        assert!(interaction.begin().is_none());
        interaction.receive(
            first,
            Prompt {
                kind: PromptKind::Secret,
                text: "Password".into(),
            },
        );
        interaction.input.push("private-password");
        assert_eq!(interaction.input.masked(), "•".repeat(16));
        let response = interaction.respond().unwrap();
        assert_eq!(format!("{response:?}"), "Secret([REDACTED])");
        interaction.receive(
            first,
            Prompt {
                kind: PromptKind::Secret,
                text: "One-time code".into(),
            },
        );
        interaction.cancel();
        let second = interaction.begin().unwrap();
        assert!(!interaction.complete(first, true));
        assert!(!interaction.receive(
            first,
            Prompt {
                kind: PromptKind::Visible,
                text: "Stale".into()
            }
        ));
        assert!(interaction.complete(second, true));
        assert!(!interaction.complete(second, true));
    }
    #[test]
    fn responses_are_bounded_unicode_and_clear_on_edit_or_failure() {
        let mut secret = Secret::default();
        secret.push("aé\n");
        secret.pop();
        assert_eq!(secret.expose(), "a");
        secret.push(&"x".repeat(MAX_RESPONSE_BYTES));
        assert_eq!(secret.expose().len(), MAX_RESPONSE_BYTES);
        secret.clear();
        assert!(secret.is_empty());
        let mut interaction = Interaction::default();
        let attempt = interaction.begin().unwrap();
        interaction.input.push("secret");
        assert!(!interaction.complete(attempt, false));
        assert!(interaction.input.is_empty());
        assert!(interaction.begin().is_some());
    }
}
