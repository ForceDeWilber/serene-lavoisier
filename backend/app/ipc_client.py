import asyncio
import json
import logging
import sys
from typing import Any, Dict, Optional
from app.config import ENGINE_UDS_PATH

logger = logging.getLogger("ipc_client")

class EngineIpcClient:
    def __init__(self, socket_path: str = ENGINE_UDS_PATH):
        self.socket_path = socket_path

    async def _send_request(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        try:
            if hasattr(asyncio, "open_unix_connection") and not (":" in self.socket_path or sys.platform == "win32"):
                reader, writer = await asyncio.open_unix_connection(self.socket_path)
            else:
                host = "127.0.0.1"
                port = 9099
                if ":" in self.socket_path:
                    parts = self.socket_path.split(":")
                    host = parts[0]
                    port = int(parts[1])
                reader, writer = await asyncio.open_connection(host, port)

            message = json.dumps(payload) + "\n"
            writer.write(message.encode("utf-8"))
            await writer.drain()

            raw_response = await reader.readline()
            writer.close()
            await writer.wait_closed()

            if not raw_response:
                return {"type": "Error", "payload": {"error": "Empty response from engine daemon"}}

            return json.loads(raw_response.decode("utf-8").strip())
        except FileNotFoundError:
            return {"type": "Error", "payload": {"error": f"Engine socket not found at {self.socket_path}. Is engine-daemon running?"}}
        except ConnectionRefusedError:
            return {"type": "Error", "payload": {"error": f"Connection refused at {self.socket_path}. Is engine-daemon running?"}}
        except Exception as e:
            logger.error(f"UDS IPC error: {e}")
            return {"type": "Error", "payload": {"error": str(e)}}

    async def get_telemetry(self) -> Dict[str, Any]:
        request = {"type": "GetTelemetry", "payload": None}
        return await self._send_request(request)

    async def tune_runner(
        self,
        runner_id: str,
        paused: Optional[bool] = None,
        step_pct: Optional[float] = None,
        rebalance_threshold_pct: Optional[float] = None,
    ) -> Dict[str, Any]:
        request = {
            "type": "TuneRunner",
            "payload": {
                "runner_id": runner_id,
                "paused": paused,
                "step_pct": str(step_pct) if step_pct is not None else None,
                "rebalance_threshold_pct": str(rebalance_threshold_pct) if rebalance_threshold_pct is not None else None,
            },
        }
        return await self._send_request(request)

    async def emergency_kill_switch(self, reason: str = "Manual kill-switch triggered from Web UI") -> Dict[str, Any]:
        request = {
            "type": "EmergencyKillSwitch",
            "payload": {"reason": reason},
        }
        return await self._send_request(request)

ipc_client = EngineIpcClient()
