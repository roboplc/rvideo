use std::{
    io::{Cursor, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use binrw::BinRead;

use crate::{Error, Frame, Greetings, StreamInfo, StreamSelect};

pub struct Client {
    stream: TcpStream,
    streams_available: u16,
    ready: bool,
}

impl Client {
    pub fn connect(addr: impl ToSocketAddrs, timeout: Duration) -> Result<Self, Error> {
        let mut stream = TcpStream::connect_timeout(
            &addr
                .to_socket_addrs()?
                .next()
                .ok_or(Error::InvalidAddress)?,
            timeout,
        )?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf)?;
        let greetings = Greetings::read(&mut Cursor::new(&buf))?;
        if greetings.api_version != crate::API_VERSION {
            return Err(Error::ApiVersion(greetings.api_version));
        }
        Ok(Self {
            stream,
            streams_available: greetings.streams_available,
            ready: false,
        })
    }
    pub fn streams_available(&self) -> u16 {
        self.streams_available
    }
    pub fn select_stream(&mut self, stream_id: u16, max_fps: u8) -> Result<StreamInfo, Error> {
        let stream_select = StreamSelect { stream_id, max_fps };
        let mut writer = Cursor::new(Vec::new());
        binrw::BinWrite::write(&stream_select, &mut writer)?;
        self.stream.write_all(&writer.into_inner())?;
        let mut buf = [0u8; 8];
        self.stream.read_exact(&mut buf)?;
        let stream_info = StreamInfo::read(&mut Cursor::new(&buf))?;
        if stream_info.id == stream_id {
            self.ready = true;
            Ok(stream_info)
        } else {
            Err(Error::InvalidStream)
        }
    }
}

impl Iterator for Client {
    type Item = Result<Frame, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if !self.ready {
            return Some(Err(Error::NotReady));
        }
        let mut len_buf = [0u8; 4];
        if let Err(e) = self.stream.read_exact(&mut len_buf) {
            return Some(Err(e.into()));
        }
        let Ok(len) = usize::try_from(u32::from_le_bytes(len_buf)) else {
            return Some(Err(Error::FrameMetaDataTooLarge));
        };
        let metadata = if len > 0 {
            let mut buf = vec![0u8; len];
            if let Err(e) = self.stream.read_exact(&mut buf) {
                return Some(Err(e.into()));
            }
            Some(buf)
        } else {
            None
        };
        if let Err(e) = self.stream.read_exact(&mut len_buf) {
            return Some(Err(e.into()));
        }
        let Ok(len) = usize::try_from(u32::from_le_bytes(len_buf)) else {
            return Some(Err(Error::FrameDataTooLarge));
        };
        let mut data = vec![0u8; len];
        if let Err(e) = self.stream.read_exact(&mut data) {
            return Some(Err(e.into()));
        }
        Some(Ok(Frame {
            metadata: metadata.map(Into::into),
            data: data.into(),
        }))
    }
}
