use std::net::SocketAddr;
use std::thread;

use tcprelay;

fn main() {
    let echo_server_thread = thread::spawn(|| {
        let server_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();

        tcprelay::run_tcp_echo_server(&server_addr).ok();
    });

    let relay_server_thread = thread::spawn(|| {
        let server_addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let relay_addr: SocketAddr = "127.0.0.1:8000".parse().unwrap();

        tcprelay::run_tcp_relay_server(&relay_addr, &server_addr).ok();
    });

    echo_server_thread.join().unwrap();
    relay_server_thread.join().unwrap();
}
