"""Archive the staged release directory and write SHA256SUMS.txt.

Run by CI as `python scripts/package.py <version>` after the package directory
has been assembled. Uses Python's `zipfile` because both shell-native options
proved unreliable on the Windows runner:

- `tar -a` in the runner's Git Bash wrote a 320 KB file whose header was "raze"
  instead of "PK" — a broken archive that shipped as a release.
- `Compress-Archive` depends on a PowerShell module that is not guaranteed to
  load.

The output archive is asserted to start with "PK" rather than trusted, since a
non-zip silently reaching a release is the exact failure being prevented.
"""

import hashlib
import os
import sys
import zipfile

PACKAGE_DIR = "dist/package"
OUT_DIR = "dist"
ARCHIVE_PREFIX = "razer-battery-taskbar"
# Everything inside the archive sits under a single top-level folder so
# extracting it does not scatter files into the user's directory.
ARCHIVE_ROOT = "razer-battery-taskbar"


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <version>", file=sys.stderr)
        return 2
    version = sys.argv[1]
    if not os.path.isdir(PACKAGE_DIR):
        print(f"error: {PACKAGE_DIR} missing; stage the package first", file=sys.stderr)
        return 1

    files = sorted(os.listdir(PACKAGE_DIR))
    if not files:
        print(f"error: {PACKAGE_DIR} is empty", file=sys.stderr)
        return 1

    name = f"{ARCHIVE_PREFIX}-{version}-windows-x64.zip"
    path = os.path.join(OUT_DIR, name)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for entry in files:
            archive.write(
                os.path.join(PACKAGE_DIR, entry),
                arcname=f"{ARCHIVE_ROOT}/{entry}",
            )

    with open(path, "rb") as fh:
        data = fh.read()
    if data[:2] != b"PK":
        print(f"error: {path} is not a zip (header {data[:4]!r})", file=sys.stderr)
        return 1

    digest = hashlib.sha256(data).hexdigest()
    with open(os.path.join(OUT_DIR, "SHA256SUMS.txt"), "w", encoding="ascii", newline="\n") as fh:
        fh.write(f"{digest}  {name}\n")

    print(f"{name}: {len(data)} bytes, sha256 {digest}")
    print(f"contents: {', '.join(files)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
