#![crate_type = "lib"]
#![crate_name = "bracha_bcast"]

extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate bincode;
#[macro_use]
extern crate serde_derive;

use tokio_core::net::{UdpFramed, UdpCodec};
use futures::{stream, Sink, Stream, Future};
use futures::sync::mpsc;

use bincode::{SizeLimit, serde};
use std::io;
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Msg {
    round: u32,
    ty: MsgType,
    body: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum MsgType {
    Init,
    Echo,
    Ready,
}

pub struct MsgCodec;

use std::net::SocketAddr;
impl UdpCodec for MsgCodec {
    type In = (SocketAddr, Msg);
    type Out = (SocketAddr, Msg);

    fn decode(&mut self, src: &SocketAddr, buf: &[u8]) -> io::Result<Self::In> {
        let res = serde::deserialize(buf).map(|res| (*src, res))
            .map_err(|deserialize_err| io::Error::new(io::ErrorKind::Other, deserialize_err));
        res
    }

    fn encode(&mut self, (src, msg): Self::Out, buf: &mut Vec<u8>) -> SocketAddr {
        match serde::serialize_into(buf, &msg, SizeLimit::Infinite) {
            Ok(_) => (),
            Err(e) => println!("{:?}", e),
        };
        src
    }
}

pub struct Server {
    n: u32,
    t: u32,
    round: u32,
    step: ServerStep,
    init_count: u32,
    echo_count: u32,
    ready_count: u32,
    nodes: Rc<RefCell<Vec<SocketAddr>>>, // include myself
}

#[derive(Debug)]
enum ServerStep {
    // step Zero is only on the transmitter
    One,
    Two,
    Three,
}

impl Server {
    pub fn new(nodes: Vec<SocketAddr>) -> Self {
        let n = nodes.len() as u32;
        Server {
            n: n,
            t: (n - 1) / 3,
            round: 0,
            step: ServerStep::One,
            init_count: 0,
            echo_count: 0,
            ready_count: 0,
            nodes: Rc::new(RefCell::new(nodes)),
        }
    }

    pub fn serve(mut self, framed: UdpFramed<MsgCodec>, rx: mpsc::Receiver<String>)
            -> Box<Future<Item=(), Error=io::Error>> {

        use MsgType::*;
        use ServerStep::*;
        let (sink, stream) = framed.split();

        let nodes1 = self.nodes.clone();
        let nodes2 = self.nodes.clone();

        println!("starting, n = {}, t = {}", self.n, self.t);

        let udp_stream = stream.filter_map(move |(src, msg)| {
            let nodes = nodes1.borrow();
            // TODO check msg.body to match
            // check validity
            if !nodes.contains(&src) {
                return None;
            }
            // check round number
            if msg.ty != Init && msg.round != self.round {
                return None;
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
                        return Some(stream::iter(make_msgs(&nodes,
                            Msg{ round: self.round, ty: Echo, body: msg.body })));
                    }
                },
                Two => {
                    if self.ok_to_ready() {
                        self.step = Three;
                        println!("into Three");
                        return Some(stream::iter(make_msgs(&nodes,
                            Msg{ round: self.round, ty: Ready, body: msg.body })));
                    }
                },
                Three => {
                    self.step = One;
                    self.round = 0;
                    println!("Decided {:?}", msg.body);
                },
            };
            None
        }).flatten();

        let rx_stream = rx.then(move |res| {
            let nodes = nodes2.borrow();
            match res {
                // TODO randomise round
                Ok(body) => Ok(stream::iter((make_msgs(&nodes, Msg{round: 9, ty: Init, body: body})))),
                Err(()) => Err(io::Error::from_raw_os_error(-1)),
            }
        }).flatten();

        let f = sink.send_all(
            udp_stream.select(rx_stream)
        );
        Box::new(f.then(|_| Ok(())))
    }

    fn ok_to_echo(&self) -> bool {
        if self.init_count > 0 || self.ok_to_ready() {
            return true;
        }     
        return false;
    }

    fn ok_to_ready(&self) -> bool {
        if (self.n + self.t) / 2 >= self.echo_count || (self.t + 1) >= self.ready_count {
            return true;
        }     
        return false;
    }
}

pub fn strings_to_addrs(ss: Vec<String>) -> Vec<SocketAddr> {
    ss.iter().map(|s| {
        s.parse().unwrap()
    }).collect()
}

fn make_msgs(addrs: &Vec<SocketAddr>, msg: Msg) -> Vec<Result<(SocketAddr, Msg), io::Error>> {
    let mut v = vec![];
    for addr in addrs {
        v.push(Ok((*addr, msg.clone())));
    };
    v
}


/*
#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct Msg {
    ty: u32,
    body: String,
}

pub struct MsgCodec {
    state: CodecState,
}

enum CodecState {
    Id,
    Len { id: RequestId }, // alias for u64
    Payload { id: RequestId, len: u64 },
}

impl MsgCodec {
    fn new() -> Self {
        MsgCodec {
            state: CodecState::Id,
        }
    }
}

impl Codec for MsgCodec {
    type In = (RequestId, Msg);
    type Out = (RequestId, Msg);

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
        use self::CodecState::*;
        let len_u64 = mem::size_of::<u64>();
        loop {
            match self.state {
                Id if buf.len() < len_u64 => {
                    info!("buf len is {}, waiting for {} at id", buf.len(), len_u64);
                    return Ok(None);
                }
                Id => {
                    let mut id_buf = buf.drain_to(len_u64);
                    let id = Cursor::new(&mut id_buf).read_u64::<BigEndian>()?;
                    info!("parsed id = {} from {:?}", id, id_buf.as_slice());
                    self.state = Len { id: id };
                }
                Len { .. } if buf.len() < len_u64 => {
                    info!("buf len is {}, waiting for {} at len", buf.len(), len_u64);
                    return Ok(None);
                }
                Len { id } => {
                    let mut len_buf = buf.drain_to(len_u64);
                    let len = Cursor::new(&mut len_buf).read_u64::<BigEndian>()?;
                    info!("parsed len = {} from {:?}", len, len_buf.as_slice());
                    self.state = Payload { id: id, len: len };
                }
                Payload { len, .. } if buf.len() < len as usize => {
                    info!("buf len is {}, waiting for {}", buf.len(), len);
                    return Ok(None);
                }
                Payload { id, len } => {
                    let payload = buf.drain_to(len as usize);
                    let res = bc::deserialize_from(&mut Cursor::new(payload), SizeLimit::Infinite)
                        .map_err(|deserialize_err| io::Error::new(io::ErrorKind::Other, deserialize_err))?;

                    self.state = Id;

                    info!("decoded payload: {}", len);
                    return Ok(Some((id, res)))
                }
            }
        }
    }

    fn encode(&mut self, (id, msg): Self::Out, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_u64::<BigEndian>(id)?;
        buf.write_u64::<BigEndian>(bc::serialized_size(&msg))?;
        bc::serialize_into(buf, &msg, SizeLimit::Infinite)
            .map_err(|serialize_err| io::Error::new(io::ErrorKind::Other, serialize_err))?;
        info!("encoded msg {:?} into {:?}", msg, buf);
        Ok(())
    }
}

*/