extern crate bracha_bcast;
extern crate tokio_core;
extern crate tokio_timer;
extern crate futures;
extern crate log;

use futures::Future;
use tokio_core::reactor::Core;
use tokio_timer::Timer;

use std::env;
use std::time::Duration;

use bracha_bcast::*;

fn main() {
    let mut args: Vec<String> = env::args().collect();
    args.remove(0);
    if args.len() < 1 {
        println!("need args");
        return;
    }

    // last one is my own address
    let mut addrs = strings_to_addrs(args);
    let me = addrs.pop().unwrap();

    let config = Config::new(me, addrs, 1);
    let mut core = Core::new().unwrap();
    let handle = core.handle();

    let config2 = config.clone();
    if config2.borrow().me == "127.0.0.1:12345".parse().unwrap() {
        let sleep = Timer::default().sleep(Duration::from_millis(500));
        handle.spawn(
            sleep.and_then(move |_| {
                broadcast(&config2.borrow().nodes, Msg::new("hello".to_string()));
                println!("sent...");
                Ok(())
            }).map_err(|e| println!("broadcast failed {:?}", e))
        );
    }

    core.run(serve(config, handle)).unwrap();
}
