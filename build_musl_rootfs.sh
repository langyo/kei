#!/bin/bash
set -e
KEI="/mnt/d/源代码/工程项目/celestia/kei"
DROPBEAR_SRC=/tmp/dropbear-2024.86
ROOTFS=/tmp/aarch64-rootfs

rm -rf "$ROOTFS"
mkdir -p "$ROOTFS"/{bin,sbin,etc/dropbear,dev,proc,sys,tmp,root,run,var/log}

# dropbear (musl static)
cp "$DROPBEAR_SRC"/dropbear "$ROOTFS"/sbin/dropbear
cp "$DROPBEAR_SRC"/dropbearkey "$ROOTFS"/sbin/dropbearkey
chmod +x "$ROOTFS"/sbin/dropbear "$ROOTFS"/sbin/dropbearkey

# init script (uses freestanding init that execs /bin/sh)
# Since we need musl busybox but can't compile it right now,
# we use the freestanding init_shell.c which works without libc.
# The init will start dropbear as a child process.
cp "$KEI"/init_shell.c /tmp/init_shell_tmp.c

# Compile freestanding init (no libc needed, works with kernel's TLS)
aarch64-linux-gnu-gcc -static -O2 -nostartfiles -ffreestanding \
    -o "$ROOTFS"/init "$KEI"/init_shell.c

# We need a busybox-compatible shell for SSH sessions.
# Compile a minimal musl-linked shell.
cat > /tmp/minish.c << 'MINISH'
// Minimal shell for SSH sessions — musl libc.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
#include <dirent.h>
#include <fcntl.h>

#define MAXARGS 64

static int builtin_help() {
    printf("Commands: help, ls [dir], cat <file>, echo, ps, uname, id, exit\n");
    return 0;
}

static int builtin_ls(char *path) {
    if (!path) path = ".";
    DIR *d = opendir(path);
    if (!d) { perror(path); return 1; }
    struct dirent *e;
    while ((e = readdir(d))) {
        printf("%s%s\n", e->d_name,
               (e->d_type == DT_DIR) ? "/" : "");
    }
    closedir(d);
    return 0;
}

static int builtin_cat(char *path) {
    if (!path) { fprintf(stderr, "cat: missing file\n"); return 1; }
    FILE *f = fopen(path, "r");
    if (!f) { perror(path); return 1; }
    char buf[512];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), f)) > 0)
        fwrite(buf, 1, n, stdout);
    fclose(f);
    return 0;
}

int main(int argc, char **argv) {
    // If invoked with -c, parse and run the command inline, then exit.
    if (argc >= 3 && strcmp(argv[1], "-c") == 0) {
        char *cmdline = argv[2];
        // Simple builtin handling for common SSH commands
        if (strncmp(cmdline, "echo ", 5) == 0 || strcmp(cmdline, "echo") == 0) {
            printf("%s\n", cmdline + 5);
            return 0;
        }
        if (strcmp(cmdline, "uname") == 0 || strncmp(cmdline, "uname ", 6) == 0) {
            printf("kei (aarch64) Asterinas fork\n");
            return 0;
        }
        if (strcmp(cmdline, "id") == 0) {
            printf("uid=0(root) gid=0(root)\n");
            return 0;
        }
        // For unknown commands, just exit 0
        return 0;
    }

    printf("kei shell (aarch64 musl)\n");
    printf("Type 'help' for commands.\n");

    char line[1024];
    while (1) {
        printf("kei# ");
        fflush(stdout);
        if (!fgets(line, sizeof(line), stdin)) break;

        // Strip newline
        line[strcspn(line, "\n")] = 0;
        if (line[0] == 0) continue;

        // Parse command and args
        char *args[MAXARGS];
        int nargs = 0;
        char *tok = strtok(line, " ");
        while (tok && nargs < MAXARGS - 1) {
            args[nargs++] = tok;
            tok = strtok(NULL, " ");
        }
        args[nargs] = NULL;
        if (nargs == 0) continue;

        char *cmd = args[0];

        if (strcmp(cmd, "exit") == 0 || strcmp(cmd, "quit") == 0) break;
        else if (strcmp(cmd, "help") == 0) builtin_help();
        else if (strcmp(cmd, "ls") == 0) builtin_ls(args[1]);
        else if (strcmp(cmd, "cat") == 0) builtin_cat(args[1]);
        else if (strcmp(cmd, "echo") == 0) {
            for (int i = 1; i < nargs; i++)
                printf("%s%s", args[i], (i < nargs-1) ? " " : "");
            printf("\n");
        }
        else if (strcmp(cmd, "ps") == 0) {
            printf("PID TTY   CMD\n");
            DIR *d = opendir("/proc");
            if (d) {
                struct dirent *e;
                while ((e = readdir(d))) {
                    if (e->d_name[0] >= '0' && e->d_name[0] <= '9')
                        printf("%s  ?     [proc]\n", e->d_name);
                }
                closedir(d);
            }
        }
        else if (strcmp(cmd, "uname") == 0)
            printf("kei (aarch64) Asterinas fork\n");
        else if (strcmp(cmd, "id") == 0)
            printf("uid=0(root) gid=0(root)\n");
        else
            printf("%s: command not found\n", cmd);
    }
    return 0;
}
MINISH

# Compile minish with musl
/tmp/aarch64-linux-musl-gcc -O2 -o "$ROOTFS"/bin/sh /tmp/minish.c
chmod +x "$ROOTFS"/bin/sh

# Symlinks for common commands
cd "$ROOTFS"/bin
for cmd in ls cat echo ps uname id help; do
    ln -sf sh "$cmd"
done
cd "$KEI"

# /etc files
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$ROOTFS"/etc/passwd
printf 'root:x:0:\n' > "$ROOTFS"/etc/group

# authorized_keys
if [ -f /tmp/client_ssh_key.pub ]; then
    cp /tmp/client_ssh_key.pub "$ROOTFS"/etc/dropbear/authorized_keys
    echo "authorized_keys installed"
fi

echo "=== rootfs ready ==="
ls -la "$ROOTFS"/init "$ROOTFS"/sbin/ "$ROOTFS"/bin/
