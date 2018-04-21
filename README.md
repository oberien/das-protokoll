# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Liveübertragungen und balancierte Lastverteilung (Das PROTOKOLL)

## General Design

* "Registration" (server just lazily create directory if not exists): Client generates random blob (secret), sends to server. Server hashes it (solves encoding issue) → create directory of hex of hash.
* Authentication: Client sends his secret (blob), server finds corresponding directory
* 3-way-handshake (Keine Daten im ACK-Paket, da dies durch Festplattenlatenz die RTT zum Server verfälscht) → Both Server and Client have RTT information
* Chunks fortlaufend indizieren → Out-of-Order solved
* Client sends chunk after chunk
* Server tracks which chunks have been received and asks client for missing chunks every now and again.
* Server creates file of size sent by client
* Server creates tracking-file: bitmap of chunks
* Server sends missing chunks as runlength encoding of the bitmap file (optimized to not include value because it's binary)
* varint encoding of numbers
* Length of file in bytes is sent before client starts upload
* Size of chunk-id is automatically calculated from that: ⌈log₂(len)⌉
* End of transmission is indicated by largest chunk-id → no explicit end-message from client required
* Checksums / data integrity handled by UDP Checksum
* RTT: moving average
* Packet Rate via inter-packet-time
* Status Report Timing:
    - gegeben:
        + avg paketrate p
        + logarithmische skalierungsfunktion für feedbackintervall l(x)
        + zeit seit dem letzten statusreport z
        + interpacket time i
    - in dem moment wenn wir ein status paket schicken: deadline für das nächste status update := l(p*z) * i + 1/2 rtt

## 3-Way-Handshake

* Client → Server: Client Token
    - Server must answer before doing anything else to not influence RTT
* Server → Client: Empty Packet
    - Calc RTT on Client
* Client → Server: Empty Packet
    - Calc RTT on Server

## Initiation Packet

* 1 byte Tag
* Additional data based on Tag

## Tags

Client → Server:

* `0`: Upload File
    - Varint length of file
    - Path / Filename
        + Rust: Server must remove leading `/` before using `PathBuf::push`

## status reporting

* we keep a moving average of inter packet times
* magic function `floor(x / ln(x + 1))`
* we calculate packets/sec as input for this function
* it outputs the status interval (number of packets until we send a status update)

## Example: Sending a File

* 3-Way-Handshake
* Client → Server:


## Congestion Control

* ?? Initially Client sends burst of packets for `RTT / m` milliseconds ??
* probably: slow start like tcp (but mb a bit faster?)

## Problems:

* small MTU size such that runlength number is larger than MTU:
    - storage space for a varint integer in bytes `space(x) = ceil(log2(x) / 7)`
    - assuming the server status report contains no headers or anything else protocol is bounded by: `space(x) < MTU`
    - thus: `ceil(log2(x) / 7) < MTU` ; `x < 2 ^ (MTU * 7)`
    - where `x` is the RLE integer that counts the number of existing chunks at the start of the file
    - assuming a worst case chunksize of 1 byte, this means that we need an MTU of at least 10 bytes to support all 64-bit file sizes (16 EiB).
* MTU Probing
* MTU must have a size of least `max(10, maxlength of file path)`
* Foreslashes inside filenames must be escaped to differentiate them from folders

