from __future__ import annotations

import argparse
import logging

import uvicorn

from symphony.config import load_config
from symphony.orchestrator import SymphonyOrchestrator
from symphony.web import create_app


def main() -> None:
    parser = argparse.ArgumentParser(prog="symphony")
    parser.add_argument("command", choices=["run", "serve"])
    parser.add_argument("--config", default="symphony.json")
    parser.add_argument("--once", action="store_true")
    parser.add_argument("--log-level", default="INFO")
    args = parser.parse_args()

    logging.basicConfig(
        level=args.log_level.upper(),
        format="%(asctime)s %(levelname)s %(message)s",
    )
    config = load_config(args.config)
    if args.command == "serve":
        uvicorn.run(create_app(config), host=config.host, port=config.port)
        return
    orchestrator = SymphonyOrchestrator(config)
    if args.once:
        summary = orchestrator.poll_once()
        logging.info(
            "poll complete: started=%s skipped=%s failed=%s",
            summary.started,
            summary.skipped,
            summary.failed,
        )
        return
    orchestrator.run_forever()


if __name__ == "__main__":
    main()
