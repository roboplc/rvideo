#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(rustdoc::missing_crate_level_docs)]

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
use egui::ColorImage;
use image::{DynamicImage, GrayImage, Rgb, RgbImage};
use imageproc::{drawing::draw_hollow_rect_mut, rect::Rect};
use rvideo::{BoundingBox, StreamInfo};
use serde::Deserialize;
use serde_json::Value;

#[derive(Parser)]
struct Args {
    #[clap()]
    source: String,
    #[clap(long, default_value = "255")]
    max_fps: u8,
    #[clap(long, default_value = "5")]
    timeout: u16,
    #[clap(long, default_value = "0")]
    stream_id: u16,
}

fn handle_connection(
    client: rvideo::Client,
    tx: Sender<(ColorImage, Option<Value>)>,
    stream_info: StreamInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = stream_info.width.into();
    let height = stream_info.height.into();
    for frame in client {
        let frame = frame?;
        let img_data = Arc::try_unwrap(frame.data).unwrap();
        let mut img: RgbImage = match stream_info.pixel_format {
            rvideo::PixelFormat::Luma8 => {
                DynamicImage::ImageLuma8(GrayImage::from_raw(width, height, img_data).unwrap())
                    .to_rgb8()
            }
            rvideo::PixelFormat::Rgb8 => RgbImage::from_raw(width, height, img_data).unwrap(),
        };
        let mut meta: Option<Value> = frame.metadata.and_then(|m| rmp_serde::from_slice(&m).ok());
        if let Some(ref mut meta) = meta {
            if let Value::Object(ref mut map) = meta {
                if let Some(Value::Array(vals)) = map.remove(".bboxes") {
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
        }
        let img = ColorImage::from_rgb(
            [width.try_into().unwrap(), height.try_into().unwrap()],
            &img,
        );
        tx.send((img, meta))?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("Source: {}", args.source);
    let mut client =
        rvideo::Client::connect(&args.source, Duration::from_secs(args.timeout.into()))?;
    let stream_info = client.select_stream(args.stream_id, args.max_fps)?;
    println!("Stream connected: {} {}", args.source, stream_info);
    if stream_info.compression != rvideo::Compression::No {
        return Err("Unsupported compression".into());
    }
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
        &format!("{}/{} - rvideo", args.source, args.stream_id),
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::new(MyApp {
                rx,
                stream_info,
                source: args.source,
                last_frame: None,
            })
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
    rx: Receiver<(ColorImage, Option<Value>)>,
    stream_info: StreamInfo,
    source: String,
    last_frame: Option<Instant>,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (img, maybe_meta) = self.rx.recv().unwrap();
        let now = Instant::now();
        let time_between_frames = self.last_frame.map(|t| now - t);
        let fps = time_between_frames
            .map(|t| (1.0 / t.as_secs_f64()) as u8)
            .unwrap_or(0);
        self.last_frame.replace(now);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                let texture = ui.ctx().load_texture("frame", img, <_>::default());
                ui.label(format!(
                    "Stream: {} {}, Actual FPS: {}",
                    self.source, self.stream_info, fps
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
