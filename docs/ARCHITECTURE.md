# Architecture

Utterform uses Tauri 2 as its desktop shell, Rust for all privileged or compute-heavy work, and Svelte with TypeScript for the interface.

## Boundaries

- `audio.rs` — device discovery, CPAL capture, bounded handoff, temporary WAV lifecycle, Whisper normalization
- `transcription/openai.rs` — GPT Transcribe and Responses API calls
- `transcription/local.rs` — blocking local Whisper inference
- `models.rs` — curated model catalog, downloads, progress events, and SHA-256 verification
- `output.rs` — independent clipboard and file delivery
- `settings.rs` — non-secret JSON settings
- `history.rs` — bounded, atomic, device-local text history; local titles, no AI calls
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
7. Persist the completed text to local history (if enabled), before external delivery.
8. Deliver to each selected output independently.
9. Delete the temporary recording when the artifact leaves scope.

## Text history

`history.json` lives in Tauri's app-local-data directory, separately from settings and exported files. The newest 100 texts are retained, newest first, including clipboard-only results. Titles are the first words of the text (Unicode-safe, at most 64 characters plus ellipsis), without timestamps or additional model calls. The UI restores the latest entry and keeps the displayed text while another recording runs.

Writes are serialized with a mutex, use an owner-only temporary file in the same directory, flush data, and atomically replace the old file. Unreadable/corrupt history is never silently overwritten; the UI still receives the new text with a persistence warning. History is unencrypted and local to each device, not stored in Git or synchronized by the app. Users can disable future history and separately confirm clearing existing history in Settings. Clearing does not touch the clipboard or exported files.

## Deliberate MVP constraints

- Batch transcription only
- Focused-window shortcuts only
- CPU local inference by default
- No autostart, updater, or automatic text insertion
- No files-as-clipboard-objects
- No local LLM for transformations

GPU backends, global shortcuts, and release signing require separate platform work and testing.
