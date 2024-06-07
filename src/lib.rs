#![ doc = include_str!( concat!( env!( "CARGO_MANIFEST_DIR" ), "/", "README.md" ) ) ]
use std::sync::Arc;

use binrw::binrw;

mod client;
mod client_async;
mod server;
pub use client::Client;
pub use client_async::ClientAsync;
pub use server::Server;
use server::StreamServerInner;

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
pub enum PixelFormat {
    Luma8 = 1,
    Rgb8 = 2,
}

#[binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Compression {
    No = 0,
    Jpeg = 1,
    H264 = 2,
    H265 = 3,
}

#[binrw]
#[brw(little, magic = b"R")]
#[derive(Clone, Debug)]
pub struct Greetings {
    pub api_version: u8,
    pub streams_available: u16,
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug)]
pub struct StreamSelect {
    stream_id: u16,
    max_fps: u8,
}

#[binrw]
#[brw(little)]
#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub id: u16,
    pub pixel_format: PixelFormat,
    pub compression: Compression,
    pub width: u16,
    pub height: u16,
}

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
