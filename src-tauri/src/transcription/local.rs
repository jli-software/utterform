use tauri::AppHandle;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{audio::RecordingArtifact, diagnostics, domain::AppSettings, models};

pub async fn transcribe(
    app: &AppHandle,
    artifact: &RecordingArtifact,
    settings: &AppSettings,
) -> Result<String, String> {
    let model_id = settings
        .local_model_id
        .as_deref()
        .ok_or_else(|| "Select a local Whisper model in Settings".to_string())?;
    let model_path = models::model_path(app, model_id)?;
    if !model_path.exists() {
        return Err(format!(
            "The {model_id} Whisper model is not downloaded. Open Settings to download it."
        ));
    }
    let audio = artifact.whisper_pcm()?;
    if audio.is_empty() {
        return Err("The recording contains no audio samples".into());
    }
    let language = settings
        .language_hints
        .iter()
        .find(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());
    // Whisper has no keyword field; the documented way to bias it towards a
    // spelling is to put the words in the prompt it starts from. The same
    // vocabulary therefore helps whichever engine is selected.
    let vocabulary = initial_prompt(&settings.vocabulary);

    tokio::task::spawn_blocking(move || {
        report_build_once();
        let context = WhisperContext::new_with_params(
            model_path.to_string_lossy().as_ref(),
            WhisperContextParameters::default(),
        )
        .map_err(|error| format!("Could not load the Whisper model: {error}"))?;
        let mut state = context
            .create_state()
            .map_err(|error| format!("Could not initialize Whisper: {error}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_language(language.as_deref());
        if let Some(prompt) = vocabulary.as_deref() {
            params.set_initial_prompt(prompt);
        }
        state
            .full(params, &audio)
            .map_err(|error| format!("Local Whisper transcription failed: {error}"))?;
        let transcript = state
            .as_iter()
            .map(|segment| segment.to_string())
            .collect::<Vec<_>>()
            .join("");
        if transcript.trim().is_empty() {
            return Err("Local Whisper returned an empty transcription".into());
        }
        Ok(transcript.trim().to_string())
    })
    .await
    .map_err(|error| format!("Local Whisper worker failed: {error}"))?
}

/// Which instruction set this build of whisper.cpp was compiled for, written
/// to the log the first time a recording needs it.
///
/// A binary built for a larger processor than it runs on does not fail here,
/// it dies: the operating system stops it with an illegal instruction while
/// the model is loading, before a sample is read, and nothing is left to
/// report. 0.7.2 shipped such a Linux binary. This line is what tells the next
/// report apart from every other reason a transcription can go wrong.
fn report_build_once() {
    static REPORTED: std::sync::Once = std::sync::Once::new();
    REPORTED.call_once(|| {
        diagnostics::log(format!(
            "local whisper build: {}",
            whisper_rs::print_system_info()
        ));
    });
}

/// The vocabulary as a sentence Whisper can start from. Whisper takes prior
/// text rather than a keyword list, and a comma-separated run of the words is
/// the documented way to bias its spelling.
fn initial_prompt(vocabulary: &[String]) -> Option<String> {
    let terms = vocabulary
        .iter()
        .map(|term| term.trim())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_vocabulary_leaves_whisper_alone() {
        assert_eq!(initial_prompt(&[]), None);
        assert_eq!(initial_prompt(&["  ".to_string()]), None);
    }

    /// The build must run on the desktops it is shipped to, not only on the
    /// machine that compiled it.
    ///
    /// ggml builds for the current processor unless told otherwise, and the
    /// 0.7.2 Linux release was therefore compiled on a runner with AVX-512 and
    /// AMX. It died with an illegal instruction on an Intel N300 the moment a
    /// model was loaded — in ordinary C++ frame code, before ggml's own
    /// runtime dispatch could choose a kernel, which is why that dispatch did
    /// not save it. `packaging/cmake/portable-cpu.cmake` is what prevents it;
    /// this is what notices if that ever stops arriving, including when a
    /// cached build of `whisper-rs-sys` outlives the change that fixed it.
    #[test]
    fn the_build_uses_no_instruction_a_supported_desktop_may_lack() {
        // Reading the list runs ggml's own backend registration — the very
        // code that faults on a processor below the baseline — so only a
        // machine at or above it can answer the question. Every build host
        // and CI runner is; the answer is the same everywhere regardless,
        // because these flags are compiled in, not detected.
        #[cfg(target_arch = "x86_64")]
        if !std::arch::is_x86_feature_detected!("avx2") {
            return;
        }
        let build = whisper_rs::print_system_info();
        for absent in ["AVX512", "AMX", "AVX_VNNI"] {
            assert!(
                !build.contains(absent),
                "{absent} is in this build: {build}"
            );
        }
        #[cfg(target_arch = "x86_64")]
        for present in ["AVX2", "FMA"] {
            assert!(
                build.contains(present),
                "{present} is missing from this build: {build}"
            );
        }
        #[cfg(target_arch = "aarch64")]
        assert!(build.contains("NEON"), "NEON is missing: {build}");
    }

    #[test]
    fn the_words_reach_whisper_as_prior_text() {
        let vocabulary = ["Careum".to_string(), "  ".into(), " Utterform ".into()];
        assert_eq!(initial_prompt(&vocabulary).unwrap(), "Careum, Utterform");
    }
}
