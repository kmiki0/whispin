// At-rest encryption for secrets (API keys) using Windows DPAPI.
//
// `CryptProtectData` ties the ciphertext to the current Windows user account,
// so settings.json on disk no longer holds plaintext keys: a copied file or a
// backup is useless to a different user / machine. Encryption is transparent
// to the rest of the app — settings::load() returns plaintext, settings::save()
// writes ciphertext.

#![cfg(windows)]

use base64::{engine::general_purpose, Engine as _};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

/// Marker prefix on encrypted values so we can tell DPAPI ciphertext apart from
/// legacy plaintext (settings.json files written before encryption existed).
const PREFIX: &str = "dpapi:v1:";

/// Encrypt a secret for storage. Empty stays empty. On failure we fall back to
/// storing the plaintext rather than silently dropping the user's key.
pub fn protect(plaintext: &str) -> String {
    if plaintext.is_empty() {
        return String::new();
    }
    // Already encrypted (e.g. a load->save round-trip that didn't touch keys).
    if plaintext.starts_with(PREFIX) {
        return plaintext.to_string();
    }
    match dpapi(plaintext.as_bytes(), Op::Protect) {
        Some(cipher) => format!("{PREFIX}{}", general_purpose::STANDARD.encode(cipher)),
        None => {
            eprintln!("[whispin] DPAPI protect failed; storing key unencrypted");
            plaintext.to_string()
        }
    }
}

/// Decrypt a stored secret. Values without the marker are treated as legacy
/// plaintext and returned as-is (they get encrypted on the next save).
pub fn unprotect(stored: &str) -> String {
    let Some(b64) = stored.strip_prefix(PREFIX) else {
        return stored.to_string();
    };
    let Ok(cipher) = general_purpose::STANDARD.decode(b64) else {
        eprintln!("[whispin] DPAPI value base64 decode failed");
        return String::new();
    };
    match dpapi(&cipher, Op::Unprotect) {
        Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        None => {
            eprintln!("[whispin] DPAPI unprotect failed (different user/machine?)");
            String::new()
        }
    }
}

enum Op {
    Protect,
    Unprotect,
}

fn dpapi(data: &[u8], op: Op) -> Option<Vec<u8>> {
    unsafe {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        let result = match op {
            Op::Protect => CryptProtectData(
                &in_blob,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            ),
            Op::Unprotect => CryptUnprotectData(
                &in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            ),
        };
        result.ok()?;

        if out_blob.pbData.is_null() {
            return None;
        }
        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let owned = slice.to_vec();
        let _ = LocalFree(HLOCAL(out_blob.pbData as *mut core::ffi::c_void));
        Some(owned)
    }
}
