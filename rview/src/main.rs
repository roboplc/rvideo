#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(rustdoc::missing_crate_level_docs)]

use std::{
    sync::mpsc::{channel, Receiver, Sender},
    thread,
    time::Duration,
};

use clap::Parser;
use eframe::egui;
use egui::ColorImage;
use rvideo::StreamInfo;

#[derive(Parser)]
struct Args {
    #[clap()]
    target: String,
    #[clap(long, default_value = "255")]
    max_fps: u8,
    #[clap(long, default_value = "5")]
    timeout: u16,
    #[clap(long, default_value = "0")]
    stream_id: u16,
}

fn handle_connection(
    client: rvideo::Client,
    tx: Sender<ColorImage>,
    stream_info: StreamInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = stream_info.width.into();
    let height = stream_info.height.into();
    for frame in client {
        let frame = frame?;
        let img = match stream_info.pixel_format {
            rvideo::PixelFormat::Luma8 => ColorImage::from_gray([width, height], &frame.data),
            rvideo::PixelFormat::Rgb8 => ColorImage::from_rgb([width, height], &frame.data),
        };
        tx.send(img)?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{}", args.target);
    let mut client =
        rvideo::Client::connect(&args.target, Duration::from_secs(args.timeout.into()))?;
    let stream_info = client.select_stream(args.stream_id, args.max_fps)?;
    dbg!(&stream_info);
    if stream_info.compression != rvideo::Compression::No {
        return Err("Unsupported compression".into());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([f32::from(stream_info.width), f32::from(stream_info.height)]),
        ..Default::default()
    };
    let (tx, rx) = channel();
    thread::spawn(move || {
        let code = if let Err(e) = handle_connection(client, tx, stream_info) {
            eprintln!("Error: {:?}", e);
            1
        } else {
            0
        };
        std::process::exit(code);
    });
    eframe::run_native(
        &format!("{}/{} - rvideo", args.target, args.stream_id),
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::new(MyApp { rx })
        }),
    )?;
    Ok(())
}

struct MyApp {
    rx: Receiver<ColorImage>,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let img = self.rx.recv().unwrap();
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().show(ui, |ui| {
                let texture = ui.ctx().load_texture("logo", img, <_>::default());
                ui.image(&texture);
            });
        });
        ctx.request_repaint();
    }
}
