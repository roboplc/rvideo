#![ doc = include_str!( concat!( env!( "CARGO_MANIFEST_DIR" ), "/", "README.md" ) ) ]
#![deny(missing_docs)]
use core::fmt;
use std::{sync::Arc, time::Duration};

use binrw::binrw;

mod client;
#[cfg(feature = "async")]
mod client_async;
mod semaphore;
mod server;
pub use client::Client;
#[cfg(feature = "async")]
pub use client_async::ClientAsync;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
pub use server::Server;
use server::StreamServerInner;
use std::net::ToSocketAddrs;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

static DEFAULT_SERVER: Lazy<Server> = Lazy::new(|| Server::new(DEFAULT_TIMEOUT));

/// Add a stream to the default server
pub fn add_stream(format: Format, width: u16, height: u16) -> Result<Stream, Error> {
    DEFAULT_SERVER.add_stream(format, width, height)
}

/// Send frame to the default server with stream id
pub fn send_frame(stream_id: u16, frame: Frame) -> Result<(), Error> {
    DEFAULT_SERVER.send_frame(stream_id, frame)
}

/// Serve the default server
pub fn serve(addr: impl ToSocketAddrs + std::fmt::Debug) -> Result<(), Error> {
    DEFAULT_SERVER.serve(addr)
}

/// Video frame
#[derive(Clone, Debug)]
pub struct Frame {
    /// An optional metadata (encoded in a way, known to remotes)
    pub metadata: Option<Arc<Vec<u8>>>,
    /// The frame data (encoded/compressed into the stream format)
    pub data: Arc<Vec<u8>>,
}

impl From<Vec<u8>> for Frame {
    fn from(data: Vec<u8>) -> Self {
        Self {
            metadata: None,
            data: data.into(),
        }
    }
}

impl From<Arc<Vec<u8>>> for Frame {
    fn from(data: Arc<Vec<u8>>) -> Self {
        Self {
            metadata: None,
            data,
        }
    }
}

impl Frame {
    /// Create a new frame with no metadata. Arc is used to avoid copying the data, as many video
    /// apps already cover their data with Arc.
    pub fn new(data: Arc<Vec<u8>>) -> Self {
        Self {
            metadata: None,
            data,
        }
    }
    /// Create a new frame with metadata. Arc is used to avoid copying the data, as many video apps
    /// already cover their data with Arc. The metadata should be encoded in a way, known to
    /// remotes
    pub fn new_with_metadata(metadata: Arc<Vec<u8>>, data: Arc<Vec<u8>>) -> Self {
        Self {
            metadata: Some(metadata),
            data,
        }
    }
}

/// Server API version
pub const API_VERSION: u8 = 1;

/// Error type
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Invalid stream (not known to the server)
    #[error("Invalid stream")]
    InvalidStream,
    /// Too many streams (max supported per server is u16::MAX)
    #[error("Too many streams")]
    TooManyStreams,
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported API version: {0}")]
    /// Unsupported API version
    ApiVersion(u8),
    /// Invalid data (binrw decode error)
    #[error("Invalid binary data: {0}")]
    Decode(#[from] binrw::Error),
    /// Frame metadata is larger than u32::MAX
    #[error("Frame metadata too large")]
    FrameMetaDataTooLarge,
    /// Frame data is larger than u32::MAX
    #[error("Frame data too large")]
    FrameDataTooLarge,
    /// Invalid TCP/IP address/host name/port
    #[error("Invalid address")]
    InvalidAddress,
    /// Client not ready (not connected/stream not selected)
    #[error("Not ready")]
    NotReady,
    /// Async timeouts
    #[error("Timed out")]
    #[cfg(feature = "async")]
    AsyncTimeout(#[from] tokio::time::error::Elapsed),
}

/// Video formats. Note: a frame should be MANUALLY encoded/compressed with the selected format
/// BEFORE sending
#[binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Format {
    /// 8-bit luma
    Luma8 = 0,
    /// 16-bit luma
    Luma16 = 1,
    /// 8-bit luma with alpha
    LumaA8 = 2,
    /// 16-bit luma with alpha
    LumaA16 = 3,
    /// 24-bit RGB
    Rgb8 = 4,
    /// 48-bit RGB
    Rgb16 = 5,
    /// 32-bit RGBA
    Rgba8 = 6,
    /// 64-bit RGBA
    Rgba16 = 7,
    /// Motion JPEG (JPEG frames can be encoded in any way)
    MJpeg = 64,
}

/// The default bounding box which can be used in custom applications. The bounding box format is
/// also recognized by [rvideo-view](https://crates.io/crates/rvideo-view).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BoundingBox {
    #[serde(rename = "c")]
    /// The color of the bounding box in RGB format
    pub color: [u8; 3],
    /// The x coordinate of the top-left corner
    pub x: u16,
    /// The y coordinate of the top-left corner
    pub y: u16,
    /// The width of the bounding box
    #[serde(rename = "w")]
    pub width: u16,
    /// The height of the bounding box
    #[serde(rename = "h")]
    pub height: u16,
}

#[binrw]
#[brw(little, magic = b"R")]
#[derive(Clone, Debug)]
struct Greetings {
    api_version: u8,
    streams_available: u16,
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug)]
struct StreamSelect {
    stream_id: u16,
    max_fps: u8,
}

/// Stream information
#[binrw]
#[brw(little)]
#[derive(Clone, Debug)]
pub struct StreamInfo {
    /// Stream id
    pub id: u16,
    /// Stream format
    pub format: Format,
    /// Picture width
    pub width: u16,
    /// Picture height
    pub height: u16,
}

impl fmt::Display for StreamInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{}, WxH: {}x{}, Fmt: {:?}",
            self.id, self.width, self.height, self.format
        )
    }
}

/// A stream helper object. Contains a stream id and a reference to the server inner object
#[derive(Clone)]
pub struct Stream {
    id: u16,
    server_inner: Arc<StreamServerInner>,
}

impl Stream {
    /// Get the stream id
    pub fn id(&self) -> u16 {
        self.id
    }
    /// Send a frame to the stream
    pub fn send_frame(&self, frame: Frame) -> Result<(), Error> {
        self.server_inner.send_frame(self.id, frame)
    }
}
