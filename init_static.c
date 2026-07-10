// Minimal freestanding init+shell for kei aarch64 — no libc, no TLS.
// Implements a tiny TCP command server on port 22.
// Compile: aarch64-linux-gnu-gcc -static -O2 -nostartfiles -ffreestanding -o init init_static.c

#define SYS_write    64
#define SYS_read     63
#define SYS_close    57
#define SYS_exit     93
#define SYS_clone   220
#define SYS_execve  221
#define SYS_mount   165
#define SYS_mkdir    34
#define SYS_nanosleep 101
#define SYS_socket  198
#define SYS_bind    200
#define SYS_listen  201
#define SYS_accept4 242
#define SYS_wait4   260
#define SYS_getpid  172
#define SYS_uname   160
#define SYS_openat  56
#define SYS_fstat   80
#define SYS_brk     214

#define AF_INET      2
#define SOCK_STREAM  1
#define SOL_SOCKET    1
#define SO_REUSEADDR  2
#define SOCK_CLOEXEC 02000000

struct sockaddr_in { short family; unsigned short port; unsigned addr; char pad[8]; };

static inline long sc3(long nr, long a, long b, long c) {
    register long x8 asm("x8")=nr; register long x0 asm("x0")=a;
    register long x1 asm("x1")=b; register long x2 asm("x2")=c;
    asm volatile("svc 0":"=r"(x0):"r"(x8),"r"(x0),"r"(x1),"r"(x2):"memory"); return x0;
}
static inline long sc6(long nr, long a, long b, long c, long d, long e, long f) {
    register long x8 asm("x8")=nr; register long x0 asm("x0")=a;
    register long x1 asm("x1")=b; register long x2 asm("x2")=c;
    register long x3 asm("x3")=d; register long x4 asm("x4")=e; register long x5 asm("x5")=f;
    asm volatile("svc 0":"=r"(x0):"r"(x8),"r"(x0),"r"(x1),"r"(x2),"r"(x3),"r"(x4),"r"(x5):"memory"); return x0;
}

static int slen(const char*s){int n=0;while(s[n])n++;return n;}
static void putstr(const char*s){sc3(SYS_write,2,(long)s,slen(s));}
static void putnum(long n){
    char buf[24]; int i=23; buf[i--]=0;
    if(n==0){buf[i--]='0';}
    while(n>0){buf[i--]='0'+(n%10);n/=10;}
    sc3(SYS_write,2,(long)&buf[i+1],slen(&buf[i+1]));
}

static void handle_client(int fd) {
    const char *banner =
        "\r\n"
        "========================================\r\n"
        "  kei kernel (aarch64) — serial console  \r\n"
        "========================================\r\n"
        "  Available commands:                    \r\n"
        "    help    - show this help             \r\n"
        "    uname   - system info                \r\n"
        "    pid     - show init PID              \r\n"
        "    echo X  - echo back X                \r\n"
        "    exit    - close connection           \r\n"
        "========================================\r\n"
        "kei> ";
    sc3(SYS_write, fd, (long)banner, slen(banner));

    char buf[256];
    while (1) {
        long n = sc3(SYS_read, fd, (long)buf, 255);
        if (n <= 0) break;

        // Process line by line
        for (int i = 0; i < n; i++) {
            if (buf[i] == '\r' || buf[i] == '\n') buf[i] = '\0';
        }
        buf[n] = 0;

        // Find first non-empty line
        char *cmd = buf;
        while (*cmd == '\0' && cmd < buf + n) cmd++;

        if (slen(cmd) == 0) {
            sc3(SYS_write, fd, (long)"kei> ", 5);
            continue;
        }

        // Check commands
        if (cmd[0]=='h' && cmd[1]=='e' && cmd[2]=='l' && cmd[3]=='p') {
            sc3(SYS_write, fd, (long)banner, slen(banner));
        } else if (cmd[0]=='u' && cmd[1]=='n' && cmd[2]=='a') {
            const char *info = "kei kernel (aarch64) Asterinas fork\r\nQEMU virt machine\r\n";
            sc3(SYS_write, fd, (long)info, slen(info));
            sc3(SYS_write, fd, (long)"PID=", 4);
            putnum(sc3(SYS_getpid, 0, 0, 0));
            sc3(SYS_write, fd, (long)"\r\n", 2);
        } else if (cmd[0]=='p' && cmd[1]=='i' && cmd[2]=='d') {
            sc3(SYS_write, fd, (long)"init PID=", 9);
            putnum(sc3(SYS_getpid, 0, 0, 0));
            sc3(SYS_write, fd, (long)"\r\n", 2);
        } else if (cmd[0]=='e' && cmd[1]=='x' && cmd[2]=='i' && cmd[3]=='t') {
            sc3(SYS_write, fd, (long)"Goodbye!\r\n", 10);
            break;
        } else if (cmd[0]=='e' && cmd[1]=='c' && cmd[2]=='h' && cmd[3]=='o') {
            char *arg = cmd + 4;
            while (*arg == ' ') arg++;
            sc3(SYS_write, fd, (long)arg, slen(arg));
            sc3(SYS_write, fd, (long)"\r\n", 2);
        } else {
            const char *err = "Unknown command. Type 'help'.\r\n";
            sc3(SYS_write, fd, (long)err, slen(err));
        }
        sc3(SYS_write, fd, (long)"kei> ", 5);
    }
    sc3(SYS_close, fd, 0, 0);
}

void _start(void) {
    sc6(SYS_mount, (long)"none",(long)"/proc",(long)"proc",0,0,0);
    sc6(SYS_mount, (long)"none",(long)"/sys",(long)"sysfs",0,0,0);
    sc6(SYS_mkdir, (long)"/var/run",0755,0,0,0,0);
    sc6(SYS_mkdir, (long)"/tmp",0777,0,0,0,0);

    putstr("\n=== kei ignition (aarch64) ===\n");

    // Create TCP socket on port 22
    long sockfd = sc3(SYS_socket, AF_INET, SOCK_STREAM, 0);
    if (sockfd < 0) {
        putstr("ERROR: socket failed\n");
        sc6(SYS_exit, 1, 0,0,0,0,0);
    }

    // Set SO_REUSEADDR
    int optval = 1;
    sc6(SYS_socket + 0, sockfd, SOL_SOCKET, SO_REUSEADDR, (long)&optval, 4, 0); // setsockopt=208

    // Bind to 0.0.0.0:22
    struct sockaddr_in addr;
    addr.family = AF_INET;
    addr.port = 0x1600; // port 22 in network byte order (big-endian)
    addr.addr = 0; // INADDR_ANY
    for (int i = 0; i < 8; i++) addr.pad[i] = 0;

    long r = sc3(SYS_bind, sockfd, (long)&addr, sizeof(addr));
    if (r < 0) {
        putstr("ERROR: bind failed\n");
        sc6(SYS_exit, 1, 0,0,0,0,0);
    }

    r = sc3(SYS_listen, sockfd, 5, 0);
    if (r < 0) {
        putstr("ERROR: listen failed\n");
        sc6(SYS_exit, 1, 0,0,0,0,0);
    }

    putstr("TCP shell server listening on port 22\n");
    putstr("Connect from host: nc localhost 2222\n");

    // Accept loop
    while (1) {
        struct sockaddr_in cli;
        int clilen = sizeof(cli);
        long clientfd = sc6(SYS_accept4, sockfd, (long)&cli, (long)&clilen, 0, 0, 0);
        if (clientfd < 0) {
            struct { long sec; long nsec; } ts = {1, 0};
            sc6(SYS_nanosleep, (long)&ts, 0,0,0,0,0);
            continue;
        }

        putstr("Client connected!\n");

        // Fork to handle client
        long pid = sc6(SYS_clone, 17, 0,0,0,0,0);
        if (pid == 0) {
            // Child: close listen socket, handle client
            sc3(SYS_close, sockfd, 0, 0);
            handle_client(clientfd);
            sc6(SYS_exit, 0, 0,0,0,0,0);
        } else {
            // Parent: close client socket
            sc3(SYS_close, clientfd, 0, 0);
        }
    }
}
