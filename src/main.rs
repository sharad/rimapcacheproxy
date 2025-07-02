use clap::Parser;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_native_tls::TlsConnector;
use native_tls::TlsConnector as NativeTlsConnector;
use tokio_native_tls::TlsStream;
use anyhow::Result;


// #[derive(Parser, Debug)]
// #[command(author, version, about = "IMAP Proxy with TLS/plain upstream")]
// struct Args {
//     /// Address to listen on (e.g., 127.0.0.1:1143)
//     #[arg(short, long)]
//     proxy_addr: String,

//     /// Upstream IMAP server address (e.g., imap.gmail.com:993)
//     #[arg(short, long)]
//     upstream_addr: String,

//     /// Use TLS for upstream connection
//     #[arg(long)]
//     tls: bool,
// }


#[derive(Parser, Debug)]
#[command(author, version, about = "IMAP Proxy with TLS/plain upstream")]
struct Args {
    /// Address to listen on (e.g., 127.0.0.1:1143)
    #[arg(short, long, default_value = "127.0.0.1:1143")]
    proxy_addr: String,

    /// Upstream IMAP server address (e.g., imap.gmail.com:993)
    #[arg(short, long, default_value = "imap.gmail.com:993")]
    upstream_addr: String,

    /// Use TLS for upstream connection
    #[arg(long, action = clap::ArgAction::Set, default_value_t = true)]
    tls: bool,
}


enum Upstream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
}

impl Upstream {
    fn split(self) -> (Box<dyn AsyncRead + Send + Unpin>, Box<dyn AsyncWrite + Send + Unpin>) {
        match self {
            Upstream::Plain(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w))
            },
            Upstream::Tls(s) => {
                let (r, w) = tokio::io::split(s);
                (Box::new(r), Box::new(w))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // let proxy_addr = "127.0.0.1:1143";
    // let upstream_addr = "imap.gmail.com:993";
    // let use_tls = true;


    let args = Args::parse();
    let proxy_addr = args.proxy_addr;
    let upstream_addr = args.upstream_addr;
    let use_tls = args.tls;

    let listener = TcpListener::bind(&proxy_addr).await?;
    println!("IMAP proxy with TLS/plain upstream listening on {}", proxy_addr);

    loop {
        let (client, addr) = listener.accept().await?;
        println!("New connection from {}", addr);
        let upstream_addr = upstream_addr.to_string();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(client, &upstream_addr, use_tls).await {
                eprintln!("Connection error: {:?}", e);
            }
        });
    }
}

async fn handle_connection(client: TcpStream, upstream_addr: &str, use_tls: bool) -> Result<()> {
    let upstream_tcp = TcpStream::connect(upstream_addr).await?;

    let upstream = if use_tls {
        let connector = TlsConnector::from(NativeTlsConnector::new()?);
        let domain = upstream_addr.split(':').next().unwrap();
        let tls_stream = connector.connect(domain, upstream_tcp).await?;
        println!("Connected with TLS to {}", upstream_addr);
        Upstream::Tls(tls_stream)
    } else {
        println!("Connected in plaintext to {}", upstream_addr);
        Upstream::Plain(upstream_tcp)
    };

    let (mut client_read, mut client_write) = client.into_split();


    // let (mut upstream_read, mut upstream_write): (
    //     Box<dyn AsyncRead + Send + Unpin>,
    //     Box<dyn AsyncWrite + Send + Unpin>
    // ) = match upstream {
    //     Upstream::Plain(s) => {
    //         let (r, w) = tokio::io::split(s);
    //         (Box::new(r), Box::new(w))
    //     },
    //     Upstream::Tls(s) => {
    //         let (r, w) = tokio::io::split(s);
    //         (Box::new(r), Box::new(w))
    //     }
    // };

    let (mut upstream_read, mut upstream_write): (
        Box<dyn AsyncRead + Send + Unpin>,
        Box<dyn AsyncWrite + Send + Unpin>
    ) = upstream.split();

    let c2u = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let n = match client_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Client read error: {:?}", e);
                    break;
                }
            };
            if let Err(e) = upstream_write.write_all(&buf[..n]).await {
                eprintln!("Write to upstream error: {:?}", e);
                break;
            }
        }
    });

    let u2c = tokio::spawn(async move {
        let mut buf = [0u8; 4096];
        loop {
            let n = match upstream_read.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("Upstream read error: {:?}", e);
                    break;
                }
            };
            if let Err(e) = client_write.write_all(&buf[..n]).await {
                eprintln!("Write to client error: {:?}", e);
                break;
            }
        }
    });

    let _ = tokio::try_join!(c2u, u2c);
    Ok(())
}
