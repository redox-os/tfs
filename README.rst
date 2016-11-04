.. image:: icon_small.png
    :alt: The TFS icon.
    :align: center

================================
TFS: Next-generation file system
================================

TFS is a modular, fast, and feature rich next-gen file system, employing
mordern techniques for high performance, high space efficiency, and high
scalability.

TFS was created out of the need for a modern file system for Redox OS, as a
replacement for ZFS, which proved to be slow to be implement because of its
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
    TFS is the first file system to encorporate complete full-disk compression
    through a scheme we call RACC (random-access cluster compression). This
    means that every cluster is compressed only affecting performance slightly.
    It is estimated that you get 60-100% more usable space.
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
    space leak. The system is never damaged by such shutdowns.
Improved caching
    TFS puts a lot of effort into caching the disk to speed up disk accesses.
Concurrent
    TFS contains very few locks and aims to be as suitable for multithreded
    systems as possible. It makes use of multiple truely concurrent structures
    to manage the data, and scales linearly by the number of cores.
Better file monitoring
    COW is very suitable for high-performance, scalable file monitoring, but
    unfortunately only few file systems incorporate that. TFS is one of those.
All memory safe
    TFS uses only components written in Rust. As such, memory unsafety is only
    possible in code marked `unsafe`, which is checked extra carefully.
Full coverage testing
    TFS aims to be full coverage with respect to testing. This gives relatively
    strong guarantees on correctness by instantly revealing large classes of
    bugs.
