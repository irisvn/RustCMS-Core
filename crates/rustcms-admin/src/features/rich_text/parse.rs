//! Parsing: a JSON value becomes a [`Document`] only if it follows the closed
//! grammar within the size limits.

use super::*;

pub(super) const MAX_NODES: usize = 10_000;

pub(super) const MAX_DEPTH: usize = 16;

pub(super) const MAX_IMAGES: usize = 100;

pub(super) const MAX_HREF: usize = 2_048;

pub(super) const MAX_ALT: usize = 500;

/// Marks on one text (each kind at most once).
pub(super) const MAX_MARKS: usize = 5;

pub(super) fn err(message: impl Into<String>) -> DocumentError {
    DocumentError(message.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawNode {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    content: Option<Vec<RawNode>>,
    #[serde(default)]
    attrs: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    marks: Option<Vec<RawMark>>,
    #[serde(default)]
    version: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawMark {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    attrs: Option<serde_json::Map<String, Value>>,
}

pub(super) struct Budget {
    nodes: usize,
    images: usize,
}

/// Parses and validates a stored or submitted document.
pub fn parse(value: &Value) -> Result<Document, DocumentError> {
    // Bounded before any deep work.
    let bytes = serde_json::to_vec(value).map_err(|_| err("is not a document"))?.len();
    if bytes > MAX_BYTES {
        return Err(err(format!("may not be larger than {} KiB", MAX_BYTES / 1024)));
    }
    if depth(value, 0) > MAX_DEPTH * 2 + 4 {
        return Err(err("is nested too deeply"));
    }
    let raw: RawNode = serde_json::from_value(value.clone()).map_err(|_| err("is not a valid document"))?;
    if raw.kind != "doc"
        || raw.version != Some(VERSION)
        || raw.attrs.is_some()
        || raw.text.is_some()
        || raw.marks.is_some()
    {
        return Err(err("is not a valid document (version 1)"));
    }
    let mut budget = Budget { nodes: 0, images: 0 };
    let content = raw.content.unwrap_or_default();
    // List items only inside lists (as the editor's schema requires).
    if content.iter().any(|n| n.kind == "listItem") {
        return Err(err("has a list item outside a list"));
    }
    let content = blocks(content, &mut budget, 1)?;
    Ok(Document { content })
}

/// JSON nesting depth (arrays and objects).
pub(super) fn depth(value: &Value, level: usize) -> usize {
    match value {
        Value::Array(items) => items.iter().map(|v| depth(v, level + 1)).max().unwrap_or(level + 1),
        Value::Object(map) => map.values().map(|v| depth(v, level + 1)).max().unwrap_or(level + 1),
        _ => level,
    }
}

pub(super) fn count(budget: &mut Budget, level: usize) -> Result<(), DocumentError> {
    budget.nodes += 1;
    if budget.nodes > MAX_NODES {
        return Err(err(format!("may not have more than {MAX_NODES} elements")));
    }
    if level > MAX_DEPTH {
        return Err(err("is nested too deeply"));
    }
    Ok(())
}

pub(super) fn no_extras(
    raw: &RawNode,
    attrs: bool,
    text: bool,
    marks: bool,
    content: bool,
) -> Result<(), DocumentError> {
    let ok = (attrs || raw.attrs.is_none())
        && (text || raw.text.is_none())
        && (marks || raw.marks.is_none())
        && (content || raw.content.is_none())
        && raw.version.is_none();
    if ok { Ok(()) } else { Err(err(format!("has unexpected properties on `{}`", raw.kind))) }
}

pub(super) fn blocks(raw: Vec<RawNode>, budget: &mut Budget, level: usize) -> Result<Vec<Node>, DocumentError> {
    raw.into_iter().map(|node| block(node, budget, level)).collect()
}

pub(super) fn block(raw: RawNode, budget: &mut Budget, level: usize) -> Result<Node, DocumentError> {
    count(budget, level)?;
    Ok(match raw.kind.as_str() {
        "paragraph" => {
            no_extras(&raw, false, false, false, true)?;
            Node::Paragraph(inlines(raw.content.unwrap_or_default(), budget, level + 1)?)
        }
        "heading" => {
            no_extras(&raw, true, false, false, true)?;
            let attrs = raw.attrs.clone().unwrap_or_default();
            let level_attr = attrs.get("level").and_then(Value::as_u64).filter(|l| (2..=4).contains(l));
            let Some(heading) = level_attr.filter(|_| attrs.len() == 1) else {
                return Err(err("has a heading without a level from 2 to 4"));
            };
            Node::Heading(heading as u8, inlines(raw.content.unwrap_or_default(), budget, level + 1)?)
        }
        "bulletList" | "orderedList" => {
            no_extras(&raw, false, false, false, true)?;
            let kind = raw.kind.clone();
            let items = raw.content.unwrap_or_default();
            if items.iter().any(|i| i.kind != "listItem") {
                return Err(err("has a list holding something else than list items"));
            }
            let items = blocks(items, budget, level + 1)?;
            if kind == "bulletList" { Node::BulletList(items) } else { Node::OrderedList(items) }
        }
        "listItem" | "blockquote" => {
            no_extras(&raw, false, false, false, true)?;
            let kind = raw.kind.clone();
            let items = raw.content.unwrap_or_default();
            if items.iter().any(|n| n.kind == "listItem") {
                return Err(err("has a list item outside a list"));
            }
            let items = blocks(items, budget, level + 1)?;
            if kind == "listItem" { Node::ListItem(items) } else { Node::Blockquote(items) }
        }
        "codeBlock" => {
            no_extras(&raw, false, false, false, true)?;
            let mut text = String::new();
            for part in raw.content.unwrap_or_default() {
                match part {
                    RawNode { kind, text: Some(t), marks: None, attrs: None, content: None, version: None }
                        if kind == "text" =>
                    {
                        count(budget, level + 1)?;
                        text.push_str(&t);
                    }
                    _ => return Err(err("has a code block holding something else than plain text")),
                }
            }
            Node::CodeBlock(text)
        }
        "horizontalRule" => {
            no_extras(&raw, false, false, false, false)?;
            Node::HorizontalRule
        }
        "image" => image(raw, budget)?,
        other => return Err(err(format!("has an unknown block `{}`", other.escape_debug()))),
    })
}

pub(super) fn image(raw: RawNode, budget: &mut Budget) -> Result<Node, DocumentError> {
    no_extras(&raw, true, false, false, false)?;
    budget.images += 1;
    if budget.images > MAX_IMAGES {
        return Err(err(format!("may not have more than {MAX_IMAGES} images")));
    }
    let attrs = raw.attrs.unwrap_or_default();
    if attrs.keys().any(|k| k != "media" && k != "alt") {
        return Err(err("has an image with unexpected attributes"));
    }
    let media = attrs
        .get("media")
        .and_then(Value::as_str)
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .ok_or_else(|| err("has an image that is not a media item"))?;
    let alt = match attrs.get("alt") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(alt)) if alt.chars().count() <= MAX_ALT => alt.clone(),
        _ => return Err(err(format!("has an image description longer than {MAX_ALT} characters"))),
    };
    Ok(Node::Image { media, alt })
}

pub(super) fn inlines(raw: Vec<RawNode>, budget: &mut Budget, level: usize) -> Result<Vec<Node>, DocumentError> {
    let mut out = Vec::with_capacity(raw.len());
    for node in raw {
        count(budget, level)?;
        out.push(match node.kind.as_str() {
            "text" => {
                no_extras(&node, false, true, true, false)?;
                let text = node.text.ok_or_else(|| err("has a text without text"))?;
                if text.is_empty() {
                    return Err(err("has an empty text"));
                }
                let raw_marks = node.marks.unwrap_or_default();
                if raw_marks.len() > MAX_MARKS {
                    return Err(err("has a text with too many marks"));
                }
                let marks: Vec<Mark> = raw_marks.into_iter().map(mark).collect::<Result<_, _>>()?;
                let kinds: std::collections::HashSet<_> = marks.iter().map(std::mem::discriminant).collect();
                if kinds.len() != marks.len() {
                    return Err(err("has a text with the same mark twice"));
                }
                Node::Text(text, marks)
            }
            "hardBreak" => {
                no_extras(&node, false, false, false, false)?;
                Node::HardBreak
            }
            // Images are blocks (the editor's schema): never inside text.
            other => return Err(err(format!("has an unknown inline `{}`", other.escape_debug()))),
        });
    }
    Ok(out)
}

pub(super) fn mark(raw: RawMark) -> Result<Mark, DocumentError> {
    let plain = |mark: Mark| if raw.attrs.is_none() { Ok(mark) } else { Err(err("has a mark with attributes")) };
    match raw.kind.as_str() {
        "bold" => plain(Mark::Bold),
        "italic" => plain(Mark::Italic),
        "strike" => plain(Mark::Strike),
        "code" => plain(Mark::Code),
        "link" => {
            let attrs = raw.attrs.unwrap_or_default();
            if attrs.keys().any(|k| k != "href") {
                return Err(err("has a link with unexpected attributes"));
            }
            let href = attrs.get("href").and_then(Value::as_str).unwrap_or_default();
            if safe_href(href) {
                Ok(Mark::Link(href.to_owned()))
            } else {
                Err(err("has a link that is not http, https or mailto"))
            }
        }
        other => Err(err(format!("has an unknown mark `{}`", other.escape_debug()))),
    }
}
