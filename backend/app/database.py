from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker, AsyncSession
from app.config import DATABASE_URL, DATABASE_URL_PAPER, DATABASE_URL_LIVE
from app.models import Base

# Dedicated engine for Paper Sandbox & Backtesting
paper_engine = create_async_engine(DATABASE_URL_PAPER, echo=False)
PaperSessionLocal = async_sessionmaker(paper_engine, class_=AsyncSession, expire_on_commit=False)

# Dedicated engine strictly for Live Real-Capital Execution
live_engine = create_async_engine(DATABASE_URL_LIVE, echo=False)
LiveSessionLocal = async_sessionmaker(live_engine, class_=AsyncSession, expire_on_commit=False)

# Default aliases for backwards compatibility
engine = paper_engine
AsyncSessionLocal = PaperSessionLocal

async def init_db():
    """Initializes schema on both Paper and Live databases independently."""
    async with paper_engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
    async with live_engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)

async def get_db(mode: str = "paper"):
    """Returns database session strictly mapped to the requested environment mode."""
    target_sessionmaker = LiveSessionLocal if mode.lower() == "live" else PaperSessionLocal
    async with target_sessionmaker() as session:
        yield session
