# Protokoll für Richtigkeit, Ordnung, Transport, Optimierung, Kanalunabhängigkeit, Ortsunabhängigkeit, Latenzminimierung und balancierte Lastverteilung (Das PROTOKOLL) v2

Unfortunately we were not able to create a proper implementation.
In fact, we can't even submit a running implementation.
Due to other university work, unavailability of members, illness and partial lack of internet, we didn't have as much
time in the end as we expected to have.
Thus, we focused on producing a complete specification and weren't able to produce a proper implementation.

We hope that this can be excused, the specification should be feature-complete and give a detailed overview about
how the implementation should have looked like.
For example the flow chart state machine should indicate, that we did think about our implementation throughout
writing the spec, and such it should be straightforward to implement the stated concepts.
Both members of the group want to actually use the tool we write during protocol design later on for local file synchronization.
That's why we focus a lot on performance, optimizations and want to perfectionize everything.

Again, we're sorry that we weren't able to provide a proper implementation for this assignment and how that the
detailed specification is enough for this assignment.

# How to run

You can't. There is nothing present in the main function.

# Issues during Implementation

Time and illness.

# Spec Changes during Implementation

We did switch from our original intend of using flags for packet distinction and FIN handling
to using a simple discriminator-byte instead.

# Spec as incomplete and partially incorrect / obsolete notes

For the actual spec look at [specification.pdf](specification.pdf) or its raw
form [specification.md](specification.md).
The full specification can be built with pandoc with the following command line:

```sh
pandoc specification-v2.md -o specification-v2.pdf
```

