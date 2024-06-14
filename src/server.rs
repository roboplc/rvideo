use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::{atomic, Arc},
    thread,
    time::{Duration, Instant},
};

use binrw::{BinRead, BinWrite};
use parking_lot_rt::{Condvar, Mutex};
use tracing::{error, trace};

const DEFAULT_MAX_CLIENTS: usize = 16;

use crate::{Error, Format, Frame, Greetings, Stream, StreamInfo, StreamSelect, API_VERSION};

#[derive(Default)]
struct FrameValue {
    current: Option<Frame>,
    closed: bool,
}

#[derive(Default)]
struct FrameCellInner {
    value: Mutex<FrameValue>,
    data_available: Condvar,
}

#[derive(Default)]
struct FrameCell {
    inner: Arc<FrameCellInner>,
}

impl Clone for FrameCell {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl FrameCell {
    fn close(&self) {
        let mut value = self.inner.value.lock();
        value.closed = true;
        self.inner.data_available.notify_all();
    }
    fn set(&self, frame: Frame) {
        let mut value = self.inner.value.lock();
        value.current = Some(frame);
        self.inner.data_available.notify_one();
    }
    fn get(&self) -> Option<Frame> {
        let mut value = self.inner.value.lock();
        if value.closed {
            return None;
        }
        loop {
            if let Some(current) = value.current.take() {
                return Some(current);
            }
            self.inner.data_available.wait(&mut value);
        }
    }
}

impl Iterator for FrameCell {
    type Item = Frame;
    fn next(&mut self) -> Option<Self::Item> {
        self.get()
    }
}

struct StreamInternal {
    format: Format,
    width: u16,
    height: u16,
    clients: BTreeMap<usize, FrameCell>,
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
    pub fn serve(&self, addr: impl ToSocketAddrs + std::fmt::Debug) -> Result<(), Error> {
        trace!(?addr, "starting server");
        let semaphore = crate::semaphore::Semaphore::new(
            self.inner.max_clients.load(atomic::Ordering::Relaxed),
        );
        let listener = TcpListener::bind(addr)?;
        while let Ok((mut socket, addr)) = listener.accept() {
            trace!(?addr, "new connection");
            let inner = self.inner.clone();
            let permission = semaphore.acquire();
            trace!(?addr, "handling connection");
            thread::spawn(move || {
                let _permission = permission;
                let _r = inner.handle_connection(&mut socket);
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

impl Drop for StreamServerInner {
    fn drop(&mut self) {
        for stream in &*self.streams.lock() {
            for cell in stream.clients.values() {
                cell.close();
            }
        }
    }
}

impl StreamServerInner {
    fn add_stream(&self, format: Format, width: u16, height: u16) -> Result<u16, Error> {
        trace!(?format, width, height, "adding stream");
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
        trace!(stream_id, ?format, width, height, "stream added");
        Ok(stream_id)
    }
    fn add_client(&self, stream_id: u16, client_id: usize) -> Result<FrameCell, Error> {
        trace!(stream_id, client_id, "adding client");
        let frame_cell = FrameCell::default();
        if let Some(stream) = self.streams.lock().get_mut(usize::from(stream_id)) {
            stream.clients.insert(client_id, frame_cell.clone());
            trace!(stream_id, client_id, "client added");
            Ok(frame_cell)
        } else {
            error!(stream_id, client_id, "client requested invalid stream");
            Err(Error::InvalidStream)
        }
    }
    fn remove_client(&self, stream_id: u16, client_id: usize) {
        trace!(stream_id, client_id, "removing client");
        if let Some(stream) = self.streams.lock().get_mut(usize::from(stream_id)) {
            stream.clients.remove(&client_id);
        }
    }
    fn stream_count(&self) -> usize {
        self.streams.lock().len()
    }
    pub(crate) fn send_frame(&self, stream_id: u16, frame: Frame) -> Result<(), Error> {
        trace!(stream_id, "sending frame");
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
                stream.clients.values().cloned().collect::<Vec<FrameCell>>()
            } else {
                return Err(Error::InvalidStream);
            }
        };
        for tx in clients {
            tx.set(frame.clone());
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
    fn handle_connection(&self, socket: &mut TcpStream) -> Result<(), Error> {
        socket.set_nodelay(true)?;
        socket.set_read_timeout(Some(self.timeout))?;
        socket.set_write_timeout(Some(self.timeout))?;
        socket.write_all(&self.greetings())?;
        let stream_select_buf = &mut [0u8; 3];
        socket.read_exact(stream_select_buf)?;
        let stream_select = StreamSelect::read(&mut Cursor::new(stream_select_buf)).unwrap();
        let stram_info_packed = self.stream_info_packed(stream_select.stream_id)?;
        socket.write_all(&stram_info_packed)?;
        let client_id = self.client_id.fetch_add(1, atomic::Ordering::Relaxed);
        trace!(
            stream_id = stream_select.stream_id,
            max_fps = stream_select.max_fps,
            client_id,
            "stream connection established"
        );
        let min_time_between_frames: Duration =
            Duration::from_secs_f64(1.0 / f64::from(stream_select.max_fps));
        let rx = self.add_client(stream_select.stream_id, client_id)?;
        let mut last_frame = None;
        for frame in rx {
            let now = Instant::now();
            if let Some(last_frame) = last_frame {
                let elapsed = now.duration_since(last_frame);
                if elapsed < min_time_between_frames {
                    continue;
                }
            }
            last_frame.replace(now);
            if Self::write_frame(socket, frame).is_err() {
                self.remove_client(stream_select.stream_id, client_id);
                break;
            }
        }
        Ok(())
    }
    fn write_frame(socket: &mut TcpStream, frame: Frame) -> Result<(), Error> {
        let metadata_len = u32::try_from(frame.metadata.as_ref().map_or(0, |v| v.len())).unwrap();
        socket.write_all(&metadata_len.to_le_bytes())?;
        if let Some(ref metadata) = frame.metadata {
            socket.write_all(metadata)?;
        }
        let data_len = u32::try_from(frame.data.len()).unwrap();
        socket.write_all(&data_len.to_le_bytes())?;
        socket.write_all(&frame.data)?;
        Ok(())
    }
}
