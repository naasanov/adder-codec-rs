extern crate adder_codec_core;

use adder_codec_core::codec::compressed::stream::CompressedOutput;
use adder_codec_core::codec::encoder::Encoder;

use adder_codec_core::codec::{CodecError, EncoderOptions};
use adder_codec_core::open_file_decoder;
use std::error::Error;
use std::io::BufWriter;


#[test]
fn test_read_adder_raw() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (stream, _bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;

    assert!(stream.meta().plane.w() == 192);

    Ok(())
}

#[test]
fn test_build_first_frame() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize; // This is a fix since we're reading a v2-encoded file

    // Create the compressed encoder
    let bufwriter = BufWriter::new(vec![]);
    let compression = CompressedOutput::new(*stream.meta(), bufwriter);

    let mut encoder: Encoder<BufWriter<Vec<u8>>> =
        Encoder::new_compressed(compression, EncoderOptions::default((stream.meta()).plane));

    for _i in 0..24000 {
        // Loop through the events and ingest them to the compressor
        let event = stream.digest_event(&mut bitreader)?;
        encoder.ingest_event(event)?;
    }

    Ok(())
}

#[test]
fn test_build_many_frames() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
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

    Ok(())
}

#[test]
fn test_compression_ratio() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file as a RawInput
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;

    let original_file_size = std::fs::metadata("tests/samples/virat_small_gray.adder")?.len();

    // Create the compressed encoder
    let bufwriter = BufWriter::new(vec![]);
    let compression = CompressedOutput::new(*stream.meta(), bufwriter);
    let mut encoder: Encoder<BufWriter<Vec<u8>>> =
        Encoder::new_compressed(compression, EncoderOptions::default((stream.meta()).plane));

    let mut event_count: u32 = 0;
    loop {
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
    let compressed_size = compressed.len() as u64;

    // Calculate metrics
    let raw_event_bytes = event_count as u64 * stream.meta().event_size as u64;
    let compression_ratio = compressed_size as f64 / raw_event_bytes as f64;
    let bits_per_event = (compressed_size as f64 * 8.0) / event_count as f64;
    let space_savings = ((raw_event_bytes - compressed_size) as f64 / raw_event_bytes as f64) * 100.0;

    // Print detailed report
    println!("\n=== COMPRESSION RATIO TEST RESULTS ===");
    println!("Original file size:        {} bytes", original_file_size);
    println!("Event count:               {}", event_count);
    println!("Event size (raw):          {} bytes", stream.meta().event_size);
    println!("Raw event stream size:     {} bytes ({} events x {} bytes)",
             raw_event_bytes, event_count, stream.meta().event_size);
    println!("Compressed size:           {} bytes", compressed_size);
    println!("\n=== PERFORMANCE METRICS ===");
    println!("Compression ratio:         {:.4} ({:.2}% of original)",
             compression_ratio, compression_ratio * 100.0);
    println!("Bits per event:            {:.2} bits", bits_per_event);
    println!("Space savings:             {:.2}%", space_savings);
    println!("======================================\n");

    // Assert that we achieved some compression
    assert!(compressed_size < raw_event_bytes,
            "Compressed size ({}) should be less than raw event bytes ({})",
            compressed_size, raw_event_bytes);

    // Assert reasonable compression (should compress by at least some amount)
    assert!(compression_ratio < 1.0,
            "Compression ratio should be less than 1.0, got {:.4}",
            compression_ratio);

    Ok(())
}
