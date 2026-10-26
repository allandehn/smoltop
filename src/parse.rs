//! Parsers for `machine ls --json`, `machine stats`, and `--error-format json`.
//!
//! `machine ls --json` is a pretty-printed array of the objects built by
//! `machine_status_json` in smolvm v1.32.3. Fields used here: `name`, `state`,
//! `cpus`, `memory_mib`, `image`, `parent_machine`. Anything else is ignored.
//!
//! `machine stats --json` is `machine_stats_json`: `cpu_percent` (number or
//! null) and `memory` (object or null) with `used_bytes` and `total_bytes`.
//! The text form (`machine_stats_text`) is accepted when JSON is missing, so a
//! stats sample still shows if the shape drifts.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    pub name: String,
    pub state: String,
    pub cpus: u64,
    pub memory_mib: u64,
    pub image: Option<String>,
    pub parent: Option<String>,
}

impl Machine {
    pub fn origin(&self) -> String {
        match (&self.image, &self.parent) {
            (Some(image), Some(parent)) => format!("{image}  from {parent}"),
            (Some(image), None) => image.clone(),
            (None, Some(parent)) => format!("from {parent}"),
            (None, None) => "-".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveStats {
    pub cpu_percent: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub cpu_text: Option<String>,
    pub memory_text: Option<String>,
}

impl LiveStats {
    pub fn cpu_label(&self) -> String {
        if let Some(percent) = self.cpu_percent {
            if percent.is_finite() && percent >= 0.0 {
                return format!("{percent:.1}%");
            }
        }
        if let Some(text) = &self.cpu_text {
            return text.clone();
        }
        "unavailable".into()
    }

    pub fn memory_label(&self) -> String {
        if let (Some(used), Some(total)) = (self.memory_used_bytes, self.memory_total_bytes) {
            return format!("{} / {}", format_bytes(used), format_bytes(total));
        }
        if let Some(text) = &self.memory_text {
            return text.clone();
        }
        "unavailable".into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmolvmError {
    pub code: Option<String>,
    pub message: String,
}

pub fn parse_machine_list(stdout: &str) -> Result<Vec<Machine>, String> {
    let value = first_json_value(stdout)?;
    let items = value
        .as_array()
        .ok_or_else(|| "machine ls JSON was not an array".to_string())?;
    let mut machines = Vec::new();
    for item in items {
        if let Some(machine) = machine_from_value(item) {
            machines.push(machine);
        }
    }
    Ok(machines)
}

pub fn parse_stats(stdout: &str) -> Result<LiveStats, String> {
    if let Ok(value) = first_json_value(stdout) {
        if let Some(object) = value.as_object() {
            if !object.contains_key("error") {
                return Ok(stats_from_json(&value));
            }
        }
    }
    stats_from_text(stdout).ok_or_else(|| "unrecognized stats output".to_string())
}

pub fn explain_failure(output: &CommandOutput) -> String {
    if let Some(error) = parse_smolvm_error(&output.stderr) {
        return clip_line(match error.code {
            Some(code) => format!("{code}: {}", error.message),
            None => error.message,
        });
    }
    let stderr = output.stderr.trim();
    let stdout = output.stdout.trim();
    if !stderr.is_empty() {
        clip_line(one_line(stderr))
    } else if !stdout.is_empty() {
        clip_line(one_line(stdout))
    } else {
        format!("smolvm exited {}", output.status)
    }
}

pub fn interpret_list(result: Result<CommandOutput, String>) -> Result<Vec<Machine>, String> {
    match result {
        Err(error) => Err(error),
        Ok(output) if output.status == 0 => parse_machine_list(&output.stdout),
        Ok(output) => Err(explain_failure(&output)),
    }
}

pub fn interpret_stats(result: Result<CommandOutput, String>) -> Result<LiveStats, String> {
    match result {
        Err(error) => Err(error),
        Ok(output) if output.status == 0 => parse_stats(&output.stdout),
        Ok(output) => Err(explain_failure(&output)),
    }
}

pub fn interpret_action(result: Result<CommandOutput, String>) -> Result<(), String> {
    match result {
        Err(error) => Err(error),
        Ok(output) if output.status == 0 => Ok(()),
        Ok(output) => Err(explain_failure(&output)),
    }
}

fn machine_from_value(value: &Value) -> Option<Machine> {
    let object = value.as_object()?;
    let name = object.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let state = object
        .get("state")
        .and_then(|value| value.as_str())
        .filter(|state| !state.is_empty())
        .unwrap_or("unknown")
        .to_string();
    Some(Machine {
        name: name.to_string(),
        state,
        cpus: object.get("cpus").and_then(json_u64).unwrap_or(0),
        memory_mib: object.get("memory_mib").and_then(json_u64).unwrap_or(0),
        image: optional_text(object.get("image")),
        parent: optional_text(object.get("parent_machine")),
    })
}

fn optional_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn stats_from_json(value: &Value) -> LiveStats {
    let cpu_percent = value.get("cpu_percent").and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_f64()
        }
    });
    let memory = value.get("memory").filter(|value| !value.is_null());
    let (memory_used_bytes, memory_total_bytes) = match memory {
        Some(memory) => (
            memory.get("used_bytes").and_then(json_u64),
            memory.get("total_bytes").and_then(json_u64),
        ),
        None => (None, None),
    };
    LiveStats {
        cpu_percent,
        memory_used_bytes,
        memory_total_bytes,
        cpu_text: None,
        memory_text: None,
    }
}

fn stats_from_text(text: &str) -> Option<LiveStats> {
    let mut cpu_text = None;
    let mut memory_text = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("cpu:") {
            cpu_text = Some(cpu_phrase(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("memory:") {
            memory_text = Some(memory_phrase(rest.trim()));
        }
    }
    if cpu_text.is_none() && memory_text.is_none() {
        return None;
    }
    Some(LiveStats {
        cpu_percent: None,
        memory_used_bytes: None,
        memory_total_bytes: None,
        cpu_text,
        memory_text,
    })
}

fn cpu_phrase(rest: &str) -> String {
    if rest == "unavailable" {
        return "unavailable".into();
    }
    match rest.split('%').next() {
        Some(percent) if !percent.trim().is_empty() => format!("{}%", percent.trim()),
        _ => "unavailable".into(),
    }
}

fn memory_phrase(rest: &str) -> String {
    if rest == "unavailable" {
        return "unavailable".into();
    }
    if let Some((used, tail)) = rest.split_once(" of ") {
        let total = tail
            .split(" used")
            .next()
            .unwrap_or(tail)
            .split(',')
            .next()
            .unwrap_or(tail)
            .trim();
        return format!("{} / {total}", used.trim());
    }
    "unavailable".into()
}

fn json_u64(value: &Value) -> Option<u64> {
    value.as_u64()
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    if bytes >= GIB {
        let whole = bytes / GIB;
        let frac = (bytes % GIB) * 100 / GIB;
        format!("{whole}.{frac:02} GiB")
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

fn first_json_value(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty output".into());
    }
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }
    let bytes = trimmed.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'[' || *byte == b'{' {
            let slice = &trimmed[index..];
            let mut deserializer = serde_json::Deserializer::from_str(slice).into_iter::<Value>();
            if let Some(Ok(value)) = deserializer.next() {
                return Ok(value);
            }
        }
    }
    Err("no JSON value in output".into())
}

fn parse_smolvm_error(stderr: &str) -> Option<SmolvmError> {
    let value = first_json_value(stderr).ok()?;
    let error = value.get("error")?;
    let message = error.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }
    Some(SmolvmError {
        code: error
            .get("code")
            .and_then(|code| code.as_str())
            .filter(|code| !code.is_empty())
            .map(str::to_string),
        message: message.to_string(),
    })
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clip_line(text: String) -> String {
    const MAX: usize = 200;
    if text.chars().count() <= MAX {
        text
    } else {
        let clipped: String = text.chars().take(MAX).collect();
        format!("{clipped}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine_json() -> &'static str {
        r#"[
          {
            "name": "web",
            "state": "running",
            "cpus": 4,
            "memory_mib": 8192,
            "image": "alpine",
            "parent_machine": null,
            "gpu": false,
            "labels": {}
          },
          {
            "name": "child",
            "state": "stopped",
            "cpus": 2,
            "memory_mib": 1024,
            "image": null,
            "parent_machine": "web"
          },
          {
            "state": "running"
          }
        ]"#
    }

    #[test]
    fn parses_machine_list_and_skips_nameless_rows() {
        let machines = parse_machine_list(machine_json()).unwrap();
        assert_eq!(machines.len(), 2);
        assert_eq!(machines[0].name, "web");
        assert_eq!(machines[0].state, "running");
        assert_eq!(machines[0].cpus, 4);
        assert_eq!(machines[0].memory_mib, 8192);
        assert_eq!(machines[0].image.as_deref(), Some("alpine"));
        assert_eq!(machines[0].parent, None);
        assert_eq!(machines[0].origin(), "alpine");
        assert_eq!(machines[1].parent.as_deref(), Some("web"));
        assert_eq!(machines[1].origin(), "from web");
        assert!(parse_machine_list("[]").unwrap().is_empty());
        assert!(parse_machine_list("not json").is_err());
        assert!(parse_machine_list("{\"name\":\"web\"}").is_err());
    }

    #[test]
    fn parses_list_with_a_leading_note() {
        let text = format!("note: swept orphans\n{}", machine_json());
        let machines = parse_machine_list(&text).unwrap();
        assert_eq!(machines[0].name, "web");
    }

    #[test]
    fn missing_fields_degrade_inside_a_row() {
        let machines = parse_machine_list(r#"[{"name":"solo"}]"#).unwrap();
        assert_eq!(machines[0].state, "unknown");
        assert_eq!(machines[0].cpus, 0);
        assert_eq!(machines[0].origin(), "-");
    }

    #[test]
    fn parses_stats_json_including_nulls() {
        let stats = parse_stats(
            r#"{
              "name": "web",
              "cpus": 4,
              "cpu_percent": 0.4,
              "memory": {
                "total_bytes": 8332230656,
                "used_bytes": 104857600,
                "available_bytes": 1,
                "free_bytes": 1,
                "cached_bytes": 0,
                "swap_total_bytes": 0,
                "swap_used_bytes": 0
              }
            }"#,
        )
        .unwrap();
        assert_eq!(stats.cpu_label(), "0.4%");
        assert_eq!(stats.memory_label(), "100 MiB / 7.75 GiB");

        let missing =
            parse_stats(r#"{"name":"web","cpus":4,"cpu_percent":null,"memory":null}"#).unwrap();
        assert_eq!(missing.cpu_label(), "unavailable");
        assert_eq!(missing.memory_label(), "unavailable");
    }

    #[test]
    fn stats_text_fallback_and_unrecognized_output() {
        let text = "\
Machine 'dev': running
  cpu: 0.4% of 400% (4 CPUs)
  memory: 0.10 GiB of 7.77 GiB used (1%), 7.67 GiB available
";
        let stats = parse_stats(text).unwrap();
        assert_eq!(stats.cpu_label(), "0.4%");
        assert_eq!(stats.memory_label(), "0.10 GiB / 7.77 GiB");

        let unavailable = parse_stats("  cpu: unavailable\n  memory: unavailable\n").unwrap();
        assert_eq!(unavailable.cpu_label(), "unavailable");
        assert_eq!(unavailable.memory_label(), "unavailable");
        assert!(parse_stats("nope").is_err());
    }

    #[test]
    fn explains_json_errors_and_plain_stderr() {
        let output = CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr:
                "{\"error\":{\"code\":\"NOT_FOUND\",\"message\":\"machine 'web' not found\"}}\n"
                    .into(),
        };
        assert_eq!(
            explain_failure(&output),
            "NOT_FOUND: machine 'web' not found"
        );

        let noisy = CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr:
                "warning: swept\n{\"error\":{\"code\":\"BUSY\",\"message\":\"still stopping\"}}\n"
                    .into(),
        };
        assert_eq!(explain_failure(&noisy), "BUSY: still stopping");

        let plain = CommandOutput {
            status: 2,
            stdout: String::new(),
            stderr: "Error: boom\n".into(),
        };
        assert_eq!(explain_failure(&plain), "Error: boom");
    }

    #[test]
    fn interpret_helpers_use_status_and_parse_errors() {
        let ok = CommandOutput {
            status: 0,
            stdout: "[]".into(),
            stderr: String::new(),
        };
        assert!(interpret_list(Ok(ok)).unwrap().is_empty());

        let failed = CommandOutput {
            status: 1,
            stdout: String::new(),
            stderr: "{\"error\":{\"code\":\"NOT_FOUND\",\"message\":\"missing\"}}".into(),
        };
        assert_eq!(
            interpret_action(Ok(failed)).unwrap_err(),
            "NOT_FOUND: missing"
        );
        assert!(interpret_stats(Err("smolvm is not on PATH".into()))
            .unwrap_err()
            .contains("PATH"));
    }
}
