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
* Server creates file of size sent by client
* Server creates tracking-file: bitmap of chunks
* Client sends chunk after chunk
* Server sends missing chunks as runlength encoding of the bitmap file
    + optimized to not include value because it's binary
* varint encoding of numbers
* Length of file in bytes is sent before client starts upload
* Size of chunk-id is automatically calculated from that: ⌈log₂(len)⌉
* End of transmission is indicated by largest chunk-id → no explicit end-message from client required
* Checksums / data integrity handled by UDP Checksum

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
    + any byte-array
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
    + Length of path is length of packet - length of varint
* Following packets are [Chunks](#chunk)

## Chunk

* Chunk ID
    + Number of Chunk starting at 0
    + Encoded as little endian in n bytes
        - n = ceil(log2(number of chunks needed))

## Example: Uploading a File

* [3-Way-Handshake](#3-way-handshake)
    + Client → Server: [Login Packet](#login-packet)
    + Server → Client: Empty Packet
    + Client → Server: [Upload Request](#upload-request)
* [Chunks](#chunk)

## Status Reporting

* RTT: moving average
* Inter packet times: moving average
* magic function `floor(x / ln(x + 1))`
* we calculate packets/sec as input for this function
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
    - assuming a worst case chunksize of 1 byte, this means that we need an MTU of at least 10 bytes to support all 64-bit file sizes (16 EiB).
* MTU Probing
* MTU must have a size of least `max(10, maxlength of file path + length of chunk-index-field)`
* Foreslashes inside filenames must be escaped to differentiate them from folders

