# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Liveübertragungen und balancierte Lastverteilung (Das PROTOKOLL)

## General Design

* "Registration" (server just lazily create directory if not exists): Client generates random blob (secret), sends to server. Server hashes it (solves encoding issue) → create directory of hex of hash.
* Authentication: Client sends his secret (blob), server finds corresponding directory
* 3-way-handshake (Keine Daten im ACK-Paket, da dies durch Festplattenlatenz die RTT zum Server verfälscht) → Both Server and Client have RTT information
* Chunks fortlaufend indizieren ⇢ Out-of-Order solved
* Client sends chunk after chunk
* Server tracks which chunks have been received and asks client for missing chunks every n milliseconds.
* Server creates file of size sent by client
* Server creates tracking-file: bitmap of chunks
* varint encoding of numbers

Problem:

* small MTU size such that runlength number is larger than MTU:
  - storage space for a varint integer in bytes `space(x) = ceil(log2(x) / 7)`
  - assuming the server status report contains no headers or anything else protocol is bounded by: `space(x) < MTU`
  - thus: `ceil(log2(x) / 7) < MTU` ; `x < 2 ^ (MTU * 7)`
  - where `x` is the RLE integer that counts the number of existing chunks at the start of the file
  - assuming a worst case chunksize of 1 byte, this means that we need an MTU of at least 10 bytes to support all 64-bit file sizes (16 EiB).

## Congestion Control

* Initially Client sends burst of packets for `RTT / m` milliseconds


