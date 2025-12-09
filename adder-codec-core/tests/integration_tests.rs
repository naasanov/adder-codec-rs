extern crate adder_codec_core;

use adder_codec_core::codec::compressed::stream::{CompressedInput, CompressedOutput};
use adder_codec_core::codec::decoder::Decoder;
use adder_codec_core::codec::encoder::Encoder;

use adder_codec_core::codec::{CodecError, EncoderOptions};
use adder_codec_core::open_file_decoder;
use bitstream_io::{BigEndian, BitReader};
use std::error::Error;
use std::io::{BufReader, BufWriter,Write};
use std::fs::File;


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
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/lake_scaled_hd_crop.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;

    let original_file_size = std::fs::metadata("tests/samples/lake_scaled_hd_crop.adder")?.len();

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
