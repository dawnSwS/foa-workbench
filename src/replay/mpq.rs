//! A bounded reader for the classic MPQ tables used by SC2 replays.
//!
//! Supports MPQ versions 0–3 with 32-bit classic table offsets, stored/zlib/bzip2
//! members, and unencrypted single-unit or sector-based files. Extended HET/BET
//! tables are not needed when the archive also contains classic tables.
//!
//! Format and hashing references: `StormLib`'s `MoPaQ` format documentation and
//! mpyq 0.2.5. The latter's copyright and license are retained below.
/*
Copyright (c) 2010-2014 Aku Kotkavuo. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

  1. Redistributions of source code must retain the above copyright
     notice, this list of conditions and the following disclaimer.

  2. Redistributions in binary form must reproduce the above copyright
     notice, this list of conditions and the following disclaimer in the
     documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
*/

use std::io::Read;

const MAX_MEMBER_BYTES: usize = 32 * 1024 * 1024;
const MAX_TABLE_ENTRIES: usize = 65_536;
const FILE_COMPRESS: u32 = 0x0000_0200;
const FILE_SINGLE_UNIT: u32 = 0x0100_0000;
const FILE_SECTOR_CRC: u32 = 0x0400_0000;
const FILE_EXISTS: u32 = 0x8000_0000;
const UNSUPPORTED_FLAGS: u32 = 0x0000_0100 | 0x0001_0000 | 0x0002_0000 | 0x0010_0000;

fn slice(bytes: &[u8], offset: usize, size: usize) -> Result<&[u8], String> {
    let end = offset.checked_add(size).ok_or("MPQ 偏移溢出。")?;
    bytes
        .get(offset..end)
        .ok_or_else(|| "MPQ 数据截断或偏移越界。".into())
}

fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let array = slice(bytes, offset, 4)?
        .try_into()
        .map_err(|_| "MPQ 整数截断。")?;
    Ok(u32::from_le_bytes(array))
}

fn short(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let array = slice(bytes, offset, 2)?
        .try_into()
        .map_err(|_| "MPQ 整数截断。")?;
    Ok(u16::from_le_bytes(array))
}

fn offset(bytes: &[u8], position: usize) -> Result<usize, String> {
    usize::try_from(word(bytes, position)?).map_err(|_| "MPQ 偏移过大。".into())
}

/// Returns the versioned replay header, without the 16-byte MPQ user header.
pub fn user_data(bytes: &[u8]) -> Result<&[u8], String> {
    if !bytes.starts_with(b"MPQ\x1b") {
        return Err("录像缺少 MPQ 用户数据头。".into());
    }
    let reserved = offset(bytes, 4)?;
    let archive_start = offset(bytes, 8)?;
    let size = offset(bytes, 12)?;
    if size > reserved || archive_start < 16 || size > archive_start - 16 {
        return Err("MPQ 用户数据长度无效。".into());
    }
    // Validate that the destination is an actual archive, not an arbitrary offset.
    if slice(bytes, archive_start, 4)? != b"MPQ\x1a" {
        return Err("MPQ 归档头无效。".into());
    }
    slice(bytes, 16, size)
}

struct Archive<'a> {
    data: &'a [u8],
    header: &'a [u8],
    version: u16,
    sector_size: usize,
}

impl<'a> Archive<'a> {
    fn read(bytes: &'a [u8]) -> Result<Self, String> {
        let start = if bytes.starts_with(b"MPQ\x1b") {
            user_data(bytes)?;
            offset(bytes, 8)?
        } else if bytes.starts_with(b"MPQ\x1a") {
            0
        } else {
            return Err("文件不是有效的 SC2Replay / MPQ。".into());
        };
        let raw = bytes.get(start..).ok_or("MPQ 归档偏移越界。")?;
        let version = short(raw, 12)?;
        let minimum = match version {
            0 => 32,
            1 => 44,
            2 => 68,
            3 => 208,
            _ => return Err(format!("暂不支持 MPQ 格式版本 {version}。")),
        };
        let header_size = offset(raw, 4)?;
        if header_size < minimum {
            return Err("MPQ 归档头长度不足。".into());
        }
        let header = slice(raw, 0, header_size)?;
        if version >= 1
            && (word(header, 32)? != 0 || word(header, 36)? != 0 || word(header, 40)? != 0)
        {
            return Err("暂不支持 MPQ 高位文件偏移表。".into());
        }
        let archive_size = if version >= 2 {
            if word(header, 48)? != 0 {
                return Err("MPQ 归档超过支持的大小。".into());
            }
            offset(header, 44)?
        } else {
            offset(header, 8)?
        };
        if archive_size < header_size {
            return Err("MPQ 归档大小小于头部。".into());
        }
        let data = slice(raw, 0, archive_size)?;
        let shift = u32::from(short(header, 14)?);
        if shift > 15 {
            return Err("MPQ 扇区大小超过支持的范围。".into());
        }
        let sector_size = 512_usize.checked_shl(shift).ok_or("MPQ 扇区大小溢出。")?;
        Ok(Self {
            data,
            header,
            version,
            sector_size,
        })
    }

    fn table(&self, hash: bool, crypt: &[u32]) -> Result<Vec<u8>, String> {
        let (position, count_position, size_position, name) = if hash {
            (16, 24, 68, "(hash table)")
        } else {
            (20, 28, 76, "(block table)")
        };
        let count = offset(self.header, count_position)?;
        if count == 0 || count > MAX_TABLE_ENTRIES {
            return Err("MPQ 经典索引表缺失或条目过多。".into());
        }
        let size = count.checked_mul(16).ok_or("MPQ 索引表长度溢出。")?;
        if self.version >= 3
            && (offset(self.header, size_position)? != size
                || word(self.header, size_position + 4)? != 0)
        {
            return Err("暂不支持压缩的 MPQ 经典索引表。".into());
        }
        let position = offset(self.header, position)?;
        if position < self.header.len() {
            return Err("MPQ 索引表与归档头重叠。".into());
        }
        decrypt_table(
            slice(self.data, position, size)?,
            hash_name(name, 3, crypt)?,
            crypt,
        )
    }
}

fn crypt_table() -> Result<Vec<u32>, String> {
    let mut result = vec![0; 1280];
    let mut seed = 0x0010_0001_u32;
    for column in 0..256 {
        for row in 0..5 {
            seed = (seed * 125 + 3) % 0x002a_aaab;
            let high = (seed & 0xffff) << 16;
            seed = (seed * 125 + 3) % 0x002a_aaab;
            *result
                .get_mut(row * 256 + column)
                .ok_or("MPQ 散列表索引错误。")? = high | (seed & 0xffff);
        }
    }
    Ok(result)
}

fn hash_name(name: &str, kind: usize, crypt: &[u32]) -> Result<u32, String> {
    if !name.is_ascii() {
        return Err("MPQ 成员名必须是 ASCII。".into());
    }
    let mut first = 0x7fed_7fed_u32;
    let mut second = 0xeeee_eeee_u32;
    for byte in name.bytes().map(|byte| byte.to_ascii_uppercase()) {
        let code = crypt
            .get(kind * 256 + usize::from(byte))
            .ok_or("MPQ 散列表索引错误。")?;
        first = code ^ first.wrapping_add(second);
        second = u32::from(byte)
            .wrapping_add(first)
            .wrapping_add(second)
            .wrapping_add(second << 5)
            .wrapping_add(3);
    }
    Ok(first)
}

fn decrypt_table(data: &[u8], mut key: u32, crypt: &[u32]) -> Result<Vec<u8>, String> {
    let mut seed = 0xeeee_eeee_u32;
    let mut output = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<4>().0 {
        let index = usize::try_from(key & 0xff).map_err(|_| "MPQ 密钥无效。")?;
        seed = seed.wrapping_add(*crypt.get(0x400 + index).ok_or("MPQ 密钥索引错误。")?);
        let value = word(chunk, 0)? ^ key.wrapping_add(seed);
        key = ((!key << 21).wrapping_add(0x1111_1111)) | (key >> 11);
        seed = value
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
        output.extend_from_slice(&value.to_le_bytes());
    }
    Ok(output)
}

fn unpack(data: &[u8], expected: usize, compressed: bool) -> Result<Vec<u8>, String> {
    if data.len() == expected {
        return Ok(data.to_vec());
    }
    if data.len() > expected || !compressed {
        return Err("MPQ 成员实际长度与声明长度不符。".into());
    }
    let (method, payload) = data.split_first().ok_or("MPQ 压缩数据为空。")?;
    let (output, consumed) = match method {
        2 => {
            let mut decoder = flate2::read::ZlibDecoder::new(payload);
            let output = read_bounded(&mut decoder, expected)?;
            (output, decoder.total_in())
        }
        16 => {
            let mut decoder = bzip2::read::BzDecoder::new(payload);
            let output = read_bounded(&mut decoder, expected)?;
            (output, decoder.total_in())
        }
        _ => {
            return Err(format!(
                "暂不支持 MPQ 压缩类型 0x{method:02X}（支持 zlib / bzip2）。"
            ))
        }
    };
    if output.len() != expected || consumed != u64::try_from(payload.len()).unwrap_or(u64::MAX) {
        return Err("MPQ 解压长度不符或压缩流包含多余数据。".into());
    }
    Ok(output)
}

fn read_bounded(reader: &mut impl Read, expected: usize) -> Result<Vec<u8>, String> {
    let limit = u64::try_from(expected).map_err(|_| "MPQ 解压长度过大。")? + 1;
    let mut output = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut output)
        .map_err(|error| format!("MPQ 解压失败：{error}"))?;
    Ok(output)
}

fn unpack_sectors(
    data: &[u8],
    size: usize,
    flags: u32,
    sector_size: usize,
) -> Result<Vec<u8>, String> {
    if flags & FILE_COMPRESS == 0 {
        return unpack(data, size, false);
    }
    let sectors = size.div_ceil(sector_size);
    let entries = sectors + 1 + usize::from(flags & FILE_SECTOR_CRC != 0);
    let table_bytes = entries.checked_mul(4).ok_or("MPQ 扇区索引长度溢出。")?;
    slice(data, 0, table_bytes)?;
    let mut start = offset(data, 0)?;
    if start < table_bytes {
        return Err("MPQ 扇区数据与索引重叠。".into());
    }
    let mut output = Vec::with_capacity(size);
    for sector in 0..sectors {
        let end = offset(data, (sector + 1) * 4)?;
        let length = end.checked_sub(start).ok_or("MPQ 扇区偏移顺序无效。")?;
        let expected = sector_size.min(size - output.len());
        output.extend(unpack(slice(data, start, length)?, expected, true)?);
        start = end;
    }
    if flags & FILE_SECTOR_CRC != 0 {
        let end = offset(data, (entries - 1) * 4)?;
        let length = end.checked_sub(start).ok_or("MPQ 校验扇区偏移无效。")?;
        slice(data, start, length)?;
        start = end;
    }
    if start != data.len() {
        return Err("MPQ 扇区尾部长度不符。".into());
    }
    Ok(output)
}

/// Extracts one named replay member; never scans for guessed byte patterns.
pub fn extract(bytes: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let archive = Archive::read(bytes)?;
    let crypt = crypt_table()?;
    let hashes = archive.table(true, &crypt)?;
    let hash_a = hash_name(name, 1, &crypt)?;
    let hash_b = hash_name(name, 2, &crypt)?;
    let mut block_index = None;
    for entry in hashes.as_chunks::<16>().0 {
        let index = word(entry, 12)?;
        if index < 0xffff_fffe && word(entry, 0)? == hash_a && word(entry, 4)? == hash_b {
            if block_index.is_some() {
                return Err(format!("MPQ 成员 {name} 存在多个语言版本，无法唯一定位。"));
            }
            block_index = Some(usize::try_from(index).map_err(|_| "MPQ 块索引过大。")?);
        }
    }
    let index = block_index.ok_or_else(|| format!("录像中缺少 {name}。"))?;
    let blocks = archive.table(false, &crypt)?;
    let entry_offset = index.checked_mul(16).ok_or("MPQ 块索引溢出。")?;
    let entry = slice(&blocks, entry_offset, 16)?;
    let position = offset(entry, 0)?;
    let packed_size = offset(entry, 4)?;
    let size = offset(entry, 8)?;
    let flags = word(entry, 12)?;
    if flags & FILE_EXISTS == 0 || flags & 0x0200_0000 != 0 {
        return Err(format!("MPQ 成员 {name} 已删除或不存在。"));
    }
    if flags & UNSUPPORTED_FLAGS != 0 {
        return Err(format!(
            "MPQ 成员 {name} 使用暂不支持的加密、补丁或 implode 标记。"
        ));
    }
    if size > MAX_MEMBER_BYTES || packed_size > MAX_MEMBER_BYTES {
        return Err(format!("MPQ 成员 {name} 超过 32 MiB 限制。"));
    }
    if size == 0 && packed_size == 0 {
        return Ok(Vec::new());
    }
    if position < archive.header.len() {
        return Err("MPQ 成员与归档头重叠。".into());
    }
    let packed = slice(archive.data, position, packed_size)?;
    if flags & FILE_SINGLE_UNIT != 0 || size == 0 {
        unpack(packed, size, flags & FILE_COMPRESS != 0)
    } else {
        unpack_sectors(packed, size, flags, archive.sector_size)
    }
}
