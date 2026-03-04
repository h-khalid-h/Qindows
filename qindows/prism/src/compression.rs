//! # Prism LZ4 Compression
//!
//! Fast block compression for Prism storage.
//! LZ4 is optimized for decompression speed (5+ GB/s) while still
//! providing reasonable compression ratios (~2.1x typical).
//! Used transparently by the dedup engine and snapshot system.

extern crate alloc;

use alloc::vec::Vec;

/// Minimum match length for LZ4.
const MIN_MATCH: usize = 4;
/// Hash table size (4096 entries — 12-bit hash).
const HASH_SIZE: usize = 4096;
/// Maximum offset for backreferences.
const MAX_OFFSET: usize = 65535;
/// Block size for compression.
const BLOCK_SIZE: usize = 64 * 1024;

/// Hash 4 bytes to a 12-bit index.
fn hash4(data: &[u8], pos: usize) -> usize {
    if pos + 4 > data.len() { return 0; }
    let val = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
    ((val.wrapping_mul(2654435761)) >> 20) as usize & (HASH_SIZE - 1)
}

/// Count matching bytes starting from two positions.
fn match_length(data: &[u8], pos1: usize, pos2: usize) -> usize {
    let max = data.len() - pos2;
    let mut len = 0;
    while len < max && data[pos1 + len] == data[pos2 + len] {
        len += 1;
        if pos1 + len >= pos2 { break; } // Overlap guard
    }
    len
}

/// Compress data using LZ4-like algorithm.
///
/// Returns compressed data with a 4-byte original-length header.
pub fn compress(input: &[u8]) -> Vec<u8> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut output = Vec::with_capacity(input.len());

    // Write original length as a 4-byte LE header
    output.extend_from_slice(&(input.len() as u32).to_le_bytes());

    let mut hash_table = [0usize; HASH_SIZE];
    let mut pos = 0;
    let mut anchor = 0; // Start of un-matched (literal) data

    while pos + MIN_MATCH < input.len() {
        let h = hash4(input, pos);
        let ref_pos = hash_table[h];
        hash_table[h] = pos;

        // Check if we have a match
        if ref_pos > 0
            && pos - ref_pos <= MAX_OFFSET
            && pos >= ref_pos
            && input[ref_pos..ref_pos + MIN_MATCH] == input[pos..pos + MIN_MATCH]
        {
            let match_len = match_length(input, ref_pos, pos);

            if match_len >= MIN_MATCH {
                let literal_len = pos - anchor;
                let offset = pos - ref_pos;

                // Encode token: high 4 bits = literal length, low 4 bits = match length - 4
                let lit_token = literal_len.min(15) as u8;
                let match_token = (match_len - MIN_MATCH).min(15) as u8;
                output.push((lit_token << 4) | match_token);

                // Encode extra literal length bytes
                if literal_len >= 15 {
                    let mut remaining = literal_len - 15;
                    while remaining >= 255 {
                        output.push(255);
                        remaining -= 255;
                    }
                    output.push(remaining as u8);
                }

                // Copy literals
                output.extend_from_slice(&input[anchor..anchor + literal_len]);

                // Encode offset (2 bytes LE)
                output.push((offset & 0xFF) as u8);
                output.push(((offset >> 8) & 0xFF) as u8);

                // Encode extra match length bytes
                if match_len - MIN_MATCH >= 15 {
                    let mut remaining = match_len - MIN_MATCH - 15;
                    while remaining >= 255 {
                        output.push(255);
                        remaining -= 255;
                    }
                    output.push(remaining as u8);
                }

                pos += match_len;
                anchor = pos;
                continue;
            }
        }

        pos += 1;
    }

    // Write remaining literals
    let literal_len = input.len() - anchor;
    if literal_len > 0 {
        let lit_token = literal_len.min(15) as u8;
        output.push(lit_token << 4);

        if literal_len >= 15 {
            let mut remaining = literal_len - 15;
            while remaining >= 255 {
                output.push(255);
                remaining -= 255;
            }
            output.push(remaining as u8);
        }

        output.extend_from_slice(&input[anchor..]);
    }

    output
}

/// Decompress LZ4-compressed data.
pub fn decompress(input: &[u8]) -> Result<Vec<u8>, &'static str> {
    if input.len() < 4 {
        return Err("Input too short");
    }

    // Read original length
    let orig_len = u32::from_le_bytes([input[0], input[1], input[2], input[3]]) as usize;
    let mut output = Vec::with_capacity(orig_len);
    let mut pos = 4;

    while pos < input.len() {
        let token = input[pos];
        pos += 1;

        // Literal length
        let mut literal_len = ((token >> 4) & 0x0F) as usize;
        if literal_len == 15 {
            loop {
                if pos >= input.len() { return Err("Truncated literal length"); }
                let extra = input[pos] as usize;
                pos += 1;
                literal_len += extra;
                if extra < 255 { break; }
            }
        }

        // Copy literals
        if pos + literal_len > input.len() {
            return Err("Literal overflow");
        }
        output.extend_from_slice(&input[pos..pos + literal_len]);
        pos += literal_len;

        // Check if this is the last sequence (no match data)
        if pos >= input.len() { break; }

        // Read offset (2 bytes LE)
        if pos + 2 > input.len() { return Err("Truncated offset"); }
        let offset = (input[pos] as usize) | ((input[pos + 1] as usize) << 8);
        pos += 2;

        if offset == 0 { return Err("Invalid offset"); }

        // Match length
        let mut match_len = (token & 0x0F) as usize + MIN_MATCH;
        if (token & 0x0F) == 15 {
            loop {
                if pos >= input.len() { return Err("Truncated match length"); }
                let extra = input[pos] as usize;
                pos += 1;
                match_len += extra;
                if extra < 255 { break; }
            }
        }

        // Copy match (may overlap — must copy byte by byte)
        let match_start = output.len().checked_sub(offset).ok_or("Offset too large")?;
        for i in 0..match_len {
            let byte = output[match_start + i];
            output.push(byte);
        }
    }

    if output.len() != orig_len {
        return Err("Decompressed size mismatch");
    }

    Ok(output)
}

/// Compression statistics.
#[derive(Debug, Clone, Default)]
pub struct CompressionStats {
    pub total_compressed: u64,
    pub total_decompressed: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl CompressionStats {
    pub fn ratio(&self) -> f64 {
        if self.bytes_out == 0 { return 1.0; }
        self.bytes_in as f64 / self.bytes_out as f64
    }

    pub fn record_compress(&mut self, input_len: usize, output_len: usize) {
        self.total_compressed += 1;
        self.bytes_in += input_len as u64;
        self.bytes_out += output_len as u64;
    }
}
