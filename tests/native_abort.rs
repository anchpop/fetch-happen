#![cfg(not(target_arch = "wasm32"))]
use fetch_happen::{AbortController, Client, Error};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::{timeout, Duration},
};

#[tokio::test]
async fn precancelled_request_never_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let controller = AbortController::default();
    controller.abort();
    let result = Client
        .get(format!("http://{}", listener.local_addr().unwrap()))
        .abort_signal(controller.signal())
        .send()
        .await;
    assert!(matches!(result, Err(Error::Aborted)));
    assert!(timeout(Duration::from_millis(30), listener.accept())
        .await
        .is_err());
}

async fn interrupt(after_headers: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (ready, started) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        if after_headers {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\nx")
                .await
                .unwrap();
        }
        ready.send(()).unwrap();
        let mut byte = [0];
        // Cancelling drops reqwest's pending request/body, releasing the socket.
        let result = timeout(Duration::from_secs(2), stream.read(&mut byte))
            .await
            .unwrap();
        assert!(matches!(result, Ok(0) | Err(_)));
    });
    let controller = AbortController::default();
    let request = async {
        let response = Client
            .get(url)
            .abort_signal(controller.signal())
            .send()
            .await?;
        // Headers arrived; the body is what stalls.
        response.bytes().await
    };
    let cancel = async {
        started.await.unwrap();
        // Allow the client to consume the headers before interrupting the body case.
        tokio::time::sleep(Duration::from_millis(20)).await;
        controller.abort();
    };
    let (result, ()) = timeout(Duration::from_secs(3), async {
        tokio::join!(request, cancel)
    })
    .await
    .unwrap();
    assert!(matches!(result, Err(Error::Aborted)), "{result:?}");
    server.await.unwrap();
}

/// `send()` must resolve on headers, and chunks must arrive as the server
/// writes them rather than after the whole body lands.
#[tokio::test]
async fn body_streams_after_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (release, held) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhello")
            .await
            .unwrap();
        held.await.unwrap();
        stream.write_all(b"world").await.unwrap();
    });
    let response = timeout(Duration::from_secs(2), Client.get(url).send())
        .await
        .expect("send() should resolve on headers, before the body completes")
        .unwrap();
    assert_eq!(response.status(), 200);
    let reader = response.stream_reader().unwrap();
    let first = timeout(Duration::from_secs(2), reader.read_chunk())
        .await
        .expect("first chunk should arrive before the server sends the rest")
        .unwrap()
        .unwrap();
    assert_eq!(first, b"hello");
    release.send(()).unwrap();
    let mut rest = Vec::new();
    while let Some(chunk) = reader.read_chunk().await.unwrap() {
        rest.extend(chunk);
    }
    assert_eq!(rest, b"world");
    assert!(reader.read_chunk().await.unwrap().is_none());
    assert!(
        matches!(response.bytes().await, Err(Error::Transport(_))),
        "a body handed to a stream reader can't be read again"
    );
    server.await.unwrap();
}
#[tokio::test]
async fn aborts_pending_headers() {
    interrupt(false).await;
}
#[tokio::test]
async fn aborts_pending_body() {
    interrupt(true).await;
}
