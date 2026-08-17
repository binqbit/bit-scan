inline void add_words_8(__global u32 *dst, const u32 offset, const u32 *src)
{
    ulong carry = 0;

    for (int i = 0; i < 8; i++) {
        const ulong sum = (ulong) dst[offset + i] + (ulong) src[i] + carry;
        dst[offset + i] = (u32) sum;
        carry = sum >> 32;
    }
}

inline void copy_global_point_words(u32 *dst, __global const u32 *src, const u32 offset)
{
    for (int i = 0; i < ONE_COORDINATE_NUM_WORDS; i++) {
        dst[i] = src[offset + i];
    }
}

inline uint kangaroo_jump_index(const u32 *x, const uint jump_count)
{
    uint h = x[0] ^ rotate(x[1], 7u) ^ rotate(x[3], 13u) ^ rotate(x[7], 19u);
    return h % jump_count;
}

inline int kangaroo_is_distinguished(const u32 *x, const uint dp_bits)
{
    const uint full_words = dp_bits / 32u;
    const uint rem_bits = dp_bits & 31u;

    for (uint i = 0; i < full_words; i++) {
        if (x[i] != 0u) {
            return 0;
        }
    }

    if (rem_bits == 0u) {
        return 1;
    }

    const uint mask = (1u << rem_bits) - 1u;
    return (x[full_words] & mask) == 0u;
}

__kernel void bit_scan_kangaroo_step_kernel(
    __global u32 *state_x,
    __global u32 *state_y,
    __global u32 *state_distance,
    __global const u32 *jump_x,
    __global const u32 *jump_y,
    __global const u32 *jump_distance,
    const uint jump_count,
    const uint dp_bits,
    const uint batch_steps,
    __global u32 *hit,
    __global u32 *steps_done
) {
    const uint gid = get_global_id(0);
    const uint coord_offset = gid * ONE_COORDINATE_NUM_WORDS;
    const uint distance_offset = gid * 8u;

    u32 x[ONE_COORDINATE_NUM_WORDS];
    u32 y[ONE_COORDINATE_NUM_WORDS];
    u32 jx[ONE_COORDINATE_NUM_WORDS];
    u32 jy[ONE_COORDINATE_NUM_WORDS];
    u32 jd[8];

    copy_global_point_words(x, state_x, coord_offset);
    copy_global_point_words(y, state_y, coord_offset);

    uint local_steps = 0u;
    uint local_hit = 0u;

    for (uint step = 0; step < batch_steps; step++) {
        const uint jump_idx = kangaroo_jump_index(x, jump_count);
        const uint jump_offset = jump_idx * ONE_COORDINATE_NUM_WORDS;
        const uint jump_distance_offset = jump_idx * 8u;

        copy_global_point_words(jx, jump_x, jump_offset);
        copy_global_point_words(jy, jump_y, jump_offset);
        for (int word = 0; word < 8; word++) {
            jd[word] = jump_distance[jump_distance_offset + word];
        }

        point_add_xy(x, y, jx, jy);
        add_words_8(state_distance, distance_offset, jd);
        local_steps++;

        if (kangaroo_is_distinguished(x, dp_bits)) {
            local_hit = 1u;
            break;
        }
    }

    for (int word = 0; word < ONE_COORDINATE_NUM_WORDS; word++) {
        state_x[coord_offset + word] = x[word];
        state_y[coord_offset + word] = y[word];
    }

    hit[gid] = local_hit;
    steps_done[gid] = local_steps;
}
