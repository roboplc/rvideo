use std::{sync::Arc, time::Duration};

use image::{ImageBuffer, Rgb};
use serde_json::Value;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = rvideo::Client::connect("127.0.0.1:3001", Duration::from_secs(5))?;
    let info = client.select_stream(0, 5)?;
    println!("{}", info);
    let width: u32 = u32::from(info.width);
    let height: u32 = u32::from(info.height);
    for (c, frame) in client.enumerate() {
        let frame = frame?;
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
    }
    Ok(())
}
