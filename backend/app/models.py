from datetime import datetime, timezone
from sqlalchemy import Column, Integer, String, Float, Boolean, DateTime
from sqlalchemy.orm import declarative_base

Base = declarative_base()

class RunnerSession(Base):
    __tablename__ = "runner_sessions"

    id = Column(Integer, primary_key=True, autoincrement=True)
    runner_id = Column(String, index=True)
    symbol = Column(String)
    strategy_type = Column(String, default="GEOMETRIC_GRID")
    status = Column(String, default="RUNNING")
    created_at = Column(DateTime, default=lambda: datetime.now(timezone.utc))

class OrderRecord(Base):
    __tablename__ = "order_records"

    id = Column(Integer, primary_key=True, autoincrement=True)
    client_order_id = Column(String, unique=True, index=True)
    runner_id = Column(String, index=True)
    symbol = Column(String)
    side = Column(String)
    price = Column(Float)
    qty = Column(Float)
    status = Column(String)
    created_at = Column(DateTime, default=lambda: datetime.now(timezone.utc))

class TelemetryRecord(Base):
    __tablename__ = "telemetry_records"

    id = Column(Integer, primary_key=True, autoincrement=True)
    portfolio_gbp = Column(Float)
    btc_balance = Column(Float)
    eth_balance = Column(Float)
    resting_orders_count = Column(Integer)
    circuit_breaker = Column(Boolean, default=False)
    timestamp = Column(DateTime, default=lambda: datetime.now(timezone.utc))

class PairConfiguration(Base):
    __tablename__ = "pair_configurations"

    symbol = Column(String, primary_key=True)
    venue_symbol = Column(String, nullable=False)
    base_asset = Column(String, nullable=False)
    quote_asset = Column(String, nullable=False)
    envelope_capital = Column(Float, default=500.0)
    grid_step_pct = Column(Float, default=0.0040)
    grid_rungs = Column(Integer, default=5)
    order_size_fiat = Column(Float, default=50.0)
    rebalance_threshold_pct = Column(Float, default=0.012)
    sniper_enabled = Column(Boolean, default=True)
    sniper_order_size_fiat = Column(Float, default=50.0)
    sniper_hurdle_pct = Column(Float, default=0.0011)
    is_active = Column(Boolean, default=True)
    created_at = Column(DateTime, default=lambda: datetime.now(timezone.utc))

