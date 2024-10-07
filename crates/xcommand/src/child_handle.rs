use crate::StdioType;
use crate::XStatus;
use async_stream::stream;
use eyre::bail;
use eyre::Result;
use futures::pin_mut;
use futures::TryStreamExt;
use log::debug;
use log::error;
use log::info;
use nix::sys::wait::WaitPidFlag;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{close, fork, Pid};
use std::pin::Pin;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::sync::oneshot;
use tokio::sync::oneshot::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio_fd::AsyncFd;
use tokio_stream::wrappers::LinesStream;
use tokio_stream::{Stream, StreamExt, StreamMap};

#[derive(Debug)]
pub struct XStreamer {
    pid: Pid,
    stdout: i32,
    stderr: i32,
    tx: Mutex<Option<Sender<XStatus>>>,
}

impl XStreamer {
    async fn send_status(&mut self, status: WaitStatus) {
        let sender = {
            let mut tx_lock = self.tx.lock().await;
            tx_lock.take()
        };
        match sender {
            Some(tx) => {
                if tx.send(status.into()).is_err() {
                    panic!("Unable to send");
                };
            }
            None => {
                panic!("bad bad bad");
            }
        }
    }

    fn _stream(&mut self) -> impl Stream<Item = Result<(StdioType, String)>> + '_ {
        stream! {
            let pid = self.pid;

            let mut join = tokio::task::spawn(async move {
                let Ok(status) = waitpid(pid, None) else {
                    panic!("Error waiting for child to complete");
                };
                status
            });

            let stdout = AsyncFd::try_from(self.stdout).unwrap();
            let stderr = AsyncFd::try_from(self.stderr).unwrap();
            let mut stdout_reader = LinesStream::new(BufReader::new(stdout).lines());
            let mut stderr_reader = LinesStream::new(BufReader::new(stderr).lines());

            let stdout_stream = Box::pin(stream! {
                while let Some(Ok(item)) = stdout_reader.next().await {
                    yield item;
                }
            })
                as Pin<Box<dyn Stream<Item = String> + Send>>;

            let stderr_stream = Box::pin(stream! {
                while let Some(Ok(item)) = stderr_reader.next().await {
                    yield item;
                }
            })
                as Pin<Box<dyn Stream<Item = String> + Send>>;

            let mut map = StreamMap::with_capacity(2);
            map.insert(StdioType::Stdout, stdout_stream);
            map.insert(StdioType::Stderr, stderr_stream);

            loop {
                tokio::select! {
                    // Force polling in listed order instead of randomly. This prevents us from
                    // deadlocking when the command exits. - TODO: this might not be needed anymore
                    biased;
                    Some(output) = map.next() => {
                        yield Ok(output);
                    },
                    status = &mut join => {
                        let status = status.unwrap(); // TODO: handle unwrap

                        // Pick up any final output that was written in the time it took us to check
                        // this 'select!' branch
                        while let Some(output) = map.next().await {
                            yield Ok(output);
                        }

                        close(self.stdout).unwrap();
                        close(self.stderr).unwrap();

                        // TODO: do we need to handle any other WaitStatus variants?
                        // https://docs.rs/nix/latest/nix/sys/wait/enum.WaitStatus.html


                        match status {
                            WaitStatus::Exited(..) | WaitStatus::Signaled(..) | WaitStatus::Stopped(..) | WaitStatus::PtraceEvent(..) | WaitStatus::PtraceSyscall(..) | WaitStatus::Continued(..) => {
                                // Send status code back to child handle
                                self.send_status(status).await;
                                return;
                            },
                            WaitStatus::StillAlive => {
                                panic!("Child process in unexpected state: '{:?}'", status);
                            },
                        }
                    },
                }
            }
        }
    }

    pub fn stream(&mut self) -> impl Stream<Item = Result<(StdioType, String)>> + '_ {
        let stream = self._stream();
        Box::pin(stream)
    }
}

#[derive(Debug)]
enum StatusWrapper {
    Init(XStatus),
    Received(XStatus),
}

#[derive(Debug)]
pub struct XChildHandle {
    pid: Pid,
    /// Raw file descriptor
    stdout: i32,
    /// Raw file descriptor
    stderr: i32,

    rx: Mutex<Option<Receiver<XStatus>>>,
    status: StatusWrapper,
}

impl XChildHandle {
    pub fn streamer(&mut self) -> XStreamer {
        let (tx, rx) = oneshot::channel();
        self.rx = Mutex::new(Some(rx));
        XStreamer {
            pid: self.pid,
            stdout: self.stdout,
            stderr: self.stderr,
            tx: Mutex::new(Some(tx)),
        }
    }

    // TODO: I don't want this to be pub, but XCommand might need to call it
    pub fn new(pid: Pid, stdout: i32, stderr: i32) -> Result<Self> {
        let status = StatusWrapper::Init(XStatus::Running);
        Ok(XChildHandle {
            pid,
            stdout,
            stderr,
            status,
            rx: Mutex::new(None),
        })
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub async fn status(&mut self) -> Result<XStatus> {
        match &self.status {
            StatusWrapper::Init(initial_status) => {
                let receiver = {
                    let mut rx_opt = self.rx.lock().await;
                    rx_opt.take()
                };

                match receiver {
                    Some(rx) => match rx.await {
                        Ok(status) => {
                            self.status = StatusWrapper::Received(status.clone());
                            Ok(status)
                        }
                        Err(e) => bail!("Unable to get status: {:?}", e),
                    },
                    None => {
                        bail!("todo: error message");
                    }
                }
            }

            StatusWrapper::Received(status) => Ok(status.clone()),
        }
    }
}
