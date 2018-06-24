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

The final goal is to establish a Peer-to-Peer network, but this version still
chooses a single peer as server.

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
a Root Update Response sent to all clients as push-notification.

While the server does provide push-notifications, it is not guaranteed that they
won't be lost; they aren't acknowledged.
It is expected by clients to poll regularly to be notified of updates.

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
assumed to be lost and the packet SHOULD be retransmitted.

The control protocol consist of five different packet types.

#### Root Update

The Root Update is used to indicate a root update from a given block to a new block.
It is comprised of the blockid of the from-block and the blockref of the
new block.

The root update is used for multiple purposes.
Its main purpose is for a client to notify the server about an update in the
file system.
A root update is sent containing the from-blockid of the root to-be-updated and
the to-blockref pointing to the new root.
That message is automatically acknowledged by the next control message from the
server, which is either a Block Request, if the merkle tree contains an unknown
block, or a Root Update Response, indicating a successful (or unsuccessful, if
a different client was faster) update.  
Whenever a client wants to connect to the server, it MUST send a root update
as first packet, where the from-blockid and the to-blockid are equal and point
to the client blockdb's root.
The server MUST respond with a root update response, where the from-blockid and
the to-blockid are equal to its blockdb's root.
Additionally, the client SHOULD update its state to match the server state if
they are different.
RTT information is only required to retransmit possibly lost control packets
in a timely fashion.
The client has RTT information from the 2-way-handshake.
If the client needs to update its state, it'll respond with a Block Request, from
which the Server is able to extract RTT information.
The server only needs RTT information if it needs to request blocks and thus
send control packet requests.
This is only required if the client updates the root to a new state.
In that case the server will get RTT information from a Block Request Response
from the client after the server sent its Block Request.  
As long as no RTT information is available, a sane default for retransmissions
should be taken, like a few seconds.

The root update is also used as means to perform state polling.
The 2-way-handshake is reperformed, initiated by the client, such that the
client detects possible modifications.
Clients should poll in timely fashions depending on the use-case.

#### Root Update Response

The Root Update Response is used by the server to inform clients about a changed
root of the merkle tree.
Just as the Root Update, the Root Update Response contains the from-blockid
and to-blockref.
This packet MAY be sent to every client whenever the root changes on the server,
to inform clients about possible root changes.
This is similar to a push notification, but it is not acknowledged.
If a client receives the update, it MAY update its internal state instantaneously
to the new root (by fetching unknown blocks).
If that packet is lost, that's fine, because clients will fetch at a later point
anyway.
Thus, the push notification is optional.

#### Block Request

Block Requests are used by clients to request unknown blocks from the server, or
by the server to request unknown blocks from clients.
They contain the blockid of the requested block.
It MUST be answered with a Block Request Response, which acknowledges the
reception of the Block Request.

#### Block Request Response

Block Request Responses are used as responses to Block Requests.
They contain the blockid of the requested block and the transfer protocol's
allocated chunkids to transmit that block.
It initiates the transfer protocol for that block.
They are implicitly acknowledged by the first Status Update within the transfer
protocol.

### Transfer Protocol

The Transfer Protocol is a slight adaption of the PROTOKOLL version 1 file
transfer protocol.
First, the protocol is now bidirectional. One channel instance is created in
each direction.
Instead of a single transfer, there can be multiple, fixed-size, dynamically
allocated transfers.
Each channel has its own global incrementing ids.
Version 1 was able to determine the size of a file transfer, which allowed the
use of fixed-size chunkids.
In version 2, the number of blocks and thus transfer chunks is unknown.
Thus, instead of fixed-length chunkids, varints are used.
Additionally, while PROTOKOLL v1 includes the file name / path in its initial
packet, the name is already known from directory nodes within the merkle tree.
Thus it is not part of the packets of the adapted PROTOKOLL v1 for v2.

After a client connecting to a server, one transfer channel is created in each
direction, starting out idle with a random chunkid.
Implementations SHOULD use a secure random to generate at least a 32-bit
random start chunkid.
Due to the random starting chunkid, the bitmap will usually start out with that
number of zeroes.
Thus another change is that the bitmap in v2 starts counting from zeroes
instead of ones.
When a Block Request is received, PROTOKOLL v1 is applied and the number of
chunks required from the current chunkid is calculated.
Based on the chunkid this number can be different for same-length payloads due
to the varint encoding of the chunkid.
The current chunkid is used as start and the current chunkid plus the number
of required chunkids is used as end of the transfer for that blockid.
That chunkid range is allocated for that block and a Block Request Response
sent to the request origin with the start and end chunkids.

The transfer protocol can be performed independent from the control protocol.
If the end of one allocation is reached, the transfer protocol can continue
sending the next allocation if one exists without any interruptions.
When no further allocation exists, and the whole transfer is acknowledged and
the channel is idle again.
Any status update causes the transfer protocol to jump back and un-idle.

### Connection Management

Connection setup is established by the 2-way-handshake described in
[Root Update](#root-update).
Connection teardown can occur at any time.
It is performed with a 2-way handshake of FIN packets.
The FIN handling SHOULD be performed similar to the FIN handling discussed in
PROTOKOLL v1.
Additionally, the connection is assumed to be dead if the client hasn't polled
within 2.5 times its polling interval.
In that case, the connection is removed and a new connection needs to be established.
The only state held by the server for a connection is the current transfer's chunkid.
When the client polls the next time, the usual 2-way-handshake will be performed
again, reinitiating the transfer's chunkid with a random number if there is
actual transfer.

# Encoding

The transfer protocol encoding is performed as described in PROTOKOLL v1,
leaving out the filename.
Control message data is encoded using Concise Binary Object Representation (CBOR)
as described in [RFC 7049](https://tools.ietf.org/html/rfc7049).

Each message starts with a flags-byte, starting at the most significant bit,
followed by the message's payload.
The FIN-flag is used to tear down connections.
If the transfer-payload bit is set, the payload is a transfer chunk.
If the transfer-status bit is set, the payload is a transfer status update.
Otherwise, the payload is a control message.

```
flags {
    fin: bool,
    transfer_payload: bool,
    transfer_status: bool,
},
encoded payload...
```

The protocol is mainly sent without encryption or signatures, except for the
Root Update, which is AEAD'd as described in [Cryptography](#cryptography).
The signature is placed after the payload.

## Primitives

### Blockref

```
blockid: [u8; N], // fixed length
key: [u8; K], // fixed length
hints: List<Blockref>, // list of blockrefs, optional
```

### Block Type

The block type is an enum represented as u8.

```
DIRECTORY = 1,
FILE_META = 2,
LEAF = 3,
```

### Child

```
name: TextString, // name of file without path
metadata: Custom, // may be used in implementations, null otherwise
type: BlockType, // type of child
blockref: Blockref,
```


## Root Update

```
nonce: [u8; L], // fixed length
{
  from_blockid: [u8; N], // fixed length
  to_blockref: Blockref,
},
```

## Root Update Response

```
from_blockid: [u8; N], // fixed length
to_blockid: [u8; N], // fixed length
to_key: [u8; N], // fixed length
```

## Block Request

```
blockid: [u8; N] // fixed length
```

## Block Request Response

```
blockid: [u8; N], // fixed length
start_id: varint,
end_id: varint,
```

## Leaf Block

```
data... // untagged byte-array
```

## File Meta Block

```
parts: List<Blockref>, // list of blockrefs
```

## Directory Blocks

```
children: List<Child>,
```

# Cryptography

Cryptography is mostly handled within the blockdb.
The protocol layer contains barely any cryptography.
The main idea is that blocks and thus file content is encrypted with
authentication.
This design allows the whole network to store data, which might only be
decrypted by some peers, but not by others.
This allows a future version of this specification to handle synchronisation
among multiple different users, each with their own access permissions.

This version assumes that a single user wants to synchronize files among
multiple of their devices.
Thus a shared secret can be established.
That shared secret is used as symmetric key.

## BlockDB

Every block in the blockdb is encrypted using an idempotent self-synchronizing
cipher

* idempotent
* blockdb implicitly authenticated by blockid (Encrypt-then-MAC)

## Transmission Crypto

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

    + Variable receive windows, must be large enough to hold largest variant + largest possible control payload

# Extensibility

# Error Handling

# Future Work

Compression methods could be specified, allowing the use of multiple compression
algorithms with user configuration.

Edge case: File A represented as file meta block, File A copied as File B,
File B is a single Leaf Block. File A and File B saved separately / duplicated.
Handled by frontend.

# Out of Scope


[@inotify]: http://man7.org/linux/man-pages/man7/inotify.7.html
