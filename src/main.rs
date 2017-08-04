extern crate tokio_core;
extern crate tokio_io;
extern crate futures;
extern crate nix;
extern crate libc;

use std::sync::Arc;
use std::env;
use std::net::{Shutdown, SocketAddr};
use std::io::{self, Read, Write};

use futures::stream::Stream;
use futures::{Future, Poll};
use tokio_core::net::{TcpListener, TcpStream};
use tokio_core::reactor::Core;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::io::{copy, shutdown};

use nix::sys::socket::getsockopt;
use nix::sys::socket::{sockopt, InetAddr};
use std::os::unix::io::AsRawFd;

fn main() {
    let listen_addr = env::args().nth(1).unwrap_or("127.0.0.1:1080".to_string());
    let listen_addr = listen_addr.parse::<SocketAddr>().unwrap();

    // Create the event loop that will drive this server.
    let mut l = Core::new().unwrap();
    let handle = l.handle();

    // Create a TCP listener which will listen for incoming connections.
    let socket = TcpListener::bind(&listen_addr, &l.handle()).unwrap();
    println!("Listening on: {}", listen_addr);

    let done = socket
        .incoming()
        .for_each(move |(client, client_addr)| {
            let server_addr = getsockopt(client.as_raw_fd(), sockopt::OriginalDst).unwrap();
            let server_addr = InetAddr::V4(server_addr).to_std();
            if client.local_addr().unwrap() == server_addr {
                eprintln!("[WARM]from {} to {} error, cannot service to local network.",
                          client_addr,
                          server_addr);
                return Ok(());
            }
            let server = TcpStream::connect(&server_addr, &handle);
            client.set_nodelay(true).unwrap_or_default();
            let amounts = server.and_then(move |server| {
                server.set_nodelay(true).unwrap_or_default();
                let client_reader = TransparentTcpStream(Arc::new(client));
                let client_writer = client_reader.clone();
                let server_reader = TransparentTcpStream(Arc::new(server));
                let server_writer = server_reader.clone();
                let client_to_server =
                    copy(client_reader, server_writer)
                        .and_then(|(n, _, server_writer)| shutdown(server_writer).map(move |_| n));

                let server_to_client =
                    copy(server_reader, client_writer)
                        .and_then(|(n, _, client_writer)| shutdown(client_writer).map(move |_| n));

                client_to_server.join(server_to_client)
            });

            let msg = amounts
                .map(move |(from_client, from_server)| {
                    println!("[INFO]from {} to {} closed, wrote {} bytes and received {} bytes",
                             client_addr,
                             server_addr,
                             from_client,
                             from_server);
                })
                .map_err(|e| {
                             // Don't panic. Maybe the client just disconnected too soon.
                             println!("[WARM]error: {}", e);
                         });
            handle.spawn(msg);

            Ok(())
        });
    l.run(done).unwrap();
}

#[derive(Clone)]
struct TransparentTcpStream(Arc<TcpStream>);

impl Read for TransparentTcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for TransparentTcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsyncRead for TransparentTcpStream {}

impl AsyncWrite for TransparentTcpStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        try!(self.0.shutdown(Shutdown::Write));
        Ok(().into())
    }
}
