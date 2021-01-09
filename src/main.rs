#![warn(rust_2018_idioms)]

use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use futures::FutureExt;
use std::env;
use std::error::Error;

use nix::sys::socket::{sockopt,getsockopt,InetAddr};
use std::os::unix::io::AsRawFd;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:1080".to_string());

    println!("Listening on: {}", listen_addr);

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((inbound, client_addr)) = listener.accept().await {
        let server_addr = getsockopt(inbound.as_raw_fd(), sockopt::OriginalDst).unwrap();
        let server_addr = InetAddr::V4(server_addr).to_std();
        eprintln!("[INFO]{} -> {}: connection incoming", client_addr, server_addr);
        if inbound.local_addr().unwrap() == server_addr {
            eprintln!("[WARM]{} -> {}: connection from local network", client_addr, server_addr);
            continue;
        }
        let transfer = transfer(inbound, server_addr).map(move |r| {
            if let Err(e) = r {
                println!("[INFO]{} -> {}: connection closed with error; {}", client_addr, server_addr, e);
            }
        });
        tokio::spawn(transfer);
    }

    Ok(())
}

async fn transfer(mut inbound: TcpStream, proxy_addr: SocketAddr) -> Result<(), Box<dyn Error>> {
    let mut outbound = TcpStream::connect(proxy_addr).await?;

    inbound.set_nodelay(true)?;
    outbound.set_nodelay(true)?;

    let client_addr = inbound.peer_addr().unwrap();
    let server_addr = outbound.peer_addr().unwrap();

    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let client_to_server = async {
        io::copy(&mut ri, &mut wo).await?;
        wo.shutdown().await
    };

    let server_to_client = async {
        io::copy(&mut ro, &mut wi).await?;
        wi.shutdown().await
    };

    tokio::try_join!(client_to_server, server_to_client)?;
    eprintln!("[INFO]{} -> {}: connection closed", client_addr, server_addr);

    Ok(())
}
