use std::io;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use uuid::Uuid;

use futures::{Future, Stream, Sink};
use futures::sync::mpsc;
use tokio_core::io::Io;
use tokio_core::reactor::Handle;
use tokio_core::net::{TcpStream, TcpListener};

use codec::{Msg, MsgCodec};

type Tx = mpsc::UnboundedSender<Msg>;

pub struct Node {
    pub id: Uuid,
    pub addr: SocketAddr,
    pub peers: HashMap<Uuid, Tx>,
}

impl Node {
    pub fn new(addr: SocketAddr) -> Rc<RefCell<Node>> {
        let node = Node {
            id: Uuid::new_v4(),
            addr: addr,
            peers: HashMap::new(),
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
}

pub fn start_client(node: Rc<RefCell<Node>>, handle: Handle, addr: &SocketAddr)
                    -> Box<Future<Item=(), Error=io::Error>> {
    println!("starting client on {}", node.borrow().addr);
    let f = TcpStream::connect(&addr, &handle).and_then(move |socket| {
        println!("connected to server {}", node.borrow().addr);
        let (sink, stream) = socket.framed(MsgCodec).split();
        let (tx, rx) = mpsc::unbounded();
        let node2 = node.clone();
        let tx2 = tx.clone();

        let read = stream.for_each(move |msg| {
            node.borrow_mut().process(msg, tx.clone())
        });
        handle.spawn(read.then(|_| Ok(())));

        // send ping
        mpsc::UnboundedSender::send(&tx2, Msg::Ping((node2.borrow().id, node2.borrow().addr.clone())))
            .expect("tx failed");

        // send everything in rx to sink
        let write = sink.send_all(rx.map_err(|()| {
            io::Error::new(io::ErrorKind::Other, "rx shouldn't have an error")
        }));
        &handle.spawn(write.then(|_| Ok(())));

        Ok(())
    });

    return Box::new(f);
}

pub fn serve(node: Rc<RefCell<Node>>, handle: Handle) -> Box<Future<Item=(), Error=io::Error>> {
    let socket = TcpListener::bind(&node.borrow().addr, &handle).unwrap();
    println!("listening on {}", node.borrow().addr);

    let srv = socket.incoming().for_each(move |(tcpstream, addr)| {
        let (sink, stream) = tcpstream.framed(MsgCodec).split();
        let (tx, rx) = mpsc::unbounded();

        // process the incoming stream, there shouldn't be any...
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

