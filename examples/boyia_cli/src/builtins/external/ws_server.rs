//! WebSocketServer builtin via [`tokio-tungstenite`] (RFC 6455 handshake + frames).
//! Clients connect with `ws://host:port/`; server uses `receive` / `send` / `broadcast`.
//! Connections are keyed by monotonic `channel_id`; `peerPort` is the TCP peer source port (debug).

#![allow(dead_code)]

use builtin_macro::{boyia_class, boyia_native_object};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::{mpsc as tokio_mpsc, watch};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

type ClientTx = tokio_mpsc::UnboundedSender<Message>;

#[derive(Clone)]
struct InboxMessage {
    channel_id: usize,
    peer_port: u16,
    text: String,
}

#[derive(Clone)]
struct OnReceiveListener {
    ctx: crate::runner::builtin_ctx::BuiltinCtx,
    callback: crate::runner::builtin_async::ScriptCallback,
}

#[derive(Clone)]
struct OnCloseListener {
    ctx: crate::runner::builtin_ctx::BuiltinCtx,
    callback: crate::runner::builtin_async::ScriptCallback,
}

struct ServerRuntime {
    inbox_tx: Sender<InboxMessage>,
    inbox_rx: Arc<Mutex<Receiver<InboxMessage>>>,
    clients: Arc<Mutex<HashMap<usize, ClientTx>>>,
    next_channel_id: Arc<Mutex<usize>>,
    on_receive_listener: Arc<Mutex<Option<OnReceiveListener>>>,
    on_close_listener: Arc<Mutex<Option<OnCloseListener>>>,
    shutdown_tx: watch::Sender<()>,
    join: JoinHandle<()>,
}

impl ServerRuntime {
    fn start(host: String, port: u16) -> Option<Self> {
        let (inbox_tx, inbox_rx) = mpsc::channel();
        let inbox_rx = Arc::new(Mutex::new(inbox_rx));
        let clients = Arc::new(Mutex::new(HashMap::new()));
        let next_channel_id = Arc::new(Mutex::new(0usize));
        let on_receive_listener = Arc::new(Mutex::new(None));
        let on_close_listener = Arc::new(Mutex::new(None));
        let (shutdown_tx, shutdown_rx) = watch::channel(());
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);

        let inbox_bg = inbox_tx.clone();
        let clients_bg = Arc::clone(&clients);
        let next_channel_id_bg = Arc::clone(&next_channel_id);
        let on_receive_listener_bg = Arc::clone(&on_receive_listener);
        let on_close_listener_bg = Arc::clone(&on_close_listener);
        let bind_addr = format!("{host}:{port}");
        let join = std::thread::Builder::new()
            .name(format!("boyia-ws-{port}"))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .worker_threads(2)
                    .build()
                {
                    Ok(rt) => rt,
                    Err(err) => {
                        eprintln!("WebSocketServer: tokio runtime error: {err}");
                        let _ = ready_tx.send(false);
                        return;
                    }
                };
                rt.block_on(async {
                    let listener = match TcpListener::bind(&bind_addr).await {
                        Ok(listener) => listener,
                        Err(err) => {
                            eprintln!("WebSocketServer: bind {bind_addr} failed: {err}");
                            let _ = ready_tx.send(false);
                            return;
                        }
                    };
                    let _ = ready_tx.send(true);
                    run_server(
                        listener,
                        inbox_bg,
                        clients_bg,
                        next_channel_id_bg,
                        on_receive_listener_bg,
                        on_close_listener_bg,
                        shutdown_rx,
                    )
                    .await;
                });
            })
            .ok()?;

        let bound = match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(true) => true,
            _ => false,
        };
        if !bound {
            let _ = shutdown_tx.send(());
            let _ = join.join();
            return None;
        }

        Some(Self {
            inbox_tx,
            inbox_rx,
            clients,
            next_channel_id,
            on_receive_listener,
            on_close_listener,
            shutdown_tx,
            join,
        })
    }

    fn stop(self) {
        let _ = self.shutdown_tx.send(());
        let _ = self.join.join();
    }

    /// Block until a text frame arrives or the inbox channel is closed (shutdown).
    fn recv_blocking(&self) -> (usize, u16, String) {
        match self.inbox_rx.lock().unwrap().recv() {
            Ok(msg) => (msg.channel_id, msg.peer_port, msg.text),
            Err(_) => (0, 0, String::new()),
        }
    }

    fn set_on_receive_listener(&self, listener: Option<OnReceiveListener>) {
        *self.on_receive_listener.lock().unwrap() = listener;
    }

    fn set_on_close_listener(&self, listener: Option<OnCloseListener>) {
        *self.on_close_listener.lock().unwrap() = listener;
    }

    fn push_text_to_client(&self, channel_id: usize, message: String) -> bool {
        let clients = self.clients.lock().unwrap();
        let Some(tx) = clients.get(&channel_id) else {
            return false;
        };
        tx.send(Message::Text(message.into())).is_ok()
    }

    fn push_text_to_all_clients(&self, message: String) -> bool {
        let msg = Message::Text(message.into());
        let clients = self.clients.lock().unwrap();
        if clients.is_empty() {
            return false;
        }
        let mut any = false;
        for tx in clients.values() {
            if tx.send(msg.clone()).is_ok() {
                any = true;
            }
        }
        any
    }
}

fn normalize_host(host: &str) -> String {
    let host = host.trim();
    if host.is_empty() {
        "0.0.0.0".to_string()
    } else {
        host.to_string()
    }
}

fn enqueue_text(inbox_tx: &Sender<InboxMessage>, channel_id: usize, peer_port: u16, text: String) {
    let _ = inbox_tx.send(InboxMessage {
        channel_id,
        peer_port,
        text,
    });
}

async fn run_server(
    listener: TcpListener,
    inbox_tx: Sender<InboxMessage>,
    clients: Arc<Mutex<HashMap<usize, ClientTx>>>,
    next_channel_id: Arc<Mutex<usize>>,
    on_receive_listener: Arc<Mutex<Option<OnReceiveListener>>>,
    on_close_listener: Arc<Mutex<Option<OnCloseListener>>>,
    mut shutdown: watch::Receiver<()>,
) {
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_ok() {
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let peer_port = stream.peer_addr().map(|a| a.port()).unwrap_or(0);
                        let channel_id = {
                            let mut n = next_channel_id.lock().unwrap();
                            *n += 1;
                            *n
                        };
                        let inbox_tx = inbox_tx.clone();
                        let clients = Arc::clone(&clients);
                        let on_receive_listener = Arc::clone(&on_receive_listener);
                        let on_close_listener = Arc::clone(&on_close_listener);
                        tokio::spawn(handle_connection(
                            stream,
                            channel_id,
                            peer_port,
                            inbox_tx,
                            clients,
                            on_receive_listener,
                            on_close_listener,
                        ));
                    }
                    Err(err) => eprintln!("WebSocketServer: accept error: {err}"),
                }
            }
        }
    }
}

fn close_info_from_frame(frame: Option<CloseFrame>) -> (u16, String) {
    match frame {
        Some(frame) => (u16::from(frame.code), frame.reason.as_str().to_string()),
        None => (1000, String::new()),
    }
}

fn emit_on_close(
    channel_id: usize,
    peer_port: u16,
    close_code: u16,
    reason: String,
    listener: OnCloseListener,
) {
    let _ = WebSocketServerBuiltins::__boyia_emit_onClose(
        listener.ctx,
        listener.callback,
        move |vm| {
            let channel_arg =
                crate::runner::builtin_sync::push_callback_int(channel_id as i64, vm)?;
            let peer_arg = crate::runner::builtin_sync::push_callback_int(peer_port as i64, vm)?;
            let code_arg = crate::runner::builtin_sync::push_callback_int(close_code as i64, vm)?;
            let reason_arg = crate::runner::builtin_sync::push_callback_string(reason, vm)?;
            Some(vec![channel_arg, peer_arg, code_arg, reason_arg])
        },
    );
}

fn emit_on_receive(
    channel_id: usize,
    peer_port: u16,
    text: String,
    listener: OnReceiveListener,
) {
    let _ = WebSocketServerBuiltins::__boyia_emit_onReceive(
        listener.ctx,
        listener.callback,
        move |vm| {
            let channel_arg =
                crate::runner::builtin_sync::push_callback_int(channel_id as i64, vm)?;
            let peer_arg = crate::runner::builtin_sync::push_callback_int(peer_port as i64, vm)?;
            let msg_arg = crate::runner::builtin_sync::push_callback_string(text, vm)?;
            Some(vec![channel_arg, peer_arg, msg_arg])
        },
    );
}

async fn handle_connection(
    stream: TcpStream,
    channel_id: usize,
    peer_port: u16,
    inbox_tx: Sender<InboxMessage>,
    clients: Arc<Mutex<HashMap<usize, ClientTx>>>,
    on_receive_listener: Arc<Mutex<Option<OnReceiveListener>>>,
    on_close_listener: Arc<Mutex<Option<OnCloseListener>>>,
) {
    let mut ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(err) => {
            eprintln!("WebSocketServer: handshake error: {err}");
            return;
        }
    };

    let (client_tx, mut client_rx) = tokio_mpsc::unbounded_channel();
    clients.lock().unwrap().insert(channel_id, client_tx);

    // 1006 = abnormal closure when peer drops without a Close frame.
    let mut close_code: u16 = 1006;
    let mut close_reason = String::new();
    let mut close_handshake_done = false;

    loop {
        tokio::select! {
            incoming = ws.next() => {
                match incoming {
                    None => break,
                    Some(Ok(Message::Ping(data))) => {
                        if ws.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Text(text))) => {
                        let text = text.to_string();
                        enqueue_text(&inbox_tx, channel_id, peer_port, text.clone());
                        if let Some(listener) = on_receive_listener.lock().unwrap().clone() {
                            emit_on_receive(channel_id, peer_port, text, listener);
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            enqueue_text(&inbox_tx, channel_id, peer_port, text.clone());
                            if let Some(listener) = on_receive_listener.lock().unwrap().clone() {
                                emit_on_receive(channel_id, peer_port, text, listener);
                            }
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        (close_code, close_reason) = close_info_from_frame(frame.clone());
                        // Echo peer close frame to complete the handshake.
                        let _ = ws.send(Message::Close(frame)).await;
                        close_handshake_done = true;
                        break;
                    }
                    Some(Err(_)) => break,
                    Some(Ok(Message::Frame(_))) => {}
                }
            }
            outgoing = client_rx.recv() => {
                match outgoing {
                    None => break,
                    Some(msg) => {
                        if ws.send(msg).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }

    if !close_handshake_done {
        let _ = ws.close(None).await;
    } else {
        let _ = ws.flush().await;
    }

    if let Some(listener) = on_close_listener.lock().unwrap().clone() {
        emit_on_close(channel_id, peer_port, close_code, close_reason, listener);
    }

    clients.lock().unwrap().remove(&channel_id);
}

#[boyia_native_object(persistent_callbacks = ["onReceive", "onClose"])]
pub struct WebSocketServerBuiltins {
    #[boyia_field_default = "0.0.0.0"]
    host: String,
    #[boyia_field_default = "0"]
    port: u64,
    #[boyia_field_default = "false"]
    running: bool,
    #[boyia_field(skip)]
    runtime: Option<ServerRuntime>,
}

impl Drop for WebSocketServerBuiltins {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.set_on_receive_listener(None);
            runtime.set_on_close_listener(None);
        }
        self.__boyia_release_onReceive();
        self.__boyia_release_onClose();
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
        }
        self.running = false;
    }
}

#[boyia_class(name = "WebSocketServer", registrar = builtin_websocket_server_class)]
impl WebSocketServerBuiltins {
    /// Bind `host:port` and accept `ws://host:port/` clients on a background thread.
    #[boyia_sync_builtin(method = "start")]
    fn start(&mut self, host: String, port: u64) -> bool {
        if self.running {
            return false;
        }
        if port == 0 || port > u16::MAX as u64 {
            return false;
        }
        let host = normalize_host(&host);
        let Some(runtime) = ServerRuntime::start(host.clone(), port as u16) else {
            return false;
        };
        self.host = host;
        self.port = port;
        self.running = true;
        self.runtime = Some(runtime);
        true
    }

    #[boyia_sync_builtin(method = "stop")]
    fn stop(&mut self) -> bool {
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.set_on_receive_listener(None);
            runtime.set_on_close_listener(None);
        }
        self.__boyia_release_onReceive();
        self.__boyia_release_onClose();
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
        }
        self.running = false;
        true
    }

    #[boyia_sync_builtin(method = "isRunning")]
    fn is_running(&self) -> bool {
        self.running
    }

    #[boyia_sync_builtin(method = "getHost")]
    fn get_host(&self) -> String {
        self.host.clone()
    }

    #[boyia_sync_builtin(method = "getPort")]
    fn get_port(&self) -> u64 {
        self.port
    }

    #[boyia_sync_builtin(method = "clientCount")]
    fn client_count(&self) -> u64 {
        self.runtime
            .as_ref()
            .map(|rt| rt.clients.lock().unwrap().len() as u64)
            .unwrap_or(0)
    }

    /// Block until a text frame is received; returns `(channel_id, peerPort, message)`.
    #[boyia_sync_builtin(method = "receive")]
    fn receive(&self) -> (usize, u16, String) {
        let Some(runtime) = self.runtime.as_ref() else {
            return (0, 0, String::new());
        };
        runtime.recv_blocking()
    }

    /// Register a persistent callback: `(channel_id, peerPort, message)`.
    #[boyia_sync_builtin(method = "onReceive", callback = "persistent")]
    fn on_receive(&mut self) -> (usize, u16, String) {
        let Some(runtime) = self.runtime.as_ref() else {
            return (0, 0, String::new());
        };
        let Some((ctx, callback)) = self.__boyia_callback_onReceive() else {
            return (0, 0, String::new());
        };
        runtime.set_on_receive_listener(Some(OnReceiveListener { ctx, callback }));
        (0, 0, String::new())
    }

    #[boyia_sync_builtin(method = "offReceive")]
    fn off_receive(&mut self) -> bool {
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.set_on_receive_listener(None);
        }
        self.__boyia_release_onReceive();
        true
    }

    /// Persistent callback: `(channel_id, peerPort, closeCode, reason)`.
    #[boyia_sync_builtin(method = "onClose", callback = "persistent")]
    fn on_close(&mut self) -> (usize, u16, u16, String) {
        let Some(runtime) = self.runtime.as_ref() else {
            return (0, 0, 0, String::new());
        };
        let Some((ctx, callback)) = self.__boyia_callback_onClose() else {
            return (0, 0, 0, String::new());
        };
        runtime.set_on_close_listener(Some(OnCloseListener { ctx, callback }));
        (0, 0, 0, String::new())
    }

    #[boyia_sync_builtin(method = "offClose")]
    fn off_close(&mut self) -> bool {
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.set_on_close_listener(None);
        }
        self.__boyia_release_onClose();
        true
    }

    /// Send a text frame to the connection identified by `channel_id`.
    #[boyia_sync_builtin(method = "send")]
    fn send(&self, channel_id: usize, message: String) -> bool {
        let Some(runtime) = self.runtime.as_ref() else {
            return false;
        };
        runtime.push_text_to_client(channel_id, message)
    }

    /// Broadcast a text frame to every connected client.
    #[boyia_sync_builtin(method = "broadcast")]
    fn broadcast(&self, message: String) -> bool {
        let Some(runtime) = self.runtime.as_ref() else {
            return false;
        };
        runtime.push_text_to_all_clients(message)
    }
}
