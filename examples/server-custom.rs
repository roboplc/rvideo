use std::{thread, time::Duration};

use image::{ImageBuffer, Rgb};
use imageproc::drawing::draw_text_mut;
use rusttype::{Font, Scale};
use rvideo::{BoundingBox, Format, Frame, Server};
use serde::Serialize;

const FONT: &[u8] = include_bytes!("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf");

#[derive(Serialize)]
struct FrameInfo {
    source: String,
    frame_number: u64,
    #[serde(rename = ".bboxes")]
    bounding_boxes: Vec<BoundingBox>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 640;
    let height = 480;
    let server = Server::new(Duration::from_secs(5));
    let stream = server.add_stream(Format::Rgb8, width, height)?;
    thread::spawn(move || {
        let mut frame_number = 0;
        let font = Font::try_from_bytes(FONT).unwrap();
        loop {
            let mut imgbuf =
                ImageBuffer::<Rgb<u8>, Vec<u8>>::from_fn(width.into(), height.into(), |_, _| {
                    Rgb([0, 0, 0])
                });
            draw_text_mut(
                &mut imgbuf,
                Rgb::from([255, 255, 255]),
                0,
                0,
                Scale { x: 100.0, y: 100.0 },
                &font,
                &frame_number.to_string(),
            );
            dbg!(frame_number);
            let metadata = FrameInfo {
                source: "test".to_string(),
                frame_number,
                bounding_boxes: vec![
                    BoundingBox {
                        color: [255, 0, 0],
                        x: 100,
                        y: 300,
                        width: 100,
                        height: 100,
                    },
                    BoundingBox {
                        color: [0, 255, 0],
                        x: 220,
                        y: 220,
                        width: 50,
                        height: 50,
                    },
                ],
            };
            stream
                .send_frame(Frame::new_with_metadata(
                    rmp_serde::to_vec_named(&metadata).unwrap().into(),
                    imgbuf.to_vec().into(),
                ))
                .unwrap();
            thread::sleep(Duration::from_millis(100));
            frame_number += 1;
        }
    });
    server.serve("127.0.0.1:3001")?;
    Ok(())
}
