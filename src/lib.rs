#![crate_type = "lib"]
#![crate_name = "bracha_bcast"]

extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate byteorder;
extern crate bincode;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate log;

use tokio_core::reactor::Handle;
use tokio_core::io::{Io, EasyBuf, Codec};
use tokio_core::net::TcpListener;
use futures::{Sink, Stream, Future};
use futures::sync::mpsc;
use bincode::{SizeLimit, serde};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use std::io::{self, Cursor};
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::mem;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Msg {
    round: u32,
    ty: MsgType,
    body: String,
}

impl Msg {
    pub fn new(body: String) -> Msg {
        Msg {
            round: 9, // TODO randomise
            ty: MsgType::Init,
            body: body,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum MsgType {
    Init,
    Echo,
    Ready,
}

pub struct MsgCodec {
    state: CodecState,
}

impl MsgCodec {
    fn new() -> Self {
        MsgCodec {
            state: CodecState::Len,
        }
    }
}

enum CodecState {
    Len,
    Payload { len: usize }, // u64 in the wire
}

impl Codec for MsgCodec {
    type In = Msg;
    type Out = Msg;

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
        use self::CodecState::*;
        loop {
            match self.state {
                Len if buf.len() < mem::size_of::<u64>() => {
                    debug!("Len: buf len is {}, waiting", buf.len());
                    return Ok(None);
                }
                Len => {
                    let mut len_buf = buf.drain_to(mem::size_of::<u64>());
                    let len = Cursor::new(&mut len_buf).read_u64::<BigEndian>()?;
                    debug!("Len: parsed len = {}", len);
                    self.state = Payload { len: len as usize };
                }
                Payload { len } if buf.len() < len => {
                    debug!("Payload: buf len is {}, waiting", buf.len());
                    return Ok(None);
                }
                Payload { len } => {
                    let payload = buf.drain_to(len);
                    let res = serde::deserialize_from(&mut Cursor::new(payload), SizeLimit::Infinite)
                        .map_err(|deserialize_err| io::Error::new(io::ErrorKind::Other, deserialize_err))?;

                    self.state = Len;

                    debug!("Payload: parsed payload: {:?}", res);
                    return Ok(Some(res));
                }
            }
        }
    }

    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> io::Result<()> {
        //buf.write_u64::<BigEndian>(id)?;
        buf.write_u64::<BigEndian>(serde::serialized_size(&msg))?;
        serde::serialize_into(buf, &msg, SizeLimit::Infinite)
            .map_err(|serialize_err| io::Error::new(io::ErrorKind::Other, serialize_err))?;
        debug!("encoded msg {:?} into {:?}", msg, buf);
        Ok(())
    }
}

type Nodes = HashMap<SocketAddr, Option<mpsc::UnboundedSender<Msg>>>;

// global setting, read only, perhaps use Arc?
pub struct Config {
    pub n: u32,
    pub t: u32,
    pub me: SocketAddr,
    pub nodes: Nodes,
}

impl Config {
    pub fn new(me: SocketAddr, addrs: Vec<SocketAddr>, t: u32) -> Rc<RefCell<Config>> {
        let mut nodes = HashMap::new();
        nodes.insert(me, None);
        for addr in addrs {
            nodes.insert(addr, None);
        }

        Rc::new(RefCell::new(
            Config {
                n: nodes.len() as u32,
                t: t,
                me: me,
                nodes: nodes,
            }
        ))
    }
}

// make connections to all clients
pub fn run_clients(config: Rc<RefCell<Config>>, handle: Handle) -> Box<Future<Item=(), Error=io::Error>> {
    unimplemented!()
}

// 
pub fn serve(config: Rc<RefCell<Config>>, handle: Handle) -> Box<Future<Item=(), Error=io::Error>> {
    let bracha = Rc::new(RefCell::new(BrachaNode::new(config.clone())));

    let socket = TcpListener::bind(&config.borrow().me, &handle).unwrap();
    println!("listening on {}", config.borrow().me);

    let srv = socket.incoming().for_each(move |(tcpstream, addr)| {
        let bracha = bracha.clone();
        let framed = tcpstream.framed(MsgCodec::new());
        let (sink, stream) = framed.split();

        let (tx, rx) = mpsc::unbounded();
        if config.borrow().nodes.contains_key(&addr) {
            config.borrow_mut().nodes.insert(addr, Some(tx));
        } else {
            return Ok(())
        }

        // process the incoming stream
        let read = stream.for_each(move |msg| {
            // application logic
            // TODO, not here
            // bracha.borrow_mut().process(msg);
            Ok(())
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

pub struct BrachaNode {
    config: Rc<RefCell<Config>>,
    round: u32,
    step: ServerStep,
    init_count: u32,
    echo_count: u32,
    ready_count: u32,
}

#[derive(Debug)]
enum ServerStep {
    // step Zero is only on the transmitter
    One,
    Two,
    Three,
}

impl BrachaNode {
    pub fn new(config: Rc<RefCell<Config>>) -> BrachaNode {
        BrachaNode {
            config: config,
            round: 0,
            step: ServerStep::One,
            init_count: 0,
            echo_count: 0,
            ready_count: 0,
        }
    }

    // do one round of processing on a new message
    pub fn process(&mut self, msg: Msg) {
        use MsgType::*;
        use ServerStep::*;
        let config = self.config.borrow();

        // access control
        if !config.nodes.contains_key(&config.me) {
            return;
        }
        // check round number
        if msg.ty != Init && msg.round != self.round {
            return;
        }
        // update the states
        match msg.ty {
            Init => { self.round = msg.round; self.init_count += 1; }
            Echo => self.echo_count += 1,
            Ready => self.ready_count += 1,
        };
        // do action on new state
        println!("received type {:?} in {:?}", msg.ty, self.step);
        match self.step {
            One => {
                if self.ok_to_echo() {
                    self.step = Two;
                    println!("into Two");
                    self.broadcast(Msg{ round: self.round, ty: Echo, body: msg.body });
                    return;
                }
            },
            Two => {
                if self.ok_to_ready() {
                    self.step = Three;
                    println!("into Three");
                    self.broadcast(Msg{ round: self.round, ty: Ready, body: msg.body });
                    return;
                }
            },
            Three => {
                self.step = One;
                self.round = 0;
                println!("Decided {:?}", msg.body);
                return;
            },
        };
    }

    fn broadcast(&self, msg: Msg) {
        broadcast(&self.config.borrow().nodes, msg);
    }

    fn ok_to_echo(&self) -> bool {
        if self.init_count > 0 || self.ok_to_ready() {
            return true;
        }
        return false;
    }

    fn ok_to_ready(&self) -> bool {
        let config = self.config.borrow();
        if (config.n + config.t) / 2 >= self.echo_count || (config.t + 1) >= self.ready_count {
            return true;
        }     
        return false;
    }
}

pub fn strings_to_addrs(ss: Vec<String>) -> Vec<SocketAddr> {
    ss.iter().map(|s| {
        s.parse().expect("parsing failed in string_to_addrs")
    }).collect()
}

pub fn broadcast(nodes: &Nodes, msg: Msg) {
    for sock in nodes.values() {
        if let &Some(ref tx) = sock {
            tx.send(msg.clone()).expect("tx send failed");
        }
    }
}
