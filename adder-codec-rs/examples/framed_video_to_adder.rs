extern crate core;

use adder_codec_core::codec::{EncoderOptions, EncoderType};
use adder_codec_core::SourceCamera::FramedU8;
use adder_codec_core::{PixelMultiMode, TimeMode};
use adder_codec_rs::transcoder::source::framed::Framed;
use adder_codec_rs::transcoder::source::video::{Source, VideoBuilder};
use clap::Parser;
use rayon::current_num_threads;
use std::error::Error;
use std::fs::File;
use std::io;
use std::io::{BufWriter, Write};
use std::time::Instant;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Path to input video file
    #[clap(short, long)]
    pub input: String,

    /// Path to output ADΔER file
    #[clap(short, long)]
    pub output: String,

    /// Starting frame number (default: 0)
    #[clap(short, long, default_value_t = 0)]
    pub start_frame: usize,

    /// Maximum number of frames to process (0 = all frames)
    #[clap(short, long, default_value_t = 0)]
    pub max_frames: usize,

    /// Use compressed encoding (default: true)
    #[clap(short, long, default_value_t = true)]
    pub compress: bool,

    /// Frames per second
    #[clap(short, long, default_value_t = 30)]
    pub fps: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let file = File::create(&args.output)?;
    let writer = BufWriter::new(file);

    let mut source: Framed<BufWriter<File>> = Framed::new(args.input.parse().unwrap(), false, 0.5)?;
    let plane = source.get_video_ref().state.plane;

    let encoder_type = if args.compress {
        EncoderType::Compressed
    } else {
        EncoderType::Raw
    };

    source = source
        .frame_start(args.start_frame as u32)?
        .write_out(
            FramedU8,
            TimeMode::DeltaT,
            PixelMultiMode::Normal,
            Some(args.fps as usize),
            encoder_type,
            EncoderOptions::default(plane),
            writer,
        )?
        .auto_time_parameters(255, 255 * args.fps as u32, None)?;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(current_num_threads())
        .build()
        .unwrap();
    let mut now = Instant::now();

    let frame_max = args.max_frames;

    loop {
        match source.consume() {
            Ok(_) => {} // Returns Vec<Vec<Event>>, but we're just writing the events out in this example
            Err(e) => {
                println!("\nErr: {e:?}");
                break;
            }
        };

        let video = source.get_video_ref();

        if video.state.in_interval_count % args.fps as u32 == 0 {
            print!(
                "\rFrame {} in {}ms",
                video.state.in_interval_count,
                now.elapsed().as_millis()
            );
            io::stdout().flush().unwrap();
            now = Instant::now();
        }
        if frame_max != 0 && video.state.in_interval_count >= frame_max as u32 {
            break;
        }
    }

    println!("\nClosing stream...");
    source.get_video_mut().end_write_stream().unwrap();
    println!("FINISHED");

    Ok(())
}
