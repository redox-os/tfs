# Endianness

Every integer is stored in little-endian, unless otherwise specified.

# Typed pointers

We use the term "typed pointers" to talk about pointers which are represented as the quotient of the physical address over the size of the object they point to.

All pointers are 0-based unless otherwise specified.

# Nil pointers

We purposefully avoid the phrase "null pointers" because a null pointer in TFS might be valid. Instead, we make use of "nil pointers", i.e. the highest 128-bit integer value or 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF.

# Restriction on size

The address space can never exceed _2^112_.

# Disk drivers and virtual disks

A disk driver modifies the I/O stream in accordance to some rules and forwards it to the next virtual disk.

Formally, the set of virtual disks can be seen as a category under disk drivers as morphisms. This section will describe these morphisms such that the TFS I/O stack is defined as the function composite of all. They're mentioned in the same order as they're morphism composited.

### Introducer

The introduction sequence is a 32 byte long sequence, put in the start of the storage medium (e.g. partition). The first 64-bit contains the magic number, 0x5446532064617461. The rest of the introducing bytes are unused in current standard.

The introducer will shift all reads or writes 32 bytes up, such that the introduction sequence is never changed.

## Page disk driver

A page disk driver reads the virtual disk in terms of pages, 4 KiB chunks. Furthermore, it is able to allocate and deallocate pages.

Page disk drivers can redefine the page size.

## Page allocator

The page allocator uses the first 16 bytes as a typed pointer to the head of the block freelist, i.e. the first page in a sequence of linked pages. The pointer to the next data block is defined as the first 128 bits in the data block. The list is terminated by the nil pointer.

Initially, every data block on the disk is linked together and the head pointer is set to 0x0.

## Encryption

Encryption is done in accordance to the user-chosen algorithm defined by the first page of the virtual disk. A zero page defines unencrypted disk. Setting the last byte in the page to 0xFF defines an implementation defined encryption method.

# Checksums

We introduce a new kind of pointer, _checked page pointers_. These are simply typed pointers to a page. Each paged block is assigned a 2 byte checksum stored in the 16 highest bits in of the checked page pointer.

## Checksum algorithm

The checksum is byte-based reverse Rabin-Karp rolling hash with modulo _m=0xFFFF_ and base _p=0x7FED_. In particular, if the hash of a sequence is _H_, and a byte _b_ is appended, the updated hash is _H' = (pH + b) % m_.

## Error correction and redundancy

The disk is split into data blocks. The error correction driver injects 1 KiB of metadata before each of the data block. The metadata block contains a sequence of 64 leader pointers. These are typed pointers to leader data blocks. Whenever a bit is flipped on the data block, the same bit is flipped on the leader block(s).

The leader blocks uses the metadata to store its child blocks, i.e. the blocks having it as leader.

Leader blocks is the XOR of its children.

If not all 64 pointers are used, it can be terminated by the nil pointer. The values following this terminator can be used as the implementation wants.

Errors can be corrected by applying Gauss elimination to solve this linear system of equations.

To preserve atomicity, the first 128 bits of the virtual disk is spend on keeping track of the data block with pending. This is a typed pointer to the block which is was updated. When the leader update is finished, this is set to the nil pointer. If the system crashed, the leader must be recalculated in terms of its children.
