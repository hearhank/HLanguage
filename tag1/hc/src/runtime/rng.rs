//! 伪随机数生成器：xorshift64* 算法（io.rng 底层）

/// xorshift64*——`io.rng.next` 底层；state=0 时原地保持 0（调用方 seed 守卫：
/// 0 种子回退默认常量）。
pub fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}
