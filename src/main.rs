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
            node_url: "http://31.131.21.11:8080".into(),
            wallet_address: "TxWz73zsah3z5m1fLcUvYM63JA8713ajy3".into(),
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

fn log_to_file(msg: &str) {
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("info.log")
        .and_then(|mut f| {
            use std::io::Write;
            let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
            writeln!(f, "[TS: {}] {}", epoch, msg)
        });
}

fn main() -> anyhow::Result<()> {

    const CONFIG_PATH: &str = "config.json";

    let config = if let Ok(data) = fs::read_to_string(CONFIG_PATH) {
        serde_json::from_str(&data).unwrap_or_else(|_| {
            println!("[!] config.json parse error, using defaults settings");
            Config::default()
        })
    } else {
        let c = Config::default();
        let _ = fs::write(CONFIG_PATH, serde_json::to_string_pretty(&c)?);
        c
    };

    let tr = Arc::new(Translator::new(&config.language));

    if config.debug_mode {
        log_to_file("\n--- Miner Started ---");
        std::panic::set_hook(Box::new(|info| {
            let msg = format!("PANIC: {}", info);
            log_to_file(&msg);
            eprintln!("{}", msg);
        }));
    }

    if !std::path::Path::new(CONFIG_PATH).exists() {
        println!("{}", tr.t("cfg_not_found", "[!] config.json not found. Creating a new one with defaults", &[]));
    }

    println!("{}", tr.t("main_title", "=== Trevailo Coin CLI Miner ===", &[]));

    if config.wallet_address.trim().is_empty() {
        println!("{}", tr.t("cfg_no_wallet", "[!] ERROR: Please open config.json and enter your wallet_address!", &[]));
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
        println!("{}", tr.t("err_node_offline", "[!] ERROR: Node is offline or URL is incorrect!", &[]));
        return Ok(());
    } else {
        println!("{}", tr.t("info_node_online", "[+] Node is online.", &[]));
    }

    let handle = MinerHandle::start(
        config.node_url,
        config.wallet_address,
        config.threads,
        config.use_gpu,
        tr.clone(),
    );

    let mut last_hr_tick = Instant::now();

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
                    if config.debug_mode {
                        log_to_file(&s);
                    }
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
                println!("{}", tr.t("miner_stopped", "[!] Miner stopped.", &[]));
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
    }

    Ok(())
}

fn fmt_hr(hps: f64) -> String {
    if hps >= 1_000_000_000.0 { format!("{:.2} GH/s", hps / 1_000_000_000.0) }
    else if hps >= 1_000_000.0 { format!("{:.2} MH/s", hps / 1_000_000.0) }
    else if hps >= 1_000.0 { format!("{:.2} KH/s", hps / 1_000.0) }
    else { format!("{:.0} H/s", hps) }
}
