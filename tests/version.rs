use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn version_flag_prints_the_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_wowlua_ls"))
        .arg("--version")
        .output()
        .expect("run `wowlua_ls --version`");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), format!("{VERSION}\n"));
}

#[test]
fn server_reports_the_package_version() {
    let mut server = Command::new(env!("CARGO_BIN_EXE_wowlua_ls"))
        .arg("--stdio")
        .env("RUST_LOG", "info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the server");

    let request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#;
    let mut stdin = server.stdin.take().unwrap();
    write!(stdin, "Content-Length: {}\r\n\r\n{request}", request.len()).unwrap();
    stdin.flush().unwrap();

    let mut stdout = BufReader::new(server.stdout.take().unwrap());
    let response = loop {
        let message = read_message(&mut stdout);
        if message["id"] == 1 {
            break message;
        }
    };
    server.kill().unwrap();
    let stderr = server.wait_with_output().unwrap().stderr;

    assert_eq!(response["result"]["serverInfo"]["version"], VERSION);
    let log = String::from_utf8_lossy(&stderr);
    assert!(log.contains(&format!("Starting wowlua_ls {VERSION}\n")), "startup log:\n{log}");
}

fn read_message(reader: &mut impl BufRead) -> serde_json::Value {
    let mut length = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "the server closed stdout before responding");
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = Some(value.parse().unwrap());
        }
    }
    let mut body = vec![0; length.expect("a Content-Length header")];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}
