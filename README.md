tokio-p2p
========
A toy project demonstrating how to build a peer-to-peer network using [tokio](https://tokio.rs).
It has a bootstrapping mechanism for discovering initial peers but also does gossiping periodically to discover more peers.
A broadcast function exist to send a message to all known peers.

Installing
----------
`cargo build`

Running
-------
`cargo run -- -a 127.0.0.1:12345 -b hello`
