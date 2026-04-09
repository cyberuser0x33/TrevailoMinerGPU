pub const SHA256_KERNEL: &str = r#"
#define ROTRIGHT(a,b) (((a) >> (b)) | ((a) << (32-(b))))
#define CH(x,y,z) (((x) & (y)) ^ (~(x) & (z)))
#define MAJ(x,y,z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))
#define EP0(x) (ROTRIGHT(x,2) ^ ROTRIGHT(x,13) ^ ROTRIGHT(x,22))
#define EP1(x) (ROTRIGHT(x,6) ^ ROTRIGHT(x,11) ^ ROTRIGHT(x,25))
#define SIG0(x) (ROTRIGHT(x,7) ^ ROTRIGHT(x,18) ^ ((x) >> 3))
#define SIG1(x) (ROTRIGHT(x,17) ^ ROTRIGHT(x,19) ^ ((x) >> 10))

__constant uint k[64] = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2
};

void sha256_transform(uint *state, const uint *data) {
    uint a, b, c, d, e, f, g, h, i, T1, T2;
    uint m[64];

    for (i = 0; i < 16; ++i)
        m[i] = data[i];
    for ( ; i < 64; ++i)
        m[i] = SIG1(m[i - 2]) + m[i - 7] + SIG0(m[i - 15]) + m[i - 16];

    a = state[0]; b = state[1]; c = state[2]; d = state[3];
    e = state[4]; f = state[5]; g = state[6]; h = state[7];

    for (i = 0; i < 64; ++i) {
        T1 = h + EP1(e) + CH(e,f,g) + k[i] + m[i];
        T2 = EP0(a) + MAJ(a,b,c);
        h = g; g = f; f = e; e = d + T1;
        d = c; c = b; b = a; a = T1 + T2;
    }

    state[0] += a; state[1] += b; state[2] += c; state[3] += d;
    state[4] += e; state[5] += f; state[6] += g; state[7] += h;
}

// Из Rust мы получаем 152 байта заголовка + target.
// GPU вставляет свой nonce в последние 8 байт, делает pad до 192 байт и хеширует 3 чанка.
__kernel void mine(
    __global const uchar* header_in,
    __global const uchar* target_in,
    __global ulong* out_nonces,
    __global uint* out_count,
    ulong base_nonce
) {
    ulong nonce = base_nonce + get_global_id(0);

    // Подготовка буфера на 192 байта (3 блока по 64)
    uchar buf[192];
    for (int i=0; i<152; i++) {
        buf[i] = header_in[i];
    }
    
    // Вставляем nonce (little endian)
    for (int i=0; i<8; i++) {
        buf[152+i] = (uchar)((nonce >> (i * 8)) & 0xFF);
    }

    // Паддинг (0x80 ... длина в битах: 160 * 8 = 1280 (0x0500))
    buf[160] = 0x80;
    for (int i=161; i<188; i++) {
        buf[i] = 0x00;
    }
    buf[188] = 0x00;
    buf[189] = 0x00;
    buf[190] = 0x05;
    buf[191] = 0x00;

    // Конвертируем байты в 32-битные слова Big-Endian
    uint data[48];
    for (int i = 0; i < 48; i++) {
        data[i] = (buf[i*4] << 24) | (buf[i*4+1] << 16) | (buf[i*4+2] << 8) | buf[i*4+3];
    }

    // SHA-256 Init State
    uint state[8] = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
    };

    sha256_transform(state, &data[0]);
    sha256_transform(state, &data[16]);
    sha256_transform(state, &data[32]);

    // Конвертация Final State обратно в байты (Big Endian sequence)
    uchar hash[32];
    for(int i=0; i<8; i++) {
        hash[i*4+0] = (state[i] >> 24) & 0xFF;
        hash[i*4+1] = (state[i] >> 16) & 0xFF;
        hash[i*4+2] = (state[i] >> 8) & 0xFF;
        hash[i*4+3] = (state[i]) & 0xFF;
    }

    // Сравнение с Target (Лексикографическое сравнение массивов)
    bool won = true;
    for (int i=0; i<32; i++) {
        if (hash[i] < target_in[i]) {
            won = true;
            break;
        } else if (hash[i] > target_in[i]) {
            won = false;
            break;
        }
    }

    if (won) {
        // Атомарно увеличиваем счетчик и сохраняем выигрышный nonce
        uint idx = atomic_inc(out_count);
        if (idx < 10) {
            out_nonces[idx] = nonce;
        }
    }
}
"#;
