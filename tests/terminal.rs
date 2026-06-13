use kelicloud_agent_rs::protocol::BackendMessage;
use kelicloud_agent_rs::runtime::ControlMessageHandler;
use kelicloud_agent_rs::terminal::{
    parse_terminal_client_text, TerminalClientCommand, TerminalConnector,
    TerminalControlMessageHandler, TerminalError,
};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn terminal_handler_starts_connector_in_background() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut handler = TerminalControlMessageHandler::new(
        SlowTerminalConnector::new(calls.clone(), Duration::from_millis(150)),
        false,
    );

    let started_at = Instant::now();
    handler.handle(BackendMessage::Terminal {
        request_id: "term-1".to_string(),
    });

    assert!(
        started_at.elapsed() < Duration::from_millis(75),
        "terminal handler blocked for {:?}",
        started_at.elapsed()
    );
    assert!(
        wait_for_call_count(&calls, 1, Duration::from_secs(1)),
        "terminal connector was not started"
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [("term-1".to_string(), false)]
    );
}

#[test]
fn terminal_handler_passes_disabled_flag_to_connector() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut handler =
        TerminalControlMessageHandler::new(RecordingTerminalConnector::new(calls.clone()), true);

    handler.handle(BackendMessage::Terminal {
        request_id: "term-disabled".to_string(),
    });

    assert!(
        wait_for_call_count(&calls, 1, Duration::from_secs(1)),
        "terminal connector was not started"
    );
    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [("term-disabled".to_string(), true)]
    );
}

#[test]
fn terminal_handler_ignores_empty_request_id() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut handler =
        TerminalControlMessageHandler::new(RecordingTerminalConnector::new(calls.clone()), false);

    handler.handle(BackendMessage::Terminal {
        request_id: String::new(),
    });

    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn parse_terminal_client_text_extracts_input_command() {
    assert_eq!(
        parse_terminal_client_text(br#"{"type":"input","input":"whoami\n"}"#),
        TerminalClientCommand::Input(b"whoami\n".to_vec())
    );
}

#[test]
fn parse_terminal_client_text_extracts_resize_command() {
    assert_eq!(
        parse_terminal_client_text(br#"{"type":"resize","cols":120,"rows":40}"#),
        TerminalClientCommand::Resize {
            cols: 120,
            rows: 40
        }
    );
}

#[test]
fn parse_terminal_client_text_treats_plain_text_as_input() {
    assert_eq!(
        parse_terminal_client_text(b"raw input"),
        TerminalClientCommand::Input(b"raw input".to_vec())
    );
}

#[derive(Clone)]
struct RecordingTerminalConnector {
    calls: Arc<Mutex<Vec<(String, bool)>>>,
}

impl RecordingTerminalConnector {
    fn new(calls: Arc<Mutex<Vec<(String, bool)>>>) -> Self {
        Self { calls }
    }
}

impl TerminalConnector for RecordingTerminalConnector {
    fn start_terminal(
        &self,
        request_id: &str,
        remote_control_disabled: bool,
    ) -> Result<(), TerminalError> {
        self.calls
            .lock()
            .unwrap()
            .push((request_id.to_string(), remote_control_disabled));
        Ok(())
    }
}

#[derive(Clone)]
struct SlowTerminalConnector {
    calls: Arc<Mutex<Vec<(String, bool)>>>,
    delay: Duration,
}

impl SlowTerminalConnector {
    fn new(calls: Arc<Mutex<Vec<(String, bool)>>>, delay: Duration) -> Self {
        Self { calls, delay }
    }
}

impl TerminalConnector for SlowTerminalConnector {
    fn start_terminal(
        &self,
        request_id: &str,
        remote_control_disabled: bool,
    ) -> Result<(), TerminalError> {
        thread::sleep(self.delay);
        self.calls
            .lock()
            .unwrap()
            .push((request_id.to_string(), remote_control_disabled));
        Ok(())
    }
}

fn wait_for_call_count(
    calls: &Arc<Mutex<Vec<(String, bool)>>>,
    count: usize,
    timeout: Duration,
) -> bool {
    let started_at = Instant::now();
    while started_at.elapsed() < timeout {
        if calls.lock().unwrap().len() >= count {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
