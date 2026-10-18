//! WebSocket clients against a real listener (an upgrade needs a connection,
//! not an in-process request).

use std::net::SocketAddr;
use std::time::Duration;

use axum::http::{HeaderValue, StatusCode, header};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::app::TestApp;
use crate::client::TestClient;

/// The listener serving a [`TestApp`]'s router (started on first use).
pub(crate) struct Listener {
    pub(crate) addr: SocketAddr,
    task: tokio::task::AbortHandle,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestApp {
    /// The address of a real listener serving this app on `127.0.0.1`
    /// (started on first use, stopped with the app).
    pub async fn listen(&self) -> SocketAddr {
        let listener = self
            .listener
            .get_or_init(|| async {
                let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind a test listener");
                let addr = tcp.local_addr().expect("listener address");
                let service = self.router().into_make_service_with_connect_info::<SocketAddr>();
                let task = tokio::spawn(async move {
                    let _ = axum::serve(tcp, service).await;
                });
                Listener { addr, task: task.abort_handle() }
            })
            .await;
        listener.addr
    }
}

/// A refused handshake: its status and body.
#[derive(Debug)]
pub struct Refused {
    /// The response status (not 101).
    pub status: StatusCode,
    /// The response body.
    pub body: String,
}

/// An open WebSocket of a [`TestClient`].
pub struct TestSocket {
    /// The underlying client stream (to stop reading, send raw frames…).
    pub inner: WebSocketStream<TcpStream>,
    /// The subprotocol the server selected.
    pub protocol: Option<String>,
}

impl TestClient<'_> {
    /// Opens `uri` (a path under the app) as a WebSocket, with this client's
    /// cookies and `Origin`, and its CSRF token as the panel sends it
    /// (`Sec-WebSocket-Protocol: rustcms.v1, rustcms.csrf.<token>`).
    pub async fn socket(&self, uri: &str) -> Result<TestSocket, Refused> {
        let protocols = self.csrf_token().map(|csrf| format!("rustcms.v1, rustcms.csrf.{csrf}"));
        self.socket_with(uri, protocols.as_deref(), &[]).await
    }

    /// Opens `uri` with exactly these subprotocols (`None`: no header) and
    /// extra headers; cookies and `Origin` as usual.
    pub async fn socket_with(
        &self,
        uri: &str,
        protocols: Option<&str>,
        headers: &[(&str, &str)],
    ) -> Result<TestSocket, Refused> {
        let addr = self.app().listen().await;
        let mut request = format!("ws://{addr}{uri}").into_client_request().expect("websocket request");
        let map = request.headers_mut();
        if let Some(cookies) = self.cookie_header() {
            map.insert(header::COOKIE, HeaderValue::from_str(&cookies).expect("cookie header"));
        }
        if let Some(origin) = self.origin_header() {
            map.insert(header::ORIGIN, HeaderValue::from_str(origin).expect("origin"));
        }
        if let Some(protocols) = protocols {
            map.insert(header::SEC_WEBSOCKET_PROTOCOL, HeaderValue::from_str(protocols).expect("protocols"));
        }
        for (name, value) in headers {
            map.append(
                header::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        let tcp = TcpStream::connect(addr).await.expect("connect to the test listener");
        match tokio_tungstenite::client_async(request, tcp).await {
            Ok((inner, response)) => {
                let protocol = response
                    .headers()
                    .get(header::SEC_WEBSOCKET_PROTOCOL)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                Ok(TestSocket { inner, protocol })
            }
            Err(tungstenite::Error::Http(response)) => Err(Refused {
                status: response.status(),
                body: String::from_utf8_lossy(response.body().as_deref().unwrap_or_default()).into_owned(),
            }),
            Err(error) => panic!("websocket handshake failed: {error}"),
        }
    }
}

impl TestSocket {
    /// Sends a text message.
    pub async fn send_text(&mut self, text: &str) {
        self.inner.send(Message::text(text)).await.expect("send a text message");
    }

    /// Sends a binary message.
    pub async fn send_binary(&mut self, data: &[u8]) {
        self.inner.send(Message::binary(data.to_vec())).await.expect("send a binary message");
    }

    /// The next text or binary message within `wait` (pings are answered by
    /// the client); `None` when the socket closed or nothing came.
    pub async fn next_data(&mut self, wait: Duration) -> Option<Message> {
        let read = async {
            while let Some(message) = self.inner.next().await {
                match message {
                    Ok(message @ (Message::Text(_) | Message::Binary(_))) => return Some(message),
                    Ok(Message::Close(_)) | Err(_) => return None,
                    Ok(_) => {}
                }
            }
            None
        };
        tokio::time::timeout(wait, read).await.ok().flatten()
    }

    /// Reads until the server's close frame within `wait`: its code and
    /// reason, or `None` when the connection ended without one (or nothing
    /// came). Data read meanwhile is dropped.
    pub async fn closed(&mut self, wait: Duration) -> Option<(u16, String)> {
        let read = async {
            while let Some(message) = self.inner.next().await {
                match message {
                    Ok(Message::Close(Some(CloseFrame { code, reason }))) => {
                        return Some((u16::from(code), reason.as_str().to_owned()));
                    }
                    Ok(Message::Close(None)) | Err(_) => return None,
                    Ok(_) => {}
                }
            }
            None
        };
        tokio::time::timeout(wait, read).await.ok().flatten()
    }
}
