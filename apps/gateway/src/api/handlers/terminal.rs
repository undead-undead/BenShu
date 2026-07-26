use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
};

pub async fn terminal_handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_terminal_socket)
}

pub async fn handle_terminal_socket(socket: WebSocket) {
    #[cfg(target_os = "windows")]
    let mut cmd = tokio::process::Command::new("powershell.exe");

    #[cfg(not(target_os = "windows"))]
    let mut cmd = tokio::process::Command::new("bash");

    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start shell");

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut _stderr = child.stderr.take().unwrap();

    let (mut socket_sender, mut socket_receiver) = socket.split();

    let mut stdout_buf = [0u8; 1024];
    tokio::spawn(async move {
        loop {
            match stdout.read(&mut stdout_buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&stdout_buf[..n]);
                    if let Err(_) = socket_sender
                        .send(Message::Text(text.to_string().into()))
                        .await
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Some(Ok(msg)) = socket_receiver.next().await {
        if let Message::Text(text) = msg {
            let _ = stdin.write_all(text.as_bytes()).await;
        }
    }
}

use futures::{SinkExt as _, StreamExt as _};
