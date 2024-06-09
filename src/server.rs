use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::{atomic, Arc},
    time::{Duration, Instant},
};

use async_channel::{Receiver, Sender};
use binrw::{BinRead, BinWrite};
use parking_lot::Mutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
};
use tracing::{debug, error};

const DEFAULT_MAX_CLIENTS: usize = 16;

use crate::{Error, Format, Frame, Greetings, Stream, StreamInfo, StreamSelect, API_VERSION};

struct StreamInternal {
    format: Format,
    width: u16,
    height: u16,
    clients: BTreeMap<usize, Sender<Frame>>,
}

/// A server instance. The crate creates a default server, however in some circumstances it might
/// be useful to create a custom one.
#[derive(Clone)]
pub struct Server {
    inner: Arc<StreamServerInner>,
}

impl Server {
    /// Create a new server with a given timeout
    pub fn new(timeout: Duration) -> Self {
        Self {
            inner: Arc::new(StreamServerInner {
                streams: <_>::default(),
                client_id: atomic::AtomicUsize::new(0),
                timeout,
                max_clients: atomic::AtomicUsize::new(DEFAULT_MAX_CLIENTS),
            }),
        }
    }
    /// Set the maximum number of clients that can connect to the server (default is 16)
    pub fn set_max_clients(&self, max_clients: usize) {
        self.inner
            .max_clients
            .store(max_clients, atomic::Ordering::Relaxed);
    }
    /// Add a stream to the server
    pub fn add_stream(&self, format: Format, width: u16, height: u16) -> Result<Stream, Error> {
        let stream_id = self.inner.add_stream(format, width, height)?;
        Ok(Stream {
            id: stream_id,
            server_inner: self.inner.clone(),
        })
    }
    /// Send frame to the server with stream id
    pub fn send_frame(&self, stream_id: u16, frame: Frame) -> Result<(), Error> {
        self.inner.send_frame(stream_id, frame)
    }
    /// Serve (requires a tokio runtime)
    pub async fn serve(&self, addr: impl ToSocketAddrs + std::fmt::Debug) -> Result<(), Error> {
        debug!(?addr, "starting server");
        let pool = simple_pool::ResourcePool::new();
        // move to a semaphore when robplc will be split
        for _ in 0..self.inner.max_clients.load(atomic::Ordering::Relaxed) {
            pool.append(());
        }
        let listener = tokio::net::TcpListener::bind(addr).await?;
        while let Ok((mut socket, addr)) = listener.accept().await {
            debug!(?addr, "new connection");
            let inner = self.inner.clone();
            let permission = pool.get().await;
            debug!(?addr, "handling connection");
            tokio::spawn(async move {
                let _permission = permission;
                let _r = inner.handle_connection(&mut socket).await;
            });
        }
        Ok(())
    }
}

pub(crate) struct StreamServerInner {
    streams: Mutex<Vec<StreamInternal>>,
    client_id: atomic::AtomicUsize,
    timeout: Duration,
    max_clients: atomic::AtomicUsize,
}

impl StreamServerInner {
    fn add_stream(&self, format: Format, width: u16, height: u16) -> Result<u16, Error> {
        debug!(?format, width, height, "adding stream");
        let mut streams = self.streams.lock();
        if streams.len() >= usize::from(u16::MAX) {
            return Err(Error::TooManyStreams);
        }
        let stream = StreamInternal {
            format,
            clients: <_>::default(),
            width,
            height,
        };
        streams.push(stream);
        let stream_id = u16::try_from(streams.len() - 1).unwrap();
        debug!(stream_id, ?format, width, height, "stream added");
        Ok(stream_id)
    }
    fn add_client(&self, stream_id: u16, client_id: usize) -> Result<Receiver<Frame>, Error> {
        debug!(stream_id, client_id, "adding client");
        let (tx, rx) = async_channel::bounded(1);
        if let Some(stream) = self.streams.lock().get_mut(usize::from(stream_id)) {
            stream.clients.insert(client_id, tx);
            debug!(stream_id, client_id, "client added");
            Ok(rx)
        } else {
            error!(stream_id, client_id, "client requested invalid stream");
            Err(Error::InvalidStream)
        }
    }
    fn remove_client(&self, stream_id: u16, client_id: usize) {
        debug!(stream_id, client_id, "removing client");
        if let Some(stream) = self.streams.lock().get_mut(usize::from(stream_id)) {
            stream.clients.remove(&client_id);
        }
    }
    fn stream_count(&self) -> usize {
        self.streams.lock().len()
    }
    pub(crate) fn send_frame(&self, stream_id: u16, frame: Frame) -> Result<(), Error> {
        debug!(stream_id, "sending frame");
        if frame
            .metadata
            .as_ref()
            .map_or(false, |v| v.len() > usize::try_from(u32::MAX).unwrap())
        {
            return Err(Error::FrameMetaDataTooLarge);
        }
        if frame.data.len() > usize::try_from(u32::MAX).unwrap() {
            return Err(Error::FrameDataTooLarge);
        }
        let clients = {
            let streams = self.streams.lock();
            if let Some(stream) = streams.get(usize::from(stream_id)) {
                stream
                    .clients
                    .values()
                    .cloned()
                    .collect::<Vec<Sender<Frame>>>()
            } else {
                return Err(Error::InvalidStream);
            }
        };
        for tx in clients {
            tx.try_send(frame.clone()).ok();
        }
        Ok(())
    }
    fn greetings(&self) -> Vec<u8> {
        let g = Greetings {
            api_version: API_VERSION,
            streams_available: u16::try_from(self.stream_count()).unwrap(),
        };
        let mut writer = Cursor::new(Vec::new());
        g.write(&mut writer).unwrap();
        writer.into_inner()
    }
    fn stream_info_packed(&self, stream_id: u16) -> Result<Vec<u8>, Error> {
        let streams = self.streams.lock();
        let Some(stream) = streams.get(usize::from(stream_id)) else {
            return Err(Error::InvalidStream);
        };
        let si = StreamInfo {
            id: stream_id,
            format: stream.format,
            width: stream.width,
            height: stream.height,
        };
        let mut writer = Cursor::new(Vec::new());
        si.write(&mut writer).unwrap();
        Ok(writer.into_inner())
    }
    async fn handle_connection(&self, socket: &mut TcpStream) -> Result<(), Error> {
        socket.set_nodelay(true)?;
        tokio::time::timeout(self.timeout, socket.write_all(&self.greetings())).await??;
        let stream_select_buf = &mut [0u8; 3];
        tokio::time::timeout(self.timeout, socket.read_exact(stream_select_buf)).await??;
        let stream_select = StreamSelect::read(&mut Cursor::new(stream_select_buf)).unwrap();
        let stram_info_packed = self.stream_info_packed(stream_select.stream_id)?;
        tokio::time::timeout(self.timeout, socket.write_all(&stram_info_packed)).await??;
        let client_id = self.client_id.fetch_add(1, atomic::Ordering::Relaxed);
        debug!(
            stream_id = stream_select.stream_id,
            max_fps = stream_select.max_fps,
            client_id,
            "stream connection established"
        );
        let min_time_between_frames: Duration =
            Duration::from_secs_f64(1.0 / f64::from(stream_select.max_fps));
        let rx = self.add_client(stream_select.stream_id, client_id)?;
        let mut last_frame = None;
        while let Ok(frame) = rx.recv().await {
            let now = Instant::now();
            if let Some(last_frame) = last_frame {
                let elapsed = now.duration_since(last_frame);
                if elapsed < min_time_between_frames {
                    continue;
                }
            }
            last_frame.replace(now);
            if self.write_frame(socket, frame).await.is_err() {
                self.remove_client(stream_select.stream_id, client_id);
            }
        }
        Ok(())
    }
    async fn write_frame(&self, socket: &mut TcpStream, frame: Frame) -> Result<(), Error> {
        let metadata_len = u32::try_from(frame.metadata.as_ref().map_or(0, |v| v.len())).unwrap();
        tokio::time::timeout(self.timeout, socket.write_u32_le(metadata_len)).await??;
        if let Some(ref metadata) = frame.metadata {
            tokio::time::timeout(self.timeout, socket.write_all(metadata)).await??;
        }
        tokio::time::timeout(
            self.timeout,
            socket.write_u32_le(u32::try_from(frame.data.len()).unwrap()),
        )
        .await??;
        tokio::time::timeout(self.timeout, socket.write_all(&frame.data)).await??;
        Ok(())
    }
}
