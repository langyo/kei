#!/usr/bin/env python3
"""Remove the Step::forward/backward_overflowing impls from the vendored
x86_64 0.15.5 crate. Those methods only exist in core::iter::Step on
nightly >= 2026-07-10; the pinned toolchain (nightly-2026-05-01) lacks
them, producing E0407. Only the trait-impl definitions are removed;
cfg(test) code is not compiled for a dependency."""
import re
import sys

PAT = re.compile(
    r"\n    // Kani's bundled toolchain predates these methods being added to `Step`\."
    r"\n    // Exclude them there so the crate still compiles under `cargo kani`\."
    r"\n    // This can be removed once Kani upgrades its bundled toolchain to nightly-2026-07-10 or later\."
    r"\n    #\[cfg\(not\(kani\)\)\]"
    r"(?:\n    #\[inline\])?"
    r"\n    fn (?:forward_overflowing|backward_overflowing)\(start: Self, count: usize\) -> \(Self, bool\) \{"
    r"\n        match Self::(?:forward_checked|backward_checked)\(start, count\) \{"
    r"\n            Some\(next\) => \(next, false\),"
    r"\n            None => \(start, true\),"
    r"\n        \}"
    r"\n    \}"
)

FILES = [
    "packages/patches/x86_64/src/addr.rs",
    "packages/patches/x86_64/src/structures/paging/page.rs",
    "packages/patches/x86_64/src/structures/paging/page_table.rs",
]

total = 0
for path in FILES:
    with open(path, "r", encoding="utf-8") as f:
        src = f.read()
    new, n = PAT.subn("", src)
    if n == 0:
        print(f"ERROR: no impl block matched in {path}")
        sys.exit(1)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(new)
    print(f"{path}: removed {n} impl block(s)")
    total += n

if total != 6:
    print(f"ERROR: expected 6 removals, got {total}")
    sys.exit(1)
print("OK: 6 impl blocks removed")
