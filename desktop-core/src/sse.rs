//! A server-sent-events parser over raw response bytes.
//!
//! Hand-rolled rather than an SSE crate: the backend emits five event names
//! (`metadata`, `snapshot`, `event`, `end`, `error`) plus comment keep-alives, and a
//! crate would bring its own connection stack and retry semantics that fight the
//! reconnect/backoff logic ported from the frontend store. The parser is push-based —
//! feed it whatever chunks the transport hands over; events fall out whenever a chunk
//! completes them — so chunk boundaries falling mid-line or mid-UTF-8 are handled by
//! construction (bytes buffer until a full `\n`-terminated line exists).

/// One dispatched SSE event. `event` is the event name (`"message"` when the stream
/// never named one, per spec); `data` is the joined data payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one transport chunk; returns every event the chunk completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();

        // Process only complete lines; a trailing fragment stays buffered for the next
        // chunk. This is also what keeps multi-byte UTF-8 sequences intact: a code
        // point can never span a `\n`.
        while let Some(newline) = self.buf.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            if let Some(event) = self.take_line(&line) {
                events.push(event);
            }
        }
        events
    }

    fn take_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            // Blank line dispatches the pending event. Per spec, no data means no
            // event — which is exactly what a keep-alive comment block produces.
            let data_lines = std::mem::take(&mut self.data_lines);
            let event_name = self.event_name.take();
            if data_lines.is_empty() {
                return None;
            }
            return Some(SseEvent {
                event: event_name.unwrap_or_else(|| "message".to_string()),
                data: data_lines.join("\n"),
            });
        }
        if let Some(rest) = line.strip_prefix(':') {
            let _ = rest; // comment (the server's keep-alive) — ignored
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => self.event_name = Some(value.to_string()),
            "data" => self.data_lines.push(value.to_string()),
            // "id" and "retry" are unused by the backend's streams.
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(event: &str, data: &str) -> Vec<SseEvent> {
        vec![SseEvent {
            event: event.to_string(),
            data: data.to_string(),
        }]
    }

    #[test]
    fn parses_a_named_event() {
        let mut parser = SseParser::new();
        assert_eq!(
            parser.push(b"event: snapshot\ndata: {\"t\":1.0}\n\n"),
            one("snapshot", "{\"t\":1.0}")
        );
    }

    #[test]
    fn defaults_the_event_name_to_message() {
        let mut parser = SseParser::new();
        assert_eq!(parser.push(b"data: hello\n\n"), one("message", "hello"));
    }

    #[test]
    fn joins_multiple_data_lines_with_newlines() {
        let mut parser = SseParser::new();
        assert_eq!(
            parser.push(b"event: e\ndata: a\ndata: b\n\n"),
            one("e", "a\nb")
        );
    }

    #[test]
    fn survives_chunk_boundaries_mid_line_and_mid_event() {
        let mut parser = SseParser::new();
        assert!(parser.push(b"event: snap").is_empty());
        assert!(parser.push(b"shot\nda").is_empty());
        assert!(parser.push(b"ta: 42\n").is_empty());
        assert_eq!(parser.push(b"\n"), one("snapshot", "42"));
    }

    #[test]
    fn survives_chunk_boundaries_mid_utf8() {
        let mut parser = SseParser::new();
        let payload = "data: pérez\n\n".as_bytes();
        // Split inside the two-byte 'é'.
        let split = payload.iter().position(|b| *b == 0xc3).unwrap() + 1;
        assert!(parser.push(&payload[..split]).is_empty());
        assert_eq!(parser.push(&payload[split..]), one("message", "pérez"));
    }

    #[test]
    fn handles_crlf_line_endings() {
        let mut parser = SseParser::new();
        assert_eq!(
            parser.push(b"event: end\r\ndata: done\r\n\r\n"),
            one("end", "done")
        );
    }

    #[test]
    fn keep_alive_comments_produce_no_events() {
        let mut parser = SseParser::new();
        assert!(parser.push(b": keep-alive\n\n").is_empty());
    }

    #[test]
    fn event_name_without_data_is_dropped() {
        let mut parser = SseParser::new();
        assert!(parser.push(b"event: end\n\n").is_empty());
    }

    #[test]
    fn dispatches_consecutive_events_from_one_chunk() {
        let mut parser = SseParser::new();
        let events = parser.push(b"event: a\ndata: 1\n\nevent: b\ndata: 2\n\n");
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: "a".into(),
                    data: "1".into()
                },
                SseEvent {
                    event: "b".into(),
                    data: "2".into()
                },
            ]
        );
    }

    #[test]
    fn resets_the_event_name_after_dispatch() {
        let mut parser = SseParser::new();
        parser.push(b"event: snapshot\ndata: 1\n\n");
        assert_eq!(parser.push(b"data: 2\n\n"), one("message", "2"));
    }

    #[test]
    fn data_value_keeps_only_one_leading_space_stripped() {
        let mut parser = SseParser::new();
        assert_eq!(parser.push(b"data:  padded\n\n"), one("message", " padded"));
    }

    #[test]
    fn field_with_no_colon_is_a_field_with_empty_value() {
        let mut parser = SseParser::new();
        // A bare "data" line contributes an empty data line, which still dispatches.
        assert_eq!(parser.push(b"data\n\n"), one("message", ""));
    }
}
