


use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, split};
use std::sync::{Arc, Mutex};
use chrono::{Utc, Duration, DateTime};
use std::collections::HashMap;

type UID = u32;

struct CachedMessage {
    data: Vec<u8>,
    timestamp: DateTime<Utc>,
}

struct Cache {
    messages: Mutex<HashMap<UID, CachedMessage>>,
}

impl Cache {
    fn new() -> Self {
        Self {
            messages: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, uid: UID) -> Option<Vec<u8>> {
        let mut msgs = self.messages.lock().unwrap();
        if let Some(entry) = msgs.get(&uid) {
            if Utc::now() - entry.timestamp < Duration::hours(24) {
                return Some(entry.data.clone());
            } else {
                msgs.remove(&uid);
            }
        }
        None
    }

    fn insert(&self, uid: UID, data: Vec<u8>) {
        let mut msgs = self.messages.lock().unwrap();
        msgs.insert(uid, CachedMessage { data, timestamp: Utc::now() });
    }
}

async fn handle_client(mut client: TcpStream, upstream_addr: &str, cache: Arc<Cache>) -> anyhow::Result<()> {
    let mut upstream = TcpStream::connect(upstream_addr).await?;

    let (mut cr, mut cw) = split(client);
    let (mut ur, mut uw) = split(upstream);

    let cache_clone = cache.clone();

    // Forward client -> upstream
    let client_to_upstream = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let n = match cr.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("read error: {:?}", e);
                    break;
                }
            };

            // Here you can parse the command and intercept FETCH for caching if desired

            if let Err(e) = uw.write_all(&buf[..n]).await {
                eprintln!("write to upstream error: {:?}", e);
                break;
            }
        }
    });

    // Forward upstream -> client
    let upstream_to_client = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let n = match ur.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("read upstream error: {:?}", e);
                    break;
                }
            };

            // Here you can parse responses, detect FETCH responses, and cache them by UID if desired

            if let Err(e) = cw.write_all(&buf[..n]).await {
                eprintln!("write to client error: {:?}", e);
                break;
            }
        }
    });

    let _ = tokio::try_join!(client_to_upstream, upstream_to_client);

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let proxy_addr = "127.0.0.1:1143";
    let upstream_addr = "imap.gmail.com:993"; // or your IMAP server

    let listener = TcpListener::bind(proxy_addr).await?;
    let cache = Arc::new(Cache::new());

    println!("IMAP proxy listening on {}", proxy_addr);

    loop {
        let (client, addr) = listener.accept().await?;
        println!("New connection from {}", addr);
        let cache_clone = cache.clone();
        let upstream = upstream_addr.to_string();

        tokio::spawn(async move {
            if let Err(e) = handle_client(client, &upstream, cache_clone).await {
                eprintln!("connection error: {:?}", e);
            }
        });
    }
}
