#![warn(rust_2018_idioms)]

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use futures::FutureExt;
use std::error::Error;
use std::{env, io};

use nix::sys::socket::{getsockopt, sockopt, InetAddr};
use std::net::SocketAddr;
use std::os::unix::io::AsRawFd;

use bytes::BytesMut;
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::sync::{broadcast, mpsc};

use tokio::{select, time};

const BUF_SIZE: usize = 1024 * 1024;
const CHANNEL_SIZE: usize = 1024;

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

    let timeout = time::Duration::from_secs(timeout);

    println!("Listening on: {}", listen_addr);

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((inbound, client_addr)) = listener.accept().await {
        let server_addr = getsockopt(inbound.as_raw_fd(), sockopt::OriginalDst).unwrap();
        let server_addr = InetAddr::V4(server_addr).to_std();
        eprintln!(
            "[INFO]{} -> {}: connection incoming",
            client_addr, server_addr
        );
        if inbound.local_addr().unwrap() == server_addr {
            eprintln!(
                "[WARN]{} -> {}: connection from local network",
                client_addr, server_addr
            );
            continue;
        }
        let transfer = transfer(inbound, server_addr, timeout).map(move |r| {
            if let Err(e) = r {
                println!(
                    "[INFO]{} -> {}: connection closed with error; {}",
                    client_addr, server_addr, e
                );
            }
        });
        tokio::spawn(transfer);
    }

    Ok(())
}

async fn transfer(
    mut inbound: TcpStream,
    proxy_addr: SocketAddr,
    timeout: time::Duration,
) -> Result<(), Box<dyn Error>> {
    let mut outbound = TcpStream::connect(proxy_addr).await?;

    inbound.set_nodelay(true)?;
    outbound.set_nodelay(true)?;

    let client_addr = inbound.peer_addr().unwrap();
    let server_addr = outbound.peer_addr().unwrap();

    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let (stat_tx, stat_rx) = mpsc::channel(32);
    let (timeout_tx, timeout_rx1) = broadcast::channel(1);
    let timeout_rx2 = timeout_tx.subscribe();
    let timeout_rx3 = timeout_tx.subscribe();
    let timeout_rx4 = timeout_tx.subscribe();

    let (tx1,rx1) = mpsc::channel::<BytesMut>(CHANNEL_SIZE);
    let (tx2,rx2) = mpsc::channel::<BytesMut>(CHANNEL_SIZE);

    tokio::join!(
        rx_to_channel(&mut ro, tx1.clone(), timeout_rx1, stat_tx.clone()),
        channel_to_tx(&mut wi, rx1, timeout_rx2),
        rx_to_channel(&mut ri, tx2.clone(), timeout_rx3, stat_tx.clone()),
        channel_to_tx(&mut wo, rx2, timeout_rx4),
        stat_and_timeout(stat_rx, timeout_tx, timeout)
    );
    eprintln!(
        "[INFO]{} -> {}: connection closed",
        client_addr, server_addr
    );

    Ok(())
}

async fn stat_and_timeout(
    mut stat_rx: mpsc::Receiver<usize>,
    timeout_tx: broadcast::Sender<()>,
    timeout: time::Duration,
) -> Result<usize, broadcast::error::SendError<()>> {
    let mut traffic_total: usize = 0;
    loop {
        match time::timeout(timeout, stat_rx.recv()).await {
            Ok(Some(traffic)) => {
                traffic_total += traffic;
            }
            Ok(None) => continue,
            Err(_) => break,
        }
    }
    eprintln!("total traffic on both sides: {}", traffic_total);
    timeout_tx.send(())
}

async fn rx_to_channel(mut r: &mut ReadHalf<'_>,
                      ch: mpsc::Sender<BytesMut>,
                      mut timeout_rx: broadcast::Receiver<()>,
                      stat_tx: mpsc::Sender<usize>)
    -> std::io::Result<()>
{
    loop {
        select! {
            _ = r.readable() => {
                let mut buf = BytesMut::with_capacity(BUF_SIZE);
                match r.try_read_buf(&mut buf) {
                    Ok(0) => {
                        break;
                    }
                    Ok(n) => {
                        stat_tx.send(n).await.ok();
                        ch.send(buf).await.ok();
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            _ = timeout_rx.recv() => {
                break
            }
        }
    }
    Ok(())
}

async fn channel_to_tx(
    mut w: &mut WriteHalf<'_>,
    mut ch: mpsc::Receiver<BytesMut>,
    mut timeout_rx: broadcast::Receiver<()>)
    -> std::io::Result<()>
{
    loop {
        select! {
            result = ch.recv() => {
                match result {
                    Some(buf) => {
                        w.write_all(&buf).await?;
                    }
                    None => {
                        break;
                    }
                }
            }
            _ = timeout_rx.recv() => {
                break
            }
        }
    }
    Ok(())
}
