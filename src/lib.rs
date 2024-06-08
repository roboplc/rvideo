#![ doc = include_str!( concat!( env!( "CARGO_MANIFEST_DIR" ), "/", "README.md" ) ) ]
use core::fmt;
use std::{sync::Arc, time::Duration};

use binrw::binrw;

mod client;
mod client_async;
mod server;
pub use client::Client;
pub use client_async::ClientAsync;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
pub use server::Server;
use server::StreamServerInner;
use tokio::net::ToSocketAddrs;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

static DEFAULT_SERVER: Lazy<Server> = Lazy::new(|| Server::new(DEFAULT_TIMEOUT));

pub fn add_stream(format: Format, width: u16, height: u16) -> Result<Stream, Error> {
    DEFAULT_SERVER.add_stream(format, width, height)
}

pub fn send_frame(stream_id: u16, frame: Frame) -> Result<(), Error> {
    DEFAULT_SERVER.send_frame(stream_id, frame)
}

pub async fn serve(addr: impl ToSocketAddrs + std::fmt::Debug) -> Result<(), Error> {
    DEFAULT_SERVER.serve(addr).await
}

/// # Panics
///
/// Will panic if tokio runtime is unable to start
pub fn run_server(addr: impl ToSocketAddrs + std::fmt::Debug) -> Result<(), Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(serve(addr))
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub metadata: Option<Arc<Vec<u8>>>,
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
    pub fn new(data: Arc<Vec<u8>>) -> Self {
        Self {
            metadata: None,
            data,
        }
    }
    pub fn new_with_metadata(metadata: Arc<Vec<u8>>, data: Arc<Vec<u8>>) -> Self {
        Self {
            metadata: Some(metadata),
            data,
        }
    }
}

pub const API_VERSION: u8 = 1;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid stream")]
    InvalidStream,
    #[error("Too many streams")]
    TooManyStreams,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unsupported API version: {0}")]
    ApiVersion(u8),
    #[error("Invalid binary data: {0}")]
    Decode(#[from] binrw::Error),
    #[error("Frame metadata too large")]
    FrameMetaDataTooLarge,
    #[error("Frame data too large")]
    FrameDataTooLarge,
    #[error("Invalid address")]
    InvalidAddress,
    #[error("Not ready")]
    NotReady,
    #[error("Timed out")]
    AsyncTimeout(#[from] tokio::time::error::Elapsed),
}

#[binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Format {
    Luma8 = 0,
    Luma16 = 1,
    LumaA8 = 2,
    LumaA16 = 3,
    Rgb8 = 4,
    Rgb16 = 5,
    Rgba8 = 6,
    Rgba16 = 7,
    MJpeg = 64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BoundingBox {
    #[serde(rename = "c")]
    pub color: [u8; 3],
    pub x: u16,
    pub y: u16,
    #[serde(rename = "w")]
    pub width: u16,
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

#[binrw]
#[brw(little)]
#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub id: u16,
    pub format: Format,
    pub width: u16,
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

#[derive(Clone)]
pub struct Stream {
    id: u16,
    server_inner: Arc<StreamServerInner>,
}

impl Stream {
    pub fn id(&self) -> u16 {
        self.id
    }
    pub fn send_frame(&self, frame: Frame) -> Result<(), Error> {
        self.server_inner.send_frame(self.id, frame)
    }
}
