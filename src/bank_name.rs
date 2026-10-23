use std::collections::BTreeSet;
use std::fmt::Write as _;

use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::Reader;

use super::attribute;

#[derive(Default)]
pub(super) struct XmlBank {
    pub version: Option<u32>,
    pub payload: Option<String>,
    pub player_name: Option<String>,
    pub signature: bool,
}

#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    closing: usize,
    empty: bool,
}

enum Node {
    Bank,
    Section(String),
    Key {
        name: String,
        value_seen: bool,
    },
    Value {
        attribute: Option<String>,
        text: String,
    },
    Signature,
}

struct Frame {
    node: Node,
    span: Span,
}

#[derive(Default)]
struct Layout {
    bank: XmlBank,
    root: Option<Span>,
    fa: Option<Span>,
    name_key: Option<Span>,
    name_value: Option<Span>,
    payload: Option<Span>,
    signatures: Vec<Span>,
}

#[derive(Default)]
struct Parser {
    layout: Layout,
    stack: Vec<Frame>,
    root_seen: bool,
    sections_seen: BTreeSet<String>,
    keys_seen: BTreeSet<(String, String)>,
}

fn scalar_attribute(element: &BytesStart<'_>) -> Result<Option<String>, String> {
    let mut found = None;
    for key in ["string", "int", "value", "text", "real", "bool"] {
        if let Some(value) = attribute(element, key)? {
            if found.is_some() {
                return Err("同一 Value 含多个数值属性，无法确定有效值。".into());
            }
            found = Some(value);
        }
    }
    Ok(found)
}

fn valid_xml_text(text: &str) -> bool {
    text.chars().all(|character| {
        (character >= ' ' || matches!(character, '\t' | '\n' | '\r'))
            && !matches!(character, '\u{fffe}' | '\u{ffff}')
    })
}

impl Parser {
    fn start(&mut self, element: &BytesStart<'_>, span: Span) -> Result<(), String> {
        // Scan all attributes, including attributes on nodes that are preserved.
        for attr in element.attributes() {
            let attr = attr.map_err(|error| format!("XML 属性错误：{error}"))?;
            let value = quick_xml::escape::unescape(&attr.value)
                .map_err(|error| format!("XML 转义错误：{error}"))?;
            if !valid_xml_text(&value) {
                return Err("XML 属性含非法字符。".into());
            }
        }
        let node = match element.name().as_ref() {
            "Bank" if self.stack.is_empty() && !self.root_seen => {
                self.root_seen = true;
                Node::Bank
            }
            "Section" if matches!(self.stack.last().map(|frame| &frame.node), Some(Node::Bank)) => {
                let name = attribute(element, "name")?.ok_or("Section 缺少 name 属性。")?;
                let canonical = name.to_ascii_lowercase();
                if matches!(canonical.as_str(), "fa" | "encrypted")
                    && !self.sections_seen.insert(canonical)
                {
                    return Err(format!("XML 含重复的 {name} 节，无法安全编辑。"));
                }
                Node::Section(name)
            }
            "Key" => self.start_key(element)?,
            "Value" => {
                let Some(Frame {
                    node: Node::Key { value_seen, .. },
                    ..
                }) = self.stack.last_mut()
                else {
                    return Err("Value 必须位于 Key 内。".into());
                };
                if *value_seen {
                    return Err("同一 Key 含多个 Value，无法确定有效值。".into());
                }
                *value_seen = true;
                Node::Value {
                    attribute: scalar_attribute(element)?,
                    text: String::new(),
                }
            }
            "Signature"
                if matches!(self.stack.last().map(|frame| &frame.node), Some(Node::Bank)) =>
            {
                Node::Signature
            }
            _ => return Err("SC2Bank XML 标签位置或结构无效。".into()),
        };
        self.stack.push(Frame { node, span });
        Ok(())
    }

    fn start_key(&mut self, element: &BytesStart<'_>) -> Result<Node, String> {
        let Some(Frame {
            node: Node::Section(section),
            ..
        }) = self.stack.last()
        else {
            return Err("Key 必须位于 Section 内。".into());
        };
        let name = attribute(element, "name")?.ok_or("Key 缺少 name 属性。")?;
        let pair = (section.to_ascii_lowercase(), name.to_ascii_lowercase());
        let relevant = matches!(
            (pair.0.as_str(), pair.1.as_str()),
            ("fa", "name" | "bankversion") | ("encrypted", "bank")
        );
        if relevant && !self.keys_seen.insert(pair) {
            return Err(format!("XML 含重复的 {section} / {name}，无法安全编辑。"));
        }
        Ok(Node::Key {
            name,
            value_seen: false,
        })
    }

    fn text(&mut self, value: &str) -> Result<(), String> {
        if !valid_xml_text(value) {
            return Err("XML 文本含非法字符。".into());
        }
        match self.stack.last_mut().map(|frame| &mut frame.node) {
            Some(Node::Value { text, .. }) => text.push_str(value),
            Some(Node::Signature) => {}
            _ if !value.trim().is_empty() => {
                return Err("SC2Bank XML 包含标签外的非空白文本。".into())
            }
            _ => {}
        }
        Ok(())
    }

    fn reference(&mut self, value: &BytesRef<'_>) -> Result<(), String> {
        if let Some(character) = value
            .resolve_char_ref()
            .map_err(|error| error.to_string())?
        {
            self.text(character.encode_utf8(&mut [0; 4]))
        } else {
            self.text(
                quick_xml::escape::resolve_predefined_entity(value)
                    .ok_or_else(|| format!("XML 转义错误：未知实体 &{};", value.as_ref()))?,
            )
        }
    }

    fn finish(&mut self, closing: usize, end: usize) -> Result<(), String> {
        let mut frame = self.stack.pop().ok_or("XML 含多余的闭合标签。")?;
        frame.span.closing = closing;
        frame.span.end = end;
        match frame.node {
            Node::Bank => self.layout.root = Some(frame.span),
            Node::Section(name) if name.eq_ignore_ascii_case("FA") => {
                self.layout.fa = Some(frame.span);
            }
            Node::Key { name, .. }
                if name.eq_ignore_ascii_case("Name") && self.in_section("FA") =>
            {
                self.layout.name_key = Some(frame.span);
            }
            Node::Key {
                name,
                value_seen: false,
            } if (name.eq_ignore_ascii_case("BankVersion") && self.in_section("FA"))
                || (name.eq_ignore_ascii_case("Bank") && self.in_section("Encrypted")) =>
            {
                return Err("存档核心键缺少 Value，不能按缺失存档处理。".into());
            }
            Node::Value { attribute, text } => {
                if attribute.is_some() && !text.trim().is_empty() {
                    return Err("Value 同时含属性值和文本值，无法确定有效值。".into());
                }
                self.store_value(attribute.unwrap_or(text), frame.span)?;
            }
            Node::Signature => {
                self.layout.bank.signature = true;
                self.layout.signatures.push(frame.span);
            }
            _ => {}
        }
        Ok(())
    }

    fn in_section(&self, target: &str) -> bool {
        self.stack.iter().any(
            |frame| matches!(&frame.node, Node::Section(name) if name.eq_ignore_ascii_case(target)),
        )
    }

    fn store_value(&mut self, value: String, span: Span) -> Result<(), String> {
        let Some(Frame {
            node: Node::Key { name, .. },
            ..
        }) = self.stack.last()
        else {
            return Err("Value 不在 Key 内。".into());
        };
        if self.in_section("FA") && name.eq_ignore_ascii_case("BankVersion") {
            self.layout.bank.version = Some(
                value
                    .parse()
                    .map_err(|error| format!("BankVersion 无效：{error}"))?,
            );
        } else if self.in_section("FA") && name.eq_ignore_ascii_case("Name") {
            self.layout.bank.player_name = Some(value);
            self.layout.name_value = Some(span);
        } else if self.in_section("Encrypted") && name.eq_ignore_ascii_case("Bank") {
            self.layout.bank.payload = Some(value);
            self.layout.payload = Some(span);
        }
        Ok(())
    }
}

fn inspect(text: &str) -> Result<Layout, String> {
    if !valid_xml_text(text) {
        return Err("XML 含非法字符。".into());
    }
    let content = text.trim_start_matches('\u{feff}');
    let prefix = text.len() - content.len();
    let mut reader = Reader::from_str(content);
    reader.config_mut().check_comments = true;
    let mut parser = Parser::default();
    loop {
        let start = prefix
            + usize::try_from(reader.buffer_position()).map_err(|error| error.to_string())?;
        let event = reader
            .read_event()
            .map_err(|error| format!("XML 格式错误：{error}"))?;
        let end = prefix
            + usize::try_from(reader.buffer_position()).map_err(|error| error.to_string())?;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                let empty = matches!(event, Event::Empty(_));
                parser.start(
                    element,
                    Span {
                        start,
                        end,
                        closing: start,
                        empty,
                    },
                )?;
                if empty {
                    parser.finish(start, end)?;
                }
            }
            Event::End(_) => parser.finish(start, end)?,
            Event::Text(value) => parser.text(&value)?,
            Event::CData(value) => parser.text(&value)?,
            Event::GeneralRef(value) => parser.reference(&value)?,
            Event::DocType(_) => return Err("SC2Bank 不支持 DTD 声明。".into()),
            Event::Decl(_) if parser.root_seen => return Err("XML 声明必须位于根标签之前。".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !parser.stack.is_empty() || parser.layout.root.is_none() {
        return Err("文件不是完整的 SC2Bank XML 存档。".into());
    }
    Ok(parser.layout)
}

pub(super) fn read_xml(text: &str) -> Result<XmlBank, String> {
    Ok(inspect(text)?.bank)
}

pub fn validate_player_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("局内名字不能为空或只含空白。".into());
    }
    if name
        .chars()
        .any(|character| character.is_control() || matches!(character, '\u{fffe}' | '\u{ffff}'))
    {
        return Err("局内名字不能含控制字符或 XML 非法字符。".into());
    }
    // This is a storage guard, not a claim about Galaxy StringLength's unit.
    if name.len() > 1024 {
        return Err("局内名字超过工具的 1024 UTF-8 字节存储上限。".into());
    }
    Ok(())
}

struct Replacement {
    start: usize,
    end: usize,
    text: String,
}

fn insert_child(text: &str, parent: Span, tag: &str, child: &str) -> Result<Replacement, String> {
    if parent.empty {
        let opening = text
            .get(parent.start..parent.end)
            .ok_or("XML 编辑范围无效。")?
            .strip_suffix("/>")
            .ok_or("XML 空标签无效。")?;
        Ok(Replacement {
            start: parent.start,
            end: parent.end,
            text: format!("{opening}>{child}</{tag}>"),
        })
    } else {
        Ok(Replacement {
            start: parent.closing,
            end: parent.closing,
            text: child.to_owned(),
        })
    }
}

fn name_change(text: &str, layout: &Layout, name: &str) -> Result<Replacement, String> {
    let value = format!("<Value string=\"{}\"/>", quick_xml::escape::escape(name));
    if let Some(span) = layout.name_value {
        return Ok(Replacement {
            start: span.start,
            end: span.end,
            text: value_markup(text, span, name)?,
        });
    }
    if let Some(span) = layout.name_key {
        return insert_child(text, span, "Key", &value);
    }
    let key = format!("<Key name=\"Name\">{value}</Key>");
    if let Some(span) = layout.fa {
        return insert_child(text, span, "Section", &key);
    }
    let section = format!("<Section name=\"FA\">{key}</Section>");
    insert_child(
        text,
        layout.root.ok_or("XML 根标签缺失。")?,
        "Bank",
        &section,
    )
}

fn value_markup(text: &str, span: Span, value: &str) -> Result<String, String> {
    let source = text.get(span.start..span.end).ok_or("XML 编辑范围无效。")?;
    let mut reader = Reader::from_str(source);
    let event = reader.read_event().map_err(|error| error.to_string())?;
    let (Event::Start(element) | Event::Empty(element)) = event else {
        return Err("XML 数值标签无效。".into());
    };
    let mut output = String::from("<Value");
    for attr in element.attributes() {
        let attr = attr.map_err(|error| error.to_string())?;
        if ["string", "int", "value", "text", "real", "bool"]
            .iter()
            .any(|key| attr.key.as_ref().eq_ignore_ascii_case(key))
        {
            continue;
        }
        let key = attr.key.as_ref();
        let preserved =
            quick_xml::escape::unescape(&attr.value).map_err(|error| error.to_string())?;
        write!(
            output,
            " {key}=\"{}\"",
            quick_xml::escape::escape(preserved.as_ref())
        )
        .map_err(|error| error.to_string())?;
    }
    write!(output, " string=\"{}\"/>", quick_xml::escape::escape(value))
        .map_err(|error| error.to_string())?;
    Ok(output)
}

pub(super) fn update_xml(
    text: &str,
    code: Option<&str>,
    name: Option<&str>,
) -> Result<String, String> {
    let layout = inspect(text)?;
    let mut changes = Vec::new();
    if let Some(name) = name {
        validate_player_name(name)?;
        if layout.bank.player_name.as_deref() != Some(name) {
            changes.push(name_change(text, &layout, name)?);
        }
    }
    if let Some(code) = code {
        let span = layout
            .payload
            .ok_or("存档缺少 Encrypted / Bank，不能修改核心字段。")?;
        changes.push(Replacement {
            start: span.start,
            end: span.end,
            text: value_markup(text, span, code)?,
        });
    }
    if changes.is_empty() {
        return Ok(text.to_owned());
    }
    for span in layout.signatures {
        changes.push(Replacement {
            start: span.start,
            end: span.end,
            text: String::new(),
        });
    }
    changes.sort_by_key(|change| change.start);
    let mut result = String::new();
    let mut cursor = 0;
    for change in changes {
        result.push_str(text.get(cursor..change.start).ok_or("XML 编辑范围重叠。")?);
        result.push_str(&change.text);
        cursor = change.end;
    }
    result.push_str(text.get(cursor..).ok_or("XML 编辑范围无效。")?);
    Ok(result)
}
