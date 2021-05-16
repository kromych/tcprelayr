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
    connection: &mut dyn Read,
    relay_connection: &mut dyn Write,
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

        print!("received {} bytes ", bytes_read);

        if bytes_read != 0 {
            let bytes_wrote = relay_connection.write(&received_data[..bytes_read]);
            if let Ok(bytes_wrote) = bytes_wrote {
                println!("; sent {} bytes", bytes_wrote)
            }

            match bytes_wrote {
                Ok(n) if n < bytes_read => return Err(io::ErrorKind::WriteZero.into()),
                Ok(_) => {}
                Err(ref err) if would_block(err) => {}
                Err(ref err) if interrupted(err) => {
                    return handle_connection_event(registry, connection, relay_connection, event)
                }
                Err(err) => return Err(err),
            }
        } else {
            println!();
        }

        if connection_closed {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn run_tcp_relay_server(server_addr: &SocketAddr, client_addr: &SocketAddr) -> io::Result<()> {
    let mut current_token: usize = 0;

    let readable = Interest::READABLE;
    let readable_or_writable = readable | Interest::WRITABLE;

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    let mut server = TcpListener::bind(*server_addr)?;

    poll.registry()
        .register(&mut server, Token(current_token), readable)?;

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

                    println!("Relay server accepted connection from: {}", address);

                    let mut upstream = TcpStream::connect(*client_addr).unwrap();

                    poll.registry().register(
                        &mut connection,
                        Token(current_token),
                        readable_or_writable,
                    )?;
                    poll.registry().register(
                        &mut upstream,
                        Token(current_token + 1),
                        readable_or_writable,
                    )?;

                    connections.insert(current_token, (connection, upstream));

                    current_token += 2;
                },
                token => {
                    let upstream_event = token & 1 == 0;
                    let token = if upstream_event { token - 1 } else { token };

                    let (connection, upstream) = connections.get_mut(&token).unwrap();
                    let done = if upstream_event {
                        handle_connection_event(poll.registry(), upstream, connection, event)?
                    } else {
                        handle_connection_event(poll.registry(), connection, upstream, event)?
                    };

                    if done {
                        connections.remove(&token);
                        connections.remove(&(token + 1));
                    } else if upstream_event {
                        poll.registry()
                            .reregister(upstream, event.token(), Interest::READABLE)
                            .ok();
                    } else {
                        poll.registry()
                            .reregister(connection, event.token(), Interest::READABLE)
                            .ok();
                    }
                }
            }
        }
    }
}
