TFS uses an unique form of file system compression, namely it does full-disk compression, possibly at the first file system ever.

Obviously, compressing the full disk in one piece is practically impossible, however, dividing it into smaller chunks (each of a couple of clusters) means that you can have random access.

We call our approach RACC: Random access cluster compression.

The core idea is to fit as many "pages" (virtual clusters) into a cluster, as possible. If we have N pages, which can be compressed to fit into one cluster, we can simply pass an offset into that cluster.

Depending on the size of cluster, this can be somewhat space inefficient in many modern compression algorithm, which is why it is important that the compression curve isn't steep.

In other words, the ideal compression algorithm shouldn't need a minimum base of data like a header, but should be to decode linearly. This is why adaptive algorithms are best.

# Ideas that are being considered

- Eliminate the compression flag by regularly compressing the clusters, instead of doing it on-the-go.
