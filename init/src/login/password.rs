//! Skip secret input only after authentication verified the empty password.
//! GPL-3.0-or-later, matching OnyxKernel. Raw empty/locked shadow fields are
//! not authentication: the existing verifier must first succeed.
pub fn read_if_required<'a>(
    verified_empty: bool,
    buffer: &'a mut [u8],
    prompt_and_read: impl FnOnce(&'a mut [u8]) -> &'a [u8],
) -> &'a [u8] {
    if verified_empty {
        b""
    } else {
        prompt_and_read(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::read_if_required;
    #[test]
    fn verified_empty_password_never_prompts_or_consumes_input() {
        let mut buffer = [0xa5; 64];
        let password = read_if_required(true, &mut buffer, |_| {
            panic!("passwordless account prompted")
        });
        assert!(password.is_empty());
        assert_eq!(buffer, [0xa5; 64]);
    }
    #[test]
    fn protected_account_keeps_secret_reader_and_password_bytes() {
        let mut buffer = [0; 64];
        let mut prompts = 0;
        let password = read_if_required(false, &mut buffer, |buf| {
            prompts += 1;
            buf[..6].copy_from_slice(b"secret");
            &buf[..6]
        });
        assert_eq!(password, b"secret");
        assert_eq!(prompts, 1);
    }
    #[test]
    fn rejected_empty_password_does_not_skip_prompt() {
        let mut buffer = [0; 64];
        let mut prompts = 0;
        let password = read_if_required(false, &mut buffer, |buf| {
            prompts += 1;
            &buf[..0]
        });
        assert_eq!(prompts, 1);
        assert!(password.is_empty());
        // The caller retains the verifier's original Fail outcome for this case.
    }
}
