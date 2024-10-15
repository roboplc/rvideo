use std::{
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use clap::Parser;
use eframe::egui;
use egui::{Button, ColorImage};
use image::{DynamicImage, ImageBuffer, ImageReader, Rgb, RgbImage};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use rvideo::{BoundingBox, StreamInfo};
use serde::Deserialize;
use serde_json::Value;

const FPS_REPORT_DELAY: Duration = Duration::from_secs(1);

#[derive(Parser)]
struct Args {
    #[clap(help = "HOST[:PORT], the default port is 3001")]
    source: String,
    #[clap(long, default_value = "255")]
    max_fps: u8,
    #[clap(long, default_value = "5")]
    timeout: u16,
    #[clap(long, default_value = "0")]
    stream_id: u16,
}

fn vec_u8_to_vec_u16(input: Vec<u8>) -> Vec<u16> {
    input
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect()
}

fn handle_connection(
    client: rvideo::Client,
    tx: Sender<(RgbImage, Option<Value>, u32, u32)>,
    stream_info: StreamInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = stream_info.width.into();
    let height = stream_info.height.into();
    for frame in client {
        let frame = frame?;
        let img_data = Arc::try_unwrap(frame.data).unwrap();
        let mut img: RgbImage = match stream_info.format {
            rvideo::Format::Luma8 => {
                DynamicImage::ImageLuma8(ImageBuffer::from_raw(width, height, img_data).unwrap())
                    .into()
            }
            rvideo::Format::Luma16 => DynamicImage::ImageLuma16(
                ImageBuffer::from_raw(width, height, vec_u8_to_vec_u16(img_data)).unwrap(),
            )
            .into(),
            rvideo::Format::LumaA8 => {
                DynamicImage::ImageLumaA8(ImageBuffer::from_raw(width, height, img_data).unwrap())
                    .into()
            }
            rvideo::Format::LumaA16 => DynamicImage::ImageLumaA16(
                ImageBuffer::from_raw(width, height, vec_u8_to_vec_u16(img_data)).unwrap(),
            )
            .into(),
            rvideo::Format::Rgb8 => RgbImage::from_raw(width, height, img_data).unwrap(),
            rvideo::Format::Rgb16 => DynamicImage::ImageRgb16(
                ImageBuffer::from_raw(width, height, vec_u8_to_vec_u16(img_data)).unwrap(),
            )
            .into(),
            rvideo::Format::Rgba8 => {
                DynamicImage::ImageRgba8(ImageBuffer::from_raw(width, height, img_data).unwrap())
                    .into()
            }
            rvideo::Format::Rgba16 => DynamicImage::ImageRgba16(
                ImageBuffer::from_raw(width, height, vec_u8_to_vec_u16(img_data)).unwrap(),
            )
            .into(),
            rvideo::Format::MJpeg => {
                let buf = std::io::Cursor::new(img_data);
                let mut reader = ImageReader::new(buf);
                reader.set_format(image::ImageFormat::Jpeg);
                reader.decode()?.into()
            }
        };
        let mut meta: Option<Value> = frame.metadata.and_then(|m| rmp_serde::from_slice(&m).ok());
        if let Some(Value::Object(ref mut o)) = meta {
            if let Some(Value::Array(vals)) = o.remove(".bboxes") {
                for val in vals {
                    let Ok(bbox) = BoundingBox::deserialize(val) else {
                        continue;
                    };
                    draw_hollow_rect_mut(
                        &mut img,
                        Rect::at(bbox.x.into(), bbox.y.into())
                            .of_size(bbox.width.into(), bbox.height.into()),
                        Rgb(bbox.color),
                    );
                }
            }
        }
        tx.send((img, meta, width, height))?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut source = args.source;
    if !source.contains(':') {
        source = format!("{}:3001", source);
    }
    println!("Source: {}", source);
    let mut client = rvideo::Client::connect(&source, Duration::from_secs(args.timeout.into()))?;
    let stream_info = client.select_stream(args.stream_id, args.max_fps)?;
    println!("Stream connected: {} {}", source, stream_info);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            f32::from(stream_info.width) + 40.0,
            f32::from(stream_info.height) + 80.0,
        ]),
        ..Default::default()
    };
    let (tx, rx) = channel();
    let stream_info_c = stream_info.clone();
    thread::spawn(move || {
        let code = if let Err(e) = handle_connection(client, tx, stream_info_c) {
            eprintln!("Error: {:?}", e);
            1
        } else {
            0
        };
        std::process::exit(code);
    });
    eframe::run_native(
        &format!("{}/{} - rvideo", source, args.stream_id),
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(MyApp {
                rx,
                stream_info,
                source,
                last_frame: None,
                fps: <_>::default(),
                anim: 0,
                captured_number: 0,
            }))
        }),
    )?;
    Ok(())
}

fn format_value(value: Value, join_object: &str) -> String {
    match value {
        Value::Object(map) => map
            .into_iter()
            .map(|(k, v)| format!("{}: {}", k, format_value(v, ",")))
            .collect::<Vec<_>>()
            .join(join_object),
        Value::Array(arr) => arr
            .into_iter()
            .map(|v| format_value(v, ","))
            .collect::<Vec<_>>()
            .join("; "),
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
    }
}

struct MyApp {
    rx: Receiver<(RgbImage, Option<Value>, u32, u32)>,
    stream_info: StreamInfo,
    source: String,
    last_frame: Option<Instant>,
    fps: Vec<(Instant, u8)>,
    anim: usize,
    captured_number: u32,
}

const ANIMATION: &[char] = &['|', '/', '-', '\\'];

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (rgb_img, maybe_meta, width, height) = self.rx.recv().unwrap();
        let egui_img = ColorImage::from_rgb(
            [width.try_into().unwrap(), height.try_into().unwrap()],
            &rgb_img,
        );
        let now = Instant::now();
        let anim_char = ANIMATION[self.anim];
        self.anim += 1;
        if self.anim >= ANIMATION.len() {
            self.anim = 0;
        }
        let time_between_frames = self.last_frame.map(|t| now - t);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let last_fps = time_between_frames.map_or(0, |t| (1.0 / t.as_secs_f64()) as u8);
        self.fps.push((now, last_fps));
        self.fps.retain(|(t, _)| now - *t < FPS_REPORT_DELAY);
        self.last_frame.replace(now);
        let fps = self
            .fps
            .iter()
            .map(|(_, fps)| usize::from(*fps))
            .sum::<usize>()
            / self.fps.len();
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                let texture = ui.ctx().load_texture("frame", egui_img, <_>::default());
                if ui.add(Button::new("Capture")).clicked() {
                    self.captured_number += 1;
                    let fname = format!("capture-{}.png", self.captured_number);
                    rgb_img.save(fname).unwrap();
                }
                ui.label(format!(
                    "Stream: {} {}, Actual FPS: {}  {}",
                    self.source, self.stream_info, fps, anim_char
                ));
                ui.image(&texture);
                if let Some(meta) = maybe_meta {
                    ui.label(format_value(meta, "\n"));
                }
            });
        });
        ctx.request_repaint();
    }
}
