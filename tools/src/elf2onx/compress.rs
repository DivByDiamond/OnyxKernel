pub fn rle_compress(src: &[u8]) -> Vec<u8> {
    let max_dst = src.len() + src.len() / 128 + 2;
    let mut dst = vec![0u8; max_dst];
    let n = src.len();
    let mut i = 0usize;
    let mut out = 0usize;
    while i < n {
        let cur = src[i];
        let mut run = 1usize;
        while i + run < n && src[i + run] == cur && run < 128 {
            run += 1;
        }
        if run >= 3 {
            dst[out] = 0x80 | ((run - 1) as u8);
            dst[out + 1] = cur;
            out += 2;
            i += run;
        } else {
            let lit_start = i;
            let mut lit_len = 0usize;
            while i + lit_len < n && lit_len < 128 {
                let b = src[i + lit_len];
                let mut k = 0usize;
                while i + lit_len + k < n && src[i + lit_len + k] == b && k < 3 {
                    k += 1;
                }
                if k >= 3 {
                    break;
                }
                lit_len += 1;
            }
            if lit_len == 0 {
                lit_len = 1;
            }
            dst[out] = (lit_len - 1) as u8;
            for j in 0..lit_len {
                dst[out + 1 + j] = src[lit_start + j];
            }
            out += 1 + lit_len;
            i += lit_len;
        }
    }
    dst.truncate(out);
    dst
}
