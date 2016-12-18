type LineNumber = usize;

struct Line {
    sector: disk::Sector,
    data: Box<[u8]>,
    dirty: bool,
    flush_dependencies: Vec<LineNumber>,
}

impl Line {
    fn reset(&mut self) {
        // The cache line starts out as clean...
        self.dirty = false;
        // ...and hence has no dependencies.
        self.flush_dependencies.clear();
    }
}

struct WriteQuery<'a> {
    sector: disk::Sector,
    data: &'a [u8],
}

struct Cached<D> {
    disk: D,
    cache_tracker: plru::DynamicCache,
    line_map: HashMap<disk::Sector, LineNumber>,
    lines: Vec<[Line]>,
}

impl<D: Disk> Cached<D> {
    fn alloc_line(&mut self) -> LineNumber {
        // Test if the cache is filled.
        if self.lines.len() < self.cache_tracker.len() {
            // The cache is not filled, so we don't need to replace any cache line, we can simply
            // push.
            self.lines.push(Line {
                sector: sector,
                data: vec![0; self.disk.sector_size()],
                dirty: false,
                flush_dependencies: Vec::new(),
            });

            self.lines.len() - 1
        } else {
            // Find a candidate for replacement.
            let line_number = self.cache_tracker.replace();

            // Flush it to the disk before throwing the data away.
            self.flush(line_number);

            // Remove the old sector from the cache line map.
            let line = &mut self.lines[line_number];
            self.line_map.remove(line.sector);

            // Reset the cache line.
            line.reset();

            line_number
        }
    }

    fn fetch_fresh(&mut self, sector: disk::Sector) -> Result<&mut Line, disk::Error> {
        // Allocate a new cache line.
        let line_number = self.alloc_line();
        let line = &mut self.lines[line_number];

        // Read the sector from the disk.
        self.disk.read(sector, &mut line.data)?;

        // Update the sector number.
        line.sector = sector;

        // Update the cache line map with the new line.
        self.line_map.insert(sector, line_number);
    }

    fn write(&mut self, write: WriteQuery) -> LineNumber {
        // Allocate a new cache line.
        let line_number = self.alloc_line();
        let line = &mut self.lines[line_number];

        // Copy the data into the freshly allocated cache line.
        line.data.copy_from_slice(data);

        // Update the sector number.
        line.sector = sector;

        // Update the cache line map with the new line.
        self.line_map.insert(sector, line_number);

        line
    }

    fn write_seq<I: Iterator<Item = WriteQuery>>(&mut self, writes: I) {
        // Execute the first query and store the number of the cache line in which it is written.
        let mut prev_line_number = self.write(writes.next().unwrap());
        // Execute the rest of the queries.
        for query in writes {
            // Execute the query.
            let line_number = self.write(query);
            // Push the previous query as a dependency on the write.
            self.lines[line_number].flush_dependencies.push(prev_line_number);

            // Update the previous line number.
            prev_line_number = line_number;
        }
    }

    fn fetch(&mut self, sector: disk::Sector) -> Result<&mut Line, disk::Error> {
        if let Some(line) = self.line_map.get_mut(sector) {
            self.cache_tracker.touch(line);
            Ok(&mut self.lines[line])
        } else {
            self.fetch_fresh(sector)
        }
    }

    fn flush(&mut self, line: LineNumber) -> Result<(), disk::Error> {
        let line = &mut self.lines[line];

        for i in line.flush_dependencies {
            self.flush(i)?;
        }

        if line.dirty {
            self.disk.write(line.sector, 0, &line.data)?;
            line.dirty = false;
        }
    }
}
