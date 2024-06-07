use std::{sync::Arc, time::Duration};

use image::{ImageBuffer, Rgb};
use rvideo::ClientAsync;
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = ClientAsync::connect("127.0.0.1:3001", Duration::from_secs(5)).await?;
    let info = client.select_stream(0, 5).await?;
    let width: u32 = u32::from(info.width);
    let height: u32 = u32::from(info.height);
    let mut c = 0;
    while let Ok(frame) = client.read_next().await {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_vec(width, height, Arc::try_unwrap(frame.data).unwrap()).unwrap();
        dbg!("frame");
        let metadata = if let Some(meta) = frame.metadata {
            rmp_serde::from_slice(&meta)?
        } else {
            Value::Null
        };
        dbg!(metadata);
        img.save(format!("frame{}.png", c)).unwrap();
        c += 1;
    }
    Ok(())
}
