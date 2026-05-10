"""Loom entry point."""

import logging
import sys
from pathlib import Path

from loom.server import initialize, mcp


def main() -> None:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(name)s] %(levelname)s: %(message)s",
    )

    target = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
    if not target.is_dir():
        logging.error("Target directory does not exist: %s", target)
        sys.exit(1)

    initialize(target)
    mcp.run()


if __name__ == "__main__":
    main()
