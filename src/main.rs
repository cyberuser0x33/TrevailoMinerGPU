mod miner;
mod http;

mod gpu_kernel;

use miner::{MinerHandle, MineEvent};
use std::time::{Duration, Instant};
use std::fs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    node_url: String,
    wallet_address: String,
    threads: usize,
    use_gpu: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_url: "http://31.131.21.11:8080".into(),
            wallet_address: "".into(),
            threads: cpu_count(),
            use_gpu: true,
        }
    }
}

fn cpu_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

fn main() -> anyhow::Result<()> {
    println!("=== Trevailo Coin CLI Miner ===");
    let config_path = "config.json";

    let config = if let Ok(data) = fs::read_to_string(config_path) {
        serde_json::from_str(&data).unwrap_or_else(|_| {
            println!("[!] Ошибка парсинга config.json, используются настройки по умолчанию.");
            Config::default()
        })
    } else {
        println!("[!] Файл config.json не найден. Создаю новый со стандартными настройками...");
        let c = Config::default();
        fs::write(config_path, serde_json::to_string_pretty(&c)?)?;
        c
    };

    if config.wallet_address.trim().is_empty() {
        println!("[!] ОШИБКА: Пожалуйста, откройте config.json и впишите ваш wallet_address!");
        return Ok(());
    }

    println!("[*] Нода: {}", config.node_url);
    println!("[*] Кошелек: {}", config.wallet_address);
    println!("[*] Потоков: {}", config.threads);
    println!("[*] Использовать GPU: {}", if config.use_gpu { "Да" } else { "Нет" });
    println!("--------------------------------");

    let handle = MinerHandle::start(
        config.node_url,
        config.wallet_address,
        config.threads,
        config.use_gpu,
    );

    let mut last_hr_tick = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(100));

        // События майнера
        while let Some(ev) = handle.try_recv() {
            match ev {
                MineEvent::NewTemplate { height, difficulty } => {
                    println!("[I] Шаблон блока #{} (сложность: {})", height, difficulty);
                }
                MineEvent::BlockFound { height, nonce, elapsed_secs } => {
                    println!("[+] Блок #{} найден! nonce={} за {:.1}с", height, nonce, elapsed_secs);
                }
                MineEvent::BlockAccepted { height, reward_tvc, txs } => {
                    println!("[$] ★ БЛОК #{} ПРИНЯТ! Вознаграждение: +{:.4} TVC | {} транзакций", height, reward_tvc, txs);
                }
                MineEvent::BlockRejected { reason } => {
                    println!("[-] Блок отклонён: {}", reason);
                }
                MineEvent::TemplateSwap { old_height, new_height } => {
                    if old_height != new_height {
                        println!("[~] ⚡ Блок #{} смайнен кем-то другим. Переключаюсь на #{}", old_height, new_height);
                    }
                }
                MineEvent::Error { msg } => {
                    println!("[!] Ошибка: {}", msg);
                }
            }
        }

        // Статистика
        if last_hr_tick.elapsed() >= Duration::from_secs(5) {
            last_hr_tick = Instant::now();
            let stats = handle.snapshot();
            if !stats.running {
                println!("[!] Майнер остановился.");
                break;
            }
            if stats.current_height > 0 {
                println!(
                    "[STAT] Hashrate: {} | Высота: #{} | Найдено: {} | Награда: {:.4} TVC",
                    fmt_hr(stats.hashrate),
                    stats.current_height,
                    stats.blocks_found,
                    stats.reward_total
                );
            }
        }
    }

    Ok(())
}

fn fmt_hr(hps: f64) -> String {
    if hps >= 1_000_000.0 { format!("{:.2} MH/s", hps / 1_000_000.0) }
    else if hps >= 1_000.0 { format!("{:.2} KH/s", hps / 1_000.0) }
    else { format!("{:.0} H/s", hps) }
}
