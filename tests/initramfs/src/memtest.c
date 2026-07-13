// kei_memtest — memory corruption diagnostic for kei kernel.
// Allocates anonymous memory via mmap and brk, writes known patterns,
// reads them back to detect corruption (dirty pages / CoW bugs).
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int check_count = 0;
static int fail_count = 0;

static void check_pattern(const char *name, uint64_t *p, int count, uint64_t expected) {
    int bad = 0;
    for (int i = 0; i < count; i++) {
        if (p[i] != expected) {
            if (bad < 3) {
                dprintf(2, "CORRUPT %s[%d]=%#lx expected %#lx\n", name, i, p[i], expected);
            }
            bad++;
        }
    }
    check_count++;
    if (bad > 0) {
        dprintf(2, "FAIL %s: %d/%d words corrupted\n", name, bad, count);
        fail_count++;
    } else {
        dprintf(2, "OK %s: %d words clean\n", name, count);
    }
}

int main() {
    dprintf(2, "kei_memtest: starting\n");

    // Test: mmap with varying sizes to find the corruption threshold
    // Test 16, 32, 64, 128, 300 pages
    int sizes[] = {16, 32, 64, 128, 300};
    for (int si = 0; si < 5; si++) {
        int npages = sizes[si];
        size_t sz = npages * 4096;
        uint64_t *m = mmap(NULL, sz, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (m == MAP_FAILED) { dprintf(2, "mmap %d pages failed\n", npages); continue; }
        // Write unique value to each page's first word
        for (int p = 0; p < npages; p++) {
            ((uint64_t *)((char *)m + p * 4096))[0] = 0xBEEF0000 + p;
        }
        // Read back and count errors
        int bad = 0;
        for (int p = 0; p < npages; p++) {
            if (((uint64_t *)((char *)m + p * 4096))[0] != 0xBEEF0000 + p) bad++;
        }
        dprintf(2, "mmap %3d pages: %d/%d corrupted\n", npages, bad, npages);
        munmap(m, sz);
    }

    // Test: brk
    uint64_t brk1 = (uint64_t)sbrk(0);
    uint64_t brk2 = (uint64_t)sbrk(4096);
    uint64_t brk3 = (uint64_t)sbrk(0);
    dprintf(2, "brk: %#lx -> %#lx (after +4096) %#lx\n", brk1, brk2, brk3);

    dprintf(2, "kei_memtest done\n");
    // Now try drawing to fb (like kei_desktop)
    int fd = open("/dev/fb0", O_RDWR);
    if (fd >= 0) {
        dprintf(2, "fb0 opened, writing test pattern\n");
        // Simple blue + green pattern (use mmap, not .bss)
        uint32_t *fbuf = mmap(NULL, 640*480*4, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        if (fbuf == MAP_FAILED) { dprintf(2, "fbuf mmap failed\n"); close(fd); while(1) sleep(3600); }
        for (int y = 0; y < 480; y++) {
            for (int x = 0; x < 640; x++) {
                if (y < 50) fbuf[y*640+x] = 0xFFEF6140; // blue header (BGRX)
                else if (y < 250) fbuf[y*640+x] = 0xFF345C28; // dark
                else fbuf[y*640+x] = 0xFF7998C3; // green-ish
            }
        }
        write(fd, fbuf, 640*480*4);
        close(fd);
        dprintf(2, "fb write done\n");
    }

    while(1) sleep(3600);
    return fail_count > 0 ? 1 : 0;
}
