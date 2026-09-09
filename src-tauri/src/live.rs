//! Live-only orchestration. Batch transcription and its delivery path stay separate.
//! One manually committed audio turn per recording avoids cross-turn reordering.
//! Native input is serialized on its own thread; neither audio nor network callbacks
//! inject keys. No reconnect/replay: an uncertain delivery remains uncertain.
use crate::{
    audio::{self, AudioCaptureState},
    diagnostics,
    domain::{AppSettings, CloudModel, ProcessRequest, ProcessResult, TranscriptionEngine},
    feedback::{self, Cue},
    history::{self, HistoryEntry},
    output, secrets, tray,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc as input_queue,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::timeout,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
const FINISH_TIMEOUT: Duration = Duration::from_secs(20);
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TEXT: usize = 512_000;
const MAX_EVENTS: usize = 50_000;
/// After a stop is requested, native typing waits this long before sending
/// another key: the dictation shortcut's modifier is often still held, and a
/// compositor combines a held modifier with any key it sees.
const STOP_HOLD: Duration = Duration::from_millis(700);
/// Idle cadence of the input worker between deltas: only queued focus events
/// and cancellation are checked, never the compositor.
const INPUT_IDLE_TICK: Duration = Duration::from_millis(20);
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub text: String,
    pub inserted_text: String,
    pub delivery_paused: bool,
    pub warning: Option<String>,
    pub phase: &'static str,
}
impl Default for LiveStatus {
    fn default() -> Self {
        Self {
            text: String::new(),
            inserted_text: String::new(),
            delivery_paused: false,
            warning: None,
            phase: "connecting",
        }
    }
}
#[derive(Default)]
pub struct LiveState(Mutex<Option<Arc<Session>>>);
struct Session {
    /// Unique per process run; labels every diagnostic line of this session.
    id: u64,
    status: Mutex<LiveStatus>,
    active: AtomicBool,
    cancelled: Arc<AtomicBool>,
    /// Milliseconds after `started` before which the input worker sends no
    /// key; raised on every stop request.
    hold_until: Arc<AtomicU64>,
    // A native command can stop input immediately even while a network send awaits.
    input_stopped: AtomicBool,
    input_finished: AtomicBool,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    settings: AppSettings,
    started: Instant,
}
impl Session {
    fn snapshot(&self) -> LiveStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn update(&self, f: impl FnOnce(&mut LiveStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }
    fn warn(&self, message: &str) {
        self.update(|s| {
            if !s
                .warning
                .as_deref()
                .is_some_and(|old| old.contains(message))
            {
                s.warning = Some(match &s.warning {
                    Some(old) => format!("{old}\n{message}"),
                    None => message.to_string(),
                });
            }
        });
    }
    fn block_input(&self, reason: &str) {
        self.input_stopped.store(true, Ordering::Release);
        self.update(|s| s.delivery_paused = true);
        self.warn(reason);
    }
    fn log(&self, message: impl std::fmt::Display) {
        diagnostics::log(format!("live session {}: {message}", self.id));
    }
    /// A stop was requested (finish or cancel): hold native typing while the
    /// shortcut settles. Later requests extend the hold.
    fn request_stop(&self, how: &str) {
        let until = self.started.elapsed().as_millis() as u64 + STOP_HOLD.as_millis() as u64;
        self.hold_until.fetch_max(until, Ordering::AcqRel);
        self.log(format!(
            "{how} requested; typing held for {} ms",
            STOP_HOLD.as_millis()
        ));
    }
}
fn new_session(settings: AppSettings) -> Arc<Session> {
    Arc::new(Session {
        id: NEXT_SESSION_ID.fetch_add(1, Ordering::AcqRel),
        status: Mutex::new(LiveStatus::default()),
        active: AtomicBool::new(true),
        cancelled: Arc::new(AtomicBool::new(false)),
        hold_until: Arc::new(AtomicU64::new(0)),
        input_stopped: AtomicBool::new(false),
        input_finished: AtomicBool::new(false),
        task: Mutex::new(None),
        settings,
        started: Instant::now(),
    })
}

/// The native typing session as the input worker drives it. Implemented by the
/// platform typer and by test doubles.
trait LiveInput {
    fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>);
    fn set_stop_hold(&mut self, epoch: Instant, until: Arc<AtomicU64>);
    fn target_description(&self) -> String;
    /// Idle check between deltas; an error is a permanent loss for this session.
    fn poll_events(&mut self) -> Result<(), String>;
    /// Type one chunk; an error may follow partial delivery and is never retried.
    fn insert(&mut self, text: &str) -> Result<(), String>;
}
impl LiveInput for crate::typing::LiveTyper {
    fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) {
        crate::typing::LiveTyper::set_cancel_flag(self, flag);
    }
    fn set_stop_hold(&mut self, epoch: Instant, until: Arc<AtomicU64>) {
        crate::typing::LiveTyper::set_stop_hold(self, epoch, until);
    }
    fn target_description(&self) -> String {
        crate::typing::LiveTyper::target_description(self)
    }
    fn poll_events(&mut self) -> Result<(), String> {
        crate::typing::LiveTyper::poll_events(self)
    }
    fn insert(&mut self, text: &str) -> Result<(), String> {
        crate::typing::LiveTyper::insert(self, text)
    }
}

/// The input worker's loop. Owns the typer for the whole session and drops it
/// before returning, so the next session never meets a live socket, keyboard
/// or observer of this one. Queued chunks after a block are discarded, never
/// replayed.
fn run_input_worker<T: LiveInput>(
    session: &Session,
    receiver: &input_queue::Receiver<String>,
    mut typer: T,
) {
    typer.set_cancel_flag(session.cancelled.clone());
    typer.set_stop_hold(session.started, session.hold_until.clone());
    session.log(format!(
        "input worker started for {}",
        typer.target_description()
    ));
    let mut chunks = 0usize;
    loop {
        if session.cancelled.load(Ordering::Acquire) {
            session.log("input worker leaving: cancelled");
            break;
        }
        if !session.input_stopped.load(Ordering::Acquire)
            && let Err(reason) = typer.poll_events()
        {
            session.log(format!("input paused while idle: {reason}"));
            session.block_input(&format!("Live typing paused: {reason} The transcript remains here; start a new recording to type again."));
        }
        match receiver.recv_timeout(INPUT_IDLE_TICK) {
            Ok(text) => {
                if session.input_stopped.load(Ordering::Acquire) {
                    continue;
                }
                match typer.insert(&text) {
                    Ok(()) => {
                        chunks += 1;
                        session.update(|s| s.inserted_text.push_str(&text));
                    }
                    Err(reason) => {
                        session.log(format!("input paused after {chunks} chunk(s): {reason}"));
                        session.block_input(&format!("Live typing paused: {reason} This text was not retried; check the target before copying any remainder."));
                    }
                }
            }
            Err(input_queue::RecvTimeoutError::Timeout) => {}
            Err(input_queue::RecvTimeoutError::Disconnected) => {
                session.log(format!(
                    "input worker leaving: queue closed after {chunks} chunk(s)"
                ));
                break;
            }
        }
    }
    drop(typer);
    session.log("input worker finished; native input released");
}
impl LiveState {
    fn session(&self) -> Option<Arc<Session>> {
        self.0.lock().ok()?.clone()
    }
    pub fn active(&self) -> bool {
        self.session()
            .is_some_and(|s| s.active.load(Ordering::Acquire))
    }
    pub fn status(&self) -> Option<LiveStatus> {
        self.session().map(|s| s.snapshot())
    }
}
pub fn selected(settings: &AppSettings) -> bool {
    settings.engine == TranscriptionEngine::OpenAi
        && settings.cloud_model == CloudModel::GptLiveTranscribe
}
#[derive(Serialize)]
pub struct Support {
    pub supported: bool,
    pub explanation: &'static str,
}
pub fn support() -> Support {
    let supported = cfg!(target_os = "windows")
        || (cfg!(target_os = "linux")
            && std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some());
    Support {
        supported,
        explanation: if supported {
            "Live Dictation types at the cursor on Windows and Omarchy/Hyprland. Start with the dictation key from the target field. Window focus loss stops delivery until the next recording."
        } else {
            "Live Dictation in 0.6.0 supports Windows and Omarchy/Hyprland only. Use GPT Transcribe here; macOS Live is planned separately."
        },
    }
}

pub fn session_update(settings: &AppSettings) -> Value {
    let mut transcription = json!({"model":"gpt-live-transcribe", "delay":"low"});
    let keywords: Vec<_> = settings
        .vocabulary
        .iter()
        .map(|k| k.trim())
        .filter(|k| !k.is_empty() && !k.contains(['<', '>', '\r', '\n']))
        .collect();
    if !keywords.is_empty() {
        transcription["keywords"] = json!(keywords);
    }
    let languages: Vec<_> = settings
        .language_hints
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if !languages.is_empty() {
        transcription["languages"] = json!(languages);
    }
    if !settings.transcription_context.trim().is_empty() {
        transcription["prompt"] = json!(settings.transcription_context.trim());
    }
    json!({"type":"session.update","session":{"type":"transcription","audio":{"input":{
        "format":{"type":"audio/pcm","rate":24000},"transcription":transcription,"turn_detection":null
    }}}})
}
async fn send(socket: &mut Socket, value: Value) -> Result<(), String> {
    timeout(
        SEND_TIMEOUT,
        socket.send(Message::Text(value.to_string().into())),
    )
    .await
    .map_err(|_| "Live connection stopped accepting audio".to_string())?
    .map_err(|_| "Live connection could not send audio".to_string())
}
async fn connect(settings: &AppSettings) -> Result<Socket, String> {
    let mut request = "wss://api.openai.com/v1/realtime?intent=transcription"
        .into_client_request()
        .map_err(|_| "Could not prepare the live connection".to_string())?;
    // Native OS keyring only. Never put authentication or raw responses in diagnostics.
    let mut authorization = format!("Bearer {}", secrets::openai_api_key()?)
        .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()
        .map_err(|_| "The saved OpenAI API key is invalid".to_string())?;
    authorization.set_sensitive(true);
    request.headers_mut().insert("Authorization", authorization);
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.map_err(|error| {
        match error {
            tokio_tungstenite::tungstenite::Error::Http(response) => format!("OpenAI Live connection refused (HTTP {}). Check API access and billing in Settings.", response.status().as_u16()),
            _ => "Could not connect to OpenAI Live. Check your network connection.".into(),
        }
    })?;
    send(&mut socket, session_update(settings)).await?;
    loop {
        let message = socket
            .next()
            .await
            .ok_or("OpenAI closed the live session during setup")?
            .map_err(|_| "Live connection failed during setup")?;
        if let Message::Text(text) = message {
            let event: Value =
                serde_json::from_str(&text).map_err(|_| "Invalid live session response")?;
            match event["type"].as_str() {
                Some("session.updated") => return Ok(socket),
                Some("error") => return Err("OpenAI rejected the Live session configuration. Check model access, language hints, vocabulary and recording context.".into()),
                _ => {}
            }
        }
    }
}

// Control characters never become actions in a foreign app. Keep CRLF a single
// space even when its two halves arrive in separate deltas.
#[derive(Default)]
struct SingleLine {
    last_cr: bool,
}
impl SingleLine {
    fn append(&mut self, text: &str) -> String {
        let mut output = String::new();
        for c in text.chars() {
            if c == '\n' && self.last_cr {
                self.last_cr = false;
                continue;
            }
            self.last_cr = c == '\r';
            if matches!(c, '\r' | '\n' | '\t' | '\u{2028}' | '\u{2029}') {
                output.push(' ');
            } else if !c.is_control() {
                output.push(c);
            }
        }
        output
    }
}

#[derive(Default)]
struct Transcript {
    item: Option<String>,
    text: String,
    events: HashSet<String>,
    completed: bool,
}
struct Change {
    append: String,
    revised: bool,
}
impl Transcript {
    fn event(&mut self, event: &Value) -> Result<Change, String> {
        let kind = event["type"].as_str().unwrap_or("");
        let mut change = Change {
            append: String::new(),
            revised: false,
        };
        if !matches!(
            kind,
            "conversation.item.input_audio_transcription.delta"
                | "conversation.item.input_audio_transcription.completed"
                | "conversation.item.input_audio_transcription.failed"
        ) {
            return Ok(change);
        }
        if let Some(id) = event["event_id"].as_str() {
            if self.events.contains(id) {
                return Ok(change);
            }
            if self.events.len() >= MAX_EVENTS {
                return Err("Live transcript event limit reached".into());
            }
            self.events.insert(id.to_string());
        }
        let item = event["item_id"]
            .as_str()
            .ok_or("Live transcription event has no item identity")?;
        match &self.item {
            Some(previous) if previous != item => {
                return Err(
                    "Unexpected second audio turn; live delivery stopped to preserve text order"
                        .into(),
                );
            }
            None => self.item = Some(item.to_string()),
            _ => {}
        }
        if self.completed {
            return Ok(change);
        }
        if kind.ends_with(".failed") {
            return Err("OpenAI could not transcribe this live audio turn. Received text has been retained.".into());
        }
        if kind.ends_with(".delta") {
            let delta = event["delta"]
                .as_str()
                .ok_or("Invalid live transcript delta")?;
            if self.text.len() + delta.len() > MAX_TEXT {
                return Err("Live transcript size limit reached".into());
            }
            self.text.push_str(delta);
            change.append = delta.to_string();
        } else {
            let final_text = event["transcript"]
                .as_str()
                .ok_or("Invalid final live transcript")?;
            if final_text.len() > MAX_TEXT {
                return Err("Live transcript size limit reached".into());
            }
            if let Some(suffix) = final_text.strip_prefix(&self.text) {
                change.append = suffix.to_string();
            } else {
                change.revised = true;
            }
            self.text = final_text.to_string();
            self.completed = true;
        }
        Ok(change)
    }
}

async fn prepare_input(
    session: Arc<Session>,
) -> Result<(input_queue::SyncSender<String>, oneshot::Receiver<()>), String> {
    let id = session.id;
    prepare_input_with(session, move || crate::typing::LiveTyper::capture(id)).await
}

/// Start the dedicated input thread. `capture` runs on that thread, so a typer
/// that must not cross threads is created where it is used.
async fn prepare_input_with<T, F>(
    session: Arc<Session>,
    capture: F,
) -> Result<(input_queue::SyncSender<String>, oneshot::Receiver<()>), String>
where
    T: LiveInput,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let (sender, receiver) = input_queue::sync_channel::<String>(128);
    let (ready_tx, ready_rx) = oneshot::channel();
    let (done_tx, done_rx) = oneshot::channel();
    std::thread::spawn(move || {
        struct Finished(Arc<Session>);
        impl Drop for Finished {
            fn drop(&mut self) {
                self.0.input_finished.store(true, Ordering::Release);
            }
        }
        let finished = Finished(session.clone());
        let typer = match capture() {
            Ok(typer) => {
                let _ = ready_tx.send(Ok(()));
                typer
            }
            Err(error) => {
                session.log(format!("native input could not be captured: {error}"));
                let _ = ready_tx.send(Err(error));
                return;
            }
        };
        run_input_worker(&session, &receiver, typer);
        // Native input is released before "done" is reported, so a caller
        // awaiting the worker never observes a session that still owns it.
        drop(finished);
        let _ = done_tx.send(());
    });
    timeout(Duration::from_secs(5), ready_rx)
        .await
        .map_err(|_| "Live typing initialization timed out")?
        .map_err(|_| "Live typing worker stopped")??;
    Ok((sender, done_rx))
}

pub async fn start(
    app: AppHandle,
    settings: AppSettings,
    input_device: Option<String>,
) -> Result<(), String> {
    if !support().supported {
        return Err(support().explanation.into());
    }
    if audio::status(&app.state::<AudioCaptureState>())?.recording {
        return Err("A recording is already active".into());
    }
    let session = new_session(settings);
    {
        let state = app.state::<LiveState>();
        let mut slot = state.0.lock().map_err(|_| "Live state unavailable")?;
        if let Some(previous) = slot.as_ref() {
            if previous.active.load(Ordering::Acquire) {
                return Err("A live recording is already active".into());
            }
            // The previous worker must have released its native input; a
            // session whose shutdown timed out keeps `active` and lands above.
            if !previous.input_finished.load(Ordering::Acquire) {
                session.log(format!(
                    "refusing to start: session {} still owns native input",
                    previous.id
                ));
                return Err(
                    "The previous live session has not released native input yet. Wait a moment and try again."
                        .into(),
                );
            }
            session.log(format!("starting after session {}", previous.id));
        } else {
            session.log("starting");
        }
        *slot = Some(session.clone());
    }
    let setup: Result<(), String> = async {
        let (input, input_done) = prepare_input(session.clone()).await?;
        let socket = timeout(CONNECT_TIMEOUT, connect(&session.settings))
            .await
            .map_err(|_| "Connecting to OpenAI Live timed out")??;
        if session.cancelled.load(Ordering::Acquire)
            || session.input_stopped.load(Ordering::Acquire)
        {
            // The worker names what it saw: a confirmed window change or a
            // technical fault, never a desktop event alone.
            return Err(session.snapshot().warning.unwrap_or_else(|| {
                "Live setup was interrupted. Start again from your text field.".into()
            }));
        }
        let (audio_tx, audio_rx) = mpsc::channel(750);
        let captured_app = app.clone();
        let sound = session.settings.sound_enabled;
        let started = tauri::async_runtime::spawn_blocking(move || {
            audio::start_recording_live(
                &captured_app.state::<AudioCaptureState>(),
                input_device.as_deref(),
                sound,
                audio_tx,
            )
        })
        .await
        .map_err(|_| "Could not open the live microphone")??;
        let recording_id = started.session;
        tray::set_recording(&app, true);
        session.update(|s| s.phase = "streaming");
        session.log("connected and streaming");
        let cued = app.clone();
        std::thread::spawn(move || {
            if let Err(reason) = started.arm(&cued.state::<AudioCaptureState>()) {
                crate::commands::announce_recording(&cued, &reason);
            }
        });
        let running_app = app.clone();
        let running_session = session.clone();
        let task = tauri::async_runtime::spawn(async move {
            let outcome = stream(
                socket,
                audio_rx,
                input,
                || audio::live_error(&running_app.state::<AudioCaptureState>()),
                &running_session,
            )
            .await;
            if let Err(error) = outcome
                && !running_session.cancelled.load(Ordering::Acquire)
            {
                running_session.log(format!("stream failed: {error}"));
                running_session.block_input(&error);
                running_session.update(|s| s.phase = "failed");
                let stopped_app = running_app.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    let _ = audio::cancel_recording(&stopped_app.state::<AudioCaptureState>());
                    tray::set_recording(&stopped_app, false);
                })
                .await;
                let _ = running_app.emit("live-failed", error);
            }
            if !matches!(
                timeout(Duration::from_secs(3), input_done).await,
                Ok(Ok(()))
            ) {
                running_session
                    .block_input("Live input shutdown timed out; delivery is incomplete.");
            }
            if running_session.snapshot().phase != "failed" {
                running_session.update(|s| s.phase = "completed");
            }
        });
        *session
            .task
            .lock()
            .map_err(|_| "Live task state unavailable")? = Some(task);
        let watched = app.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(250));
                match audio::check_limit(&watched.state::<AudioCaptureState>(), recording_id) {
                    Ok(audio::LimitCheck::Waiting) => {}
                    Ok(audio::LimitCheck::Stopped) => {
                        tray::set_recording(&watched, false);
                        let _ = watched.emit("recording-limit-reached", ());
                        break;
                    }
                    _ => break,
                }
            }
        });
        Ok(())
    }
    .await;
    if let Err(ref reason) = setup {
        session.log(format!("setup failed: {reason}"));
        session.cancelled.store(true, Ordering::Release);
        session.active.store(false, Ordering::Release);
        session.update(|s| {
            s.phase = "failed";
            s.warning = Some(reason.clone());
        });
    }
    setup
}

async fn stream(
    mut socket: Socket,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
    input: input_queue::SyncSender<String>,
    audio_error: impl Fn() -> Option<String>,
    session: &Session,
) -> Result<(), String> {
    let mut transcript = Transcript::default();
    let mut line = SingleLine::default();
    let mut total_bytes = 0usize;
    let mut committed_at: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(25));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if session.cancelled.load(Ordering::Acquire) { return Ok(()); }
                if let Some(error) = audio_error() { return Err(error); }
                if committed_at.is_some_and(|at| at.elapsed() > FINISH_TIMEOUT) { return Err("The final live transcript timed out. Received text has been retained.".into()); }
            }
            chunk = audio_rx.recv(), if committed_at.is_none() => {
                match chunk {
                    Some(bytes) => {
                        total_bytes += bytes.len();
                        send(&mut socket, json!({"type":"input_audio_buffer.append","audio":STANDARD.encode(bytes)})).await?;
                    }
                    None => {
                        if total_bytes < 4_800 { return Err("The live recording is too short. Speak for at least a moment before finishing.".into()); }
                        send(&mut socket, json!({"type":"input_audio_buffer.commit"})).await?;
                        committed_at = Some(Instant::now());
                        session.update(|s| s.phase = "finishing");
                    }
                }
            }
            message = socket.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let event: Value = serde_json::from_str(&text).map_err(|_| "OpenAI returned an invalid Live event")?;
                        if event["type"] == "error" { return Err("OpenAI reported a Live session error. Check network, API access, usage limits and voice settings. Received text has been retained.".into()); }
                        let change = transcript.event(&event)?;
                        session.update(|s| s.text.clone_from(&transcript.text));
                        if change.revised { session.warn("The final transcript differs from the live text. Text already typed was not corrected or repeated; compare it before copying."); }
                        let safe = line.append(&change.append);
                        if !safe.is_empty() && !session.input_stopped.load(Ordering::Acquire) {
                            // Limit each native injection to a short burst, including large final-only responses.
                            for chunk in text_batches(&safe) {
                                if input.try_send(chunk).is_err() {
                                    session.block_input("Live typing could not keep up. Output stopped; the remaining transcript is available here.");
                                    break;
                                }
                            }
                        }
                        if transcript.completed {
                            if committed_at.is_none() { return Err("The Live audio turn ended unexpectedly. Received text has been retained.".into()); }
                            return Ok(());
                        }
                    }
                    Some(Ok(Message::Ping(bytes))) => { timeout(SEND_TIMEOUT, socket.send(Message::Pong(bytes))).await.map_err(|_| "Live heartbeat timed out")?.map_err(|_| "Live heartbeat failed")?; }
                    Some(Ok(Message::Close(_))) | None => return Err("The Live connection closed before completion. Received text has been retained.".into()),
                    Some(Err(_)) => return Err("The Live connection was interrupted. Received text has been retained; nothing was replayed.".into()),
                    _ => {}
                }
            }
        }
    }
}
fn text_batches(text: &str) -> Vec<String> {
    let mut batches = Vec::new();
    let mut buffer = String::new();
    for (i, c) in text.chars().enumerate() {
        if i > 0 && i % 32 == 0 {
            batches.push(std::mem::take(&mut buffer));
        }
        buffer.push(c);
    }
    if !buffer.is_empty() {
        batches.push(buffer);
    }
    batches
}

async fn await_input_stop(session: &Session) -> Result<(), String> {
    let deadline = Instant::now();
    while !session.input_finished.load(Ordering::Acquire) {
        if deadline.elapsed() > Duration::from_secs(5) {
            session.block_input(
                "The native input worker did not stop. Restart Utterform before another recording.",
            );
            return Err(
                "Live input shutdown is incomplete. Restart Utterform before another recording."
                    .into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}
pub async fn cancel(app: &AppHandle) -> Result<(), String> {
    if let Some(session) = app.state::<LiveState>().session()
        && session.active.load(Ordering::Acquire)
    {
        session.request_stop("cancel");
        session.cancelled.store(true, Ordering::Release);
        session.input_stopped.store(true, Ordering::Release);
        let stopped_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            audio::cancel_recording(&stopped_app.state::<AudioCaptureState>())
        })
        .await
        .map_err(|_| "Could not stop the live microphone")??;
        let task = session
            .task
            .lock()
            .map_err(|_| "Live task state unavailable")?
            .take();
        if let Some(mut task) = task
            && timeout(Duration::from_secs(7), &mut task).await.is_err()
        {
            task.abort();
        }
        await_input_stop(&session).await?;
        session.active.store(false, Ordering::Release);
        session.log("cancelled; native input released");
        session.update(|s| s.phase = "completed");
    }
    Ok(())
}

pub async fn finish(app: AppHandle, mut request: ProcessRequest) -> Result<ProcessResult, String> {
    let session = app
        .state::<LiveState>()
        .session()
        .ok_or("No live recording is active")?;
    session.request_stop("finish");
    // Remain active throughout finalization, so no second recording can reuse audio state.
    let stopped_app = app.clone();
    let artifact = tauri::async_runtime::spawn_blocking(move || {
        tray::set_recording(&stopped_app, false);
        audio::stop_recording(&stopped_app.state::<AudioCaptureState>())
    })
    .await
    .map_err(|_| "Could not stop the live microphone")?;
    let duration_ms = match artifact {
        Ok(artifact) => artifact.duration_ms(),
        Err(error) => {
            session.warn(&error);
            session.started.elapsed().as_millis() as u64
        }
    };
    let task = session
        .task
        .lock()
        .map_err(|_| "Live task state unavailable")?
        .take();
    if let Some(mut task) = task {
        match timeout(Duration::from_secs(30), &mut task).await {
            Ok(Ok(())) => {}
            _ => {
                session.cancelled.store(true, Ordering::Release);
                session.block_input(
                    "Live finalization did not complete. Received text has been retained.",
                );
                task.abort();
            }
        }
    }
    await_input_stop(&session).await?;
    session.active.store(false, Ordering::Release);
    session.log("finished; native input released");
    let status = session.snapshot();
    let text = status.text;
    let mut warnings = status.warning.into_iter().collect::<Vec<_>>();
    let history_entry = if session.settings.history_enabled && !text.trim().is_empty() {
        let entry = HistoryEntry::new(&text, duration_ms, TranscriptionEngine::OpenAi);
        match history::append(&app, entry.clone()) {
            Ok(()) => Some(entry),
            Err(error) => {
                warnings.push(format!("History was not saved: {error}"));
                None
            }
        }
    } else {
        None
    };
    // NEVER pass live text through batch typing (including on failure/partial success).
    request.type_at_cursor = false;
    request.action = "plain".into();
    let (saved_path, copied_to_clipboard) =
        if !text.is_empty() && (request.copy_to_clipboard || request.save_to_file) {
            match output::deliver(&app, &text, &request, &session.settings) {
                Ok(delivery) => {
                    warnings.extend(delivery.warnings);
                    (delivery.saved_path, delivery.copied_to_clipboard)
                }
                Err(error) => {
                    warnings.push(error);
                    (None, false)
                }
            }
        } else {
            (None, false)
        };
    if session.settings.sound_enabled && warnings.is_empty() {
        feedback::play_detached(Cue::Done);
    }
    Ok(ProcessResult {
        history_entry,
        text,
        saved_path,
        copied_to_clipboard,
        typed_at_cursor: !status.delivery_paused && !status.inserted_text.is_empty(),
        delivery_warnings: warnings,
        duration_ms,
        engine: TranscriptionEngine::OpenAi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn delta(id: &str, text: &str) -> Value {
        json!({"type":"conversation.item.input_audio_transcription.delta", "event_id":id,"item_id":"a","delta":text})
    }
    fn done(text: &str) -> Value {
        json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"a","transcript":text})
    }
    #[test]
    fn old_settings_keep_batch_and_local_engine_never_streams() {
        let mut settings: AppSettings = serde_json::from_str(r#"{"engine":"open_ai"}"#).unwrap();
        assert!(!selected(&settings));
        settings.cloud_model = CloudModel::GptLiveTranscribe;
        assert!(selected(&settings));
        settings.engine = TranscriptionEngine::LocalWhisper;
        assert!(!selected(&settings));
    }
    #[test]
    fn live_configuration_preserves_hints_without_legacy_language() {
        let settings = AppSettings {
            vocabulary: vec![" Utterform ".into(), "a\nb".into(), "<bad>".into()],
            language_hints: vec!["de".into(), "en".into()],
            transcription_context: "Team meeting".into(),
            ..Default::default()
        };
        let value = session_update(&settings);
        let t = &value["session"]["audio"]["input"]["transcription"];
        assert_eq!(t["model"], "gpt-live-transcribe");
        assert_eq!(t["keywords"], json!(["Utterform"]));
        assert_eq!(t["languages"], json!(["de", "en"]));
        assert!(t.get("language").is_none());
        assert_eq!(
            value["session"]["audio"]["input"]["turn_detection"],
            Value::Null
        );
    }
    #[test]
    fn delta_identity_and_final_do_not_duplicate_input() {
        let mut t = Transcript::default();
        assert_eq!(t.event(&delta("1", "Hallo ")).unwrap().append, "Hallo ");
        assert!(t.event(&delta("1", "Hallo ")).unwrap().append.is_empty());
        assert_eq!(t.event(&delta("2", "Welt")).unwrap().append, "Welt");
        assert_eq!(t.event(&done("Hallo Welt!")).unwrap().append, "!");
        assert!(t.event(&done("Hallo Welt!")).unwrap().append.is_empty());
        assert!(t.event(&delta("3", "late")).unwrap().append.is_empty());
    }
    #[test]
    fn corrections_stay_internal_and_never_backspace() {
        let mut t = Transcript::default();
        t.event(&delta("1", "Hello there")).unwrap();
        let changed = t.event(&done("Hallo dort!")).unwrap();
        assert!(changed.revised);
        assert!(changed.append.is_empty());
        assert_eq!(t.text, "Hallo dort!");
    }
    #[test]
    fn missing_identity_and_unexpected_turn_fail_closed() {
        let mut t = Transcript::default();
        t.event(&delta("1", "a")).unwrap();
        let mut e = delta("2", "b");
        e["item_id"] = json!("b");
        assert!(t.event(&e).is_err());
        let e = json!({"type":"conversation.item.input_audio_transcription.delta","delta":"bad"});
        assert!(t.event(&e).is_err());
    }
    /// A typer double: records chunks, can fail on demand, and reports when it
    /// was dropped and which hold it was handed.
    struct FakeTyper {
        inserted: Arc<Mutex<Vec<String>>>,
        fail_insert_at: Option<usize>,
        fail_poll_after: Option<usize>,
        polls: usize,
        dropped: Arc<AtomicBool>,
        hold_seen: Arc<AtomicU64>,
    }
    /// Handles the test keeps while the worker owns the typer.
    struct FakeHandles {
        inserted: Arc<Mutex<Vec<String>>>,
        dropped: Arc<AtomicBool>,
        hold_seen: Arc<AtomicU64>,
    }
    impl FakeTyper {
        fn new() -> (Self, FakeHandles) {
            let handles = FakeHandles {
                inserted: Arc::new(Mutex::new(Vec::new())),
                dropped: Arc::new(AtomicBool::new(false)),
                hold_seen: Arc::new(AtomicU64::new(u64::MAX)),
            };
            let typer = Self {
                inserted: handles.inserted.clone(),
                fail_insert_at: None,
                fail_poll_after: None,
                polls: 0,
                dropped: handles.dropped.clone(),
                hold_seen: handles.hold_seen.clone(),
            };
            (typer, handles)
        }
    }
    impl LiveInput for FakeTyper {
        fn set_cancel_flag(&mut self, _flag: Arc<AtomicBool>) {}
        fn set_stop_hold(&mut self, _epoch: Instant, until: Arc<AtomicU64>) {
            self.hold_seen
                .store(until.load(Ordering::Acquire), Ordering::Release);
        }
        fn target_description(&self) -> String {
            "fake window".into()
        }
        fn poll_events(&mut self) -> Result<(), String> {
            self.polls += 1;
            match self.fail_poll_after {
                Some(limit) if self.polls > limit => Err("another window received focus".into()),
                _ => Ok(()),
            }
        }
        fn insert(&mut self, text: &str) -> Result<(), String> {
            let mut inserted = self.inserted.lock().unwrap();
            if self.fail_insert_at == Some(inserted.len()) {
                return Err("another window received focus".into());
            }
            inserted.push(text.to_owned());
            Ok(())
        }
    }
    impl Drop for FakeTyper {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }
    async fn worker_done(done: oneshot::Receiver<()>) {
        timeout(Duration::from_secs(5), done)
            .await
            .expect("worker must finish")
            .expect("worker must report completion");
    }

    #[tokio::test]
    async fn consecutive_sessions_are_isolated_and_a_late_block_stays_with_its_session() {
        let first = new_session(AppSettings::default());
        let (typer, handles) = FakeTyper::new();
        let (sender, done) = prepare_input_with(first.clone(), move || Ok(typer))
            .await
            .unwrap();
        sender.send("Hallo ".into()).unwrap();
        drop(sender);
        worker_done(done).await;
        assert!(first.input_finished.load(Ordering::Acquire));
        assert!(
            handles.dropped.load(Ordering::Acquire),
            "native input must be released"
        );
        assert_eq!(
            handles.inserted.lock().unwrap().clone(),
            vec!["Hallo ".to_owned()]
        );

        let second = new_session(AppSettings::default());
        assert!(second.id > first.id);
        let (typer, handles) = FakeTyper::new();
        let (sender, done) = prepare_input_with(second.clone(), move || Ok(typer))
            .await
            .unwrap();
        // A late failure of the old session touches only the old session.
        first.block_input("late failure of the first session");
        first.cancelled.store(true, Ordering::Release);
        sender.send("Welt".into()).unwrap();
        drop(sender);
        worker_done(done).await;
        assert_eq!(
            handles.inserted.lock().unwrap().clone(),
            vec!["Welt".to_owned()]
        );
        assert!(handles.dropped.load(Ordering::Acquire));
        let status = second.snapshot();
        assert!(!status.delivery_paused);
        assert_eq!(status.warning, None);
        assert_eq!(status.inserted_text, "Welt");
        assert_eq!(
            handles.hold_seen.load(Ordering::Acquire),
            0,
            "a new session starts without a hold"
        );
        assert!(first.snapshot().delivery_paused);
    }

    #[tokio::test]
    async fn a_focus_loss_discards_queued_chunks_without_replay() {
        let session = new_session(AppSettings::default());
        let (mut typer, handles) = FakeTyper::new();
        typer.fail_insert_at = Some(1);
        let (sender, done) = prepare_input_with(session.clone(), move || Ok(typer))
            .await
            .unwrap();
        for chunk in ["eins ", "zwei ", "drei "] {
            sender.send(chunk.into()).unwrap();
        }
        drop(sender);
        worker_done(done).await;
        assert_eq!(
            handles.inserted.lock().unwrap().clone(),
            vec!["eins ".to_owned()]
        );
        let status = session.snapshot();
        assert!(status.delivery_paused);
        assert_eq!(status.inserted_text, "eins ");
        assert!(
            status
                .warning
                .unwrap()
                .contains("another window received focus")
        );
    }

    #[tokio::test]
    async fn a_loss_noticed_while_idle_blocks_the_next_chunk() {
        let session = new_session(AppSettings::default());
        let (mut typer, handles) = FakeTyper::new();
        typer.fail_poll_after = Some(2);
        let (sender, done) = prepare_input_with(session.clone(), move || Ok(typer))
            .await
            .unwrap();
        // Wait for the idle poll to notice the loss; timing differs per runner.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !session.snapshot().delivery_paused {
            assert!(Instant::now() < deadline, "the idle poll must block input");
            tokio::time::sleep(INPUT_IDLE_TICK).await;
        }
        sender.send("late text".into()).unwrap();
        drop(sender);
        worker_done(done).await;
        assert!(handles.inserted.lock().unwrap().is_empty());
        assert!(session.snapshot().delivery_paused);
    }

    #[tokio::test]
    async fn cancellation_ends_the_worker_and_releases_native_input() {
        let session = new_session(AppSettings::default());
        let (typer, handles) = FakeTyper::new();
        let (_sender, done) = prepare_input_with(session.clone(), move || Ok(typer))
            .await
            .unwrap();
        session.cancelled.store(true, Ordering::Release);
        worker_done(done).await;
        assert!(handles.dropped.load(Ordering::Acquire));
        assert!(session.input_finished.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn a_failed_capture_still_marks_the_worker_finished() {
        let session = new_session(AppSettings::default());
        let error = prepare_input_with(session.clone(), || {
            Err::<FakeTyper, String>("no target".into())
        })
        .await
        .unwrap_err();
        assert_eq!(error, "no target");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(session.input_finished.load(Ordering::Acquire));
    }

    #[test]
    fn a_stop_request_holds_typing_and_later_requests_only_extend_it() {
        let session = new_session(AppSettings::default());
        assert_eq!(session.hold_until.load(Ordering::Acquire), 0);
        session.request_stop("finish");
        let first = session.hold_until.load(Ordering::Acquire);
        assert!(first >= STOP_HOLD.as_millis() as u64);
        std::thread::sleep(Duration::from_millis(20));
        session.request_stop("cancel");
        assert!(session.hold_until.load(Ordering::Acquire) >= first + 20);
        assert_ne!(new_session(AppSettings::default()).id, session.id);
    }

    #[test]
    fn single_line_removes_actions_across_delta_boundaries() {
        let mut line = SingleLine::default();
        assert_eq!(line.append("ä😊\r"), "ä😊 ");
        assert_eq!(line.append("\n\tX\u{8}\u{0}\u{1b}\u{2028}Y"), " X Y");
        assert!(line.append("\n").chars().all(|c| !c.is_control()));
        let text = "ä😊".repeat(70);
        assert_eq!(text_batches(&text).concat(), text);
    }
}

#[cfg(test)]
#[path = "live_transport_tests.rs"]
mod transport_tests;
