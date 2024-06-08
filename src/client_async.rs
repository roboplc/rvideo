use std::{io::Cursor, time::Duration};

use binrw::BinRead;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
};

use crate::{Error, Frame, Greetings, StreamInfo, StreamSelect};

pub struct ClientAsync {
    stream: TcpStream,
    streams_available: u16,
    ready: bool,
    timeout: Duration,
}

impl ClientAsync {
    pub async fn connect(addr: impl ToSocketAddrs, timeout: Duration) -> Result<Self, Error> {
        let mut stream = tokio::time::timeout(timeout, TcpStream::connect(addr)).await??;
        stream.set_nodelay(true)?;
        let mut buf = [0u8; 4];
        tokio::time::timeout(timeout, stream.read_exact(&mut buf)).await??;
        let greetings = Greetings::read(&mut Cursor::new(&buf))?;
        if greetings.api_version != crate::API_VERSION {
            return Err(Error::ApiVersion(greetings.api_version));
        }
        Ok(Self {
            stream,
            streams_available: greetings.streams_available,
            ready: false,
            timeout,
        })
    }
    pub fn streams_available(&self) -> u16 {
        self.streams_available
    }
    pub async fn select_stream(
        &mut self,
        stream_id: u16,
        max_fps: u8,
    ) -> Result<StreamInfo, Error> {
        let stream_select = StreamSelect { stream_id, max_fps };
        let mut writer = Cursor::new(Vec::new());
        binrw::BinWrite::write(&stream_select, &mut writer)?;
        tokio::time::timeout(self.timeout, self.stream.write_all(&writer.into_inner())).await??;
        let mut buf = [0u8; 7];
        tokio::time::timeout(self.timeout, self.stream.read_exact(&mut buf)).await??;
        let stream_info = StreamInfo::read(&mut Cursor::new(&buf))?;
        if stream_info.id == stream_id {
            self.ready = true;
            Ok(stream_info)
        } else {
            Err(Error::InvalidStream)
        }
    }
    pub async fn read_next(&mut self) -> Result<Frame, Error> {
        if !self.ready {
            return Err(Error::NotReady);
        }
        let mut len_buf = [0u8; 4];
        tokio::time::timeout(self.timeout, self.stream.read_exact(&mut len_buf)).await??;
        let len = usize::try_from(u32::from_le_bytes(len_buf))
            .map_err(|_| Error::FrameMetaDataTooLarge)?;
        dbg!(len);
        let metadata = if len > 0 {
            let mut buf = vec![0u8; len];
            tokio::time::timeout(self.timeout, self.stream.read_exact(&mut buf)).await??;
            Some(buf)
        } else {
            None
        };
        tokio::time::timeout(self.timeout, self.stream.read_exact(&mut len_buf)).await??;
        let len =
            usize::try_from(u32::from_le_bytes(len_buf)).map_err(|_| Error::FrameDataTooLarge)?;
        dbg!(len);
        let mut data = vec![0u8; len];
        tokio::time::timeout(self.timeout, self.stream.read_exact(&mut data)).await??;
        dbg!(data.len());
        Ok(Frame {
            metadata: metadata.map(Into::into),
            data: data.into(),
        })
    }
}
