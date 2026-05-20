"""Atomic envelope writes.

Writes go to a sibling ``*.partial`` file, fsync, then ``os.replace`` to the
final path. ``os.replace`` is atomic on POSIX and on Windows, and overwrites
an existing target without TOCTOU games. Downstream collection tooling skips
``*.partial`` files, so a crash mid-write leaves a discardable artifact
rather than corrupting a valid envelope.
"""

from __future__ import annotations

import os
from pathlib import Path

from fleetbench_run.envelope import Envelope, envelope_to_json
from fleetbench_run.filenames import partial_path_for


def write_envelope(envelope: Envelope, results_dir: Path, filename: str) -> Path:
    """Serialize the envelope and write it atomically into results_dir/filename.

    Creates results_dir if it does not exist. Returns the final path.
    """
    results_dir.mkdir(parents=True, exist_ok=True)
    final_path = results_dir / filename
    write_text_atomically(final_path, envelope_to_json(envelope))
    return final_path


def write_text_atomically(final_path: Path, content: str) -> None:
    """Write ``content`` to ``final_path`` via a .partial file and atomic rename.

    Steps:
      1. Write content to ``<final>.partial``.
      2. fsync the partial file so its bytes are durable before the rename.
      3. ``os.replace`` the partial onto the final name (atomic on POSIX/Windows).
      4. fsync the containing directory on POSIX so the rename itself is durable.

    Step 4 is a best-effort: it's silently skipped on platforms where opening
    a directory for fsync isn't supported (notably Windows).
    """
    partial = Path(partial_path_for(str(final_path)))

    # Step 1+2: write and fsync the partial.
    fd = os.open(partial, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(content)
            f.flush()
            os.fsync(f.fileno())
    except BaseException:
        # If anything goes wrong, try to remove the partial so we don't leave
        # a confusing artifact next to the final file.
        try:
            partial.unlink()
        except OSError:
            pass
        raise

    # Step 3: atomic rename onto the final name.
    os.replace(partial, final_path)

    # Step 4: fsync the directory so the rename itself is durable. Skip on
    # platforms (Windows) that don't allow open()-on-directory.
    try:
        dir_fd = os.open(final_path.parent, os.O_RDONLY)
    except (OSError, PermissionError):
        return
    try:
        os.fsync(dir_fd)
    except OSError:
        pass
    finally:
        os.close(dir_fd)
