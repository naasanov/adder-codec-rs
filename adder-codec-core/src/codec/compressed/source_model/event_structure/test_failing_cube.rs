use super::event_cube::EventCube;
use crate::codec::compressed::fenwick::context_switching::FenwickModel;
use crate::codec::compressed::source_model::cabac_contexts::eof_context;
use crate::codec::compressed::source_model::{ComponentCompression, HandleEvent};
use crate::open_file_decoder;
use arithmetic_coding_adder_dep::Encoder;
use bitstream_io::{BigEndian, BitReader, BitWriter};
use std::error::Error;
use std::io::Cursor;

#[test]
fn test_failing_cube_from_file() -> Result<(), Box<dyn Error>> {
    // Read events from the actual file
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;
    let meta = *stream.meta();

    // Read ALL events from first ADU
    let mut all_events = Vec::new();
    let adu_time_span = meta.ref_interval * meta.adu_interval as u32;
    loop {
        match stream.digest_event(&mut bitreader) {
            Ok(event) => {
                if event.t >= adu_time_span {
                    break; // Stop after first ADU
                }
                all_events.push(event);
            }
            Err(crate::codec::CodecError::IoError(_)) => break,
            Err(e) => return Err(Box::new(e)),
        }
    }

    println!("Read {} events from first ADU", all_events.len());

    // Calculate grid dimensions
    let plane = meta.plane;
    let cubes_x = (plane.w() + 15) / 16;
    let cubes_y = (plane.h() + 15) / 16;
    println!("Grid: {}x{} cubes", cubes_x, cubes_y);

    // Test each cube individually
    for cube_y in 0..cubes_y {
        for cube_x in 0..cubes_x {
            let start_x = cube_x * 16;
            let start_y = cube_y * 16;
            let start_t = 0u32;

            let mut cube = EventCube::new(
                start_y,
                start_x,
                1,
                start_t,
                meta.ref_interval,
                meta.adu_interval,
            );

            // Ingest events that belong to this cube
            let mut event_count = 0;
            for event in all_events.iter() {
                let evt_x = event.coord.x;
                let evt_y = event.coord.y;

                // Check if event belongs to this cube
                if evt_x >= start_x
                    && evt_x < start_x + 16
                    && evt_y >= start_y
                    && evt_y < start_y + 16
                {
                    cube.ingest_event(*event);
                    event_count += 1;
                }
            }

            if event_count == 0 && cube_x > 0 && cube_y > 0 {
                continue; // Skip empty cubes except (0,0)
            }

            println!(
                "Testing cube [{},{}] with {} events",
                cube_y, cube_x, event_count
            );

            // Compress
            let bufwriter = Vec::new();
            let mut stream_writer = BitWriter::endian(bufwriter, BigEndian);

            let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
            let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
                &mut source_model,
                meta.ref_interval,
            );

            let mut encoder = Encoder::new(source_model);

            cube.compress_intra(&mut encoder, &contexts, &mut stream_writer, None)?;
            eof_context(&contexts, &mut encoder, &mut stream_writer);

            let compressed = stream_writer.into_writer();

            // Decompress
            let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
            let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
                &mut source_model,
                meta.ref_interval,
            );
            let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
            let mut stream_reader = BitReader::endian(Cursor::new(compressed), BigEndian);

            let mut cube2 = cube.clone();

            cube2.decompress_intra(&mut decoder, &contexts, &mut stream_reader, start_t);

            // Verify
            for c in 0..1 {
                for y in 0..16 {
                    for x in 0..16 {
                        if !cube.raw_event_lists[c][y][x].is_empty() {
                            assert!(!cube2.raw_event_lists[c][y][x].is_empty(),
                                "Cube [{},{}] mismatch at [{c}][{y}][{x}]: original has event, decoded is empty", cube_y, cube_x);
                            assert_eq!(
                                cube.raw_event_lists[c][y][x][0].d,
                                cube2.raw_event_lists[c][y][x][0].d,
                                "Cube [{},{}] mismatch at [{c}][{y}][{x}]: d values differ",
                                cube_y,
                                cube_x
                            );
                            let t_diff = (cube.raw_event_lists[c][y][x][0].t as i64
                                - cube2.raw_event_lists[c][y][x][0].t as i64)
                                .abs();
                            assert!(t_diff <= 100,
                                "Cube [{},{}] mismatch at [{c}][{y}][{x}]: t values differ by {t_diff}", cube_y, cube_x);
                        } else {
                            assert!(cube2.raw_event_lists[c][y][x].is_empty(),
                                "Cube [{},{}] mismatch at [{c}][{y}][{x}]: original is empty, decoded has event", cube_y, cube_x);
                        }
                    }
                }
            }
        }
    }

    println!("All cubes passed!");
    Ok(())
}

#[test]
fn test_all_cubes_shared_encoder() -> Result<(), Box<dyn Error>> {
    // Read events from the actual file
    let (mut stream, mut bitreader) = open_file_decoder("tests/samples/virat_small_gray.adder")?;
    stream.meta_mut().adu_interval =
        (stream.meta().delta_t_max / stream.meta().ref_interval) as usize;
    let meta = *stream.meta();

    // Read ALL events from first ADU
    let mut all_events = Vec::new();
    let adu_time_span = meta.ref_interval * meta.adu_interval as u32;
    loop {
        match stream.digest_event(&mut bitreader) {
            Ok(event) => {
                if event.t >= adu_time_span {
                    break; // Stop after first ADU
                }
                all_events.push(event);
            }
            Err(crate::codec::CodecError::IoError(_)) => break,
            Err(e) => return Err(Box::new(e)),
        }
    }

    println!("Read {} events from first ADU", all_events.len());

    // Calculate grid dimensions
    let plane = meta.plane;
    let cubes_x = (plane.w() + 15) / 16;
    let cubes_y = (plane.h() + 15) / 16;
    println!("Grid: {}x{} cubes", cubes_x, cubes_y);

    // Create all cubes and populate them
    let mut cubes = Vec::new();
    for cube_y in 0..cubes_y {
        for cube_x in 0..cubes_x {
            let start_x = cube_x * 16;
            let start_y = cube_y * 16;
            let start_t = 0u32;

            let mut cube = EventCube::new(
                start_y,
                start_x,
                1,
                start_t,
                meta.ref_interval,
                meta.adu_interval,
            );

            // Ingest events that belong to this cube
            for event in all_events.iter() {
                let evt_x = event.coord.x;
                let evt_y = event.coord.y;

                if evt_x >= start_x
                    && evt_x < start_x + 16
                    && evt_y >= start_y
                    && evt_y < start_y + 16
                {
                    cube.ingest_event(*event);
                }
            }

            cubes.push(cube);
        }
    }

    // Compress ALL cubes with SHARED encoder (like ADU does)
    let bufwriter = Vec::new();
    let mut stream_writer = BitWriter::endian(bufwriter, BigEndian);

    let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
    let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
        &mut source_model,
        meta.ref_interval,
    );

    let mut encoder = Encoder::new(source_model);

    println!("Compressing all cubes with shared encoder...");
    let total_cubes = cubes.len();
    for (i, cube) in cubes.iter_mut().enumerate() {
        println!("Compressing cube {}/{}", i + 1, total_cubes);
        cube.compress_intra(&mut encoder, &contexts, &mut stream_writer, None)?;
    }
    eof_context(&contexts, &mut encoder, &mut stream_writer);

    let compressed = stream_writer.into_writer();
    println!("Compressed to {} bytes", compressed.len());

    // Decompress ALL cubes with SHARED decoder
    let mut source_model = FenwickModel::with_symbols(u16::MAX as usize, 1 << 30);
    let contexts = crate::codec::compressed::source_model::cabac_contexts::Contexts::new(
        &mut source_model,
        meta.ref_interval,
    );
    let mut decoder = arithmetic_coding_adder_dep::Decoder::new(source_model);
    let mut stream_reader = BitReader::endian(Cursor::new(compressed), BigEndian);

    println!("Decompressing all cubes with shared decoder...");
    let mut decoded_cubes = Vec::new();
    for (i, original_cube) in cubes.iter().enumerate() {
        println!("Decompressing cube {}/{}", i + 1, cubes.len());
        let mut cube2 = original_cube.clone();
        cube2.decompress_intra(&mut decoder, &contexts, &mut stream_reader, 0);
        decoded_cubes.push(cube2);
    }

    // Verify all cubes
    println!("Verifying all cubes...");
    for (i, (orig, dec)) in cubes.iter().zip(decoded_cubes.iter()).enumerate() {
        for c in 0..1 {
            for y in 0..16 {
                for x in 0..16 {
                    if !orig.raw_event_lists[c][y][x].is_empty() {
                        assert!(!dec.raw_event_lists[c][y][x].is_empty(),
                            "Cube {i} mismatch at [{c}][{y}][{x}]: original has event, decoded is empty");
                        assert_eq!(
                            orig.raw_event_lists[c][y][x][0].d,
                            dec.raw_event_lists[c][y][x][0].d,
                            "Cube {i} mismatch at [{c}][{y}][{x}]: d values differ"
                        );
                        let t_diff = (orig.raw_event_lists[c][y][x][0].t as i64
                                    - dec.raw_event_lists[c][y][x][0].t as i64)
                            .abs();
                        assert!(t_diff <= 100,
                            "Cube {i} mismatch at [{c}][{y}][{x}]: t values differ by {t_diff}");
                    } else {
                        assert!(dec.raw_event_lists[c][y][x].is_empty(),
                            "Cube {i} mismatch at [{c}][{y}][{x}]: original is empty, decoded has event");
                    }
                }
            }
        }
    }

    println!("All cubes passed with shared encoder/decoder!");
    Ok(())
}
