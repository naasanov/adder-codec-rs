extern crate adder_codec_core;

use adder_codec_core::codec::compressed::stream::CompressedOutput;
use adder_codec_core::codec::encoder::Encoder;
use adder_codec_core::codec::{CodecError, EncoderOptions};
use adder_codec_core::open_file_decoder;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};

/// Test that compressed files can be decompressed back to original events
#[test]
fn roundtrip_compression_test() -> Result<(), Box<dyn Error>> {
    let test_file = "tests/samples/virat_small_gray.adder";

    println!("Testing round-trip compression/decompression for: {}", test_file);

    // Step 1: Read original file and collect all events
    let (mut stream, mut bitreader) = open_file_decoder(test_file)?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;

    let mut original_events = Vec::new();
    loop {
        match stream.digest_event(&mut bitreader) {
            Ok(event) => original_events.push(event),
            Err(CodecError::IoError(_)) => break,
            Err(e) => return Err(Box::new(e)),
        }
    }

    println!("Original event count: {}", original_events.len());

    // Step 2: Compress events
    let bufwriter = BufWriter::new(vec![]);
    let compression = CompressedOutput::new(*stream.meta(), bufwriter);
    let mut encoder: Encoder<BufWriter<Vec<u8>>> =
        Encoder::new_compressed(compression, EncoderOptions::default(stream.meta().plane));

    for event in &original_events {
        encoder.ingest_event(*event)?;
    }

    encoder.flush_writer()?;
    let writer = encoder.close_writer()?.unwrap();
    let compressed_data = writer.into_inner()?;

    println!("Compressed size: {} bytes", compressed_data.len());
    println!("Compression ratio: {:.2}%",
        (compressed_data.len() as f64 / (original_events.len() * stream.meta().event_size as usize) as f64) * 100.0);

    // Step 3: Write compressed data to a temporary file
    let temp_file_path = "/tmp/test_compressed.adder";
    let mut temp_file = File::create(temp_file_path)?;
    temp_file.write_all(&compressed_data)?;
    temp_file.sync_all()?;
    drop(temp_file);

    println!("Wrote compressed data to: {}", temp_file_path);

    // Step 4: Decompress and verify
    let (mut decoder_stream, mut decoder_bitreader) = open_file_decoder(temp_file_path)?;

    let mut decompressed_events = Vec::new();
    loop {
        match decoder_stream.digest_event(&mut decoder_bitreader) {
            Ok(event) => decompressed_events.push(event),
            Err(CodecError::IoError(_)) => break,
            Err(e) => return Err(Box::new(e)),
        }
    }

    println!("Decompressed event count: {}", decompressed_events.len());

    // Step 5: Verify event counts match
    assert_eq!(
        original_events.len(),
        decompressed_events.len(),
        "Event count mismatch! Original: {}, Decompressed: {}",
        original_events.len(),
        decompressed_events.len()
    );

    // Step 6: Verify events match (allowing for lossy compression tolerance)
    let mut mismatch_count = 0;
    let mut max_time_error = 0i64;

    for (i, (orig, decomp)) in original_events.iter().zip(decompressed_events.iter()).enumerate() {
        // Check coordinates match exactly
        if orig.coord != decomp.coord {
            println!("Event {} coord mismatch: {:?} vs {:?}", i, orig.coord, decomp.coord);
            mismatch_count += 1;
        }

        // Check D value match (contrast)
        if orig.d != decomp.d {
            println!("Event {} d mismatch: {} vs {}", i, orig.d, decomp.d);
            mismatch_count += 1;
        }

        // Check timestamp (allow small error due to lossy compression)
        let orig_t = orig.t;
        let decomp_t = decomp.t;
        let time_diff = (orig_t as i64 - decomp_t as i64).abs();
        max_time_error = max_time_error.max(time_diff);

        if time_diff > 1000 {  // Allow up to 1000 ticks error
            println!("Event {} large time error: {} vs {} (diff: {})",
                i, orig_t, decomp_t, time_diff);
            mismatch_count += 1;
        }
    }

    println!("\n=== VERIFICATION RESULTS ===");
    println!("Total events: {}", original_events.len());
    println!("Mismatches: {}", mismatch_count);
    println!("Max time error: {} ticks", max_time_error);

    if mismatch_count == 0 {
        println!("✅ Round-trip test PASSED - All events match!");
    } else {
        println!("⚠️  Round-trip test completed with {} mismatches", mismatch_count);
    }

    // Clean up
    std::fs::remove_file(temp_file_path).ok();

    // Assert no critical mismatches (coordinates and D must match)
    assert!(
        mismatch_count == 0,
        "Round-trip verification failed with {} mismatches",
        mismatch_count
    );

    Ok(())
}

/// Test all sample files for round-trip compression
#[test]
fn roundtrip_all_samples() -> Result<(), Box<dyn Error>> {
    let files = vec![
        "tests/samples/virat_small_gray.adder",
        "tests/samples/lake_scaled_hd_crop.adder",
        "tests/samples/bunny_crop4.adder",
    ];

    for file in files {
        println!("\n{}", "=".repeat(60));
        println!("Testing: {}", file);
        println!("{}", "=".repeat(60));

        // Read and compress
        let (mut stream, mut bitreader) = open_file_decoder(file)?;
        stream.meta_mut().adu_interval =
            (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;

        let mut original_events = Vec::new();
        loop {
            match stream.digest_event(&mut bitreader) {
                Ok(event) => original_events.push(event),
                Err(CodecError::IoError(_)) => break,
                Err(e) => return Err(Box::new(e)),
            }
        }

        let bufwriter = BufWriter::new(vec![]);
        let compression = CompressedOutput::new(*stream.meta(), bufwriter);
        let mut encoder: Encoder<BufWriter<Vec<u8>>> =
            Encoder::new_compressed(compression, EncoderOptions::default(stream.meta().plane));

        for event in &original_events {
            encoder.ingest_event(*event)?;
        }

        encoder.flush_writer()?;
        let writer = encoder.close_writer()?.unwrap();
        let compressed_data = writer.into_inner()?;

        // Write and decompress
        let temp_path = format!("/tmp/test_{}.adder",
            std::path::Path::new(file).file_stem().unwrap().to_str().unwrap());

        let mut temp_file = File::create(&temp_path)?;
        temp_file.write_all(&compressed_data)?;
        temp_file.sync_all()?;
        drop(temp_file);

        let (mut decoder_stream, mut decoder_bitreader) = open_file_decoder(&temp_path)?;
        let mut decompressed_events = Vec::new();
        loop {
            match decoder_stream.digest_event(&mut decoder_bitreader) {
                Ok(event) => decompressed_events.push(event),
                Err(CodecError::IoError(_)) => break,
                Err(e) => return Err(Box::new(e)),
            }
        }

        println!("Original events: {}", original_events.len());
        println!("Decompressed events: {}", decompressed_events.len());
        println!("Compression ratio: {:.2}%",
            (compressed_data.len() as f64 / (original_events.len() * stream.meta().event_size as usize) as f64) * 100.0);

        assert_eq!(original_events.len(), decompressed_events.len(),
            "Event count mismatch for {}", file);

        // Clean up
        std::fs::remove_file(&temp_path).ok();

        println!("✅ {} passed round-trip test", file);
    }

    Ok(())
}
