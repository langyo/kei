// init_idle — minimal PID 1 that just sleeps forever.
//
// The kernel draws the aris-render Windows-style desktop at boot (in the
// virtio-gpu probe's draw_desktop()), so init does not need to touch /dev/fb0.
// This avoids the slow/crash-prone fb write_at path entirely. PID 1 just
// needs to stay alive so the kernel doesn't panic on init exit.
#include <unistd.h>
int main(void) {
    for (;;) {
        sleep(3600);
    }
    return 0;
}
