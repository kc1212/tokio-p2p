use std::io;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use uuid::Uuid;
use rand::{thread_rng, Rng, ThreadRng};

use futures::{Future, Stream, Sink};
use futures::sync::mpsc;
use tokio_core::io::Io;
use tokio_core::reactor::Handle;
use tokio_core::net::{TcpStream, TcpListener};
use tokio_timer::Interval;

use codec::{Msg, MsgCodec};

type Tx = mpsc::UnboundedSender<Msg>;

pub struct Node {
    pub id: Uuid,
    pub addr: SocketAddr,
    pub peers: HashMap<Uuid, Tx>,
    rng: Rc<RefCell<ThreadRng>>,
}

impl Node {
    pub fn new(addr: SocketAddr) -> Rc<RefCell<Node>> {
        let node = Node {
            id: Uuid::new_v4(),
            addr: addr,
            peers: HashMap::new(),
            rng: Rc::new(RefCell::new(thread_rng())),
        };
        println!("I'm {:?}", node.id);
        Rc::new(RefCell::new(node))
    }

    pub fn broadcast(&self, m: String) {
        println!("broadcasting: {}", m);
        for tx in self.peers.values() {
            // TODO do something better than expect?
            tx.send(Msg::Payload(m.clone())).expect("tx send failed");
        }
    }

    pub fn send_random(&self, m: Msg) {
        println!("sending {:?} to random node", m);
        let high = self.peers.len();
        loop {
            for tx in self.peers.values() {
                if self.rng.borrow_mut().gen_range(0, high) == 0 {
                    tx.send(m).expect("tx send failed");
                    return;
                }
            }
        }
    }

    fn process(&mut self, msg: Msg, tx: Tx) -> Result<(), io::Error> {
        match msg {
            Msg::Ping(m) => self.handle_ping(m, tx),
            Msg::Pong(m) => self.handle_pong(m, tx),
            Msg::Payload(m) => self.handle_payload(m, tx),
        }
    }

    fn handle_ping(&mut self, m: (Uuid, SocketAddr), tx: Tx) -> Result<(), io::Error> {
        println!("received ping: {:?}", m);
        match self.peers.get(&m.0) {
            Some(_) => {
                println!("PING ALREADY EXIST! {:?}", m);
                Ok(())
            }
            None => {
                println!("ADDING NODE! {:?}", m);
                let tx2 = tx.clone();
                self.peers.insert(m.0, tx);
                mpsc::UnboundedSender::send(&tx2, Msg::Pong((self.id, self.addr)))
                    .map_err(|_| io::Error::new(io::ErrorKind::Other, "tx failed"))
            }
        }
    }

    fn handle_pong(&mut self, m: (Uuid, SocketAddr), tx: Tx) -> Result<(), io::Error> {
        println!("received pong: {:?}", m);
        match self.peers.get(&m.0) {
            Some(_) => {
                println!("NODE ALREADY EXISTS {:?}", m);
            }
            None => {
                println!("ADDING NODE! {:?}", m);
                self.peers.insert(m.0, tx);
            }
        }
        Ok(())
    }

    fn handle_payload(&self, m: String, tx: Tx) -> Result<(), io::Error> {
        println!("received payload: {}", m);
        Ok(())
    }

    fn rand_port(&self) -> SocketAddr {
        // TODO this does not check whether the port is already taken
        let port: u16 = self.rng.borrow_mut().gen_range(49152, 65535);
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
    }
}

pub fn start_client(node: Rc<RefCell<Node>>, handle: Handle, addr: &SocketAddr)
                    -> Box<Future<Item=(), Error=io::Error>> {
    println!("starting client {}", addr);
    let client = TcpStream::connect(&addr, &handle).and_then(move |socket| {
        println!("connected... local: {:?}, peer {:?}", socket.local_addr(), socket.peer_addr());
        let (sink, stream) = socket.framed(MsgCodec).split();
        let (tx, rx) = mpsc::unbounded();
        let node2 = node.clone();
        let tx2 = tx.clone();

        let read = stream.for_each(move |msg| {
            node.borrow_mut().process(msg, tx.clone())
        });
        handle.spawn(read.then(|_| Ok(())));

        // client sends ping on start
        mpsc::UnboundedSender::send(&tx2, Msg::Ping((node2.borrow().id, node2.borrow().addr.clone())))
            .expect("tx failed");

        // send everything in rx to sink
        let write = sink.send_all(rx.map_err(|()| {
            io::Error::new(io::ErrorKind::Other, "rx shouldn't have an error")
        }));
        &handle.spawn(write.then(|_| Ok(())));

        Ok(())
    });

    return Box::new(client);
}

pub fn serve(node: Rc<RefCell<Node>>, handle: Handle)
             -> Box<Future<Item=(), Error=io::Error>> {
    let socket = TcpListener::bind(&node.borrow().addr, &handle).unwrap();
    println!("listening on {}", node.borrow().addr);

    let srv = socket.incoming().for_each(move |(tcpstream, addr)| {
        let (sink, stream) = tcpstream.framed(MsgCodec).split();
        let (tx, rx) = mpsc::unbounded();

        // process the incoming stream
        let node2 = node.clone();
        let read = stream.for_each(move |msg| {
            node2.borrow_mut().process(msg, tx.clone())
        });
        handle.spawn(read.then(|_| Ok(())));

        // send everything in rx to sink
        let write = sink.send_all(rx.map_err(|()| {
            io::Error::new(io::ErrorKind::Other, "rx shouldn't have an error")
        }));
        handle.spawn(write.then(|_| Ok(())));

        Ok(())
    });

    Box::new(srv)
}

/*
pub fn gossip_node_list(node: Rc<RefCell<Node>>, interval: Interval) {
    gossip_periodic(node, interval, )
}
*/

pub fn gossip_periodic(node: Rc<RefCell<Node>>, interval: Interval, m: Msg)
                  -> Box<Future<Item=(), Error=io::Error>> {
    let node = node.clone();
    let f = interval.for_each(move |_| {
        Ok(node.borrow().send_random(m.clone()))
    });

    Box::new(f.map_err(|e| {
        io::Error::new(io::ErrorKind::Other, e)
    }))
}

