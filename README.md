# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung (Das PROTOKOLL)

# How to run

Install Rust+Cargo: https://www.rust-lang.org/

Inside the `csync` directory, invoke `cargo build`.
This compiles a binary to `./target/debug/csync`.
This is our `csync` client, test this.
`csync --help` shows the help of the program.

We include a pre-built `csync` binary, built for linux systems.

Logging for the server can be enabled by passing the environment variable
`RUST_LOG=csync=debug` where `debug` is the logging level.
Supported logging levels are `error`, `warn`, `info`, `debug` and `trace`.

Notice that the implementation uses unix-specific operations for asynchronous file IO.
Thus, it may not compile on every system.
It was tested on arch-linux and Ubuntu.

Both on a connection abort due to the triggered 10 second timeout and on a
successful termination the server will currently print the following error
message:

```
Error during receive: Timeout
Client finished with error: ()
```

This is due to the server waiting for 10 seconds after the successful
termination of the connection as explained within the specification.
The same code is used to trigger the connection abort timeout and the 10 second
timeout of the shutdown state, resulting in this incorrect error message.

# Issues during Implementation

We decided to handle multiple connections at the same time on the server side
with asynchronous IO.
This means that we should be able to easily handle the c10k problem, given
some sort of send-slowdown is added to the client (which currently pumps out
packets as fast as possible for the lack of congestion control).
Unfortunately rust's async story isn't set in stone, yet.
In fact, it is currently changing heavily with futures 0.2 being released,
futures 0.3 already being worked on, and `async` and `await` keywords being
added to the language itself.
Thus, we needed to decide on which version of which libraries to use and test
which versions of those libraries were compatible with each other and with which
futures version.

Another problem was the lack of `async` / `await` currently in the language.
Currently there is a nightly-only library which adds async / await macros, but
that only compiles on a few nightly versions and produces a lot of compiler
bugs.
Additionally, one of our members did not want to use nightly, but stay with
stable, which made that library a no-go.

Rust does not (yet) have a performant platform-independent library for file IO.
We worked around that problem by using the unix-only library `tokio-file-unix`.

The library `tokio-file-unix` did not implement support for seeking in the file,
which our protocol relies heavily upon.
With [a pull request implementing that functionality](https://github.com/Rufflewind/tokio-file-unix/pull/10)
we were able to solve this problem.

While there are bitmap implementations for rust, none of them supported
owning borrowed data.
Either they need to own the data, or they only worked on borrowed data.
We mmap the bitmap, which leaves us with owning borrowed data.
Thus we needed to implement our own bitmap (in the folder bitte-ein-bit).

The linux kernel has support for MTU probing.
Unfortunately we can't use that from within rust.
That is why we moved MTU probing to Further Work for now.

# Spec Changes during Implementation

The main selling point of the specification is the runlength encoding of the
bitmap to send missing chunks to the client.
This idea was unchanged from the very beginning.

We changed the handshake and the encoding of packets.

In the beginning our idea was to have a minimal handshake to get precise RTT
measurements both on the server and the client side.
It turned out to be easier to not implement such a handshake, but instead to
directly state everything connection-initiating in the very first packet from
the client to the server.
Thus we combined the login and command packets into a single login packet.

In the encoding we first left out some length prefixes if the prefixed value
was the last one in the packet, because reading until the end of the packet
would already reveal the length.
While this is a nice optimization, it only leaves out a few bytes and makes
manual parsing when inspecting packets more complicated.
That's why we decided to prepend every variable length data with a varint length
prefix.

Similarly, we thought about adding context-sensitive commands and data, but
decided against it, because if a single packet is viewed, it should be parsable
by a human without too much context around it.

# Spec as incomplete and partially incorrect Notes

For the actual spec look at [specification.pdf](specification.pdf) or its raw
form [specification.md](specification.md).
The full specification can be built with pandoc with the following command line:

```sh
pandoc specification.md -o specification.pdf
```

## General Design

* 3-way-handshake to get RTT information
* Support for different commands
    + Command packet with type-id
* Only one command per session
    + Otw an old chunk could reach server after new upload is initiated
    + New connection must be from different port
* Chunks indexed sequentially
    + → Out-of-Order solved
* Length of file in bytes is sent before client starts upload
* Size of chunk-id is automatically calculated from that: ⌈log₂(number of chunks)/8⌉
* Server creates file of size sent by client
* We create a bitmap of successfully transferred chunks - obv this starts out with all zeroes
    + server stores it in a tracking-file along with the actual file to support resumption
    + server regularly sends this bitmap to the client
    + client is free to maintain it any way (e.g. in memory)
* Client has a "cursor" pointing into the chunk bitmap
    + start at the beginning, keep going forward as we send out chunks
    + obviously, only send chunks that are marked as "not received"
    + whenever client receivs a status update indicating lost packets, we reset the cursor to the beginning
    + we assume packet loss iff a chunk we marked as sent _more than 3*RTT ago_ is not marked as received in a status report from the server
* Server sends missing chunks as runlength encoding of the bitmap file
    + optimized to not include value because it's binary
    + **truncated to MTU**
        - obviously, the varint run-length encoding can take up to n bytes to represent n bits in the case of a strictly alternating bitmap (010101010101...)
        - this may be larger than the MTU and thus not fit into a single packet
        - we simply truncate at MTU since we expect average cases to indicate enough packets in their report that the next one will have arrived before we run out of information
        - if it doesn't this is *still* not a problem as it will simply cause gratuitous retransmits, leading only to reduced performance
* End of transmission is indicated by largest chunk-id → no explicit end-message from client required
    + protocol is terminated by a status report from the server indicating that every chunk was received (bitmask of ones), which will **always** fit into a reasonably sized MTU due to RLE
* Checksums / data integrity handled by UDP
* Fixed, configurable MTU
* Extensible by introducing new commands

## 3-Way-Handshake

* Client collects all information beforehand
    - Filename, Size of File
    - Reduces delay during Handshake
    - → More precise RTT information
* Client → Server: [Login Packet](#login-packet)
    - Server must answer before doing anything else to not influence RTT
* Server → Client: Empty Packet
    - Calc RTT on Client
* Client → Server: [Command Packet](#command-packet)
    - Calc RTT on Server

## Login Packet

* Client Token
    + any byte-array (max size: MTU)
    + unique token, identifying the client
    + Server calculates sha256 of token
    + uses hex(hash) as directory
        - creates it if it doesn't exist

## Command Packet

* 1 byte Tag
* Additional data based on Tag

## Tags

* `0`: [Upload Request](#upload-request)

## Upload Request

* tag: `0`
* Varint length of file
* Path / Filename
    + Rust: Server must remove leading `/` before using `PathBuf::push`
    + No length given - this is simply the tail of the packet
* Following packets are [Chunks](#chunk)

* If the file is already on the server:
    + If it hasn't been uploaded completely, resume upload by sending bitmap to client
    + If it has been fully uploaded in the past, delete file and upload again

## Chunk

* Chunk ID
    + Number of Chunk starting at 0
    + Encoded as little endian in n bytes
        - n = ceil(log2(number of chunks needed) / 8)

* If chunk has been received by the Server in the past (i.e. is `1` in the bitmap), it's discarded

## Example: Uploading a File

* [3-Way-Handshake](#3-way-handshake)
    + Client → Server: [Login Packet](#login-packet)
    + Server → Client: Empty Packet
    + Client → Server: [Upload Request](#upload-request)
* [Chunks](#chunk)

## Status Reporting

* RTT: moving average
* Inter packet times: moving average
* magic function `num_packets = floor(pps / ln(pps + 1))`
* we calculate packets/sec as input for this function
    + `pps = 1 / ipt`
* it outputs the status interval (number of packets until we send a status update)

* If the number of zero-bits in the bitmap is a power of two, send a status update
    + Ensures enough status updates are sent in the end, when the number of missing chunks is low
    + Doesn't reset periodic status update counters

## Congestion Control

* ?? Initially Client sends burst of packets for `RTT / m` milliseconds ??
* probably: slow start like tcp (but mb a bit faster?)

## Connection End

* All chunks have been received
* 10s without a packet from client

## Problems:

* small MTU size such that runlength number is larger than MTU:
    - storage space for a varint integer in bytes `space(x) = ceil(log2(x) / 7)`
    - assuming the server status report contains no headers or anything else protocol is bounded by: `space(x) < MTU`
    - thus: `ceil(log2(x) / 7) < MTU` ; `x < 2 ^ (MTU * 7)`
    - where `x` is the RLE integer that counts the number of existing chunks at the start of the file
    - assuming a worst case chunksize of 1 byte, this means that we need an MTU of at least 10 bytes (plus overhead from headers etc) to support all 64-bit file sizes (16 EiB).
* MTU Probing
* MTU must have a size of least `max(10, maxlength of file path + length of chunk-index-field)`
* Foreslashes inside filenames must be escaped to differentiate them from folders


## Open Questions / TODO

* Error messages to Client
    + Handle `unwrap`s
* Which files does the server have?
* How to handle a client updating the same file twice at the same time?
* How to handle successful connection FIN?
    + What if last status update of server gets lost, so the client does not know that it's over?
* Slow Start
    + How often should a status update be sent in the beginning?
* Check usage of usize vs u64 everywhere to make sure we support u64 large files
* client: don't reset cursor to 0 on every status update - instead, skip packets that are young enough

## Out of Scope (for now)

* File changes during upload
* File partially uploaded, connection aborts, file changes, new connection, file upload continues
* Congestion Control

