import asyncio
import hmac
import logging
from contextlib import asynccontextmanager
from typing import Optional
from fastapi import FastAPI, WebSocket, WebSocketDisconnect, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel

from app.config import ENGINE_UDS_PATH, ENGINE_SECRET_KEY, ALLOWED_CORS_ORIGINS, TRADING_MODE
from app.database import init_db
from app.discord_bot import start_discord_bot, send_discord_alert
from app.ipc_client import ipc_client
from app.engine_coordinator import coordinator
from app.backtest_engine import HistoricalBacktestEngine, BacktestRequest, BacktestSummary

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("fastapi_control_plane")

@asynccontextmanager
async def lifespan(app: FastAPI):
    logger.info("Initializing FastAPI Control Plane...")
    await init_db()
    # Start Trading Engine Coordinator (boots Live and/or Paper runners based on TRADING_MODE)
    await coordinator.start()
    bot_task = asyncio.create_task(start_discord_bot())
    yield
    await coordinator.stop()
    bot_task.cancel()
    logger.info("Shutting down FastAPI Control Plane.")

app = FastAPI(
    title="Multi-Venue Crypto Trading Control Plane",
    description="FastAPI orchestration daemon bridging Next.js dashboard with isolated Live and Paper execution engines",
    version="0.2.0",
    lifespan=lifespan,
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=ALLOWED_CORS_ORIGINS,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

@app.middleware("http")
async def verify_engine_auth_middleware(request, call_next):
    # If no ENGINE_SECRET_KEY is configured, allow all requests (local dev mode)
    if not ENGINE_SECRET_KEY:
        return await call_next(request)

    # Health check is public (used by uptime monitors & reverse proxy health checks)
    if request.url.path == "/api/health" or request.method == "OPTIONS":
        return await call_next(request)

    # Check X-Engine-Secret header or Bearer authorization
    secret = request.headers.get("X-Engine-Secret")
    if not secret:
        auth_header = request.headers.get("Authorization", "")
        if auth_header.startswith("Bearer "):
            secret = auth_header[7:]

    if not secret or not hmac.compare_digest(secret, ENGINE_SECRET_KEY):
        from fastapi.responses import JSONResponse
        return JSONResponse(
            status_code=401,
            content={"detail": "Unauthorized: Invalid or missing X-Engine-Secret"}
        )

    return await call_next(request)

class TuneRunnerRequest(BaseModel):
    step_pct: Optional[float] = None
    rebalance_threshold_pct: Optional[float] = None

class KillSwitchRequest(BaseModel):
    reason: str = "Manual Emergency Kill Switch Triggered via Dashboard"

@app.get("/api/health")
async def health_check():
    return {
        "status": "healthy",
        "service": "fastapi-control-plane",
        "host_mode": coordinator.mode_config,
        "socket_path": ENGINE_UDS_PATH,
        "paper_runner_active": coordinator.paper_runner.running,
        "live_runner_active": coordinator.live_runner.running,
    }

@app.get("/api/telemetry")
async def get_telemetry(mode: Optional[str] = None):
    return coordinator.get_telemetry(mode)

@app.get("/api/live/diagnostics")
async def get_live_diagnostics():
    """Directly verifies Revolut X credentials, latency, and balance for Live mode."""
    return await coordinator.live_runner.verify_credentials_and_balances()

@app.post("/api/runners/{runner_id}/pause")
async def pause_runner(runner_id: str, mode: Optional[str] = None):
    runner = coordinator.get_runner(mode)
    res = runner.tune_runner(runner_id, paused=True)
    try:
        await ipc_client.tune_runner(runner_id, paused=True)
    except Exception:
        pass
    return res

@app.post("/api/runners/{runner_id}/resume")
async def resume_runner(runner_id: str, mode: Optional[str] = None):
    runner = coordinator.get_runner(mode)
    res = runner.tune_runner(runner_id, paused=False)
    try:
        await ipc_client.tune_runner(runner_id, paused=False)
    except Exception:
        pass
    return res

@app.post("/api/runners/{runner_id}/tune")
async def tune_runner(runner_id: str, req: TuneRunnerRequest, mode: Optional[str] = None):
    runner = coordinator.get_runner(mode)
    res = runner.tune_runner(
        runner_id,
        step_pct=req.step_pct,
        rebalance_threshold_pct=req.rebalance_threshold_pct,
    )
    try:
        await ipc_client.tune_runner(
            runner_id,
            step_pct=req.step_pct,
            rebalance_threshold_pct=req.rebalance_threshold_pct,
        )
    except Exception:
        pass
    return res

@app.post("/api/emergency/kill-switch")
async def emergency_kill_switch(req: KillSwitchRequest, mode: Optional[str] = None):
    runner = coordinator.get_runner(mode)
    if hasattr(runner, "emergency_kill_switch"):
        if asyncio.iscoroutinefunction(runner.emergency_kill_switch):
            res = await runner.emergency_kill_switch(reason=req.reason)
        else:
            res = runner.emergency_kill_switch(reason=req.reason)
    else:
        res = {"status": "error", "message": "Kill switch not supported on runner"}

    try:
        await ipc_client.emergency_kill_switch(reason=req.reason)
    except Exception:
        pass

    target_mode = (mode or coordinator.mode_config).upper()
    await send_discord_alert(
        title=f"🚨 EMERGENCY KILL SWITCH TRIGGERED [{target_mode}]",
        description=f"Reason: {req.reason}\nAll open resting orders canceled.",
    )
    return res

@app.post("/api/circuit-breaker/reset")
async def reset_circuit_breaker(mode: Optional[str] = None):
    runner = coordinator.get_runner(mode)
    return runner.reset_circuit_breaker()

class TuneSniperRequest(BaseModel):
    impulse_threshold_pct: Optional[float] = None
    snipe_order_size_gbp: Optional[float] = None
    min_net_edge_pct: Optional[float] = None

class ToggleSniperRequest(BaseModel):
    enabled: Optional[bool] = None

@app.post("/api/sniper/toggle")
async def toggle_sniper(req: Optional[ToggleSniperRequest] = None):
    # Sniper is active on paper sandbox
    enabled = req.enabled if req else None
    return coordinator.paper_runner.toggle_sniper(enabled=enabled)

@app.post("/api/sniper/tune")
async def tune_sniper(req: TuneSniperRequest):
    return coordinator.paper_runner.tune_sniper(
        impulse_threshold_pct=req.impulse_threshold_pct,
        snipe_order_size_gbp=req.snipe_order_size_gbp,
        min_net_edge_pct=req.min_net_edge_pct,
    )

class ConfigureCapitalRequest(BaseModel):
    profit_lock_pct: Optional[float] = None
    split_btc_pct: Optional[float] = None
    split_eth_pct: Optional[float] = None
    starting_balance_gbp: Optional[float] = None
    rungs_per_side: Optional[int] = None

@app.post("/api/capital/configure")
async def configure_capital(req: ConfigureCapitalRequest, mode: Optional[str] = None):
    runner = coordinator.get_runner(mode)
    if hasattr(runner, "capital_manager"):
        res = runner.configure_capital(
            profit_lock_pct=req.profit_lock_pct,
            split_btc_pct=req.split_btc_pct,
            split_eth_pct=req.split_eth_pct,
            starting_balance_gbp=req.starting_balance_gbp,
            rungs_per_side=req.rungs_per_side,
        )
        return {"status": "success", "payload": res}
    return {"status": "success", "payload": "Capital configuration updated"}

@app.post("/api/capital/sync-revolut")
async def sync_revolut_balances():
    res = await coordinator.live_runner.verify_credentials_and_balances()
    return {"status": "success", "payload": res}

@app.websocket("/api/ws/stream")
async def websocket_telemetry_stream(websocket: WebSocket):
    # Verify token if ENGINE_SECRET_KEY is configured
    if ENGINE_SECRET_KEY:
        token = (
            websocket.query_params.get("token")
            or websocket.headers.get("X-Engine-Secret")
        )
        if not token or not hmac.compare_digest(token, ENGINE_SECRET_KEY):
            logger.warning("Rejected unauthenticated WebSocket connection attempt")
            await websocket.close(code=1008) # Policy Violation
            return

    await websocket.accept()
    mode = websocket.query_params.get("mode")
    logger.info(f"Dashboard connected to telemetry WebSocket [Mode: {mode or 'default'}]")
    try:
        while True:
            telemetry = coordinator.get_telemetry(mode)
            await websocket.send_json(telemetry)
            await asyncio.sleep(1.0)
    except WebSocketDisconnect:
        logger.info(f"Dashboard WebSocket disconnected [Mode: {mode or 'default'}]")
    except Exception as e:
        logger.error(f"WebSocket stream error: {e}")

@app.post("/api/backtest/run", response_model=BacktestSummary)
async def run_backtest(req: BacktestRequest):
    # Host Isolation: Disallow heavy historical backtesting on dedicated Live Production host
    if coordinator.mode_config == "LIVE":
        raise HTTPException(
            status_code=403,
            detail="Backtest engine is disabled on Live Production host to prevent CPU and latency contention. Please run backtests on the Paper Sandbox host.",
        )
    try:
        candles = await HistoricalBacktestEngine.fetch_historical_candles(req.symbol, req.timeframe_days)
        summary = HistoricalBacktestEngine.run_simulation(req, candles)
        return summary
    except HTTPException:
        raise
    except Exception as e:
        logger.error(f"Backtest failed: {e}")
        raise HTTPException(status_code=500, detail=str(e))
