//! Desktop presentations of the same session-local transcript.
use eframe::egui;
use serde_json::{Value, json};
use xtion_vision::ai::{Exchange, Transcript};

#[derive(Default)]
pub struct ChatView {
    json_mode: bool,
    entries: Vec<Exchange>,
    reader: Vec<(String, String)>,
    json_lines: Option<Vec<String>>,
}

fn body(text: Option<&str>) -> Value {
    text.filter(|s| !s.is_empty()).map_or(Value::Null, |s| {
        serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_owned()))
    })
}
fn json_document(entries: &[Exchange]) -> String {
    let payloads: Vec<_> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.id, "target":e.target.as_ref(), "status":e.status.as_ref(),
                "request":body(Some(&e.request)), "response":body(e.response.as_deref())
            })
        })
        .collect();
    serde_json::to_string_pretty(&payloads).expect("JSON values serialize")
}
fn reader_text(e: &Exchange) -> (String, String) {
    let candidates = e
        .prompt
        .rsplit_once("\nCandidates:\n")
        .map(|(_, list)| list);
    let request = if let Some(list) = candidates {
        format!(
            "Choose the observation that best explains the recent depth history ({} candidates).",
            list.lines().count()
        )
    } else if e.prompt.contains("Allowed values:") {
        "Interpret the shape and its movement using the recorded track.".into()
    } else if e.prompt.contains("Current keyframe") {
        "Interpret the depth image in the context of recent tracking events.".into()
    } else if e.target.ends_with("/models") {
        "List available models.".into()
    } else if e.target.ends_with("/openapi.json") {
        "Check which API routes this endpoint supports.".into()
    } else {
        e.prompt
            .lines()
            .next()
            .unwrap_or("Send request to endpoint.")
            .to_owned()
    };
    let response = if let Some(text) = e.answer.as_deref() {
        let value = body(Some(text));
        if let (Some(index), Some(list)) = (value["index"].as_u64(), candidates)
            && let Some((_, selected)) = list
                .lines()
                .filter_map(|s| s.split_once(": "))
                .find(|(n, _)| n.parse::<u64>() == Ok(index))
        {
            selected.to_owned()
        } else if let (Some(subject), Some(action)) =
            (value["subject"].as_str(), value["action"].as_str())
        {
            format!("{subject} · {action}")
        } else if let Some(claims) = value["claims"].as_array() {
            if claims.is_empty() {
                "No shape or activity hypothesis returned.".into()
            } else {
                claims
                    .iter()
                    .map(|c| {
                        match (
                            c["hypothesis"].as_str(),
                            c["activity"].as_str(),
                            c["z_m"].as_f64(),
                        ) {
                            (Some(h), Some(a), Some(z)) => format!("{h} · {a} · {z:.2} m"),
                            _ => c.to_string(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        } else {
            text.to_owned()
        }
    } else if let Some(raw) = e.response.as_deref() {
        let value = body(Some(raw));
        if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
            message.to_owned()
        } else if let Some(models) = value["data"].as_array() {
            format!("{} entries returned.", models.len())
        } else if value["paths"].is_object() {
            "Endpoint returned its API specification.".into()
        } else {
            raw.chars().take(200).collect()
        }
    } else {
        e.status.to_string()
    };
    (request, response)
}

impl ChatView {
    pub fn toggle(&mut self, ui: &mut egui::Ui) {
        ui.toggle_value(&mut self.json_mode, "JSON")
            .on_hover_text("Toggle between the reader and full JSON transcript");
    }
    pub fn show(&mut self, ui: &mut egui::Ui, chat: &Transcript) {
        let entries = chat.snapshot();
        if entries.is_empty() {
            ui.label("Requests and replies appear here when AI analysis runs.");
            return;
        }
        if entries.len() != self.entries.len()
            || entries
                .iter()
                .zip(&self.entries)
                .any(|(a, b)| a.id != b.id || a.status != b.status)
        {
            self.reader = entries.iter().map(reader_text).collect();
            self.entries = entries;
            self.json_lines = None;
        }
        if self.json_mode {
            let lines = self.json_lines.get_or_insert_with(|| {
                json_document(&self.entries)
                    .lines()
                    .map(str::to_owned)
                    .collect()
            });
            if ui.small_button("Copy full JSON").clicked() {
                ui.ctx().copy_text(lines.join("\n"));
            }
            let height = ui.text_style_height(&egui::TextStyle::Monospace);
            egui::ScrollArea::both()
                .id_salt("raw_chat")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show_rows(ui, height, lines.len(), |ui, range| {
                    for line in &lines[range] {
                        ui.add(
                            egui::Label::new(egui::RichText::new(line).monospace())
                                .extend()
                                .selectable(true),
                        );
                    }
                });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("reader_chat")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for (e, (request, response)) in self.entries.iter().zip(&self.reader) {
                        ui.push_id(e.id, |ui| {
                            ui.small(format!("#{}   ·   {}   ·   {}", e.id, e.target, e.status));
                            ui.label(egui::RichText::new(request).strong());
                            ui.add(egui::Label::new(response).wrap().selectable(true));
                            if !e.prompt.is_empty() {
                                ui.collapsing("Prompt and context", |ui| {
                                    ui.add(
                                        egui::Label::new(e.prompt.as_ref()).wrap().selectable(true),
                                    );
                                });
                            }
                            ui.separator();
                        });
                    }
                });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader_resolves_choice_while_json_retains_full_exchange() {
        let e = Exchange {
            id: 2,
            target: "model · POST /chat/completions".into(),
            status: "HTTP 200".into(),
            prompt: "30-second history\nCandidates:\n0: Staying still.\n1: Moving closer.".into(),
            request: r#"{"messages":[{"content":"complete prompt"}]}"#.into(),
            response: Some(r#"{"choices":[{"message":{"content":"{\"index\":1}"}}]}"#.into()),
            answer: Some(r#"{"index":1}"#.into()),
        };
        assert_eq!(reader_text(&e).1, "Moving closer.");
        let doc: Value = serde_json::from_str(&json_document(&[e])).unwrap();
        assert_eq!(
            doc[0]["request"]["messages"][0]["content"],
            "complete prompt"
        );
        assert_eq!(
            doc[0]["response"]["choices"][0]["message"]["content"],
            r#"{"index":1}"#
        );
        assert_eq!(body(Some("service unavailable")), "service unavailable");
        assert!(body(None).is_null());
    }
}
