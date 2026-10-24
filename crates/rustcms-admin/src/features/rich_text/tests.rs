use super::*;
use serde_json::json;

const MEDIA: &str = "0199d3a4-1b2c-7d3e-8f40-123456789abc";

fn v1(content: Value) -> Value {
    json!({"type": "doc", "version": 1, "content": content})
}

fn doc(content: Value) -> Value {
    json!({"type": "doc", "version": 2, "content": content})
}

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

fn para(inline: Value) -> Value {
    json!({"type": "paragraph", "content": [inline]})
}

fn cell(kind: &str, t: &str) -> Value {
    json!({"type": kind, "content": [para(text(t))]})
}

/// A `rows` × `columns` table, the first row a header row.
fn table(rows: usize, columns: usize) -> Value {
    let row = |kind: &str| json!({"type": "tableRow", "content": (0..columns).map(|c| cell(kind, &c.to_string())).collect::<Vec<_>>()});
    let mut content = vec![row("tableHeader")];
    content.extend((1..rows).map(|_| row("tableCell")));
    json!({"type": "table", "content": content})
}

fn embed(provider: &str, id: &str) -> Value {
    json!({"type": "embed", "attrs": {"provider": provider, "id": id}})
}

#[test]
fn valid_documents_round_trip_to_escaped_html() {
    let value = v1(json!([
        {"type": "heading", "attrs": {"level": 2}, "content": [{"type": "text", "text": "Title <script>"}]},
        {"type": "paragraph", "content": [
            {"type": "text", "text": "a \"b\" & 'c'", "marks": [{"type": "bold"}, {"type": "link", "attrs": {"href": "https://x.test/?a=1&b=\"2\""}}]},
            {"type": "hardBreak"}
        ]},
        {"type": "bulletList", "content": [{"type": "listItem", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "i"}]}]}]},
        {"type": "codeBlock", "content": [{"type": "text", "text": "<b>x</b>"}]},
        {"type": "image", "attrs": {"media": MEDIA, "alt": "\" onerror=\"x"}},
        {"type": "horizontalRule"}
    ]));
    let parsed = parse(&value).unwrap();
    assert_eq!(parsed.version, 1);
    assert_eq!(media_ids(&parsed), vec![uuid::Uuid::parse_str(MEDIA).unwrap()]);
    let html = to_html(&parsed, &|id| Some(format!("/media/{id}")));
    assert_eq!(
        html,
        "<h2>Title &lt;script&gt;</h2><p><a href=\"https://x.test/?a=1&amp;b=&quot;2&quot;\" rel=\"noopener noreferrer nofollow\">\
         <strong>a &quot;b&quot; &amp; &#39;c&#39;</strong></a><br></p><ul><li><p>i</p></li></ul>\
         <pre><code>&lt;b&gt;x&lt;/b&gt;</code></pre><img src=\"/media/0199d3a4-1b2c-7d3e-8f40-123456789abc\" alt=\"&quot; onerror=&quot;x\"><hr>"
    );
    assert!(!to_html(&parsed, &|_| None).contains("<img"));
}

#[test]
fn version_1_is_read_as_it_is_and_is_a_subset_of_version_2() {
    let content = json!([
        {"type": "heading", "attrs": {"level": 3}, "content": [text("A")]},
        {"type": "image", "attrs": {"media": MEDIA, "alt": "x"}},
        {"type": "blockquote", "content": [para(text("q"))]}
    ]);
    let old = parse(&v1(content.clone())).unwrap();
    let new = parse(&doc(content.clone())).unwrap();
    assert_eq!((old.version, new.version), (1, 2));
    assert_eq!(old.content, new.content);
    // `upgrade` changes the version only.
    assert_eq!(upgrade(&v1(content.clone())), doc(content.clone()));
    assert_eq!(parse(&upgrade(&v1(content.clone()))).unwrap(), new);
    for same in
        [doc(content), json!("<p>x</p>"), json!({"type": "doc", "version": 3}), json!({"type": "x", "version": 1})]
    {
        assert_eq!(upgrade(&same), same);
    }
    // What only version 2 has is refused in version 1.
    for v2_only in [
        table(2, 2),
        embed("youtube", "dQw4w9WgXcQ"),
        json!({"type": "callout", "attrs": {"tone": "info"}, "content": [para(text("x"))]}),
        json!({"type": "image", "attrs": {"media": MEDIA, "caption": "c"}}),
        json!({"type": "image", "attrs": {"media": MEDIA, "width": "normal"}}),
    ] {
        let err = parse(&v1(json!([v2_only.clone()]))).unwrap_err();
        assert!(err.0.contains("needs document version 2"), "{v2_only}: {err}");
        assert!(parse(&doc(json!([v2_only.clone()]))).is_ok(), "{v2_only}");
        let issue = rustcms_core::i18n::Issue::of(&err.0);
        assert_eq!(issue.code, "document_needs_version", "{err}");
    }
}

#[test]
fn anything_else_is_refused() {
    let link = |href: &str| json!({"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": href}}]});
    for bad in [
        json!({"type": "doc", "content": []}),
        json!({"type": "doc", "version": 0, "content": []}),
        json!({"type": "doc", "version": 3, "content": []}),
        json!("<p>html</p>"),
        doc(json!([{"type": "script", "content": []}])),
        doc(json!([{"type": "paragraph", "attrs": {"style": "x"}, "content": []}])),
        doc(json!([para(link("javascript:alert(1)"))])),
        doc(json!([para(link("JavaScript:alert(1)"))])),
        doc(json!([para(link("data:text/html,x"))])),
        doc(json!([para(link("https://a b"))])),
        doc(json!([para(
            json!({"type": "text", "text": "x", "marks": [{"type": "link", "attrs": {"href": "https://x", "target": "_blank"}}]})
        )])),
        doc(json!([para(json!({"type": "text", "text": "x", "marks": [{"type": "underline"}]}))])),
        doc(json!([para(json!({"type": "text", "text": "", "marks": []}))])),
        doc(json!([para(json!({"type": "text", "text": "x", "onclick": "y"}))])),
        doc(json!([{"type": "heading", "attrs": {"level": 1}, "content": [text("x")]}])),
        doc(json!([{"type": "heading", "attrs": {"level": 2, "id": "x"}, "content": [text("x")]}])),
        doc(json!([{"type": "image", "attrs": {"src": "https://evil.test/x.png"}}])),
        doc(json!([{"type": "image", "attrs": {"media": "not-a-uuid"}}])),
        doc(json!([{"type": "bulletList", "content": [para(text("x"))]}])),
        doc(json!([{"type": "codeBlock", "content": [{"type": "text", "text": "x", "marks": [{"type": "bold"}]}]}])),
        doc(json!([para(json!({"type": "text", "text": "x", "marks": [{"type": "bold"}, {"type": "bold"}]}))])),
        doc(json!([para(json!({"type": "image", "attrs": {"media": MEDIA}}))])),
        doc(json!([{"type": "listItem", "content": [para(text("x"))]}])),
        json!({"type": "doc", "version": 2, "content": [], "marks": [{"type": "evil", "attrs": {"x": 1}}]}),
        // Images (version 2 attributes).
        doc(json!([{"type": "image", "attrs": {"media": MEDIA, "width": "huge"}}])),
        doc(json!([{"type": "image", "attrs": {"media": MEDIA, "width": 800}}])),
        doc(json!([{"type": "image", "attrs": {"media": MEDIA, "caption": {"html": "<b>"}}}])),
        doc(json!([{"type": "image", "attrs": {"media": MEDIA, "caption": "x".repeat(MAX_CAPTION + 1)}}])),
        doc(json!([{"type": "image", "attrs": {"media": MEDIA, "title": "x"}}])),
        // Embeds: allowed providers, their id formats, nothing else.
        doc(json!([embed("dailymotion", "x7tgad0")])),
        doc(json!([embed("YouTube", "dQw4w9WgXcQ")])),
        doc(json!([embed("youtube", "dQw4w9WgXc")])),
        doc(json!([embed("youtube", "dQw4w9WgXcQ\"><script>")])),
        doc(json!([embed("youtube", "https://www.youtube.com/watch?v=dQw4w9WgXcQ")])),
        doc(json!([embed("vimeo", "76979871a")])),
        doc(json!([embed("vimeo", "")])),
        doc(json!([embed("x", "1".repeat(21).as_str())])),
        doc(json!([{"type": "embed", "attrs": {"provider": "youtube"}}])),
        doc(json!([{"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ", "html": "<iframe>"}}])),
        doc(json!([{"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ", "url": "https://x"}}])),
        doc(json!([{"type": "embed", "attrs": {"provider": "youtube", "id": "dQw4w9WgXcQ"}, "content": []}])),
        doc(json!([para(embed("youtube", "dQw4w9WgXcQ"))])),
        // Tables: rows of cells of paragraphs, rectangular, header row first only.
        doc(json!([{"type": "table", "content": []}])),
        doc(json!([{"type": "table"}])),
        doc(json!([{"type": "table", "content": [{"type": "tableRow", "content": []}]}])),
        doc(json!([{"type": "table", "content": [para(text("x"))]}])),
        doc(json!([{"type": "table", "content": [{"type": "tableRow", "content": [para(text("x"))]}]}])),
        doc(
            json!([{"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableCell", "content": []}]}]}]),
        ),
        doc(json!([{"type": "table", "content": [{"type": "tableRow", "content": [{"type": "tableCell"}]}]}])),
        doc(json!([{"type": "table", "content": [{"type": "tableRow", "content": [
            {"type": "tableCell", "content": [{"type": "bulletList", "content": []}]}
        ]}]}])),
        doc(json!([{"type": "table", "content": [{"type": "tableRow", "content": [
            {"type": "tableCell", "content": [{"type": "image", "attrs": {"media": MEDIA}}]}
        ]}]}])),
        doc(json!([{"type": "table", "content": [{"type": "tableRow", "content": [
            {"type": "tableCell", "attrs": {"colspan": 2}, "content": [para(text("x"))]}
        ]}]}])),
        doc(
            json!([{"type": "table", "attrs": {"style": "x"}, "content": [{"type": "tableRow", "content": [cell("tableCell", "x")]}]}]),
        ),
        doc(json!([{"type": "table", "content": [
            {"type": "tableRow", "content": [cell("tableCell", "a"), cell("tableCell", "b")]},
            {"type": "tableRow", "content": [cell("tableCell", "c")]}
        ]}])),
        doc(json!([{"type": "table", "content": [
            {"type": "tableRow", "content": [cell("tableHeader", "a"), cell("tableCell", "b")]}
        ]}])),
        doc(json!([{"type": "table", "content": [
            {"type": "tableRow", "content": [cell("tableCell", "a")]},
            {"type": "tableRow", "content": [cell("tableHeader", "b")]}
        ]}])),
        doc(
            json!([{"type": "table", "content": [{"type": "tableRow", "content": [cell("tableCell", "a")]}, table(1, 1)]}]),
        ),
        doc(json!([{"type": "tableRow", "content": [cell("tableCell", "a")]}])),
        doc(json!([cell("tableCell", "a")])),
        // Callouts.
        doc(json!([{"type": "callout", "attrs": {"tone": "danger"}, "content": [para(text("x"))]}])),
        doc(json!([{"type": "callout", "content": [para(text("x"))]}])),
        doc(json!([{"type": "callout", "attrs": {"tone": "info"}, "content": []}])),
        doc(json!([{"type": "callout", "attrs": {"tone": "info", "icon": "x"}, "content": [para(text("x"))]}])),
        doc(
            json!([{"type": "callout", "attrs": {"tone": "info"}, "content": [{"type": "heading", "attrs": {"level": 2}, "content": [text("x")]}]}]),
        ),
    ] {
        let err = parse(&bad).expect_err(&bad.to_string());
        // Every message has a code the panel translates.
        assert_ne!(rustcms_core::i18n::Issue::of(&err.0).code, rustcms_core::i18n::CUSTOM, "{err}");
    }
}

#[test]
fn version_2_nodes_parse_and_render() {
    let value = doc(json!([
        {"type": "heading", "attrs": {"level": 2}, "content": [text("Bảng giá")]},
        {"type": "table", "content": [
            {"type": "tableRow", "content": [cell("tableHeader", "Gói"), cell("tableHeader", "Giá <b>")]},
            {"type": "tableRow", "content": [
                {"type": "tableCell", "content": [para(json!({"type": "text", "text": "Pro", "marks": [{"type": "bold"}]})), para(text("2"))]},
                cell("tableCell", "10 $")
            ]}
        ]},
        {"type": "table", "content": [{"type": "tableRow", "content": [cell("tableCell", "no header")]}]},
        {"type": "image", "attrs": {"media": MEDIA, "alt": "Cat", "caption": "A <cat>", "width": "wide"}},
        {"type": "image", "attrs": {"media": MEDIA, "alt": "", "caption": "", "width": "normal"}},
        embed("youtube", "dQw4w9WgXcQ"),
        embed("vimeo", "76979871"),
        embed("x", "1460323737035677698"),
        {"type": "callout", "attrs": {"tone": "warn"}, "content": [para(text("Careful"))]},
        {"type": "blockquote", "content": [{"type": "heading", "attrs": {"level": 2}, "content": [text("Bảng giá")]}]}
    ]));
    let parsed = parse(&value).unwrap();
    let Node::Table(t) = &parsed.content[1] else { panic!() };
    assert!(t.header && t.rows.len() == 2 && t.rows[1][0].content.len() == 2);
    let Node::Table(t) = &parsed.content[2] else { panic!() };
    assert!(!t.header);
    assert_eq!(
        parsed.content[3],
        Node::Image(Image {
            media: uuid::Uuid::parse_str(MEDIA).unwrap(),
            alt: "Cat".into(),
            caption: "A <cat>".into(),
            width: ImageWidth::Wide
        })
    );
    let html = to_html(&parsed, &|id| Some(format!("/m/{id}")));
    assert_eq!(
        html,
        "<h2>Bảng giá</h2>\
         <table><thead><tr><th><p>Gói</p></th><th><p>Giá &lt;b&gt;</p></th></tr></thead>\
         <tbody><tr><td><p><strong>Pro</strong></p><p>2</p></td><td><p>10 $</p></td></tr></tbody></table>\
         <table><tbody><tr><td><p>no header</p></td></tr></tbody></table>\
         <img src=\"/m/0199d3a4-1b2c-7d3e-8f40-123456789abc\" alt=\"Cat\">\
         <img src=\"/m/0199d3a4-1b2c-7d3e-8f40-123456789abc\" alt=\"\">\
         <figure class=\"embed\" data-provider=\"youtube\" data-id=\"dQw4w9WgXcQ\"><a href=\"https://www.youtube.com/watch?v=dQw4w9WgXcQ\" rel=\"noopener noreferrer nofollow\">https://www.youtube.com/watch?v=dQw4w9WgXcQ</a></figure>\
         <figure class=\"embed\" data-provider=\"vimeo\" data-id=\"76979871\"><a href=\"https://vimeo.com/76979871\" rel=\"noopener noreferrer nofollow\">https://vimeo.com/76979871</a></figure>\
         <figure class=\"embed\" data-provider=\"x\" data-id=\"1460323737035677698\"><a href=\"https://x.com/i/status/1460323737035677698\" rel=\"noopener noreferrer nofollow\">https://x.com/i/status/1460323737035677698</a></figure>\
         <aside class=\"callout\" data-tone=\"warn\"><p>Careful</p></aside>\
         <blockquote><h2>Bảng giá</h2></blockquote>"
    );
    assert_eq!(media_ids(&parsed).len(), 1);
}

#[test]
fn documents_are_bounded() {
    let many: Vec<Value> = (0..10_001).map(|_| json!({"type": "horizontalRule"})).collect();
    assert!(parse(&doc(Value::Array(many))).is_err());
    let mut nested = json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]});
    for _ in 0..20 {
        nested = json!({"type": "blockquote", "content": [nested]});
    }
    assert!(parse(&doc(json!([nested]))).is_err());
    let marks: Vec<Value> = (0..15_000).map(|_| json!({"type": "bold"})).collect();
    assert!(
        parse(&doc(json!([{"type": "paragraph", "content": [{"type": "text", "text": "x", "marks": marks}]}])))
            .is_err()
    );
    let big = "x".repeat(MAX_BYTES);
    assert!(parse(&doc(json!([{"type": "paragraph", "content": [{"type": "text", "text": big}]}]))).is_err());

    // Tables: at most 50 rows of 20 cells.
    assert!(parse(&doc(json!([table(MAX_TABLE_ROWS, MAX_TABLE_COLUMNS)]))).is_ok());
    for (rows, columns, code) in
        [(MAX_TABLE_ROWS + 1, 1, "document_table_rows"), (1, MAX_TABLE_COLUMNS + 1, "document_table_columns")]
    {
        let err = parse(&doc(json!([table(rows, columns)]))).unwrap_err();
        assert_eq!(rustcms_core::i18n::Issue::of(&err.0).code, code);
    }
    // Table rows and cells count as elements: the largest table is 3,051 of them
    // (table, 50 rows, 1,000 cells, their paragraphs and texts).
    let filled = |breaks: usize| {
        let inline: Vec<Value> = (0..breaks).map(|_| json!({"type": "hardBreak"})).collect();
        doc(json!([table(MAX_TABLE_ROWS, MAX_TABLE_COLUMNS), {"type": "paragraph", "content": inline}]))
    };
    let room = MAX_NODES - 3_051 - 1;
    assert!(parse(&filled(room)).is_ok());
    assert_eq!(parse(&filled(room + 1)).unwrap_err().0, format!("may not have more than {MAX_NODES} elements"));
    // Tables nest four levels below their block: the depth limit still holds.
    let mut nested = table(1, 1);
    for _ in 0..13 {
        nested = json!({"type": "blockquote", "content": [nested]});
    }
    assert_eq!(parse(&doc(json!([nested]))).unwrap_err().0, "is nested too deeply");

    // Embeds and images are counted.
    let embeds: Vec<Value> = (0..=MAX_EMBEDS).map(|_| embed("vimeo", "1")).collect();
    assert_eq!(parse(&doc(Value::Array(embeds))).unwrap_err().0, format!("may not have more than {MAX_EMBEDS} embeds"));
    let images: Vec<Value> = (0..=MAX_IMAGES).map(|_| json!({"type": "image", "attrs": {"media": MEDIA}})).collect();
    assert!(parse(&doc(Value::Array(images))).is_err());
}
