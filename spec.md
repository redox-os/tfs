# Endianness

Every integer is stored in little-endian, unless otherwise specified.

# Typed pointers

We use the term "typed pointers" to talk about pointers which are represented as the quotient of the physical address over the size of the object they point to.

All pointers are 0-based unless otherwise specified.

# Disk drivers and virtual disks

A disk driver modifies the I/O stream in accordance to some rules and forwards it to the next virtual disk.

Formally, the set of virtual disks can be seen as a category under disk drivers as morphisms. This section will describe these morphisms such that the TFS I/O stack is defined as the function composite of all. They're mentioned in the same order as they're morphism composited.

## Introducer

The introduction sequence is a 32 byte long sequence, put in the start of the storage medium (e.g. partition). The first 64-bit contains the magic number, 0x5446532064617461. The rest of the introducing bytes are unused in current standard.

The introducer will shift all reads or writes 32 bytes up, such that the introduction sequence is never changed.

## Checksum

The checksum driver considers the disk in chunks of 16 KiB. Each chunk is assigned a 64-bit checksum.

For every 2048 chunk, a 16 KiB chunk containing the checksums of the next 2048 chunks, stored sequentially from lower to higher, is injected.

The first 128 bits of the virtual disk is a typed pointer to the dirty chunk (a chunk whose checksum has not been updated yet). Before a chunk is written, this number must be set to the chunk's number, in order to make sure the state can be restored if it crashed. When the checksum is written this number is set to 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF. On startup, the number is read and the respective chunk's checksum is updated.

### Checksum algorithm

The checksum is byte-based reverse Rabin-Karp rolling hash with modulo _m=0xFFFFFFFFFFFFFFFF_ and base _p=0xFFFFFFFFFFFFFFC5_. In particular, if the hash of a sequence is _H_, and a byte _b_ is appended, the updated hash is _H' = (pH + b) % m_.

## Encryption

Encryption is done in accordance to the user-chosen algorithm defined by the first 64 bytes in the virtual disk. All zero bytes defines unencrypted disk. First byte 1 and rest zero defines twofish.

## Page allocator

Pages are of size 4 KiB.

The page allocator driver uses the first 16 bytes as a typed pointer to the head of the page freelist, i.e. the first page in a sequence of linked pages. The list is terminated by pointer 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF.

Initially, every page on the disk is linked together and the head pointer is set to 0x0.

## Error correction and redundancy

The disk is split into 128 KiB data blocks. The error correction driver injects 1 KiB of metadata before each of the data block. The metadata block contains a sequence of leader pointers. These are typed pointers to leader data blocks. Whenever a bit is flipped on the data block, the same bit is flipped on the leader block(s).

The leader blocks uses the metadata to store its child blocks, i.e. the blocks having it as leader.

Leader blocks is the XOR of its children.

Errors can be corrected by applying Gauss elimination to solve this linear system of equations.

To preserve atomicity, the first 128 bits of the virtual disk is spend on keeping track of the data block with pending. This is set to 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF when the leaders are updated. If the system crashed, the leader must be recalculated in terms of its children.
