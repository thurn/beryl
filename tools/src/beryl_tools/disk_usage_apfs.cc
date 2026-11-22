// Read APFS private allocation and map the extents of files with shared blocks.
// The union of their physical ranges counts partially modified clones correctly.
#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <fts.h>
#include <iostream>
#include <map>
#include <string>
#include <sys/attr.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <thread>
#include <tuple>
#include <unistd.h>
#include <vector>

using Bytes = uint64_t;

struct Root {
    size_t area;
    size_t detail;
    std::string path;
};

struct Range {
    Bytes start;
    Bytes end;
};

struct Event {
    dev_t device;
    size_t detail;
    Bytes offset;
    int change;
};

struct HardLink {
    dev_t device;
    ino_t inode;
    size_t detail;
    Bytes allocated;
};

struct Usage {
    Bytes physical = 0;
    Bytes exclusive = 0;
    Bytes allocated = 0;
    Bytes unmapped = 0;
    size_t unreadable = 0;
    size_t vanished = 0;

    void add(const Usage& other) {
        physical += other.physical;
        exclusive += other.exclusive;
        allocated += other.allocated;
        unmapped += other.unmapped;
        unreadable += other.unreadable;
        vanished += other.vanished;
    }

    void print(const char* kind, size_t index) const {
        std::cout << kind << ' ' << index << ' ' << physical << ' ' << exclusive
                  << ' ' << allocated << ' ' << unmapped << ' ' << unreadable
                  << ' ' << vanished << '\n';
    }
};

struct Worker {
    std::vector<Usage> details;
    std::vector<Event> events;
    std::vector<HardLink> links;
};

struct __attribute__((packed)) PrivateReply {
    uint32_t length;
    attribute_set_t returned;
    off_t size;
};

bool all_private(const char* path, Bytes allocated) {
    struct attrlist request{};
    request.bitmapcount = ATTR_BIT_MAP_COUNT;
    request.commonattr = ATTR_CMN_RETURNED_ATTRS;
    request.forkattr = ATTR_CMNEXT_PRIVATESIZE;
    PrivateReply reply{};
    return getattrlist(path, &request, &reply, sizeof(reply),
                       FSOPT_NOFOLLOW | FSOPT_ATTR_CMN_EXTENDED | FSOPT_PACK_INVAL_ATTRS) == 0 &&
           (reply.returned.forkattr & ATTR_CMNEXT_PRIVATESIZE) &&
           reply.size >= 0 && static_cast<Bytes>(reply.size) >= allocated;
}

Bytes allocation(const struct stat& entry) {
    return static_cast<Bytes>(entry.st_blocks) * 512;
}

Bytes map_fork(const char* path, const struct stat& entry, Bytes block,
               std::vector<Range>& ranges) {
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) return 0;
    struct stat current{};
    if (fstat(fd, &current) || current.st_ino != entry.st_ino ||
        current.st_dev != entry.st_dev || current.st_size != entry.st_size ||
        current.st_blocks != entry.st_blocks) {
        close(fd);
        return 0;
    }
    auto allocated = allocation(entry);
    Bytes mapped = 0;
    off_t offset = 0;
    // APFS may preallocate real extents beyond EOF. A hole reports a negative
    // device offset and must be skipped even in a sparse image with a large EOF.
    const off_t limit = std::max<off_t>(entry.st_size, allocated) + allocated;
    while (offset < limit && mapped < allocated) {
        struct log2phys record{};
        record.l2p_devoffset = offset;
        record.l2p_contigbytes = limit - offset;
        if (fcntl(fd, F_LOG2PHYS_EXT, &record) < 0 || record.l2p_contigbytes <= 0) break;
        if (record.l2p_devoffset >= 0) {
            Bytes physical = record.l2p_devoffset;
            Bytes start = physical / block * block;
            Bytes end = (physical + record.l2p_contigbytes + block - 1) / block * block;
            ranges.push_back({start, end});
            mapped += end - start;
        }
        offset += record.l2p_contigbytes;
    }
    close(fd);
    return mapped;
}

void scan_file(const Root& root, const FTSENT& entry, Bytes block, Worker& worker,
               std::vector<Range>& ranges) {
    const auto& metadata = *entry.fts_statp;
    auto allocated = allocation(metadata);
    if (!allocated) return;

    auto& detail = worker.details[root.detail];
    const bool hard_link = entry.fts_info == FTS_F && metadata.st_nlink > 1;
    if (hard_link) {
        worker.links.push_back({metadata.st_dev, metadata.st_ino, root.detail, allocated});
    } else {
        detail.allocated += allocated;
        if (entry.fts_info != FTS_F || all_private(entry.fts_path, allocated)) {
            detail.physical += allocated;
            detail.exclusive += allocated;
            return;
        }
    }

    ranges.clear();
    Bytes mapped = map_fork(entry.fts_path, metadata, block, ranges);
    if (mapped < allocated && (metadata.st_flags & UF_COMPRESSED)) {
        // A compressed file can hold its data in the resource fork. Mapping
        // its ordinary data fork then fails even when the file holds blocks.
        std::string resource = std::string(entry.fts_path) + "/..namedfork/rsrc";
        struct stat fork{};
        if (!stat(resource.c_str(), &fork)) {
            mapped += map_fork(resource.c_str(), fork, block, ranges);
        }
    }
    if (mapped > allocated) {
        // The file's allocation changed during the walk.
        detail.unmapped += allocated;
        return;
    }
    detail.unmapped += allocated - mapped;
    for (auto range : ranges) {
        worker.events.push_back({metadata.st_dev, root.detail, range.start, 1});
        worker.events.push_back({metadata.st_dev, root.detail, range.end, -1});
    }
}

void read_error(Usage& detail, const char* path, int error) {
    if (error == ENOENT) {
        ++detail.vanished;
    } else if (++detail.unreadable <= 3) {
        std::fprintf(stderr, "disk-usage: %s: %s\n", path, std::strerror(error));
    }
}

void scan(const Root& root, Worker& worker) {
    auto& detail = worker.details[root.detail];
    struct statfs volume{};
    if (statfs(root.path.c_str(), &volume)) {
        read_error(detail, root.path.c_str(), errno);
        return;
    }
    if (std::strcmp(volume.f_fstypename, "apfs") != 0) {
        ++detail.unreadable;
        std::fprintf(stderr, "disk-usage: %s is on %s, expected APFS\n",
                     root.path.c_str(), volume.f_fstypename);
        return;
    }

    char* arguments[] = {const_cast<char*>(root.path.c_str()), nullptr};
    FTS* tree = fts_open(arguments, FTS_PHYSICAL | FTS_XDEV | FTS_NOCHDIR, nullptr);
    if (!tree) {
        read_error(detail, root.path.c_str(), errno);
        return;
    }
    std::vector<Range> ranges;
    for (;;) {
        errno = 0;
        auto* entry = fts_read(tree);
        if (!entry) {
            if (errno) read_error(detail, root.path.c_str(), errno);
            break;
        }
        switch (entry->fts_info) {
        case FTS_NS:
        case FTS_ERR:
        case FTS_DNR:
            read_error(detail, entry->fts_path, entry->fts_errno);
            break;
        case FTS_DP:
            break; // Already visited before walking the children.
        default:
            scan_file(root, *entry, volume.f_bsize, worker, ranges);
        }
    }
    fts_close(tree);
}

void update_count(std::map<size_t, int>& counts, size_t index, int change) {
    auto& count = counts[index];
    count += change;
    if (!count) counts.erase(index);
}

void sum_ranges(std::vector<Event>& events, const std::vector<size_t>& detail_areas,
                std::vector<Usage>& details, std::vector<Usage>& areas, Usage& total) {
    std::sort(events.begin(), events.end(), [](const Event& a, const Event& b) {
        return std::tie(a.device, a.offset) < std::tie(b.device, b.offset);
    });

    std::map<size_t, int> active_details, active_areas;
    dev_t device = 0;
    Bytes previous = 0;
    for (auto event : events) {
        if (device != event.device) {
            device = event.device;
            previous = event.offset;
        }
        Bytes length = event.offset - previous;
        if (length && !active_areas.empty()) {
            total.physical += length;
            for (const auto& entry : active_areas) areas[entry.first].physical += length;
            for (const auto& entry : active_details) details[entry.first].physical += length;
            if (active_areas.size() == 1) areas[active_areas.begin()->first].exclusive += length;
            if (active_details.size() == 1) details[active_details.begin()->first].exclusive += length;
        }
        update_count(active_details, event.detail, event.change);
        update_count(active_areas, detail_areas[event.detail], event.change);
        previous = event.offset;
    }
}

void sum_hard_links(std::vector<HardLink>& links, const std::vector<size_t>& detail_areas,
                    std::vector<Usage>& details, std::vector<Usage>& areas, Usage& total) {
    // A hard link adds a path but does not add even an apparent file allocation.
    // Its physical extents still participate in each location's range union.
    std::sort(links.begin(), links.end(), [&](const HardLink& a, const HardLink& b) {
        return std::tie(a.device, a.inode, detail_areas[a.detail], a.detail) <
               std::tie(b.device, b.inode, detail_areas[b.detail], b.detail);
    });
    const HardLink* last = nullptr;
    for (const auto& link : links) {
        bool new_inode = !last || link.device != last->device || link.inode != last->inode;
        if (new_inode) total.allocated += link.allocated;
        auto area = detail_areas[link.detail];
        if (new_inode || area != detail_areas[last->detail]) areas[area].allocated += link.allocated;
        if (new_inode || link.detail != last->detail) details[link.detail].allocated += link.allocated;
        last = &link;
    }
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::vector<Root> roots;
    std::vector<size_t> detail_areas;
    size_t area_count = 0;
    std::string area, detail, path;
    while (std::getline(std::cin, area, '\0') && std::getline(std::cin, detail, '\0') &&
           std::getline(std::cin, path, '\0')) {
        roots.push_back({std::stoul(area), std::stoul(detail), path});
        const auto& root = roots.back();
        area_count = std::max(area_count, root.area + 1);
        detail_areas.resize(std::max(detail_areas.size(), root.detail + 1));
        detail_areas[root.detail] = root.area;
    }

    std::atomic<size_t> next = 0;
    std::vector<Worker> workers(std::stoul(argv[1]));
    std::vector<std::thread> threads;
    for (auto& worker : workers) {
        worker.details.resize(detail_areas.size());
        threads.emplace_back([&worker, &roots, &next] {
            size_t index;
            while ((index = next++) < roots.size()) scan(roots[index], worker);
        });
    }
    for (auto& thread : threads) thread.join();

    Usage total;
    std::vector<Usage> areas(area_count), details(detail_areas.size());
    std::vector<Event> events;
    std::vector<HardLink> links;
    size_t event_count = 0;
    for (auto& worker : workers) event_count += worker.events.size();
    events.reserve(event_count);
    for (auto& worker : workers) {
        for (size_t index = 0; index < details.size(); ++index) {
            details[index].add(worker.details[index]);
        }
        events.insert(events.end(), worker.events.begin(), worker.events.end());
        std::vector<Event>().swap(worker.events);
        links.insert(links.end(), worker.links.begin(), worker.links.end());
    }
    for (size_t index = 0; index < details.size(); ++index) {
        areas[detail_areas[index]].add(details[index]);
        total.add(details[index]);
    }
    sum_hard_links(links, detail_areas, details, areas, total);
    sum_ranges(events, detail_areas, details, areas, total);

    total.print("total", 0);
    for (size_t index = 0; index < areas.size(); ++index) areas[index].print("area", index);
    for (size_t index = 0; index < details.size(); ++index) details[index].print("detail", index);
}
