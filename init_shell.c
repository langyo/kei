// Freestanding interactive shell for kei aarch64 — no libc, no TLS.
// Listens on TCP port 22, accepts one connection at a time.
// Supports: help, uname, ls, cat, free, echo, ps, exit
// Compile: aarch64-linux-gnu-gcc -static -O2 -nostartfiles -ffreestanding -o init init_shell.c

// ── aarch64 syscall numbers ──
#define SYS_write    64
#define SYS_read     63
#define SYS_close    57
#define SYS_exit     93
#define SYS_mount   165
#define SYS_mkdir    34
#define SYS_nanosleep 101
#define SYS_socket  198
#define SYS_bind    200
#define SYS_listen  201
#define SYS_accept4 242
#define SYS_getpid  172
#define SYS_openat   56
#define SYS_fstat    80
#define SYS_getdents64 61
#define SYS_uname   160

#define AF_INET      2
#define SOCK_STREAM  1

struct sockaddr_in { short family; unsigned short port; unsigned addr; char pad[8]; };
struct linux_dirent64 { unsigned long d_ino; long d_off; short d_reclen; char d_type; char d_name[]; };
struct stat_buf { char raw[128]; };

// ── syscall wrappers ──
static inline long sc(long nr, long a, long b, long c, long d, long e, long f) {
    register long x8 asm("x8")=nr; register long x0 asm("x0")=a;
    register long x1 asm("x1")=b; register long x2 asm("x2")=c;
    register long x3 asm("x3")=d; register long x4 asm("x4")=e; register long x5 asm("x5")=f;
    asm volatile("svc 0":"=r"(x0):"r"(x8),"r"(x0),"r"(x1),"r"(x2),"r"(x3),"r"(x4),"r"(x5):"memory");
    return x0;
}

// ── string helpers ──
static int slen(const char*s){int n=0;while(s[n])n++;return n;}
static int scmp(const char*a,const char*b){
    while(*a&&*b&&*a==*b){a++;b++;} return *a-*b;
}
static int starts_with(const char*s,const char*pre){
    while(*pre){if(*s!=*pre)return 0;s++;pre++;} return 1;
}
static void putstr(const char*s){sc(SYS_write,2,(long)s,slen(s),0,0,0);}
static void putstr_fd(int fd,const char*s){sc(SYS_write,fd,(long)s,slen(s),0,0,0);}
static char *sncpy(char*dst,const char*src,int n){
    int i=0;while(i<n-1&&src[i]){dst[i]=src[i];i++;}dst[i]=0;return dst;}

// ── number formatting ──
static void putnum_fd(int fd, unsigned long n) {
    char buf[24]; int i=23; buf[i--]=0;
    if(n==0) buf[i--]='0';
    while(n>0){buf[i--]='0'+(n%10);n/=10;}
    putstr_fd(fd, &buf[i+1]);
}
static void puthex_fd(int fd, unsigned long n) {
    char buf[24]; int i=23; buf[i--]=0;
    if(n==0) buf[i--]='0';
    while(n>0){int d=n&0xf;buf[i--]=(d<10)?('0'+d):('a'+d-10);n>>=4;}
    putstr_fd(fd, &buf[i+1]);
}

// ── command handlers ──
static void cmd_help(int fd) {
    putstr_fd(fd,
        "\r\nAvailable commands:\r\n"
        "  help        Show this help\r\n"
        "  uname       System information\r\n"
        "  ls [dir]    List directory\r\n"
        "  cat <file>  Display file contents\r\n"
        "  free        Memory info (from /proc/meminfo)\r\n"
        "  ps          Process list (from /proc)\r\n"
        "  echo <msg>  Echo message\r\n"
        "  ping        TCP connectivity test\r\n"
        "  exit        Close connection\r\n"
    );
}

static void cmd_uname(int fd) {
    putstr_fd(fd, "kei kernel (aarch64)\r\n");
    putstr_fd(fd, "Asterinas fork | QEMU virt machine\r\n");
    putstr_fd(fd, "Kernel PID: "); putnum_fd(fd, sc(SYS_getpid,0,0,0,0,0,0));
    putstr_fd(fd, "\r\n");
}

static void cmd_ls(int fd, const char *path) {
    if (!path || !*path) path = "/";
    int dirfd = sc(SYS_openat, -100, (long)path, 0x10000, 0, 0, 0); // O_DIRECTORY=0x10000
    if (dirfd < 0) {
        putstr_fd(fd, "ls: cannot open "); putstr_fd(fd, path); putstr_fd(fd, "\r\n");
        return;
    }
    char buf[1024];
    long n;
    while ((n = sc(SYS_getdents64, dirfd, (long)buf, 1024, 0, 0, 0)) > 0) {
        long pos = 0;
        while (pos < n) {
            struct linux_dirent64 *d = (struct linux_dirent64*)(buf + pos);
            if (d->d_name[0] != 0) {
                putstr_fd(fd, d->d_name);
                char type_marker = (d->d_type == 4) ? '/' : ' ';
                char nl[3] = {type_marker, '\r', '\n'};
                sc(SYS_write, fd, (long)nl, 3, 0, 0, 0);
            }
            pos += d->d_reclen;
        }
    }
    sc(SYS_close, dirfd, 0, 0, 0, 0, 0);
}

static void cmd_cat(int fd, const char *path) {
    if (!path || !*path) { putstr_fd(fd, "cat: no file specified\r\n"); return; }
    int filefd = sc(SYS_openat, -100, (long)path, 0, 0, 0, 0); // O_RDONLY=0
    if (filefd < 0) {
        putstr_fd(fd, "cat: "); putstr_fd(fd, path);
        putstr_fd(fd, ": No such file\r\n");
        return;
    }
    char buf[512];
    long n;
    while ((n = sc(SYS_read, filefd, (long)buf, 512, 0, 0, 0)) > 0) {
        sc(SYS_write, fd, (long)buf, n, 0, 0, 0);
    }
    sc(SYS_close, filefd, 0, 0, 0, 0, 0);
    putstr_fd(fd, "\r\n");
}

static void cmd_free(int fd) {
    putstr_fd(fd, "Reading /proc/meminfo...\r\n");
    cmd_cat(fd, "/proc/meminfo");
}

static void cmd_ps(int fd) {
    putstr_fd(fd, "PID  COMMAND\r\n");
    putstr_fd(fd, "1    init (kei shell)\r\n");
    // Try reading /proc if available
    int dirfd = sc(SYS_openat, -100, (long)"/proc", 0x10000, 0, 0, 0);
    if (dirfd >= 0) {
        char buf[1024];
        long n;
        while ((n = sc(SYS_getdents64, dirfd, (long)buf, 1024, 0, 0, 0)) > 0) {
            long pos = 0;
            while (pos < n) {
                struct linux_dirent64 *d = (struct linux_dirent64*)(buf + pos);
                if (d->d_name[0] >= '0' && d->d_name[0] <= '9') {
                    putstr_fd(fd, d->d_name);
                    putstr_fd(fd, "    [proc]\r\n");
                }
                pos += d->d_reclen;
            }
        }
        sc(SYS_close, dirfd, 0, 0, 0, 0, 0);
    }
}

// ── line buffer for telnet/netcat ──
static void process_line(int fd, char *line) {
    // Strip trailing whitespace
    int len = slen(line);
    while (len > 0 && (line[len-1]=='\r'||line[len-1]=='\n'||line[len-1]==' '))
        line[--len] = 0;
    if (len == 0) return;

    char *arg = 0;
    for (int i = 0; i < len; i++) {
        if (line[i] == ' ') { line[i] = 0; arg = &line[i+1]; while(*arg==' ')arg++; break; }
    }

    if (scmp(line, "help")==0) cmd_help(fd);
    else if (scmp(line, "uname")==0) cmd_uname(fd);
    else if (scmp(line, "ls")==0) cmd_ls(fd, arg);
    else if (scmp(line, "cat")==0) cmd_cat(fd, arg);
    else if (scmp(line, "free")==0) cmd_free(fd);
    else if (scmp(line, "ps")==0) cmd_ps(fd);
    else if (scmp(line, "echo")==0) {
        if (arg) { putstr_fd(fd, arg); }
        putstr_fd(fd, "\r\n");
    }
    else if (scmp(line, "ping")==0) {
        putstr_fd(fd, "pong! TCP connection alive.\r\n");
    }
    else if (scmp(line, "exit")==0 || scmp(line, "quit")==0) {
        putstr_fd(fd, "Goodbye!\r\n");
        // Signal exit by closing — caller checks for -1
    }
    else {
        putstr_fd(fd, line);
        putstr_fd(fd, ": command not found. Type 'help'.\r\n");
    }
}

static int handle_client(int fd) {
    const char *banner =
        "\r\n"
        "╔══════════════════════════════════════╗\r\n"
        "║     kei kernel (aarch64) shell       ║\r\n"
        "║     Asterinas | QEMU virt            ║\r\n"
        "╚══════════════════════════════════════╝\r\n"
        "\r\n"
        "Type 'help' for available commands.\r\n"
        "kei> ";
    sc(SYS_write, fd, (long)banner, slen(banner), 0,0,0);

    char buf[256];
    char line[256];
    int linepos = 0;
    int should_exit = 0;

    while (!should_exit) {
        long n = sc(SYS_read, fd, (long)buf, 255, 0,0,0);
        if (n <= 0) break;

        for (int i = 0; i < n; i++) {
            char c = buf[i];
            if (c == '\r') continue;
            if (c == '\n') {
                line[linepos] = 0;
                if (linepos > 0) {
                    if (scmp(line, "exit")==0 || scmp(line, "quit")==0) {
                        sc(SYS_write, fd, (long)"Goodbye!\r\n", 10, 0,0,0);
                        should_exit = 1;
                        break;
                    }
                    process_line(fd, line);
                }
                linepos = 0;
                sc(SYS_write, fd, (long)"kei> ", 5, 0,0,0);
            } else if (c == 0x7f || c == 0x08) { // backspace
                if (linepos > 0) {
                    linepos--;
                    sc(SYS_write, fd, (long)"\b \b", 3, 0,0,0);
                }
            } else if (linepos < 254) {
                line[linepos++] = c;
                // Echo back for interactive terminals
                sc(SYS_write, fd, (long)&c, 1, 0,0,0);
            }
        }
    }
    return should_exit;
}

// Syscall numbers needed for init
#define SYS_execve  221
#define SYS_clone   220

void _start(void) {
    sc(SYS_mount, (long)"none",(long)"/proc",(long)"proc",0,0,0);
    sc(SYS_mount, (long)"none",(long)"/sys",(long)"sysfs",0,0,0);
    sc(SYS_mkdir, (long)"/var/run",0755,0,0,0,0);
    sc(SYS_mkdir, (long)"/tmp",0777,0,0,0,0);

    putstr("\n=== kei ignition (aarch64) ===\n");
    putstr("Exec'ing dropbear SSH server...\n");

    // Directly execve dropbear — replaces init process.
    // dropbear runs as PID 1, handles accept+fork internally.
    static char *db_argv[] = {"/sbin/dropbear","-F","-R","-p","22", 0};
    static char *db_envp[] = {"PATH=/bin:/sbin","HOME=/root", 0};
    sc(SYS_execve, (long)"/sbin/dropbear", (long)db_argv, (long)db_envp, 0,0,0);

    // If execve fails, fall through to TCP shell on port 23
    putstr("ERROR: exec dropbear failed, starting TCP shell on port 23\n");

    long sockfd = sc(SYS_socket, AF_INET, SOCK_STREAM, 0, 0,0,0);
    if (sockfd < 0) {
        sc(SYS_exit, 1, 0,0,0,0,0);
    }

    struct sockaddr_in addr;
    addr.family = AF_INET;
    addr.port = 0x1700; // port 23 big-endian
    addr.addr = 0;
    for (int i=0;i<8;i++) addr.pad[i]=0;

    if (sc(SYS_bind, sockfd, (long)&addr, 16, 0,0,0) < 0 ||
        sc(SYS_listen, sockfd, 5, 0,0,0,0) < 0) {
        sc(SYS_exit, 1, 0,0,0,0,0);
    }
    putstr("Connect: nc localhost 2222\n");

    // Single-threaded accept loop (no fork needed)
    while (1) {
        struct sockaddr_in cli;
        int clilen = 16;
        long clientfd = sc(SYS_accept4, sockfd, (long)&cli, (long)&clilen, 0, 0, 0);
        if (clientfd < 0) {
            struct { long sec; long nsec; } ts = {1, 0};
            sc(SYS_nanosleep, (long)&ts, 0,0,0,0,0);
            continue;
        }

        putstr("Client connected\n");
        handle_client(clientfd);
        sc(SYS_close, clientfd, 0,0,0,0,0);
        putstr("Client disconnected\n");
    }
}
