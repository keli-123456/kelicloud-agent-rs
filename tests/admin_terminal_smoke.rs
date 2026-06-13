use kelicloud_agent_rs::admin_terminal_smoke::{
    build_admin_terminal_ws_url, session_cookie_header,
};

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
