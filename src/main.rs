extern crate tokio_core;
extern crate tokio_timer;
extern crate futures;
extern crate getopts;
extern crate tokio_p2p;

use futures::Future;
use tokio_core::reactor::Core;
use tokio_timer::Timer;

use std::env;
use std::time::Duration;
use std::net::SocketAddr;

use tokio_p2p::node::*;

fn print_usage(program: &str, opts: getopts::Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn do_work(me: SocketAddr, msg: Option<String>) {
    let peers = vec!["127.0.0.1:12345", "127.0.0.1:12346", "127.0.0.1:12347"];

    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let node = Node::new(me);
    let node2 = node.clone();
    let node3 = node.clone();
    let s = serve(node, handle.clone());

    for p in peers {
        handle.spawn(start_client(node2.clone(), handle.clone(), &p.parse().unwrap())
                     .then(|e| {
                         println!("Failed to connect {:?}", e);
                         Ok(())
                     }));
    }

    match msg {
        Some(msg) => {
            let timer = Timer::default();
            let f = timer.sleep(Duration::from_millis(5000))
                .and_then(move |_| {
                    node3.borrow().broadcast(msg);
                    Ok(())
                });
            handle.spawn(f.then(|_| Ok(())));
        }
        None => (),
    }

    core.run(s).unwrap();
}

fn main() {
    // the first argument is the host address
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = getopts::Options::new();
    opts.optopt("a", "", "set the host address (required)", "ADDR");
    opts.optopt("b", "", "broadcast a message after 1 second", "MSG");
    opts.optflag("h", "", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };

    if matches.opt_present("h") {
        print_usage(&program, opts);
        return;
    }

    match matches.opt_str("a") {
        Some(addr) => do_work(addr.parse().unwrap(), matches.opt_str("b")),
        None => print_usage(&program, opts),
    }
}
