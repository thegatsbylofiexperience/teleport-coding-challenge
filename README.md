# Teleport Coding Challenge -- Load Balancer Edition

Hi Teleporters!

I am done. I have spent a fait bit of time on this challenge.
I've actually learnt a whole lot about TLS, as through my coding travels I have never really used it (I've used libsodium before but not SSL/TLS).

So I just want to say thanks for your time, even if I don't get the job, I've enjoyed the challenge that this has provided, learnt some stuff and 
need to think closely about how I want to code in the future. I like to use finite state machines ALOTi for network code,
when thinking about scale, I think I can say that they will not. 

Thinking about scale has opened my eyes to other problems that can be encountered.

Thank you for that.

Anyway onto the repository!

## Build Instructions

This is a rust project, so install rust (rustup.rs has the instructions).

This project has been developed in the v1.61 of rust using the stable compiler branch.

Please build using "cargo build" for a debug build and "cargo build --release" for a relase build

To run please stay in the root project directory (where this README is located).

- ./target/debug/client
- ./target/debug/load_balancer
- ./target/debug/upstream

replace debug with release for release builds.


## Executables

There are three executables:
    client
    loadbalancer
    upstream

Their configs are mostly hard coded.
The only changable things are ports and certificate configurations.

- The load balancer listens on 127.0.0.1:8443 (default).
- The load balancer has the ability to select from two CAs for server cert and ca
- The client connects to the load balancer @ 127.0.0.1:8443 (default).
- The client has the ability to select certificates and client keys from two different CAs 
- The client also has the ability to select the client certificate (first second third and fourth) 
- All executables use argparse (like python argparse) to document command line flags ->  use --help for more info

### Client 
The client is a TLS client that must use mTLS authentication when connecting to the load balancer.

Once authenticated the client will send a message to the server and wait for a response which it will print to stdout.

### Load balancer
The load balancer acts as TLS server on one end with mTLS and a TcpClient to the upstream sevrers it can connect to.

The load balancer has all the features for the level 5 challenge.
- mTLS endpoint
- least connections forwarder
- client rate limiter
- health check

### Upstream
The upstream server is a simple TCP Echoer program, it takes what ever packed it has been send and pushes them back down the same connection.

## :)
Anyway I think that is it.

Thanks again,
Chris

