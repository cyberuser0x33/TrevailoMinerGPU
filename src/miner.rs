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
use crate::i18n::Translator;

const TEMPLATE_POLL_SECS: u64 = 5;
const BATCH_SIZE_CPU: u64 = 50_000;
const BATCH_SIZE_GPU: usize = 4_194_304; // 4 миллиона хэшей за оборот

#[derive(Debug, Clone)]
pub enum MineEvent {
    NewTemplate { height: u64, difficulty: u32 },
    BlockFound { height: u64, nonce: u64, elapsed_secs: f64 },
    BlockAccepted { height: u64, reward_tvc: f64, txs: usize },
    BlockRejected { reason: String },
    TemplateSwap { old_height: u64, new_height: u64 },
    Info { msg: String },
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
        tr: Arc<Translator>,
    ) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(MinerState { running: true, ..Default::default() }));
        let events = Arc::new(Mutex::new(Vec::<MineEvent>::new()));

        let (sf, st, ev) = (stop_flag.clone(), state.clone(), events.clone());
        thread::spawn(move || mining_main(node_url, address, num_threads, use_gpu, sf, st, ev, tr));

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

fn create_midstate_sha2(h: &BlockHeader) -> Sha256 {
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

// Ручное вычисление SHA-256 состояния (Midstate) после обработки 128 байт.
fn sha256_midstate(data: &[u8; 128]) -> [u32; 8] {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..(i + 1) * 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = state[0]; let mut b = state[1]; let mut c = state[2]; let mut d = state[3];
        let mut e = state[4]; let mut f = state[5]; let mut g = state[6]; let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g; g = f; f = e; e = d.wrapping_add(temp1); d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    state
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
    let mut midstate_sha2 = create_midstate_sha2(&local_block.header);
    
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
            let mut hasher = midstate_sha2.clone();
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
                midstate_sha2 = create_midstate_sha2(&local_block.header);
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
    tr: Arc<Translator>,
) {
    use opencl3::device::{get_all_devices, Device, CL_DEVICE_TYPE_GPU};
    use opencl3::context::Context;
    use opencl3::command_queue::CommandQueue;
    use opencl3::memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_WRITE_ONLY, CL_MEM_READ_WRITE};
    use opencl3::program::Program;
    use opencl3::kernel::{Kernel, ExecuteKernel};
    use opencl3::types::{cl_ulong, cl_uint, CL_BLOCKING};

    let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU).unwrap_or_default();
    if device_ids.is_empty() {
        push(&events, MineEvent::Error { 
            msg: tr.t("err_no_gpu", "GPU Initialization failed: No OpenCL GPUs available", &[]) 
        });
        return;
    }

    let device_id = device_ids[0];
    let device = Device::new(device_id);
    let dev_name = device.name().unwrap_or("Unknown GPU".into());
    
    // Используем вариант MineEvent::Info, а не Error
    push(&events, MineEvent::Info { 
        msg: tr.t("info_gpu_ok", "✅ GPU INITIALIZED SUCCESSFULLY: {dev}", &[("dev", &dev_name)]) 
    });

    let context = Context::from_device(&device).expect("Context::from_device failed");
    let queue = CommandQueue::create_default_with_properties(&context, 0, 0).expect("CommandQueue failed");

    let src = crate::gpu_kernel::SHA256_KERNEL;
    let program = Program::create_and_build_from_source(&context, src, "").unwrap_or_else(|e| {
        panic!("OpenCL compiler error: {}", e);
    });
    
    let kernel = Kernel::create(&program, "mine").expect("Kernel::create failed");

    let (mut midstate_buf, mut tail_data_buf, mut target_buf, nonces_buf, mut count_buf) = unsafe {
        (
            Buffer::<cl_uint>::create(&context, CL_MEM_READ_ONLY, 8, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_uint>::create(&context, CL_MEM_READ_ONLY, 16, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_uint>::create(&context, CL_MEM_READ_ONLY, 8, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_ulong>::create(&context, CL_MEM_WRITE_ONLY, 10, std::ptr::null_mut()).unwrap(),
            Buffer::<cl_uint>::create(&context, CL_MEM_READ_WRITE, 1, std::ptr::null_mut()).unwrap(),
        )
    };

    let (mut local_block, mut local_version) = {
        let t = tmpl.lock().unwrap();
        (t.block.clone(), t.version)
    };
    
    let mut b_target = difficulty_to_target(local_block.header.difficulty);
    let mut target_u32 = [0u32; 8];
    for i in 0..8 {
        target_u32[i] = u32::from_be_bytes(b_target[i*4..(i+1)*4].try_into().unwrap());
    }

    let mut b_header = header_bytes_152(&local_block.header);
    let mut b_midstate = sha256_midstate(b_header[0..128].try_into().unwrap());
    
    let mut tail_data = [0u32; 16];
    let tail_bytes = &b_header[128..152];
    for i in 0..6 {
        tail_data[i] = u32::from_be_bytes(tail_bytes[i*4..(i+1)*4].try_into().unwrap());
    }
    tail_data[8] = 0x80000000;
    tail_data[15] = 1280;

    // Initial explicit block write (Blocking)
    let _ = unsafe { queue.enqueue_write_buffer(&mut midstate_buf, CL_BLOCKING, 0, &b_midstate, &[]) }.unwrap();
    let _ = unsafe { queue.enqueue_write_buffer(&mut tail_data_buf, CL_BLOCKING, 0, &tail_data, &[]) }.unwrap();
    let _ = unsafe { queue.enqueue_write_buffer(&mut target_buf, CL_BLOCKING, 0, &target_u32, &[]) }.unwrap();

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
                .set_arg(&midstate_buf)
                .set_arg(&tail_data_buf)
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
                for i in 0..8 {
                    target_u32[i] = u32::from_be_bytes(b_target[i*4..(i+1)*4].try_into().unwrap());
                }

                b_header = header_bytes_152(&local_block.header);
                b_midstate = sha256_midstate(b_header[0..128].try_into().unwrap());
                
                let tail_bytes = &b_header[128..152];
                for i in 0..6 {
                    tail_data[i] = u32::from_be_bytes(tail_bytes[i*4..(i+1)*4].try_into().unwrap());
                }
                
                // Передача обновленных данных
                let _ = unsafe { queue.enqueue_write_buffer(&mut midstate_buf, CL_BLOCKING, 0, &b_midstate, &[]) }.unwrap();
                let _ = unsafe { queue.enqueue_write_buffer(&mut tail_data_buf, CL_BLOCKING, 0, &tail_data, &[]) }.unwrap();
                let _ = unsafe { queue.enqueue_write_buffer(&mut target_buf, CL_BLOCKING, 0, &target_u32, &[]) }.unwrap();
                
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

fn fetch_template(client: &NodeClient, address: &str, tr: &Translator) -> anyhow::Result<Block> {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    let tmpl: serde_json::Value = client.get(
        &format!("/block_template?miner_address={}&_t={}", address, ts)
    )?;
    let block: Block = serde_json::from_value(tmpl["block"].clone())
        .map_err(|e| anyhow::anyhow!("{}", tr.t("err_parse", "Block parsing error: {err}", &[("err", &e.to_string())])))?;
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
    tr: Arc<Translator>,
) {
    let client = match NodeClient::new(&node_url) {
        Ok(c) => c,
        Err(e) => {
            push(&events, MineEvent::Error { 
                msg: tr.t("err_conn", "Failed to connect: {err}", &[("err", &e.to_string())]) 
            });
            state.lock().unwrap().running = false;
            return;
        }
    };

    let first_block = loop {
        if stop.load(Ordering::Relaxed) {
            state.lock().unwrap().running = false;
            return;
        }
        match fetch_template(&client, &address, &tr) {
            Ok(b) => break b,
            Err(e) => {
                push(&events, MineEvent::Error { 
                    msg: tr.t("err_tmpl", "Template error: {err}", &[("err", &e.to_string())]) 
                });
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
        let (tm, gs, ff, fn_, fv, hc, nc, ev_push, tr_gpu) = (
            shared_tmpl.clone(), stop.clone(),
            found_flag.clone(), found_nonce.clone(), found_version.clone(),
            hashes_ctr.clone(), nonce_ctr.clone(), events.clone(), tr.clone()
        );
        pow_handles.push(thread::spawn(move || {
            gpu_pow_thread(tm, gs, ff, fn_, fv, hc, nc, ev_push, tr_gpu);
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

            match fetch_template(&client, &address, &tr) {
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
                    push(&events, MineEvent::Error { 
                        msg: tr.t("err_tmpl", "Watcher: {err}", &[("err", &e.to_string())]) 
                    });
                }
            }
        }

        if !found_flag.load(Ordering::Relaxed) { continue; }
        if found_version.load(Ordering::SeqCst) != current_version { 
            // If a thread somehow found a block for an older version right during a swap, discard it.
            found_flag.store(false, Ordering::SeqCst);
            continue; 
        }

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

        match fetch_template(&client, &address, &tr) {
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
                push(&events, MineEvent::Error { 
                    msg: tr.t("err_next_tmpl", "Next template error: {err}", &[("err", &e.to_string())]) 
                });
                found_flag.store(false, Ordering::SeqCst);
            }
        }
    }

    stop.store(true, Ordering::SeqCst);
    for h in pow_handles { let _ = h.join(); }
    state.lock().unwrap().running = false;
}
