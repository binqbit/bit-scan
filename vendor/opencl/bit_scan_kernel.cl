inline void add_scalar_u32_array_private(u32 *dst, const u32 *src, u32 increment)
{
    ulong carry = increment;

    for (int i = 0; i < PRIVATE_KEY_MAX_NUM_WORDS; i++) {
        ulong sum = (ulong)src[i] + carry;
        dst[i] = (u32)sum;
        carry = sum >> 32;

        if (carry == 0) {
            for (int j = i + 1; j < PRIVATE_KEY_MAX_NUM_WORDS; j++) {
                dst[j] = src[j];
            }
            return;
        }
    }
}

inline void increment_u32_array_private(u32 *value)
{
    ulong carry = 1;

    for (int i = 0; i < PRIVATE_KEY_MAX_NUM_WORDS; i++) {
        ulong sum = (ulong)value[i] + carry;
        value[i] = (u32)sum;
        carry = sum >> 32;
        if (carry == 0) {
            return;
        }
    }
}

inline int hash160_matches(const u32 *hash_words, __global const uchar *target_hash)
{
    const __private uchar *hash_bytes = (const __private uchar *) hash_words;

    for (int i = 0; i < RIPEMD160_HASH_NUM_BYTES; i++) {
        if (hash_bytes[i] != target_hash[i]) {
            return 0;
        }
    }

    return 1;
}

__kernel void bit_scan_match_kernel(
    __global const u32 *base_key,
    __global const uchar *target_hash,
    const u32 loopCount,
    __global volatile uint *found_flag,
    __global uint *found_key
) {
    if (*found_flag != 0) {
        return;
    }

    u32 base_key_local[PRIVATE_KEY_MAX_NUM_WORDS];
    u32 current_key[PRIVATE_KEY_MAX_NUM_WORDS];
    u32 x_littleEndian_local[ONE_COORDINATE_NUM_WORDS];
    u32 y_littleEndian_local[ONE_COORDINATE_NUM_WORDS];
    u32 x_bigEndian_local[ONE_COORDINATE_NUM_WORDS];
    u32 y_bigEndian_local[ONE_COORDINATE_NUM_WORDS];
    u32 x1_local[ONE_COORDINATE_NUM_WORDS];
    u32 y1_local[ONE_COORDINATE_NUM_WORDS];
    u32 sha256_input_compressed[SHA256_INPUT_TOTAL_WORDS_COMPRESSED];
    u32 ripemd160_input_compressed[RIPEMD160_INPUT_BLOCK_SIZE_WORDS];
    uchar sec_compressed[SEC_PUBLIC_KEY_COMPRESSED_NUM_BYTES];
    sha256_ctx_t sha_ctx_compressed;
    ripemd160_ctx_t ripemd_ctx_compressed;

    copy_global_u32_array_private_u32(base_key_local, base_key, PRIVATE_KEY_MAX_NUM_WORDS);
    copy_constant_u32_array_private_u32(x1_local, &g_precomputed.xy[G_OFFSET_X1], ONE_COORDINATE_NUM_WORDS);
    copy_constant_u32_array_private_u32(y1_local, &g_precomputed.xy[G_OFFSET_Y1], ONE_COORDINATE_NUM_WORDS);

    add_scalar_u32_array_private(current_key, base_key_local, get_global_id(0) * loopCount);

    for (u32 i = 0; i < loopCount; i++) {
        if (*found_flag != 0) {
            return;
        }

        if (i == 0) {
            point_mul_xy(x_littleEndian_local, y_littleEndian_local, current_key, &g_precomputed);
        } else {
            point_add_xy(x_littleEndian_local, y_littleEndian_local, x1_local, y1_local);
        }

        copy_and_reverse_endianness_u32_array(x_bigEndian_local, 0, x_littleEndian_local, ONE_COORDINATE_NUM_WORDS);
        copy_and_reverse_endianness_u32_array(y_bigEndian_local, 0, y_littleEndian_local, ONE_COORDINATE_NUM_WORDS);

        get_sec_bytes_compressed(x_bigEndian_local, y_bigEndian_local, sec_compressed);
        build_sha256_block_from_compressed_pubkey(sec_compressed, sha256_input_compressed);
        sha256_init(&sha_ctx_compressed);
        sha256_update(&sha_ctx_compressed, sha256_input_compressed, SHA256_INPUT_TOTAL_BYTES_COMPRESSED);

        build_ripemd160_block_from_sha256(sha_ctx_compressed.h, ripemd160_input_compressed);
        ripemd160_init(&ripemd_ctx_compressed);
        ripemd160_update_swap(&ripemd_ctx_compressed, ripemd160_input_compressed, RIPEMD160_INPUT_BLOCK_SIZE_BYTES);

        if (hash160_matches(ripemd_ctx_compressed.h, target_hash)
            && atomic_cmpxchg(found_flag, 0u, 1u) == 0u) {
            for (int word = 0; word < PRIVATE_KEY_MAX_NUM_WORDS; word++) {
                found_key[word] = current_key[word];
            }
            return;
        }

        increment_u32_array_private(current_key);
    }
}
