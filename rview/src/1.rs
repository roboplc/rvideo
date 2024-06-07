use image::{DynamicImage, ImageBuffer, Luma};
use std::{sync::Arc, time::Duration};
use x11_dl::xlib;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = rvideo::Client::connect("localhost:3001", Duration::from_secs(2))?;
    let stream_info = client.select_stream(0, 1)?;
    if stream_info.compression != rvideo::Compression::No {
        return Err("Unsupported compression".into());
    }
    let xlib = xlib::Xlib::open().unwrap();
    let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
    let screen = unsafe { (xlib.XDefaultScreen)(display) };
    let root = unsafe { (xlib.XRootWindow)(display, screen) };

    let width = stream_info.width.into();
    let height = stream_info.height.into();
    let win = unsafe {
        let win = (xlib.XCreateSimpleWindow)(display, root, 0, 0, width, height, 1, 0, 0);
        (xlib.XMapWindow)(display, win);
        (xlib.XStoreName)(display, win, "Image Display\0".as_ptr() as *const i8);
        win
    };
    let gc = unsafe { (xlib.XCreateGC)(display, win, 0, std::ptr::null_mut()) };

    //let img = ImageBuffer::from_fn(width, height, |x, y| Rgb([255, 0, 255]));
    let mut img_buf;

    for frame in client {
        let frame = frame?;
        img_buf = match stream_info.pixel_format {
            rvideo::PixelFormat::Luma8 => {
                let img: ImageBuffer<Luma<u8>, Vec<u8>> =
                    ImageBuffer::from_vec(width, height, Arc::into_inner(frame.data).unwrap())
                        .unwrap();
                let img = DynamicImage::ImageLuma8(img);
                img.to_rgb8().to_vec()
            }
            rvideo::PixelFormat::Rgb8 => Arc::into_inner(frame.data).unwrap(),
        };
        dbg!("frame", img_buf.len(), width, height);
        unsafe {
            let ximage = (xlib.XCreateImage)(
                display,
                (xlib.XDefaultVisual)(display, screen),
                24,
                xlib::ZPixmap,
                0,
                img_buf.as_mut_ptr() as *mut _,
                width,
                height,
                32,
                0,
            );
            if ximage.is_null() {
                panic!("Cannot create XImage");
            }
            (xlib.XPutImage)(display, win, gc, ximage, 0, 0, 0, 0, width, height);
            //(xlib.XDestroyImage)(ximage);
            (xlib.XFlush)(display);
        }
    }
    Ok(())
}
