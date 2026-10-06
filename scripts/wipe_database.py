#!/usr/bin/env python3
"""Delete the Aura Seeker API's SQLite database and uploaded files.

Run it from the directory the server is started in, with the server stopped.
Usage: wipe_database.py [--yes]
"""

import argparse
import os
import shutil
import sys
from pathlib import Path
from urllib.parse import unquote, urlparse

DATABASE_URL = os.environ.get("DATABASE_URL", "sqlite://aura.db?mode=rwc")
STORAGE_URL = os.environ.get("STORAGE_URL", Path.cwd().joinpath("uploads").as_uri())
SQLITE_SIDE_FILES = ["", "-wal", "-shm", "-journal"]


def database_files():
    if not DATABASE_URL.startswith("sqlite:"):
        sys.exit(f"only SQLite databases can be wiped, not {DATABASE_URL}")
    path = DATABASE_URL.removeprefix("sqlite:").removeprefix("//").split("?")[0]
    if path in ("", ":memory:"):
        return []
    return [Path(path + suffix) for suffix in SQLITE_SIDE_FILES]


def uploads_directory():
    storage = urlparse(STORAGE_URL)
    if storage.scheme != "file":
        sys.exit(f"only local file storage can be wiped, not {STORAGE_URL}")
    return Path(unquote(storage.path))


def main():
    parser = argparse.ArgumentParser(description="Delete the API's SQLite database and uploaded files.")
    parser.add_argument("--yes", action="store_true", help="do not ask for confirmation")
    arguments = parser.parse_args()

    targets = [path.resolve() for path in [*database_files(), uploads_directory()] if path.exists()]
    if not targets:
        print("nothing to wipe")
        return
    print("this permanently deletes:", *targets, sep="\n  ")
    if not arguments.yes and input("continue? [y/N] ").strip().lower() != "y":
        sys.exit("aborted, nothing was deleted")
    for target in targets:
        shutil.rmtree(target) if target.is_dir() else target.unlink()
    print("wiped; the server recreates an empty database at its next start")


if __name__ == "__main__":
    main()
