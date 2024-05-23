#![warn(rust_2018_idioms)]

use std::env;
use std::error::Error;
use std::sync::Arc;

use futures::FutureExt;
use socket2::{SockRef, TcpKeepalive};
use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:1080".to_string());

    let timeout = env::args()
        .nth(2)
        .unwrap_or_else(|| "3600".to_string())
        .parse::<u64>()
        .unwrap();

    let keepalive = Arc::new(TcpKeepalive::new().with_time(Duration::from_secs(timeout)));

    println!("Listening on: {}", listen_addr);

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((inbound, client_addr)) = listener.accept().await {
        let server_addr = get_original_dst(&inbound);
        eprintln!(
            "[INFO] {} -> {}: connection incoming",
            client_addr, server_addr
        );

        if inbound.local_addr().unwrap() == server_addr {
            eprintln!(
                "[WARN] {} -> {}: connection from local network",
                client_addr, server_addr
            );
            continue;
        }

        // Set keepalive options for inbound socket
        let sock_ref = SockRef::from(&inbound);
        sock_ref.set_tcp_keepalive(&keepalive)?;

        let transfer = handle_connection(inbound, server_addr, keepalive.clone()).map(move |r| {
            if let Err(e) = r {
                println!(
                    "[INFO] {} -> {}: connection closed with error; {}",
                    client_addr, server_addr, e
                );
            }
        });
        tokio::spawn(transfer);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn get_original_dst(stream: &TcpStream) -> std::net::SocketAddr {
    use nix::sys::socket::{getsockopt, sockopt::OriginalDst, InetAddr};
    use std::os::unix::io::AsRawFd;

    let server_addr = getsockopt(stream.as_raw_fd(), OriginalDst).unwrap();
    InetAddr::V4(server_addr).to_std()
}

#[cfg(not(target_os = "linux"))]
fn get_original_dst(_stream: &TcpStream) -> std::net::SocketAddr {
    unimplemented!();
}

async fn handle_connection(
    mut inbound: TcpStream,
    proxy_addr: std::net::SocketAddr,
    keepalive: Arc<TcpKeepalive>,
) -> Result<(), Box<dyn Error>> {
    let mut outbound = TcpStream::connect(proxy_addr).await?;

    // Set keepalive option
    SockRef::from(&inbound).set_tcp_keepalive(&keepalive)?;
    SockRef::from(&outbound).set_tcp_keepalive(&keepalive)?;

    inbound.set_nodelay(true)?;
    outbound.set_nodelay(true)?;

    let client_addr = inbound.peer_addr()?;
    let server_addr = outbound.peer_addr()?;

    let result = copy_bidirectional(&mut inbound, &mut outbound).await?;

    eprintln!(
        "[INFO] {} -> {}: connection closed with {} bytes transferred",
        client_addr,
        server_addr,
        result.0 + result.1
    );

    Ok(())
}
