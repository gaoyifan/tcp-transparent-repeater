#![warn(rust_2018_idioms)]

use std::env;
use std::error::Error;
use std::sync::Arc;

use futures::FutureExt;
use nix::sys::socket::{getsockopt, setsockopt, sockopt::Mark, sockopt::OriginalDst};
use socket2::{SockRef, TcpKeepalive};
use std::net::{Ipv4Addr, SocketAddr};
use tokio::io::copy_bidirectional_with_sizes;
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::time::Duration;

const BUF_SIZE: usize = 256 * 1024; // 256KB

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

fn get_original_dst(stream: &TcpStream) -> std::net::SocketAddr {
    let sockaddr_in = getsockopt(stream, OriginalDst).unwrap();

    // Convert libc::sockaddr_in to std::net::SocketAddr
    let ip = Ipv4Addr::from(u32::from_be(sockaddr_in.sin_addr.s_addr));
    let port = u16::from_be(sockaddr_in.sin_port);
    SocketAddr::from((ip, port))
}

fn get_fwmark(stream: &TcpStream) -> Option<u32> {
    getsockopt(stream, Mark).ok().filter(|&mark| mark != 0)
}

fn set_fwmark(socket: &TcpSocket, mark: u32) {
    if let Err(e) = setsockopt(socket, Mark, &mark) {
        eprintln!("[WARN] Failed to set fwmark {}: {}", mark, e);
    }
}

async fn handle_connection(
    mut inbound: TcpStream,
    proxy_addr: std::net::SocketAddr,
    keepalive: Arc<TcpKeepalive>,
) -> Result<(), Box<dyn Error>> {
    // Get the fwmark from the incoming connection
    let fwmark = get_fwmark(&inbound);

    // Create a TcpSocket to set options before connecting
    let socket = if proxy_addr.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };

    // Set the same fwmark on the outbound socket if we got one
    if let Some(mark) = fwmark {
        set_fwmark(&socket, mark);
    }

    // Connect using the configured socket
    let mut outbound = socket.connect(proxy_addr).await?;

    // Set keepalive option
    SockRef::from(&inbound).set_tcp_keepalive(&keepalive)?;
    SockRef::from(&outbound).set_tcp_keepalive(&keepalive)?;

    inbound.set_nodelay(true)?;
    outbound.set_nodelay(true)?;

    let client_addr = inbound.peer_addr()?;
    let server_addr = outbound.peer_addr()?;

    let result = copy_bidirectional_with_sizes(&mut inbound, &mut outbound, BUF_SIZE, BUF_SIZE).await?;

    eprintln!(
        "[INFO] {} -> {}: connection closed with {} bytes transferred",
        client_addr,
        server_addr,
        result.0 + result.1
    );

    Ok(())
}
