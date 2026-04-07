use std::env;
use std::path::PathBuf;

use ffmpeg_sidecar::command::FfmpegCommand;
use workspace_root::get_workspace_root;

use std::fs::File;
use std::io::BufReader;
use std::io::{BufWriter, Write};

use bitbit::BitReader;
use bitbit::BitWriter;
use bitbit::MSB;

use toy_ac::decoder::Decoder;
use toy_ac::encoder::Encoder;
use toy_ac::symbol_model::VectorCountSymbolModel;

use ffmpeg_sidecar::event::StreamTypeSpecificData::Video;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Make sure ffmpeg is installed
    ffmpeg_sidecar::download::auto_download().unwrap();

    // Command line options
    // -verbose, -no_verbose                Default: -no_verbose
    // -report, -no_report                  Default: -report
    // -check_decode, -no_check_decode      Default: -no_check_decode
    // -skip_count n                        Default: -skip_count 0
    // -count n                             Default: -count 10
    // -in file_path                        Default: bourne.mp4 in data subdirectory of workplace
    // -out file_path                       Default: out.dat in data subdirectory of workplace

    let mut verbose = false;
    let mut report = true;
    let mut check_decode = false;
    let mut skip_count = 0;
    let mut count = 10;

    let mut data_folder_path = get_workspace_root();
    data_folder_path.push("data");

    let mut input_file_path = data_folder_path.join("bourne.mp4");
    let mut output_file_path = data_folder_path.join("out.dat");

    parse_args(
        &mut verbose,
        &mut report,
        &mut check_decode,
        &mut skip_count,
        &mut count,
        &mut input_file_path,
        &mut output_file_path,
    );

    let mut iter = FfmpegCommand::new()
        .input(input_file_path.to_str().unwrap())
        .format("rawvideo")
        .pix_fmt("gray8")
        .output("-")
        .spawn()?
        .iter()?;

    
    let mut width = 0;
    let mut height = 0;

    let metadata = iter.collect_metadata()?;
    for i in 0..metadata.output_streams.len() {
        match &metadata.output_streams[i].type_specific_data {
            Video(vid_stream) => {
                width = vid_stream.width;
                height = vid_stream.height;

                if verbose {
                    println!(
                        "Found video stream at output stream index {} with dimensions {} x {}",
                        i, width, height
                    );
                }
                break;
            }
            _ => (),
        }
    }

    assert!(width != 0);
    assert!(height != 0);

    let mut prior_frame = vec![128u8; (width * height) as usize];

    let output_file = match File::create(&output_file_path) {
        Err(_) => panic!("Error opening output file"),
        Ok(f) => f,
    };

    let mut buf_writer = BufWriter::new(output_file);
    let mut bw = BitWriter::new(&mut buf_writer);
    let mut enc = Encoder::new();


    let mut pixel_difference_pdfs: Vec<VectorCountSymbolModel<i32>> = (0..16)
        .map(|_| VectorCountSymbolModel::new((0..=255).collect()))
        .collect();

    // the encoding of the frames
    for frame in iter.filter_frames() {
        if frame.frame_num < skip_count {
            if verbose {
                println!("Skipping frame {}", frame.frame_num);
            }
        } else if frame.frame_num < skip_count + count {
            let current_frame: Vec<u8> = frame.data;

            let bits_written_at_start = enc.bits_written();

            for r in 0..height {
                for c in 0..width {
                    let pixel_index = (r * width + c) as usize;

                    let prior = prior_frame[pixel_index];

                    let left = if c > 0 {
                        current_frame[(r * width + (c - 1)) as usize]
                    } else {
                        prior
                    };

                    let up = if r > 0 {
                        current_frame[((r - 1) * width + c) as usize]
                    } else {
                        prior
                    };

                    // predicting using the average of left pixel, upwards pixel, and prior pixel
                    let prediction = ((left as u16 + up as u16 + prior as u16) / 3) as u8;

                    // setting range as between 0..255
                    let pixel_difference =
                        (((current_frame[pixel_index] as i32) - (prediction as i32)) + 256) % 256;

        
                    let context = (left.abs_diff(up) / 16) as usize; // 0..15

                    enc.encode(&pixel_difference, &pixel_difference_pdfs[context], &mut bw);
                    pixel_difference_pdfs[context].incr_count(&pixel_difference);
                }
            }

            prior_frame = current_frame;

            let bits_written_at_end = enc.bits_written();

            if verbose {
                println!(
                    "frame: {}, compressed size (bits): {}",
                    frame.frame_num,
                    bits_written_at_end - bits_written_at_start
                );
            }
        } else {
            break;
        }
    }

    // encoding
    enc.finish(&mut bw)?;
    bw.pad_to_byte()?;
    buf_writer.flush()?;

    // checking correctness and decoding
    if check_decode {
        let output_file = match File::open(&output_file_path) {
            Err(_) => panic!("Error opening output file"),
            Ok(f) => f,
        };

        let mut buf_reader = BufReader::new(output_file);
        let mut br: BitReader<_, MSB> = BitReader::new(&mut buf_reader);

        let iter = FfmpegCommand::new()
            .input(input_file_path.to_str().unwrap())
            .format("rawvideo")
            .pix_fmt("gray8")
            .output("-")
            .spawn()?
            .iter()?;

        let mut dec = Decoder::new();

        let mut pixel_difference_pdfs: Vec<VectorCountSymbolModel<i32>> = (0..16)
            .map(|_| VectorCountSymbolModel::new((0..=255).collect()))
            .collect();

        let mut prior_frame = vec![128u8; (width * height) as usize];

        'outer_loop: for frame in iter.filter_frames() {
            if frame.frame_num < skip_count + count {
                if verbose {
                    print!("Checking frame: {} ... ", frame.frame_num);
                }

                let current_frame: Vec<u8> = frame.data;
                let mut reconstructed_frame = vec![0u8; (width * height) as usize];

                for r in 0..height {
                    for c in 0..width {
                        let pixel_index = (r * width + c) as usize;

                        let prior = prior_frame[pixel_index];

                        let left = if c > 0 {
                            reconstructed_frame[(r * width + (c - 1)) as usize]
                        } else {
                            prior
                        };

                        let up = if r > 0 {
                            reconstructed_frame[((r - 1) * width + c) as usize]
                        } else {
                            prior
                        };

                        let prediction = ((left as u16 + up as u16 + prior as u16) / 3) as u8;
                        let context = (left.abs_diff(up) / 16) as usize;

                        let decoded_pixel_difference =
                            dec.decode(&pixel_difference_pdfs[context], &mut br).to_owned();

                        pixel_difference_pdfs[context].incr_count(&decoded_pixel_difference);

                        let pixel_value =
                            ((prediction as i32 + decoded_pixel_difference) % 256) as u8;

                        reconstructed_frame[pixel_index] = pixel_value;

                        if pixel_value != current_frame[pixel_index] {
                            println!(
                                " error at ({}, {}), should decode {}, got {}",
                                c, r, current_frame[pixel_index], pixel_value
                            );
                            println!("Abandoning check of remaining frames");
                            break 'outer_loop;
                        }
                    }
                }

                println!("correct.");
                prior_frame = reconstructed_frame;
            } else {
                break 'outer_loop;
            }
        }
    }

    // Report
    if report {
        println!(
            "{} frames encoded, average size (bits): {}, compression ratio: {:.2}",
            count,
            enc.bits_written() / count as u64,
            (width * height * 8 * count) as f64 / enc.bits_written() as f64
        )
    }

    Ok(())
}

fn parse_args(
    verbose: &mut bool,
    report: &mut bool,
    check_decode: &mut bool,
    skip_count: &mut u32,
    count: &mut u32,
    input_file_path: &mut PathBuf,
    output_file_path: &mut PathBuf,
) -> () {
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "-verbose" {
            *verbose = true;
        } else if arg == "-no_verbose" {
            *verbose = false;
        } else if arg == "-report" {
            *report = true;
        } else if arg == "-no_report" {
            *report = false;
        } else if arg == "-check_decode" {
            *check_decode = true;
        } else if arg == "-no_check_decode" {
            *check_decode = false;
        } else if arg == "-skip_count" {
            match args.next() {
                Some(skip_count_string) => {
                    *skip_count = skip_count_string.parse::<u32>().unwrap();
                }
                None => {
                    panic!("Expected count after -skip_count option");
                }
            }
        } else if arg == "-count" {
            match args.next() {
                Some(count_string) => {
                    *count = count_string.parse::<u32>().unwrap();
                }
                None => {
                    panic!("Expected count after -count option");
                }
            }
        } else if arg == "-in" {
            match args.next() {
                Some(input_file_path_string) => {
                    *input_file_path = PathBuf::from(input_file_path_string);
                }
                None => {
                    panic!("Expected input file name after -in option");
                }
            }
        } else if arg == "-out" {
            match args.next() {
                Some(output_file_path_string) => {
                    *output_file_path = PathBuf::from(output_file_path_string);
                }
                None => {
                    panic!("Expected output file name after -out option");
                }
            }
        }
    }
}