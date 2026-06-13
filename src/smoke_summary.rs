pub const CORE_AGENT_STARTED: &str = "core_agent_started";
pub const CORE_LOOP_COMPLETED: &str = "core_loop_completed";
pub const CORE_NO_ERRORS: &str = "core_no_errors";
pub const CORE_BASIC_INFO: &str = "core_basic_info";
pub const CORE_REPORT_CONNECTED: &str = "core_report_connected";
pub const CORE_REPORT_SENT: &str = "core_report_sent";
pub const CONTROL_PING_RESULT: &str = "control_ping_result";
pub const CONTROL_EXEC_RESULT: &str = "control_exec_result";
pub const CONTROL_TERMINAL_SESSION: &str = "control_terminal_session";
pub const CONTROL_CN_CONNECTIVITY: &str = "control_cn_connectivity";

pub fn smoke_event_line(event: &str, fields: &[(&str, &str)]) -> String {
    let mut output = format!("smoke: {}", sanitize_event_part(event).replace(' ', "_"));
    for (key, value) in fields {
        output.push(' ');
        output.push_str(&sanitize_event_part(key).replace(' ', "_"));
        output.push('=');
        output.push_str(&sanitize_event_part(value));
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmokeEvidenceStatus {
    Pass,
    Fail,
    Missing,
}

impl SmokeEvidenceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmokeEvidence {
    pub id: &'static str,
    pub label: &'static str,
    pub status: SmokeEvidenceStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmokeSummary {
    source: String,
    evidence: Vec<SmokeEvidence>,
    compatibility_gaps: Vec<String>,
}

impl SmokeSummary {
    pub fn evidence_status(&self, id: &str) -> Option<SmokeEvidenceStatus> {
        self.evidence
            .iter()
            .find(|evidence| evidence.id == id)
            .map(|evidence| evidence.status)
    }

    pub fn is_pass(&self) -> bool {
        self.evidence
            .iter()
            .all(|evidence| evidence.status == SmokeEvidenceStatus::Pass)
    }

    pub fn failed_or_missing_evidence(&self) -> Vec<&SmokeEvidence> {
        self.evidence
            .iter()
            .filter(|evidence| evidence.status != SmokeEvidenceStatus::Pass)
            .collect()
    }

    pub fn to_markdown(&self) -> String {
        let mut output = String::new();
        output.push_str("# kelicloud-agent-rs Smoke Summary\n\n");
        output.push_str(&format!(
            "Source: `{}`\n\n",
            escape_markdown_cell(&self.source)
        ));
        output.push_str("## Evidence\n\n");
        output.push_str("| Check | Status | Detail |\n");
        output.push_str("| --- | --- | --- |\n");
        for evidence in &self.evidence {
            output.push_str(&format!(
                "| {} | {} | {} |\n",
                escape_markdown_cell(evidence.label),
                evidence.status.as_str(),
                escape_markdown_cell(&evidence.detail)
            ));
        }

        output.push_str("\n## Compatibility Gaps\n\n");
        if self.compatibility_gaps.is_empty() {
            output.push_str("No missing live control-plane evidence.\n");
        } else {
            for gap in &self.compatibility_gaps {
                output.push_str("- ");
                output.push_str(gap);
                output.push('\n');
            }
        }

        output
    }
}

pub fn summarize_log_text(source: impl Into<String>, text: &str) -> SmokeSummary {
    let source = source.into();
    let lower = text.to_ascii_lowercase();
    let fatal_error = first_fatal_error(text);
    let agent_started = lower.contains("kelicloud-agent-rs prototype");
    let loop_completed =
        lower.contains("agent loop: completed") || lower.contains("live smoke duration reached");
    let basic_info_observed = lower.contains("smoke: basic_info_uploaded") || loop_completed;
    let report_connected_observed =
        lower.contains("smoke: report_websocket_connected") || loop_completed;
    let report_sent_observed = lower.contains("smoke: report_sent") || loop_completed;

    let mut items = vec![
        evidence(
            CORE_AGENT_STARTED,
            "Agent startup",
            observed(agent_started),
            observed_detail(agent_started),
        ),
        evidence(
            CORE_LOOP_COMPLETED,
            "Agent loop completed or stayed alive",
            observed(loop_completed),
            observed_detail(loop_completed),
        ),
        evidence(
            CORE_NO_ERRORS,
            "No fatal agent errors",
            if fatal_error.is_some() {
                SmokeEvidenceStatus::Fail
            } else {
                SmokeEvidenceStatus::Pass
            },
            fatal_error.unwrap_or_else(|| "none".to_string()),
        ),
        evidence(
            CORE_BASIC_INFO,
            "Basic info upload",
            observed(basic_info_observed),
            observed_detail(basic_info_observed),
        ),
        evidence(
            CORE_REPORT_CONNECTED,
            "Report websocket connection",
            observed(report_connected_observed),
            observed_detail(report_connected_observed),
        ),
        evidence(
            CORE_REPORT_SENT,
            "Report payload send",
            observed(report_sent_observed),
            observed_detail(report_sent_observed),
        ),
    ];

    let control_checks = [
        (
            CONTROL_PING_RESULT,
            "Ping result upload",
            contains_any(
                &lower,
                &[
                    "smoke: ping_result_uploaded",
                    "\"type\":\"ping_result\"",
                    "ping_result uploaded",
                ],
            ),
            "Ping task was not observed; trigger a TCP ping from the panel during live smoke.",
        ),
        (
            CONTROL_EXEC_RESULT,
            "Exec task result upload",
            contains_any(
                &lower,
                &[
                    "smoke: task_result_uploaded",
                    "/api/clients/task/result",
                    "task result uploaded",
                ],
            ),
            "Exec task result upload was not observed; run a small command such as whoami from the panel.",
        ),
        (
            CONTROL_TERMINAL_SESSION,
            "Terminal session",
            contains_any(
                &lower,
                &[
                    "smoke: terminal_session_started",
                    "/api/clients/terminal",
                    "terminal session started",
                ],
            ),
            "Terminal session was not observed; open WebSSH during live smoke.",
        ),
        (
            CONTROL_CN_CONNECTIVITY,
            "CN connectivity config",
            contains_any(
                &lower,
                &[
                    "smoke: cn_connectivity_config_received",
                    "cn_connectivity_probe_config",
                ],
            ),
            "CN connectivity config was not observed; wait for the panel to send probe config during live smoke.",
        ),
    ];

    let mut compatibility_gaps = Vec::new();
    for (id, label, passed, missing_note) in control_checks {
        items.push(evidence(
            id,
            label,
            observed(passed),
            observed_detail(passed),
        ));
        if !passed {
            compatibility_gaps.push(missing_note.to_string());
        }
    }

    SmokeSummary {
        source,
        evidence: items,
        compatibility_gaps,
    }
}

fn evidence(
    id: &'static str,
    label: &'static str,
    status: SmokeEvidenceStatus,
    detail: impl Into<String>,
) -> SmokeEvidence {
    SmokeEvidence {
        id,
        label,
        status,
        detail: detail.into(),
    }
}

fn observed(value: bool) -> SmokeEvidenceStatus {
    if value {
        SmokeEvidenceStatus::Pass
    } else {
        SmokeEvidenceStatus::Missing
    }
}

fn observed_detail(value: bool) -> &'static str {
    if value {
        "observed"
    } else {
        "not observed"
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn first_fatal_error(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("runtime error:")
                || lower.starts_with("configuration error:")
                || lower.starts_with("transport error:")
                || lower.starts_with("auto-discovery error:")
                || lower.starts_with("task uploader error:")
                || lower.starts_with("error:")
        })
        .map(ToString::to_string)
}

fn escape_markdown_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn sanitize_event_part(value: &str) -> String {
    let text = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() && ch != '|' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();
    let sanitized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if sanitized.is_empty() {
        "-".to_string()
    } else {
        sanitized
    }
}
