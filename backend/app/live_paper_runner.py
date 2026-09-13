import logging

logger = logging.getLogger("live_paper_runner")

class LivePaperRunnerStub:
    """
    Stub class to replace the legacy Python paper runner.
    All paper execution and state management is now strictly handled by the Rust engine simulator.
    """
    def __init__(self):
        self.running = False
        
    def get_telemetry(self):
        return {"status": "IPC required for telemetry"}

live_paper_runner = LivePaperRunnerStub()
