---
title: "Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung - Das PROTOKOLL v2"
author: Jaro Fietz & Noah Bergbauer
link-citations: true
papersize: a4
documenttype: scrartcl
toc: true
header-includes: |
  \usepackage{mathtools}
  \DeclarePairedDelimiter{\ceil}{\lceil}{\rceil}
  \DeclarePairedDelimiter{\floor}{\lfloor}{\rfloor}
---

# Abstract

This document defines version 2 of the Protokoll für Richtigkeit, Ordnung,
Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit,
Latenzminimierung und balancierte Lastverteilung, short Das PROTOKOLL.
Das PROTOKOLL is a UDP-based protocol optimized for file synchronisation between peers.
It is designed for a single-user multiple-clients scenario and interacts nicely
with both short-range networks and long fat pipes.
It features interruptible, resumable, secure, parallel synchronization of files
of any size.

# Introduction

Most modern file upload servers and services use a HTTP-based protocol for
uploading files.
HTTP is based on TCP, which has its advantages like automatic congestion
control and reordering, and thus less complexity.
But these advantages are also the reason why TCP is not optimized for file upload.
For example if a packet is lost, the application won't get any following packet
until that packet is retransmitted and received.
For file transfer this is undesired behaviour, because following data chunks
do not depend on previous chunks and can be written to disk at the chunk's
corresponding position.
Das PROTOKOLL tries to eliminate these disadvantages of TCP by choosing UDP as
underlying transport protocol.

Together with this specification comes a reference implementation of the
proposed protocol called `csync`.
Whenever implementation decisions are described, the decision of the reference
implementation is discussed.

## Terminology

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL
NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED",  "MAY", and
"OPTIONAL" in this document are to be interpreted as described in
[RFC 2119](https://tools.ietf.org/html/rfc2119).

## Terms and Definitions

### User Datagram Protocol (UDP)

The User Datagram Protocol is an OSI layer 4 (transport layer) protocol
[specified by the IETF](https://www.ietf.org/rfc/rfc768.txt).

### UDP Flow

The UDP flow for Das PROTOKOLL is defined for the server by the 2-tuple of the
client's IP and port.
For the client the UDP flow is defined as the 2-tuple of the server's IP and port.

### Maximum Segment Size (MSS)

While MSS is a term defined by TCP, this specification uses MSS similarly for UDP.
The maximum segment size as used in this specification defines the maximum
number of data octets within a UDP packet.
The MSS is calculated with $MTU - packet\_overhead$ where the $packet\_overhead$
for UDP is 40 bytes.
Common values are usually 1460 for the internet and 65496 for localhost
connections of linux systems.

### Round-Trip Time (RTT)

The time between sending a packet and receiving an answer to that packet.

### Inter-Packet Time (IPT)

The time between reception of two packets in the same UDP flow.

### `inotify`

On Linux based systems the inotify provides a mechanism for monitoring
file system events [@inotify].

### FUSE

Filesystem in Userspace is an interface to allow userspace programs to create
custom file systems, handling all reads, writes and events themselves.

### WebDAV

WebDAV is an extension to HTTP allowing file operations.

### Samba

Samba is a network protocol for file transfers.

### NFS

Network File System specifies a protocol for communication in a distributed
network storage system.

# Design

This specification describes three main different aspects, before combining them
into the final protocol.
First, the frontend is what is creating the actual files.
It takes the information from the blockdb to assemble files.
Second, the blockdb is the internal representation of blocks of files.
Third, the node network topology will be discussed.

## Frontend

The frontend is used to generate the actual file tree and file content.
It uses the information stored in the blockdb to assemble files.
It is informed about changes in the blockdb, which it MUST propagate to the
actual files.
Additionally, it MUST detect changes of files, translating them into blockdb
changes.

The actual file-backend of the frontend are deliberately unspecified to allow
for use-case implementation specific optimizations.
Examples for different file-backends are plain files with `inotify`, FUSE,
a webdav / HTTP API or existing network file storages like Samba or NFS.

The reference implementation uses plain files with `inotify`.

## BlockDB

* blockdb
    + independent storage of raw blocks
    + used to construct actual file-tree (frontend)
    + modified Merkle-Tree
    + abstract backend, e.g.
        - plain files
        - sqlite
        - …
    + invariants:
        - block identified by blockid, aka hash(block), thus is CoW
        - hash algorithm configurable but not compatible - if you wanna change it, you have to convert your entire db on every participant
        - variable block size, by default each file is one block and each dir is one block
        - blocks can be arbitrarily large
        - can store partial blocks! (e.g. incomplete/aborted transfers)
    + MAY use compression! lots of low hanging fruit here in terms of space savings!
    + different types of blocks:
        - leaf blocks
            * actual file contents
            * contain arbitrary payload data
        - file meta blocks
            * a list of blockrefs
            * the file is a concatenation of the blockref's payloads
            * cycles of file meta blocks not easily possible due to merkle tree structure
        - directory blocks
            * files / folders and one blockref each
            * metadata: Name, lastmodified, size
    + blockref = (blockid, key, hints)
        - NB changing the key of a blockref changes the plaintext
        - however modifications of the key (and the ciphertext if the cipher is CCA secure) produce random changes in plaintext
        - hints = Vec<(blockref, offset, length)>
            * "this block consists of these parts of other blocks"
            * prevent redownload of known subblocks
            * important when subdividing file blocks (e.g. change this part here in the middle: separate into 3 leaf blocks: first hints at beginning of original block, last at end, middle is new -> no redownload of first and last leaf block)
            * optional (can be empty or ignored), but can be used for optimizations
            * if set, it MUST be correct
            * correctness MUST be verified by blockid
            * MAY be used
            * SHOULD be forwarded
            * MAY be implemented for file meta blocks (2 PB file which gets changed often) and directory blocks (directory with 1e6 files)
            * SHOULD be implemented for leaf blocks
* random thought: zfs module possible; control front- and backend; use snapshots as blockdb

#### Example Merkle Tree

### bootstrap

folder always starts out empty; empty block hash is known, thus no bootstrap is required - everyone spontaneously starts out in the same state


### conflict resolution

* block db: there are no conflicts
* root updates: longest chain wins, numerically lower hash to break ties
    + Simplification for implementation (server-client): Just use an RwLock on server, first come first serve
* bulk work done in the frontend
    + frontend has to aggregate its changes into "transactions" that atomically update the entire tree up to the root
    + frontend has to sort out "transaction aborts", aka you made a new root but it was rejected
    + we basically have a 3way diff at this point
    + can auto-merge on directory level: simply take the most recent version of each file
    + can't auto-merge files -> link both versions into the directory tree, just like dropbox
    

## Node Network Topology

One client is chosen to be the server and all other clients connect to it.

## Protocol

### basic protocol graph

### control protocol

* similar to simple TCP
    + Not often, not too relevant
    + open transfers can still produce enough traffic to fill pipe
    + TODO

### transfer protocol

basically PROTOKOLL v1 with a few changes:

* bidirectional (one channel instance in each direction)
* instead of one transfer we have
    + multiple
    + fixed-size
    + dynamically allocated
    + transfers
* global, incrementing ids
* to deal with unknown number of chunks, ids are varints
    + Variable receive windows, must be large enough to hold largest variant + largest possible control payload
* channel starts out idle
* receiver requests a block
* sender allocates a range of chunk ids for the block transfer
* keep sending until we sent it all, then connection is idle again
* naturally, any status report causes us to jump back and un-idle
* Control Payload
    + can open new transfers
* Data Payload
    + within a transfer

### connection management

* connection setup: simple handshake (TODO: really needed anymore?)
* connection teardown can happen at any time, just a handshake of fin packets

## State-Machine

TODO: finish graph

```flow
st=>start: Start
e=>end: End
handshake=>operation: Handshake
checkstatus=>operation: Check transfer cursor
statuswait=>operation: Wait for status update
send=>operation: Send chunk

workcond=>condition: Cursor at end?

st->handshake->checkstatus->workcond
workcond(no, bottom)->send(left)->checkstatus
workcond(yes)->statuswait(right)->checkstatus
```


# Encoding

### packet format

General:

```
flags {
    fin: bool,
    transfer payload: bool,
    transfer status: bool,
    
},
data
```

(compare exchange)


Root Update:

* AEAD

```
from blockid: [u8; N], // fixed length
to blockid: [u8; N], // fixed length
nonce: [u8; L], // fixed length
TODO
```

Root Update Response:

```
from blockid: [u8; N], // fixed length
to blockid: [u8; N], // fixed length
Ok / Nope
```

Block Request:

```
blockid: [u8; N] // fixed length
```

Block Request Response:

```
blockid: [u8; N] // fixed length
ids A to B reserved
```

Blocks:

TODO

# Cryptography

* design goal: sophisticated/fancy crypto in later versions only
    + including fine grained multi user permissions
* idempotent

#### File system Crypto

* v1: 1 folder/root = 1 user = 1 symmetric key, end of the story
* blockdb implicitly authenticated by blockid (Encrypt-then-MAC)

#### Transmission Crypto

* No Handshake
* blocks already encrypted
* control protocol mostly unencrypted
* Only Root Update encrypted, because it contains the key
    + AEAD
    + GCM with random nonce
    + nonce sent in same packet
* knowing a blockid entitles you to its contents
    + Guessing blockids not a problem, because attacker can't decrypt
* to support subslicing, each block is encrypted with a self-synchronizing stream cipher
* key of block = hash(plaintext) (possible attacks?)
    + same file will result in same blockref (with a random key it won't)
    + no duplicated data even if file exists multiple times

## Threat Model

* attacker is not the user, i.e. does not know the secret master key
* attacker can eavesdrop on all network communications (Eve)
* attacker can send/modify network traffic arbitrarily (Mallory)
* confidentiality: encryption
    + block contents are encrypted (provably secure, todo verify)
    + transfers are transferring encrypted data
    + side channel: block lengths
    + side channel: block update frequency (roots/directory blocks trivially distinguishable)
    + root updates are AEAD'd under the master key
* authentication
    + block contents unforgeable, have to match the hash
    + merkle tree property: having a legit root confirms everything below
    + root updates being aead ensures roots are legit
    + real danger here: leaf block contents are completely arbitrary, you can serialize a root in there if you find a way to control the root ptr
* denial of service
    + trivial; withhold all traffic
    + defending against attacker that can spoof/inject but not drop: future work, right now trivial by just sending FINs
* resource exhaustion
    + synflood -> TODO need smth like syncookies to mitigate
* Replay
    + irrelevant, known blocks discarded
    + can only result in DoS, which is possible anyway
    + TODO
* Spoofed attacker
    + probably irrelevant, same as replay
    + TODO
* Can we be used as DoS amplifier?
    + TODO
* ddos
    + transfers bound to handshake
* 1 user means "inside job" attacks (access someone else's private folder, resource exhaustion attacks after auth, etc) don't apply
* instead, each user has its own seperate storage (block db) on the server

# Minimum MSS

# Extensibility

# Error Handling

# Future Work

# Out of Scope


[@inotify]: http://man7.org/linux/man-pages/man7/inotify.7.html
