use clap::Parser;
use logfather::{Level, Logger};
use num_format::{Locale, ToFormattedString};
use rand::{distributions::Alphanumeric, Rng};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use sha2::{Digest, Sha256};
use solana_pubkey::Pubkey;
use reqwest::blocking::Client;
use std::thread::sleep;
use std::time::Duration;
use serde_json::json;
use std::{
    array,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

#[derive(Debug, Parser)]
pub struct GrindArgs {
    // Add a return url
    #[clap(long, value_parser = parse_url)]
    pub return_url: String,

    #[clap(long)]
    pub uuid: Option<String>,

    /// The pubkey that will be the signer for the CreateAccountWithSeed instruction
    #[clap(long, value_parser = parse_pubkey)]
    pub base: Pubkey,

    /// The account owner, e.g. BPFLoaderUpgradeab1e11111111111111111111111 or TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
    #[clap(long, value_parser = parse_pubkey)]
    pub owner: Pubkey,

    /// The target prefix for the pubkey
    #[clap(long)]
    pub prefix: Option<String>,

    #[clap(long)]
    pub suffix: Option<String>,

    /// Whether user cares about the case of the pubkey
    #[clap(long, default_value_t = false)]
    pub case_insensitive: bool,

    /// Optional log file
    #[clap(long)]
    pub logfile: Option<String>,

    /// Number of gpus to use for mining
    #[clap(long, default_value_t = 1)]
    #[cfg(feature = "gpu")]
    pub num_gpus: u32,

    /// Number of cpu threads to use for mining
    #[clap(long, default_value_t = 0)]
    pub num_cpus: u32,
}

static EXIT: AtomicBool = AtomicBool::new(false);

fn main() {
    rayon::ThreadPoolBuilder::new().build_global().unwrap();

    // Parse command line arguments
    let args = GrindArgs::parse();
    grind(args);
}

fn grind(mut args: GrindArgs) {
    maybe_update_num_cpus(&mut args.num_cpus);
    let prefix = get_validated_prefix(&args);
    let suffix = get_validated_suffix(&args);

    // Initialize logger with optional logfile
    let mut logger = Logger::new();
    if let Some(ref logfile) = args.logfile {
        logger.file(true);
        logger.path(logfile);
    }

    // Slightly more compact log format
    logger.log_format("[{timestamp} {level}] {message}");
    logger.timestamp_format("%Y-%m-%d %H:%M:%S");
    logger.level(Level::Info);

    // Print resource usage
    logfather::info!("using {} threads", args.num_cpus);
    #[cfg(feature = "gpu")]
    logfather::info!("using {} gpus", args.num_gpus);

    #[cfg(feature = "gpu")]
    let _gpu_threads: Vec<_> = (0..args.num_gpus)
        .map(move |gpu_index| {
            std::thread::Builder::new()
                .name(format!("gpu{gpu_index}"))
                .spawn(move || {
                    logfather::trace!("starting gpu {gpu_index}");

                    let mut out = [0; 24];
                    for iteration in 0_u64.. {
                        // Exit if a thread found a solution
                        if EXIT.load(Ordering::SeqCst) {
                            logfather::trace!("gpu thread {gpu_index} exiting");
                            return;
                        }

                        // Generate new seed for this gpu & iteration
                        let seed = new_gpu_seed(gpu_index, iteration);
                        let timer = Instant::now();
                        unsafe {
                            vanity_round(gpu_index, seed.as_ref().as_ptr(), args.base.to_bytes().as_ptr(), args.owner.to_bytes().as_ptr(), prefix.as_ptr(), suffix.as_ptr(), prefix.len() as u64, suffix.len() as u64,out.as_mut_ptr(), args.case_insensitive);
                        }
                        let time_sec = timer.elapsed().as_secs_f64();

                        // Reconstruct solution
                        let reconstructed: [u8; 32] = Sha256::new()
                            .chain_update(&args.base)
                            .chain_update(&out[..16])
                            .chain_update(&args.owner)
                            .finalize()
                            .into();
                        let out_str = fd_bs58::encode_32(reconstructed);
                        let out_str_target_check = maybe_bs58_aware_lowercase(&out_str, args.case_insensitive);
                        let count = u64::from_le_bytes(array::from_fn(|i| out[16 + i]));
                        logfather::info!(
                            "{} found in {:.3} seconds on gpu {gpu_index:>3}; {:>13} iters; {:>12} iters/sec",
                            &out_str,
                            time_sec,
                            count.to_formatted_string(&Locale::en),
                            ((count as f64 / time_sec) as u64).to_formatted_string(&Locale::en)
                        );

                        if out_str_target_check.starts_with(prefix) && out_str_target_check.ends_with(suffix) {
                            logfather::info!("out seed = {out:?} -> {}", core::str::from_utf8(&out[..16]).unwrap());
                            EXIT.store(true, Ordering::SeqCst);

                            // Send result to server
                            let url = &args.return_url;
                            let max_retries = 5;

                            for _ in 0..max_retries {
                                let client = Client::new(); // New client each time
                                
                                let payload = json!({
                                    "pubkey": out_str,
                                    "seed": core::str::from_utf8(&out[..16]).unwrap(),
                                    "seed_bytes": &out[..16],
                                    "count": count,
                                    "time_secs": time_sec,
                                    "uuid": args.uuid.as_deref().unwrap_or(""),
                                });

                                let res = client.post(url).json(&payload).send();

                                match res {
                                    Ok(resp) if resp.status() == 200 => {
                                        println!("Success!");
                                        break;
                                    }
                                    _ => {
                                        println!("Retrying...");
                                        sleep(Duration::from_millis(1500));
                                    }
                                }
                            }

                            logfather::trace!("gpu thread {gpu_index} exiting");
                            return;
                        }
                    }
                })
                .unwrap()
        })
        .collect();

    (0..args.num_cpus).into_par_iter().for_each(|i| {
        let timer = Instant::now();
        let mut count = 0_u64;

        let base_sha = Sha256::new().chain_update(args.base);
        loop {
            if EXIT.load(Ordering::Acquire) {
                return;
            }

            let mut seed_iter = rand::thread_rng().sample_iter(&Alphanumeric).take(16);
            let seed: [u8; 16] = array::from_fn(|_| seed_iter.next().unwrap());

            let pubkey_bytes: [u8; 32] = base_sha
                .clone()
                .chain_update(seed)
                .chain_update(args.owner)
                .finalize()
                .into();
            let pubkey = fd_bs58::encode_32(pubkey_bytes);
            let out_str_target_check = maybe_bs58_aware_lowercase(&pubkey, args.case_insensitive);

            count += 1;

            // Did cpu find target?
            if out_str_target_check.starts_with(prefix) && out_str_target_check.ends_with(suffix) {
                let time_secs = timer.elapsed().as_secs_f64();
                logfather::info!(
                    "cpu {i} found target: {pubkey}; {seed:?} -> {} in {:.3}s; {} attempts; {} attempts per second",
                    core::str::from_utf8(&seed).unwrap(),
                    time_secs,
                    count.to_formatted_string(&Locale::en),
                    ((count as f64 / time_secs) as u64).to_formatted_string(&Locale::en)
                );

                EXIT.store(true, Ordering::Release);

                // Send result to server
                let url = &args.return_url;
                let max_retries = 5;

                for _ in 0..max_retries {
                    let client = Client::new(); // New client each time
                    
                    let payload = json!({
                        "pubkey": pubkey,
                        "seed": core::str::from_utf8(&seed).unwrap(),
                        "seed_bytes": &seed,
                        "count": count,
                        "time_secs": time_secs,
                        "uuid": args.uuid.as_deref().unwrap_or(""),
                    });

                    let res = client.post(url).json(&payload).send();

                    match res {
                        Ok(resp) if resp.status() == 200 => {
                            println!("Success!");
                            break;
                        }
                        _ => {
                            println!("Retrying...");
                            sleep(Duration::from_millis(1500));
                        }
                    }
                }

                break;
            }
        }
    });
}

fn get_validated_prefix(args: &GrindArgs) -> &'static str {
    // Static string of BS58 characters
    const BS58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    // Validate target (i.e. does it include 0, O, I, l)
    //
    // maybe TODO: technically we could accept I or o if case-insensitivity but I suspect
    // most users will provide lowercase targets for case-insensitive searches

    if let Some(ref prefix) = args.prefix {
        for c in prefix.chars() {
            assert!(
                BS58_CHARS.contains(c),
                "your prefix contains invalid bs58: {}",
                c
            );
        }
        let prefix = maybe_bs58_aware_lowercase(&prefix, args.case_insensitive);
        return prefix.leak()
    }
    ""
}

fn get_validated_suffix(args: &GrindArgs) -> &'static str {
    // Static string of BS58 characters
    const BS58_CHARS: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

    // Validate target (i.e. does it include 0, O, I, l)
    //
    // maybe TODO: technically we could accept I or o if case-insensitivity but I suspect
    // most users will provide lowercase targets for case-insensitive searches

    if let Some(ref suffix) = args.suffix {
        for c in suffix.chars() {
            assert!(
                BS58_CHARS.contains(c),
                "your suffix contains invalid bs58: {}",
                c
            );
        }
        let suffix = maybe_bs58_aware_lowercase(&suffix, args.case_insensitive);
        return suffix.leak()
    }
    ""
}

fn maybe_bs58_aware_lowercase(target: &str, case_insensitive: bool) -> String {
    // L is only char that shouldn't be converted to lowercase in case-insensitivity case
    const LOWERCASE_EXCEPTIONS: &str = "L";

    if case_insensitive {
        target
            .chars()
            .map(|c| {
                if LOWERCASE_EXCEPTIONS.contains(c) {
                    c
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect::<String>()
    } else {
        target.to_string()
    }
}

extern "C" {
    pub fn vanity_round(
        gpus: u32,
        seed: *const u8,
        base: *const u8,
        owner: *const u8,
        target: *const u8,
        suffix: *const u8,
        target_len: u64,
        suffix_len: u64,
        out: *mut u8,
        case_insensitive: bool,
    );
}

#[cfg(feature = "gpu")]
fn new_gpu_seed(gpu_id: u32, iteration: u64) -> [u8; 32] {
    Sha256::new()
        .chain_update(rand::random::<[u8; 32]>())
        .chain_update(gpu_id.to_le_bytes())
        .chain_update(iteration.to_le_bytes())
        .finalize()
        .into()
}

fn parse_url(input: &str) -> Result<String, String> {
    if input.starts_with("http://") || input.starts_with("https://") {
        Ok(input.to_string())
    } else {
        Err("URL must start with http:// or https://".to_string())
    }
}

fn parse_pubkey(input: &str) -> Result<Pubkey, String> {
    Pubkey::from_str(input).map_err(|e| e.to_string())
}

fn maybe_update_num_cpus(num_cpus: &mut u32) {
    if *num_cpus == 0 {
        *num_cpus = rayon::current_num_threads() as u32;
    }
}
