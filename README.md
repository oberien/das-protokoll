# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung (Das PROTOKOLL)

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


## Open Questions

* Error messages to Client
    + Handle `unwrap`s
* Which files does the server have?
* How to handle a client updating the same file twice at the same time?

## Out of Scope (for now)

* File changes during upload
* File partially uploaded, connection aborts, file changes, new connection, file upload continues
* Congestion Control
