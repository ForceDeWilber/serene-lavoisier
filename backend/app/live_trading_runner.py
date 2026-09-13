import logging

logger = logging.getLogger("live_trading_runner")

class LiveTradingRunnerStub:
    """
    Stub class to replace the legacy Python execution runner.
    All live execution and state management is now strictly handled by the Rust engine.
    """
    def __init__(self):
        self.running = False
        
    async def verify_credentials_and_balances(self):
        return {"status": "success", "message": "Verification is handled directly by Rust engine via IPC"}
        
    def get_telemetry(self):
        return {"status": "IPC required for telemetry"}

live_trading_runner = LiveTradingRunnerStub()
