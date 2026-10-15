//! S3-compatible storage against a real server (`RUSTCMS_TEST_S3`, S3Mock in
//! docker compose), its timeout budget, and how media downloads fail when the
//! storage does: never with backend details.

mod support;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine;
use http_body_util::BodyExt;
use rustcms_admin::media::entity as media;
use rustcms_core::storage::{self, Storage, StorageConfig, StorageError, StorageKey};
use rustcms_testing::{TestApp, TestClient, TestDb, TestResponse, TestS3, db_test};
use sea_orm::EntityTrait;
use tower::ServiceExt;

fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")
        .unwrap()
}

fn s3_storage(s3: &TestS3, timeout_secs: u64) -> Result<Arc<dyn Storage>, StorageError> {
    s3_storage_in(s3, timeout_secs, std::path::Path::new("storage/app"))
}

fn s3_storage_in(s3: &TestS3, timeout_secs: u64, root: &std::path::Path) -> Result<Arc<dyn Storage>, StorageError> {
    let mut config = StorageConfig { disk: "s3".into(), root: root.to_owned(), ..Default::default() };
    config.s3.bucket = s3.bucket.clone();
    config.s3.endpoint = Some(s3.endpoint.clone());
    config.s3.allow_http = s3.endpoint.starts_with("http://");
    config.s3.timeout_secs = timeout_secs;
    storage::connect(&config)
}

async fn put(storage: &dyn Storage, bytes: &[u8]) -> StorageKey {
    let key = StorageKey::generate("bin").unwrap();
    let file = storage.staging_dir().join(format!("{}.upload", uuid::Uuid::now_v7()));
    tokio::fs::write(&file, bytes).await.unwrap();
    storage.put_file(&key, &file, "application/octet-stream").await.unwrap();
    tokio::fs::remove_file(&file).await.unwrap();
    key
}

async fn read(storage: &dyn Storage, key: &StorageKey) -> (Vec<u8>, u64) {
    let (body, len) = storage.open(key).await.unwrap();
    (body.collect().await.unwrap().to_bytes().to_vec(), len)
}

#[tokio::test]
async fn objects_round_trip_through_the_bucket() {
    let Some(s3) = TestS3::from_env() else { return };
    let storage = s3_storage(&s3, 30).unwrap();
    assert_eq!(storage.disk(), "s3");
    let key = put(storage.as_ref(), b"hello bucket").await;
    assert!(storage.exists(&key).await.unwrap());
    assert_eq!(read(storage.as_ref(), &key).await, (b"hello bucket".to_vec(), 12));
    storage.delete(&key).await.unwrap();
    storage.delete(&key).await.unwrap(); // a second delete succeeds
    assert!(!storage.exists(&key).await.unwrap());
    assert!(matches!(storage.open(&key).await, Err(StorageError::NotFound)));

    // Larger than one part: a multipart upload.
    let big: Vec<u8> = (0..12 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let key = put(storage.as_ref(), &big).await;
    let (back, len) = read(storage.as_ref(), &key).await;
    assert_eq!((len, back.len()), (big.len() as u64, big.len()));
    assert!(back == big, "same bytes");
    storage.delete(&key).await.unwrap();
}

/// A file in `dir` last modified two hours ago.
fn stale(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7_200)).unwrap();
    path
}

#[tokio::test]
async fn interrupted_uploads_are_swept_from_staging() {
    let Some(s3) = TestS3::from_env() else { return };
    // A root of its own: no other test stages files here.
    let root = tempfile::tempdir().unwrap();
    let storage = s3_storage_in(&s3, 30, root.path()).unwrap();
    let staging = storage.staging_dir();
    assert!(staging.starts_with(root.path()), "staged under the storage root");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&staging).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let left = stale(&staging, &format!("{}.upload", uuid::Uuid::now_v7()));
    let recent = staging.join(format!("{}.upload", uuid::Uuid::now_v7()));
    std::fs::write(&recent, b"in progress").unwrap();
    let other = stale(&staging, "notes.txt");
    assert_eq!(storage.sweep(std::time::Duration::from_secs(3_600)).await.unwrap(), 1);
    assert!(!left.exists(), "the interrupted upload goes");
    assert!(recent.exists(), "uploads in progress stay");
    assert!(other.exists(), "only staged uploads are removed");
}

#[cfg(unix)]
#[tokio::test]
async fn staging_is_never_a_link() {
    let Some(s3) = TestS3::from_env() else { return };
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join(".s3-staging")).unwrap();
    assert!(matches!(s3_storage_in(&s3, 30, root.path()), Err(StorageError::Refused(_))));
}

#[tokio::test]
async fn a_bucket_that_never_answers_fails_within_the_budget() {
    // A server that accepts connections and never replies.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    let s3 = TestS3 { endpoint: format!("http://127.0.0.1:{port}"), bucket: "silent".into() };
    // Credentials come from the environment (the matrix sets test ones).
    if std::env::var("AWS_ACCESS_KEY_ID").is_err() {
        eprintln!("skipped: AWS_ACCESS_KEY_ID is not set");
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let storage = match s3_storage_in(&s3, 2, root.path()) {
        Ok(storage) => storage,
        // Built without the `s3` feature.
        Err(StorageError::Backend(message)) if message.contains("unknown storage disk") => return,
        Err(other) => panic!("{other}"),
    };
    let key = StorageKey::generate("bin").unwrap();
    // Every control request ends within its budget (2 s), retries included.
    let started = std::time::Instant::now();
    assert!(storage.exists(&key).await.is_err());
    assert!(storage.delete(&key).await.is_err());
    assert!(storage.open(&key).await.is_err());
    let elapsed = started.elapsed();
    assert!(elapsed < std::time::Duration::from_secs(9), "three calls of 2 s took {elapsed:?}");
}

async fn upload(app: &TestApp, client: &TestClient<'_>) -> TestResponse {
    upload_with(app, client, None).await
}

async fn upload_with(app: &TestApp, client: &TestClient<'_>, idempotency_key: Option<&str>) -> TestResponse {
    let boundary = "rustcms-test-boundary";
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.png\"\r\n\
         Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(&png());
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    client.fetch_csrf().await;
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(app.admin_api("media"))
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .header("content-length", body.len());
    if let Some(key) = idempotency_key {
        request = request.header("idempotency-key", key);
    }
    let request = request.body(Body::from(body)).unwrap();
    client.request(request).await
}

db_test!(media_live_in_the_bucket, |db: TestDb| async move {
    let Some(s3) = TestS3::from_env() else { return };
    db.migrate::<rustcms_migration::Migrator>().await;
    support::create_tables(db.conn()).await;
    let config = s3.configure(rustcms_testing::test_config(db.url()));
    let app = TestApp::with_config(support::app::build, config, db).await;
    let user = rustcms_testing::Factory::create(&rustcms_testing::UserFactory::new(), app.state().db()).await;
    app.make_super_admin(&user).await;
    let admin = app.acting_as(&user).await;
    let created = upload(&app, &admin).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["data"]["id"].as_str().unwrap().to_owned();
    let row =
        media::Entity::find_by_id(uuid::Uuid::parse_str(&id).unwrap()).one(app.state().db()).await.unwrap().unwrap();
    assert_eq!(row.disk, "s3");
    let bucket = s3_storage(&s3, 30).unwrap();
    let key = StorageKey::parse(&row.key).unwrap();
    assert!(bucket.exists(&key).await.unwrap(), "the object is in the bucket");

    let served = admin.get(&app.admin_api(&format!("media/{id}/file"))).await;
    served.assert_status(StatusCode::OK);
    assert_eq!(served.body.as_ref(), png().as_slice());
    assert_eq!(served.headers["content-type"], "image/png");
    // The object itself carries the detected type (S3Mock answers without signing).
    let head = std::process::Command::new("curl")
        .args(["-sfI", "--connect-timeout", "2", "-m", "5"])
        .arg(format!("{}/{}/{}", s3.endpoint, s3.bucket, row.key))
        .output()
        .unwrap();
    let head = String::from_utf8_lossy(&head.stdout).to_ascii_lowercase();
    assert!(head.contains("content-type: image/png"), "{head}");

    // Deleted for good: the queued deletion removes the object.
    admin.fetch_csrf().await;
    let record = app.admin_api(&format!("resources/media/records/{id}"));
    admin.send(Method::DELETE, &record, None).await.assert_status(StatusCode::NO_CONTENT);
    admin.fetch_csrf().await;
    admin.send(Method::DELETE, &format!("{record}/force"), None).await.assert_status(StatusCode::NO_CONTENT);
    rustcms_admin::media::prune(app.state().db(), bucket.as_ref()).await.unwrap();
    assert!(!bucket.exists(&key).await.unwrap(), "the object is gone");
});

/// A storage that keeps objects in memory and fails reads on demand.
#[derive(Default)]
struct Flaky {
    objects: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    /// `open` fails before any byte.
    fail_open: std::sync::atomic::AtomicBool,
    /// The body fails after its first chunk.
    fail_midway: std::sync::atomic::AtomicBool,
    /// The body ends cleanly after its first chunk, short of its length.
    short: std::sync::atomic::AtomicBool,
}

/// One chunk, then an error.
struct Broken(Option<axum::body::Bytes>);

impl http_body::Body for Broken {
    type Data = axum::body::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        std::task::Poll::Ready(Some(match self.0.take() {
            Some(chunk) => Ok(http_body::Frame::data(chunk)),
            None => Err(std::io::Error::other("secret-backend-detail: connection reset by https://bucket.internal")),
        }))
    }
}

#[async_trait::async_trait]
impl Storage for Flaky {
    fn disk(&self) -> &'static str {
        "local"
    }

    fn staging_dir(&self) -> std::path::PathBuf {
        std::env::temp_dir()
    }

    async fn put_file(&self, key: &StorageKey, file: &std::path::Path, _: &str) -> Result<(), StorageError> {
        let bytes = tokio::fs::read(file).await?;
        self.objects.lock().unwrap().insert(key.as_str().to_owned(), bytes);
        Ok(())
    }

    async fn open(&self, key: &StorageKey) -> Result<(Body, u64), StorageError> {
        use std::sync::atomic::Ordering;
        if self.fail_open.load(Ordering::SeqCst) {
            return Err(StorageError::Backend("secret-backend-detail: AccessDenied for key AKIA1234".into()));
        }
        let bytes = self.objects.lock().unwrap().get(key.as_str()).cloned().ok_or(StorageError::NotFound)?;
        let len = bytes.len() as u64;
        if self.short.load(Ordering::SeqCst) {
            // A stream, as a bucket's body is: its length is not known up front.
            let chunk = Ok::<_, std::io::Error>(axum::body::Bytes::from(bytes[..4].to_vec()));
            return Ok((Body::from_stream(futures_util::stream::iter([chunk])), len));
        }
        if self.fail_midway.load(Ordering::SeqCst) {
            return Ok((Body::new(Broken(Some(axum::body::Bytes::from(bytes[..4].to_vec())))), len));
        }
        Ok((Body::from(bytes), len))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        self.objects.lock().unwrap().remove(key.as_str());
        Ok(())
    }

    async fn exists(&self, key: &StorageKey) -> Result<bool, StorageError> {
        Ok(self.objects.lock().unwrap().contains_key(key.as_str()))
    }
}

/// A fresh `Flaky` per app (the database variants run in parallel), also
/// bound as itself so the test can reach it.
fn flaky_build(config: rustcms_core::Config) -> rustcms_core::AppBuilder {
    let mut app = rustcms_core::AppBuilder::new(config);
    let flaky = Arc::new(Flaky::default());
    let storage: Arc<dyn Storage> = flaky.clone();
    app.bind(flaky);
    app.bind(storage);
    app.bind(StorageConfig::default());
    app.provider(rustcms_auth::AuthProvider::new());
    app.provider(rustcms_admin::AdminProvider::new().media());
    app
}

db_test!(storage_failures_never_reach_the_client, |db: TestDb| async move {
    use std::sync::atomic::Ordering;
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(flaky_build, db).await;
    let user = rustcms_testing::Factory::create(&rustcms_testing::UserFactory::new(), app.state().db()).await;
    app.make_super_admin(&user).await;
    let admin = app.acting_as(&user).await;
    let flaky = app.state().service::<Arc<Flaky>>().unwrap();
    let created = upload(&app, &admin).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["data"]["id"].as_str().unwrap().to_owned();
    let url = app.admin_api(&format!("media/{id}/file"));

    // Before any byte: a plain 500, without the backend's message.
    flaky.fail_open.store(true, Ordering::SeqCst);
    let failed = admin.get(&url).await;
    failed.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!failed.text().contains("secret-backend-detail") && !failed.text().contains("AKIA"), "{}", failed.text());
    flaky.fail_open.store(false, Ordering::SeqCst);

    // While streaming: the response is cut short; nothing of the error is in it.
    flaky.fail_midway.store(true, Ordering::SeqCst);
    let cookie = admin.cookie("rustcms_session").unwrap();
    let request = Request::get(&url).header("cookie", format!("rustcms_session={cookie}")).body(Body::empty()).unwrap();
    let mut request = request;
    request.extensions_mut().insert(axum::extract::ConnectInfo(std::net::SocketAddr::from(([127, 0, 0, 1], 40000))));
    let response = app.router().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-length"], png().len().to_string().as_str());
    let mut body = response.into_body();
    let mut received = Vec::new();
    let mut failed = false;
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => received.extend_from_slice(frame.data_ref().map(|d| d.as_ref()).unwrap_or_default()),
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert!(failed, "the body ends in an error, not a clean end");
    assert_eq!(received, png()[..4].to_vec(), "only the bytes that were read");
    flaky.fail_midway.store(false, Ordering::SeqCst);
});

db_test!(a_replayed_upload_stores_nothing, |db: TestDb| async move {
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(flaky_build, db).await;
    let user = rustcms_testing::Factory::create(&rustcms_testing::UserFactory::new(), app.state().db()).await;
    app.make_super_admin(&user).await;
    let admin = app.acting_as(&user).await;
    let flaky = app.state().service::<Arc<Flaky>>().unwrap();
    let first = upload_with(&app, &admin, Some("replay-1")).await;
    first.assert_status(StatusCode::CREATED);
    let again = upload_with(&app, &admin, Some("replay-1")).await;
    again.assert_status(StatusCode::CREATED);
    assert_eq!(first.json()["data"]["id"], again.json()["data"]["id"]);
    assert_eq!(flaky.objects.lock().unwrap().len(), 1, "one object in storage");
});

/// The response bytes as a real client sees them over TCP, until the server
/// closes the connection.
async fn raw_get(addr: std::net::SocketAddr, path: &str, cookie: &str) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nCookie: rustcms_session={cookie}\r\n\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut received = Vec::new();
    let read = tokio::time::timeout(std::time::Duration::from_secs(10), socket.read_to_end(&mut received)).await;
    assert!(read.is_ok(), "the server closes the connection");
    received
}

db_test!(a_broken_stream_closes_the_connection, |db: TestDb| async move {
    use std::sync::atomic::Ordering;
    db.migrate::<rustcms_migration::Migrator>().await;
    let app = TestApp::new(flaky_build, db).await;
    let user = rustcms_testing::Factory::create(&rustcms_testing::UserFactory::new(), app.state().db()).await;
    app.make_super_admin(&user).await;
    let admin = app.acting_as(&user).await;
    let flaky = app.state().service::<Arc<Flaky>>().unwrap();
    let created = upload(&app, &admin).await;
    created.assert_status(StatusCode::CREATED);
    let id = created.json()["data"]["id"].as_str().unwrap().to_owned();
    let path = app.admin_api(&format!("media/{id}/file"));
    let cookie = admin.cookie("rustcms_session").unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = app.router().into_make_service_with_connect_info::<std::net::SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, router).await });

    for (failure, label) in [(&flaky.fail_midway, "an error mid-stream"), (&flaky.short, "an early end")] {
        failure.store(true, Ordering::SeqCst);
        let received = raw_get(addr, &path, &cookie).await;
        failure.store(false, Ordering::SeqCst);
        let text = String::from_utf8_lossy(&received);
        assert!(!text.contains("secret-backend-detail"), "{label}");
        // Either the connection closes before anything went out (hyper
        // buffers the headers with the first chunk), or a 200 with fewer
        // bytes than announced: a truncated download, never a complete one.
        if received.is_empty() {
            continue;
        }
        assert!(text.starts_with("HTTP/1.1 200"), "{label}: {text}");
        let body = &received[received.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..];
        assert!(body.len() < png().len(), "{label}: {} bytes", body.len());
    }
});
