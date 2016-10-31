<img src="./icon.png" height="500" />

# TFS: A next-gen file system

TFS (abbrv. Ticki's File System or The File System) is a file system loosly based on ZFS, but with improved features. TFS includes support for O(1) snapshotting, caching, error correction, compression, and more.

It was originally designed for Redox OS, but will work on Linux and BSD too.

In contrary to ZFS, which TFS takes a lot inspiration from, TFS fixes certain flaws ZFS had:

- TFS is not monolithic: It is based on a stack of disk drivers, each making up disjoint components. This makes it easy to maintain and implement.
- TFS is disk-centric: TFS puts a lot of effort into handling as much of the raw bytes without semantic information as possible. This gives great performance and simple code.
- TFS puts greater emphasis on memory caching: This can improve performance significantly.
- TFS has much improved file monitoring: inotify and friends are all hacks. TFS provides a integrated solution to file monitoring.
- TFS puts even more emphasis on compression: TFS has built in random-access compression.

WIP

## Terminology

- **Disk driver**: A particular component in the Disk I/O pipeline, which modifies the data stream. A lot of the functionality of TFS is implemented as disk drivers.
- **Cache driver**: The disk driver which has to job to memory cache the active parts of the disk.
- **Post-cache driver**: A disk driver, which is below the cache (the I/O stream is produces will not go through the cache driver).
- **Pre-cache driver**: A disk driver, whose stream is cached (pass through the cache driver).
- **Page**: A 4096 byte block. These are the smallest atomic unit in TFS (known as cluster size or allocation units in other file systems).
- **Superpage**: A page storing the state of the file system. The superpage cannot be restored.
- **Page pointer**: A 16 byte integer defining the address of a particular page.
- **Cache line**: An entry containing a cached page in the cache structure.
- **Page freelist**: A linked list of pages of entries considered free and ready for use.
- **File**: The unit of storage exposed to the user.
- **Snapshot**: A restorable state of the file system.
- **Zone**: A substate representing some subset of the file system.
- **Root zone**: The zone spanning the whole file system (stored in the superpage).
- **PLRU**: Pseudo-Least-Recently-Used, a particular cache page replacement policy, which replaces the oldest (least-recently used) page in the cache.
- **Data block**: The higher-level unit of storage, 128 MB.
- **Redundancy group**: A set of data blocks, such that their XOR is zero. This is used for data recovery.
- **Redundancy group leader**: A particular data block in a redundancy group, which is not used as storage for other purposes than error correction. In particular, if any bit in some data block in the redundancy group is flipped, the same bit is flipped in the redundancy group leader.
- **Mirror**: A redundancy group with only two data blocks.
- **COW semantics**: The idea that no pages are mutated directly, but rather a new page is allocated, which the content is written to, and the old page is thrown away.
- **Checksum**: A small number checking the integrity of the data.
- **Atomicity**: The idea that the file system should never enter an inconsistent (invalid) state naturally (i.e. not hardware faults).
- **wqueue**: Certain drivers cannot be entirely atomic and hence use write queues to back them up. Especially for post-cache drivers this is common.

## Disk drivers

0. Hard disk - 1b: The actual (physical) driver.
1. Introducer - 1b: Stuff to identify a TFS disk.
2. Checksums - 4096b: Integrity checking.
3. (Encryption) - 4096b: Optional encryption of the disk.
4. Caching - 4096b: Memory caching to avoid excessive reads and writes to the disk.
5. Redundancy and error correction - 128mb: Redundancy group management in order to correct hardware failures.
