Caching is pretty important for the performance of any file system, and TFS is no exception.

Caching is usually only done when reading, and most file systems don't have write buffering (write caching) by default. This is because it can hurt consistency, as write caching may reorder the way that the data is being written, resulting in the file system passing through an inconsistent state.

Therefore it is important to have a strict model of ordering, when implementing write caching. TFS achieves consistency by enforcing dependencies such that one sector may first be flushed to the disk when another sector is.

In order to lift the burden from the programmer, so that he or she does not have to construct the dependency graph, we introduce the concept of a _pipeline_, which is a chunk of regular writes, whose flush order will be preserved. The pipeline can be committed, putting them into the dependency graph, which will later be flushed.
