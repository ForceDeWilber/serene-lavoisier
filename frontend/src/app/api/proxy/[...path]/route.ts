import { NextRequest, NextResponse } from "next/server";
import { verifySessionToken, SESSION_COOKIE_NAME } from "@/lib/auth";

async function forwardRequest(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  try {
    // 1. Session verification if DASHBOARD_PIN is configured
    const pinConfigured = Boolean(process.env.DASHBOARD_PIN || process.env.ADMIN_PIN);
    if (pinConfigured) {
      const token = req.cookies.get(SESSION_COOKIE_NAME)?.value;
      const isValid = await verifySessionToken(token);
      if (!isValid) {
        return NextResponse.json(
          { error: "Unauthorized: Invalid or expired dashboard session" },
          { status: 401 }
        );
      }
    }

    // 2. Resolve destination URL & Host Isolation
    const { path } = await context.params;
    const subPath = path.join("/");
    const search = req.nextUrl.search;
    const mode = req.nextUrl.searchParams.get("mode")?.toLowerCase();

    const isLive = mode === "live" || subPath.startsWith("live");
    const liveUrl = process.env.LIVE_ENGINE_URL;
    const paperUrl = process.env.PAPER_ENGINE_URL;
    const defaultUrl = process.env.ENGINE_URL || "http://localhost:8000";

    // Host isolation: live routes to dedicated live VM; paper/backtest routes to sandbox host
    const engineUrl = (isLive ? (liveUrl || defaultUrl) : (paperUrl || defaultUrl)).replace(/\/$/, "");
    const targetUrl = `${engineUrl}/api/${subPath}${search}`;

    // 3. Prepare headers
    const headers: Record<string, string> = {
      "Accept": "application/json",
    };

    const contentType = req.headers.get("content-type");
    if (contentType) {
      headers["Content-Type"] = contentType;
    }

    // Securely inject engine secret on server-side
    const engineSecret = isLive
      ? (process.env.LIVE_ENGINE_SECRET_KEY || process.env.ENGINE_SECRET_KEY)
      : (process.env.PAPER_ENGINE_SECRET_KEY || process.env.ENGINE_SECRET_KEY);
    if (engineSecret) {
      headers["X-Engine-Secret"] = engineSecret;
    }

    // 4. Prepare request options
    const init: RequestInit = {
      method: req.method,
      headers,
      cache: "no-store",
    };

    if (req.method !== "GET" && req.method !== "HEAD") {
      const bodyText = await req.text();
      if (bodyText) {
        init.body = bodyText;
      }
    }

    // 5. Fetch from trading engine (with local fallback if remote host unreachable)
    let backendRes: Response;
    try {
      backendRes = await fetch(targetUrl, init);
    } catch (fetchErr: any) {
      if (isLive && liveUrl && defaultUrl && liveUrl !== defaultUrl) {
        const fallbackTarget = `${defaultUrl.replace(/\/$/, "")}/api/${subPath}${search}`;
        backendRes = await fetch(fallbackTarget, init);
      } else {
        throw fetchErr;
      }
    }

    const data = await backendRes.text();
    let parsedData;
    try {
      parsedData = JSON.parse(data);
    } catch {
      parsedData = data;
    }

    return NextResponse.json(parsedData, {
      status: backendRes.status,
      headers: {
        "Cache-Control": "no-store, max-age=0",
      },
    });
  } catch (error: any) {
    return NextResponse.json(
      {
        error: "Engine proxy request failed",
        message: error?.message || "Failed to contact trading engine",
      },
      { status: 502 }
    );
  }
}

export async function GET(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return forwardRequest(req, context);
}

export async function POST(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return forwardRequest(req, context);
}

export async function PUT(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return forwardRequest(req, context);
}

export async function DELETE(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return forwardRequest(req, context);
}
