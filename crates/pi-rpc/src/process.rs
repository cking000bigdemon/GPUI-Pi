//! pi RPC 子进程监督与线程式客户端。

use std::{
    collections::HashMap,
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command as ProcessCommand, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::Value;
use thiserror::Error;

use crate::{
    jsonl::{JsonlError, JsonlFramer},
    protocol::{Command, ExtensionUiResponse, RpcEvent, RpcRequest, RpcResponse, RpcSessionState},
};

const POLL_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub binary: PathBuf,
    pub current_dir: Option<PathBuf>,
    /// 首次启动即恢复此会话；不能先创建空会话再在启动后补设恢复目标。
    pub initial_session: Option<PathBuf>,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub max_restarts: usize,
    pub restart_window: Duration,
    pub restart_delay: Duration,
    pub shutdown_grace_period: Duration,
    pub max_frame_len: usize,
}

impl ClientConfig {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            current_dir: None,
            initial_session: None,
            args: Vec::new(),
            env: Vec::new(),
            max_restarts: 3,
            restart_window: Duration::from_secs(30),
            restart_delay: Duration::from_millis(100),
            shutdown_grace_period: Duration::from_secs(2),
            max_frame_len: crate::jsonl::DEFAULT_MAX_FRAME_LEN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    Started {
        pid: u32,
        resumed_session: Option<PathBuf>,
    },
    Exited {
        pid: u32,
        code: Option<i32>,
        success: bool,
    },
    Restarting {
        attempt: usize,
        session_file: Option<PathBuf>,
    },
    Restarted {
        pid: u32,
        session_file: Option<PathBuf>,
    },
    RestartFailed {
        error: String,
    },
    Stderr {
        line: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent {
    Rpc(Box<RpcEvent>),
    Unknown(Value),
    Lifecycle(LifecycleEvent),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ClientError {
    #[error("failed to spawn pi: {0}")]
    Spawn(String),
    #[error("pi RPC process is not running")]
    NotRunning,
    #[error("failed to write pi stdin: {0}")]
    Write(String),
    #[error("request {id} timed out")]
    Timeout { id: String },
    #[error("request {id} failed because pi exited")]
    ProcessExited { id: String },
    #[error("RPC command {command} failed: {message}")]
    Rpc { command: String, message: String },
    #[error("invalid RPC response data: {0}")]
    Decode(String),
    #[error("pi RPC supervisor stopped: {0}")]
    Supervisor(String),
}

struct PendingRequest {
    tx: Sender<Result<RpcResponse, ClientError>>,
}

struct Shared {
    writer: Mutex<Option<ChildStdin>>,
    pending: Mutex<HashMap<String, PendingRequest>>,
    subscribers: Mutex<Vec<Sender<ClientEvent>>>,
    shutdown: AtomicBool,
    next_id: AtomicU64,
    pid: AtomicU64,
    resume_session: Mutex<Option<PathBuf>>,
}

/// 可 clone 的同步客户端。每个 blocking request 只阻塞调用线程；stdout/stderr/监督各自独立线程。
#[derive(Clone)]
pub struct Client {
    shared: Arc<Shared>,
    supervisor: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Client {
    pub fn spawn(config: ClientConfig) -> Result<Self, ClientError> {
        let initial_session = config.initial_session.clone();
        let shared = Arc::new(Shared {
            writer: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            subscribers: Mutex::new(Vec::new()),
            shutdown: AtomicBool::new(false),
            next_id: AtomicU64::new(0),
            pid: AtomicU64::new(0),
            resume_session: Mutex::new(initial_session),
        });
        let (start_tx, start_rx) = mpsc::sync_channel(1);
        let thread_shared = Arc::clone(&shared);
        let supervisor = thread::Builder::new()
            .name("pi-rpc-supervisor".into())
            .spawn(move || supervise(config, thread_shared, start_tx))
            .map_err(|error| ClientError::Spawn(error.to_string()))?;

        match start_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                shared,
                supervisor: Arc::new(Mutex::new(Some(supervisor))),
            }),
            Ok(Err(error)) => {
                let _ = supervisor.join();
                Err(error)
            }
            Err(error) => {
                let _ = supervisor.join();
                Err(ClientError::Supervisor(error.to_string()))
            }
        }
    }

    /// 订阅事件流。stdout reader 与 reducer pump 分线程消费，慢 UI 不会让订阅在 burst
    /// 中被静默永久断开；上层必须持续 drain 并按帧合并事件。
    pub fn subscribe(&self) -> Receiver<ClientEvent> {
        let (tx, rx) = mpsc::channel();
        self.shared.subscribers.lock().unwrap().push(tx);
        rx
    }

    /// 当前活跃进程退出后，监督器用于自动恢复的会话文件。
    ///
    /// `new_session` / `switch_session` 成功后，上层应立即用新会话路径调用本方法；
    /// 成功的 `get_state` 响应也会自动更新该值。
    pub fn set_resume_session(&self, session_file: Option<PathBuf>) {
        *self.shared.resume_session.lock().unwrap() = session_file;
    }

    pub fn resume_session(&self) -> Option<PathBuf> {
        self.shared.resume_session.lock().unwrap().clone()
    }

    pub fn pid(&self) -> Option<u32> {
        u32::try_from(self.shared.pid.load(Ordering::Acquire))
            .ok()
            .filter(|pid| *pid != 0)
    }

    pub fn request(&self, command: Command, timeout: Duration) -> Result<RpcResponse, ClientError> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(ClientError::NotRunning);
        }
        let id = format!(
            "req_{}",
            self.shared.next_id.fetch_add(1, Ordering::Relaxed) + 1
        );
        let request = RpcRequest {
            id: Some(id.clone()),
            command,
        };
        let (tx, rx) = mpsc::channel();
        self.shared
            .pending
            .lock()
            .unwrap()
            .insert(id.clone(), PendingRequest { tx });
        if let Err(error) = write_json(&self.shared, &request) {
            self.shared.pending.lock().unwrap().remove(&id);
            return Err(error);
        }
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.shared.pending.lock().unwrap().remove(&id);
                Err(ClientError::Timeout { id })
            }
            Err(RecvTimeoutError::Disconnected) => Err(ClientError::ProcessExited { id }),
        }
    }

    pub fn request_data<T: for<'de> serde::Deserialize<'de>>(
        &self,
        command: Command,
        timeout: Duration,
    ) -> Result<T, ClientError> {
        let response = self.request(command, timeout)?;
        if !response.success {
            return Err(ClientError::Rpc {
                command: response.command,
                message: response.error.unwrap_or_else(|| "unknown RPC error".into()),
            });
        }
        response
            .decode_data()
            .map_err(|error| ClientError::Decode(error.to_string()))
    }

    pub fn send_extension_ui_response(
        &self,
        response: &ExtensionUiResponse,
    ) -> Result<(), ClientError> {
        write_json(&self.shared, response)
    }

    /// 主动 shutdown：先关闭 stdin 允许 pi 正常 dispose，监督线程不会自动重启。
    pub fn shutdown(&self) -> Result<(), ClientError> {
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.writer.lock().unwrap().take();
        if let Some(handle) = self.supervisor.lock().unwrap().take() {
            handle
                .join()
                .map_err(|_| ClientError::Supervisor("supervisor thread panicked".into()))?;
        }
        Ok(())
    }

    /// 进程树强杀，供用户显式取消或测试外部故障。未调用 shutdown 时会触发自动重启。
    pub fn kill_process_tree(&self) -> Result<(), ClientError> {
        let pid = self.pid().ok_or(ClientError::NotRunning)?;
        kill_process_tree(pid).map_err(|error| ClientError::Supervisor(error.to_string()))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if Arc::strong_count(&self.supervisor) == 1 {
            let _ = self.shutdown();
        }
    }
}

fn write_json<T: serde::Serialize>(shared: &Shared, value: &T) -> Result<(), ClientError> {
    let mut line =
        serde_json::to_vec(value).map_err(|error| ClientError::Write(error.to_string()))?;
    line.push(b'\n');
    let mut writer = shared.writer.lock().unwrap();
    let stdin = writer.as_mut().ok_or(ClientError::NotRunning)?;
    stdin
        .write_all(&line)
        .and_then(|()| stdin.flush())
        .map_err(|error| ClientError::Write(error.to_string()))
}

fn supervise(
    config: ClientConfig,
    shared: Arc<Shared>,
    start_tx: mpsc::SyncSender<Result<(), ClientError>>,
) {
    let mut first_start = Some(start_tx);
    let mut restart_times = Vec::new();
    let mut restarting = false;

    loop {
        if shared.shutdown.load(Ordering::Acquire) {
            return;
        }
        let resume_session = shared.resume_session.lock().unwrap().clone();
        let spawned = spawn_child(&config, resume_session.as_deref());
        let (mut child, stdout, stderr, stdin) = match spawned {
            Ok(parts) => parts,
            Err(error) => {
                if let Some(tx) = first_start.take() {
                    let _ = tx.send(Err(error));
                } else {
                    broadcast(
                        &shared,
                        ClientEvent::Lifecycle(LifecycleEvent::RestartFailed {
                            error: error.to_string(),
                        }),
                    );
                }
                fail_all_pending(&shared);
                return;
            }
        };
        let pid = child.id();
        shared.pid.store(u64::from(pid), Ordering::Release);
        *shared.writer.lock().unwrap() = Some(stdin);
        let (io_tx, io_rx) = mpsc::channel();
        let stdout_handle = spawn_stdout_reader(stdout, config.max_frame_len, io_tx.clone());
        let stderr_handle = spawn_stderr_reader(stderr, io_tx.clone());

        if let Some(tx) = first_start.take() {
            broadcast(
                &shared,
                ClientEvent::Lifecycle(LifecycleEvent::Started {
                    pid,
                    resumed_session: resume_session.clone(),
                }),
            );
            let _ = tx.send(Ok(()));
        } else if restarting {
            broadcast(
                &shared,
                ClientEvent::Lifecycle(LifecycleEvent::Restarted {
                    pid,
                    session_file: resume_session.clone(),
                }),
            );
        }

        let mut shutdown_started = None;
        let mut next_shutdown_kill = None;
        let status = loop {
            while let Ok(message) = io_rx.try_recv() {
                handle_io_message(&shared, message);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if shared.shutdown.load(Ordering::Acquire) {
                        shared.writer.lock().unwrap().take();
                        let started = shutdown_started.get_or_insert_with(Instant::now);
                        let now = Instant::now();
                        if started.elapsed() >= config.shutdown_grace_period
                            && next_shutdown_kill.is_none_or(|next| now >= next)
                            && let Err(error) = kill_process_tree(pid)
                        {
                            broadcast(
                                &shared,
                                ClientEvent::Lifecycle(LifecycleEvent::RestartFailed {
                                    error: format!("shutdown process-tree kill failed: {error}"),
                                }),
                            );
                            next_shutdown_kill = Some(now + Duration::from_millis(100));
                        }
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                Err(error) => {
                    broadcast(
                        &shared,
                        ClientEvent::Lifecycle(LifecycleEvent::RestartFailed {
                            error: error.to_string(),
                        }),
                    );
                    let _ = kill_process_tree(pid);
                    match child.wait() {
                        Ok(status) => break status,
                        Err(_) => return,
                    }
                }
            }
        };

        *shared.writer.lock().unwrap() = None;
        while let Ok(message) = io_rx.try_recv() {
            handle_io_message(&shared, message);
        }
        let _ = stdout_handle.join();
        let _ = stderr_handle.join();
        while let Ok(message) = io_rx.try_recv() {
            handle_io_message(&shared, message);
        }
        shared.pid.store(0, Ordering::Release);
        broadcast(
            &shared,
            ClientEvent::Lifecycle(LifecycleEvent::Exited {
                pid,
                code: status.code(),
                success: status.success(),
            }),
        );
        fail_all_pending(&shared);

        if shared.shutdown.load(Ordering::Acquire) {
            return;
        }

        let now = Instant::now();
        restart_times.retain(|time| now.duration_since(*time) <= config.restart_window);
        if restart_times.len() >= config.max_restarts {
            broadcast(
                &shared,
                ClientEvent::Lifecycle(LifecycleEvent::RestartFailed {
                    error: format!(
                        "restart limit {} reached within {:?}",
                        config.max_restarts, config.restart_window
                    ),
                }),
            );
            return;
        }
        restart_times.push(now);
        restarting = true;
        let restart_session = shared.resume_session.lock().unwrap().clone();
        broadcast(
            &shared,
            ClientEvent::Lifecycle(LifecycleEvent::Restarting {
                attempt: restart_times.len(),
                session_file: restart_session,
            }),
        );
        thread::sleep(config.restart_delay);
    }
}

type SpawnedChild = (Child, ChildStdout, ChildStderr, ChildStdin);

fn spawn_child(
    config: &ClientConfig,
    session_file: Option<&Path>,
) -> Result<SpawnedChild, ClientError> {
    let mut command = ProcessCommand::new(&config.binary);
    command.args(["--mode", "rpc"]);
    command.args(&config.args);
    command.envs(config.env.iter().cloned());
    if let Some(session_file) = session_file {
        command.arg("--session").arg(session_file);
    }
    if let Some(current_dir) = &config.current_dir {
        command.current_dir(current_dir);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command
        .spawn()
        .map_err(|error| ClientError::Spawn(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ClientError::Spawn("missing stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ClientError::Spawn("missing stderr pipe".into()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| ClientError::Spawn("missing stdin pipe".into()))?;
    Ok((child, stdout, stderr, stdin))
}

#[derive(Debug)]
enum IoMessage {
    Stdout(Result<Value, String>),
    Stderr(String),
}

fn spawn_stdout_reader(
    mut stdout: ChildStdout,
    max_frame_len: usize,
    tx: Sender<IoMessage>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("pi-rpc-stdout".into())
        .spawn(move || {
            let mut framer = JsonlFramer::new(max_frame_len);
            let mut chunk = [0_u8; 8192];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => {
                        if let Ok(Some(frame)) = framer.finish() {
                            send_stdout_frame(&tx, frame);
                        }
                        return;
                    }
                    Ok(read) => match framer.push(&chunk[..read]) {
                        Ok(frames) => {
                            for frame in frames {
                                send_stdout_frame(&tx, frame);
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(IoMessage::Stdout(Err(error.to_string())));
                            return;
                        }
                    },
                    Err(error) => {
                        let _ = tx.send(IoMessage::Stdout(Err(error.to_string())));
                        return;
                    }
                }
            }
        })
        .expect("failed to spawn stdout reader")
}

fn send_stdout_frame(tx: &Sender<IoMessage>, frame: Vec<u8>) {
    if frame.is_empty() {
        return;
    }
    let parsed = serde_json::from_slice(&frame).map_err(|error| error.to_string());
    let _ = tx.send(IoMessage::Stdout(parsed));
}

fn spawn_stderr_reader(mut stderr: ChildStderr, tx: Sender<IoMessage>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("pi-rpc-stderr".into())
        .spawn(move || {
            let mut framer = JsonlFramer::new(crate::jsonl::DEFAULT_MAX_FRAME_LEN);
            let mut chunk = [0_u8; 4096];
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) => {
                        if let Ok(Some(frame)) = framer.finish() {
                            let _ = tx.send(IoMessage::Stderr(
                                String::from_utf8_lossy(&frame).into_owned(),
                            ));
                        }
                        return;
                    }
                    Ok(read) => match framer.push(&chunk[..read]) {
                        Ok(frames) => {
                            for frame in frames {
                                let _ = tx.send(IoMessage::Stderr(
                                    String::from_utf8_lossy(&frame).into_owned(),
                                ));
                            }
                        }
                        Err(JsonlError::FrameTooLarge { .. }) => return,
                    },
                    Err(_) => return,
                }
            }
        })
        .expect("failed to spawn stderr reader")
}

fn handle_io_message(shared: &Shared, message: IoMessage) {
    match message {
        IoMessage::Stderr(line) => broadcast(
            shared,
            ClientEvent::Lifecycle(LifecycleEvent::Stderr { line }),
        ),
        IoMessage::Stdout(Err(error)) => broadcast(
            shared,
            ClientEvent::Unknown(Value::String(format!("invalid stdout JSON: {error}"))),
        ),
        IoMessage::Stdout(Ok(value)) => {
            if value.get("type").and_then(Value::as_str) == Some("response") {
                match serde_json::from_value::<RpcResponse>(value.clone()) {
                    Ok(response) => {
                        if response.success
                            && response.command == "get_state"
                            && let Ok(state) = response.decode_data::<RpcSessionState>()
                        {
                            *shared.resume_session.lock().unwrap() =
                                state.session_file.map(PathBuf::from);
                        }
                        if let Some(id) = response.id.clone()
                            && let Some(pending) = shared.pending.lock().unwrap().remove(&id)
                        {
                            let _ = pending.tx.send(Ok(response));
                            return;
                        }
                        broadcast(shared, ClientEvent::Unknown(value));
                    }
                    Err(_) => broadcast(shared, ClientEvent::Unknown(value)),
                }
            } else {
                match serde_json::from_value::<RpcEvent>(value.clone()) {
                    Ok(event) => broadcast(shared, ClientEvent::Rpc(Box::new(event))),
                    Err(_) => broadcast(shared, ClientEvent::Unknown(value)),
                }
            }
        }
    }
}

fn fail_all_pending(shared: &Shared) {
    let pending = std::mem::take(&mut *shared.pending.lock().unwrap());
    for (id, request) in pending {
        let _ = request.tx.send(Err(ClientError::ProcessExited { id }));
    }
}

fn broadcast(shared: &Shared, event: ClientEvent) {
    shared
        .subscribers
        .lock()
        .unwrap()
        .retain(|subscriber| subscriber.send(event.clone()).is_ok());
}

#[cfg(windows)]
pub fn kill_process_tree(pid: u32) -> std::io::Result<()> {
    let status = ProcessCommand::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "taskkill exited with {status}"
        )))
    }
}

#[cfg(unix)]
pub fn kill_process_tree(pid: u32) -> std::io::Result<()> {
    let status = ProcessCommand::new("kill")
        // `--` 避免 procps-ng kill 把负 PGID 误解为旧式 signal 参数。
        .args(["-TERM", "--", &format!("-{pid}")])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("kill exited with {status}")))
    }
}
