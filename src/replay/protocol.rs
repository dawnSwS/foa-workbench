//! Bounded Rust implementation of Blizzard's bit-packed/versioned decoders.
//!
//! Schema and decoding rules: Blizzard/s2protocol 5.0.17.98310.0,
//! `s2protocol/versions/protocol98310.py` and `s2protocol/decoders.py`.
//! Copyright (c) 2013-2017 Blizzard Entertainment; MIT license, reproduced in
//! LICENSE-s2protocol.txt. Build selection and compatibility reporting belong
//! to the caller: this module implements only the bundled 98310 schema.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Number, Value};

const MAX_INPUT: usize = 64 * 1024 * 1024;
const MAX_BLOB: usize = 1024 * 1024;
const MAX_ITEMS: usize = 131_072;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 50_000_000;
const MAX_EVENTS: usize = 2_000_000;
const MAX_BANK_EVENTS: usize = 100_000;

#[derive(Deserialize)]
struct Bounds {
    min: i64,
    bits: u32,
}

#[derive(Deserialize)]
struct Field {
    name: String,
    type_id: usize,
    #[serde(default)]
    tag: i64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Type {
    Int {
        bounds: Bounds,
    },
    Array {
        bounds: Bounds,
        type_id: usize,
    },
    Bitarray {
        bounds: Bounds,
    },
    Blob {
        bounds: Bounds,
    },
    Bool,
    Choice {
        bounds: Bounds,
        fields: BTreeMap<String, Field>,
    },
    Fourcc,
    Null,
    Optional {
        type_id: usize,
    },
    Real32,
    Real64,
    Struct {
        fields: Vec<Field>,
    },
}

#[derive(Deserialize)]
struct Schema {
    types: Vec<Type>,
    events: BTreeMap<u64, Field>,
    tracker_events: BTreeMap<u64, Field>,
}

fn schema() -> Result<Schema, String> {
    serde_json::from_str(include_str!("protocol98310.json"))
        .map_err(|error| format!("内置录像协议表无效：{error}"))
}

struct Buffer<'a> {
    bytes: &'a [u8],
    used: usize,
    next: u8,
    remaining: u32,
}

impl<'a> Buffer<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() > MAX_INPUT {
            return Err("录像协议数据超过 64 MiB 限制。".into());
        }
        Ok(Self {
            bytes,
            used: 0,
            next: 0,
            remaining: 0,
        })
    }

    fn done(&self) -> bool {
        self.remaining == 0 && self.used == self.bytes.len()
    }

    fn align(&mut self) {
        self.remaining = 0;
    }

    fn used_bits(&self) -> usize {
        self.used * 8 - usize::try_from(self.remaining).unwrap_or(0)
    }

    fn bits(&mut self, bits: u32) -> Result<u64, String> {
        if bits > 64 {
            return Err("录像整数位宽超过 64。".into());
        }
        let mut value = 0_u64;
        let mut copied = 0;
        while copied < bits {
            if self.remaining == 0 {
                self.next = *self
                    .bytes
                    .get(self.used)
                    .ok_or_else(|| format!("录像协议数据截断（字节 {}）。", self.used))?;
                self.used += 1;
                self.remaining = 8;
            }
            let count = (bits - copied).min(self.remaining);
            let part = u64::from(self.next) & ((1_u64 << count) - 1);
            value |= part << (bits - copied - count);
            // Shifting a u8 by eight is invalid even when all bits are consumed.
            self.next =
                u8::try_from(u16::from(self.next) >> count).map_err(|error| error.to_string())?;
            self.remaining -= count;
            copied += count;
        }
        Ok(value)
    }

    fn aligned(&mut self, count: usize) -> Result<&'a [u8], String> {
        self.align();
        let end = self.used.checked_add(count).ok_or("录像数据长度溢出。")?;
        let result = self.bytes.get(self.used..end).ok_or_else(|| {
            format!(
                "录像协议数据截断（字节 {}，需要 {count} 字节）。",
                self.used
            )
        })?;
        self.used = end;
        Ok(result)
    }

    fn unaligned(&mut self, count: usize) -> Result<Vec<u8>, String> {
        (0..count)
            .map(|_| u8::try_from(self.bits(8)?).map_err(|error| error.to_string()))
            .collect()
    }
}

struct Decoder<'a> {
    buffer: Buffer<'a>,
    versioned: bool,
    nodes: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8], versioned: bool) -> Result<Self, String> {
        Ok(Self {
            buffer: Buffer::new(bytes)?,
            versioned,
            nodes: 0,
        })
    }

    fn budget(&mut self, depth: usize) -> Result<(), String> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            Err("录像协议复杂度超过解析限制。".into())
        } else {
            Ok(())
        }
    }

    fn expect(&mut self, tag: u64) -> Result<(), String> {
        if self.versioned && self.buffer.bits(8)? != tag {
            return Err(format!("录像协议类型标签不匹配（预期 {tag}）。"));
        }
        Ok(())
    }

    fn vint(&mut self) -> Result<i128, String> {
        let first = self.buffer.bits(8)?;
        let negative = first & 1 != 0;
        let mut value = u128::from((first >> 1) & 0x3f);
        let mut byte = first;
        let mut shift = 6;
        while byte & 0x80 != 0 {
            if shift >= 69 {
                return Err("录像变长整数超过 64 位。".into());
            }
            byte = self.buffer.bits(8)?;
            value |= u128::from(byte & 0x7f) << shift;
            shift += 7;
        }
        if value > u128::from(u64::MAX) {
            return Err("录像变长整数溢出。".into());
        }
        let magnitude = i128::try_from(value).map_err(|error| error.to_string())?;
        Ok(if negative { -magnitude } else { magnitude })
    }

    fn integer(&mut self, bounds: &Bounds) -> Result<i128, String> {
        if self.versioned {
            self.expect(9)?;
            self.vint()
        } else {
            Ok(i128::from(bounds.min) + i128::from(self.buffer.bits(bounds.bits)?))
        }
    }

    fn count(&mut self, bounds: &Bounds, limit: usize) -> Result<usize, String> {
        let number = if self.versioned {
            self.vint()?
        } else {
            self.integer(bounds)?
        };
        bounded_count(number, limit)
    }

    fn boolean(&mut self, optional: bool) -> Result<bool, String> {
        if !optional {
            self.expect(6)?;
        }
        self.buffer
            .bits(if self.versioned { 8 } else { 1 })
            .map(|value| value != 0)
    }

    fn instance(&mut self, schema: &Schema, type_id: usize, depth: usize) -> Result<Value, String> {
        self.budget(depth)?;
        let kind = schema
            .types
            .get(type_id)
            .ok_or("录像协议引用了未知类型。")?;
        match kind {
            Type::Int { bounds } => number(self.integer(bounds)?),
            Type::Bool => self.boolean(false).map(Value::Bool),
            Type::Null => Ok(Value::Null),
            Type::Optional { type_id } => {
                self.expect(4)?;
                if self.boolean(true)? {
                    self.instance(schema, *type_id, depth + 1)
                } else {
                    Ok(Value::Null)
                }
            }
            Type::Blob { bounds } => {
                self.expect(2)?;
                let count = self.count(bounds, MAX_BLOB)?;
                Ok(byte_array(self.buffer.aligned(count)?))
            }
            Type::Fourcc => {
                self.expect(7)?;
                Ok(byte_array(&self.buffer.unaligned(4)?))
            }
            Type::Array { bounds, type_id } => {
                self.expect(0)?;
                let count = self.count(bounds, MAX_ITEMS)?;
                let mut values = Vec::with_capacity(count.min(4096));
                for _ in 0..count {
                    values.push(self.instance(schema, *type_id, depth + 1)?);
                }
                Ok(Value::Array(values))
            }
            Type::Bitarray { bounds } => self.bit_array(bounds),
            Type::Choice { bounds, fields } => self.choice(schema, bounds, fields, depth),
            Type::Struct { fields } => self.structure(schema, fields, depth),
            Type::Real32 => self.real(false),
            Type::Real64 => self.real(true),
        }
    }

    fn bit_array(&mut self, bounds: &Bounds) -> Result<Value, String> {
        self.expect(1)?;
        let count = self.count(bounds, MAX_BLOB * 8)?;
        let mut bytes = Vec::with_capacity(count.div_ceil(8));
        if self.versioned {
            bytes.extend_from_slice(self.buffer.aligned(count.div_ceil(8))?);
        } else {
            bytes.resize(count.div_ceil(8), 0);
            let mut output_bit = bytes.len() * 8 - count;
            let mut remaining = count;
            while remaining > 0 {
                // Blizzard's big-endian reader concatenates fragments at the
                // *input byte boundaries*. Repeated read_bits(8) calls at an
                // unaligned position do not preserve that fragment order.
                let available = if self.buffer.remaining == 0 {
                    8
                } else {
                    self.buffer.remaining
                };
                let take =
                    remaining.min(usize::try_from(available).map_err(|error| error.to_string())?);
                let fragment = self
                    .buffer
                    .bits(u32::try_from(take).map_err(|error| error.to_string())?)?;
                for bit in (0..take).rev() {
                    let value =
                        u8::try_from((fragment >> bit) & 1).map_err(|error| error.to_string())?;
                    let output = bytes.get_mut(output_bit / 8).ok_or("位数组输出越界。")?;
                    *output |= value << (7 - output_bit % 8);
                    output_bit += 1;
                }
                remaining -= take;
            }
        }
        Ok(serde_json::json!({"bit_length": count, "bytes": bytes}))
    }

    fn choice(
        &mut self,
        schema: &Schema,
        bounds: &Bounds,
        fields: &BTreeMap<String, Field>,
        depth: usize,
    ) -> Result<Value, String> {
        self.expect(3)?;
        let tag = if self.versioned {
            self.vint()?
        } else {
            self.integer(bounds)?
        };
        let tag = i64::try_from(tag).map_err(|error| error.to_string())?;
        let mut result = Map::new();
        if let Some(field) = fields.get(&tag.to_string()) {
            result.insert(
                field.name.clone(),
                self.instance(schema, field.type_id, depth + 1)?,
            );
        } else if self.versioned {
            self.skip(depth + 1)?;
        } else {
            return Err(format!("录像协议含未知 choice 标签 {tag}。"));
        }
        Ok(Value::Object(result))
    }

    fn structure(
        &mut self,
        schema: &Schema,
        fields: &[Field],
        depth: usize,
    ) -> Result<Value, String> {
        self.expect(5)?;
        let count = if self.versioned {
            bounded_count(self.vint()?, MAX_ITEMS)?
        } else {
            fields.len()
        };
        let mut result = Map::new();
        for index in 0..count {
            let field = if self.versioned {
                let tag = self.vint()?;
                fields.iter().find(|field| i128::from(field.tag) == tag)
            } else {
                fields.get(index)
            };
            if let Some(field) = field {
                let value = self.instance(schema, field.type_id, depth + 1)?;
                if field.name == "__parent" {
                    if let Value::Object(parent) = value {
                        result.extend(parent);
                    } else if fields.len() == 1 {
                        return Ok(value);
                    } else {
                        result.insert(field.name.clone(), value);
                    }
                } else if result.insert(field.name.clone(), value).is_some() {
                    return Err(format!("录像协议含重复字段 {}。", field.name));
                }
            } else {
                self.skip(depth + 1)?;
            }
        }
        Ok(Value::Object(result))
    }

    fn real(&mut self, double: bool) -> Result<Value, String> {
        self.expect(if double { 8 } else { 7 })?;
        let value = if double {
            let bytes: [u8; 8] = self
                .buffer
                .unaligned(8)?
                .try_into()
                .map_err(|_| "浮点数长度无效。")?;
            f64::from_be_bytes(bytes)
        } else {
            let bytes: [u8; 4] = self
                .buffer
                .unaligned(4)?
                .try_into()
                .map_err(|_| "浮点数长度无效。")?;
            f64::from(f32::from_be_bytes(bytes))
        };
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| "录像协议含非有限浮点数。".into())
    }

    fn skip(&mut self, depth: usize) -> Result<(), String> {
        self.budget(depth)?;
        match self.buffer.bits(8)? {
            0 => {
                let count = bounded_count(self.vint()?, MAX_ITEMS)?;
                for _ in 0..count {
                    self.skip(depth + 1)?;
                }
            }
            tag @ (1 | 2) => {
                let count = bounded_count(self.vint()?, MAX_BLOB * 8)?;
                let bytes = if tag == 1 { count.div_ceil(8) } else { count };
                if bytes > MAX_BLOB {
                    return Err("录像未知字段过大。".into());
                }
                self.buffer.aligned(bytes)?;
            }
            3 => {
                self.vint()?;
                self.skip(depth + 1)?;
            }
            4 => {
                if self.buffer.bits(8)? != 0 {
                    self.skip(depth + 1)?;
                }
            }
            5 => {
                let count = bounded_count(self.vint()?, MAX_ITEMS)?;
                for _ in 0..count {
                    self.vint()?;
                    self.skip(depth + 1)?;
                }
            }
            6 => {
                self.buffer.aligned(1)?;
            }
            7 => {
                self.buffer.aligned(4)?;
            }
            8 => {
                self.buffer.aligned(8)?;
            }
            9 => {
                self.vint()?;
            }
            tag => return Err(format!("录像协议含未知跳过标签 {tag}。")),
        }
        Ok(())
    }
}

fn number(value: i128) -> Result<Value, String> {
    if let Ok(value) = i64::try_from(value) {
        Ok(Value::from(value))
    } else {
        u64::try_from(value)
            .map(Value::from)
            .map_err(|_| "录像整数超出 JSON 范围。".into())
    }
}

fn byte_array(bytes: &[u8]) -> Value {
    Value::Array(bytes.iter().copied().map(Value::from).collect())
}

fn bounded_count(value: i128, limit: usize) -> Result<usize, String> {
    let count = usize::try_from(value).map_err(|_| "录像长度为负数或过大。")?;
    if count > limit {
        Err(format!("录像长度 {count} 超过限制 {limit}。"))
    } else {
        Ok(count)
    }
}

fn record(bytes: &[u8], type_id: usize, versioned: bool) -> Result<Value, String> {
    let schema = schema()?;
    let mut decoder = Decoder::new(bytes, versioned)?;
    let result = decoder.instance(&schema, type_id, 0)?;
    decoder.buffer.align();
    if !decoder.buffer.done() {
        return Err("录像记录含未消费的尾部数据，协议可能不兼容。".into());
    }
    Ok(result)
}

pub fn header(bytes: &[u8]) -> Result<Value, String> {
    record(bytes, 18, true)
}
pub fn details(bytes: &[u8]) -> Result<Value, String> {
    record(bytes, 40, true)
}
pub fn init_data(bytes: &[u8]) -> Result<Value, String> {
    record(bytes, 73, false)
}

pub struct GameEvents {
    pub banks: Vec<Value>,
    pub loading_sync: Vec<Value>,
}

/// Consume the complete stream before absence of a preloaded bank is usable.
pub fn game_events(bytes: &[u8]) -> Result<GameEvents, String> {
    let schema = schema()?;
    let mut decoder = Decoder::new(bytes, false)?;
    let mut gameloop = 0_u64;
    let mut events = Vec::new();
    let mut loading_sync = Vec::new();
    let mut count = 0;
    while !decoder.buffer.done() {
        count += 1;
        if count > MAX_EVENTS {
            return Err("录像事件数量超过解析限制。".into());
        }
        let start = decoder.buffer.used_bits();
        let delta = decoder.instance(&schema, 7, 0)?;
        let delta = delta
            .as_object()
            .and_then(|map| map.values().next())
            .and_then(Value::as_u64)
            .ok_or("录像事件时间差无效。")?;
        gameloop = gameloop.checked_add(delta).ok_or("录像时间溢出。")?;
        let user_id = decoder.instance(&schema, 8, 0)?;
        let event_id = decoder
            .instance(&schema, 0, 0)?
            .as_u64()
            .ok_or("录像事件 ID 无效。")?;
        let event_type = schema
            .events
            .get(&event_id)
            .ok_or_else(|| format!("录像含未知事件 {event_id}（bit {start}），协议可能不兼容。"))?;
        let mut event = decoder
            .instance(&schema, event_type.type_id, 0)
            .map_err(|error| format!("{} / bit {start}：{error}", event_type.name))?;
        decoder.buffer.align();
        let bank_event = event_type.name.starts_with("NNet.Game.SBank");
        let loading_event = matches!(
            event_type.name.as_str(),
            "NNet.Game.SUserFinishedLoadingSyncEvent" | "NNet.Game.SUserOptionsEvent"
        );
        if bank_event || loading_event {
            if events.len() + loading_sync.len() >= MAX_BANK_EVENTS {
                return Err("录像 Bank 事件超过解析限制。".into());
            }
            let fields = event.as_object_mut().ok_or("录像事件必须是对象。")?;
            fields.insert("_event".into(), Value::from(event_type.name.clone()));
            fields.insert("_eventid".into(), Value::from(event_id));
            fields.insert("_gameloop".into(), Value::from(gameloop));
            fields.insert("_userid".into(), user_id);
            fields.insert(
                "_bits".into(),
                Value::from(decoder.buffer.used_bits() - start),
            );
            if bank_event {
                events.push(event);
            } else {
                loading_sync.push(event);
            }
        }
    }
    Ok(GameEvents {
        banks: events,
        loading_sync,
    })
}

/// Consume the entire tracker stream, retaining setup and upgrade facts.
pub fn tracker_events(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let schema = schema()?;
    let mut decoder = Decoder::new(bytes, true)?;
    let mut gameloop = 0_u64;
    let mut events = Vec::new();
    let mut count = 0;
    while !decoder.buffer.done() {
        count += 1;
        if count > MAX_EVENTS {
            return Err("录像 Tracker 事件数量超过解析限制。".into());
        }
        let start = decoder.buffer.used_bits();
        let delta = decoder.instance(&schema, 7, 0)?;
        let delta = delta
            .as_object()
            .and_then(|map| map.values().next())
            .and_then(Value::as_u64)
            .ok_or("录像 Tracker 时间差无效。")?;
        gameloop = gameloop.checked_add(delta).ok_or("录像时间溢出。")?;
        let event_id = decoder
            .instance(&schema, 2, 0)?
            .as_u64()
            .ok_or("录像 Tracker 事件 ID 无效。")?;
        let event_type = schema
            .tracker_events
            .get(&event_id)
            .ok_or_else(|| format!("录像含未知 Tracker 事件 {event_id}（bit {start}）。"))?;
        let mut event = decoder
            .instance(&schema, event_type.type_id, 0)
            .map_err(|error| format!("{} / bit {start}：{error}", event_type.name))?;
        decoder.buffer.align();
        if matches!(event_id, 5 | 9) {
            if events.len() >= MAX_BANK_EVENTS {
                return Err("录像 Tracker 升级与槽位事件超过解析限制。".into());
            }
            let fields = event.as_object_mut().ok_or("录像事件必须是对象。")?;
            fields.insert("_event".into(), Value::from(event_type.name.clone()));
            fields.insert("_eventid".into(), Value::from(event_id));
            fields.insert("_gameloop".into(), Value::from(gameloop));
            fields.insert(
                "_bits".into(),
                Value::from(decoder.buffer.used_bits() - start),
            );
            events.push(event);
        }
    }
    Ok(events)
}
