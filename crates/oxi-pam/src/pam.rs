use crate::codec;
use oxi_auth_ui::{Attempt, MAX_RESPONSE_BYTES, Prompt, PromptKind};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io,
    os::unix::net::UnixStream,
    ptr,
};

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_USER: c_int = 2;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 1;
const MAX_PAM_MESSAGES: c_int = 32;

#[repr(C)]
struct PamMessage {
    style: c_int,
    text: *const c_char,
}
#[repr(C)]
struct PamResponse {
    response: *mut c_char,
    code: c_int,
}
#[repr(C)]
struct PamConversation {
    callback: unsafe extern "C" fn(
        c_int,
        *mut *const PamMessage,
        *mut *mut PamResponse,
        *mut c_void,
    ) -> c_int,
    data: *mut c_void,
}
#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conversation: *const PamConversation,
        handle: *mut *mut c_void,
    ) -> c_int;
    #[cfg(test)]
    fn pam_start_confdir(
        service: *const c_char,
        user: *const c_char,
        conversation: *const PamConversation,
        directory: *const c_char,
        handle: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_get_item(handle: *const c_void, item: c_int, value: *mut *const c_void) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
}

pub fn current_user() -> io::Result<String> {
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = ptr::null_mut();
    let mut bytes = vec![0u8; 65536];
    // getpwuid_r writes its result and strings into caller-owned storage.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() || entry.pw_name.is_null() {
        return Err(io::ErrorKind::NotFound.into());
    }
    Ok(unsafe { CStr::from_ptr(entry.pw_name) }
        .to_str()
        .map_err(|_| io::ErrorKind::InvalidData)?
        .to_owned())
}

struct Conversation<'a> {
    stream: &'a mut UnixStream,
    attempt: Attempt,
}

unsafe extern "C" fn converse(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    // PAM supplies these pointers for this synchronous conversation call.
    if !(1..=MAX_PAM_MESSAGES).contains(&count)
        || messages.is_null()
        || responses.is_null()
        || data.is_null()
    {
        return PAM_CONV_ERR;
    }
    unsafe {
        *responses = ptr::null_mut();
    }
    let conversation = unsafe { &mut *data.cast::<Conversation<'_>>() };
    let allocation = unsafe { libc::calloc(count as usize, std::mem::size_of::<PamResponse>()) }
        .cast::<PamResponse>();
    if allocation.is_null() {
        return PAM_BUF_ERR;
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> io::Result<()> {
        for index in 0..count as usize {
            let message = unsafe { *messages.add(index) };
            if message.is_null() || unsafe { (*message).text.is_null() } {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let message = unsafe { &*message };
            let kind = match message.style {
                1 => PromptKind::Secret,
                2 => PromptKind::Visible,
                3 => PromptKind::Error,
                4 => PromptKind::Info,
                _ => return Err(io::ErrorKind::InvalidData.into()),
            };
            let text = unsafe { CStr::from_ptr(message.text) }
                .to_str()
                .map_err(|_| io::ErrorKind::InvalidData)?;
            if text.len() > MAX_RESPONSE_BYTES {
                return Err(io::ErrorKind::InvalidData.into());
            }
            codec::write(
                conversation.stream,
                codec::Frame::Prompt(
                    conversation.attempt,
                    Prompt {
                        kind,
                        text: text.to_owned(),
                    },
                ),
            )?;
            if matches!(kind, PromptKind::Secret | PromptKind::Visible) {
                let codec::Frame::Response(attempt, secret) = codec::read(conversation.stream)?
                else {
                    return Err(io::ErrorKind::InvalidData.into());
                };
                if attempt != conversation.attempt {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                let length = secret.expose().len();
                let response = unsafe { libc::malloc(length + 1) }.cast::<c_char>();
                if response.is_null() {
                    return Err(io::ErrorKind::OutOfMemory.into());
                }
                unsafe {
                    ptr::copy_nonoverlapping(
                        secret.expose().as_ptr(),
                        response.cast::<u8>(),
                        length,
                    );
                    *response.add(length) = 0;
                    (*allocation.add(index)).response = response;
                }
            }
        }
        Ok(())
    }));
    if matches!(outcome, Ok(Ok(()))) {
        unsafe {
            *responses = allocation;
        }
        PAM_SUCCESS
    } else {
        for index in 0..count as usize {
            let response = unsafe { (*allocation.add(index)).response };
            if !response.is_null() {
                let length = unsafe { CStr::from_ptr(response) }.to_bytes().len();
                unsafe {
                    libc::explicit_bzero(response.cast(), length);
                    libc::free(response.cast());
                }
            }
        }
        unsafe {
            libc::free(allocation.cast());
        }
        PAM_CONV_ERR
    }
}

pub(crate) fn authenticate(
    stream: &mut UnixStream,
    attempt: Attempt,
    user: &str,
    configuration: Option<&CStr>,
) -> bool {
    let Ok(user) = CString::new(user) else {
        return false;
    };
    let mut state = Conversation { stream, attempt };
    let conversation = PamConversation {
        callback: converse,
        data: (&mut state as *mut Conversation<'_>).cast(),
    };
    let mut handle = ptr::null_mut();
    #[cfg(test)]
    let status = unsafe {
        match configuration {
            Some(directory) => pam_start_confdir(
                c"oxilock".as_ptr(),
                user.as_ptr(),
                &conversation,
                directory.as_ptr(),
                &mut handle,
            ),
            None => pam_start(
                c"oxilock".as_ptr(),
                user.as_ptr(),
                &conversation,
                &mut handle,
            ),
        }
    };
    #[cfg(not(test))]
    let status = {
        let _ = configuration;
        unsafe {
            pam_start(
                c"oxilock".as_ptr(),
                user.as_ptr(),
                &conversation,
                &mut handle,
            )
        }
    };
    if status != PAM_SUCCESS || handle.is_null() {
        return false;
    }
    let mut status = unsafe { pam_authenticate(handle, PAM_DISALLOW_NULL_AUTHTOK) };
    if status == PAM_SUCCESS {
        status = unsafe { pam_acct_mgmt(handle, 0) };
    }
    let mut authenticated_user = ptr::null();
    let identity_matches = status == PAM_SUCCESS
        && unsafe { pam_get_item(handle, PAM_USER, &mut authenticated_user) } == PAM_SUCCESS
        && !authenticated_user.is_null()
        && unsafe { CStr::from_ptr(authenticated_user.cast()) } == user.as_c_str();
    unsafe {
        pam_end(handle, status);
    }
    identity_matches
}
