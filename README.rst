.. image:: https://rawgit.com/ticki/tfs/master/icon.svg
    :alt: The TFS icon.
    :align: center

================================
TFS: Next-generation file system
================================

TFS is a modular, fast, and feature rich next-gen file system, employing
modern techniques for high performance, high space efficiency, and high
scalability.

TFS was created out of the need for a modern file system for Redox OS, as a
replacement for ZFS, which proved to be slow to implement because of its
monolithic design.

TFS is inspired by the ideas behind ZFS, but at the same time it aims to be
modular and easier to implement.

TFS is not related to the file system of the same name by *terminalcloud*.

*While many components are complete, TFS itself is not ready for use.*

.. image:: https://img.shields.io/github/license/ticki/tfs.svg
    :target: https://en.wikipedia.org/wiki/MIT_License
    :alt: MIT/X11 permissive license.
.. image:: https://img.shields.io/github/stars/ticki/tfs.svg?style=social&label=Star
    :alt: GitHub Stars

Design goals
------------

TFS is designed with the following goals in mind:

Concurrent
    TFS contains very few locks and aims to be as suitable for multithreaded
    systems as possible. It makes use of multiple truly concurrent structures
    to manage the data, and scales linearly by the number of cores. **This is
    perhaps the most important feature of TFS.**
Asynchronous
    TFS is asynchronous: operations can happen independently; writes and
    reads from the disk need not block.
Full-disk compression
    TFS is the first file system to incorporate complete full-disk compression
    through a scheme we call RACC (random-access cluster compression). This
    means that every cluster is compressed only affecting performance slightly.
    It is estimated that you get 60-120% more usable space.
Revision history
    TFS stores a revision history of every file without imposing extra
    overhead. This means that you can revert any file into an earlier version,
    backing up the system automatically and without imposed overhead from
    copying.
Data integrity
    TFS, like ZFS, stores full checksums of the file (not just metadata), and
    on top of that, it is done in the parent block. That means that almost all
    data corruption will be detected upon read.
Copy-on-write semantics
    Similarly to Btrfs and ZFS, TFS uses CoW semantics, meaning that no cluster
    is ever overwritten directly, but instead it is copied and written to a new
    cluster.
O(1) recursive copies
    Like some other file systems, TFS can do recursive copies in constant time,
    but there is an unique addition: TFS doesn't copy even after it is mutated.
    How? It maintains segments of the file individually, such that only the
    updated segment needs copying.
Guaranteed atomicity
    The system will never enter an inconsistent state (unless there is hardware
    failure), meaning that unexpected power-off won't ever damage the system.
Improved caching
    TFS puts a lot of effort into caching the disk to speed up disk accesses.
    It uses machine learning to learn patterns and predict future uses to
    reduce the number of cache misses. TFS also compresses the in-memory cache,
    reducing the amount of memory needed.
Better file monitoring
    CoW is very suitable for high-performance, scalable file monitoring, but
    unfortunately only few file systems incorporate that. TFS is one of those.
All memory safe
    TFS uses only components written in Rust. As such, memory unsafety is only
    possible in code marked `unsafe`, which is checked extra carefully.
Full coverage testing
    TFS aims to be full coverage with respect to testing. This gives relatively
    strong guarantees on correctness by instantly revealing large classes of
    bugs.
SSD friendly
    TFS tries to avoid the write limitation in SSD by repositioning dead sectors.
Improved garbage collection
    TFS uses Bloom filters for space-efficient and fast garbage collection. TFS
    allows the FS garbage collector to run in the background without blocking
    the rest of the file system.

FAQ
---

Why do you use SPECK as the default cipher?
    SPECK is a relatively young cipher, yet it has been subject to a lot of
    (ineffective) cryptanalysis, so it is relatively secure. It has really
    good performance and a simple implementation. Portability is an important
    part of the TFS design, and truly portable AES implementations without
    side-channel attacks is harder than many think (particularly, there are
    issues with `SubBytes` in most portable implementations). SPECK does not
    have this issue, and can thus be securely implemented portably with minimal
    effort.
How similar is TFS and ZFS?
    Not that similar, actually. They share many of the basic ideas, but
    otherwise they are essentially unconnected. But ZFS' design has shaped TFS'
    a lot.
Is TFS Redox-only?
    No, and it was never planned to be Redox-only.
How does whole-disk compression work?
    Whole-disk compression is -- to my knowledge -- exclusive to TFS. It works
    by collecting as many "pages" (virtual data blocks) into a "cluster"
    (allocation unit). By doing this, the pages can be read by simply
    decompressing the respective cluster.
Why is ZMicro so slow? Will it affect the performance of TFS?
    The reason ZMicro is so slow is because it works on a bit level, giving
    excellent compression ratio on the cost of performance. This horribly slow
    performance is paid back by the reduced number of writes. In fact, more
    than 50% of the allocations with ZMicro will only write one sector, as
    opposed to 3. Secondly, no matter how fast your disk is, it will not get
    anywhere near the performance of ZMicro because disk operations are
    inherently slow, and when put in perspective, the performance of the
    compression is really unimportant.
Extendible hashing or B+ trees?
    Neither. TFS uses a combination of trees and hash tables: Nested hash
    tables, a form of hash trees. The idea is that instead of reallocating, a
    new subtable is created in the bucket.

Resources on design
-------------------

I've written a number of pieces on the design of TFS:

- `SeaHash: Explained <http://ticki.github.io/blog/seahash-explained/>`_. This
  describes the default checksum algorithm designed for TFS.
- `On Random-Access Compression <http://ticki.github.io/blog/on-random-access-compression/>`_.
  This post describes the algorithm used for random-access compression.
- `Ternary as a prediction residue code <http://ticki.github.io/blog/ternary-as-a-prediction-residue-code/>`_. The
  use of this is related to creating a good adaptive (headerless) entropy
  compressor.
- `How LZ4 works <http://ticki.github.io/blog/how-lz4-works/>`_. This describes
  how the LZ4 compression algorithm works.
- `Collision Resolution with Nested Hash Tables <https://ticki.github.io/blog/collision-resolution-with-nested-hash-tables/>`_.
  This describes the method of nested hash tables we use for the directory
  structure.
- `An Atomic Hash Table <https://ticki.github.io/blog/an-atomic-hash-table/>`_.
  This describes the concurrent, in-memory hash table/key-value store.

Specification
-------------

The full specification can be found in `specification.tex`. To render it, install `pdflatex`, and run

.. code:: bash

    pdflatex --shell-escape specification.tex

Then open the file named `specification.pdf`.
