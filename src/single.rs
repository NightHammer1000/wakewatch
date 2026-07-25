//! Single-instance guard built on a named mutex.

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, SetLastError, WIN32_ERROR,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::{HSTRING, PCWSTR};

pub const MUTEX_NAME: &str = "Local\\WakeWatch-singleton";

/// Holds the mutex for as long as this process should be the live instance.
pub struct InstanceGuard {
    handle: HANDLE,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

/// Returns `None` when another instance already holds the mutex.
///
/// If the mutex cannot be created at all we deliberately fail *open* and hand
/// back a guard: refusing to start because of an unexpected error is worse
/// than briefly running two copies.
pub fn acquire(name: &str) -> Option<InstanceGuard> {
    let wide = HSTRING::from(name);
    unsafe {
        // CreateMutexW only *sets* ERROR_ALREADY_EXISTS; it does not reliably
        // clear a stale value, so clear it ourselves before asking.
        SetLastError(WIN32_ERROR(0));
        match CreateMutexW(None, false, PCWSTR(wide.as_ptr())) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = CloseHandle(handle);
                    None
                } else {
                    Some(InstanceGuard { handle })
                }
            }
            Err(_) => Some(InstanceGuard {
                handle: HANDLE::default(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_acquire_succeeds_and_second_is_blocked() {
        let name = "Local\\WakeWatch-test-first-second";
        let first = acquire(name).expect("first acquire should succeed");
        assert!(acquire(name).is_none(), "second acquire should be blocked");
        drop(first);
        // Once released, the name is available again.
        assert!(acquire(name).is_some(), "should be reusable after drop");
    }

    #[test]
    fn a_fresh_name_is_always_available() {
        assert!(acquire("Local\\WakeWatch-test-fresh-name").is_some());
    }

    #[test]
    fn stale_last_error_does_not_cause_a_false_positive() {
        // Poison GetLastError, then confirm acquire still reports "first".
        unsafe { SetLastError(ERROR_ALREADY_EXISTS) };
        assert!(
            acquire("Local\\WakeWatch-test-stale-error").is_some(),
            "a stale ERROR_ALREADY_EXISTS must not be mistaken for a duplicate"
        );
    }
}
