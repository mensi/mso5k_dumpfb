#![feature(with_options)]
use clap::{App, Arg};
use libc;
use memmap::MmapOptions;
use simple_error::SimpleError;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;

fn main() -> Result<(), Box<dyn Error>> {
    let matches = App::new("MSO5k Framebuffer Dumper")
        .about("Reads the different layers of the framebuffer")
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .value_name("FILE")
                .about("Framebuffer device file")
                .default_value("/dev/fb0")
                .takes_value(true),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("FILE")
                .about("File to write to or - for stdout")
                .default_value("-")
                .takes_value(true),
        )
        .arg(
            Arg::new("raw")
                .long("raw")
                .about("Write raw buffer data")
                .takes_value(false),
        )
        .arg(
            Arg::new("printscreen")
                .short('p')
                .long("printscreen")
                .about("Instruct the hardware to do a printscreen")
                .takes_value(false),
        )
        .arg(Arg::new("layer").about("Layer number").value_name("LAYER"))
        .get_matches();

    let layer = match matches.value_of("layer") {
        Some(x) => x.parse::<i32>()?,
        None => {
            if matches.is_present("printscreen") {
                4 // If no layer is specified and the printscreen option is passed, take that layer.
            } else {
                return Err(Box::new(SimpleError::new("No layer number given.")));
            }
        }
    };
    if layer > 5 || layer < 0 {
        return Err(Box::new(SimpleError::new("Layer must be from 0 to 5")));
    }

    // Determine layer metrics.
    let width: usize = match layer {
        1 | 2 => 1000,
        _ => 1024,
    };
    let height: usize = match layer {
        1 | 2 => 480,
        _ => 600,
    };
    let bytes_per_pixel: usize = match layer {
        1 => 4,
        _ => 2,
    };
    let layer_len: usize = width * height * bytes_per_pixel;

    // Set up the output - either a file or stdout.
    let mut output: Box<dyn Write> = match matches.value_of("output").unwrap() {
        "-" => Box::new(io::stdout()),
        path => Box::new(File::with_options().write(true).create(true).open(path)?),
    };
    let input = File::open(matches.value_of("input").unwrap())?;

    // Take a screenshot if requested.
    if matches.is_present("printscreen") {
        let res = do_printscreen(&input)?;
        if res != 0 {
            return Err(Box::new(SimpleError::new(format!(
                "Printscreen failed with code: {}",
                res
            ))));
        }
        if layer != 4 {
           eprintln!("Took screenshot but the layer to dump is different (not 4)");
        }
    }

    // Switch layer, map the memory and switch back.
    let old_layer = get_layer(&input)?;
    eprintln!("Active layer is: {}", old_layer);
    swap_layer(&input, layer)?;
    let mmap = unsafe { MmapOptions::new().len(layer_len).map(&input)? };
    swap_layer(&input, old_layer)?;
    eprintln!("Layer has been switched back to: {}", old_layer);

    // Generate the output either raw or as a PNG.
    if matches.is_present("raw") {
        output.write_all(&mmap)?;
    } else {
        let mut encoder = png::Encoder::new(output, width as u32, height as u32);
        encoder.set_color(png::ColorType::RGBA);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .unwrap()
            .into_stream_writer_with_size(4000);

        let mut buf: Vec<u8> = vec![0; width * 4];
        for row in 0..height {
            match layer {
                1 => {
                    buf.copy_from_slice(&mmap[row * width * 4..(row + 1) * width * 4]);
                    for pix in 0..width {
                        if buf[pix * 4] != 0xcc
                            || buf[pix * 4 + 1] != 0xcc
                            || buf[pix * 4 + 2] != 0xcc
                        {
                            // This pixel is not transparent (0xCCCCCC), so set the alpha channel to 255.
                            buf.swap(pix * 4, pix * 4 + 2);
                            buf[pix * 4 + 3] = 0xff;
                        }
                    }
                }
                _ => {
                    // The other layers are RGB565
                    for pix in 0..width {
                        let packed: u16 = (mmap[(row * width + pix) * 2 + 1] as u16) << 8
                            | (mmap[(row * width + pix) * 2]) as u16;
                        buf[pix * 4] = ((packed & 0xf800) >> 8) as u8;
                        buf[pix * 4 + 1] = ((packed & 0x7e0) >> 3) as u8;
                        buf[pix * 4 + 2] = ((packed & 0x1f) << 3) as u8;

                        if packed == 0xcccc {
                            buf[pix * 4 + 3] = 0;
                        } else {
                            buf[pix * 4 + 3] = 0xff;
                        }
                    }
                }
            }
            writer.write_all(&buf)?;
        }
        writer.finish()?;
    }

    Ok(())
}

#[derive(Debug)]
struct IoctlError {
    return_value: i32,
}
impl fmt::Display for IoctlError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Ioctl returned: {}", self.return_value)
    }
}
impl Error for IoctlError {}

fn get_layer(file: &File) -> Result<i32, IoctlError> {
    unsafe {
        let mut layer: i32 = -1;
        let layer_ptr: *mut i32 = &mut layer;
        let res = libc::ioctl(file.as_raw_fd(), 0x0f000001, layer_ptr);
        if res != 0 {
            Err(IoctlError { return_value: res })
        } else {
            Ok(layer)
        }
    }
}

fn swap_layer(file: &File, idx: i32) -> Result<(), IoctlError> {
    unsafe {
        let res = libc::ioctl(file.as_raw_fd(), 0x0f000000, idx);
        if res != 0 {
            Err(IoctlError { return_value: res })
        } else {
            Ok(())
        }
    }
}

fn do_printscreen(file: &File) -> Result<i32, IoctlError> {
    unsafe {
        let mut result: i32 = -1;
        let result_ptr: *mut i32 = &mut result;
        let res = libc::ioctl(file.as_raw_fd(), 0x0f00000c, result_ptr);
        if res != 0 {
            Err(IoctlError { return_value: res })
        } else {
            Ok(result)
        }
    }
}
