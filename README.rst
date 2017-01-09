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

Modular
    TFS is highly modular, and is divided into various independent components.
    A significant amount of TFS's components are simply disk drivers without
    any semantic information. This makes TFS relatively straight-forward to
    implement.
Full-disk compression
    TFS is the first file system to incorporate complete full-disk compression
    through a scheme we call RACC (random-access cluster compression). This
    means that every cluster is compressed only affecting performance slightly.
    It is estimated that you get 60-120% more usable space.
O(1) snapshots
    TFS allows full or partial disk revertable and writable snapshots in
    constant-time without clones or the alike.
Copy-on-write semantics
    Similarly to Btrfs and ZFS, TFS uses CoW semantics, meaning that no cluster
    is ever overwritten directly, but instead it is copied and written to a new
    cluster.
Guaranteed atomicity
    The system will never enter an inconsistent state (unless there is hardware
    failure), meaning that unexpected power-off at worst results in a 4 KiB
    space leak. The system is never damaged by such shutdowns. The space can be
    recovered easily by running the GC command.
Improved caching
    TFS puts a lot of effort into caching the disk to speed up disk accesses.
    It uses machine learning to learn patterns and predict future uses to
    reduce the number of cache misses.
Concurrent
    TFS contains very few locks and aims to be as suitable for multithreaded
    systems as possible. It makes use of multiple truly concurrent structures
    to manage the data, and scales linearly by the number of cores.
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

FAQ
---

Why do you use SPECK as the default cipher?
    SPECK is a relatively young cipher, yet it has been subject to a lot of
    (ineffective) cryptanalysis, so it is quite secure, but more importantly is
    that it has really good performance and a simple implementation.
    Portability is an important part of the TFS design, and truely portable AES
    implementations without side-channel attacks is harder than many think
    (particularly, there are issues with `SubBytes` in most portable
    implementations). SPECK does not have this issue, and can be implemented
    portably with minimal effort.
How similar is TFS and ZFS?
    Not that similar, actually. The share many of the basic ideas, but
    otherwise they are essentially unconnected.
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
    anywhere near the performance of ZMicro, because disk operations are
    inheritly slow, and when put in perspective, the performance of the
    compression is really unimportant.

Resources on design
-------------------

I've written a number of pieces on the design of TFS:

- [SeaHash: Explained](http://ticki.github.io/blog/seahash-explained/). This
  describes the default checksum algorithm designed for TFS.
- [On Random-Access Compression](http://ticki.github.io/blog/ternary-as-a-prediction-residue-code/).
  This post describes the algorithm used for random-access compression.
- [Ternary as a prediction residue code](http://ticki.github.io/blog/ternary-as-a-prediction-residue-code/). The
  use of this is related to creating a good adaptive (headerless) entropy
  compressor.
- [How LZ4 works](http://ticki.github.io/blog/how-lz4-works/). This describes
  how the LZ4 compression algorithm works.

Specification
-------------

The full specification can be found in `specification.tex`. To render it, install `pdflatex`, and run

.. code:: bash
    pdflatex --shell-escape specification.tex

Then open the file named `specification.pdf`.
