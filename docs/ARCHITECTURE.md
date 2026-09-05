# Architecture

Utterform uses Tauri 2 as its desktop shell, Rust for all privileged or compute-heavy work, and Svelte with TypeScript for the interface.

## Boundaries

- `audio.rs` — device discovery, CPAL capture, bounded handoff, temporary WAV lifecycle, Whisper normalization
- `transcription/openai.rs` — GPT Transcribe and Responses API calls
- `transcription/local.rs` — blocking local Whisper inference
- `models.rs` — curated model catalog, downloads, progress events, and SHA-256 verification
- `output.rs` — independent clipboard and file delivery
- `settings.rs` — non-secret JSON settings
- `secrets.rs` — OS credential-store access
- `commands.rs` — thin Tauri command boundary
- `src/` — presentation and focused-window keyboard interaction

The frontend never receives an API key or temporary audio path. Network calls originate in Rust.

## Recording lifecycle

1. Resolve the selected device by CPAL's stable device ID, falling back to the system default.
2. Capture native `f32`, `i16`, or `u16` samples.
3. Send small sample chunks over a bounded channel to a WAV writer thread.
4. Stop capture and finalize the temporary WAV.
5. Send the WAV to GPT Transcribe, or normalize it to 16 kHz mono `f32` for local Whisper.
6. Optionally transform the transcript with a separate text-model request.
7. Deliver to each selected output independently.
8. Delete the temporary recording when the artifact leaves scope.

## Deliberate MVP constraints

- Batch transcription only
- Focused-window shortcuts only
- CPU local inference by default
- No history, autostart, updater, or automatic text insertion
- No files-as-clipboard-objects
- No local LLM for transformations

GPU backends, global shortcuts, history, and release signing require separate platform work and testing.
