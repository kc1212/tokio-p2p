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
    let s = serve(node.clone(), handle.clone());

    for p in peers {
        handle.spawn(start_client(node.clone(), handle.clone(), &p.parse().unwrap())
                     .then(move |x| {
                         println!("client {} done {:?}", p, x);
                         Ok(())
                     }));
    }

    let timer = Timer::default();
    let node2 = node.clone();
    match msg {
        Some(msg) => {
            let f = timer.sleep(Duration::from_secs(1))
                .and_then(move |_| {
                    node2.borrow().broadcast(msg);
                    Ok(())
                });
            handle.spawn(f.then(|_| Ok(())));
        }
        None => (),
    }

    /*
    handle.spawn(gossip_periodic(node.clone(), timer.interval(Duration::from_secs(1)), Msg::).then(|e| {
        println!("Run periodic failed {:?}", e);
        Ok(())
    }));
    */

    core.run(s).unwrap();
}

fn main() {
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