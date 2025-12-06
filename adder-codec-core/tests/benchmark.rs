extern crate adder_codec_core;

use adder_codec_core::codec::compressed::stream::CompressedOutput;
use adder_codec_core::codec::encoder::Encoder;

use adder_codec_core::codec::{CodecError, EncoderOptions};
use adder_codec_core::open_file_decoder;
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter, Seek};
use std::path::Path;

enum BenchmarkResult {}

#[test]
fn compression_benchmark() -> Result<(), Box<dyn Error>> {
    let files = vec![
        "tests/samples/virat_small_gray.adder",    // Given file
        "tests/samples/lake_scaled_hd_crop.adder", // Medium, full resolution
        "tests/samples/bunny_crop4.adder",         // Large, contains color
    ];

    // Old size, new size
    let mut results: Vec<(&str, u64, u64)> = vec![];

    for file in files.iter() {
        // Skip missing sample files so the benchmark can run on available data.
        if !Path::new(file).exists() {
            eprintln!("Skipping missing sample file: {}", file);
            continue;
        }

        let (mut stream, mut bitreader) = open_file_decoder(file)?;
        println!("{}", file);
        stream.meta_mut().adu_interval =
            (stream.meta().delta_t_max / stream.meta().ref_interval) as usize; // This is a fix since we're reading a v2-encoded file

        // Create the compressed encoder
        let bufwriter = BufWriter::new(vec![]);
        let compression = CompressedOutput::new(*stream.meta(), bufwriter);
        let mut encoder: Encoder<BufWriter<Vec<u8>>> =
            Encoder::new_compressed(compression, EncoderOptions::default((stream.meta()).plane));

        let mut event_count: u32 = 0;
        loop {
            // Loop through the events and ingest them to the compressor
            let res = stream.digest_event(&mut bitreader);
            match res {
                Ok(event) => {
                    encoder.ingest_event(event)?;
                    event_count += 1;
                }
                Err(CodecError::IoError(_e)) => {
                    break;
                }
                Err(e) => return Err(Box::new(e)),
            }
        }

        encoder.flush_writer()?;
        let writer = encoder.close_writer()?.unwrap();
        let compressed = writer.into_inner()?;
        dbg!(compressed.len());
        assert!((compressed.len() as u32) < event_count * stream.meta().event_size as u32);

        // Collecting results
        let init_size = bitreader.reader().ok_or("oops")?.stream_position()?;
        let post_size = compressed.len();
        results.push((file, init_size, post_size as u64));
    }

    // Printing the results
    println!("=== RESULTS ===");
    println!(
        "{:<28}\t{:<9}\t{:<9}\t{:<8}",
        "File name", "Old size", "New size", "Ratio"
    );
    for result in results {
        let filename = Path::new(result.0)
            .file_name()
            .ok_or("File missing a name")?
            .to_str()
            .ok_or("Cannot convert OsStr to str")?;

        println!(
            "{:<28}\t{:>9}\t{:>9}\t{:>8.3}%",
            filename,
            result.1,
            result.2,
            (result.2 as f64 / result.1 as f64) * 100.0,
        );
    }

    Ok(())
}
