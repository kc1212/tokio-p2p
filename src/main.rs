extern crate bracha_bcast;
extern crate tokio_core;
extern crate tokio_timer;
extern crate futures;

use bracha_bcast::*;
use tokio_core::reactor::Core;
use tokio_core::net::UdpSocket;
use tokio_timer::Timer;
use futures::sync::mpsc;
use futures::{Future, Sink};

use std::env;
use std::time::Duration;

fn main() {
    let mut args: Vec<String> = env::args().collect();
    args.remove(0);
    if args.len() < 1 {
        println!("need args");
        return;
    }

    let addrs = strings_to_addrs(args);
    let mut core = Core::new().unwrap();
    let sock = UdpSocket::bind(&addrs[0], &core.handle()).unwrap();
    let (tx, rx) = mpsc::channel(1);
    let remote = core.remote();

    if addrs[0] == "127.0.0.1:12345".parse().unwrap() {
        let timer = Timer::default();
        let sleep = timer.sleep(Duration::from_millis(500));
        let msg = tx.send("hello".to_string());

        remote.spawn(|_| {
            sleep.then(|_| msg).then(|_| Ok(()))
        })
    }

    let server = Server::new(addrs);
    core.run(server.serve(sock.framed(MsgCodec), rx)).expect("core failure");
}
