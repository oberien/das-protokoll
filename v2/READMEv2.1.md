# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung (Das PROTOKOLL) v2

Implemented:

* General structure
* In-memory BlockDB
* Convert directory into blockdb
* Convert blockdb to directory (write tree beginning from root into directory)
* Upload of whole directory tree from Client to Server
* BlockDB Crypto

Missing:

* Proper RTT calculation / usage
* Persisted BlockDB (currently only in-memory)
* Resumption of uploads
* Control Protocol Crypto
* Use full MTU (currently transfer chunks are chunkid + 1450 bytes payload for simplification)
* Merkle-Tree hints
* Proper status-update sending (currently it's sent after every received chunk)
* Proper diffing / conflict resolution (currently server accepts every root, overwriting its own state)
* Multi-client handling (multi-client not possible without proper diffing)
* Inotify / diffing (shouldn't be more than 20 lines with `inotify` and `diff` crate )
* Verification that blockid is valid hash of block
* Proper error handling (currently crash on any unexpected message)
* Resending of RootUpdate (currently only BlockRequests are resent)

Notes:

* from_id in RootUpdate not required for implementation, but required for replay protection

Possible Modifications:

* Send byte-wise bitmap instead of chunk-wise one, which allows easier resumption with different MTU
* Keep block-type in its own block instead of identifying it from root (makes debugging / inspection easier and protocol more idempotent)
* Don't CBOR encode Leaf blocks, because their length already dictates their length (no length prefix needed)
* Don't use CBOR at all (smaller encoding, because CBOR encodes field-names as well)

# How to run

Install Rust+Cargo: https://www.rust-lang.org/

Inside the `scsync` directory, invoke `cargo build`.
This compiles a binary to `./target/debug/scsync`.
This is our `scsync` client, test this.
`scsync --help` shows the help of the program.

A pre-built binary built for linux is included.

Logging can be enabled by passing the environment variable
`RUST_LOG=csync=trace`, other log levels aren't used.

On successful termination of everything, both the server and client will hang, doing nothing.
The server will print `done` before hanging.

## Example execution

Setup files:

```sh
mkdir source
touch source/foo
mkdir source/bar
dd if=/dev/zero of=source/bar/baz bs=1024 count=1024
mkdir destination
```

Start Server:

```sh
RUST_BACKTRACE=1 cargo run -- -c 100000000 -f bar -s
```

Start Client:

```sh
RUST_BACKTRACE=1 cargo run -- -c 10000000 -f foo
```

# Issues during Implementation

We needed to reimplement large parts of v1 of das PROTOKOLL, because we applied a different project structure.
The current state of async in rust sucks, we want async/await and a proper tokio.
We wanted to have a single-threaded task executor, but unfortunately the tokio-timer crate doesn't work with it.
Thus, we needed to switch to a multithreaded executor after having implemented everything.

# Spec Changes during Implementation

We didn't modify the specification document, but we did decide for some changes during implementation.
The first change is that in addition to the RootUpdate we discovered that the RootUpdateResponse must also be encrypted.
This is due to the RootUpdateResponse containing the key of the block in its blockref.
For obvious reasons that key must only ever be transmitted in encrypted form.  
Another change is to include the length of the block in the BlockUpdateResponse.
That allows preallocation of storage and removes the initial setup packet of the transfer protocol.
