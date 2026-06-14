use kelicloud_agent_rs::admin_terminal_smoke::{
    admin_terminal_origin, build_admin_terminal_ws_url, run_admin_terminal_smoke,
    session_cookie_header, AdminTerminalSmokeRequest,
};
use std::io::ErrorKind;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;
use tungstenite::Message;

#[test]
fn admin_terminal_ws_url_targets_panel_terminal_endpoint() {
    let url = build_admin_terminal_ws_url("http://127.0.0.1:25775/base/", "node-1").unwrap();

    assert_eq!(
        url,
        "ws://127.0.0.1:25775/base/api/admin/client/node-1/terminal"
    );
}

#[test]
fn admin_terminal_ws_url_converts_https_to_wss() {
    let url = build_admin_terminal_ws_url("https://panel.example.com", "node-1").unwrap();

    assert_eq!(
        url,
        "wss://panel.example.com/api/admin/client/node-1/terminal"
    );
}

#[test]
fn session_cookie_header_uses_backend_cookie_name() {
    assert_eq!(
        session_cookie_header(" session-token "),
        "session_token=session-token"
    );
}

#[test]
fn admin_terminal_origin_matches_panel_origin() {
    assert_eq!(
        admin_terminal_origin("http://127.0.0.1:25775/base/").unwrap(),
        "http://127.0.0.1:25775"
    );
    assert_eq!(
        admin_terminal_origin("https://panel.example.com/base/").unwrap(),
        "https://panel.example.com"
    );
}

#[test]
fn admin_terminal_smoke_sends_xterm_compatible_binary_input() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let message = socket.read().unwrap();
        match message {
            Message::Binary(bytes) => assert_eq!(bytes.as_ref(), b"whoami\r"),
            other => panic!("expected binary terminal input, got {other:?}"),
        }
        socket
            .send(Message::Binary(b"root\n".to_vec().into()))
            .unwrap();
    });

    run_admin_terminal_smoke(&AdminTerminalSmokeRequest {
        endpoint,
        session_token: "session-token".to_string(),
        client_uuid: "node-1".to_string(),
        command: "whoami".to_string(),
        expect: "root".to_string(),
        timeout: Duration::from_secs(2),
    })
    .unwrap();
    server.join().unwrap();
}

#[test]
fn admin_terminal_smoke_does_not_send_input_immediately_after_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(150)))
            .unwrap();
        match socket.read() {
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            other => panic!("terminal input was sent before the bridge was ready: {other:?}"),
        }

        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .send(Message::Text("waiting for agent".to_string().into()))
            .unwrap();
        let message = socket.read().unwrap();
        match message {
            Message::Binary(bytes) => assert_eq!(bytes.as_ref(), b"whoami\r"),
            other => panic!("expected delayed binary terminal input, got {other:?}"),
        }
        socket
            .send(Message::Binary(b"root\n".to_vec().into()))
            .unwrap();
    });

    run_admin_terminal_smoke(&AdminTerminalSmokeRequest {
        endpoint,
        session_token: "session-token".to_string(),
        client_uuid: "node-1".to_string(),
        command: "whoami".to_string(),
        expect: "root".to_string(),
        timeout: Duration::from_secs(2),
    })
    .unwrap();
    server.join().unwrap();
}

#[test]
fn admin_terminal_smoke_waits_for_shell_prompt_before_sending_input() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        socket
            .send(Message::Text("waiting for agent".to_string().into()))
            .unwrap();

        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(1200)))
            .unwrap();
        match socket.read() {
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            other => panic!("terminal input was sent before shell prompt: {other:?}"),
        }

        socket
            .get_mut()
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .send(Message::Binary(b"runner@test:~$ ".to_vec().into()))
            .unwrap();
        let message = socket.read().unwrap();
        match message {
            Message::Binary(bytes) => assert_eq!(bytes.as_ref(), b"whoami\r"),
            other => panic!("expected prompt-delayed binary input, got {other:?}"),
        }
        socket
            .send(Message::Binary(b"root\n".to_vec().into()))
            .unwrap();
    });

    run_admin_terminal_smoke(&AdminTerminalSmokeRequest {
        endpoint,
        session_token: "session-token".to_string(),
        client_uuid: "node-1".to_string(),
        command: "whoami".to_string(),
        expect: "root".to_string(),
        timeout: Duration::from_secs(3),
    })
    .unwrap();
    server.join().unwrap();
}
