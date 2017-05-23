use std::io;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
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

#[derive(Clone)]
pub struct Node {
    inner: Rc<RefCell<NodeInner>>,
}

impl Node {
    pub fn new(addr: SocketAddr) -> Node {
        let node = NodeInner {
            id: Uuid::new_v4(),
            addr: addr,
            peers: HashMap::new(),
            rng: Rc::new(RefCell::new(thread_rng())),
        };
        println!("I'm {:?}", node.id);
        Node { inner: Rc::new(RefCell::new(node)) }
    }

    pub fn run<I: Iterator<Item=SocketAddr>>(&self, handle: Handle, peers: I)
               -> Box<Future<Item=(), Error=io::Error>> {
        let inner = self.inner.clone();
        let f = Node::serve(inner.clone(), handle.clone());
        for peer in peers {
            let inner = inner.clone();
            Node::start_client(inner, handle.clone(), peer);
        }
        f
    }

    fn start_client(inner: Rc<RefCell<NodeInner>>, handle: Handle, addr: SocketAddr) {
        handle.spawn(Node::start_client_actual(inner, handle.clone(), &addr).then(move |x| {
            println!("client {} done {:?}", addr, x);
            Ok(())
        }));
    }

    fn start_client_actual(inner: Rc<RefCell<NodeInner>>, handle: Handle, addr: &SocketAddr)
                        -> Box<Future<Item=(), Error=io::Error>> {
        println!("starting client {}", addr);
        let client = TcpStream::connect(&addr, &handle).and_then(move |socket| {
            println!("connected... local: {:?}, peer {:?}", socket.local_addr(), socket.peer_addr());
            let (sink, stream) = socket.framed(MsgCodec).split();
            let (tx, rx) = mpsc::unbounded();

            let inner1 = inner.clone();
            let tx1 = tx.clone();
            let handle1 = handle.clone();
            let read = stream.for_each(move |msg| {
                // inner.borrow_mut().process(msg, tx.clone())
                Node::process(inner1.clone(), msg, tx1.clone(), handle1.clone())
            });
            handle.spawn(read.then(|_| Ok(())));

            // client sends ping on start
            let inner2 = inner.clone();
            let tx2 = tx.clone();
            mpsc::UnboundedSender::send(&tx2, Msg::Ping((inner2.borrow().id, inner2.borrow().addr.clone())))
                .expect("tx failed");

            // send everything in rx to sink
            let write = sink.send_all(rx.map_err(|()| {
                io::Error::new(io::ErrorKind::Other, "rx shouldn't have an error")
            }));
            handle.spawn(write.then(|_| Ok(())));

            Ok(())
        });

        return Box::new(client);
    }

    fn serve(inner: Rc<RefCell<NodeInner>>, handle: Handle)
                 -> Box<Future<Item=(), Error=io::Error>> {
        let socket = TcpListener::bind(&inner.borrow().addr, &handle).unwrap();
        println!("listening on {}", inner.borrow().addr);

        let srv = socket.incoming().for_each(move |(tcpstream, addr)| {
            let (sink, stream) = tcpstream.framed(MsgCodec).split();
            let (tx, rx) = mpsc::unbounded();

            // process the incoming stream
            let inner1= inner.clone();
            let tx1 = tx.clone();
            let handle1 = handle.clone();
            let read = stream.for_each(move |msg| {
                Node::process(inner1.clone(), msg, tx1.clone(), handle1.clone())
                // inner2.borrow_mut().process(msg, tx.clone())
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

    fn process(inner: Rc<RefCell<NodeInner>>, msg: Msg, tx: Tx, handle: Handle) -> Result<(), io::Error> {
        match msg {
            Msg::Ping(m) => inner.borrow_mut().handle_ping(m, tx),
            Msg::Pong(m) => inner.borrow_mut().handle_pong(m, tx),
            Msg::Payload(m) => inner.borrow_mut().handle_payload(m, tx),
            Msg::AddrVec(m) => {
                for (id, addr) in m {
                    if !inner.borrow().peers.contains_key(&id) {
                        Node::start_client(inner.clone(), handle.clone(), addr);
                    }
                }
                Ok(())
            },
        }
    }

    pub fn broadcast(&self, m: String) {
        self.inner.borrow().broadcast(m)
    }

    pub fn send_random(&self, m: Msg) {
        self.inner.borrow().send_random(m)
    }

    // TODO use this function to gossip peers
    pub fn gossip_periodic(&self, interval: Interval, m: Msg)
                           -> Box<Future<Item=(), Error=io::Error>> {
        let node = self.inner.clone();
        let f = interval.for_each(move |_| {
            Ok(node.borrow().send_random(m.clone()))
        });

        Box::new(f.map_err(|e| {
            io::Error::new(io::ErrorKind::Other, e)
        }))
    }
}


struct NodeInner {
    pub id: Uuid,
    pub addr: SocketAddr,
    pub peers: HashMap<Uuid, Tx>,
    rng: Rc<RefCell<ThreadRng>>,
}

impl NodeInner {
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


