extern crate adder_codec_core;

use adder_codec_core::codec::compressed::source_model::entropy_analysis::EntropyAnalyzer;
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

/// Analyze the entropy distribution of events in the sample file to evaluate
/// potential compression gains from context switching.
#[test]
fn test_entropy_analysis() -> Result<(), Box<dyn Error>> {
    // Open the virat_small_gray.adder sample file
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;

    // Create the entropy analyzer with the reference interval from the file
    let mut analyzer = EntropyAnalyzer::new(stream.meta().ref_interval);

    let mut event_count: u64 = 0;
    loop {
        let res = stream.digest_event(&mut bitreader);
        match res {
            Ok(event) => {
                analyzer.process_event(&event);
                event_count += 1;
            }
            Err(CodecError::IoError(_e)) => {
                break;
            }
            Err(e) => return Err(Box::new(e)),
        }
    }

    // Generate and print the report
    let report = analyzer.generate_report();
    println!("\n{}", report);

    // Basic sanity checks
    assert_eq!(report.total_events, event_count);
    assert!(report.global_d_entropy > 0.0, "D entropy should be positive");

    // Print summary recommendations
    println!("\n=== RECOMMENDATIONS ===");

    let best_d_savings = report.d_by_trend_potential_savings
        .max(report.d_by_magnitude_potential_savings)
        .max(report.d_by_density_potential_savings);

    if best_d_savings > 5.0 {
        println!("D context switching looks promising! Best potential: {:.2}% reduction", best_d_savings);
        if report.d_by_trend_potential_savings == best_d_savings {
            println!("  -> Recommended approach: Switch context by D trend (rising/falling/stable)");
        } else if report.d_by_magnitude_potential_savings == best_d_savings {
            println!("  -> Recommended approach: Switch context by previous D magnitude");
        } else {
            println!("  -> Recommended approach: Switch context by pixel event density");
        }
    } else if best_d_savings > 2.0 {
        println!("D context switching may provide modest gains (~{:.2}%)", best_d_savings);
    } else {
        println!("D context switching unlikely to help significantly ({:.2}%)", best_d_savings);
        println!("The current single-context approach may already be near optimal for this data.");
    }

    let best_t_savings = report.t_by_magnitude_potential_savings
        .max(report.t_by_trend_potential_savings);

    if best_t_savings > 5.0 {
        println!("\nT context switching looks promising! Best potential: {:.2}% reduction", best_t_savings);
    }

    Ok(())
}

#[test]
fn test_compression_ratio() -> Result<(), Box<dyn Error>> {
    // Reset global context stats at the start
    adder_codec_core::codec::compressed::source_model::cabac_contexts::reset_global_context_stats();

    let file_path: &str = "tests/samples/virat_small_gray.adder";

    // Open the virat_small_gray.adder sample file as a RawInput
    let (mut stream, mut bitreader) = open_file_decoder(file_path)?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;

    let original_file_size = std::fs::metadata(file_path)?.len();
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

    // Print global context usage statistics for the entire video
    adder_codec_core::codec::compressed::source_model::cabac_contexts::print_global_context_stats();

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
    println!("Raw event stream size:     {} bytes ({} events × {} bytes)",
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
