//! Encrypting secrets for the current user (DPAPI).

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

pub const SCHEME: &str = "dpapi";

fn input(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    return CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
}

fn take(ok: i32, out: CRYPT_INTEGER_BLOB) -> Option<Vec<u8>> {
    if ok == 0 || out.pbData.is_null() {
        return None;
    }
    // SAFETY: on success the system returns a buffer of cbData bytes, freed with LocalFree.
    unsafe {
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        return Some(bytes);
    }
}

pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
    // SAFETY: valid input blob, optional arguments null.
    let ok =
        unsafe { CryptProtectData(&input(data), null(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
    return take(ok, out);
}

pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
    // SAFETY: valid input blob, optional arguments null.
    let ok = unsafe {
        CryptUnprotectData(&input(data), null_mut(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
    };
    return take(ok, out);
}
