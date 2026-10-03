//! Values Windows protects for the current user (DPAPI).
//!
//! Both the Network Channel and the settings imported from the previous .NET application are kept
//! this way, so the calls live in one place instead of being copied per caller. Protected bytes are
//! the only form any of these values takes on disk.

use std::ptr;
use std::slice;

use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};

use crate::domain::AppError;

/// Protects `data` so only this user on this computer can open it again.
///
/// `entropy` is the extra secret the previous application used for a value; passing the same value
/// is what makes an existing protected value readable.
pub(crate) fn protect(data: &[u8], entropy: Option<&[u8]>) -> Result<Vec<u8>, AppError> {
    let input = blob(data);
    let entropy = entropy.map(blob);
    let entropy = entropy
        .as_ref()
        .map_or(ptr::null(), |blob| blob as *const CRYPT_INTEGER_BLOB);
    let mut output = CRYPT_INTEGER_BLOB::default();
    let protected = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if protected == 0 {
        return Err(AppError::internal(
            "Windows could not protect this value for the current user.",
        ));
    }
    Ok(take_output(output))
}

/// Opens bytes written by [`protect`] with the same entropy, or by the previous application.
pub(crate) fn unprotect(data: &[u8], entropy: Option<&[u8]>) -> Result<Vec<u8>, AppError> {
    let input = blob(data);
    let entropy = entropy.map(blob);
    let entropy = entropy
        .as_ref()
        .map_or(ptr::null(), |blob| blob as *const CRYPT_INTEGER_BLOB);
    let mut output = CRYPT_INTEGER_BLOB::default();
    let opened = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            entropy,
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if opened == 0 {
        return Err(AppError::internal(
            "Windows could not open this protected value for the current user.",
        ));
    }
    Ok(take_output(output))
}

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        // The Windows calls only read the input buffers, so the cast is for the type they declare.
        pbData: bytes.as_ptr() as *mut u8,
    }
}

/// Copies the result out of the buffer Windows allocated and releases it.
///
/// Windows allocates this buffer on success, so an absent one is not expected; `from_raw_parts`
/// still requires a non-null, aligned pointer even for a zero length, so a null buffer is answered
/// as no output rather than being relied on never to happen.
fn take_output(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = if output.pbData.is_null() || output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec()
    };
    // The buffer is only ours to release when Windows handed one back.
    if !output.pbData.is_null() {
        unsafe {
            LocalFree(output.pbData.cast());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_buffer_is_reported_as_no_output() {
        assert!(take_output(CRYPT_INTEGER_BLOB::default()).is_empty());
    }
}
