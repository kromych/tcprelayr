use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::SocketAddr;

use mio::event::Event;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Registry, Token};

/// Returns `true` if the connection has been closed,
/// returns `false` otherwise.
fn handle_connection_event(
    registry: &Registry,
    connection: &mut TcpStream,
    event: &Event,
) -> io::Result<bool> {
    let mut received_data = vec![0; 4096];
    let would_block = |err: &io::Error| err.kind() == io::ErrorKind::WouldBlock;
    let interrupted = |err: &io::Error| err.kind() == io::ErrorKind::Interrupted;

    if event.is_readable() {
        let mut connection_closed = false;
        let mut bytes_read = 0;
        loop {
            match connection.read(&mut received_data[bytes_read..]) {
                Ok(0) => {
                    connection_closed = true;
                    break;
                }
                Ok(n) => {
                    bytes_read += n;
                    if bytes_read == received_data.len() {
                        received_data.resize(received_data.len() + 1024, 0);
                    }
                }
                Err(ref err) if would_block(err) => break,
                Err(ref err) if interrupted(err) => continue,
                Err(err) => return Err(err),
            }
        }

        if bytes_read != 0 {
            match connection.write(&received_data[..bytes_read]) {
                Ok(n) if n < bytes_read => return Err(io::ErrorKind::WriteZero.into()),
                Ok(_) => registry.reregister(connection, event.token(), Interest::READABLE)?,
                Err(ref err) if would_block(err) => {}
                Err(ref err) if interrupted(err) => {
                    return handle_connection_event(registry, connection, event)
                }
                Err(err) => return Err(err),
            }
        }

        if connection_closed {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn run_tcp_echo_server(addr: &SocketAddr) -> io::Result<()> {
    let mut current_token: usize = 0;

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    let mut server = TcpListener::bind(*addr)?;

    poll.registry()
        .register(&mut server, Token(current_token), Interest::READABLE)?;

    current_token += 1;

    let mut connections = HashMap::new();

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token().0 {
                0 => loop {
                    let (mut connection, address) = match server.accept() {
                        Ok((connection, address)) => (connection, address),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            break;
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    };

                    println!("Echo server accepted connection from: {}", address);

                    poll.registry().register(
                        &mut connection,
                        Token(current_token),
                        Interest::READABLE | Interest::WRITABLE,
                    )?;

                    connections.insert(current_token, connection);

                    current_token += 1;
                },
                token => {
                    let done = if let Some(connection) = connections.get_mut(&token) {
                        handle_connection_event(poll.registry(), connection, event)?
                    } else {
                        false
                    };
                    if done {
                        connections.remove(&token);
                    }
                }
            }
        }
    }
}
