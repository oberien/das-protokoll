---
title: "Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung - Das PROTOKOLL v2"
author: Jaro Fietz & Noah Bergbauer
link-citations: true
papersize: a4
documenttype: scrartcl
toc: true
header-includes: |
  \usepackage{mathtools}
  \usepackage{cleveref}
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

The blockdb is the core of the specification.
It stores files and the file tree as an efficient internal representation.
This allows certain optimisations like only needing to store equal files once,
reducing file transfer through usage of existing chunks and chunk level
encryption.  
The blockdb is an independent storage of raw blocks.
It delivers information to the frontend, allowing it to render the file-tree.
In general it's a modified version of a Merkle-Tree with use-case specific
optimisations.
Just as the frontend, the blockdb has a deliberately unspecified backend to
store blocks in.
Examples are plain files or databases with large blob storage.

The root of the blockdb is a single blockref without any hints (thus only
containing a blockid and its key).
The root MUST point to a directory block.

### Invariants

The blockdb has several invariants.
Each block is identified by its blockid, which is the hash of the encrypted block.
Thus blocks are copy on write (CoW).
The hash algorithm is configurable, but not compatible.
If it's changed, the entire blockdb must be converted accordingly on every participant.
The suggested hash algorithm is Keccak-256.  
The length of blocks is variable, blocks can be arbitrarily large.
By default each file and directory is a single block.
Partial blocks allow resumption of incomplete or aborted transfers and SHOULD
be supported by implementations to reduce network overhead.

### Compression

The blockdb MAY implement compression.
Compression must be performed on the plaintext before encryption is applied.
After encryption the output can't be distinguished from random data.
Thus, encrypted data has a very high entropy and can't be compressed well.
This results in the requirement to implement the same compression on every client.
Otherwise decrypted, but compressed data could incorrectly interpreted as
the actual plaintext, rendering the file invalid.

### Blocks

Each block is identified by its blockid, which is the hash of the encrypted data.
There are three different types of blocks.
Directory blocks contain blockrefs to child directories or files and holds their
metadata.
Leaf blocks contain actual file contents, which can be any arbitrary payload data.
File meta blocks represent a file consisting of multiple blocks.
Before the different block types are discussed in detail, blockrefs are needed.

#### Blockrefs

Blockrefs are used to identify and reference blocks and contain all information
needed to decrypt those blocks.
It consists of the blockid of the referenced block, its decryption key MAY
contain hints.
Hints indicate that a block can be created from other blocks without needing to
download the whole block if other blocks are present.
Hints are a list of blockrefs, each with an offset and a length.
If hints are provided, the referenced block can be created by concatenating
the respective decrypted subranges of the referenced blocks each from their
respective offset until their offset plus their respective length.
This can be used to prevent the download of blocks which can be created from
existing blocks.
This behaviour can be used when changes are detected within a file.
Given a file is currently represented as a single block and that file is changed
in the middle, the file can be split into three new leaf blocks referenced from
a file meta block.
The first leaf block hints at the beginning of the file's original block.
The last part hints at the end of the file's original block.
The middle part is a leaf block without any hints, because it is new, containing
the new middle content.

It is possible for any block to be referenced at multiple places.
This is useful if two files are equivalent (e.g. copied), because that file
will only be saved once.
This case is automatically handled through the use of blockids in blockrefs.

Hints MAY be provided.
If a hint is provided, it MUST be correct.
If a hint is provided, it MAY be used.
Hints SHOULD be implemented for leaf blocks to reduce the number of downloads
of blocks and to allow for efficient partial file modifications.
If hints are used to create a block, the correctness of the hints MUST be
verified by checking the hash of the final encrypted block against its blockid.
If it is not used, the block needs to be downloaded, even though it may be
possible to assemble it from existing already downloaded files.
If a hint is present, it SHOULD be forwarded to other clients.
While the above example demonstrates hints for leaf blocks, they MAY also be
implemented for file meta blocks to support very large files (e.g. 2 PB),
which changes very often with less overhead.
They MAY also be supported for directory blocks to optimize small changes in a
flat file tree (e.g. 1 million files in a single directory).

#### Directory Blocks

Directory blocks are a list of blockrefs to the directory's children.
Each child MUST be annotated with its block type.
Metadata MAY be provided along each blockref, like its name, date of creation
(only available on file systems supporting it like NTFS, not e.g. on ext4), last
modified, permissions, owner and group.
Children can be other directory nodes, leaf nodes or file meta blocks.

#### Leaf Blocks

Leaf Blocks contain any arbitrary data.
They can be used as whole files, or as file parts which are concatenated through
file meta blocks.

#### File Meta Blocks

File meta blocks contain a list of blockrefs.
The concatenation of the blocks referenced by blockrefs creates the final file.

#### Example Merkle Tree

In \cref{exmerkle} an example modified merkle tree can be seen.
The root references `5891`, which is a directory node containing two blockrefs.
One blockref points to the file `ff0b`, which has the name `bar` in that directory.
The referenced directory itself contains a file named `qux`, which is the same
as `bar` of the parent.
Thus it's able to reference the same block.
The file `baz` is a file meta block, stating that the file is comprised of the
leaf nodes `110f` and `bd9d`.
Additionally, `110f` has a hint, stating that it's equal to the subrange from
byte 10 to 30 of `21bb`.
Thus, if the block `21bb` already exists, the leaf block `110f` can be created
without needing to download it.

\begin{figure}[htbp]
  \centering
  \includegraphics[width=\textwidth]{exmerkle.pdf}
  \caption{Example Merkle Tree}
  \label{exmerkle}
\end{figure}

### Bootstrap

The initial state of the blockdb of every client in the very beginning, before
any file is setup is the empty block.
Everyone spontaneously starts out in the same state, where the blockid of the
blockref of the root points to the empty block.
Once an actual directory or file is added, the root blockref's blockid will
point to that respective block.

### Conflict Resolution

Within the blockdb, there are no conflicts, because it is CoW.
The only conflict that can arise are root updates.
If two clients try to update the root at the same time, a conflict resolution
strategy MUST be applied.

This specification does not enforce an explicit conflict resolution strategy,
because it can be implementation dependent within every single client without
breakage of other implementations.

A simple conflict resolution possibility would be to let the longest chain win,
in case of a tie use the numerically lower hash of the root to break ties.
`Csync`, striving to be a minimal reference implementation of this specification,
uses a simple RwLock on the server and is thus first come first serve.
In case of a conflict, the rejected client MUST fetch the new state and
apply its changes to the new root again, then attempt another root update.

Yet another possibility is to use a 3-way-merge.
The frontend aggregates changes into transactions, that atomically update the
entire tree up to the root.
Then, the frontend has to sort out transaction aborts, which specifies the
behaviour when the root was changed while applying the transaction.
That allows an auto-merge on the directory level, the most recent version
of each file is taken.
If there is a conflict on the file-level, both versions are linked into the
directory tree (similar to how dropbox handles this case).

## Node Network Topology

Every client will have implemented the server functionality.
One client is chosen to be the server and all other clients connect to it.
This allows every client to synchronize a folder without the need of a dedicated
server that doesn't actually want to synchronize the folder, but is only used
as intermediate hop for communication.

## Protocol

Version 2 of Das PROTOKOLL uses version 1 for block transfers.
Additionally, it adds a control protocol on top.

### General Design

\begin{figure}[htbp]
  \centering
  \includegraphics[width=\textwidth]{protoseq.pdf}
  \caption{Basic Protocol Sequence}
  \label{protoseq}
\end{figure}

In \cref{protoseq} the basic protocol sequence is shown.
Bob is chosen to be the server and Alice wants to synchronize changes.
It is assumed that Alice and Bob have already established a connection.
First, her frontend detects changes and updates the blockdb, creating a new root.
A Root Update message is sent to Bob, which states that the root should be
updated from `13fd` to `53d3`.
Bob's root is at `13fd`, so the update is valid, but `53d3` is unknown.
Thus he requests that block from Alice, who answers with a block request
response allocating chunkids 0 through 2 to transfer that block.
The transfer is initiated with Bob sending his first Status Update to Alice.
After transferring that block, Bob requests two new missing blocks at the same
time from Alice, who allocates different chunkids for both blocks.
Those blocks can be transferred simultaneously, while Bob keeps traversing
the new merkle tree, requesting all unknown blocks asynchronously.
After having received all unknown blocks, Bob traverses the tree, verifying its
integrity and consistency.
Following, the root is updated, the frontend notified to update the files and
a Root Update Response sent to all clients.
The Root Update Response is acknowledged by a Root Update Response Ack.

### Control Protocol

The Control Protocol is used as meta-layer to handle updates of the blockdb.
It is rather stateless, holding only minimal connection information.
It has small packets and sends minimal data, without any contents.
The actual block transfer, and thus file transfer, is performed with the
Transfer Protocol.
The Control Protocol can be used at the same time the Transfer Protocol is
synchronizing files.
Thus, most of the bandwidth is assumed to be occupied by the Transfer Protocol.
Control packets are assumed to occur rarely compared to transfer packets, and
they are assumed not to be too relevant.
As long as a transfer is in progress, losses in the control protocol aren't
relevant, because the main purpose of the control protocol is to initiate
further transfers.
Due to these arguments, the Control Protocol can have a simple structure.

The Control Protocol is similar to a simple version of TCP.
Packets are acknowledged by proper responses to the according package.
If a control packet hasn't been acknowledged after $1.5 \cdot RTT$, it is
assumed to be lost and the packet MUST be retransmitted.

The control protocol consist of five different packet types.

#### Root Update

The Root Update is used to indicate a root update from a given block to a new block.
It is comprised of the blockid of the from-block and the blockref of the
new block.

The root update is used for multiple purposes.
Whenever a client wants to connect to the server, it MUST send a root update
as first packet, where the from-blockid and the to-blockid are equal and point
to the client blockdb's root.
The server MUST respond with a root update, where the from-blockid and the
to-blockid are equal to its blockdb's root.
Then the client SHOULD update its state to match the server state if they are different.  
The root update is also used to 


#### Root Update Response

#### Root Update Response Ack

#### Block Request

#### Block Request Response

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

    + blockrefs
        - NB changing the key of a blockref changes the plaintext
        - however modifications of the key (and the ciphertext if the cipher is CCA secure) produce random changes in plaintext

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

Compression methods could be specified, allowing the use of multiple compression
algorithms with user configuration.

Edge case: File A represented as file meta block, File A copied as File B,
File B is a single Leaf Block. File A and File B saved separately / duplicated.
Not a problem for FUSE, because copy can be recognized and file meta block
referenced muiltiple times.

# Out of Scope


[@inotify]: http://man7.org/linux/man-pages/man7/inotify.7.html
