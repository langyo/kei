#!/bin/bash
set -e
export AR=aarch64-linux-gnu-ar
export RANLIB=aarch64-linux-gnu-ranlib
cd /tmp/dropbear-2022.83

# Force vfork instead of fork to avoid kernel CoW fork bug
cat > localoptions.h << 'OPT'
#define DROPBEAR_SVR_PASSWORD_AUTH 0
#define DROPBEAR_SVR_PAM_AUTH 0
#define DROPBEAR_SVR_PUBKEY_AUTH 1
#define DEFAULT_PATH "/bin:/sbin"
#define DROPBEAR_X11FWD 0
// Force vfork: avoids kernel clone/fork EC=0 trap
#undef HAVE_FORK
#define DROPBEAR_VFORK 1
OPT

make clean 2>/dev/null || true
CC=/tmp/aarch64-linux-musl-gcc ./configure \
    --host=aarch64-linux-musl \
    --disable-zlib --disable-syslog \
    --disable-lastlog --disable-utmp --disable-utmpx \
    --disable-wtmp --disable-wtmpx \
    --disable-loginfunc --disable-pututline --disable-pututxline \
    ac_cv_func_fork=no \
    2>&1 | tail -5
sed -i 's/-Wl,-pie//' Makefile
echo "=== building ==="
make -j4 PROGRAMS="dropbear dropbearkey" 2>&1 | tail -5
echo "=== verify ==="
file dropbear
aarch64-linux-gnu-readelf -l dropbear 2>/dev/null | grep interpreter && echo "DYNAMIC!" || echo "STATIC_OK"
