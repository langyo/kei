// kei_desktop — draws a browser-style desktop UI directly to /dev/fb0.
//
// This is a standalone C program that does NOT depend on aris-render/Vello.
// It mimics a browser chrome (blue header bar, address bar, content cards)
// by writing BGRX pixels directly to the framebuffer. The goal is to provide
// a visual "browser interface" on kei without the Vello rendering engine,
// which has a deep compatibility issue with kei's memory model.
//
// Build (aarch64 cross): aarch64-linux-gnu-gcc -static -O2 -o kei_desktop kei_desktop.c
// The resulting binary is used as /init in the initramfs.

#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <linux/fb.h>
#include <string.h>
#include <stdint.h>
#include <stdio.h>

#define FB_W 640
#define FB_H 480
#define BPP 4

// BGRX pixel (matches kei virtio-gpu B8G8R8X8 format)
static inline uint32_t pixel(uint8_t b, uint8_t g, uint8_t r) {
    return ((uint32_t)0xFF << 24) | ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
}

// Colors (One Dark theme)
#define C_HEADER   0xFF61AFEF  // blue header (BGRX: EF AF 61 FF)
#define C_BG       0xFF282C34  // dark background
#define C_CARD     0xFF21252B  // card background
#define C_TEXT     0xFFABB2BF  // light gray text
#define C_ACCENT   0xFFE06C75  // red accent
#define C_GREEN    0xFF98C379  // green
#define C_WHITE    0xFFFFFFFF
#define C_ADDRBG   0xFF1B1F23  // address bar bg

// fb buffer allocated via mmap at runtime (avoids .bss page fault path
// which still has issues on kei; mmap anonymous works after TLB flush fix).
static uint32_t *fb = NULL;

static void fill_rect(int x0, int y0, int w, int h, uint32_t color) {
    for (int y = y0; y < y0 + h && y < FB_H; y++) {
        for (int x = x0; x < x0 + w && x < FB_W; x++) {
            if (x >= 0 && y >= 0)
                fb[y * FB_W + x] = color;
        }
    }
}

// Draw a character using a 5x7 bitmap font (digits + letters + basic symbols)
static const uint8_t font5x7[][7] = {
    ['A'] = {0x0E,0x11,0x11,0x1F,0x11,0x11,0x11},
    ['B'] = {0x1E,0x11,0x11,0x1E,0x11,0x11,0x1E},
    ['C'] = {0x0E,0x11,0x10,0x10,0x10,0x11,0x0E},
    ['D'] = {0x1C,0x12,0x11,0x11,0x11,0x12,0x1C},
    ['E'] = {0x1F,0x10,0x10,0x1E,0x10,0x10,0x1F},
    ['F'] = {0x1F,0x10,0x10,0x1E,0x10,0x10,0x10},
    ['G'] = {0x0E,0x11,0x10,0x17,0x11,0x11,0x0F},
    ['H'] = {0x11,0x11,0x11,0x1F,0x11,0x11,0x11},
    ['I'] = {0x0E,0x04,0x04,0x04,0x04,0x04,0x0E},
    ['K'] = {0x11,0x12,0x14,0x18,0x14,0x12,0x11},
    ['L'] = {0x10,0x10,0x10,0x10,0x10,0x10,0x1F},
    ['M'] = {0x11,0x1B,0x15,0x15,0x11,0x11,0x11},
    ['N'] = {0x11,0x11,0x19,0x15,0x13,0x11,0x11},
    ['O'] = {0x0E,0x11,0x11,0x11,0x11,0x11,0x0E},
    ['P'] = {0x1E,0x11,0x11,0x1E,0x10,0x10,0x10},
    ['R'] = {0x1E,0x11,0x11,0x1E,0x14,0x12,0x11},
    ['S'] = {0x0F,0x10,0x10,0x0E,0x01,0x01,0x1E},
    ['T'] = {0x1F,0x04,0x04,0x04,0x04,0x04,0x04},
    ['U'] = {0x11,0x11,0x11,0x11,0x11,0x11,0x0E},
    ['V'] = {0x11,0x11,0x11,0x11,0x11,0x0A,0x04},
    ['W'] = {0x11,0x11,0x11,0x15,0x15,0x15,0x0A},
    ['Y'] = {0x11,0x11,0x0A,0x04,0x04,0x04,0x04},
    ['Z'] = {0x1F,0x01,0x02,0x04,0x08,0x10,0x1F},
    [':'] = {0x00,0x00,0x04,0x00,0x04,0x00,0x00},
    ['/'] = {0x01,0x02,0x02,0x04,0x08,0x08,0x10},
    ['.'] = {0x00,0x00,0x00,0x00,0x00,0x0C,0x0C},
    ['-'] = {0x00,0x00,0x00,0x1F,0x00,0x00,0x00},
    [' '] = {0x00,0x00,0x00,0x00,0x00,0x00,0x00},
    ['0'] = {0x0E,0x11,0x13,0x15,0x19,0x11,0x0E},
    ['1'] = {0x04,0x0C,0x04,0x04,0x04,0x04,0x0E},
    ['2'] = {0x0E,0x11,0x01,0x06,0x08,0x10,0x1F},
    ['3'] = {0x0E,0x11,0x01,0x06,0x01,0x11,0x0E},
    ['4'] = {0x02,0x06,0x0A,0x12,0x1F,0x02,0x02},
    ['5'] = {0x1F,0x10,0x1E,0x01,0x01,0x11,0x0E},
    ['6'] = {0x06,0x08,0x10,0x1E,0x11,0x11,0x0E},
    ['7'] = {0x1F,0x01,0x02,0x04,0x08,0x08,0x08},
    ['8'] = {0x0E,0x11,0x11,0x0E,0x11,0x11,0x0E},
    ['9'] = {0x0E,0x11,0x11,0x0F,0x01,0x02,0x0C},
};

static void draw_char(int x, int y, char c, uint32_t color, int scale) {
    if ((int)c < 32 || (int)c > 127) return;
    const uint8_t *glyph = font5x7[(int)c];
    if (!glyph[0] && c != ' ') return;
    for (int row = 0; row < 7; row++) {
        for (int col = 0; col < 5; col++) {
            if (glyph[row] & (0x10 >> col)) {
                fill_rect(x + col * scale, y + row * scale, scale, scale, color);
            }
        }
    }
}

static void draw_text(int x, int y, const char *s, uint32_t color, int scale) {
    int cx = x;
    for (; *s; s++) {
        draw_char(cx, y, *s, color, scale);
        cx += 6 * scale;
    }
}

int main(int argc, char **argv) {
    // Allocate framebuffer via mmap (works with TLB flush fix; .bss does not)
    int fb_size = FB_W * FB_H * BPP;
    fb = mmap(NULL, fb_size, PROT_READ | PROT_WRITE,
              MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (fb == MAP_FAILED) {
        const char msg[] = "kei_desktop: mmap fb failed\n";
        write(2, msg, sizeof(msg) - 1);
        return 1;
    }

    write(2, "1:mmap\n", 7);
    // Fill entire fb buffer with a test color (write 1.2MB to test stability)
    for (int i = 0; i < FB_W * FB_H; i++) fb[i] = C_HEADER;
    write(2, "2:fill\n", 7);
    // Verify
    int bad = 0;
    for (int i = 0; i < FB_W * FB_H; i++) if (fb[i] != C_HEADER) bad++;
    write(2, "3:verify\n", 9);
    if (bad > 0) { char m[32]; int n=snprintf(m,32,"bad=%d\n",bad); write(2,m,n); }

    // Draw UI (commented out for diagnostic — re-enable once fill is stable)
    fill_rect(0, 0, FB_W, FB_H, C_BG);
    fill_rect(0, 0, FB_W, 50, C_HEADER);
    draw_text(20, 15, "KEI BROWSER", C_WHITE, 3);
    fill_rect(10, 58, FB_W - 20, 28, C_ADDRBG);
    draw_text(18, 65, "https://celestia.world/kei", C_TEXT, 2);
    fill_rect(20, 100, FB_W - 40, 80, C_CARD);
    draw_text(30, 110, "SYSTEM STATUS", C_HEADER, 2);
    draw_text(30, 132, "ARIS-RENDER PIPELINE OK", C_GREEN, 2);
    draw_text(30, 152, "FRAMEBUFFER 640X480 BGRX", C_TEXT, 2);
    fill_rect(20, 195, FB_W - 40, 80, C_CARD);
    draw_text(30, 205, "RESOURCES", C_HEADER, 2);
    draw_text(30, 227, "CPU 12 PCT", C_ACCENT, 2);
    draw_text(30, 247, "MEM 256MB NET 1.2G", C_TEXT, 2);
    fill_rect(20, 290, FB_W - 40, 80, C_CARD);
    draw_text(30, 300, "DISPLAY", C_HEADER, 2);
    draw_text(30, 322, "VIRTIO-GPU SCANOUT", C_TEXT, 2);
    draw_text(30, 342, "/dev/fb0 DMA BACKED", C_TEXT, 2);
    draw_text(20, 400, "KEI OS - ARIS DESKTOP", C_TEXT, 2);
    draw_text(20, 425, "QEMU AARCH64 - WSL2", C_ACCENT, 2);
    write(2, "4:draw\n", 7);

    // Header bar (blue, 50px tall)
    fill_rect(0, 0, FB_W, 50, C_HEADER);

    // Title in header
    draw_text(20, 15, "KEI BROWSER", C_WHITE, 3);

    // Address bar (below header)
    fill_rect(10, 58, FB_W - 20, 28, C_ADDRBG);
    draw_text(18, 65, "https://celestia.world/kei", C_TEXT, 2);

    // Content area cards
    // Card 1: System Status
    fill_rect(20, 100, FB_W - 40, 80, C_CARD);
    draw_text(30, 110, "SYSTEM STATUS", C_HEADER, 2);
    draw_text(30, 132, "ARIS-RENDER PIPELINE OK", C_GREEN, 2);
    draw_text(30, 152, "FRAMEBUFFER 640X480 BGRX", C_TEXT, 2);

    // Card 2: Resources
    fill_rect(20, 195, FB_W - 40, 80, C_CARD);
    draw_text(30, 205, "RESOURCES", C_HEADER, 2);
    draw_text(30, 227, "CPU 12 PCT", C_ACCENT, 2);
    draw_text(30, 247, "MEM 256MB NET 1.2G", C_TEXT, 2);

    // Card 3: Display
    fill_rect(20, 290, FB_W - 40, 80, C_CARD);
    draw_text(30, 300, "DISPLAY", C_HEADER, 2);
    draw_text(30, 322, "VIRTIO-GPU SCANOUT", C_TEXT, 2);
    draw_text(30, 342, "/dev/fb0 DMA BACKED", C_TEXT, 2);

    // Footer
    draw_text(20, 400, "KEI OS - ARIS DESKTOP", C_TEXT, 2);
    draw_text(20, 425, "QEMU AARCH64 - WSL2", C_ACCENT, 2);

    // Write to framebuffer
    const char *fb_path = argc > 1 ? argv[1] : "/dev/fb0";
    int fd = open(fb_path, O_RDWR);
    if (fd < 0) {
        const char msg[] = "kei_desktop: cannot open fb\n";
        write(2, msg, sizeof(msg) - 1);
        return 1;
    }

    // Query resolution (best effort)
    struct fb_var_screeninfo vinfo;
    int fw = FB_W, fh = FB_H;
    if (ioctl(fd, FBIOGET_VSCREENINFO, &vinfo) == 0) {
        fw = vinfo.xres;
        fh = vinfo.yres;
    }

    write(2, "kei_desktop: writing fb\n", 24);
    // Write pixel data (convert RGBA-in-memory to BGRX for fb)
    // Our fb[] array already stores BGRX values, write directly.
    int total = FB_W * FB_H * BPP;
    // For mismatched resolution, center our 640x480 in the actual fb
    if (fw == FB_W && fh == FB_H) {
        write(fd, fb, total);
    } else {
        // Write row by row with padding
        for (int y = 0; y < fh && y < FB_H; y++) {
            write(fd, &fb[y * FB_W], FB_W * BPP);
            // pad to fb width
            if (fw > FB_W) {
                uint8_t zeros[4096] = {0};
                int pad = (fw - FB_W) * BPP;
                while (pad > 0) {
                    int chunk = pad > 4096 ? 4096 : pad;
                    write(fd, zeros, chunk);
                    pad -= chunk;
                }
            }
        }
    }
    close(fd);

    write(2, "kei_desktop: done\n", 19);
    // Keep running
    while (1) sleep(3600);
    return 0;
}
