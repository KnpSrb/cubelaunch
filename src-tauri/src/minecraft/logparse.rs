//! Разбор вывода игры. С конфигом log4j из манифеста игра пишет события в XML:
//!
//! ```xml
//! <log4j:Event logger="..." timestamp="1700000000000" level="INFO" thread="Render thread">
//!   <log4j:Message><![CDATA[Setting user: Steve]]></log4j:Message>
//! </log4j:Event>
//! ```
//!
//! Обычные строки (старые версии, вывод JVM) разбираются эвристически.

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "TRACE" => Self::Trace,
            "DEBUG" => Self::Debug,
            "INFO" => Self::Info,
            "WARN" | "WARNING" => Self::Warn,
            "ERROR" | "SEVERE" => Self::Error,
            "FATAL" => Self::Fatal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub level: LogLevel,
    /// Время `HH:MM:SS` (если известно).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    pub text: String,
}

#[derive(Default)]
pub struct LogParser {
    buf: Option<String>,
}

fn attr<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let start = xml.find(&pat)? + pat.len();
    let end = xml[start..].find('"')?;
    Some(&xml[start..start + end])
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)?;
    Some(&s[start..start + end])
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn element_text(xml: &str, tag: &str) -> Option<String> {
    let inner = between(xml, &format!("<{tag}>"), &format!("</{tag}>"))?;
    Some(match between(inner, "<![CDATA[", "]]>") {
        Some(c) => c.to_string(),
        None => unescape(inner),
    })
}

fn format_ts(ms: &str) -> Option<String> {
    let ms: i64 = ms.parse().ok()?;
    let dt = chrono::DateTime::from_timestamp_millis(ms)?.with_timezone(&chrono::Local);
    Some(dt.format("%H:%M:%S").to_string())
}

fn parse_event(xml: &str) -> LogLine {
    let level = attr(xml, "level").and_then(LogLevel::parse).unwrap_or(LogLevel::Info);
    let mut text = element_text(xml, "log4j:Message").unwrap_or_default();
    if let Some(t) = element_text(xml, "log4j:Throwable") {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(t.trim_end());
    }
    LogLine {
        level,
        time: attr(xml, "timestamp").and_then(format_ts),
        thread: attr(xml, "thread").map(unescape),
        text,
    }
}

fn plain_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // [12:34:56] [Render thread/INFO]: text   или   [12:34:56] [main/WARN] [minecraft/Foo]: text
    RE.get_or_init(|| Regex::new(r"^\[(\d{2}:\d{2}:\d{2})\] \[([^\]/]+)/([A-Z]+)\]").unwrap())
}

fn parse_plain(line: &str, stderr: bool) -> LogLine {
    if let Some(c) = plain_re().captures(line) {
        if let Some(level) = LogLevel::parse(&c[3]) {
            return LogLine { level, time: Some(c[1].to_string()), thread: Some(c[2].to_string()), text: line.to_string() };
        }
    }
    let t = line.trim_start();
    let is_error = t.starts_with("at ")
        || t.starts_with("Caused by:")
        || (t.contains("Exception") && !t.contains("INFO"))
        || line.contains("[SEVERE]")
        || line.contains("ERROR")
        || line.contains("FATAL");
    let level = if is_error {
        LogLevel::Error
    } else if line.contains("[WARNING]") || line.contains("WARN") {
        LogLevel::Warn
    } else if stderr {
        LogLevel::Error
    } else {
        LogLevel::Info
    };
    LogLine { level, time: None, thread: None, text: line.to_string() }
}

impl LogParser {
    /// Подаёт очередную строку вывода. Возвращает строку лога, когда событие собрано.
    pub fn feed(&mut self, line: &str, stderr: bool) -> Option<LogLine> {
        if let Some(buf) = &mut self.buf {
            buf.push_str(line);
            buf.push('\n');
            if line.contains("</log4j:Event>") {
                let xml = self.buf.take().unwrap();
                return Some(parse_event(&xml));
            }
            // Защита от бесконечного накопления при битом XML.
            if buf.len() > 1 << 20 {
                let text = self.buf.take().unwrap();
                return Some(LogLine { level: LogLevel::Info, time: None, thread: None, text });
            }
            return None;
        }
        if line.trim_start().starts_with("<log4j:Event") {
            if line.contains("</log4j:Event>") {
                return Some(parse_event(line));
            }
            self.buf = Some(format!("{line}\n"));
            return None;
        }
        Some(parse_plain(line, stderr))
    }

    /// Выдаёт недособранное событие (при завершении процесса).
    pub fn flush(&mut self) -> Option<LogLine> {
        self.buf.take().map(|text| LogLine { level: LogLevel::Info, time: None, thread: None, text })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xml_event() {
        let mut p = LogParser::default();
        assert!(p.feed(r#"<log4j:Event logger="ewn" timestamp="1700000000000" level="WARN" thread="Render thread">"#, false).is_none());
        assert!(p.feed("  <log4j:Message><![CDATA[Missing sound <x> & y]]></log4j:Message>", false).is_none());
        let l = p.feed("</log4j:Event>", false).unwrap();
        assert_eq!(l.level, LogLevel::Warn);
        assert_eq!(l.thread.as_deref(), Some("Render thread"));
        assert_eq!(l.text, "Missing sound <x> & y");
        assert_eq!(l.time.as_ref().unwrap().len(), 8);
    }

    #[test]
    fn parses_throwable_and_escaped_message() {
        let mut p = LogParser::default();
        let lines = [
            r#"<log4j:Event logger="a" timestamp="1" level="ERROR" thread="main">"#,
            "<log4j:Message>a &lt;b&gt;</log4j:Message>",
            "<log4j:Throwable><![CDATA[java.lang.RuntimeException: boom",
            "\tat Foo.bar(Foo.java:1)",
            "]]></log4j:Throwable>",
            "</log4j:Event>",
        ];
        let mut out = None;
        for l in lines {
            out = p.feed(l, false).or(out);
        }
        let l = out.unwrap();
        assert_eq!(l.level, LogLevel::Error);
        assert_eq!(l.text, "a <b>\njava.lang.RuntimeException: boom\n\tat Foo.bar(Foo.java:1)");
    }

    #[test]
    fn plain_lines() {
        let mut p = LogParser::default();
        let l = p.feed("[12:00:01] [Render thread/INFO]: Setting user: Steve", false).unwrap();
        assert_eq!((l.level, l.time.as_deref(), l.thread.as_deref()), (LogLevel::Info, Some("12:00:01"), Some("Render thread")));
        assert_eq!(p.feed("\tat net.minecraft.Main.main(Main.java:1)", false).unwrap().level, LogLevel::Error);
        assert_eq!(p.feed("2024-01-01 [WARNING] something", false).unwrap().level, LogLevel::Warn);
        assert_eq!(p.feed("OpenJDK warning thing", true).unwrap().level, LogLevel::Error);
        assert_eq!(p.feed("hello", false).unwrap().level, LogLevel::Info);
    }

    #[test]
    fn single_line_event_and_flush() {
        let mut p = LogParser::default();
        let l = p
            .feed(r#"<log4j:Event level="DEBUG" thread="t"><log4j:Message>x</log4j:Message></log4j:Event>"#, false)
            .unwrap();
        assert_eq!((l.level, l.text.as_str()), (LogLevel::Debug, "x"));
        p.feed(r#"<log4j:Event level="INFO">"#, false);
        assert!(p.flush().is_some());
    }
}
