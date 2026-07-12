// kei_desktop — browser-style desktop UI rendered row-by-row to /dev/fb0.
//
// Renders each row into a small (2.5KB) buffer and writes it to fb0,
// avoiding the large-mmap corruption bug (>16 pages has 56% corruption).
// Draws: blue header bar, "KEI BROWSER" title, dark content cards.
//
// Build: aarch64-linux-gnu-gcc -static -O2 -o kei_desktop kei_desktop.c
#include <fcntl.h>
#include <unistd.h>
#include <sys/mman.h>
#include <sys/ioctl.h>
#include <linux/fb.h>
#include <string.h>
#include <stdint.h>

#define FB_W 640
#define FB_H 480
#define BPP 4

// BGRX colors (One Dark theme)
#define C_HEADER 0xFF61AFEF
#define C_BG     0xFF282C34
#define C_CARD   0xFF21252B
#define C_ADDRBG 0xFF1B1F23
#define C_WHITE  0xFFFFFFFF
#define C_GREEN  0xFF98C379
#define C_TEXT   0xFFABB2BF
#define C_ACCENT 0xFFE06C75

// 5x7 bitmap font
static const uint8_t font5x7[][7] = {
    ['A'] = {0x0E,0x11,0x11,0x1F,0x11,0x11,0x11},
    ['B'] = {0x1E,0x11,0x11,0x1E,0x11,0x11,0x1E},
    ['C'] = {0x0E,0x11,0x10,0x10,0x10,0x11,0x0E},
    ['D'] = {0x1C,0x12,0x11,0x11,0x11,0x12,0x1C},
    ['E'] = {0x1F,0x10,0x10,0x1E,0x10,0x10,0x1F},
    ['G'] = {0x0E,0x11,0x10,0x17,0x11,0x11,0x0F},
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
    ['W'] = {0x11,0x11,0x11,0x15,0x15,0x15,0x0A},
    [' ']= {0,0,0,0,0,0,0},
    [':']= {0,0,0x04,0,0x04,0,0},
    ['/']= {0x01,0x02,0x02,0x04,0x08,0x08,0x10},
    ['.']= {0,0,0,0,0,0x0C,0x0C},
    ['-']= {0,0,0,0x1F,0,0,0},
    ['0']={0x0E,0x11,0x13,0x15,0x19,0x11,0x0E},
    ['1']={0x04,0x0C,0x04,0x04,0x04,0x04,0x0E},
    ['2']={0x0E,0x11,0x01,0x06,0x08,0x10,0x1F},
};

int main(int argc, char **argv) {
    // Small row buffer (within the 16-page safe zone)
    int row_bytes = FB_W * BPP;
    uint32_t *rowbuf = mmap(NULL, row_bytes, PROT_READ | PROT_WRITE,
                            MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (rowbuf == MAP_FAILED) { write(2, "mmap fail\n", 10); return 1; }

    const char *fb_path = argc > 1 ? argv[1] : "/dev/fb0";
    int fd = open(fb_path, O_RDWR);
    if (fd < 0) { write(2, "open fb fail\n", 13); return 1; }

    // Try query resolution
    struct fb_var_screeninfo vinfo;
    if (ioctl(fd, FBIOGET_VSCREENINFO, &vinfo) == 0) {
        // use actual resolution if reasonable
        if (vinfo.xres > 0 && vinfo.xres <= 1920) { /* keep FB_W */ }
    }

    write(2, "render\n", 7);
    for (int y = 0; y < FB_H; y++) {
        // Background color per region
        uint32_t bg;
        if (y < 50) bg = C_HEADER;
        else if (y >= 58 && y < 86) bg = C_ADDRBG;
        else if (y >= 100 && y < 180) bg = C_CARD;
        else if (y >= 195 && y < 275) bg = C_CARD;
        else if (y >= 290 && y < 370) bg = C_CARD;
        else bg = C_BG;

        for (int x = 0; x < FB_W; x++) rowbuf[x] = bg;

        // Draw "KEI BROWSER" title (scale 3, y=15..36)
        if (y >= 15 && y < 36) {
            const char *title = "KEI BROWSER";
            int ty = y - 15;
            int glyph_row = ty / 3;
            if (glyph_row < 7) {
                for (int ci = 0; title[ci]; ci++) {
                    unsigned char c = (unsigned char)title[ci];
                    const uint8_t *g = (c < 128) ? font5x7[c] : font5x7[' '];
                    for (int col = 0; col < 5; col++) {
                        if (g[glyph_row] & (0x10 >> col)) {
                            int px = 20 + ci * 18 + col * 3;
                            for (int dx = 0; dx < 3 && px + dx < FB_W; dx++)
                                rowbuf[px + dx] = C_WHITE;
                        }
                    }
                }
            }
        }
        // Draw address bar text (scale 2, y=65..79)
        if (y >= 65 && y < 79) {
            const char *url = "https://celestia.world/kei";
            int ty = y - 65;
            int gr = ty / 2;
            if (gr < 7) {
                for (int ci = 0; url[ci]; ci++) {
                    unsigned char c = (unsigned char)url[ci];
                    const uint8_t *g = (c < 128 && font5x7[c][0]|font5x7[c][1]|font5x7[c][2]) ? font5x7[c] : font5x7[' '];
                    for (int col = 0; col < 5; col++) {
                        if (g[gr] & (0x10 >> col)) {
                            int px = 18 + ci * 12 + col * 2;
                            for (int dx = 0; dx < 2 && px + dx < FB_W; dx++)
                                rowbuf[px + dx] = C_TEXT;
                        }
                    }
                }
            }
        }
        // Card titles and content (scale 2)
        // Card 1 (y=110..160): "SYSTEM STATUS"
        // Card 2 (y=205..255): "RESOURCES"
        // Card 3 (y=300..350): "DISPLAY"
        struct { int y0, y1; const char *text; uint32_t color; } labels[] = {
            {110, 124, "SYSTEM STATUS", C_HEADER},
            {132, 146, "ARIS-RENDER OK", C_GREEN},
            {205, 219, "RESOURCES", C_HEADER},
            {227, 241, "CPU 12 MEM 256M", C_ACCENT},
            {300, 314, "DISPLAY", C_HEADER},
            {322, 336, "VIRTIO-GPU 640X480", C_TEXT},
            {0,0,0,0}
        };
        for (int li = 0; labels[li].text; li++) {
            if (y >= labels[li].y0 && y < labels[li].y1) {
                int ty = y - labels[li].y0;
                int gr = ty / 2;
                if (gr < 7) {
                    for (int ci = 0; labels[li].text[ci]; ci++) {
                        unsigned char c = (unsigned char)labels[li].text[ci];
                        const uint8_t *g = (c < 128) ? font5x7[c] : font5x7[' '];
                        if (!g[0] && c != ' ') continue;
                        for (int col = 0; col < 5; col++) {
                            if (g[gr] & (0x10 >> col)) {
                                int px = 30 + ci * 12 + col * 2;
                                for (int dx = 0; dx < 2 && px + dx < FB_W; dx++)
                                    rowbuf[px + dx] = labels[li].color;
                            }
                        }
                    }
                }
            }
        }

        // Write row to fb via seek+write
        lseek(fd, (off_t)(y * row_bytes), SEEK_SET);
        write(fd, rowbuf, row_bytes);
    }

    write(2, "done\n", 5);
    close(fd);
    while (1) sleep(3600);
    return 0;
}
