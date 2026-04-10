pub const SHA256_KERNEL: &str = r#"
#pragma OPENCL EXTENSION cl_khr_byte_addressable_store : enable

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

__kernel void mine(
    __global const uint* midstate_in,
    __global const uint* tail_data_in,
    __global const uint* target_in,
    __global ulong* out_nonces,
    __global uint* out_count,
    ulong base_nonce
) {
    ulong nonce = base_nonce + get_global_id(0);
    
    uint state[8];
    state[0] = midstate_in[0];
    state[1] = midstate_in[1];
    state[2] = midstate_in[2];
    state[3] = midstate_in[3];
    state[4] = midstate_in[4];
    state[5] = midstate_in[5];
    state[6] = midstate_in[6];
    state[7] = midstate_in[7];

    uint m[64];
    m[0] = tail_data_in[0];
    m[1] = tail_data_in[1];
    m[2] = tail_data_in[2];
    m[3] = tail_data_in[3];
    m[4] = tail_data_in[4];
    m[5] = tail_data_in[5];
    // words 6 & 7 contain the little-endian nonce encoded into big-endian struct
    m[6] = ((nonce & 0xFF) << 24) | (((nonce >> 8) & 0xFF) << 16) | (((nonce >> 16) & 0xFF) << 8) | ((nonce >> 24) & 0xFF);
    m[7] = (((nonce >> 32) & 0xFF) << 24) | (((nonce >> 40) & 0xFF) << 16) | (((nonce >> 48) & 0xFF) << 8) | ((nonce >> 56) & 0xFF);
    
    m[8] = tail_data_in[8]; // 0x80000000
    m[9] = 0;  m[10] = 0; m[11] = 0;
    m[12] = 0; m[13] = 0; m[14] = 0;
    m[15] = tail_data_in[15]; // length in bits (1280)

    #pragma unroll
    for(int i = 16; i < 64; ++i) {
        m[i] = SIG1(m[i - 2]) + m[i - 7] + SIG0(m[i - 15]) + m[i - 16];
    }

    uint a = state[0];
    uint b = state[1];
    uint c = state[2];
    uint d = state[3];
    uint e = state[4];
    uint f = state[5];
    uint g = state[6];
    uint h = state[7];

    #pragma unroll
    for(int i = 0; i < 64; ++i) {
        uint temp1 = h + EP1(e) + CH(e,f,g) + k[i] + m[i];
        uint temp2 = EP0(a) + MAJ(a,b,c);
        h = g;
        g = f;
        f = e;
        e = d + temp1;
        d = c;
        c = b;
        b = a;
        a = temp1 + temp2;
    }

    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    state[4] += e;
    state[5] += f;
    state[6] += g;
    state[7] += h;

    bool passed = true;
    #pragma unroll
    for (int i=0; i<8; i++) {
        uint s = state[i];
        uint t = target_in[i];
        if (s > t) { passed = false; break; }
        if (s < t) { break; }
    }

    if (passed) {
        uint idx = atomic_inc(out_count);
        if (idx < 10) {
            out_nonces[idx] = nonce;
        }
    }
}
"#;
