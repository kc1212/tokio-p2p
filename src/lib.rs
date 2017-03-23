#![crate_type = "lib"]
#![crate_name = "tokio_p2p"]

pub mod codec;
pub mod node;

extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_timer;
extern crate uuid;
extern crate serde;
extern crate serde_json;
extern crate rand;
#[macro_use]
extern crate serde_derive;

