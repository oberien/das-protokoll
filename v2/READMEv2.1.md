# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung (Das PROTOKOLL) v2

Implemented:

Missing:

* Proper RTT calculation / usage
* Persisted BlockDB (currently only in-memory)
* Resumption of uploads
* Control Protocol Crypto
* Use full MTU (currently transfer chunks are chunkid + 1000 bytes payload for simplification)
* Merkle-Tree hints
* Proper status-update sending (currently it's sent after every received chunk)

# How to run

TODO

# Issues during Implementation

We needed to reimplement v1 of das PROTOKOLL, because we applied a different project structure.

TODO

# Spec Changes during Implementation

We didn't modify the specification document, but we did decide for some changes during implementation.
The first change is that in addition to the RootUpdate we discovered that the RootUpdateResponse must also be encrypted.
This is due to the RootUpdateResponse containing the key of the block in its blockref.
For obvious reasons that key must only ever be transmitted in encrypted form.  
Another change is to include the length of the block in the BlockUpdateResponse.
That allows preallocation of storage and removes the initial setup packet of the transfer protocol.

# Spec as incomplete and partially incorrect / obsolete notes

For the actual spec look at [specification.pdf](specification.pdf) or its raw
form [specification.md](specification.md).
The full specification can be built with pandoc with the following command line:

```sh
pandoc specification-v2.md -o specification-v2.pdf
```

