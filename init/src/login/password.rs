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
mod tests;
