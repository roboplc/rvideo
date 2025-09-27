use std::{
    sync::{
        atomic,
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use ab_glyph::FontRef;
use clap::Parser;
use eframe::egui;
use egui::{Button, Color32, ColorImage, RichText};
use image::{DynamicImage, ImageBuffer, ImageReader, Rgb, RgbImage};
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_hollow_rect_mut, draw_text_mut, text_size},
    rect::Rect,
};
use rvideo::{BoundingBox, StreamInfo};
use serde::Deserialize;
use serde_json::Value;

const FPS_REPORT_DELAY: Duration = Duration::from_secs(1);
const FONT_TTF: &[u8] = include_bytes!("../fonts/DejaVuSans.ttf");

type MaybeFrame = Option<(RgbImage, Option<Value>, u32, u32)>;

#[derive(Default)]
struct Controls {
    inner: Arc<ControlsInner>,
}

impl Clone for Controls {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Controls {
    fn show_labels(&self) -> bool {
        self.inner.show_labels.load(atomic::Ordering::Relaxed)
    }
    fn set_show_labels(&self, v: bool) {
        self.inner.show_labels.store(v, atomic::Ordering::Relaxed);
    }
    fn toggle_show_labels(&self) {
        let v = !self.show_labels();
        self.set_show_labels(v);
    }
}

struct ControlsInner {
    show_labels: atomic::AtomicBool,
}

impl Default for ControlsInner {
    fn default() -> Self {
        Self {
            show_labels: atomic::AtomicBool::new(true),
        }
    }
}

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
    #[clap(short = 'r', long, default_value = "false")]
    auto_reconnect: bool,
    #[clap(long)]
    hide_labels: bool,
}

fn vec_u8_to_vec_u16(input: Vec<u8>) -> Vec<u16> {
    input
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect()
}

fn handle_connection(
    client: rvideo::Client,
    tx: Sender<MaybeFrame>,
    stream_info: StreamInfo,
    controls: Controls,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = stream_info.width.into();
    let height = stream_info.height.into();
    let font = FontRef::try_from_slice(FONT_TTF).unwrap();
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
                    if !controls.show_labels() {
                        continue;
                    }
                    let mut label = bbox.label.unwrap_or_default();
                    if let Some(conf) = bbox.confidence {
                        label.push(' ');
                        label.push_str(format!("{:.2}", conf).as_str());
                    }
                    if !label.is_empty() {
                        let (mut label_w, mut label_h) = text_size(12.0, &font, &label);
                        label_w += 4;
                        label_h += 4;
                        let label_x = if label_w + u32::from(bbox.x) > width {
                            width - label_w - 1
                        } else {
                            bbox.x.into()
                        };
                        let label_y = if u32::from(bbox.y) > label_h {
                            u32::from(bbox.y) - label_h
                        } else {
                            (bbox.y + bbox.height).into()
                        };
                        draw_filled_rect_mut(
                            &mut img,
                            Rect::at(
                                label_x.try_into().unwrap_or_default(),
                                label_y.try_into().unwrap_or_default(),
                            )
                            .of_size(label_w, label_h),
                            Rgb(bbox.color),
                        );
                        draw_text_mut(
                            &mut img,
                            Rgb([0, 0, 0]),
                            i32::try_from(label_x).unwrap_or_default() + 2,
                            i32::try_from(label_y).unwrap_or_default() + 2,
                            ab_glyph::PxScale::from(12.0),
                            &font,
                            &label,
                        );
                    }
                }
            }
        }
        tx.send(Some((img, meta, width, height)))?;
    }
    Ok(())
}

fn connect(
    source: &str,
    timeout: Duration,
    stream_id: u16,
    max_fps: u8,
    auto_reconnect: bool,
) -> Result<(rvideo::Client, StreamInfo), Box<dyn std::error::Error>> {
    loop {
        println!("Connecting to {}...", source);
        match rvideo::Client::connect(source, timeout) {
            Ok(mut v) => match v.select_stream(stream_id, max_fps) {
                Ok(stream_info) => return Ok((v, stream_info)),
                Err(e) => {
                    eprintln!("Stream selection error: {:?}", e);
                    if !auto_reconnect {
                        return Err(e.into());
                    }
                }
            },
            Err(e) => {
                eprintln!("Connection error: {:?}", e);
                if !auto_reconnect {
                    return Err(e.into());
                }
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut source = args.source;
    let controls = Controls::default();
    if args.hide_labels {
        controls.set_show_labels(false);
    }
    if !source.contains(':') {
        source = format!("{}:3001", source);
    }
    let auto_reconnect = args.auto_reconnect;
    println!("Source: {}", source);
    let timeout = Duration::from_secs(u64::from(args.timeout));
    let (mut client, stream_info) = connect(
        &source,
        timeout,
        args.stream_id,
        args.max_fps,
        auto_reconnect,
    )?;
    println!("Stream connected: {} {}", source, stream_info);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([
            f32::from(stream_info.width) + 40.0,
            f32::from(stream_info.height) + 80.0,
        ]),
        ..Default::default()
    };
    let (tx, rx) = channel();
    let mut stream_info_c = stream_info.clone();
    let source_c = source.clone();
    let online_beacon = Arc::new(atomic::AtomicBool::new(true));
    let online_beacon_c = online_beacon.clone();
    thread::spawn({
        let controls = controls.clone();
        move || {
            while let Err(e) =
                handle_connection(client, tx.clone(), stream_info_c, controls.clone())
            {
                online_beacon_c.store(false, atomic::Ordering::Relaxed);
                tx.send(None).unwrap();
                eprintln!("Error: {:?}", e);
                if !auto_reconnect {
                    break;
                }
                (client, stream_info_c) = connect(
                    &source_c,
                    timeout,
                    args.stream_id,
                    args.max_fps,
                    auto_reconnect,
                )
                .expect("Reconnect failed");
                online_beacon_c.store(true, atomic::Ordering::Relaxed);
            }
        }
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
                online_beacon,
                controls,
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
    rx: Receiver<MaybeFrame>,
    stream_info: StreamInfo,
    source: String,
    last_frame: Option<Instant>,
    fps: Vec<(Instant, u8)>,
    anim: usize,
    captured_number: u32,
    online_beacon: Arc<atomic::AtomicBool>,
    controls: Controls,
}

const ANIMATION: &[char] = &['|', '/', '-', '\\'];

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (rgb_img, maybe_meta, width, height) = match self.rx.recv() {
            Ok(Some((rgb_img, maybe_meta, width, height))) => {
                self.stream_info.width = width.try_into().unwrap();
                self.stream_info.height = height.try_into().unwrap();
                (rgb_img, maybe_meta, width, height)
            }
            Ok(None) => {
                self.last_frame = None;
                self.fps.clear();
                (
                    RgbImage::new(
                        self.stream_info.width.into(),
                        self.stream_info.height.into(),
                    ),
                    None,
                    self.stream_info.width.into(),
                    self.stream_info.height.into(),
                )
            }
            Err(_) => {
                std::process::exit(1);
            }
        };
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
                ui.horizontal(|ui| {
                    let online = self.online_beacon.load(atomic::Ordering::Relaxed);
                    let text = if online {
                        RichText::new("ONLINE")
                            .color(Color32::BLACK)
                            .background_color(Color32::GREEN)
                    } else {
                        RichText::new("OFFLINE")
                            .color(Color32::BLACK)
                            .background_color(Color32::GRAY)
                    };
                    ui.label(text);
                    if ui.add(Button::new("Capture")).clicked() {
                        self.captured_number += 1;
                        let fname = format!("capture-{}.png", self.captured_number);
                        rgb_img.save(fname).unwrap();
                    }
                    let labels = if self.controls.show_labels() {
                        "Hide Labels"
                    } else {
                        "Show Labels"
                    };
                    if ui.add(Button::new(labels)).clicked() {
                        self.controls.toggle_show_labels();
                    }
                });
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
