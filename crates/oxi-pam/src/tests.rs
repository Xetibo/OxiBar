use super::{Event, Worker};
use oxi_auth_ui::{Attempt, PromptKind, Secret};
use std::{
    ffi::CString,
    fs, io,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Fixture {
    directory: PathBuf,
}
impl Fixture {
    fn new(policy: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "oxi-pam-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("fixture.c"), MODULE).unwrap();
        let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
        let output = Command::new(compiler)
            .args(["-shared", "-fPIC", "-O2"])
            .arg(directory.join("fixture.c"))
            .arg("-o")
            .arg(directory.join("fixture.so"))
            .arg("-lpam")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(
            directory.join("oxilock"),
            format!(
                "auth required {} {policy}\naccount required {} {policy}\n",
                directory.join("fixture.so").display(),
                directory.join("fixture.so").display()
            ),
        )
        .unwrap();
        Self { directory }
    }
    fn worker(&self, attempt: Attempt) -> Worker {
        let mut command = Command::new("/proc/self/exe");
        command
            .args([
                "--ignored",
                "--exact",
                "tests::worker_process",
                "--test-threads=1",
            ])
            .env("OXI_PAM_TEST_CONFIGURATION", &self.directory);
        Worker::spawn(command, attempt).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
#[ignore = "private child entry point, launched by PAM fixture tests"]
fn worker_process() {
    super::harden_process().unwrap();
    let configuration = CString::new(std::env::var("OXI_PAM_TEST_CONFIGURATION").unwrap()).unwrap();
    // The test-only subprocess receives the same private descriptor as production.
    let mut stream = super::private_stream(3).unwrap();
    super::worker(&mut stream, Some(&configuration)).unwrap();
}

fn finish(worker: &mut Worker, password: &str) -> (bool, Vec<PromptKind>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut prompts = Vec::new();
    loop {
        assert!(
            Instant::now() < deadline,
            "private PAM conversation timed out"
        );
        match worker.poll().unwrap() {
            Some(Event::Prompt(_, prompt)) => {
                prompts.push(prompt.kind);
                if matches!(prompt.kind, PromptKind::Secret | PromptKind::Visible) {
                    let mut response = Secret::default();
                    response.push(match prompt.text.as_str() {
                        "Challenge name" => "fixture",
                        "Password" => password,
                        "One-time code" => "123456",
                        _ => panic!("unexpected fixture prompt"),
                    });
                    worker.respond(response).unwrap();
                }
            }
            Some(Event::Finished(_, success)) => return (success, prompts),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

#[test]
fn real_pam_private_module_handles_mfa_failure_account_policy_and_identity() {
    let fixture = Fixture::new("");
    let (success, prompts) = finish(&mut fixture.worker(Attempt(1)), "fixture-password");
    assert!(success);
    assert_eq!(
        prompts,
        [
            PromptKind::Info,
            PromptKind::Error,
            PromptKind::Visible,
            PromptKind::Secret,
            PromptKind::Secret
        ]
    );
    assert!(!finish(&mut fixture.worker(Attempt(2)), "wrong-password").0);
    assert!(
        !finish(
            &mut Fixture::new("deny-account").worker(Attempt(3)),
            "fixture-password"
        )
        .0
    );
    assert!(
        !finish(
            &mut Fixture::new("remap-user").worker(Attempt(4)),
            "fixture-password"
        )
        .0
    );
}

#[test]
fn cancelling_a_blocked_conversation_reaps_its_private_worker() {
    let fixture = Fixture::new("");
    let mut worker = fixture.worker(Attempt(6));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline);
        if matches!(worker.poll().unwrap(), Some(Event::Prompt(_, prompt)) if prompt.kind == PromptKind::Visible)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let pid = worker.child.id() as i32;
    drop(worker);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    assert!(finish(&mut fixture.worker(Attempt(7)), "fixture-password").0);
}

const MODULE: &str = r#"
#include <security/pam_modules.h>
#include <security/pam_appl.h>
#include <string.h>
#include <stdlib.h>
int pam_sm_authenticate(pam_handle_t *pam, int flags, int argc, const char **argv) {
    const struct pam_conv *conv = NULL;
    if (pam_get_item(pam, PAM_CONV, (const void **)&conv) != PAM_SUCCESS) return PAM_SYSTEM_ERR;
    struct pam_message messages[] = {
        { PAM_TEXT_INFO, "Private authentication fixture" },
        { PAM_ERROR_MSG, "Fixture challenge" },
        { PAM_PROMPT_ECHO_ON, "Challenge name" },
        { PAM_PROMPT_ECHO_OFF, "Password" },
        { PAM_PROMPT_ECHO_OFF, "One-time code" }
    };
    const struct pam_message *pointers[5];
    for (int i = 0; i < 5; ++i) pointers[i] = &messages[i];
    struct pam_response *responses = NULL;
    if (conv->conv(5, pointers, &responses, conv->appdata_ptr) != PAM_SUCCESS) return PAM_CONV_ERR;
    int ok = responses[2].resp && responses[3].resp && responses[4].resp
        && strcmp(responses[2].resp, "fixture") == 0
        && strcmp(responses[3].resp, "fixture-password") == 0
        && strcmp(responses[4].resp, "123456") == 0;
    for (int i = 0; i < 5; ++i) { if (responses[i].resp) { explicit_bzero(responses[i].resp, strlen(responses[i].resp)); free(responses[i].resp); } }
    free(responses);
    if (argc && strcmp(argv[0], "remap-user") == 0) pam_set_item(pam, PAM_USER, "fixture-other-user");
    return ok ? PAM_SUCCESS : PAM_AUTH_ERR;
}
int pam_sm_acct_mgmt(pam_handle_t *pam, int flags, int argc, const char **argv) {
    return argc && strcmp(argv[0], "deny-account") == 0 ? PAM_ACCT_EXPIRED : PAM_SUCCESS;
}
int pam_sm_setcred(pam_handle_t *pam, int flags, int argc, const char **argv) { return PAM_SUCCESS; }
"#;
