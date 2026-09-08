//! Offline integration checks of the actual WebSocket/audio/input pump. The
//! server is a loopback peer; no API key, microphone, or native typing is used.
use super::*;
use tokio::net::TcpListener;

type Peer = WebSocketStream<TcpStream>;
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn test_session() -> Session {
    Session {
        status: Mutex::new(LiveStatus::default()),
        active: AtomicBool::new(true),
        cancelled: Arc::new(AtomicBool::new(false)),
        input_stopped: AtomicBool::new(false),
        input_finished: AtomicBool::new(false),
        task: Mutex::new(None),
        settings: AppSettings::default(),
        started: Instant::now(),
    }
}

async fn sockets() -> (Socket, Peer) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let connect = tokio_tungstenite::connect_async(format!("ws://{address}"));
    let accept = async {
        let (tcp, _) = listener.accept().await.unwrap();
        tokio_tungstenite::accept_async(tcp).await.unwrap()
    };
    let (client, server) = tokio::join!(connect, accept);
    (client.unwrap().0, server)
}

async fn peer_event(peer: &mut Peer) -> Value {
    let message = peer.next().await.unwrap().unwrap();
    let Message::Text(text) = message else {
        panic!("expected a JSON event, received {message:?}");
    };
    serde_json::from_str(&text).unwrap()
}

async fn peer_send(peer: &mut Peer, event: Value) {
    peer.send(Message::Text(event.to_string().into()))
        .await
        .unwrap();
}

fn delta(id: &str, text: &str) -> Value {
    json!({
        "type": "conversation.item.input_audio_transcription.delta",
        "event_id": id,
        "item_id": "loopback-turn",
        "delta": text,
    })
}

async fn inserted(input: &input_queue::Receiver<String>) -> String {
    // Never block Tokio's current-thread test runtime on a native input queue.
    loop {
        match input.try_recv() {
            Ok(text) => return text,
            Err(input_queue::TryRecvError::Empty) => {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Err(input_queue::TryRecvError::Disconnected) => {
                panic!("transport closed before delivering the expected text");
            }
        }
    }
}

async fn final_transcript_case(suffix: &str) {
    timeout(TEST_TIMEOUT, async {
        let (socket, mut peer) = sockets().await;
        let session = test_session();
        let (audio_tx, audio_rx) = mpsc::channel(4);
        let (input_tx, input_rx) = input_queue::sync_channel(16);
        let pcm = vec![0x34, 0x12].repeat(2_400);
        audio_tx.send(pcm.clone()).await.unwrap();
        let transport = stream(socket, audio_rx, input_tx, || None, &session);
        let server = async {
            let append = peer_event(&mut peer).await;
            assert_eq!(append["type"], "input_audio_buffer.append");
            assert_eq!(
                STANDARD.decode(append["audio"].as_str().unwrap()).unwrap(),
                pcm
            );
            peer_send(&mut peer, delta("first", "Grüezi 😊\r")).await;
            let first = inserted(&input_rx).await;
            assert_eq!(first, "Grüezi 😊 ");
            // Capture is still open: receiving this proves delivery does not
            // wait for sender closure, commit, or a completed transcript event.
            assert!(!audio_tx.is_closed());
            assert_eq!(session.snapshot().text, "Grüezi 😊\r");
            assert_ne!(session.snapshot().phase, "finishing");

            peer_send(&mut peer, delta("first", "Grüezi 😊\r")).await;
            peer_send(&mut peer, delta("second", "\nWelt\t!")).await;
            let second = inserted(&input_rx).await;
            assert_eq!(second, "Welt !");
            // A new audio packet still reaches the peer before commit.
            audio_tx.send(vec![0; 960]).await.unwrap();
            assert_eq!(
                peer_event(&mut peer).await["type"],
                "input_audio_buffer.append"
            );
            drop(audio_tx);
            assert_eq!(
                peer_event(&mut peer).await["type"],
                "input_audio_buffer.commit"
            );
            let final_text = format!("Grüezi 😊\r\nWelt\t!{suffix}");
            peer_send(
                &mut peer,
                json!({
                    "type": "conversation.item.input_audio_transcription.completed",
                    "event_id": "final",
                    "item_id": "loopback-turn",
                    "transcript": final_text,
                }),
            )
            .await;
            (first, second)
        };
        let (outcome, (first, second)) = tokio::join!(transport, server);
        outcome.unwrap();
        let tail = input_rx.try_iter().collect::<Vec<_>>().concat();
        assert_eq!(tail, suffix, "final text must append only its new suffix");
        let delivered = format!("{first}{second}{tail}");
        assert_eq!(delivered, format!("Grüezi 😊 Welt !{suffix}"));
        assert!(!delivered.chars().any(char::is_control));
        assert_eq!(
            session.snapshot().text,
            format!("Grüezi 😊\r\nWelt\t!{suffix}")
        );
        assert!(session.snapshot().warning.is_none());
        assert!(matches!(
            input_rx.try_recv(),
            Err(input_queue::TryRecvError::Disconnected)
        ));
    })
    .await
    .expect("live final-transcript loopback timed out");
}

#[tokio::test]
async fn live_delta_precedes_commit_and_identical_final_is_not_replayed() {
    final_transcript_case("").await;
}

#[tokio::test]
async fn live_final_appends_only_new_suffix_and_split_crlf_never_types_enter() {
    final_transcript_case(" Genau.").await;
}

#[tokio::test]
async fn abrupt_disconnect_keeps_received_transcript_without_replay() {
    timeout(TEST_TIMEOUT, async {
        let (socket, mut peer) = sockets().await;
        let session = test_session();
        let (audio_tx, audio_rx) = mpsc::channel(4);
        let (input_tx, input_rx) = input_queue::sync_channel(16);
        audio_tx.send(vec![0; 4_800]).await.unwrap();
        let transport = stream(socket, audio_rx, input_tx, || None, &session);
        let server = async {
            assert_eq!(
                peer_event(&mut peer).await["type"],
                "input_audio_buffer.append"
            );
            peer_send(&mut peer, delta("first", "Keep this text")).await;
            assert_eq!(inserted(&input_rx).await, "Keep this text");
            // Drop TCP without a WebSocket close handshake, while capture
            // remains open. Reconnecting/replaying is deliberately forbidden.
            drop(peer);
        };
        let (outcome, ()) = tokio::join!(transport, server);
        assert!(outcome.unwrap_err().contains("retained"));
        assert_eq!(session.snapshot().text, "Keep this text");
        assert!(audio_tx.is_closed());
        assert!(matches!(
            input_rx.try_recv(),
            Err(input_queue::TryRecvError::Disconnected)
        ));
    })
    .await
    .expect("disconnect loopback timed out");
}

#[tokio::test]
async fn cancellation_and_audio_failure_stop_an_open_connection() {
    for capture_fails in [false, true] {
        timeout(TEST_TIMEOUT, async {
            let (socket, mut peer) = sockets().await;
            let session = test_session();
            let fail_audio = AtomicBool::new(false);
            let (audio_tx, audio_rx) = mpsc::channel(4);
            let (input_tx, input_rx) = input_queue::sync_channel(16);
            let (finished_tx, finished_rx) = oneshot::channel();
            audio_tx.send(vec![0; 4_800]).await.unwrap();
            let transport = async {
                let outcome = stream(
                    socket,
                    audio_rx,
                    input_tx,
                    || {
                        fail_audio
                            .load(Ordering::Acquire)
                            .then(|| "Synthetic microphone overrun".into())
                    },
                    &session,
                )
                .await;
                let _ = finished_tx.send(());
                outcome
            };
            let server = async {
                assert_eq!(
                    peer_event(&mut peer).await["type"],
                    "input_audio_buffer.append"
                );
                peer_send(&mut peer, delta("first", "Already received")).await;
                assert_eq!(inserted(&input_rx).await, "Already received");
                if capture_fails {
                    fail_audio.store(true, Ordering::Release);
                } else {
                    session.cancelled.store(true, Ordering::Release);
                }
                // Keep the peer and microphone sender alive until the pump
                // notices the local stop condition, not a socket disconnect.
                finished_rx.await.unwrap();
                assert!(audio_tx.is_closed());
                drop(peer);
            };
            let (outcome, ()) = tokio::join!(transport, server);
            if capture_fails {
                assert_eq!(outcome.unwrap_err(), "Synthetic microphone overrun");
            } else {
                outcome.unwrap();
            }
            assert_eq!(session.snapshot().text, "Already received");
            assert!(matches!(
                input_rx.try_recv(),
                Err(input_queue::TryRecvError::Disconnected)
            ));
        })
        .await
        .expect("local-stop loopback timed out");
    }
}
