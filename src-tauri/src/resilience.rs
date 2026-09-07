//! Making sure a request always comes back with an answer.
//!
//! A Tauri command that panics never settles its promise, so the interface
//! waits for ever with no error to show. That is exactly how one dependency
//! conflict turned into "Transcribing…" that never ended. Running the work on
//! its own task turns a panic into an error the user can read.

use std::future::Future;

/// Run `work` so that a panic inside it becomes an error instead of silence.
pub async fn always_answers<F, T>(work: F) -> Result<T, String>
where
    F: Future<Output = Result<T, String>> + Send + 'static,
    T: Send + 'static,
{
    match tokio::spawn(work).await {
        Ok(result) => result,
        Err(error) if error.is_panic() => {
            Err("Utterform hit an internal error and stopped. The recording was not lost if it was already saved to history.".into())
        }
        Err(_) => Err("Utterform cancelled this step before it finished".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_result_passes_through_untouched() {
        assert_eq!(always_answers(async { Ok(7) }).await, Ok(7));
        assert_eq!(
            always_answers(async { Err::<(), _>("no key".to_string()) }).await,
            Err("no key".to_string())
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_panic_becomes_an_error_rather_than_an_endless_wait() {
        let answer = always_answers(async {
            panic!("a dependency did something impossible");
            #[allow(unreachable_code)]
            Ok::<(), String>(())
        })
        .await;
        assert!(answer.is_err(), "a panic must still produce an answer");
        assert!(!answer.unwrap_err().is_empty(), "the user needs a message");
    }
}
