---
authors: Chris Coulter (c.coulter@gmail.com)
state: draft
---

# RFD 27 - TCP Load Balancer

## What

A Toy TCP Load Balancer utilising mTLS to authenticate clients and distribute load.

## Why

Coding challenges can give a reasonable expectation of my performance *if* employed by Teleport.

## Details

### Assumptions

- There is only one type of upstream server. This could be changed if using Application Layer Protocol Negotiation extension as part of TLS.
- Upstream server connection will not include any authntication/authorisation and is a straight tcp connection.
- Upstream server is expected to implement ping/pong functionality.
- Upstream servers are grouped and tcp addresses / ports will be hard coded.
- Authorisation for a client to a server group will be hard coded.
- The CA, Server keys + cert and client keys + cert will be precomputed using openssl for creation and signing.
- Client certificate email field will be used for authorization.

### Library
- Single threaded
    - simplicity
    - much harder to deadlock - at expense of throughput
    - could offload to threads or use async - at expense of possible deadlocks, greater complexity.

```rust
struct LoadBalancer
{
    clients         : HashMap<String, Client>, // email address, Client
    server_groups   : HashMap<u32, ServerGroup>, // server group id, ServerGroup
}
```

The main structure will contain a hashmap of clients and server groups.
Server Groups are an arbitary number of servers that are grouped for security aurhorisation reasons.
Clients can only connect to one server group, this will be hardcoded as part of the configuration code.
The Clients key will be their email address stored as a string.

#### Client Structure
```rust
struct Client
{
    email                : String,
    connections          : Vec<Connection>,
    cxn_time             : i64,
    cxn_cnt              : usize,
    allowed_server_group : u32,
}
```

This struct will house all data required to handle all connections (downstream and upstream) on a per client basis in the connections list.
The client email address will be used for identification and authorisation.
This will hard coded as part of configuration on the server side as a means to quickly implement the load balancer.


```rust
enum ConnState
{
    INIT,
    UP_CONNECTED,
    OKAY,
    UP_DISCONNECT,
    UP_TIMEOUT,
    DOWN_DISCONNECT,
    DOWN_TIMEOUT,
    DOWN_ENC_ERR,
}
```

The state of both connections will be managed by a finite state machine.
Any state after OKAY will result in the whole connection being dropped, the client will need to retry that connection.

These states could also be used to aid the HealthChecker in marking a node as unhealthy (in the case of UP_DISCONNECTED or UP_TIMEOUT).

The DOWN_ENC_ERR is a catchall for any and all rustls::Stream errors.

```rust
struct Connection
{
    down_stream         : TcpStream,
    up_stream           : TcpStream,
    tls                 : rustls::Stream, 
    conn_state          : ConnState,
    upstream_serv_group : u32
    upstream_serv_id    : u32,
}
```

The connection structure will house all TcpStreams and the rustls::Stream the id of the upstream server and the connection state enum.

#### Client Rate Limiter

The main library will contain a hashmap of clients with the connections that they store.
Each new connection will be first checked against the current count for the last 30 seconds if it is less than a pre determined limit then
the connection is placed throught the health and least connection forwarder and data is sent on accrdingly.

```rust
impl Client
{
    fn add_connection(&mut self, down_stream: TcpStream, rustls::Stream, up_id: u32, up_address: String) -> Result<(), Box<dyn std::error::Error>>
    {
        // convert current ts to i64 and divide 30 to give current 30s period
        let now : i64 = chrono::NaiveDateTime::now().as_timestamp() / 30;

        if now == self.cxn_time
        {
            if self.cxn_cnt >= CLIENT_LIMIT
            {
                return Err("Client rate limit hit".into());
            }
        }
        // cxn_time does not match start a new 30s period
        else
        {
            self.cxn_time = now;
            self.cxn_cnt  = 0;
        }

        ... // connect to upstream

        self.connections.push(Connection {...});
        self.cxn_cnt += 1;

        Ok(())
    }
}
```

#### Least Connections Forwarder

Once the client has been authenticated and authorised for a server group, the least connection forwarder can forward the client to 
an appropriate server.
The ServerGroup struct will house connection stats and addresser for each server.

Connections will be incremented and decremented as connections are created and destroyed by clients.

As such there will be an simple API that will increment or decrement the server's connection count.

```rust
struct ServerGroup
{
    id           : u32,
    server_addrs : HashMap<u32, String>,
    cxn_cntr     : HasnMap<u32, usize>,
}

impl ServerGroup
{
    fn find_min(&self) -> Option<u32>
    {
        // Not all servers have a connection yet
        if self.cxn_cntr.len() != self.server_addrs.len()
        {
            let server_id_set : HasSet<u32> = self.server_addrs.keys().collect();
            let cxn_id_set    : HasSet<u32> = self.cxn_cntr.keys().collect();

            // return the first server id that is not in the cxn_id_set but is in the server_id_set
            for id in server_id_set.difference(cxn_id_set)
            {
                return Some(id);
            }
        }

        // All servers are in the list.
        // Find the least connected server.
        let mut min_conns = usize::MAX;
        let mut min_id : Option<u32> = None;
        for (id, num_conns) in self.cnx_cntr.iter()
        {
            if min > num_conns
            {
                min_conns = num_conns;
                min_id = Some(id);
            }
        }

        min_id
    }
}

```

#### Upstream Health Check

The health checker will simply connect to an upstream server, send a ping and receive a pong message and then disconnect.

It will continue to do this indefinitely every 30 seconds (hard coded for development speed).

If the upstream server rejects the connection, does not reply to the ping  within 1 second or disconnects while the connection is active, the upstream server is marked unhealthy.

If the upstream completes a full connect - ping - disconnect cycle, it is again marked healthy.

```rust
enum UpstreamState
{
    HEALTHY,
    UNHEALTHY,
}
```

```rust
enum PingState
{
    IDLE,
    PING_SENT(i64),
    PONG_RECEIVED(i64),
}
```

```rust
struct HealthChecker
{
    up_stream       : TcpStream,
    ping_state      : PingState,
    UpstreamState   : UpstreamState,
    last_update     : chrono::NaiveDateTime,
}
```

```rust
struct ServerGroup
{
    ...
    server_health   : HashMap<u32, HealthChecker>,
}
```


### Server

The server will listen on port 8443 for client connections, using std::net::TcpListener to listen connections from clients.

Each connection will be then authenticated by the rusttls server instance.

Once authenticated, the authorisation process will occur as listed below.


#### Security

The rustls crate will be utilised as for all cryptogaphic functionality.
- Since rustls has *NOT* had a security audit yet, this code should definitely *NOT* be used in production.
- I found rustls to have a better API and docs (from my research) than native-tls.

The x509-parser crate will be used for extracting fields from client certificates provided to the server during the TLS handshake.

TLS v1.3 with ECDHE will be used exclusively as it newer, has better performance on initial connection and gives perfect forward secrecy out of the box on all cipher suites.

#### Cipher Suites

The following cipher suites will be:
    - TLS_AES_256_GCM_SHA384
    - TLS_AES_128_GCM_SHA256

These cipher suites are supported and enabled by Linux, Apple via OpenSSL/BoringSSL, Windows TLS and rustls.

##### Authentication
A certificate authority will be created via openssl.
The server and all client private keys will be signed by the CA.

Since both client and server will be able to authenticate their respective chains of trust, authentication can occur.

As all certificates will be precreated - a bash script will be created which will create:
    - A CA.
    - Server private key and cert that is signed by the CA.
    - N clients private key and cert (with client auth extensions and a specified email address) that is signed by the CA.
    - Each client will have use the certificates email fields as a unique identifier.
    - All private keys (CA, server, clients) will generated using the elliptic curve - secp256k1.

##### Authorisation
Rustls presents the certificate chain in DER format.
The client enmail field will be extracted from the client certificate via the x509-parser crate.
There will be a hard coded lookup table with all client email addresses that will be configured per client.

From there the library can lookup client information and return allows server group, and the most healthy least connected upstream server, available to that client.


