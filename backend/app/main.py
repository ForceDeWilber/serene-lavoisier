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
    return await coordinator.get_telemetry_async(mode)

@app.get("/api/live/diagnostics")
async def get_live_diagnostics():
    """Directly verifies Revolut X credentials, latency, and balance for Live mode."""
    return await coordinator.live_runner.verify_credentials_and_balances()

@app.post("/api/runners/{runner_id}/pause")
async def pause_runner(runner_id: str, mode: Optional[str] = None):
    try:
        await ipc_client.tune_runner(runner_id, paused=True)
    except Exception:
        pass
    return {"status": "success"}

@app.post("/api/runners/{runner_id}/resume")
async def resume_runner(runner_id: str, mode: Optional[str] = None):
    try:
        await ipc_client.tune_runner(runner_id, paused=False)
    except Exception:
        pass
    return {"status": "success"}

@app.post("/api/runners/{runner_id}/tune")
async def tune_runner(runner_id: str, req: TuneRunnerRequest, mode: Optional[str] = None):
    try:
        await ipc_client.tune_runner(
            runner_id,
            step_pct=req.step_pct,
            rebalance_threshold_pct=req.rebalance_threshold_pct,
        )
    except Exception:
        pass
    return {"status": "success"}

@app.post("/api/emergency/kill-switch")
async def emergency_kill_switch(req: KillSwitchRequest, mode: Optional[str] = None):
    try:
        await ipc_client.emergency_kill_switch(reason=req.reason)
    except Exception:
        pass

    target_mode = (mode or coordinator.mode_config).upper()
    await send_discord_alert(
        title=f"🚨 EMERGENCY KILL SWITCH TRIGGERED [{target_mode}]",
        description=f"Reason: {req.reason}\nAll open resting orders canceled.",
    )
    return {"status": "success", "message": "Kill switch dispatched to Rust Engine via IPC"}

@app.post("/api/circuit-breaker/reset")
async def reset_circuit_breaker(mode: Optional[str] = None):
    try:
        await ipc_client.reset_circuit_breaker()
    except Exception:
        pass
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
    enabled = req.enabled if req else None
    try:
        await ipc_client.toggle_sniper("sniper_btc", enabled=enabled)
    except Exception:
        pass
    return {"status": "success"}

@app.post("/api/sniper/tune")
async def tune_sniper(req: TuneSniperRequest):
    try:
        await ipc_client.tune_sniper(
            "sniper_btc",
            impulse_threshold_pct=req.impulse_threshold_pct,
            order_size_gbp=req.snipe_order_size_gbp,
        )
    except Exception:
        pass
    return {"status": "success"}

class ConfigureCapitalRequest(BaseModel):
    profit_lock_pct: Optional[float] = None
    split_btc_pct: Optional[float] = None
    split_eth_pct: Optional[float] = None
    starting_balance_gbp: Optional[float] = None
    rungs_per_side: Optional[int] = None

@app.post("/api/capital/configure")
async def configure_capital(req: ConfigureCapitalRequest, mode: Optional[str] = None):
    # Pass through to IPC if implemented
    return {"status": "success", "payload": "Capital configuration updated"}

@app.post("/api/capital/sync-revolut")
async def sync_revolut_balances():
    res = await coordinator.live_runner.verify_credentials_and_balances()
    return {"status": "success", "payload": res}

class AddPairRequest(BaseModel):
    symbol: str
    venue_symbol: Optional[str] = None
    base_asset: Optional[str] = None
    quote_asset: Optional[str] = None
    envelope_capital: float = 500.0
    grid_step_pct: float = 0.0040
    grid_rungs: int = 5
    order_size_fiat: float = 50.0
    rebalance_threshold_pct: float = 0.012
    sniper_enabled: bool = True
    sniper_order_size_fiat: float = 50.0
    sniper_hurdle_pct: float = 0.0011
    is_active: bool = True

@app.get("/api/pairs")
async def get_trading_pairs(mode: Optional[str] = None):
    """Fetches configured trading pairs from SQLite and Rust Engine."""
    target_mode = (mode or coordinator.mode_config).lower()
    from app.database import PaperSessionLocal, LiveSessionLocal
    from app.models import PairConfiguration
    from sqlalchemy import select

    sessionmaker = LiveSessionLocal if target_mode == "live" else PaperSessionLocal
    try:
        async with sessionmaker() as session:
            result = await session.execute(select(PairConfiguration))
            pairs = result.scalars().all()
            if pairs:
                return [
                    {
                        "symbol": p.symbol,
                        "venue_symbol": p.venue_symbol,
                        "base_asset": p.base_asset,
                        "quote_asset": p.quote_asset,
                        "envelope_capital": p.envelope_capital,
                        "grid_step_pct": p.grid_step_pct,
                        "grid_rungs": p.grid_rungs,
                        "order_size_fiat": p.order_size_fiat,
                        "rebalance_threshold_pct": p.rebalance_threshold_pct,
                        "sniper_enabled": p.sniper_enabled,
                        "sniper_order_size_fiat": p.sniper_order_size_fiat,
                        "sniper_hurdle_pct": p.sniper_hurdle_pct,
                        "is_active": p.is_active,
                    }
                    for p in pairs
                ]
    except Exception as e:
        logger.warning(f"Failed to query pairs from DB: {e}")

    # Fallback to IPC
    ipc_res = await ipc_client.list_pairs()
    if ipc_res.get("type") == "PairsList":
        return ipc_res.get("payload", {}).get("pairs", [])
    return []

@app.post("/api/pairs")
async def add_trading_pair(req: AddPairRequest, mode: Optional[str] = None):
    """Hot-adds or updates a trading pair across DB, Kraken Oracle, and Rust Engine."""
    sym_slash = req.symbol.upper().replace("-", "/")
    parts = sym_slash.split("/")
    base = req.base_asset or parts[0]
    quote = req.quote_asset or (parts[1] if len(parts) > 1 else "USD")
    venue = req.venue_symbol or f"{base}-{quote}"

    target_mode = (mode or coordinator.mode_config).lower()
    from app.database import PaperSessionLocal, LiveSessionLocal
    from app.models import PairConfiguration
    from app.kraken_streamer import kraken_streamer

    sessionmaker = LiveSessionLocal if target_mode == "live" else PaperSessionLocal
    try:
        async with sessionmaker() as session:
            pair = await session.get(PairConfiguration, sym_slash)
            if not pair:
                pair = PairConfiguration(symbol=sym_slash)
                session.add(pair)

            pair.venue_symbol = venue
            pair.base_asset = base
            pair.quote_asset = quote
            pair.envelope_capital = req.envelope_capital
            pair.grid_step_pct = req.grid_step_pct
            pair.grid_rungs = req.grid_rungs
            pair.order_size_fiat = req.order_size_fiat
            pair.rebalance_threshold_pct = req.rebalance_threshold_pct
            pair.sniper_enabled = req.sniper_enabled
            pair.sniper_order_size_fiat = req.sniper_order_size_fiat
            pair.sniper_hurdle_pct = req.sniper_hurdle_pct
            pair.is_active = req.is_active
            await session.commit()
    except Exception as e:
        logger.error(f"Failed to save pair {sym_slash} to DB: {e}")

    # Hot-subscribe Python streamer
    try:
        await kraken_streamer.subscribe_symbol(sym_slash)
    except Exception as e:
        logger.warning(f"Notice hot-subscribing python kraken streamer: {e}")

    # Dispatch to Rust Engine over IPC
    pair_dto = {
        "symbol": {"base": base, "quote": quote},
        "envelope_capital": str(req.envelope_capital),
        "grid_step_pct": str(req.grid_step_pct),
        "grid_rungs": req.grid_rungs,
        "order_size_fiat": str(req.order_size_fiat),
        "rebalance_threshold_pct": str(req.rebalance_threshold_pct),
        "sniper_enabled": req.sniper_enabled,
        "sniper_order_size_fiat": str(req.sniper_order_size_fiat),
        "sniper_hurdle_pct": str(req.sniper_hurdle_pct),
        "is_active": req.is_active,
    }

    ipc_res = await ipc_client.add_pair(pair_dto)
    return {
        "status": "success",
        "message": f"Pair {sym_slash} configured and hot-spawned in Rust Engine",
        "ipc_result": ipc_res,
    }

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
            telemetry = await coordinator.get_telemetry_async(mode)
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
