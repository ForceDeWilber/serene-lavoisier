import asyncio
import logging
import discord
from discord import app_commands
from discord.ext import commands
from app.config import DISCORD_BOT_TOKEN, DISCORD_CHANNEL_ID
from app.ipc_client import ipc_client

logger = logging.getLogger("discord_bot")

intents = discord.Intents.default()
bot = commands.Bot(command_prefix="!", intents=intents)

@bot.event
async def on_ready():
    logger.info(f"Discord Bot logged in as {bot.user} (ID: {bot.user.id})")
    try:
        synced = await bot.tree.sync()
        logger.info(f"Synced {len(synced)} application slash commands.")
    except Exception as e:
        logger.error(f"Failed to sync slash commands: {e}")

@bot.tree.command(name="status", description="[Read-only] View current trading engine status and portfolio balances")
async def slash_status(interaction: discord.Interaction):
    await interaction.response.defer()
    resp = await ipc_client.get_telemetry()

    if resp.get("type") == "Error":
        await interaction.followup.send(f"⚠️ Engine Error: {resp.get('payload', {}).get('error', 'Unknown')}")
        return

    payload = resp.get("payload", {})
    cb = payload.get("circuit_breaker_tripped", False)
    balances = payload.get("balances", {})
    resting = payload.get("resting_orders_count", 0)

    color = discord.Color.red() if cb else discord.Color.green()
    embed = discord.Embed(
        title="⚡ Trading Engine Telemetry",
        color=color,
    )
    embed.add_field(name="Circuit Breaker", value="🚨 TRIPPED" if cb else "✅ NORMAL", inline=True)
    embed.add_field(name="Resting Orders", value=str(resting), inline=True)
    embed.add_field(name="Venue Model", value="Revolut X (0% Maker) + Kraken Pro WS", inline=False)

    balance_str = "\n".join([f"• **{k}**: {float(v):.6f}" if k != "GBP" else f"• **GBP**: £{float(v):.2f}" for k, v in balances.items()])
    embed.add_field(name="Portfolio Balances", value=balance_str or "No balances", inline=False)

    embed.set_footer(text="Read-only monitoring | Zero trade execution privileges")
    await interaction.followup.send(embed=embed)

@bot.tree.command(name="runners", description="[Read-only] List active strategy runners and execution states")
async def slash_runners(interaction: discord.Interaction):
    await interaction.response.defer()
    resp = await ipc_client.get_telemetry()

    if resp.get("type") == "Error":
        await interaction.followup.send(f"⚠️ Engine Error: {resp.get('payload', {}).get('error', 'Unknown')}")
        return

    runners = resp.get("payload", {}).get("runners", [])
    embed = discord.Embed(title="🤖 Active Strategy Runners", color=discord.Color.blue())

    for r in runners:
        status = "⏸️ PAUSED" if r.get("is_paused") else "🟢 ACTIVE"
        val = (
            f"• **Symbol**: {r.get('symbol')}\n"
            f"• **Status**: {status}\n"
            f"• **Active Orders**: {r.get('active_orders_count')}\n"
            f"• **Inventory**: {float(r.get('inventory_base', 0)):.6f}\n"
            f"• **Realized PnL**: £{float(r.get('realized_pnl', 0)):.4f}"
        )
        embed.add_field(name=f"Runner: {r.get('runner_id')}", value=val, inline=False)

    await interaction.followup.send(embed=embed)

async def send_discord_alert(title: str, description: str, color: discord.Color = discord.Color.orange()):
    if not DISCORD_CHANNEL_ID or not bot.is_ready():
        return
    try:
        channel = bot.get_channel(int(DISCORD_CHANNEL_ID))
        if channel:
            embed = discord.Embed(title=title, description=description, color=color)
            await channel.send(embed=embed)
    except Exception as e:
        logger.error(f"Failed to send Discord alert: {e}")

async def start_discord_bot():
    if not DISCORD_BOT_TOKEN:
        logger.info("DISCORD_BOT_TOKEN not configured. Discord bot gateway disabled.")
        return
    try:
        await bot.start(DISCORD_BOT_TOKEN)
    except Exception as e:
        logger.error(f"Discord bot error: {e}")
