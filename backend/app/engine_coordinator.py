import logging
import os
from typing import Any, Dict, Optional
from app.config import TRADING_MODE
from app.live_paper_runner import live_paper_runner
from app.live_trading_runner import live_trading_runner

logger = logging.getLogger("engine_coordinator")

class TradingEngineCoordinator:
    """
    Coordinates multi-mode trading execution between:
    - 'paper': Pure sandbox simulation runner with virtual funds & backtesting
    - 'live': Genuine Revolut X live order execution with real capital & strict auth
    """

    def __init__(self):
        self.mode_config: str = os.getenv("TRADING_MODE", TRADING_MODE).upper() # "PAPER", "LIVE", or "DUAL"
        self.paper_runner = live_paper_runner
        self.live_runner = live_trading_runner

    async def start(self):
        logger.info(f"Starting Trading Engine Coordinator in [{self.mode_config}] mode...")
        if self.mode_config == "LIVE":
            # Live host only runs live engine. Zero backtesting or historical data downloads.
            logger.info("Host Mode: DEDICATED LIVE PRODUCTION. Starting LiveTradingRunner only.")
            await self.live_runner.start()
        elif self.mode_config == "PAPER":
            # Sandbox host runs paper simulator + backtesting
            logger.info("Host Mode: PAPER SANDBOX. Starting PaperTradingRunner.")
            await self.paper_runner.start()
        else:
            # DUAL mode (for local testing / dev)
            logger.info("Host Mode: DUAL (Paper + Live). Starting both runners concurrently.")
            await self.paper_runner.start()
            await self.live_runner.start()

    async def stop(self):
        logger.info("Stopping Trading Engine Coordinator...")
        if self.mode_config in ("PAPER", "DUAL"):
            await self.paper_runner.stop()
        if self.mode_config in ("LIVE", "DUAL"):
            await self.live_runner.stop()

    def get_runner(self, mode: Optional[str] = None):
        """
        Returns the appropriate runner based on requested mode.
        If mode is not specified, defaults to current host configuration or 'paper'.
        """
        selected_mode = (mode or "").lower()
        if selected_mode == "live":
            return self.live_runner
        return self.paper_runner

    def get_telemetry(self, mode: Optional[str] = None) -> Dict[str, Any]:
        runner = self.get_runner(mode)
        telemetry = runner.get_telemetry()
        # Add coordinator context
        telemetry["host_mode"] = self.mode_config
        return telemetry

coordinator = TradingEngineCoordinator()
