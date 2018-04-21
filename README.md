# General Design

* "Registration" (server just lazily create directory if not exists): Client generates random blob (secret), sends to server. Server hashes it (solves encoding issue) → create directory of hex of hash.
* Authentication: Client sends his secret (blob), server finds corresponding directory
* 3-way-handshake (Keine Daten im ACK-Paket, da dies durch Festplattenlatenz die RTT zum Server verfälscht) → Both Server and Client have RTT information
* Chunks fortlaufend indizieren ⇢ Out-of-Order solved
* Client sends chunk after chunk
* Server tracks which chunks have been received and asks client for missing chunks every n milliseconds.
* Server creates file of size sent by client
* Server creates tracking-file: runlength bitmap of chunks
* varint encoding of numbers

Problem:

* small MTU size such that runlength number is larger than MTU

# Congestion Control

* Initially Client sends burst of packets for `RTT / m` milliseconds


