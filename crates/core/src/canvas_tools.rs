//! The tools a canvas agent edits its canvas with. The canvas page runs them
//! against its own document, so these are declarations only.

use serde_json::{json, Value};

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

fn ids(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": description })
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "read_canvas",
            "List the canvas: every card's id, kind, title (first line), position and size, \
             the frames, the arrows, the selected cards and the visible area. Coordinates \
             are canvas pixels; x grows right, y grows down.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool(
            "read_cards",
            "Return the full text of cards: Markdown for notes, the HTML source for HTML cards.",
            json!({
                "type": "object",
                "properties": { "ids": ids("Card ids, at most 20.") },
                "required": ["ids"]
            }),
        ),
        tool(
            "create_cards",
            "Add cards. A note holds Markdown. An HTML card holds a complete HTML page that \
             runs in a sandbox: inline all CSS and JavaScript, no network, no storage, no \
             popups; it should fill its width and height. Without x and y a card is placed \
             in free space near the visible area, or beside the card named in `near`.",
            json!({
                "type": "object",
                "properties": {
                    "cards": {
                        "type": "array",
                        "description": "At most 30 cards.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["note", "html"] },
                                "text": { "type": "string", "description": "Markdown, or the HTML page." },
                                "x": { "type": "number" },
                                "y": { "type": "number" },
                                "w": { "type": "number", "description": "Width; notes default to 280, HTML to 480." },
                                "h": { "type": "number", "description": "HTML cards only; defaults to 320." },
                                "near": { "type": "string", "description": "Place beside this card id." }
                            },
                            "required": ["text"]
                        }
                    }
                },
                "required": ["cards"]
            }),
        ),
        tool(
            "update_cards",
            "Change cards: replace their text, move them, or resize them. Omitted fields stay. \
             A note stays Markdown and an HTML card stays HTML; to show notes as a chart or \
             other page, create an HTML card beside them instead.",
            json!({
                "type": "object",
                "properties": {
                    "cards": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "text": { "type": "string" },
                                "x": { "type": "number" },
                                "y": { "type": "number" },
                                "w": { "type": "number" },
                                "h": { "type": "number" }
                            },
                            "required": ["id"]
                        }
                    }
                },
                "required": ["cards"]
            }),
        ),
        tool(
            "delete_cards",
            "Remove cards and the arrows touching them. The user can undo it.",
            json!({
                "type": "object",
                "properties": { "ids": ids("Card ids.") },
                "required": ["ids"]
            }),
        ),
        tool(
            "connect_cards",
            "Draw arrows between cards, from the first card to the second.",
            json!({
                "type": "object",
                "properties": {
                    "links": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "from": { "type": "string" },
                                "to": { "type": "string" }
                            },
                            "required": ["from", "to"]
                        }
                    }
                },
                "required": ["links"]
            }),
        ),
        tool(
            "frame_cards",
            "Put a titled frame around cards, to group a section of the canvas.",
            json!({
                "type": "object",
                "properties": {
                    "ids": ids("The cards to frame."),
                    "title": { "type": "string" }
                },
                "required": ["ids", "title"]
            }),
        ),
    ]
}
