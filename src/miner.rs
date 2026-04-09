use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::http::NodeClient;

const TEMPLATE_POLL_SECS: u64 = 5;
const BATCH_SIZE_CPU: u64 = 50_000;
const BATCH_SIZE_GPU: usize = 1_048_576; // 1 миллион хэшей за оборот

#[derive(Debug, Clone)]
pub enum MineEvent {
    NewTemplate { height: u64, difficulty: u32 },
    BlockFound { height: u64, nonce: u64, elapsed_secs: f64 },
    BlockAccepted { height: u64, reward_tvc: f64, txs: usize },
    BlockRejected { reason: String },
    TemplateSwap { old_height: u64, new_height: u64 },
    Error { msg: String },
}

#[derive(Clone, Default)]
pub struct MinerState {
    pub running: bool,
    pub hashrate: f64,
    pub total_hashes: u64,
    pub blocks_found: u64,
    pub current_height: u64,
    pub difficulty: u32,
    pub nonce: u64,
    pub uptime_secs: u64,
    pub reward_total: f64,
}

pub struct MinerHandle {
    stop_flag: Arc<AtomicBool>,
    state: Arc<Mutex<MinerState>>,
    events: Arc<Mutex<Vec<MineEvent>>>,
}

impl MinerHandle {
    pub fn start(
        node_url: String,
        address: String,
        num_threads: usize,
        use_gpu: bool,
    ) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(MinerState { running: true, ..Default::default() }));
        let events = Arc::new(Mutex::new(Vec::<MineEvent>::new()));

        let (sf, st, ev) = (stop_flag.clone(), state.clone(), events.clone());
        thread::spawn(move || mining_main(node_url, address, num_threads, use_gpu, sf, st, ev));

        MinerHandle { stop_flag, state, events }
    }

    #[allow(dead_code)]
    pub fn stop(self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    pub fn snapshot(&self) -> MinerState {
        self.state.lock().unwrap().clone()
    }

    pub fn try_recv(&self) -> Option<MineEvent> {
        let mut v = self.events.lock().unwrap();
        if v.is_empty() { None } else { Some(v.remove(0)) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlockHeader {
    version: u32,
    height: u64,
    prev_hash: String,
    merkle_root: String,
    timestamp: u64,
    difficulty: u32,
    nonce: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Block {
    header: BlockHeader,
    transactions: Vec<serde_json::Value>,
    hash: String,
}

fn header_hash_string(h: &BlockHeader) -> String {
    let mut s = Sha256::new();
    s.update(h.version.to_le_bytes());
    s.update(h.height.to_le_bytes());
    s.update(h.prev_hash.as_bytes());
    s.update(h.merkle_root.as_bytes());
    s.update(h.timestamp.to_le_bytes());
    s.update(h.difficulty.to_le_bytes());
    s.update(h.nonce.to_le_bytes());
    hex::encode(s.finalize())
}

fn create_midstate(h: &BlockHeader) -> Sha256 {
    let mut s = Sha256::new();
    s.update(h.version.to_le_bytes());
    s.update(h.height.to_le_bytes());
    s.update(h.prev_hash.as_bytes());
    s.update(h.merkle_root.as_bytes());
    s.update(h.timestamp.to_le_bytes());
    s.update(h.difficulty.to_le_bytes());
    s
}

fn header_bytes_152(h: &BlockHeader) -> Vec<u8> {
    let mut header_bytes = Vec::with_capacity(152);
    header_bytes.extend_from_slice(&h.version.to_le_bytes());
    header_bytes.extend_from_slice(&h.height.to_le_bytes());
    header_bytes.extend_from_slice(h.prev_hash.as_bytes());
    header_bytes.extend_from_slice(h.merkle_root.as_bytes());
    header_bytes.extend_from_slice(&h.timestamp.to_le_bytes());
    header_bytes.extend_from_slice(&h.difficulty.to_le_bytes());
    while header_bytes.len() < 152 { header_bytes.push(0); }
    header_bytes.truncate(152);
    header_bytes
}

fn difficulty_to_target(difficulty: u32) -> [u8; 32] {
    let d = difficulty.min(60) as usize;
    let mut t = [0xFFu8; 32];
    for b in t.iter_mut().take(d / 2) { *b = 0x00; }
    if d % 2 == 1 && d / 2 < 32 { t[d / 2] = 0x0F; }
    t
}

struct SharedTemplate {
    block: Block,
    version: u64,
}

// -----------------------------------------------------
// --------------- CPU POW THREAD ----------------------
// -----------------------------------------------------
fn cpu_pow_thread(
    tid: usize,
    nthreads: usize,
    tmpl: Arc<Mutex<SharedTemplate>>,
    global_stop: Arc<AtomicBool>,
    found_flag: Arc<AtomicBool>,
    found_nonce: Arc<AtomicU64>,
    found_version: Arc<AtomicU64>,
    hashes_ctr: Arc<AtomicU64>,
    nonce_ctr: Arc<AtomicU64>,
) {
    let (mut local_block, mut local_version) = {
        let t = tmpl.lock().unwrap();
        (t.block.clone(), t.version)
    };
    let mut target = difficulty_to_target(local_block.header.difficulty);
    let mut midstate = create_midstate(&local_block.header);
    
    // CPU берет старшую половину возможных Nonce, чтобы не пересекаться с GPU
    let mut nonce = (u64::MAX / 2).wrapping_add(tid as u64);

    loop {
        if global_stop.load(Ordering::Relaxed) { return; }

        if found_flag.load(Ordering::Relaxed) {
            if found_version.load(Ordering::Relaxed) == local_version {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }

        for _ in 0..BATCH_SIZE_CPU {
            let mut hasher = midstate.clone();
            hasher.update(nonce.to_le_bytes());
            let hash = hasher.finalize();

            if hash.as_slice() <= target.as_slice() {
                if found_flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                    found_nonce.store(nonce, Ordering::SeqCst);
                    found_version.store(local_version, Ordering::SeqCst);
                }
                break;
            }
            nonce = nonce.wrapping_add(nthreads as u64);
        }

        hashes_ctr.fetch_add(BATCH_SIZE_CPU, Ordering::Relaxed);
        if tid == 0 { nonce_ctr.store(nonce, Ordering::Relaxed); }

        {
            let t = tmpl.lock().unwrap();
            if t.version != local_version {
                local_version = t.version;
                local_block = t.block.clone();
                target = difficulty_to_target(local_block.header.difficulty);
                midstate = create_midstate(&local_block.header);
                nonce = (u64::MAX / 2).wrapping_add(tid as u64);
            }
        }
    }
}

// -----------------------------------------------------
// --------------- GPU POW THREAD ----------------------
// -----------------------------------------------------
fn gpu_pow_thread(
    tmpl: Arc<Mutex<SharedTemplate>>,
    global_stop: Arc<AtomicBool>,
    found_flag: Arc<AtomicBool>,
    found_nonce: Arc<AtomicU64>,
    found_version: Arc<AtomicU64>,
    hashes_ctr: Arc<AtomicU64>,
    nonce_ctr: Arc<AtomicU64>,
    events: Arc<Mutex<Vec<MineEvent>>>,
) {
    use opencl3::device::{get_all_devices, Device, CL_DEVICE_TYPE_GPU};
    use opencl3::context::Context;
    use opencl3::command_queue::CommandQueue;
    use opencl3::memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_WRITE_ONLY, CL_MEM_READ_WRITE};
    use opencl3::program::Program;
    use opencl3::kernel::{Kernel, ExecuteKernel};
    use opencl3::types::{cl_uchar, cl_ulong, cl_uint, CL_BLOCKING};

    let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU).unwrap_or_default();
    if device_ids.is_empty() {
        push(&events, MineEvent::Error { msg: "GPU Инициализация не удалась: Нет доступных GPU (OpenCL)".into() });
        return;
    }

    let device_id = device_ids[0];
    let device = Device::new(device_id);
    let dev_name = device.name().unwrap_or("Unknown GPU".into());
    push(&events, MineEvent::Error { msg: format!("✅ GPU ИНИЦИАЛИЗИРОВАН УСПЕШНО: {}", dev_name) });

    let context = Context::from_device(&device).expect("Context::from_device failed");
    let queue = CommandQueue::create_default_with_properties(&context, 0, 0).expect("CommandQueue failed");

    let src = crate::gpu_kernel::SHA256_KERNEL;
    let program = Program::create_and_build_from_source(&context, src, "").unwrap_or_else(|e| {
        panic!("OpenCL compiler error: {}", e);
    });
    
    let kernel = Kernel::create(&program, "mine").expect("Kernel::create failed");

    let (mut header_buf, mut target_buf, nonces_buf, mut count_buf) = unsafe {
        (
            Buffer::<cl_uchar>::create(&context, CL_MEM_READ_ONLY, 152, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_uchar>::create(&context, CL_MEM_READ_ONLY, 32, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_ulong>::create(&context, CL_MEM_WRITE_ONLY, 10, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_uint>::create(&context, CL_MEM_READ_WRITE, 1, std::ptr::null_mut()).unwrap(),
        )
    };

    let (mut local_block, mut local_version) = {
        let t = tmpl.lock().unwrap();
        (t.block.clone(), t.version)
    };
    
    let mut b_target = difficulty_to_target(local_block.header.difficulty);
    let mut b_header = header_bytes_152(&local_block.header);

    // Initial explicit block write (Blocking)
    let _ = unsafe { queue.enqueue_write_buffer(&mut header_buf, CL_BLOCKING, 0, &b_header, &[]) }.unwrap();
    let _ = unsafe { queue.enqueue_write_buffer(&mut target_buf, CL_BLOCKING, 0, &b_target, &[]) }.unwrap();

    let mut base_nonce = 0u64; // GPU проверяет младшую половину

    loop {
        if global_stop.load(Ordering::Relaxed) { return; }

        if found_flag.load(Ordering::Relaxed) {
            if found_version.load(Ordering::Relaxed) == local_version {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        }

        // Обнуляем счетчик выигравших (блокирующий write)
        let _ = unsafe { queue.enqueue_write_buffer(&mut count_buf, CL_BLOCKING, 0, &[0u32], &[]) }.unwrap();

        let execute_event = unsafe {
            ExecuteKernel::new(&kernel)
                .set_arg(&header_buf)
                .set_arg(&target_buf)
                .set_arg(&nonces_buf)
                .set_arg(&count_buf)
                .set_arg(&base_nonce)
                .set_global_work_size(BATCH_SIZE_GPU)
                .enqueue_nd_range(&queue)
        }.unwrap();

        // Дожидаемся завершения исполнения Ядра (CPU ждет GPU)
        execute_event.wait().unwrap();

        // Читаем количество найденных (блокирующий read)
        let mut count_res = vec![0u32; 1];
        let _ = unsafe { queue.enqueue_read_buffer(&count_buf, CL_BLOCKING, 0, &mut count_res, &[]) }.unwrap();

        if count_res[0] > 0 {
            let mut out_nonces = vec![0u64; 10];
            let _ = unsafe { queue.enqueue_read_buffer(&nonces_buf, CL_BLOCKING, 0, &mut out_nonces, &[]) }.unwrap();

            let winning_nonce = out_nonces[0];
            
            if found_flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                found_nonce.store(winning_nonce, Ordering::SeqCst);
                found_version.store(local_version, Ordering::SeqCst);
            }
        }

        hashes_ctr.fetch_add(BATCH_SIZE_GPU as u64, Ordering::Relaxed);
        nonce_ctr.store(base_nonce.wrapping_add(BATCH_SIZE_GPU as u64), Ordering::Relaxed);
        base_nonce = base_nonce.wrapping_add(BATCH_SIZE_GPU as u64);

        {
            let t = tmpl.lock().unwrap();
            if t.version != local_version {
                local_version = t.version;
                local_block = t.block.clone();
                b_target = difficulty_to_target(local_block.header.difficulty);
                b_header = header_bytes_152(&local_block.header);
                
                let _ = unsafe { queue.enqueue_write_buffer(&mut header_buf, CL_BLOCKING, 0, &b_header, &[]) }.unwrap();
                let _ = unsafe { queue.enqueue_write_buffer(&mut target_buf, CL_BLOCKING, 0, &b_target, &[]) }.unwrap();
                
                base_nonce = 0;
            }
        }
    }
}

// -----------------------------------------------------

fn push(events: &Arc<Mutex<Vec<MineEvent>>>, ev: MineEvent) {
    let mut v = events.lock().unwrap();
    v.push(ev);
    if v.len() > 200 { v.remove(0); }
}

fn fetch_template(client: &NodeClient, address: &str) -> anyhow::Result<Block> {
    let tmpl: serde_json::Value = client.get(
        &format!("/block_template?miner_address={}", address)
    )?;
    let block: Block = serde_json::from_value(tmpl["block"].clone())
        .map_err(|e| anyhow::anyhow!("Парсинг блока: {}", e))?;
    Ok(block)
}

fn mining_main(
    node_url: String,
    address: String,
    nthreads: usize,
    use_gpu: bool,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<MinerState>>,
    events: Arc<Mutex<Vec<MineEvent>>>,
) {
    let client = match NodeClient::new(&node_url) {
        Ok(c) => c,
        Err(e) => {
            push(&events, MineEvent::Error { msg: format!("Не удалось подключиться: {}", e) });
            state.lock().unwrap().running = false;
            return;
        }
    };

    let first_block = loop {
        if stop.load(Ordering::Relaxed) {
            state.lock().unwrap().running = false;
            return;
        }
        match fetch_template(&client, &address) {
            Ok(b) => break b,
            Err(e) => {
                push(&events, MineEvent::Error { msg: format!("Шаблон: {}", e) });
                for _ in 0..40 {
                    if stop.load(Ordering::Relaxed) {
                        state.lock().unwrap().running = false;
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    };

    let height = first_block.header.height;
    let diff = first_block.header.difficulty;
    push(&events, MineEvent::NewTemplate { height, difficulty: diff });
    {
        let mut s = state.lock().unwrap();
        s.current_height = height;
        s.difficulty = diff;
    }

    let shared_tmpl = Arc::new(Mutex::new(SharedTemplate {
        block: first_block,
        version: 1,
    }));

    let found_flag = Arc::new(AtomicBool::new(false));
    let found_nonce = Arc::new(AtomicU64::new(0));
    let found_version = Arc::new(AtomicU64::new(0));
    let hashes_ctr = Arc::new(AtomicU64::new(0));
    let nonce_ctr = Arc::new(AtomicU64::new(0));

    let mut pow_handles = Vec::new();

    if use_gpu {
        let (tm, gs, ff, fn_, fv, hc, nc, ev_push) = (
            shared_tmpl.clone(), stop.clone(),
            found_flag.clone(), found_nonce.clone(), found_version.clone(),
            hashes_ctr.clone(), nonce_ctr.clone(), events.clone()
        );
        pow_handles.push(thread::spawn(move || {
            gpu_pow_thread(tm, gs, ff, fn_, fv, hc, nc, ev_push);
        }));
    }

    for tid in 0..nthreads {
        let (tm, gs, ff, fn_, fv, hc, nc) = (
            shared_tmpl.clone(), stop.clone(),
            found_flag.clone(), found_nonce.clone(), found_version.clone(),
            hashes_ctr.clone(), nonce_ctr.clone(),
        );
        pow_handles.push(thread::spawn(move || {
            cpu_pow_thread(tid, nthreads, tm, gs, ff, fn_, fv, hc, nc);
        }));
    }

    let start = Instant::now();
    let mut last_hr_time = Instant::now();
    let mut last_hr_hashes = 0u64;
    let mut blocks_found = 0u64;
    let mut reward_total = 0.0f64;
    let mut last_tmpl_check = Instant::now();
    let mut current_version: u64 = 1;
    let mut mine_start = Instant::now();

    'outer: loop {
        if stop.load(Ordering::Relaxed) { break; }
        thread::sleep(Duration::from_millis(50));

        {
            let now = Instant::now();
            let dt = now.duration_since(last_hr_time).as_secs_f64();
            if dt >= 1.0 {
                let total = hashes_ctr.load(Ordering::Relaxed);
                let delta = total.saturating_sub(last_hr_hashes);
                last_hr_hashes = total;
                last_hr_time = now;

                let cur_height;
                let cur_diff;
                {
                    let t = shared_tmpl.lock().unwrap();
                    cur_height = t.block.header.height;
                    cur_diff = t.block.header.difficulty;
                }

                let mut s = state.lock().unwrap();
                s.hashrate = delta as f64 / dt;
                s.total_hashes = total;
                s.nonce = nonce_ctr.load(Ordering::Relaxed);
                s.uptime_secs = start.elapsed().as_secs();
                s.current_height = cur_height;
                s.difficulty = cur_diff;
                s.blocks_found = blocks_found;
                s.reward_total = reward_total;
            }
        }

        if last_tmpl_check.elapsed() >= Duration::from_secs(TEMPLATE_POLL_SECS)
            && !found_flag.load(Ordering::Relaxed)
        {
            last_tmpl_check = Instant::now();

            match fetch_template(&client, &address) {
                Ok(fresh) => {
                    let old_height = shared_tmpl.lock().unwrap().block.header.height;
                    if fresh.header.height != old_height {
                        let new_height = fresh.header.height;
                        let new_diff = fresh.header.difficulty;
                        {
                            let mut t = shared_tmpl.lock().unwrap();
                            t.block = fresh;
                            t.version += 1;
                            current_version = t.version;
                        }
                        mine_start = Instant::now();
                        push(&events, MineEvent::TemplateSwap { old_height, new_height });
                        push(&events, MineEvent::NewTemplate { height: new_height, difficulty: new_diff });
                        found_flag.store(false, Ordering::SeqCst);
                    }
                }
                Err(e) => {
                    push(&events, MineEvent::Error { msg: format!("Watcher: {}", e) });
                }
            }
        }

        if !found_flag.load(Ordering::Relaxed) { continue; }
        if found_version.load(Ordering::SeqCst) != current_version { continue; }

        let mut mined_block = {
            let t = shared_tmpl.lock().unwrap();
            if t.version != current_version { continue; }
            t.block.clone()
        };

        let nonce = found_nonce.load(Ordering::SeqCst);
        mined_block.header.nonce = nonce;
        mined_block.hash = header_hash_string(&mined_block.header);

        let elapsed = mine_start.elapsed().as_secs_f64();
        let height = mined_block.header.height;
        push(&events, MineEvent::BlockFound { height, nonce, elapsed_secs: elapsed });

        let payload = serde_json::json!({ "block": mined_block });
        match client.post::<serde_json::Value>("/submit_block", &payload) {
            Ok(resp) => {
                let reward = resp["reward_tvc"].as_f64().unwrap_or(0.0);
                let txs = resp["transactions"].as_u64().unwrap_or(0) as usize;
                let h = resp["height"].as_u64().unwrap_or(height);
                blocks_found += 1;
                reward_total += reward;
                push(&events, MineEvent::BlockAccepted { height: h, reward_tvc: reward, txs });
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("Invalid prev_hash") {
                    push(&events, MineEvent::TemplateSwap {
                        old_height: height,
                        new_height: height,
                    });
                } else {
                    push(&events, MineEvent::BlockRejected { reason: msg });
                }
            }
        }

        if stop.load(Ordering::Relaxed) { break 'outer; }

        match fetch_template(&client, &address) {
            Ok(next) => {
                let new_height = next.header.height;
                let new_diff = next.header.difficulty;
                {
                    let mut t = shared_tmpl.lock().unwrap();
                    t.block = next;
                    t.version += 1;
                    current_version = t.version;
                }
                mine_start = Instant::now();
                found_flag.store(false, Ordering::SeqCst);
                push(&events, MineEvent::NewTemplate { height: new_height, difficulty: new_diff });
            }
            Err(e) => {
                push(&events, MineEvent::Error { msg: format!("Следующий шаблон: {}", e) });
                found_flag.store(false, Ordering::SeqCst);
            }
        }
    }

    stop.store(true, Ordering::SeqCst);
    for h in pow_handles { let _ = h.join(); }
    state.lock().unwrap().running = false;
}
