//! Nonblocking session gate for synchronous UI commands during background I/O.
use crate::error::CoreError;
use std::sync::{Mutex, MutexGuard, TryLockError};

pub(crate) fn try_lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, CoreError> {
    mutex.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => CoreError::OperationBusy,
        TryLockError::Poisoned(_) => CoreError::StateUnavailable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_commands_fail_fast_and_work_after_owner_finishes() {
        let mutex = std::sync::Arc::new(Mutex::new(7));
        let held = try_lock(&mutex).unwrap();
        let other = mutex.clone();
        std::thread::spawn(move || {
            assert!(matches!(try_lock(&other), Err(CoreError::OperationBusy)))
        })
        .join()
        .unwrap();
        drop(held);
        assert_eq!(*try_lock(&mutex).unwrap(), 7);
    }
}
